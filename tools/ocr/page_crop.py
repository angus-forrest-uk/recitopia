#!/usr/bin/env python3
"""Crop cookbook page photos before OCR.

This script is designed for the Linux/GPU OCR pipeline. It accepts the same
contract as the macOS Swift cropper:

    page_crop.py [--metadata PATH] [--inset RATIO] INPUT_IMAGE OUTPUT_PNG

It first tries an OpenCV contour/perspective crop when cv2 is installed. If
OpenCV is unavailable, or no strong quadrilateral is found, it falls back to a
NumPy page-background crop that removes neighboring-page spill without requiring
Apple Vision.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import numpy as np
except Exception as exc:  # pragma: no cover - dependency failure path.
    print(f"could not import numpy: {exc}", file=sys.stderr)
    raise SystemExit(3)

try:  # Pillow is the lightweight image I/O fallback.
    from PIL import Image, ImageOps
except Exception:  # pragma: no cover - exercised only in alternate envs.
    Image = None
    ImageOps = None

try:  # OpenCV is optional but gives better perspective correction.
    import cv2
except Exception:  # pragma: no cover - local test env may not have cv2.
    cv2 = None


@dataclass
class CropResult:
    image: np.ndarray
    method: str
    area_ratio: float
    points: list[tuple[float, float]]
    confidence: float | None = None


class CropError(Exception):
    pass


def point_report(point: tuple[float, float]) -> dict[str, float]:
    return {"x": float(point[0]), "y": float(point[1])}


def load_rgb(path: Path) -> np.ndarray:
    if Image is not None and ImageOps is not None:
        with Image.open(path) as image:
            return np.asarray(ImageOps.exif_transpose(image).convert("RGB"))

    if cv2 is not None:
        bgr = cv2.imread(str(path), cv2.IMREAD_COLOR)
        if bgr is None:
            raise CropError(f"could not load image: {path}")
        return cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)

    raise CropError("could not import Pillow or OpenCV for image I/O")


def save_png(image: np.ndarray, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    output = np.clip(image, 0, 255).astype(np.uint8)

    if Image is not None:
        Image.fromarray(output, mode="RGB").save(path, "PNG")
        return

    if cv2 is not None:
        ok = cv2.imwrite(str(path), cv2.cvtColor(output, cv2.COLOR_RGB2BGR))
        if ok:
            return

    raise CropError(f"could not write image: {path}")


def resize_rgb(image: np.ndarray, max_dimension: int) -> tuple[np.ndarray, float]:
    height, width = image.shape[:2]
    scale = min(1.0, max_dimension / max(width, height))
    if scale >= 1:
        return image, 1.0

    new_size = (max(1, int(round(width * scale))), max(1, int(round(height * scale))))
    if cv2 is not None:
        return cv2.resize(image, new_size, interpolation=cv2.INTER_AREA), scale
    if Image is not None:
        resized = Image.fromarray(image, mode="RGB").resize(new_size, Image.Resampling.BILINEAR)
        return np.asarray(resized), scale
    raise CropError("could not resize image")


def polygon_area(points: np.ndarray) -> float:
    x = points[:, 0]
    y = points[:, 1]
    return float(abs(np.dot(x, np.roll(y, -1)) - np.dot(y, np.roll(x, -1))) / 2.0)


def order_points(points: np.ndarray) -> np.ndarray:
    ordered = np.zeros((4, 2), dtype=np.float32)
    sums = points.sum(axis=1)
    diffs = np.diff(points, axis=1).reshape(-1)
    ordered[0] = points[np.argmin(sums)]
    ordered[2] = points[np.argmax(sums)]
    ordered[1] = points[np.argmin(diffs)]
    ordered[3] = points[np.argmax(diffs)]
    return ordered


def distance(a: np.ndarray, b: np.ndarray) -> float:
    return float(math.hypot(float(a[0] - b[0]), float(a[1] - b[1])))


def inset_crop(image: np.ndarray, inset_ratio: float) -> np.ndarray:
    height, width = image.shape[:2]
    dx = int(round(width * inset_ratio))
    dy = int(round(height * inset_ratio))
    if dx <= 0 and dy <= 0:
        return image
    if width - 2 * dx < 32 or height - 2 * dy < 32:
        return image
    return image[dy : height - dy, dx : width - dx]


def opencv_perspective_crop(image: np.ndarray, inset_ratio: float) -> CropResult | None:
    if cv2 is None:
        return None

    height, width = image.shape[:2]
    small, scale = resize_rgb(image, 1200)
    gray = cv2.cvtColor(small, cv2.COLOR_RGB2GRAY)
    gray = cv2.GaussianBlur(gray, (5, 5), 0)
    edges = cv2.Canny(gray, 35, 120)
    kernel = cv2.getStructuringElement(cv2.MORPH_RECT, (5, 5))
    edges = cv2.dilate(edges, kernel, iterations=1)

    contours, _ = cv2.findContours(edges, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    candidates: list[tuple[float, np.ndarray, float]] = []
    small_area = float(small.shape[0] * small.shape[1])

    for contour in contours:
        area_ratio = cv2.contourArea(contour) / small_area
        if area_ratio < 0.18 or area_ratio > 0.98:
            continue

        perimeter = cv2.arcLength(contour, True)
        approx = cv2.approxPolyDP(contour, 0.02 * perimeter, True)
        if len(approx) != 4:
            continue

        points = order_points(approx.reshape(4, 2).astype(np.float32))
        top_width = distance(points[0], points[1])
        bottom_width = distance(points[3], points[2])
        left_height = distance(points[0], points[3])
        right_height = distance(points[1], points[2])
        average_width = (top_width + bottom_width) / 2.0
        average_height = (left_height + right_height) / 2.0
        if average_width < 120 or average_height < 120:
            continue

        aspect = average_width / average_height if average_height else 0
        aspect_score = max(0.2, 1.0 - abs(aspect - 0.72))
        score = area_ratio * aspect_score
        candidates.append((score, points / scale, area_ratio))

    if not candidates:
        return None

    _score, source_points, _small_area_ratio = max(candidates, key=lambda item: item[0])
    top_width = distance(source_points[0], source_points[1])
    bottom_width = distance(source_points[3], source_points[2])
    left_height = distance(source_points[0], source_points[3])
    right_height = distance(source_points[1], source_points[2])
    output_width = max(1, int(round(max(top_width, bottom_width))))
    output_height = max(1, int(round(max(left_height, right_height))))

    destination = np.array(
        [
            [0, 0],
            [output_width - 1, 0],
            [output_width - 1, output_height - 1],
            [0, output_height - 1],
        ],
        dtype=np.float32,
    )
    transform = cv2.getPerspectiveTransform(source_points.astype(np.float32), destination)
    warped = cv2.warpPerspective(
        image,
        transform,
        (output_width, output_height),
        flags=cv2.INTER_CUBIC,
        borderMode=cv2.BORDER_REPLICATE,
    )
    warped = inset_crop(warped, inset_ratio)
    area_ratio = polygon_area(source_points) / float(width * height)

    return CropResult(
        image=warped,
        method="opencv-rectangle",
        area_ratio=area_ratio,
        points=[tuple(map(float, point)) for point in source_points],
        confidence=None,
    )


def smooth_scores(scores: np.ndarray, radius: int) -> np.ndarray:
    if radius <= 0:
        return scores
    kernel = np.ones((radius * 2 + 1,), dtype=np.float64)
    numerator = np.convolve(scores, kernel, mode="same")
    denominator = np.convolve(np.ones_like(scores), kernel, mode="same")
    return numerator / denominator


def best_interval(scores: np.ndarray, threshold: float, minimum_length: int) -> tuple[int, int] | None:
    best: tuple[int, int] | None = None
    best_weight = 0.0
    start: int | None = None
    total = 0.0

    for index in range(len(scores) + 1):
        active = index < len(scores) and scores[index] >= threshold
        if active:
            if start is None:
                start = index
                total = 0.0
            total += float(scores[index])
            continue

        if start is None:
            continue

        length = index - start
        if length >= minimum_length and total > best_weight:
            best = (start, index)
            best_weight = total
        start = None
        total = 0.0

    return best


def background_crop(image: np.ndarray) -> CropResult | None:
    height, width = image.shape[:2]
    small, scale = resize_rgb(image, 900)
    small_height, small_width = small.shape[:2]
    as_float = small.astype(np.float32) / 255.0
    red = as_float[:, :, 0]
    green = as_float[:, :, 1]
    blue = as_float[:, :, 2]
    brightness = 0.2126 * red + 0.7152 * green + 0.0722 * blue
    max_channel = np.maximum.reduce([red, green, blue])
    min_channel = np.minimum.reduce([red, green, blue])
    saturation = np.divide(
        max_channel - min_channel,
        np.maximum(max_channel, 1e-6),
        out=np.zeros_like(max_channel),
    )

    page_like = (brightness > 0.34) & (saturation < 0.28)
    column_scores = smooth_scores(page_like.mean(axis=0).astype(np.float64), max(3, small_width // 100))
    row_scores = smooth_scores(page_like.mean(axis=1).astype(np.float64), max(3, small_height // 100))

    x_range = best_interval(column_scores, threshold=0.42, minimum_length=small_width // 2)
    y_range = best_interval(row_scores, threshold=0.42, minimum_length=small_height // 2)
    if x_range is None or y_range is None:
        return None

    x0_small, x1_small = x_range
    y0_small, y1_small = y_range
    x0 = x0_small / scale
    x1 = x1_small / scale
    y0 = y0_small / scale
    y1 = y1_small / scale

    left_padding = width * (0.06 if x0_small > small_width / 20 else 0.012)
    right_padding = width * 0.012
    top_padding = height * 0.012
    bottom_padding = height * 0.06

    left = max(0, int(math.floor(x0 - left_padding)))
    right = min(width, int(math.ceil(x1 + right_padding)))
    top = max(0, int(math.floor(y0 - top_padding)))
    bottom = min(height, int(math.ceil(y1 + bottom_padding)))
    left, right = trim_bright_edge_slivers(image, left, right, top, bottom)
    if right - left < 64 or bottom - top < 64:
        return None

    area_ratio = ((right - left) * (bottom - top)) / float(width * height)
    if area_ratio < 0.18 or area_ratio > 0.995:
        return None

    cropped = image[top:bottom, left:right]
    points = [
        (float(left), float(top)),
        (float(right), float(top)),
        (float(right), float(bottom)),
        (float(left), float(bottom)),
    ]
    return CropResult(
        image=cropped,
        method="background-fallback",
        area_ratio=area_ratio,
        points=points,
        confidence=None,
    )


def trim_bright_edge_slivers(
    image: np.ndarray,
    left: int,
    right: int,
    top: int,
    bottom: int,
) -> tuple[int, int]:
    """Trim narrow bright facing-page strips that remain attached to a page band."""

    crop = image[top:bottom, left:right].astype(np.float32) / 255.0
    height, width = crop.shape[:2]
    if width < 400 or height < 400:
        return left, right

    red = crop[:, :, 0]
    green = crop[:, :, 1]
    blue = crop[:, :, 2]
    brightness = 0.2126 * red + 0.7152 * green + 0.0722 * blue
    dark = brightness < 0.36
    y0 = int(height * 0.08)
    y1 = int(height * 0.92)
    usable_brightness = brightness[y0:y1, :]
    usable_dark = dark[y0:y1, :]

    edge_width = max(24, int(width * 0.035))
    search_width = min(int(width * 0.24), 720)
    minimum_remaining = int(width * 0.65)

    def trim_left(current_left: int) -> int:
        edge_brightness = float(usable_brightness[:, :edge_width].mean())
        edge_dark_ratio = float(usable_dark[:, :edge_width].mean())
        if edge_brightness < 0.62 or edge_dark_ratio < 0.015:
            return current_left

        column_scores = smooth_scores(usable_brightness.mean(axis=0), max(3, width // 180))
        threshold = edge_brightness - 0.14
        candidates = np.flatnonzero(column_scores[:search_width] < threshold)
        if len(candidates) == 0:
            return current_left

        trim = int(candidates[0])
        if trim < int(edge_width * 0.6) or width - trim < minimum_remaining:
            return current_left
        return current_left + trim

    def trim_right(current_right: int) -> int:
        edge_brightness = float(usable_brightness[:, -edge_width:].mean())
        edge_dark_ratio = float(usable_dark[:, -edge_width:].mean())
        if edge_brightness < 0.62 or edge_dark_ratio < 0.015:
            return current_right

        column_scores = smooth_scores(usable_brightness.mean(axis=0), max(3, width // 180))
        threshold = edge_brightness - 0.14
        right_scores = column_scores[width - search_width :]
        candidates = np.flatnonzero(right_scores < threshold)
        if len(candidates) == 0:
            return current_right

        trim = int(search_width - candidates[-1])
        if trim < int(edge_width * 0.6) or width - trim < minimum_remaining:
            return current_right
        return current_right - trim

    return trim_left(left), trim_right(right)


def crop_page(input_path: Path, output_path: Path, inset_ratio: float) -> tuple[CropResult, int, int]:
    image = load_rgb(input_path)
    height, width = image.shape[:2]

    result = opencv_perspective_crop(image, inset_ratio)
    if result is None:
        result = background_crop(image)
    if result is None:
        raise CropError("no page boundary detected")

    save_png(result.image, output_path)
    return result, width, height


def write_report(path: Path | None, report: dict[str, Any]) -> None:
    text = json.dumps(report, ensure_ascii=False, sort_keys=True)
    if path is None:
        print(text)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text + "\n", encoding="utf-8")


def image_size(path: Path) -> tuple[int, int]:
    try:
        image = load_rgb(path)
    except Exception:
        return (0, 0)
    height, width = image.shape[:2]
    return (width, height)


def build_report(
    *,
    input_path: Path,
    output_path: Path,
    did_crop: bool,
    method: str,
    source_width: int,
    source_height: int,
    output_width: int = 0,
    output_height: int = 0,
    confidence: float | None = None,
    area_ratio: float | None = None,
    points: list[tuple[float, float]] | None = None,
    error: str | None = None,
) -> dict[str, Any]:
    points = points or []
    point_values = [point_report(point) for point in points]
    while len(point_values) < 4:
        point_values.append(None)

    return {
        "areaRatio": area_ratio,
        "bottomLeft": point_values[3],
        "bottomRight": point_values[2],
        "confidence": confidence,
        "didCrop": did_crop,
        "error": error,
        "inputPath": str(input_path),
        "method": method,
        "outputHeight": output_height,
        "outputPath": str(output_path),
        "outputWidth": output_width,
        "sourceHeight": source_height,
        "sourceWidth": source_width,
        "topLeft": point_values[0],
        "topRight": point_values[1],
    }


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Crop a cookbook page image before OCR.")
    parser.add_argument("--metadata", type=Path)
    parser.add_argument("--inset", type=float, default=0.006)
    parser.add_argument("input_image", type=Path)
    parser.add_argument("output_png", type=Path)
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        result, source_width, source_height = crop_page(args.input_image, args.output_png, args.inset)
        output_height, output_width = result.image.shape[:2]
        report = build_report(
            input_path=args.input_image,
            output_path=args.output_png,
            did_crop=True,
            method=result.method,
            source_width=source_width,
            source_height=source_height,
            output_width=output_width,
            output_height=output_height,
            confidence=result.confidence,
            area_ratio=result.area_ratio,
            points=result.points,
        )
        write_report(args.metadata, report)
        return 0
    except Exception as exc:
        source_width, source_height = image_size(args.input_image)
        report = build_report(
            input_path=args.input_image,
            output_path=args.output_png,
            did_crop=False,
            method="none",
            source_width=source_width,
            source_height=source_height,
            error=str(exc),
        )
        try:
            write_report(args.metadata, report)
        except Exception:
            pass
        print(str(exc), file=sys.stderr)
        return 4


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
