from __future__ import annotations

import json
import os
import sys
import time
import unittest
from contextlib import contextmanager
from pathlib import Path
from typing import Any


TOOLS_ML = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_ML))

import deepseek_cookbook_mapper as mapper  # noqa: E402


FIXTURE = TOOLS_ML / "testdata" / "mini_cookbook_payload.json"


def load_payload() -> dict[str, Any]:
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


@contextmanager
def patched_env(**values: str):
    previous = {key: os.environ.get(key) for key in values}
    try:
        for key, value in values.items():
            os.environ[key] = value
        yield
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


class DeepSeekCookbookMapperTests(unittest.TestCase):
    def test_section_batch_payloads_split_small_chapter(self) -> None:
        payload = load_payload()
        chapter = payload["sections"][1]

        with patched_env(DEEPSEEK_COOKBOOK_PAGES_PER_REQUEST="2", DEEPSEEK_COOKBOOK_PAGE_OVERLAP="0"):
            batches = mapper.section_batch_payloads(payload, chapter, 0)

        self.assertEqual(2, len(batches))
        self.assertEqual("mini-section-01-batch-1", batches[0]["section"]["id"])
        self.assertEqual([3, 4], [page["scanPageNumber"] for page in batches[0]["pages"]])
        self.assertEqual([5, 6], [page["scanPageNumber"] for page in batches[1]["pages"]])
        self.assertEqual(4, batches[0]["pages"][1]["detectedBookPageNumber"])
        self.assertEqual(1000, batches[1]["outputPositionOffset"])

    def test_compact_pages_prefers_detected_book_page_for_citations(self) -> None:
        pages = [
            {
                "id": "page-025",
                "printedPageNumber": 25,
                "imageIndex": 25,
                "pageKind": "recipe",
                "ocrText": "Chicken & Sesame Oil Porridge\nServes 6\n42 Rice & Savoury Porridge",
            }
        ]

        compacted = mapper.compact_pages(pages)

        self.assertEqual(42, compacted[0]["printedPageNumber"])
        self.assertEqual(25, compacted[0]["scanPageNumber"])
        self.assertEqual(42, compacted[0]["detectedBookPageNumber"])

    def test_invalid_unbounded_section_is_not_treated_as_whole_book(self) -> None:
        payload = load_payload()
        bad_section = {"id": "bad-section", "title": "Acknowledgements", "kind": "back_matter"}
        whole_book = {"id": None, "title": "Whole cookbook", "kind": "recipes"}

        self.assertEqual([], mapper.section_batch_payloads(payload, bad_section, 0))
        self.assertGreater(len(mapper.section_batch_payloads(payload, whole_book, 0)), 0)

    def test_parallel_extraction_merges_in_planned_order(self) -> None:
        payload = load_payload()
        chapter = payload["sections"][1]
        original_request_deepseek = mapper.request_deepseek

        def fake_request_deepseek(section_payload: dict[str, Any]) -> dict[str, list[dict[str, Any]]]:
            start = section_payload["section"]["pageStart"]
            time.sleep({3: 0.03, 4: 0.02, 5: 0.01, 6: 0.0}.get(start, 0.0))
            return {
                "recipes": [
                    {
                        "id": f"recipe-page-{start}",
                        "title": f"Recipe page {start}",
                        "cookbookId": "mini-kitchen",
                        "sourceLabel": f"Mini Kitchen, p. {start}",
                        "ingredients": [],
                        "steps": [],
                        "tags": ["imported", "needs-review"],
                        "images": [],
                        "notes": [],
                    }
                ],
                "contentBlocks": [
                    {
                        "id": f"context-page-{start}",
                        "cookbookId": "mini-kitchen",
                        "kind": "paragraph",
                        "position": start,
                        "text": f"Context page {start}",
                        "sourceJson": "{}",
                    }
                ],
            }

        mapper.request_deepseek = fake_request_deepseek
        try:
            with patched_env(
                DEEPSEEK_COOKBOOK_PAGES_PER_REQUEST="1",
                DEEPSEEK_COOKBOOK_PAGE_OVERLAP="0",
                DEEPSEEK_COOKBOOK_PARALLELISM="4",
            ):
                result = mapper.extract_sections_parallel(payload, [chapter])
        finally:
            mapper.request_deepseek = original_request_deepseek

        self.assertEqual(
            ["recipe-page-3", "recipe-page-4", "recipe-page-5", "recipe-page-6"],
            [recipe["id"] for recipe in result["recipes"]],
        )
        self.assertEqual(
            ["context-page-3", "context-page-4", "context-page-5", "context-page-6"],
            [block["id"] for block in result["contentBlocks"]],
        )
        self.assertEqual(0, result["failedSections"])

    def test_normalize_output_deduplicates_and_repositions_blocks(self) -> None:
        payload = load_payload()
        result = {
            "recipes": [
                {
                    "id": "lemon-rice",
                    "title": "Lemon Rice",
                    "cookbookId": "wrong",
                    "sourceLabel": "Mini Kitchen, p. 4",
                    "ingredients": [],
                    "steps": [],
                    "tags": ["imported"],
                    "images": [],
                    "notes": [],
                },
                {
                    "id": "lemon-rice",
                    "title": "Lemon Rice duplicate",
                    "cookbookId": "wrong",
                    "sourceLabel": "Mini Kitchen, p. 4",
                    "ingredients": [],
                    "steps": [],
                    "tags": ["imported"],
                    "images": [],
                    "notes": [],
                },
            ],
            "contentBlocks": [
                {
                    "id": "block-b",
                    "cookbookId": "wrong",
                    "kind": "essay",
                    "position": 99,
                    "text": "Second",
                    "sourceJson": "{}",
                },
                {
                    "id": "block-a",
                    "cookbookId": "wrong",
                    "kind": "intro",
                    "position": 42,
                    "text": "First",
                    "sourceJson": "{}",
                },
            ],
        }

        normalized = mapper.validate_normalized_output(mapper.normalize_output(payload, result))

        self.assertEqual(1, len(normalized["recipes"]))
        self.assertEqual("mini-kitchen", normalized["recipes"][0]["cookbookId"])
        self.assertEqual([1, 2], [block["position"] for block in normalized["contentBlocks"]])
        self.assertEqual(["paragraph", "paragraph"], [block["kind"] for block in normalized["contentBlocks"]])

    def test_normalize_output_coerces_deepseek_loose_types_for_zig(self) -> None:
        payload = load_payload()
        result = {
            "recipes": [
                {
                    "id": 42,
                    "title": "Loose Rice",
                    "cookbookId": "wrong",
                    "authorIds": "anna-jones",
                    "pageStart": "12",
                    "pageEnd": "p. 13",
                    "sourceLabel": "Mini Kitchen, pp. 12-13",
                    "yieldQuantity": "2",
                    "prepMinutes": "15 minutes",
                    "cookMinutes": "20",
                    "sourcePageSpans": [
                        {
                            "pageId": 12,
                            "printedPageNumber": "12",
                            "lineStart": "4",
                            "lineEnd": "19",
                            "confidence": "0.82",
                        }
                    ],
                    "picturedPageNumber": "13",
                    "images": ["images/loose-rice.jpg"],
                    "ingredients": [
                        {
                            "id": 7,
                            "position": "1",
                            "displayName": "1/2 cup rice",
                            "item": "rice",
                            "quantity": "1/2",
                            "optional": "false",
                            "sourceLine": "8",
                        },
                        "salt",
                    ],
                    "steps": [
                        {"id": 9, "position": "1", "text": "Rinse.", "sourceLineStart": "12"},
                        "Cook until tender.",
                    ],
                    "tags": "imported",
                }
            ],
            "contentBlocks": [
                {
                    "id": 99,
                    "cookbookId": "wrong",
                    "sectionId": 3,
                    "pageStart": "12",
                    "pageEnd": "13",
                    "kind": "essay",
                    "title": 123,
                    "text": "Context",
                    "confidence": "0.91",
                    "sourceJson": {"source": "deepseek"},
                }
            ],
        }

        normalized = mapper.validate_normalized_output(mapper.normalize_output(payload, result))
        encoded = json.dumps(normalized)

        recipe = normalized["recipes"][0]
        self.assertIsInstance(encoded, str)
        self.assertEqual("42", recipe["id"])
        self.assertEqual([12, 13], [recipe["pageStart"], recipe["pageEnd"]])
        self.assertEqual(["anna-jones"], recipe["authorIds"])
        self.assertEqual(0.5, recipe["ingredients"][0]["quantity"])
        self.assertEqual("salt", recipe["ingredients"][1]["displayName"])
        self.assertEqual(2, recipe["steps"][1]["position"])
        self.assertEqual(12, recipe["sourcePageSpans"][0]["printedPageNumber"])
        self.assertEqual("image-1", recipe["images"][0]["id"])
        self.assertEqual({"source": "deepseek"}, json.loads(normalized["contentBlocks"][0]["sourceJson"]))

    def test_normalize_ingredients_marks_ocr_digit_loss_for_review(self) -> None:
        ingredients = mapper.normalize_ingredients(
            [
                {
                    "displayName": "tbsp mirin",
                    "item": "mirin",
                    "quantity": None,
                    "unit": "tbsp",
                },
                {
                    "displayName": "00g halibut, cut into large bite-size chunks",
                    "item": "halibut",
                    "quantity": 0,
                    "unit": "g",
                    "preparation": "cut into large bite-size chunks",
                },
            ]
        )

        self.assertEqual([None, None], [item["quantity"] for item in ingredients])
        self.assertEqual(["unknown", "unknown"], [item["quantityKind"] for item in ingredients])
        self.assertEqual(
            ["needs_review", "needs_review"],
            [item["quantityReviewStatus"] for item in ingredients],
        )
        self.assertEqual(
            ["possible_ocr_leading_digit_loss", "possible_ocr_leading_digit_loss"],
            [item["quantityReviewReason"] for item in ingredients],
        )

    def test_normalize_ingredients_preserves_ranges_without_midpoints(self) -> None:
        ingredients = mapper.normalize_ingredients(
            [
                {
                    "displayName": "1-2 tsp maple syrup, to taste",
                    "item": "maple syrup",
                    "quantity": 1.5,
                    "unit": "tsp",
                }
            ]
        )

        ingredient = ingredients[0]
        self.assertIsNone(ingredient["quantity"])
        self.assertEqual("1-2", ingredient["quantityText"])
        self.assertEqual("range", ingredient["quantityKind"])
        self.assertEqual(1, ingredient["quantityMin"])
        self.assertEqual(2, ingredient["quantityMax"])
        self.assertEqual("parsed", ingredient["quantityReviewStatus"])

    def test_normalize_ingredients_fixes_mixed_fractions_and_size_units(self) -> None:
        ingredients = mapper.normalize_ingredients(
            [
                {
                    "displayName": "1 1/2 cm piece of ginger, finely grated",
                    "item": "ginger",
                    "quantity": None,
                    "unit": "cm",
                    "preparation": "piece, finely grated",
                },
                {
                    "displayName": "21/2 tbsp honey",
                    "item": "honey",
                    "quantity": "21/2",
                    "unit": "tbsp",
                },
                {
                    "displayName": "6 large dried anchovies",
                    "item": "dried anchovies",
                    "quantity": 6,
                    "unit": "large",
                },
            ]
        )

        self.assertEqual(1.5, ingredients[0]["quantity"])
        self.assertEqual("1 1/2", ingredients[0]["quantityText"])
        self.assertEqual(2.5, ingredients[1]["quantity"])
        self.assertEqual(6, ingredients[2]["quantity"])
        self.assertIsNone(ingredients[2]["unit"])
        self.assertEqual("large", ingredients[2]["preparation"])

    def test_normalize_ingredients_treats_as_needed_zero_as_parsed_null(self) -> None:
        ingredients = mapper.normalize_ingredients(
            [
                {
                    "displayName": "wasabi paste, to taste",
                    "item": "wasabi paste",
                    "quantity": 0,
                    "unit": None,
                    "preparation": "to taste",
                }
            ]
        )

        ingredient = ingredients[0]
        self.assertIsNone(ingredient["quantity"])
        self.assertEqual("as_needed", ingredient["quantityKind"])
        self.assertEqual("parsed", ingredient["quantityReviewStatus"])
        self.assertIsNone(ingredient["quantityReviewReason"])


if __name__ == "__main__":
    unittest.main()
