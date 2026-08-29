#!/usr/bin/env python3
"""Map OCR text into a Recitopia recipe draft with DeepSeek JSON mode."""

from __future__ import annotations

import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

import llm_provider
from typing import Any


SYSTEM_PROMPT = """You convert OCR text from a user's owned cookbook into Recitopia recipe json.
Return only one valid json object matching this exact recipe shape:
{
  "id": "kebab-case-id",
  "title": "Recipe title",
  "subtitle": null,
  "alternateNames": [
    { "kind": "romanization", "value": "romanized or alternate name" }
  ],
  "cookbookId": "existing-cookbook-id",
  "authorIds": [],
  "pageStart": null,
  "pageEnd": null,
  "sourceLabel": "Cookbook, p. 12",
  "headnote": "brief source headnote from the OCR text, or null",
  "servingContext": null,
  "yieldQuantity": null,
  "yieldUnit": "servings",
  "prepMinutes": null,
  "cookMinutes": null,
  "totalMinutes": null,
  "cuisine": null,
  "category": null,
  "tags": ["imported", "needs-review"],
  "searchableText": "",
  "sourceBlockId": null,
  "sourcePageSpans": [],
  "componentRecipeIds": [],
  "picturedPageNumber": null,
  "extractionStatus": "needs_review",
  "images": [],
  "ingredients": [
    {
      "id": "ingredient-1",
      "position": 1,
      "displayName": "1 cup rice",
      "item": "rice",
      "quantity": 1,
      "unit": "cup",
      "preparation": null,
      "section": null,
      "optional": false,
      "alternativeText": null,
      "sourceLine": null,
      "sourcePageId": null,
      "unitCostCents": null,
      "estimatedCostCents": null
    }
  ],
  "steps": [
    {
      "id": "step-1",
      "position": 1,
      "section": null,
      "text": "Cook the rice.",
      "sourcePageId": null,
      "sourceLineStart": null,
      "sourceLineEnd": null
    }
  ],
  "notes": [],
  "lastMadeAt": null,
  "timesMade": 0,
  "costCents": null,
  "costPerServingCents": null,
  "cacheKey": "uncached",
  "cacheUpdatedAt": null
}
Use null when OCR is uncertain. Do not invent ingredients or timings that are not supported by the text.
Keep source/cookbook ids exactly as provided. Make ids lowercase kebab-case.
"""

_verbose_fifo: Any | None = None
_verbose_sequence = 0


def log_event(level: str, event: str, **fields: Any) -> None:
    global _verbose_fifo
    payload = {"level": level, "event": event, **fields}
    line = json.dumps(payload, ensure_ascii=False, sort_keys=True)
    if level == "VERBOSE" and _verbose_fifo is not None:
        try:
            print(line, file=_verbose_fifo, flush=True)
            return
        except OSError:
            _verbose_fifo = None
    print(line, file=sys.stderr, flush=True)


def verbose_enabled() -> bool:
    value = os.getenv("RECITOPIA_VERBOSE_DEEPSEEK_LOGS", "true").strip().lower()
    return value not in {"0", "false", "no", "off"}


def safe_log_name(value: str) -> str:
    safe = re.sub(r"[^a-zA-Z0-9._-]+", "-", value).strip("-._")
    return safe or "deepseek-log"


def init_verbose_stream() -> None:
    global _verbose_fifo
    if not verbose_enabled() or _verbose_fifo is not None:
        return
    fifo_path = os.getenv("RECITOPIA_VERBOSE_LOG_FIFO")
    if not fifo_path:
        return
    try:
        _verbose_fifo = open(fifo_path, "w", encoding="utf-8", buffering=1)
    except OSError as exc:
        log_event(
            "WARN",
            "deepseek_verbose_fifo_unavailable",
            fifo=fifo_path,
            error=f"{type(exc).__name__}: {exc}",
        )


def close_verbose_stream() -> None:
    global _verbose_fifo
    if _verbose_fifo is None:
        return
    try:
        _verbose_fifo.close()
    except OSError:
        pass
    finally:
        _verbose_fifo = None


def write_verbose_file(event: str, text: str, **fields: Any) -> str | None:
    global _verbose_sequence
    directory = os.getenv("RECITOPIA_VERBOSE_LOG_DIR")
    if not directory or not verbose_enabled():
        return None
    try:
        Path(directory).mkdir(parents=True, exist_ok=True)
        _verbose_sequence += 1
        path = Path(directory) / f"{_verbose_sequence:04d}-{safe_log_name(event)}.json"
        record = {
            "event": event,
            "fields": fields,
            "text": text,
            "text_bytes": len(text.encode("utf-8")),
        }
        path.write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
        return str(path)
    except OSError as exc:
        log_event(
            "WARN",
            "deepseek_verbose_file_write_failed",
            event=event,
            error=f"{type(exc).__name__}: {exc}",
        )
        return None


def log_verbose_text(event: str, text: str, **fields: Any) -> None:
    if not verbose_enabled():
        return
    init_verbose_stream()
    file_path = write_verbose_file(event, text, **fields)
    chunk_size = max(512, int(os.getenv("RECITOPIA_VERBOSE_LOG_CHUNK_SIZE", "3000")))
    chunks = [text[index : index + chunk_size] for index in range(0, len(text), chunk_size)] or [""]
    for index, chunk in enumerate(chunks, start=1):
        log_event(
            "VERBOSE",
            event,
            chunk_index=index,
            chunk_total=len(chunks),
            file_path=file_path,
            text_bytes=len(text.encode("utf-8")),
            text=chunk,
            **fields,
        )


def request_model(payload: dict[str, Any]) -> dict[str, Any]:
    config = llm_provider.resolve_config()
    source_label = payload["sourceLabel"]

    user_prompt = {
        "cookbookId": payload["cookbookId"],
        "authorIds": payload.get("authorIds", []),
        "pageStart": payload.get("pageStart"),
        "pageEnd": payload.get("pageEnd"),
        "sourceLabel": source_label,
        "sourceBlockId": payload.get("sourceBlockId"),
        "sourcePageSpans": payload.get("sourcePageSpans", []),
        "ocrText": payload["ocrText"],
    }
    user_content = "Create Recitopia recipe json from this OCR payload:\n" + json.dumps(
        user_prompt, ensure_ascii=False
    )

    log_verbose_text(
        "llm_recipe_request_body",
        user_content,
        source_label=source_label,
        provider=config.provider,
        model=config.model,
    )
    log_event(
        "INFO",
        "llm_recipe_request_start",
        source_label=source_label,
        provider=config.provider,
        model=config.model,
        ocr_bytes=len(payload["ocrText"]),
        attempts=config.attempts,
    )

    started = time.monotonic()

    def on_attempt(attempt: int, outcome: str, detail: Any) -> None:
        if outcome == "complete":
            log_verbose_text(
                "llm_recipe_message_content",
                detail.text,
                source_label=source_label,
                attempt=attempt,
            )
            log_event(
                "INFO",
                "llm_recipe_request_complete",
                source_label=source_label,
                provider=config.provider,
                model=config.model,
                attempt=attempt,
                duration_ms=int((time.monotonic() - started) * 1000),
                finish_reason=detail.finish_reason,
                prompt_tokens=detail.prompt_tokens,
                completion_tokens=detail.completion_tokens,
                total_tokens=detail.total_tokens,
            )
            return
        log_event(
            "ERROR" if attempt == config.attempts else "WARN",
            "llm_recipe_request_failed",
            source_label=source_label,
            provider=config.provider,
            attempt=attempt,
            attempts=config.attempts,
            error=f"{type(detail).__name__}: {detail}",
        )

    result, _ = llm_provider.complete_json(
        config, SYSTEM_PROMPT, user_content, on_attempt
    )
    return result


request_deepseek = request_model


def require_list(value: Any, path: str) -> list[Any]:
    if not isinstance(value, list):
        raise RuntimeError(f"{path} must be a list")
    return value


def require_string(value: Any, path: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError(f"{path} must be a non-empty string")
    return value


def validate_recipe_output(recipe: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(recipe, dict):
        raise RuntimeError("recipe output must be an object")
    require_string(recipe.get("id"), "id")
    require_string(recipe.get("title"), "title")
    require_string(recipe.get("cookbookId"), "cookbookId")
    require_string(recipe.get("sourceLabel"), "sourceLabel")
    require_list(recipe.get("ingredients"), "ingredients")
    require_list(recipe.get("steps"), "steps")
    require_list(recipe.get("tags"), "tags")
    require_list(recipe.get("images"), "images")
    require_list(recipe.get("notes"), "notes")
    return recipe


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: deepseek_mapper.py REQUEST_JSON", file=sys.stderr)
        return 2

    payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    init_verbose_stream()
    try:
        recipe = validate_recipe_output(request_deepseek(payload))
        recipe_json = json.dumps(recipe, ensure_ascii=False)
        log_verbose_text("deepseek_recipe_normalized_output", recipe_json, source_label=payload["sourceLabel"])
        print(recipe_json)
        return 0
    except (urllib.error.URLError, KeyError, RuntimeError, json.JSONDecodeError) as exc:
        log_event("ERROR", "deepseek_recipe_mapping_failed", error=f"{type(exc).__name__}: {exc}")
        return 3
    finally:
        close_verbose_stream()


if __name__ == "__main__":
    raise SystemExit(main())
