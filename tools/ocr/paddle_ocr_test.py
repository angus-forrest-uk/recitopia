from __future__ import annotations

import sys
import unittest
from pathlib import Path
from typing import Any


TOOLS_OCR = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_OCR))

import paddle_ocr  # noqa: E402


def box(x: int, y: int, width: int, height: int = 12) -> list[list[int]]:
    return [[x, y], [x + width, y], [x + width, y + height], [x, y + height]]


def raw_from_lines(lines: list[tuple[str, int, int, int]]) -> dict[str, Any]:
    return {
        "rec_texts": [text for text, _, _, _ in lines],
        "rec_scores": [0.98 for _ in lines],
        "dt_polys": [box(x, y, width) for _, x, y, width in lines],
    }


class PaddleOcrLayoutTests(unittest.TestCase):
    def test_jsonable_preserves_numpy_like_coordinate_lists(self) -> None:
        class ArrayLike:
            def tolist(self) -> list[list[int]]:
                return [[10, 20], [30, 20], [30, 40], [10, 40]]

        self.assertEqual(
            [[10, 20], [30, 20], [30, 40], [10, 40]],
            paddle_ocr._jsonable(ArrayLike()),
        )

    def test_layout_aware_text_reads_three_column_contents_by_column(self) -> None:
        raw = raw_from_lines(
            [
                ("01", 295, 0, 30),
                ("26. Short-grain Rice", 40, 30, 150),
                ("32. Raw Fish, Vegetable", 250, 30, 160),
                ("43. Black Sesame", 470, 30, 145),
                ("& Rice Salad", 250, 46, 90),
                ("Seed Porridge", 470, 46, 110),
                ("26. Five-grain Rice", 40, 68, 150),
                ("34. Rice & Seaweed Rolls", 250, 68, 175),
                ("44. Pumpkin Rice Porridge", 470, 68, 180),
                ("立叫夸", 45, 84, 60),
                ("28. Mixed Rice with", 40, 100, 150),
                ("Vegetables & Beef", 40, 116, 135),
                ("40. Pine Nut & Rice Porridge", 250, 100, 190),
                ("48. Braised Rice Cakes with", 470, 100, 190),
                ("Cabbage & Fishcakes", 470, 116, 160),
                ("30. Jeonju Bibimbap", 40, 148, 155),
                ("50. Crispy Chilli Rice Cakes", 250, 148, 200),
                ("31. Kimchi Fried Rice", 40, 180, 160),
                ("42. Chicken & Sesame Oil", 250, 180, 180),
                ("51. Crispy Soy Rice Cakes", 470, 180, 185),
                ("Porridge", 250, 196, 65),
                ("Rice & Savoury Porridge 25", 360, 230, 210),
            ]
        )

        text, layout = paddle_ocr._layout_aware_text(paddle_ocr._extract_line_items(raw))

        self.assertEqual(3, layout["columns"])
        self.assertLess(text.index("31. Kimchi Fried Rice"), text.index("32. Raw Fish"))
        self.assertLess(text.index("42. Chicken & Sesame Oil"), text.index("43. Black Sesame"))
        self.assertLess(text.index("51. Crispy Soy Rice Cakes"), text.index("Rice & Savoury Porridge 25"))
        self.assertNotIn("立叫夸", text)

    def test_layout_aware_text_keeps_recipe_preamble_before_ingredient_column(self) -> None:
        raw = raw_from_lines(
            [
                ("Serves 6", 300, 0, 75),
                ("Short-grain Rice", 300, 20, 130),
                ("& Five-grain Rice", 300, 36, 150),
                ("bap & ogokbap", 300, 52, 115),
                ("Here we have included the two basic rice dishes,", 300, 76, 360),
                ("plain short-grain and five-grain rice.", 300, 92, 285),
                ("400g short-grain", 55, 120, 130),
                ("white rice", 55, 136, 70),
                ("Five-grain Rice", 55, 170, 120),
                ("75g aduki beans,", 55, 190, 135),
                ("soaked overnight", 55, 206, 130),
                ("For the Short-grain Rice: Rinse and drain the rice 3 times", 300, 130, 430),
                ("place it in a bowl, cover with cold water and leave to soak", 300, 146, 430),
                ("For the Five-grain Rice: If you have a rice cooker", 300, 220, 390),
                ("26 Rice & Savoury Porridge", 430, 300, 210),
            ]
        )

        text, layout = paddle_ocr._layout_aware_text(paddle_ocr._extract_line_items(raw))

        self.assertEqual(2, layout["columns"])
        self.assertLess(text.index("Short-grain Rice"), text.index("400g short-grain"))
        self.assertLess(text.index("400g short-grain"), text.index("For the Short-grain Rice"))
        self.assertLess(text.index("For the Five-grain Rice"), text.index("26 Rice & Savoury Porridge"))

    def test_text_extraction_falls_back_without_boxes(self) -> None:
        raw = {"rec_texts": ["Line one", "한글", "Line 한글 two", "カナ", "号人部"]}

        line_items = paddle_ocr._extract_line_items(raw)
        texts = paddle_ocr._extract_texts(raw)

        self.assertEqual([], line_items)
        self.assertEqual(["Line one", "Line two"], texts)

    def test_footer_page_number_uses_bottom_left_or_bottom_right_location(self) -> None:
        left_raw = raw_from_lines(
            [
                ("Index", 300, 20, 50),
                ("sweet rice cakes 256-7, 258-9", 80, 120, 220),
                ("270 Index", 40, 760, 80),
            ]
        )
        right_raw = raw_from_lines(
            [
                ("Index", 300, 20, 50),
                ("prawn & sweet potato tempura 150,151", 80, 140, 260),
                ("Index 271", 520, 760, 80),
            ]
        )

        self.assertEqual(
            (270, "left"),
            paddle_ocr._detect_printed_page_number(paddle_ocr._extract_line_items(left_raw)),
        )
        self.assertEqual(
            (271, "right"),
            paddle_ocr._detect_printed_page_number(paddle_ocr._extract_line_items(right_raw)),
        )

    def test_footer_page_number_ignores_body_index_references(self) -> None:
        raw = raw_from_lines(
            [
                ("Index", 300, 20, 50),
                ("sweet rice cakes 256-7, 258-9", 80, 120, 220),
                ("prawn & sweet potato tempura 150,151", 80, 140, 260),
            ]
        )
        lines = paddle_ocr._extract_line_items(raw)

        self.assertEqual((None, None), paddle_ocr._detect_printed_page_number(lines))

    def test_jsonable_prefers_dict_like_access_over_lossy_json_attr(self) -> None:
        class FakeArray:
            def tolist(self) -> list[list[int]]:
                return [[40, 760], [120, 760], [120, 772], [40, 772]]

        class FakePaddleResult:
            """paddlex OCRResult look-alike: .json stringifies arrays with
            `...` elision, but keys/__getitem__ expose live values."""

            _data = {
                "rec_texts": ["270 Index"],
                "rec_scores": [0.99],
                "rec_polys": [FakeArray()],
            }

            @property
            def json(self) -> dict[str, str]:
                return {"rec_polys": "[[ 40 760]\n ...\n [ 40 772]]"}

            def keys(self):
                return self._data.keys()

            def __getitem__(self, key: str):
                return self._data[key]

        raw = paddle_ocr._jsonable([FakePaddleResult()])
        lines = paddle_ocr._extract_line_items(raw)

        self.assertEqual(1, len(lines))
        self.assertEqual("270 Index", lines[0].text)
        self.assertEqual(40, lines[0].x_min)

    def test_column_gutter_detected_when_column_centers_are_close(self) -> None:
        # Two adjacent index columns whose center gap (~250px on a 620px page)
        # is real but whose gutter is narrow; the old center-gap heuristic
        # needed a much larger spread. Gutter: x in [300, 330].
        lines = []
        for row in range(10):
            y = 60 + row * 24
            lines.append((f"left entry {row} 12{row}", 40, y, 255))
            lines.append((f"right entry {row} 20{row}", 335, y, 250))
        raw = raw_from_lines(lines)

        text, layout = paddle_ocr._layout_aware_text(paddle_ocr._extract_line_items(raw))

        self.assertEqual("columns", layout["mode"])
        self.assertEqual(2, layout["columns"])
        # All left-column entries read before any right-column entry.
        self.assertLess(text.index("left entry 9"), text.index("right entry 0"))

    def test_edge_alignment_detects_columns_when_boxes_overlap_gutter(self) -> None:
        # Broad OCR boxes can overlap the visual gutter, so projection sees no
        # empty band. The repeated x_min values still reveal two left-aligned
        # columns with ragged right edges.
        lines = []
        for row in range(8):
            y = 60 + row * 24
            lines.append((f"ingredient line {row}", 40, y, 355 - (row % 3) * 18))
            lines.append((f"method line {row}", 330, y, 270 - (row % 4) * 15))
        raw = raw_from_lines(lines)

        text, layout = paddle_ocr._layout_aware_text(paddle_ocr._extract_line_items(raw))

        self.assertEqual("columns", layout["mode"])
        self.assertEqual("edge-alignment", layout["detection"])
        self.assertEqual(2, layout["columns"])
        self.assertLess(text.index("ingredient line 7"), text.index("method line 0"))

    def test_single_column_prose_is_not_split(self) -> None:
        lines = [
            (f"prose line {row} with words spread across the page", 40, 60 + row * 24, 520)
            for row in range(10)
        ]
        raw = raw_from_lines(lines)

        _, layout = paddle_ocr._layout_aware_text(paddle_ocr._extract_line_items(raw))

        self.assertEqual(1, layout["columns"])

    def test_bare_corner_page_number_validated_by_position(self) -> None:
        # Bare number in the bottom-left corner: accepted, side reported.
        corner_raw = raw_from_lines(
            [
                ("Suppliers", 300, 20, 90),
                ("www.asiangrocerystore.com.au", 80, 200, 240),
                ("265", 36, 760, 30),
            ]
        )
        # The same number centered in the footer (a fold artifact or design
        # element) is rejected; so is a bare number in the body. A wide header
        # line pins the body extents so "centered" is meaningful.
        centered_raw = raw_from_lines(
            [
                ("Where to buy Korean ingredients in the UK", 60, 20, 500),
                ("www.asiangrocerystore.com.au", 80, 200, 240),
                ("265", 300, 760, 30),
            ]
        )
        body_raw = raw_from_lines(
            [
                ("Suppliers", 300, 20, 90),
                ("265", 40, 300, 30),
                ("www.asiangrocerystore.com.au", 80, 760, 240),
            ]
        )

        self.assertEqual(
            (265, "left"),
            paddle_ocr._detect_printed_page_number(paddle_ocr._extract_line_items(corner_raw)),
        )
        self.assertEqual(
            (None, None),
            paddle_ocr._detect_printed_page_number(paddle_ocr._extract_line_items(centered_raw)),
        )
        self.assertEqual(
            (None, None),
            paddle_ocr._detect_printed_page_number(paddle_ocr._extract_line_items(body_raw)),
        )


if __name__ == "__main__":
    unittest.main()


class PruneRawTest(unittest.TestCase):
    def test_drops_preprocessor_tensors(self) -> None:
        raw = [
            {
                "rec_texts": ["CORN BREAD"],
                "rec_polys": [[[0, 0], [1, 0], [1, 1], [0, 1]]],
                "doc_preprocessor_res": {"output_img": [[0] * 32] * 32},
            }
        ]
        pruned = paddle_ocr._prune_raw(raw)
        self.assertNotIn("doc_preprocessor_res", pruned[0])
        self.assertEqual(pruned[0]["rec_texts"], ["CORN BREAD"])
        self.assertEqual(pruned[0]["rec_polys"], raw[0]["rec_polys"])

    def test_drops_nested_and_all_heavy_keys(self) -> None:
        raw = {"a": {"img": [1, 2, 3], "keep": 1}, "input_img": [9], "output_img": [9]}
        pruned = paddle_ocr._prune_raw(raw)
        self.assertEqual(pruned, {"a": {"keep": 1}})

    def test_leaves_scalars_alone(self) -> None:
        self.assertEqual(paddle_ocr._prune_raw("text"), "text")
        self.assertEqual(paddle_ocr._prune_raw(7), 7)
        self.assertIsNone(paddle_ocr._prune_raw(None))
