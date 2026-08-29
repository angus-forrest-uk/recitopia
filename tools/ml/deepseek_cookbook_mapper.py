#!/usr/bin/env python3
"""Map a whole OCR'd cookbook import into Recitopia recipes and context blocks."""

from __future__ import annotations

import json
import os
import re
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from pathlib import Path

import llm_provider
from typing import Any


SYSTEM_PROMPT = """You convert OCR text from a user's owned cookbook into Recitopia cookbook data.
Return only one valid JSON object:
{
  "recipes": [RECITOPIA_RECIPE_OBJECTS],
  "contentBlocks": [COOKBOOK_CONTEXT_BLOCK_OBJECTS]
}

Recipe objects must match this shape:
{
  "id": "kebab-case-id",
  "title": "Recipe title",
  "subtitle": null,
  "alternateNames": [{ "kind": "romanization", "value": "romanized name" }],
  "cookbookId": "existing-cookbook-id",
  "authorIds": [],
  "pageStart": null,
  "pageEnd": null,
  "sourceLabel": "Cookbook, pp. 12-13",
  "headnote": "source headnote only, or null",
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
  "sourcePageSpans": [
    { "pageId": "page-id", "printedPageNumber": 12, "lineStart": null, "lineEnd": null, "confidence": null }
  ],
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
      "quantityText": "1",
      "quantityMin": null,
      "quantityMax": null,
      "quantityKind": "exact",
      "quantityReviewStatus": "parsed",
      "quantityReviewReason": null,
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

Context blocks must match this shape:
{
  "id": "import-id-context-1",
  "cookbookId": "existing-cookbook-id",
  "sectionId": "section-id-or-null",
  "pageStart": null,
  "pageEnd": null,
  "position": 1,
  "kind": "paragraph",
  "title": "Section introduction",
  "text": "Non-recipe contextual text only.",
  "confidence": null,
  "sourceJson": "{\"source\":\"deepseek-cookbook\"}"
}

Use the recipe lists, chapter opener pages, page numbers, and multi-page OCR text to infer complete
recipe spans. Page objects include scanPageNumber for locating the uploaded image and may include
printedPageNumber/detectedBookPageNumber for the cookbook's printed page. For pageStart, pageEnd,
sourceLabel, and sourcePageSpans.printedPageNumber, use printedPageNumber or detectedBookPageNumber
when present. Only fall back to scanPageNumber/imageIndex when no printed cookbook page number is
available. OCR scan order and book page numbers may differ.
Do not require a human to draft individual recipes. Put introductory essays, pantry notes, chapter
introductions, menu discussion, supplier/index narrative, captions, and other non-recipe prose into
contentBlocks. Do not duplicate full recipe instructions in contentBlocks. If this payload starts or
ends in the middle of a recipe, return that recipe only when enough ingredient and step text is
visible to make a useful draft. Preserve source wording when possible. Use null for uncertain
normalized values. Do not invent ingredients, steps, timings, or yields unsupported by OCR.
For ingredient quantities, preserve ambiguous source text rather than forcing it into a single
number. Use quantityKind="range" with quantityMin/quantityMax for ranges such as "1-2 tsp"; set
quantity to null for ranges. If OCR appears to have lost leading digits, such as "tbsp mirin" or
"00g halibut", keep the original displayName, set quantity to null, and mark
quantityReviewStatus="needs_review" with a short quantityReviewReason.
"""

_verbose_fifo: Any | None = None
_verbose_sequence = 0
_verbose_lock = threading.Lock()


DeepSeekLengthError = llm_provider.LengthLimitError


def log_event(level: str, event: str, **fields: Any) -> None:
    global _verbose_fifo
    payload = {"level": level, "event": event, **fields}
    line = json.dumps(payload, ensure_ascii=False, sort_keys=True)
    with _verbose_lock:
        if level == "VERBOSE" and _verbose_fifo is not None:
            try:
                print(line, file=_verbose_fifo, flush=True)
                return
            except OSError:
                _verbose_fifo = None
        print(line, file=sys.stderr, flush=True)


_progress_lock = threading.Lock()


class PipelineCancelled(Exception):
    """Raised when the Recitopia API has requested this mapper stop."""


def cancelled() -> bool:
    path = os.getenv("RECITOPIA_CANCEL_FILE")
    return bool(path and Path(path).exists())


def raise_if_cancelled() -> None:
    if cancelled():
        raise PipelineCancelled("cookbook extraction canceled")


def report_progress(completed: int, total: int, section_title: str) -> None:
    """Append a progress line for the Zig pipeline's watcher so the UI can
    show LLM extraction advancing batch by batch. Best-effort: progress must
    never fail the extraction."""
    path = os.getenv("RECITOPIA_PROGRESS_FILE")
    if not path:
        return
    line = json.dumps(
        {"completed": completed, "total": total, "sectionTitle": section_title},
        ensure_ascii=False,
    )
    try:
        with _progress_lock, open(path, "a", encoding="utf-8") as handle:
            handle.write(line + "\n")
    except OSError:
        pass


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
    with _verbose_lock:
        if _verbose_fifo is not None:
            return
        try:
            _verbose_fifo = open(fifo_path, "w", encoding="utf-8", buffering=1)
        except OSError as exc:
            print(
                json.dumps(
                    {
                        "level": "WARN",
                        "event": "deepseek_verbose_fifo_unavailable",
                        "fifo": fifo_path,
                        "error": f"{type(exc).__name__}: {exc}",
                    },
                    ensure_ascii=False,
                    sort_keys=True,
                ),
                file=sys.stderr,
                flush=True,
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
        with _verbose_lock:
            _verbose_sequence += 1
            sequence = _verbose_sequence
        path = Path(directory) / f"{sequence:04d}-{safe_log_name(event)}.json"
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


def slugify(value: str, fallback: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")
    return slug if len(slug) >= 2 else fallback


def normalize_block_kind(value: Any) -> str:
    raw = str(value or "paragraph").strip().lower().replace("-", "_").replace(" ", "_")
    if raw in {
        "paragraph",
        "recipe",
        "recipe_headnote",
        "ingredient_glossary_entry",
        "menu",
        "supplier",
        "index_entry",
        "caption",
        "callout",
    }:
        return raw
    if raw in {"intro", "introduction", "essay", "context", "connective", "note", "notes"}:
        return "paragraph"
    if raw in {"headnote", "recipe_note", "recipe_intro"}:
        return "recipe_headnote"
    if raw in {"glossary", "ingredient_glossary", "ingredient"}:
        return "ingredient_glossary_entry"
    if raw in {"index", "index_item"}:
        return "index_entry"
    if raw in {"supplier_note", "shop", "source"}:
        return "supplier"
    return "paragraph"


def optional_string(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def string_list(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        text = value.strip()
        return [text] if text else []
    if not isinstance(value, list):
        return []
    out: list[str] = []
    for item in value:
        text = optional_string(item)
        if text:
            out.append(text)
    return out


def optional_int(value: Any) -> int | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value if value >= 0 else None
    if isinstance(value, float):
        return int(value) if value >= 0 and value.is_integer() else None
    text = str(value).strip()
    if not text:
        return None
    match = re.search(r"-?\d+", text)
    if not match:
        return None
    try:
        number = int(match.group(0))
        return number if number >= 0 else None
    except ValueError:
        return None


def optional_float(value: Any) -> float | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    text = str(value).strip()
    if not text:
        return None
    parsed = parse_quantity_number(text)
    if parsed is not None:
        return parsed
    match = re.search(r"-?\d+(?:\.\d+)?", text)
    if not match:
        return None
    try:
        return float(match.group(0))
    except ValueError:
        return None


def optional_positive_float(value: Any) -> float | None:
    number = optional_float(value)
    return number if number is not None and number > 0 else None


def optional_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value is None:
        return False
    text = str(value).strip().lower()
    return text in {"1", "true", "yes", "y", "optional"}


FRACTION_VALUES = {
    "¼": 0.25,
    "½": 0.5,
    "¾": 0.75,
    "⅓": 1 / 3,
    "⅔": 2 / 3,
    "⅛": 0.125,
    "⅜": 0.375,
    "⅝": 0.625,
    "⅞": 0.875,
}

QUANTITY_KIND_VALUES = {"exact", "range", "as_needed", "unknown"}
QUANTITY_REVIEW_STATUS_VALUES = {"parsed", "needs_review"}
MEASURE_UNITS = {
    "tbsp",
    "tablespoon",
    "tablespoons",
    "tsp",
    "teaspoon",
    "teaspoons",
    "g",
    "gram",
    "grams",
    "kg",
    "ml",
    "l",
    "litre",
    "litres",
    "cm",
}
SIZE_WORD_UNITS = {"large", "small", "medium", "medium-size"}
AS_NEEDED_PATTERN = re.compile(
    r"\b(to taste|to serve|for frying|for dusting|as needed|as required)\b",
    re.IGNORECASE,
)
OCR_LEADING_DIGIT_LOSS_PATTERN = re.compile(
    r"^\s*(?:0+\s*(?:g|kg|ml|l|litres?|tbsp|tsp|cm)\b|(?:tbsp|tsp|g|kg|ml|l|litres?|cm)\b)",
    re.IGNORECASE,
)
OCR_QUANTITY_CORRUPTION_PATTERN = re.compile(r"\d[¼½¾⅓⅔⅛⅜⅝⅞]\d")


def optional_enum(value: Any, allowed: set[str], default: str) -> str:
    text = optional_string(value)
    return text if text in allowed else default


def parse_quantity_number(text: str) -> float | None:
    text = text.strip()
    if not text:
        return None
    if text in FRACTION_VALUES:
        return FRACTION_VALUES[text]
    if match := re.fullmatch(r"(-?\d+)\s+(\d+)\s*/\s*(\d+)", text):
        denominator = int(match.group(3))
        return int(match.group(1)) + int(match.group(2)) / denominator if denominator else None
    if match := re.fullmatch(r"(-?\d+)([¼½¾⅓⅔⅛⅜⅝⅞])", text):
        return int(match.group(1)) + FRACTION_VALUES[match.group(2)]
    if match := re.fullmatch(r"(\d)(\d)\s*/\s*([2348])", text):
        return int(match.group(1)) + int(match.group(2)) / int(match.group(3))
    if match := re.fullmatch(r"(-?\d+)\s*/\s*(\d+)", text):
        denominator = int(match.group(2))
        return int(match.group(1)) / denominator if denominator else None
    if match := re.fullmatch(r"-?\d+(?:\.\d+)?", text):
        return float(match.group(0))
    return None


QUANTITY_TOKEN = r"(?:\d+\s+\d+\s*/\s*\d+|\d\d\s*/\s*[2348]|\d+\s*/\s*\d+|\d+(?:\.\d+)?[¼½¾⅓⅔⅛⅜⅝⅞]?|[¼½¾⅓⅔⅛⅜⅝⅞])"
RANGE_PATTERN = re.compile(
    rf"^\s*(?P<left>{QUANTITY_TOKEN})\s*(?:-|–|—|to)\s*(?P<right>{QUANTITY_TOKEN})(?=\s|[a-zA-Z]|$)",
    re.IGNORECASE,
)
EXACT_PATTERN = re.compile(rf"^\s*(?:about|approx\.?|approximately)?\s*(?P<value>{QUANTITY_TOKEN})(?=\s|[a-zA-Z]|$)", re.IGNORECASE)


def leading_quantity_from_display(display: str) -> dict[str, Any] | None:
    if range_match := RANGE_PATTERN.search(display):
        left = parse_quantity_number(range_match.group("left"))
        right = parse_quantity_number(range_match.group("right"))
        if left is not None and right is not None:
            return {
                "quantityText": range_match.group(0).strip(),
                "quantity": None,
                "quantityMin": min(left, right),
                "quantityMax": max(left, right),
                "quantityKind": "range",
            }
    if exact_match := EXACT_PATTERN.search(display):
        value = parse_quantity_number(exact_match.group("value"))
        if value is not None and value > 0:
            return {
                "quantityText": exact_match.group("value").strip(),
                "quantity": value,
                "quantityMin": None,
                "quantityMax": None,
                "quantityKind": "exact",
            }
    return None


def append_preparation(preparation: str | None, text: str) -> str:
    if preparation:
        return f"{text}; {preparation}"
    return text


def normalize_ingredient_quantity(ingredient: dict[str, Any]) -> dict[str, Any]:
    display = ingredient["displayName"]
    unit = ingredient["unit"]
    preparation = ingredient["preparation"]
    explicit_status = optional_enum(
        ingredient.get("quantityReviewStatus"), QUANTITY_REVIEW_STATUS_VALUES, "parsed"
    )
    explicit_reason = optional_string(ingredient.get("quantityReviewReason"))

    if unit and unit.lower() in SIZE_WORD_UNITS:
        ingredient["unit"] = None
        ingredient["preparation"] = append_preparation(preparation, unit)
        unit = ingredient["unit"]
        preparation = ingredient["preparation"]

    parsed = leading_quantity_from_display(display)
    if parsed:
        ingredient["quantityText"] = optional_string(ingredient.get("quantityText")) or parsed["quantityText"]
        ingredient["quantityKind"] = parsed["quantityKind"]
        ingredient["quantityMin"] = parsed["quantityMin"]
        ingredient["quantityMax"] = parsed["quantityMax"]
        if parsed["quantityKind"] == "range":
            ingredient["quantity"] = None
        elif ingredient["quantity"] is None:
            ingredient["quantity"] = parsed["quantity"]
    else:
        ingredient["quantityText"] = optional_string(ingredient.get("quantityText"))
        ingredient["quantityMin"] = optional_positive_float(ingredient.get("quantityMin"))
        ingredient["quantityMax"] = optional_positive_float(ingredient.get("quantityMax"))
        ingredient["quantityKind"] = optional_enum(
            ingredient.get("quantityKind"), QUANTITY_KIND_VALUES, "exact"
        )

    needs_review = explicit_status == "needs_review"
    reason = explicit_reason

    if ingredient["quantity"] is not None and ingredient["quantity"] <= 0:
        ingredient["quantity"] = None
        if AS_NEEDED_PATTERN.search(display):
            ingredient["quantityKind"] = "as_needed"
        else:
            needs_review = True
            reason = reason or "non_positive_quantity"

    if (
        AS_NEEDED_PATTERN.search(display)
        and ingredient["quantity"] is None
        and ingredient["quantityKind"] != "range"
        and not needs_review
    ):
        ingredient["quantityKind"] = "as_needed"

    if OCR_LEADING_DIGIT_LOSS_PATTERN.search(display):
        ingredient["quantity"] = None
        ingredient["quantityKind"] = "unknown"
        needs_review = True
        if reason is None or reason == "non_positive_quantity":
            reason = "possible_ocr_leading_digit_loss"

    if OCR_QUANTITY_CORRUPTION_PATTERN.search(display):
        needs_review = True
        reason = reason or "possible_ocr_quantity_corruption"

    if ingredient["quantity"] is None and unit and unit.lower() in MEASURE_UNITS and not AS_NEEDED_PATTERN.search(display):
        if parsed is None:
            needs_review = True
            reason = reason or "missing_quantity_for_measured_unit"

    if ingredient["quantityKind"] == "range":
        if ingredient["quantityMin"] is None or ingredient["quantityMax"] is None:
            needs_review = True
            reason = reason or "invalid_quantity_range"
        ingredient["quantity"] = None

    ingredient["quantityReviewStatus"] = "needs_review" if needs_review else "parsed"
    ingredient["quantityReviewReason"] = reason if needs_review else None
    return ingredient


def normalize_alternate_names(value: Any) -> list[dict[str, str]]:
    if not isinstance(value, list):
        return []
    out: list[dict[str, str]] = []
    for index, item in enumerate(value, start=1):
        if isinstance(item, dict):
            name = optional_string(item.get("value"))
            kind = optional_string(item.get("kind")) or "alternate"
        else:
            name = optional_string(item)
            kind = "alternate"
        if name:
            out.append({"kind": kind, "value": name})
    return out


def normalize_source_page_spans(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        return []
    spans: list[dict[str, Any]] = []
    for item in value:
        if not isinstance(item, dict):
            continue
        spans.append(
            {
                "pageId": optional_string(item.get("pageId")),
                "printedPageNumber": optional_int(item.get("printedPageNumber")),
                "lineStart": optional_int(item.get("lineStart")),
                "lineEnd": optional_int(item.get("lineEnd")),
                "confidence": optional_float(item.get("confidence")),
            }
        )
    return spans


def normalize_images(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        return []
    images: list[dict[str, Any]] = []
    for index, item in enumerate(value, start=1):
        if isinstance(item, dict):
            url = optional_string(item.get("url"))
            alt = optional_string(item.get("alt")) or "Recipe image"
            image_id = optional_string(item.get("id")) or f"image-{index}"
            credit = optional_string(item.get("credit"))
            is_primary = optional_bool(item.get("isPrimary"))
        else:
            url = optional_string(item)
            alt = "Recipe image"
            image_id = f"image-{index}"
            credit = None
            is_primary = index == 1
        if url:
            images.append(
                {
                    "id": image_id,
                    "url": url,
                    "alt": alt,
                    "credit": credit,
                    "isPrimary": is_primary,
                }
            )
    return images


def normalize_ingredients(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        return []
    ingredients: list[dict[str, Any]] = []
    for index, item in enumerate(value, start=1):
        if isinstance(item, dict):
            display = optional_string(item.get("displayName")) or optional_string(item.get("sourceLine"))
            ingredient_name = optional_string(item.get("item")) or display
            ingredient_id = optional_string(item.get("id")) or f"ingredient-{index}"
            ingredients.append(
                normalize_ingredient_quantity(
                    {
                        "id": ingredient_id,
                        "position": optional_int(item.get("position")) or index,
                        "displayName": display or ingredient_name or "Review OCR ingredient",
                        "item": ingredient_name or display or "Review OCR ingredient",
                        "quantity": optional_float(item.get("quantity")),
                        "quantityText": optional_string(item.get("quantityText")),
                        "quantityMin": optional_positive_float(item.get("quantityMin")),
                        "quantityMax": optional_positive_float(item.get("quantityMax")),
                        "quantityKind": optional_enum(
                            item.get("quantityKind"), QUANTITY_KIND_VALUES, "exact"
                        ),
                        "quantityReviewStatus": optional_enum(
                            item.get("quantityReviewStatus"),
                            QUANTITY_REVIEW_STATUS_VALUES,
                            "parsed",
                        ),
                        "quantityReviewReason": optional_string(item.get("quantityReviewReason")),
                        "unit": optional_string(item.get("unit")),
                        "preparation": optional_string(item.get("preparation")),
                        "section": optional_string(item.get("section")),
                        "optional": optional_bool(item.get("optional")),
                        "alternativeText": optional_string(item.get("alternativeText")),
                        "sourceLine": optional_int(item.get("sourceLine")),
                        "sourcePageId": optional_string(item.get("sourcePageId")),
                        "unitCostCents": optional_int(item.get("unitCostCents")),
                        "estimatedCostCents": optional_int(item.get("estimatedCostCents")),
                    }
                )
            )
        else:
            display = optional_string(item)
            if display:
                ingredients.append(
                    normalize_ingredient_quantity(
                        {
                            "id": f"ingredient-{index}",
                            "position": index,
                            "displayName": display,
                            "item": display,
                            "quantity": None,
                            "quantityText": None,
                            "quantityMin": None,
                            "quantityMax": None,
                            "quantityKind": "exact",
                            "quantityReviewStatus": "parsed",
                            "quantityReviewReason": None,
                            "unit": None,
                            "preparation": None,
                            "section": None,
                            "optional": False,
                            "alternativeText": None,
                            "sourceLine": None,
                            "sourcePageId": None,
                            "unitCostCents": None,
                            "estimatedCostCents": None,
                        }
                    )
                )
    return ingredients


def normalize_steps(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        return []
    steps: list[dict[str, Any]] = []
    for index, item in enumerate(value, start=1):
        if isinstance(item, dict):
            text = optional_string(item.get("text"))
            step_id = optional_string(item.get("id")) or f"step-{index}"
            if text:
                steps.append(
                    {
                        "id": step_id,
                        "position": optional_int(item.get("position")) or index,
                        "section": optional_string(item.get("section")),
                        "text": text,
                        "sourcePageId": optional_string(item.get("sourcePageId")),
                        "sourceLineStart": optional_int(item.get("sourceLineStart")),
                        "sourceLineEnd": optional_int(item.get("sourceLineEnd")),
                    }
                )
        else:
            text = optional_string(item)
            if text:
                steps.append(
                    {
                        "id": f"step-{index}",
                        "position": index,
                        "section": None,
                        "text": text,
                        "sourcePageId": None,
                        "sourceLineStart": None,
                        "sourceLineEnd": None,
                    }
                )
    return steps


def page_number(page: dict[str, Any]) -> int:
    return int(page.get("printedPageNumber") or page.get("imageIndex") or 0)


def scan_page_number(page: dict[str, Any]) -> int:
    return int(page.get("imageIndex") or page.get("printedPageNumber") or 0)


def is_whole_cookbook_section(section: dict[str, Any]) -> bool:
    title = str(section.get("title") or "").strip().lower()
    return section.get("id") is None and title in {"", "whole cookbook"}


def page_in_range(page: dict[str, Any], start: int | None, end: int | None) -> bool:
    number = page_number(page)
    if start is not None and number < start:
        return False
    if end is not None and number > end:
        return False
    return True


def section_pages(payload: dict[str, Any], section: dict[str, Any]) -> list[dict[str, Any]]:
    start = section.get("pageStart")
    end = section.get("pageEnd")
    if start is None and end is None:
        if is_whole_cookbook_section(section):
            return payload["pages"]
        log_event(
            "WARN",
            "deepseek_section_bounds_missing",
            section_id=section.get("id"),
            section_title=section.get("title"),
        )
        return []
    if start is None:
        log_event(
            "WARN",
            "deepseek_section_start_missing",
            section_id=section.get("id"),
            section_title=section.get("title"),
            page_end=end,
        )
        return []
    if end is None:
        end = max((page_number(page) for page in payload["pages"]), default=start)
    if start > end:
        log_event(
            "WARN",
            "deepseek_section_bounds_invalid",
            section_id=section.get("id"),
            section_title=section.get("title"),
            page_start=start,
            page_end=end,
        )
        return []
    return [page for page in payload["pages"] if page_in_range(page, start, end)]


def recipe_candidates(pages: list[dict[str, Any]]) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []
    for page in pages:
        if page.get("pageKind") != "chapter_opener":
            continue
        text = page.get("ocrText", "")
        matches = re.finditer(r"(?P<page>\d{1,3})[.\s]+(?P<title>[A-Z][^0-9]{3,80}?)(?=\s+\d{1,3}[.\s]+|$)", text)
        for match in matches:
            title = re.sub(r"\s+", " ", match.group("title")).strip(" .-")
            if title:
                candidates.append({"title": title, "page": int(match.group("page"))})
    return candidates


def detected_book_page_number(text: str) -> int | None:
    candidates: list[int] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        lower = line.lower()
        if lower.startswith(("serves ", "serves:", "makes ", "makes:")):
            continue
        if match := re.match(r"^(\d{1,3})\s+[A-Z][A-Za-z &,'-]{2,}$", line):
            raw = match.group(1)
            if not (len(raw) > 1 and raw.startswith("0")):
                candidates.append(int(raw))
        if match := re.search(r"\b(\d{1,3})\s+[A-Z][A-Za-z &,'-]{2,}$", line):
            raw = match.group(1)
            if not (len(raw) > 1 and raw.startswith("0")):
                candidates.append(int(raw))
        if match := re.search(r"^[A-Z][A-Za-z &,'-]{2,}\s+(\d{1,3})$", line):
            raw = match.group(1)
            if not (len(raw) > 1 and raw.startswith("0")):
                candidates.append(int(raw))
    return candidates[-1] if candidates else None


def compact_pages(pages: list[dict[str, Any]], char_budget: int | None = None) -> list[dict[str, Any]]:
    if char_budget is None:
        char_budget = int(os.getenv("DEEPSEEK_COOKBOOK_SECTION_CHAR_BUDGET", "28000"))
    compact: list[dict[str, Any]] = []
    used = 0
    for page in pages:
        text = page.get("ocrText", "")
        remaining = max(char_budget - used, 0)
        if remaining == 0:
            break
        clipped = text[:remaining]
        used += len(clipped)
        detected = detected_book_page_number(text)
        printed = detected or page.get("printedPageNumber")
        compact.append(
            {
                "id": page["id"],
                "printedPageNumber": printed,
                "scanPageNumber": scan_page_number(page),
                "detectedBookPageNumber": detected,
                "imageIndex": page.get("imageIndex"),
                "pageKind": page.get("pageKind"),
                "ocrText": clipped,
            }
        )
    return compact


def request_model(payload: dict[str, Any]) -> dict[str, Any]:
    raise_if_cancelled()
    config = llm_provider.resolve_config()
    section = payload.get("section") or {}
    section_id = section.get("id")
    section_title = section.get("title") or section_id or "whole-cookbook"

    user_content = "Extract cookbook recipes and context from this payload:\n" + json.dumps(
        payload, ensure_ascii=False
    )

    log_verbose_text(
        "llm_section_request_body",
        user_content,
        section_id=section_id,
        section_title=section_title,
        provider=config.provider,
        model=config.model,
    )
    log_event(
        "INFO",
        "llm_section_request_start",
        section_id=section_id,
        section_title=section_title,
        provider=config.provider,
        model=config.model,
        request_bytes=len(user_content),
        attempts=config.attempts,
    )

    started = time.monotonic()

    def on_attempt(attempt: int, outcome: str, detail: Any) -> None:
        if outcome == "complete":
            log_verbose_text(
                "llm_section_message_content",
                detail.text,
                section_id=section_id,
                section_title=section_title,
                attempt=attempt,
            )
            return
        log_event(
            "ERROR" if attempt == config.attempts else "WARN",
            "llm_section_request_failed",
            section_id=section_id,
            section_title=section_title,
            provider=config.provider,
            attempt=attempt,
            attempts=config.attempts,
            retryable=not isinstance(detail, llm_provider.LengthLimitError),
            error=f"{type(detail).__name__}: {detail}",
        )

    result, completion = llm_provider.complete_json(
        config, SYSTEM_PROMPT, user_content, on_attempt
    )

    log_event(
        "INFO",
        "llm_section_request_complete",
        section_id=section_id,
        section_title=section_title,
        provider=config.provider,
        model=config.model,
        duration_ms=int((time.monotonic() - started) * 1000),
        finish_reason=completion.finish_reason,
        prompt_tokens=completion.prompt_tokens,
        completion_tokens=completion.completion_tokens,
        total_tokens=completion.total_tokens,
        recipes=len(result.get("recipes", [])),
        content_blocks=len(result.get("contentBlocks", [])),
    )
    return {
        "recipes": result.get("recipes", []),
        "contentBlocks": result.get("contentBlocks", []),
    }


request_deepseek = request_model


def section_batch_payloads(
    payload: dict[str, Any],
    section: dict[str, Any],
    position_offset: int,
) -> list[dict[str, Any]]:
    pages = section_pages(payload, section)
    if not pages:
        return []

    page_batch_size = max(1, int(os.getenv("DEEPSEEK_COOKBOOK_PAGES_PER_REQUEST", "4")))
    overlap = max(0, int(os.getenv("DEEPSEEK_COOKBOOK_PAGE_OVERLAP", "1")))
    if len(pages) <= page_batch_size:
        batches = [pages]
    else:
        batches: list[list[dict[str, Any]]] = []
        index = 0
        while index < len(pages):
            batch = pages[index : index + page_batch_size]
            if batch:
                batches.append(batch)
            if index + page_batch_size >= len(pages):
                break
            index += max(1, page_batch_size - overlap)

    section_payloads: list[dict[str, Any]] = []
    section_candidates = recipe_candidates(pages)
    for batch_index, batch_pages in enumerate(batches, start=1):
        batch_section = dict(section)
        if len(batches) > 1:
            first_page = page_number(batch_pages[0])
            last_page = page_number(batch_pages[-1])
            batch_section["id"] = f"{section.get('id') or 'section'}-batch-{batch_index}"
            batch_section["title"] = f"{section.get('title') or 'Section'} pages {first_page}-{last_page}"
            batch_section["pageStart"] = first_page
            batch_section["pageEnd"] = last_page
        section_payloads.append(
            {
                "importId": payload["importId"],
                "cookbookId": payload["cookbookId"],
                "cookbookTitle": payload.get("cookbookTitle"),
                "authorIds": payload.get("authorIds", []),
                "section": batch_section,
                "recipeCandidates": section_candidates,
                "pages": compact_pages(batch_pages),
                "outputPositionOffset": position_offset + ((batch_index - 1) * 1000),
                "batch": {
                    "index": batch_index,
                    "total": len(batches),
                    "overlapPages": overlap if len(batches) > 1 else 0,
                },
            }
        )
    return section_payloads


def extract_section(payload: dict[str, Any], section: dict[str, Any], position_offset: int) -> dict[str, Any]:
    payloads = section_batch_payloads(payload, section, position_offset)
    combined: dict[str, list[dict[str, Any]]] = {"recipes": [], "contentBlocks": []}
    errors: list[str] = []
    for section_payload in payloads:
        batch_section = section_payload["section"]
        batch = section_payload.get("batch") or {}
        try:
            extracted = request_deepseek(section_payload)
            combined["recipes"].extend(extracted.get("recipes", []))
            combined["contentBlocks"].extend(extracted.get("contentBlocks", []))
        except (urllib.error.URLError, KeyError, RuntimeError, json.JSONDecodeError) as exc:
            title = batch_section.get("title") or batch_section.get("id") or "untitled section"
            error = f"{type(exc).__name__}: {exc}"
            errors.append(error)
            log_event(
                "WARN",
                "deepseek_section_mapping_skipped",
                section_id=batch_section.get("id"),
                section_title=title,
                batch_index=batch.get("index"),
                batch_total=batch.get("total"),
                error=error,
            )

    if errors and not combined["recipes"] and not combined["contentBlocks"]:
        combined["_error"] = "; ".join(errors)
    return combined


def cookbook_parallelism() -> int:
    return max(1, int(os.getenv("DEEPSEEK_COOKBOOK_PARALLELISM", "4")))


def extract_sections_parallel(
    payload: dict[str, Any],
    sections: list[dict[str, Any]],
) -> dict[str, list[dict[str, Any]] | int]:
    jobs: list[dict[str, Any]] = []
    position_offset = 0
    for section_index, section in enumerate(sections):
        payloads = section_batch_payloads(payload, section, position_offset)
        position_offset += max(1, len(payloads)) * 1000
        for batch_index, section_payload in enumerate(payloads):
            jobs.append(
                {
                    "sectionIndex": section_index,
                    "batchIndex": batch_index,
                    "section": section,
                    "payload": section_payload,
                    "result": None,
                    "error": None,
                }
            )

    if not jobs:
        return {"recipes": [], "contentBlocks": [], "failedSections": 0}

    max_workers = min(cookbook_parallelism(), len(jobs))
    log_event(
        "INFO",
        "deepseek_cookbook_queue_start",
        sections=len(sections),
        jobs=len(jobs),
        parallelism=max_workers,
    )
    with ThreadPoolExecutor(max_workers=max_workers) as executor:
        report_progress(0, len(jobs), "")
        futures: dict[Any, dict[str, Any]] = {}
        next_job_index = 0
        completed = 0

        def submit_more() -> None:
            nonlocal next_job_index
            while len(futures) < max_workers and next_job_index < len(jobs):
                raise_if_cancelled()
                job = jobs[next_job_index]
                next_job_index += 1
                futures[executor.submit(request_deepseek, job["payload"])] = job

        try:
            submit_more()
            while futures:
                done, _pending = wait(futures.keys(), return_when=FIRST_COMPLETED)
                for future in done:
                    job = futures.pop(future)
                    completed += 1
                    batch = job["payload"].get("batch") or {}
                    batch_section = job["payload"].get("section") or {}
                    report_progress(completed, len(jobs), str(batch_section.get("title") or ""))
                    try:
                        job["result"] = future.result()
                        result = job["result"]
                        log_event(
                            "INFO",
                            "deepseek_cookbook_queue_item_complete",
                            completed=completed,
                            total=len(jobs),
                            section_id=batch_section.get("id"),
                            section_title=batch_section.get("title"),
                            batch_index=batch.get("index"),
                            batch_total=batch.get("total"),
                            recipes=len(result.get("recipes", [])),
                            content_blocks=len(result.get("contentBlocks", [])),
                        )
                    except PipelineCancelled:
                        raise
                    except (urllib.error.URLError, KeyError, RuntimeError, json.JSONDecodeError) as exc:
                        error = f"{type(exc).__name__}: {exc}"
                        job["error"] = error
                        log_event(
                            "WARN",
                            "deepseek_cookbook_queue_item_failed",
                            completed=completed,
                            total=len(jobs),
                            section_id=batch_section.get("id"),
                            section_title=batch_section.get("title"),
                            batch_index=batch.get("index"),
                            batch_total=batch.get("total"),
                            error=error,
                        )
                    raise_if_cancelled()
                    submit_more()
        except PipelineCancelled:
            for future in futures:
                future.cancel()
            log_event(
                "WARN",
                "deepseek_cookbook_queue_canceled",
                completed=completed,
                total=len(jobs),
                queued=len(jobs) - next_job_index,
                running=len(futures),
            )
            raise

    combined: dict[str, list[dict[str, Any]] | int] = {
        "recipes": [],
        "contentBlocks": [],
        "failedSections": 0,
    }
    section_indexes_with_jobs = {int(job["sectionIndex"]) for job in jobs}
    section_indexes_with_results = {
        int(job["sectionIndex"])
        for job in jobs
        if isinstance(job.get("result"), dict)
        and (job["result"].get("recipes") or job["result"].get("contentBlocks"))
    }
    combined["failedSections"] = len(section_indexes_with_jobs - section_indexes_with_results)

    for job in sorted(jobs, key=lambda item: (int(item["sectionIndex"]), int(item["batchIndex"]))):
        result = job.get("result")
        if not isinstance(result, dict):
            continue
        combined["recipes"].extend(result.get("recipes", []))
        combined["contentBlocks"].extend(result.get("contentBlocks", []))

    log_event(
        "INFO",
        "deepseek_cookbook_queue_complete",
        jobs=len(jobs),
        failed_sections=combined["failedSections"],
        recipes=len(combined["recipes"]),
        content_blocks=len(combined["contentBlocks"]),
    )
    return combined


def normalize_output(payload: dict[str, Any], result: dict[str, Any]) -> dict[str, Any]:
    cookbook_id = payload["cookbookId"]
    author_ids = payload.get("authorIds", [])
    seen_ids: set[str] = set()
    recipes: list[dict[str, Any]] = []
    for index, recipe in enumerate(result.get("recipes", []), start=1):
        if not isinstance(recipe, dict):
            log_event("WARN", "deepseek_non_object_recipe_skipped", index=index)
            continue
        title = optional_string(recipe.get("title")) or f"Imported recipe {index}"
        recipe_id = slugify(optional_string(recipe.get("id")) or title, f"imported-recipe-{index}")
        if recipe_id in seen_ids:
            log_event("WARN", "deepseek_duplicate_recipe_skipped", recipe_id=recipe_id, title=title)
            continue
        seen_ids.add(recipe_id)
        recipe["id"] = recipe_id
        recipe["title"] = title
        recipe["subtitle"] = optional_string(recipe.get("subtitle"))
        recipe["alternateNames"] = normalize_alternate_names(recipe.get("alternateNames"))
        recipe["cookbookId"] = cookbook_id
        recipe["authorIds"] = string_list(recipe.get("authorIds")) or string_list(author_ids)
        recipe["pageStart"] = optional_int(recipe.get("pageStart"))
        recipe["pageEnd"] = optional_int(recipe.get("pageEnd"))
        recipe["sourceLabel"] = optional_string(recipe.get("sourceLabel")) or f"{payload.get('cookbookTitle') or cookbook_id}"
        recipe["headnote"] = optional_string(recipe.get("headnote"))
        recipe["servingContext"] = optional_string(recipe.get("servingContext"))
        recipe["yieldQuantity"] = optional_positive_float(recipe.get("yieldQuantity"))
        recipe["yieldUnit"] = optional_string(recipe.get("yieldUnit"))
        recipe["prepMinutes"] = optional_int(recipe.get("prepMinutes"))
        recipe["cookMinutes"] = optional_int(recipe.get("cookMinutes"))
        recipe["totalMinutes"] = optional_int(recipe.get("totalMinutes"))
        recipe["cuisine"] = optional_string(recipe.get("cuisine"))
        recipe["category"] = optional_string(recipe.get("category"))
        recipe["tags"] = string_list(recipe.get("tags")) or ["imported", "needs-review"]
        recipe["searchableText"] = optional_string(recipe.get("searchableText")) or ""
        recipe["sourceBlockId"] = optional_string(recipe.get("sourceBlockId"))
        recipe["sourcePageSpans"] = normalize_source_page_spans(recipe.get("sourcePageSpans"))
        recipe["componentRecipeIds"] = string_list(recipe.get("componentRecipeIds"))
        recipe["picturedPageNumber"] = optional_int(recipe.get("picturedPageNumber"))
        recipe["extractionStatus"] = "needs_review"
        recipe["images"] = normalize_images(recipe.get("images"))
        recipe["ingredients"] = normalize_ingredients(recipe.get("ingredients"))
        recipe["steps"] = normalize_steps(recipe.get("steps"))
        recipe["notes"] = []
        recipe["lastMadeAt"] = None
        recipe["timesMade"] = optional_int(recipe.get("timesMade")) or 0
        recipe["costCents"] = optional_int(recipe.get("costCents"))
        recipe["costPerServingCents"] = optional_int(recipe.get("costPerServingCents"))
        recipe["cacheKey"] = "uncached"
        recipe["cacheUpdatedAt"] = None
        recipes.append(recipe)

    blocks: list[dict[str, Any]] = []
    for index, block in enumerate(result.get("contentBlocks", []), start=1):
        if not isinstance(block, dict):
            log_event("WARN", "deepseek_non_object_content_block_skipped", index=index)
            continue
        block["id"] = optional_string(block.get("id")) or f"{payload['importId']}-context-{index}"
        block["cookbookId"] = cookbook_id
        block["sectionId"] = optional_string(block.get("sectionId"))
        block["pageStart"] = optional_int(block.get("pageStart"))
        block["pageEnd"] = optional_int(block.get("pageEnd"))
        block["position"] = index
        block["kind"] = normalize_block_kind(block.get("kind"))
        block["title"] = optional_string(block.get("title"))
        block["text"] = optional_string(block.get("text")) or ""
        block["confidence"] = optional_float(block.get("confidence"))
        raw_source_json = block.get("sourceJson")
        block["sourceJson"] = (
            raw_source_json
            if isinstance(raw_source_json, str) and raw_source_json.strip()
            else json.dumps(raw_source_json or {"source": "deepseek-cookbook"})
        )
        blocks.append(block)

    return {"recipes": recipes, "contentBlocks": blocks}


def require_list(value: Any, path: str) -> list[Any]:
    if not isinstance(value, list):
        raise RuntimeError(f"{path} must be a list")
    return value


def require_string(value: Any, path: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError(f"{path} must be a non-empty string")
    return value


def validate_normalized_output(value: dict[str, Any]) -> dict[str, Any]:
    recipes = require_list(value.get("recipes"), "recipes")
    blocks = require_list(value.get("contentBlocks"), "contentBlocks")
    for index, recipe in enumerate(recipes):
        if not isinstance(recipe, dict):
            raise RuntimeError(f"recipes[{index}] must be an object")
        require_string(recipe.get("id"), f"recipes[{index}].id")
        require_string(recipe.get("title"), f"recipes[{index}].title")
        require_string(recipe.get("cookbookId"), f"recipes[{index}].cookbookId")
        require_string(recipe.get("sourceLabel"), f"recipes[{index}].sourceLabel")
        require_list(recipe.get("ingredients"), f"recipes[{index}].ingredients")
        require_list(recipe.get("steps"), f"recipes[{index}].steps")
        require_list(recipe.get("tags"), f"recipes[{index}].tags")
        require_list(recipe.get("images"), f"recipes[{index}].images")
        require_list(recipe.get("notes"), f"recipes[{index}].notes")
    for index, block in enumerate(blocks):
        if not isinstance(block, dict):
            raise RuntimeError(f"contentBlocks[{index}] must be an object")
        require_string(block.get("id"), f"contentBlocks[{index}].id")
        require_string(block.get("cookbookId"), f"contentBlocks[{index}].cookbookId")
        require_string(block.get("kind"), f"contentBlocks[{index}].kind")
        if not isinstance(block.get("position"), int):
            raise RuntimeError(f"contentBlocks[{index}].position must be an integer")
        if not isinstance(block.get("text"), str):
            raise RuntimeError(f"contentBlocks[{index}].text must be a string")
        if not isinstance(block.get("sourceJson"), str):
            raise RuntimeError(f"contentBlocks[{index}].sourceJson must be a string")
    return value


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: deepseek_cookbook_mapper.py REQUEST_JSON", file=sys.stderr)
        return 2

    payload = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    init_verbose_stream()
    try:
        sections = payload.get("sections") or [
            {"id": None, "title": "Whole cookbook", "kind": "recipes", "pageStart": None, "pageEnd": None}
        ]
        combined = extract_sections_parallel(payload, list(sections))
        failed_sections = int(combined.get("failedSections") or 0)
        if (
            failed_sections == len(sections)
            and not combined["recipes"]
            and not combined["contentBlocks"]
        ):
            raise RuntimeError("DeepSeek failed for all cookbook sections")

        normalized = validate_normalized_output(normalize_output(payload, combined))
        normalized_json = json.dumps(normalized, ensure_ascii=False)
        log_verbose_text("deepseek_cookbook_normalized_output", normalized_json, import_id=payload["importId"])
        print(normalized_json)
        return 0
    except PipelineCancelled as exc:
        log_event("WARN", "deepseek_cookbook_mapping_canceled", error=str(exc))
        print(f"deepseek cookbook mapping canceled: {exc}", file=sys.stderr)
        return 4
    except (urllib.error.URLError, KeyError, RuntimeError, json.JSONDecodeError) as exc:
        print(f"deepseek cookbook mapping failed: {exc}", file=sys.stderr)
        return 3
    finally:
        close_verbose_stream()


if __name__ == "__main__":
    raise SystemExit(main())
