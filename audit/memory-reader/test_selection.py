"""Verify the accepted wrapper without weakening the synthetic turn schema."""

import unittest

from run import WORLD, parse_selection


class SelectionTests(unittest.TestCase):
    def test_accepts_the_same_turn_numbers_with_or_without_one_code_fence(self):
        self.assertEqual(parse_selection("[1, 901]"), ([1, 901], "json"))
        self.assertEqual(parse_selection("```json\n[1, 901]\n```"), ([1, 901], "fenced_json"))

    def test_rejects_commentary_and_invalid_turns(self):
        for value in ("Prefix [1]", "```json\n[1]\n```\nsuffix", "[1, 1]", "[true]",
                      "[0]", str([WORLD["turns"] + 1]), str(list(range(1, 14)))):
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                parse_selection(value)


if __name__ == "__main__":
    unittest.main()
