from __future__ import annotations

import json
import unittest

import llm_provider as lp


def base_env(provider: str, **extra: str) -> dict[str, str]:
    env = {"RECITOPIA_LLM_PROVIDER": provider, "RECITOPIA_LLM_API_KEY": "k"}
    env.update(extra)
    return env


class ResolveConfigTest(unittest.TestCase):
    def test_provider_required(self) -> None:
        with self.assertRaises(lp.ProviderNotConfigured) as ctx:
            lp.resolve_config({})
        self.assertIn("RECITOPIA_LLM_PROVIDER", str(ctx.exception))

    def test_unknown_provider_rejected(self) -> None:
        with self.assertRaises(lp.ProviderNotConfigured):
            lp.resolve_config({"RECITOPIA_LLM_PROVIDER": "llama"})

    def test_api_key_required(self) -> None:
        with self.assertRaises(lp.ProviderNotConfigured) as ctx:
            lp.resolve_config({"RECITOPIA_LLM_PROVIDER": "openai"})
        self.assertIn("OPENAI_API_KEY", str(ctx.exception))

    def test_provider_specific_key_accepted(self) -> None:
        for provider, name in (
            ("anthropic", "ANTHROPIC_API_KEY"),
            ("google", "GEMINI_API_KEY"),
            ("openai", "OPENAI_API_KEY"),
            ("openrouter", "OPENROUTER_API_KEY"),
            ("deepseek", "DEEPSEEK_API_KEY"),
        ):
            config = lp.resolve_config(
                {"RECITOPIA_LLM_PROVIDER": provider, name: "secret"}
            )
            self.assertEqual(config.api_key, "secret", provider)

    def test_shared_key_wins_over_provider_key(self) -> None:
        config = lp.resolve_config(
            {
                "RECITOPIA_LLM_PROVIDER": "deepseek",
                "RECITOPIA_LLM_API_KEY": "shared",
                "DEEPSEEK_API_KEY": "specific",
            }
        )
        self.assertEqual(config.api_key, "shared")

    def test_defaults_per_provider(self) -> None:
        for provider in lp.PROVIDERS:
            config = lp.resolve_config(base_env(provider))
            self.assertEqual(config.model, lp.DEFAULT_MODELS[provider])
            self.assertEqual(config.base_url, lp.DEFAULT_BASE_URLS[provider])

    def test_base_url_trailing_slash_trimmed(self) -> None:
        config = lp.resolve_config(
            base_env("openai", RECITOPIA_LLM_BASE_URL="https://proxy.test/v1/")
        )
        self.assertEqual(config.base_url, "https://proxy.test/v1")

    def test_deepseek_attempts_env_still_honoured(self) -> None:
        config = lp.resolve_config(base_env("deepseek", DEEPSEEK_ATTEMPTS="7"))
        self.assertEqual(config.attempts, 7)

    def test_bad_integer_rejected(self) -> None:
        with self.assertRaises(lp.ProviderError):
            lp.resolve_config(base_env("openai", RECITOPIA_LLM_MAX_TOKENS="lots"))

    def test_openrouter_optional_headers(self) -> None:
        config = lp.resolve_config(
            base_env(
                "openrouter",
                RECITOPIA_LLM_HTTP_REFERER="https://example.test",
                RECITOPIA_LLM_APP_TITLE="Recitopia",
            )
        )
        self.assertEqual(config.extra_headers["HTTP-Referer"], "https://example.test")
        self.assertEqual(config.extra_headers["X-Title"], "Recitopia")


class BuildRequestTest(unittest.TestCase):
    def test_openai_uses_max_completion_tokens(self) -> None:
        config = lp.resolve_config(base_env("openai"))
        url, headers, body = lp.build_request(config, "sys", "user")
        payload = json.loads(body)
        self.assertEqual(url, "https://api.openai.com/v1/chat/completions")
        self.assertEqual(headers["authorization"], "Bearer k")
        self.assertIn("max_completion_tokens", payload)
        self.assertNotIn("max_tokens", payload)
        self.assertEqual(payload["response_format"], {"type": "json_object"})

    def test_deepseek_uses_max_tokens(self) -> None:
        config = lp.resolve_config(base_env("deepseek"))
        url, _, body = lp.build_request(config, "sys", "user")
        payload = json.loads(body)
        self.assertEqual(url, "https://api.deepseek.com/chat/completions")
        self.assertIn("max_tokens", payload)

    def test_openrouter_path_and_headers(self) -> None:
        config = lp.resolve_config(base_env("openrouter"))
        url, headers, _ = lp.build_request(config, "sys", "user")
        self.assertEqual(url, "https://openrouter.ai/api/v1/chat/completions")
        self.assertEqual(headers["authorization"], "Bearer k")

    def test_anthropic_headers_and_prefill(self) -> None:
        config = lp.resolve_config(base_env("anthropic"))
        url, headers, body = lp.build_request(config, "sys", "user")
        payload = json.loads(body)
        self.assertEqual(url, "https://api.anthropic.com/v1/messages")
        self.assertEqual(headers["x-api-key"], "k")
        self.assertEqual(headers["anthropic-version"], lp.ANTHROPIC_VERSION)
        self.assertNotIn("authorization", headers)
        self.assertEqual(payload["system"], "sys")
        self.assertEqual(payload["messages"][-1], {"role": "assistant", "content": "{"})

    def test_google_url_carries_model_and_key_header(self) -> None:
        config = lp.resolve_config(base_env("google", RECITOPIA_LLM_MODEL="gemini-x"))
        url, headers, body = lp.build_request(config, "sys", "user")
        payload = json.loads(body)
        self.assertTrue(url.endswith("/v1beta/models/gemini-x:generateContent"))
        self.assertEqual(headers["x-goog-api-key"], "k")
        self.assertEqual(
            payload["generationConfig"]["responseMimeType"], "application/json"
        )
        self.assertEqual(payload["systemInstruction"]["parts"][0]["text"], "sys")


class ParseResponseTest(unittest.TestCase):
    def test_chat_completions_shape(self) -> None:
        config = lp.resolve_config(base_env("openai"))
        raw = json.dumps(
            {
                "choices": [
                    {
                        "message": {"content": '{"title":"x"}'},
                        "finish_reason": "stop",
                    }
                ],
                "usage": {
                    "prompt_tokens": 3,
                    "completion_tokens": 4,
                    "total_tokens": 7,
                },
            }
        )
        out = lp.parse_response(config, raw)
        self.assertEqual(json.loads(out.text), {"title": "x"})
        self.assertEqual(out.finish_reason, "stop")
        self.assertEqual(out.total_tokens, 7)

    def test_chat_completions_empty_content_rejected(self) -> None:
        config = lp.resolve_config(base_env("deepseek"))
        raw = json.dumps({"choices": [{"message": {"content": ""}}]})
        with self.assertRaises(lp.ProviderError):
            lp.parse_response(config, raw)

    def test_anthropic_prefill_is_restored(self) -> None:
        config = lp.resolve_config(base_env("anthropic"))
        raw = json.dumps(
            {
                "content": [{"type": "text", "text": '"title":"x"}'}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 5, "output_tokens": 6},
            }
        )
        out = lp.parse_response(config, raw)
        self.assertEqual(json.loads(out.text), {"title": "x"})
        self.assertEqual(out.total_tokens, 11)

    def test_anthropic_complete_object_not_double_braced(self) -> None:
        config = lp.resolve_config(base_env("anthropic"))
        raw = json.dumps({"content": [{"type": "text", "text": '{"title":"x"}'}]})
        out = lp.parse_response(config, raw)
        self.assertEqual(json.loads(out.text), {"title": "x"})

    def test_anthropic_skips_non_text_blocks(self) -> None:
        config = lp.resolve_config(base_env("anthropic"))
        raw = json.dumps(
            {
                "content": [
                    {"type": "thinking", "thinking": "ignored"},
                    {"type": "text", "text": '{"a":1}'},
                ]
            }
        )
        out = lp.parse_response(config, raw)
        self.assertEqual(json.loads(out.text), {"a": 1})

    def test_google_shape(self) -> None:
        config = lp.resolve_config(base_env("google"))
        raw = json.dumps(
            {
                "candidates": [
                    {
                        "content": {"parts": [{"text": '{"title":'}, {"text": '"x"}'}]},
                        "finishReason": "STOP",
                    }
                ],
                "usageMetadata": {
                    "promptTokenCount": 1,
                    "candidatesTokenCount": 2,
                    "totalTokenCount": 3,
                },
            }
        )
        out = lp.parse_response(config, raw)
        self.assertEqual(json.loads(out.text), {"title": "x"})
        self.assertEqual(out.finish_reason, "STOP")
        self.assertEqual(out.total_tokens, 3)

    def test_google_no_candidates_rejected(self) -> None:
        config = lp.resolve_config(base_env("google"))
        with self.assertRaises(lp.ProviderError):
            lp.parse_response(config, json.dumps({"candidates": []}))


class TruncationTest(unittest.TestCase):
    def test_openai_length_finish_raises(self) -> None:
        config = lp.resolve_config(base_env("openai"))
        raw = json.dumps(
            {"choices": [{"message": {"content": "{}"}, "finish_reason": "length"}]}
        )
        with self.assertRaises(lp.LengthLimitError):
            lp.parse_response(config, raw)

    def test_anthropic_max_tokens_raises(self) -> None:
        config = lp.resolve_config(base_env("anthropic"))
        raw = json.dumps(
            {"content": [{"type": "text", "text": '"a":1'}], "stop_reason": "max_tokens"}
        )
        with self.assertRaises(lp.LengthLimitError):
            lp.parse_response(config, raw)

    def test_google_max_tokens_raises(self) -> None:
        config = lp.resolve_config(base_env("google"))
        raw = json.dumps(
            {
                "candidates": [
                    {"content": {"parts": [{"text": "{}"}]}, "finishReason": "MAX_TOKENS"}
                ]
            }
        )
        with self.assertRaises(lp.LengthLimitError):
            lp.parse_response(config, raw)

    def test_length_error_is_not_retried(self) -> None:
        config = lp.resolve_config(base_env("deepseek", RECITOPIA_LLM_ATTEMPTS="3"))
        calls: list[int] = []

        class FakeResponse:
            status = 200

            def read(self) -> bytes:
                calls.append(1)
                return json.dumps(
                    {
                        "choices": [
                            {"message": {"content": "{}"}, "finish_reason": "length"}
                        ]
                    }
                ).encode("utf-8")

            def __enter__(self):
                return self

            def __exit__(self, *_: object) -> None:
                return None

        original = lp.urllib.request.urlopen
        lp.urllib.request.urlopen = lambda *a, **k: FakeResponse()  # type: ignore[assignment]
        try:
            with self.assertRaises(lp.LengthLimitError):
                lp.complete_json(config, "sys", "user")
        finally:
            lp.urllib.request.urlopen = original  # type: ignore[assignment]
        self.assertEqual(len(calls), 1)


class CompleteJsonTest(unittest.TestCase):
    def test_retries_then_succeeds(self) -> None:
        config = lp.resolve_config(base_env("deepseek", RECITOPIA_LLM_ATTEMPTS="2"))
        calls: list[int] = []
        good = json.dumps({"choices": [{"message": {"content": '{"ok":true}'}}]})

        class FakeResponse:
            status = 200

            def __init__(self, body: str) -> None:
                self._body = body.encode("utf-8")

            def read(self) -> bytes:
                return self._body

            def __enter__(self):
                return self

            def __exit__(self, *_: object) -> None:
                return None

        def fake_urlopen(request, timeout=None):  # noqa: ANN001, ARG001
            calls.append(1)
            if len(calls) == 1:
                raise lp.urllib.error.URLError("boom")
            return FakeResponse(good)

        original = lp.urllib.request.urlopen
        sleeps: list[float] = []
        original_sleep = lp.time.sleep
        lp.urllib.request.urlopen = fake_urlopen  # type: ignore[assignment]
        lp.time.sleep = sleeps.append  # type: ignore[assignment]
        try:
            payload, completion = lp.complete_json(config, "sys", "user")
        finally:
            lp.urllib.request.urlopen = original  # type: ignore[assignment]
            lp.time.sleep = original_sleep  # type: ignore[assignment]

        self.assertEqual(payload, {"ok": True})
        self.assertEqual(len(calls), 2)
        self.assertEqual(sleeps, [1])
        self.assertEqual(json.loads(completion.text), {"ok": True})

    def test_non_object_json_rejected(self) -> None:
        config = lp.resolve_config(base_env("deepseek", RECITOPIA_LLM_ATTEMPTS="1"))

        class FakeResponse:
            status = 200

            def read(self) -> bytes:
                return json.dumps(
                    {"choices": [{"message": {"content": "[1,2]"}}]}
                ).encode("utf-8")

            def __enter__(self):
                return self

            def __exit__(self, *_: object) -> None:
                return None

        original = lp.urllib.request.urlopen
        lp.urllib.request.urlopen = lambda *a, **k: FakeResponse()  # type: ignore[assignment]
        try:
            with self.assertRaises(lp.ProviderError):
                lp.complete_json(config, "sys", "user")
        finally:
            lp.urllib.request.urlopen = original  # type: ignore[assignment]


if __name__ == "__main__":
    unittest.main()
