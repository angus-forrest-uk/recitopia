#!/usr/bin/env python3
"""Run PaddleOCR for Recitopia image imports.

the server should install PaddleOCR with a GPU-enabled PaddlePaddle wheel and run
this script through RECITOPIA_OCR_PYTHON. The script prints a compact JSON
payload so the API can remain the orchestration layer.
"""

from __future__ import annotations

import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


def _env_bool(key: str, default: bool) -> bool:
    value = os.getenv(key)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _jsonable(value: Any) -> Any:
    if isinstance(value, dict):
        return {str(key): _jsonable(child) for key, child in value.items()}
    if isinstance(value, (list, tuple)):
        return [_jsonable(child) for child in value]
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if hasattr(value, "tolist"):
        try:
            return _jsonable(value.tolist())
        except Exception:
            pass
    # Dict-like result objects (paddlex OCRResult) expose keys/__getitem__ with
    # live numpy values. Prefer that over the .json accessor: paddlex's own
    # serializer stringifies ndarrays with `...` elision, which destroys the
    # box coordinates and silently disables all layout-aware processing.
    keys_attr = getattr(value, "keys", None)
    if callable(keys_attr) and hasattr(value, "__getitem__"):
        try:
            return {str(key): _jsonable(value[key]) for key in value.keys()}
        except Exception:
            pass
    for attr in ("to_dict", "dict", "json", "to_json"):
        if not hasattr(value, attr):
            continue
        child = getattr(value, attr)
        try:
            return _jsonable(child() if callable(child) else child)
        except Exception:
            continue
    if hasattr(value, "__dict__"):
        return _jsonable(value.__dict__)
    return str(value)


CJK_TEXT_RE = re.compile(
    "["
    "\u1100-\u11ff"  # Hangul Jamo
    "\u2e80-\u2eff"  # CJK radicals
    "\u2f00-\u2fdf"  # Kangxi radicals
    "\u3040-\u30ff"  # Hiragana and Katakana
    "\u3130-\u318f"  # Hangul compatibility Jamo
    "\u31f0-\u31ff"  # Katakana extensions
    "\u3400-\u4dbf"  # CJK extension A
    "\u4e00-\u9fff"  # CJK unified ideographs
    "\ua960-\ua97f"  # Hangul Jamo extended A
    "\uac00-\ud7af"  # Hangul syllables
    "\uf900-\ufaff"  # CJK compatibility ideographs
    "\U00020000-\U0002fa1f"  # CJK extensions B through I
    "]"
)


def _sanitize_ocr_text(value: Any) -> str:
    text = CJK_TEXT_RE.sub("", str(value)).strip()
    return re.sub(r"\s{2,}", " ", text)


@dataclass(frozen=True)
class OcrLine:
    text: str
    score: float | None
    points: tuple[tuple[float, float], ...]

    @property
    def x_min(self) -> float:
        return min(point[0] for point in self.points)

    @property
    def x_max(self) -> float:
        return max(point[0] for point in self.points)

    @property
    def y_min(self) -> float:
        return min(point[1] for point in self.points)

    @property
    def y_max(self) -> float:
        return max(point[1] for point in self.points)

    @property
    def center_x(self) -> float:
        return (self.x_min + self.x_max) / 2

    @property
    def width(self) -> float:
        return self.x_max - self.x_min


def _extract_texts(value: Any) -> list[str]:
    if isinstance(value, dict):
        texts: list[str] = []
        for key in ("rec_texts", "texts"):
            items = value.get(key)
            if isinstance(items, list):
                texts.extend(text for item in items if (text := _sanitize_ocr_text(item)))
        for key in ("rec_text", "text", "transcription"):
            item = value.get(key)
            if isinstance(item, str):
                text = _sanitize_ocr_text(item)
                if text:
                    texts.append(text)
        for child in value.values():
            texts.extend(_extract_texts(child))
        return texts
    if isinstance(value, (list, tuple)):
        texts: list[str] = []
        if len(value) >= 2 and isinstance(value[0], str) and isinstance(value[1], (int, float)):
            text = _sanitize_ocr_text(value[0])
            if text:
                texts.append(text)
        if (
            len(value) >= 2
            and isinstance(value[1], (list, tuple))
            and len(value[1]) >= 1
            and isinstance(value[1][0], str)
        ):
            text = _sanitize_ocr_text(value[1][0])
            if text:
                texts.append(text)
        for child in value:
            texts.extend(_extract_texts(child))
        return texts
    return []


def _number(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    return None


def _point(value: Any) -> tuple[float, float] | None:
    if not isinstance(value, (list, tuple)) or len(value) < 2:
        return None
    x = _number(value[0])
    y = _number(value[1])
    if x is None or y is None:
        return None
    return (x, y)


def _points_from_box(value: Any) -> tuple[tuple[float, float], ...] | None:
    if isinstance(value, dict):
        for key in (
            "points",
            "box",
            "bbox",
            "poly",
            "polys",
            "polygon",
            "dt_poly",
            "dt_polys",
            "rec_poly",
            "rec_polys",
        ):
            if key in value:
                points = _points_from_box(value[key])
                if points:
                    return points
        return None
    if not isinstance(value, (list, tuple)):
        return None
    if len(value) == 4 and all(_number(item) is not None for item in value):
        x1, y1, x2, y2 = (float(item) for item in value)
        return ((x1, y1), (x2, y1), (x2, y2), (x1, y2))
    points = [_point(item) for item in value]
    if len(points) >= 2 and all(point is not None for point in points):
        return tuple(point for point in points if point is not None)
    if len(value) == 1:
        return _points_from_box(value[0])
    return None


def _score(value: Any) -> float | None:
    number = _number(value)
    return number if number is not None else None


def _line_from_parts(text: Any, box: Any, score: Any = None) -> OcrLine | None:
    text_value = _sanitize_ocr_text(text) if text is not None else ""
    if not text_value:
        return None
    points = _points_from_box(box)
    if not points:
        return None
    return OcrLine(text=text_value, score=_score(score), points=points)


def _first_list(value: dict[str, Any], keys: tuple[str, ...]) -> list[Any] | None:
    for key in keys:
        items = value.get(key)
        if isinstance(items, list):
            return items
    return None


def _extract_line_items(value: Any) -> list[OcrLine]:
    if isinstance(value, dict):
        lines: list[OcrLine] = []

        texts = _first_list(value, ("rec_texts", "texts"))
        boxes = _first_list(
            value,
            (
                "rec_polys",
                "dt_polys",
                "text_det_polys",
                "boxes",
                "polys",
                "points",
                "rec_boxes",
            ),
        )
        scores = _first_list(value, ("rec_scores", "scores", "confidences"))
        if texts is not None and boxes is not None:
            for index, text in enumerate(texts):
                if index >= len(boxes):
                    break
                line = _line_from_parts(
                    text,
                    boxes[index],
                    scores[index] if scores is not None and index < len(scores) else None,
                )
                if line:
                    lines.append(line)

        single_text = None
        for key in ("rec_text", "text", "transcription"):
            if isinstance(value.get(key), str):
                single_text = value[key]
                break
        if single_text is not None:
            for key in ("box", "bbox", "poly", "polygon", "points", "dt_poly", "rec_poly"):
                if key in value:
                    line = _line_from_parts(single_text, value[key], value.get("score"))
                    if line:
                        lines.append(line)
                        break

        for child in value.values():
            lines.extend(_extract_line_items(child))
        return _dedupe_lines(lines)

    if isinstance(value, (list, tuple)):
        if len(value) >= 2:
            line = _line_from_v2_item(value)
            if line:
                return [line]
        lines: list[OcrLine] = []
        for child in value:
            lines.extend(_extract_line_items(child))
        return _dedupe_lines(lines)

    return []


def _line_from_v2_item(value: list[Any] | tuple[Any, ...]) -> OcrLine | None:
    box = value[0] if value else None
    payload = value[1] if len(value) >= 2 else None
    if isinstance(payload, (list, tuple)) and payload:
        text = payload[0]
        score = payload[1] if len(payload) >= 2 else None
        return _line_from_parts(text, box, score)
    return None


def _dedupe_lines(lines: list[OcrLine]) -> list[OcrLine]:
    seen: set[tuple[str, int, int, int, int]] = set()
    unique: list[OcrLine] = []
    for line in lines:
        key = (
            line.text,
            round(line.x_min),
            round(line.y_min),
            round(line.x_max),
            round(line.y_max),
        )
        if key in seen:
            continue
        seen.add(key)
        unique.append(line)
    return unique


def _median(values: list[float]) -> float:
    ordered = sorted(values)
    if not ordered:
        return 0
    midpoint = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[midpoint]
    return (ordered[midpoint - 1] + ordered[midpoint]) / 2


def _column_boundaries(lines: list[OcrLine], page_width: float) -> tuple[list[float], str | None]:
    gutter_boundaries = _gutter_column_boundaries(lines, page_width)
    edge_boundaries = _edge_alignment_column_boundaries(lines, page_width)

    if edge_boundaries and (
        not gutter_boundaries or len(edge_boundaries) > len(gutter_boundaries)
    ):
        return edge_boundaries, "edge-alignment"
    if gutter_boundaries:
        return gutter_boundaries, "gutter"
    return [], None


def _gutter_column_boundaries(lines: list[OcrLine], page_width: float) -> list[float]:
    """Find column gutters by projecting line extents onto the x axis.

    Center-gap heuristics miss adjacent columns whose centers are close (the
    common case for index and ingredient/method layouts); a gutter — an x band
    that no column-width line crosses — is a much stronger signal.
    """
    candidates = [line for line in lines if line.width <= page_width * 0.72]
    if len(candidates) < 6:
        return []

    x_min = min(line.x_min for line in candidates)
    x_max = max(line.x_max for line in candidates)
    span = x_max - x_min
    if span <= 0:
        return []

    buckets = 256
    coverage = [0] * buckets

    def bucket(x: float) -> int:
        return min(buckets - 1, max(0, int((x - x_min) / span * (buckets - 1))))

    for line in candidates:
        for index in range(bucket(line.x_min), bucket(line.x_max) + 1):
            coverage[index] += 1

    # A gutter tolerates a few bridging lines (running footers, headings that
    # cross columns, smears) but must be essentially empty, reasonably wide,
    # and interior to the text body.
    allowed = max(1, len(candidates) // 25)
    min_run = max(3, int(buckets * 0.012))
    runs: list[tuple[int, int, int]] = []  # (length, start, end)
    start: int | None = None
    for index in range(buckets):
        if coverage[index] <= allowed:
            if start is None:
                start = index
            continue
        if start is not None:
            runs.append((index - start, start, index - 1))
            start = None
    if start is not None:
        runs.append((buckets - start, start, buckets - 1))

    def interior(run: tuple[int, int, int]) -> bool:
        _, run_start, run_end = run
        center = (run_start + run_end) / 2 / buckets
        return run[0] >= min_run and 0.15 <= center <= 0.85

    gutters = sorted((run for run in runs if interior(run)), reverse=True)[:2]
    if not gutters:
        return []

    boundaries = sorted(
        x_min + ((run_start + run_end) / 2) / (buckets - 1) * span
        for _, run_start, run_end in gutters
    )

    # Every resulting column must hold real content or the boundary is noise.
    counts = [0] * (len(boundaries) + 1)
    for line in candidates:
        counts[_column_index(line, boundaries)] += 1
    if any(count < 3 for count in counts):
        return []
    return boundaries


def _stddev(values: list[float]) -> float:
    if not values:
        return 0
    mean = sum(values) / len(values)
    return (sum((value - mean) ** 2 for value in values) / len(values)) ** 0.5


def _edge_alignment_column_boundaries(lines: list[OcrLine], page_width: float) -> list[float]:
    """Find columns by repeated aligned left edges.

    Cookbook pages often have boxes that bridge the visual gutter a little
    (wide ingredient boxes, OCR smears, wrapped index entries). In those cases
    projection says "no empty gutter", but the line starts still reveal the
    columns: x_min stacks tightly while x_max wanders.
    """
    candidates = [line for line in lines if line.width <= page_width * 0.82]
    if len(candidates) < 6:
        return []

    tolerance = max(12.0, page_width * 0.025)
    min_center_separation = page_width * 0.16
    min_cluster_lines = max(3, len(candidates) // 10)

    clusters: list[list[OcrLine]] = []
    for line in sorted(candidates, key=lambda item: item.x_min):
        if not clusters:
            clusters.append([line])
            continue
        current = clusters[-1]
        current_edge = _median([item.x_min for item in current])
        if abs(line.x_min - current_edge) <= tolerance:
            current.append(line)
        else:
            clusters.append([line])

    scored: list[dict[str, Any]] = []
    for cluster in clusters:
        if len(cluster) < min_cluster_lines:
            continue
        x_starts = [line.x_min for line in cluster]
        x_ends = [line.x_max for line in cluster]
        start_spread = _stddev(x_starts)
        if start_spread > tolerance:
            continue
        scored.append(
            {
                "lines": cluster,
                "count": len(cluster),
                "xStart": _median(x_starts),
                "center": _median([line.center_x for line in cluster]),
                "raggedness": _stddev(x_ends),
                "score": len(cluster) * (1.0 + min(_stddev(x_ends) / max(tolerance, 1), 2.0)),
            }
        )

    if len(scored) < 2:
        return []

    selected: list[dict[str, Any]] = []
    for cluster in sorted(scored, key=lambda item: (-float(item["score"]), float(item["xStart"]))):
        center = float(cluster["center"])
        if any(abs(center - float(existing["center"])) < min_center_separation for existing in selected):
            continue
        selected.append(cluster)
        if len(selected) == 4:
            break

    selected.sort(key=lambda item: float(item["center"]))
    if len(selected) < 2:
        return []

    # Drop decorative/heading alignments at the page edges if they do not form
    # usable columns under center-based assignment.
    boundaries = [
        (float(left["center"]) + float(right["center"])) / 2
        for left, right in zip(selected, selected[1:])
    ]

    counts = [0] * (len(boundaries) + 1)
    for line in candidates:
        counts[_column_index(line, boundaries)] += 1
    if any(count < 3 for count in counts):
        return []
    return boundaries


def _column_index(line: OcrLine, boundaries: list[float]) -> int:
    for index, boundary in enumerate(boundaries):
        if line.center_x < boundary:
            return index
    return len(boundaries)


def _sort_reading_order(lines: list[OcrLine]) -> list[OcrLine]:
    median_height = _median([line.y_max - line.y_min for line in lines]) or 16
    return sorted(lines, key=lambda line: (round(line.y_min / max(median_height, 1)), line.x_min))


FOOTER_RE = re.compile(r"^(?:(?P<leading>[1-9]\d{0,2})\s+(?P<leading_title>[A-Z][A-Za-z&,'’ -]{2,})|(?P<trailing_title>[A-Z][A-Za-z&,'’ -]{2,})\s+(?P<trailing>[1-9]\d{0,2}))$")


def _footer_page_number(line: OcrLine, page_width: float, page_height: float, page_y_min: float) -> int | None:
    if page_height <= 0 or page_width <= 0:
        return None
    if line.y_min < page_y_min + page_height * 0.84:
        return None

    match = FOOTER_RE.match(line.text.strip())
    if not match:
        return None

    number_text = match.group("leading") or match.group("trailing")
    title = match.group("leading_title") or match.group("trailing_title") or ""
    if any(char.isdigit() for char in title):
        return None

    center_ratio = line.center_x / page_width
    if line.x_min < page_width * 0.34 or line.x_max > page_width * 0.66:
        return int(number_text)
    if center_ratio < 0.42 or center_ratio > 0.58:
        return int(number_text)
    return None


BARE_NUMBER_RE = re.compile(r"^[1-9]\d{0,2}$")


def _corner_page_number(
    line: OcrLine,
    page_width: float,
    page_height: float,
    page_x_min: float,
    page_y_min: float,
) -> tuple[int, str] | None:
    """A bare 1-3 digit number counts as a page number only when its box sits
    in the bottom-left or bottom-right corner of the page — position is the
    validation, since body text (index entries, quantities) is full of bare
    numbers."""
    if page_height <= 0 or page_width <= 0:
        return None
    if line.y_min < page_y_min + page_height * 0.88:
        return None
    if not BARE_NUMBER_RE.match(line.text.strip()):
        return None

    center_ratio = (line.center_x - page_x_min) / page_width
    if center_ratio <= 0.30:
        return int(line.text.strip()), "left"
    if center_ratio >= 0.70:
        return int(line.text.strip()), "right"
    return None


def _detect_printed_page_number(lines: list[OcrLine]) -> tuple[int | None, str | None]:
    if not lines:
        return None, None

    page_x_min = min(line.x_min for line in lines)
    page_x_max = max(line.x_max for line in lines)
    page_y_min = min(line.y_min for line in lines)
    page_y_max = max(line.y_max for line in lines)
    page_width = page_x_max - page_x_min
    page_height = page_y_max - page_y_min

    # Footer "266 Index" style lines are the strongest signal; bare corner
    # numbers back them up on pages without running footers.
    footer_candidates: list[tuple[float, int, str]] = []
    corner_candidates: list[tuple[float, int, str]] = []
    for line in lines:
        number = _footer_page_number(line, page_width, page_height, page_y_min)
        if number is not None:
            center_ratio = (line.center_x - page_x_min) / max(page_width, 1)
            side = "left" if center_ratio < 0.5 else "right"
            footer_candidates.append((line.y_min, number, side))
            continue
        corner = _corner_page_number(line, page_width, page_height, page_x_min, page_y_min)
        if corner is not None:
            corner_candidates.append((line.y_min, corner[0], corner[1]))

    for candidates in (footer_candidates, corner_candidates):
        if candidates:
            _, number, side = max(candidates)
            return number, side
    return None, None


def _layout_aware_text(lines: list[OcrLine]) -> tuple[str, dict[str, Any]]:
    lines = _dedupe_lines([line for line in lines if line.text.strip()])
    if len(lines) < 4:
        ordered = _sort_reading_order(lines)
        return "\n".join(line.text for line in ordered), {"mode": "single", "columns": 1}

    page_x_min = min(line.x_min for line in lines)
    page_x_max = max(line.x_max for line in lines)
    page_width = page_x_max - page_x_min
    boundaries, detection = _column_boundaries(lines, page_width)
    if not boundaries:
        ordered = _sort_reading_order(lines)
        return "\n".join(line.text for line in ordered), {"mode": "single", "columns": 1}

    column_count = len(boundaries) + 1
    column_first_y = [float("inf")] * column_count
    for line in lines:
        if line.width > page_width * 0.72:
            continue
        column = _column_index(line, boundaries)
        column_first_y[column] = min(column_first_y[column], line.y_min)

    finite_first_y = [value for value in column_first_y if value != float("inf")]
    body_start_y = max(finite_first_y) if len(finite_first_y) >= 2 else min(line.y_min for line in lines)
    median_height = _median([line.y_max - line.y_min for line in lines]) or 16

    preamble = [line for line in lines if line.y_min < body_start_y - median_height * 0.5]
    body = [line for line in lines if line not in preamble]
    spanning = [line for line in body if line.width > page_width * 0.72]
    column_lines = [line for line in body if line.width <= page_width * 0.72]

    output: list[str] = []
    blocks: list[dict[str, Any]] = []
    if preamble:
        ordered = _sort_reading_order(preamble)
        text = "\n".join(line.text for line in ordered)
        output.append(text)
        blocks.append({"kind": "preamble", "text": text})

    for column in range(column_count):
        ordered = _sort_reading_order(
            [line for line in column_lines if _column_index(line, boundaries) == column]
        )
        if not ordered:
            continue
        text = "\n".join(line.text for line in ordered)
        output.append(text)
        blocks.append({"kind": "column", "index": column + 1, "text": text})

    if spanning:
        ordered = _sort_reading_order(spanning)
        text = "\n".join(line.text for line in ordered)
        output.append(text)
        blocks.append({"kind": "spanning", "text": text})

    return "\n\n".join(part for part in output if part.strip()), {
        "mode": "columns",
        "columns": column_count,
        "detection": detection,
        "boundaries": boundaries,
        "bodyStartY": body_start_y,
        "blocks": blocks,
    }


def _build_ocr(PaddleOCR: Any) -> tuple[Any, str]:
    lang = os.getenv("RECITOPIA_PADDLE_LANG", "en")
    attempts: list[tuple[str, dict[str, Any]]] = [
        (
            "paddleocr3",
            {
                "use_doc_orientation_classify": _env_bool(
                    "RECITOPIA_PADDLE_DOC_ORIENTATION", True
                ),
                "use_doc_unwarping": _env_bool("RECITOPIA_PADDLE_DOC_UNWARPING", True),
                "use_textline_orientation": _env_bool(
                    "RECITOPIA_PADDLE_TEXTLINE_ORIENTATION", True
                ),
            },
        ),
        (
            "paddleocr2",
            {
                "use_angle_cls": _env_bool("RECITOPIA_PADDLE_USE_ANGLE_CLS", True),
                "use_gpu": _env_bool("RECITOPIA_PADDLE_USE_GPU", True),
                "lang": lang,
            },
        ),
        ("paddleocr-default", {}),
    ]

    last_type_error: TypeError | None = None
    for name, kwargs in attempts:
        try:
            return PaddleOCR(**kwargs), name
        except TypeError as exc:
            last_type_error = exc
            print(f"{name} constructor rejected arguments: {exc}", file=sys.stderr)

    if last_type_error is not None:
        raise last_type_error
    raise RuntimeError("could not construct PaddleOCR")


def _predict(ocr: Any, image_path: Path) -> Any:
    if hasattr(ocr, "predict"):
        try:
            return ocr.predict(str(image_path))
        except TypeError:
            return ocr.predict(input=str(image_path))
    if hasattr(ocr, "ocr"):
        return ocr.ocr(str(image_path), cls=True)
    raise RuntimeError("PaddleOCR object has neither predict nor ocr")


def load_ocr() -> tuple[Any, str, str]:
    import paddleocr
    from paddleocr import PaddleOCR

    ocr, api_mode = _build_ocr(PaddleOCR)
    version = getattr(paddleocr, "__version__", "unknown")
    return ocr, api_mode, version


HEAVY_RAW_KEYS = (
    "doc_preprocessor_res",
    "doc_preprocessor_image",
    "input_img",
    "output_img",
    "img",
)


def _prune_raw(raw_items: Any) -> Any:
    """Drop PaddleOCR's preprocessed image tensors from the raw payload.

    doc_preprocessor_res alone is ~88 MB per page while every field the layout
    and review code reads (rec_texts, rec_polys, rec_scores, rec_boxes,
    dt_polys) totals a few kilobytes. The raw payload is persisted per page, so
    keeping the tensors costs gigabytes per book and buys nothing.
    """
    if isinstance(raw_items, list):
        return [_prune_raw(item) for item in raw_items]
    if isinstance(raw_items, dict):
        return {
            key: _prune_raw(value)
            for key, value in raw_items.items()
            if key not in HEAVY_RAW_KEYS
        }
    return raw_items


def run_ocr(image_path: Path, ocr: Any | None = None, api_mode: str | None = None, version: str | None = None) -> dict[str, Any]:
    if not image_path.exists():
        raise FileNotFoundError(f"image not found: {image_path}")

    loaded_ocr = ocr
    loaded_api_mode = api_mode
    loaded_version = version
    if loaded_ocr is None:
        loaded_ocr, loaded_api_mode, loaded_version = load_ocr()

    result = _predict(loaded_ocr, image_path)
    raw_items = _jsonable(result)
    line_items = _extract_line_items(raw_items)
    printed_page_number, printed_page_number_side = _detect_printed_page_number(line_items)
    if line_items:
        text, layout = _layout_aware_text(line_items)
    else:
        lines = _extract_texts(raw_items)
        text = "\n".join(dict.fromkeys(lines))
        layout = {"mode": "fallback", "columns": 1}
    return {
        "engine": f"paddleocr:{loaded_version or 'unknown'}:{loaded_api_mode or 'unknown'}",
        "text": text,
        "layout": layout,
        "printedPageNumber": printed_page_number,
        "printedPageNumberSide": printed_page_number_side,
        "raw": _prune_raw(raw_items),
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: paddle_ocr.py IMAGE_PATH", file=sys.stderr)
        return 2

    image_path = Path(sys.argv[1])
    try:
        payload = run_ocr(image_path)
    except Exception as exc:  # pragma: no cover - exercised on the server.
        print(f"could not run paddleocr: {exc}", file=sys.stderr)
        return 3

    print(json.dumps(payload, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
