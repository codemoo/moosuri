"""Offline checks for the model-selection and receipt contract, with no inference."""

import json
import os
import subprocess
import unittest
from unittest.mock import patch

import claude_cli


def cli_result(data, code=0):
    return subprocess.CompletedProcess(["claude"], code, json.dumps(data), "")


class ClaudeEvaluationTests(unittest.TestCase):
    def setUp(self):
        self.receipt = {"is_error": False, "result": "fixture", "stop_reason": "end_turn",
                        "modelUsage": {claude_cli.MODEL: {"outputTokens": 1}},
                        "usage": {key: 1 for key in claude_cli.USAGE}}

    def test_pins_the_model_and_effort_for_every_call(self):
        with patch.dict(os.environ, {"ANTHROPIC_MODEL": "fixture-override",
                                     "ANTHROPIC_BASE_URL": "https://example.invalid"}), \
             patch("claude_cli.subprocess.run", return_value=cli_result(self.receipt)) as run:
            answer, _ = claude_cli.model(".", "synthetic question", 0.1)
        run.assert_called_once()
        command = run.call_args.args[0]
        self.assertEqual(command[command.index("--model") + 1], "claude-sonnet-5-5")
        self.assertEqual(command[command.index("--effort") + 1], "medium")
        self.assertNotIn("--fallback-model", command)
        self.assertNotIn("ANTHROPIC_BASE_URL", run.call_args.kwargs["env"])
        self.assertNotIn("ANTHROPIC_MODEL", run.call_args.kwargs["env"])
        self.assertEqual(answer, "fixture")

    def test_requires_a_completed_receipt_from_only_the_pinned_model(self):
        for change in ({"modelUsage": {}}, {"modelUsage": {"other-fixture-model": {}}},
                       {"modelUsage": {claude_cli.MODEL: {}, "other-fixture-model": {}}},
                       {"is_error": True}, {"result": ""}, {"stop_reason": "max_tokens"}):
            response = cli_result({**self.receipt, **change})
            with self.subTest(change=change), patch("claude_cli.subprocess.run", return_value=response) as run:
                with self.assertRaises(RuntimeError):
                    claude_cli.model(".", "synthetic question", 0.1)
                run.assert_called_once()

    def test_requires_an_authenticated_first_party_cli(self):
        for status in ({"loggedIn": False, "apiProvider": "firstParty"},
                       {"loggedIn": True, "apiProvider": "fixture"}):
            with self.subTest(status=status), patch("claude_cli.which", return_value="/fixture/claude"), \
                 patch("claude_cli.subprocess.run", return_value=cli_result(status)) as run:
                with self.assertRaises(RuntimeError):
                    claude_cli.require_login(".")
                self.assertEqual(run.call_args.args[0], ["claude", "auth", "status"])
                run.assert_called_once()


if __name__ == "__main__":
    unittest.main()
