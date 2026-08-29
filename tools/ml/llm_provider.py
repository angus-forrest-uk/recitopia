from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Callable

PROVIDERS = ("anthropic", "google", "openai", "openrouter", "deepseek")

DEFAULT_BASE_URLS = {
    "anthropic": "https://api.anthropic.com",
    "google": "https://generativelanguage.googleapis.com",
    "openai": "https://api.openai.com/v1",
    "openrouter": "https://openrouter.ai/api/v1",
    "deepseek": "https://api.deepseek.com",
}

DEFAULT_MODELS = {
    "anthropic": "claude-sonnet-5",
    "google": "gemini-2.5-flash",
    "openai": "gpt-5",
    "openrouter": "anthropic/claude-sonnet-5",
    "deepseek": "deepseek-v4-flash",
}

API_KEY_ENV = {
    "anthropic": ("RECITOPIA_LLM_API_KEY", "ANTHROPIC_API_KEY"),
    "google": ("RECITOPIA_LLM_API_KEY", "GOOGLE_API_KEY", "GEMINI_API_KEY"),
    "openai": ("RECITOPIA_LLM_API_KEY", "OPENAI_API_KEY"),
    "openrouter": ("RECITOPIA_LLM_API_KEY", "OPENROUTER_API_KEY"),
    "deepseek": ("RECITOPIA_LLM_API_KEY", "DEEPSEEK_API_KEY"),
}

BASE_URL_ENV = {
    "anthropic": ("RECITOPIA_LLM_BASE_URL", "ANTHROPIC_BASE_URL"),
    "google": ("RECITOPIA_LLM_BASE_URL", "GOOGLE_BASE_URL"),
    "openai": ("RECITOPIA_LLM_BASE_URL", "OPENAI_BASE_URL"),
    "openrouter": ("RECITOPIA_LLM_BASE_URL", "OPENROUTER_BASE_URL"),
    "deepseek": ("RECITOPIA_LLM_BASE_URL", "DEEPSEEK_BASE_URL"),
}

MODEL_ENV = {
    "anthropic": ("RECITOPIA_LLM_MODEL", "ANTHROPIC_MODEL"),
    "google": ("RECITOPIA_LLM_MODEL", "GOOGLE_MODEL", "GEMINI_MODEL"),
    "openai": ("RECITOPIA_LLM_MODEL", "OPENAI_MODEL"),
    "openrouter": ("RECITOPIA_LLM_MODEL", "OPENROUTER_MODEL"),
    "deepseek": ("RECITOPIA_LLM_MODEL", "DEEPSEEK_MODEL"),
}

ANTHROPIC_VERSION = "2023-06-01"

TRUNCATION_REASONS = frozenset({"length", "max_tokens", "MAX_TOKENS"})


class ProviderError(RuntimeError):
    pass


class ProviderNotConfigured(ProviderError):
    pass


class LengthLimitError(ProviderError):
    pass


@dataclass(frozen=True)
class Config:
    provider: str
    api_key: str
    base_url: str
    model: str
    max_tokens: int
    timeout: float
    attempts: int
    extra_headers: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class Completion:
    text: str
    finish_reason: str | None
    prompt_tokens: int | None
    completion_tokens: int | None
    total_tokens: int | None
    raw: str


def _first_env(names: tuple[str, ...]) -> str | None:
    for name in names:
        value = os.getenv(name)
        if value and value.strip():
            return value.strip()
    return None


def _int_env(names: tuple[str, ...], fallback: int) -> int:
    value = _first_env(names)
    if value is None:
        return fallback
    try:
        return int(value)
    except ValueError as exc:
        raise ProviderError(f"{names[0]} must be an integer, got {value!r}") from exc


def _float_env(names: tuple[str, ...], fallback: float) -> float:
    value = _first_env(names)
    if value is None:
        return fallback
    try:
        return float(value)
    except ValueError as exc:
        raise ProviderError(f"{names[0]} must be a number, got {value!r}") from exc


def resolve_config(env: dict[str, str] | None = None) -> Config:
    if env is not None:
        previous = dict(os.environ)
        os.environ.clear()
        os.environ.update(env)
        try:
            return resolve_config()
        finally:
            os.environ.clear()
            os.environ.update(previous)

    provider = _first_env(("RECITOPIA_LLM_PROVIDER",))
    if provider is None:
        raise ProviderNotConfigured(
            "RECITOPIA_LLM_PROVIDER is not set; choose one of " + ", ".join(PROVIDERS)
        )
    provider = provider.lower()
    if provider not in PROVIDERS:
        raise ProviderNotConfigured(
            f"unknown provider {provider!r}; choose one of " + ", ".join(PROVIDERS)
        )

    api_key = _first_env(API_KEY_ENV[provider])
    if api_key is None:
        raise ProviderNotConfigured(
            f"no api key for {provider}; set " + " or ".join(API_KEY_ENV[provider])
        )

    base_url = _first_env(BASE_URL_ENV[provider]) or DEFAULT_BASE_URLS[provider]
    model = _first_env(MODEL_ENV[provider]) or DEFAULT_MODELS[provider]

    extra_headers: dict[str, str] = {}
    if provider == "openrouter":
        referer = _first_env(("RECITOPIA_LLM_HTTP_REFERER", "OPENROUTER_HTTP_REFERER"))
        title = _first_env(("RECITOPIA_LLM_APP_TITLE", "OPENROUTER_APP_TITLE"))
        if referer:
            extra_headers["HTTP-Referer"] = referer
        if title:
            extra_headers["X-Title"] = title

    return Config(
        provider=provider,
        api_key=api_key,
        base_url=base_url.rstrip("/"),
        model=model,
        max_tokens=_int_env(("RECITOPIA_LLM_MAX_TOKENS",), 16000),
        timeout=_float_env(("RECITOPIA_LLM_TIMEOUT",), 90.0),
        attempts=max(1, _int_env(("RECITOPIA_LLM_ATTEMPTS", "DEEPSEEK_ATTEMPTS"), 3)),
        extra_headers=extra_headers,
    )


def _chat_completions_request(
    config: Config, system_prompt: str, user_prompt: str, token_field: str
) -> tuple[str, dict[str, str], bytes]:
    url = f"{config.base_url}/chat/completions"
    headers = {
        "authorization": f"Bearer {config.api_key}",
        "content-type": "application/json",
    }
    headers.update(config.extra_headers)
    body = {
        "model": config.model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt},
        ],
        "response_format": {"type": "json_object"},
        token_field: config.max_tokens,
    }
    return url, headers, json.dumps(body).encode("utf-8")


def _truncation_detail(usage: dict[str, Any]) -> str:
    details = usage.get("completion_tokens_details") or {}
    reasoning = details.get("reasoning_tokens")
    if reasoning:
        return (
            f"; {reasoning} of {usage.get('completion_tokens')} completion tokens "
            "went to reasoning, so raise RECITOPIA_LLM_MAX_TOKENS"
        )
    return ""


def _chat_completions_parse(raw: str) -> Completion:
    parsed = json.loads(raw)
    choices = parsed.get("choices")
    if not choices:
        raise ProviderError("response contained no choices")
    usage = parsed.get("usage") or {}
    finish_reason = choices[0].get("finish_reason")
    if finish_reason in TRUNCATION_REASONS:
        raise LengthLimitError(
            "stopped at the token limit before finishing json"
            + _truncation_detail(usage)
        )
    message = choices[0].get("message") or {}
    text = message.get("content")
    if not text:
        raise ProviderError("response contained empty content")
    return Completion(
        text=text,
        finish_reason=choices[0].get("finish_reason"),
        prompt_tokens=usage.get("prompt_tokens"),
        completion_tokens=usage.get("completion_tokens"),
        total_tokens=usage.get("total_tokens"),
        raw=raw,
    )


def _openai_request(config: Config, system_prompt: str, user_prompt: str):
    return _chat_completions_request(
        config, system_prompt, user_prompt, "max_completion_tokens"
    )


def _compatible_request(config: Config, system_prompt: str, user_prompt: str):
    return _chat_completions_request(config, system_prompt, user_prompt, "max_tokens")


def _anthropic_request(
    config: Config, system_prompt: str, user_prompt: str
) -> tuple[str, dict[str, str], bytes]:
    url = f"{config.base_url}/v1/messages"
    headers = {
        "x-api-key": config.api_key,
        "anthropic-version": ANTHROPIC_VERSION,
        "content-type": "application/json",
    }
    headers.update(config.extra_headers)
    body = {
        "model": config.model,
        "max_tokens": config.max_tokens,
        "system": system_prompt,
        "messages": [
            {"role": "user", "content": user_prompt},
            {"role": "assistant", "content": "{"},
        ],
    }
    return url, headers, json.dumps(body).encode("utf-8")


def _anthropic_parse(raw: str) -> Completion:
    parsed = json.loads(raw)
    if parsed.get("stop_reason") in TRUNCATION_REASONS:
        raise LengthLimitError("stopped at the token limit before finishing json")
    blocks = parsed.get("content") or []
    text = "".join(
        block.get("text", "") for block in blocks if block.get("type") == "text"
    )
    if not text.strip():
        raise ProviderError("response contained no text blocks")
    if not text.lstrip().startswith("{"):
        text = "{" + text
    usage = parsed.get("usage") or {}
    prompt_tokens = usage.get("input_tokens")
    completion_tokens = usage.get("output_tokens")
    total = None
    if prompt_tokens is not None and completion_tokens is not None:
        total = prompt_tokens + completion_tokens
    return Completion(
        text=text,
        finish_reason=parsed.get("stop_reason"),
        prompt_tokens=prompt_tokens,
        completion_tokens=completion_tokens,
        total_tokens=total,
        raw=raw,
    )


def _google_request(
    config: Config, system_prompt: str, user_prompt: str
) -> tuple[str, dict[str, str], bytes]:
    url = f"{config.base_url}/v1beta/models/{config.model}:generateContent"
    headers = {
        "x-goog-api-key": config.api_key,
        "content-type": "application/json",
    }
    headers.update(config.extra_headers)
    body = {
        "systemInstruction": {"parts": [{"text": system_prompt}]},
        "contents": [{"role": "user", "parts": [{"text": user_prompt}]}],
        "generationConfig": {
            "responseMimeType": "application/json",
            "maxOutputTokens": config.max_tokens,
        },
    }
    return url, headers, json.dumps(body).encode("utf-8")


def _google_parse(raw: str) -> Completion:
    parsed = json.loads(raw)
    candidates = parsed.get("candidates") or []
    if not candidates:
        raise ProviderError("response contained no candidates")
    if candidates[0].get("finishReason") in TRUNCATION_REASONS:
        raise LengthLimitError("stopped at the token limit before finishing json")
    parts = (candidates[0].get("content") or {}).get("parts") or []
    text = "".join(part.get("text", "") for part in parts)
    if not text.strip():
        raise ProviderError("response contained empty content")
    usage = parsed.get("usageMetadata") or {}
    return Completion(
        text=text,
        finish_reason=candidates[0].get("finishReason"),
        prompt_tokens=usage.get("promptTokenCount"),
        completion_tokens=usage.get("candidatesTokenCount"),
        total_tokens=usage.get("totalTokenCount"),
        raw=raw,
    )


RequestBuilder = Callable[[Config, str, str], "tuple[str, dict[str, str], bytes]"]
ResponseParser = Callable[[str], Completion]

BUILDERS: dict[str, RequestBuilder] = {
    "anthropic": _anthropic_request,
    "google": _google_request,
    "openai": _openai_request,
    "openrouter": _compatible_request,
    "deepseek": _compatible_request,
}

PARSERS: dict[str, ResponseParser] = {
    "anthropic": _anthropic_parse,
    "google": _google_parse,
    "openai": _chat_completions_parse,
    "openrouter": _chat_completions_parse,
    "deepseek": _chat_completions_parse,
}


def build_request(config: Config, system_prompt: str, user_prompt: str):
    return BUILDERS[config.provider](config, system_prompt, user_prompt)


def parse_response(config: Config, raw: str) -> Completion:
    completion = PARSERS[config.provider](raw)
    if completion.finish_reason in TRUNCATION_REASONS:
        raise LengthLimitError(
            f"{config.provider} stopped at the token limit before finishing json"
        )
    return completion


def complete_json(
    config: Config,
    system_prompt: str,
    user_prompt: str,
    on_attempt: Callable[[int, str, Any], None] | None = None,
) -> tuple[dict[str, Any], Completion]:
    url, headers, body = build_request(config, system_prompt, user_prompt)
    last_error: Exception | None = None

    for attempt in range(1, config.attempts + 1):
        request = urllib.request.Request(url, data=body, method="POST", headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=config.timeout) as response:
                raw = response.read().decode("utf-8")
                status = response.status
            completion = parse_response(config, raw)
            payload = json.loads(completion.text)
            if not isinstance(payload, dict):
                raise ProviderError("model returned json that is not an object")
            if on_attempt is not None:
                on_attempt(attempt, "complete", completion)
            return payload, completion
        except LengthLimitError:
            raise
        except urllib.error.HTTPError as exc:
            detail = exc.read(4096).decode("utf-8", errors="replace")
            last_error = ProviderError(
                f"{config.provider} HTTP {exc.code}: {detail[:512]}"
            )
        except (
            urllib.error.URLError,
            ProviderError,
            KeyError,
            IndexError,
            json.JSONDecodeError,
        ) as exc:
            last_error = exc

        if on_attempt is not None:
            on_attempt(attempt, "failed", last_error)
        if attempt < config.attempts:
            time.sleep(min(2 ** (attempt - 1), 8))

    raise ProviderError(str(last_error or f"{config.provider} request failed"))
