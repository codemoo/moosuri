"""Pure harness regressions: no Docker/Linux/browser launch or network calls."""
import ipaddress
import subprocess
import sys
import unittest
from unittest.mock import Mock, patch

import smoke


class HarnessTests(unittest.TestCase):
    def test_subnet_is_private_canonical_and_collision_checked(self):
        for invalid in ["1.1.1.0/24", "127.0.0.0/24", "169.254.0.0/24", "10.0.0.0/8", "10.1.2.3/24"]:
            with self.assertRaises(ValueError):
                smoke.private_subnet(invalid)
        occupied = [ipaddress.IPv4Network("10.42.0.0/16")]
        with self.assertRaises(ValueError):
            smoke.choose_subnet("10.42.17.0/24", occupied)
        self.assertEqual(str(smoke.choose_subnet("10.43.17.0/24", occupied)), "10.43.17.0/24")

    def test_ownership_mismatch_blocks_destructive_cleanup(self):
        harness = smoke.Harness()
        self.addCleanup(harness.config_dir.cleanup)
        harness.workers = []
        harness.resource_ids = Mock(return_value=["foreign"])
        harness.run = Mock()
        with patch.object(smoke, "run_output", return_value='[{"Config":{"Labels":{"com.docker.compose.project":"other"}}}]') as command:
            errors = harness.cleanup()
        self.assertTrue(errors)
        harness.run.assert_not_called()
        self.assertTrue(all("inspect" in call.args[0] for call in command.call_args_list))

    def test_worker_stop_and_compose_failure_do_not_skip_owned_cleanup(self):
        harness = smoke.Harness()
        self.addCleanup(harness.config_dir.cleanup)
        first, second = Mock(), Mock()
        first.stop.side_effect = RuntimeError("synthetic failed stop")
        harness.workers = [first, second]
        harness.resource_ids = Mock(side_effect=lambda kind: ["owned-" + kind])
        harness.verify_owned = Mock()
        harness.run = Mock(side_effect=subprocess.TimeoutExpired("compose down", 30))
        with patch.object(smoke, "run_output", return_value="") as command:
            errors = harness.cleanup()
        self.assertEqual(len(errors), 2)
        first.stop.assert_called_once()
        second.stop.assert_called_once()
        self.assertEqual([c.args[0] for c in command.call_args_list], [
            harness.docker + ["rm", "-f", "owned-container"],
            harness.docker + ["network", "rm", "owned-network"],
        ])

    def test_engine_selection_is_explicit_and_kiri_needs_a_pinned_image(self):
        with self.assertRaises(ValueError):
            smoke.Harness("kiri")
        with self.assertRaises(ValueError):
            smoke.Harness("tcp://example.test:2375")
        with patch.dict(smoke.os.environ, {"DOCKER_HOST":"tcp://example.test:2375", "DOCKER_CONTEXT":"foreign", "COMPOSE_FILE":"foreign.yaml"}):
            harness = smoke.Harness("kiri", "sha256:" + "a" * 64)
        self.addCleanup(harness.config_dir.cleanup)
        self.assertEqual(harness.docker[:2], ["/usr/bin/docker", "--host=unix:///run/kiri/docker.sock"])
        self.assertEqual(harness.docker[2], "--config=" + harness.config_dir.name)
        self.assertNotIn("DOCKER_HOST", harness.env)
        self.assertNotIn("DOCKER_CONTEXT", harness.env)
        self.assertNotIn("COMPOSE_FILE", harness.env)
        self.assertIn("/dev/null", harness.compose)
        self.assertEqual(harness.env["DOCKER_API_VERSION"], "1.47")

    def test_worker_stdout_missing_newline_is_bounded_and_deadline_enforced(self):
        p = subprocess.Popen([sys.executable, "-c", "import os,time;os.write(1,b'x'*100);time.sleep(2)"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        try:
            with patch.object(smoke, "MAX_LINE", 32):
                with self.assertRaisesRegex(RuntimeError, "exceeds bound"):
                    smoke.JsonPipe(p).read(0.5)
        finally:
            p.kill();p.wait(timeout=2);p.stdin.close();p.stdout.close()
        p = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(2)"], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        try:
            with self.assertRaises(TimeoutError):
                smoke.JsonPipe(p).read(0.02)
        finally:
            p.kill();p.wait(timeout=2);p.stdin.close();p.stdout.close()


if __name__ == "__main__":
    unittest.main()
