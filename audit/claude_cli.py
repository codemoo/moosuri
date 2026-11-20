"""Official Claude CLI for the opt-in conversation and memory evaluations."""

import json
import os
from shutil import which
import subprocess
import time

MODEL = "claude-sonnet-5-5"
EFFORT = "medium"
USAGE = ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens")


def cli_environment():
    # Use the current first-party login. Omit model, endpoint and fallback overrides.
    keys = ("PATH", "HOME", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL",
            "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
            "NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE", "XDG_CONFIG_HOME", "XDG_CACHE_HOME",
            "CLAUDE_CONFIG_DIR", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY")
    return {**{key: os.environ[key] for key in keys if key in os.environ},
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1", "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS": "1",
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY": "1", "CLAUDE_CODE_MAX_RETRIES": "0",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "512"}


def require_login(cwd):
    if which("claude") is None:
        raise RuntimeError("Install the official Claude CLI and run 'claude auth login'.")
    try:
        status = subprocess.run(["claude", "auth", "status"], cwd=cwd, env=cli_environment(),
                                capture_output=True, text=True, timeout=15)
        account = json.loads(status.stdout)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        raise RuntimeError("Could not verify the current Claude CLI login.") from None
    if (status.returncode or not isinstance(account, dict) or account.get("loggedIn") is not True
            or account.get("apiProvider") != "firstParty"):
        raise RuntimeError("Claude login required. Run 'claude auth login' for the evaluation account.")


def model(cwd, prompt, budget, system="Use only the supplied synthetic evidence and follow the requested output format."):
    args = [
        "claude", "-p", "--model", MODEL, "--effort", EFFORT, "--no-session-persistence",
        "--tools", "", "--disable-slash-commands", "--mcp-config", '{"mcpServers":{}}',
        "--strict-mcp-config", "--setting-sources", "", "--permission-mode", "dontAsk",
        "--system-prompt", system, "--output-format", "json", "--max-budget-usd", str(budget),
    ]
    start = time.monotonic_ns()
    try:
        proc = subprocess.run(args, cwd=cwd, env=cli_environment(), input=prompt,
                              capture_output=True, text=True, timeout=110)
        elapsed_ms = (time.monotonic_ns() - start) // 1_000_000
        if proc.returncode:
            raise RuntimeError(f"Claude exited with code {proc.returncode}.")
        data = json.loads(proc.stdout)
        if (data["is_error"] is not False or not isinstance(data["modelUsage"], dict)
                or set(data["modelUsage"]) != {MODEL}
                or not isinstance(data["result"], str) or not data["result"].strip()
                or data.get("stop_reason") not in (None, "end_turn", "stop_sequence")):
            raise RuntimeError(f"Claude did not report a completed answer from {MODEL}.")
        metrics = {k: data["usage"][k] for k in USAGE}
        if any(type(value) is not int or value < 0 for value in metrics.values()) or not metrics["output_tokens"]:
            raise ValueError("invalid usage")
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired):
        raise RuntimeError("Claude did not return a valid result and usage receipt.") from None
    return data["result"].strip(), dict(metrics, wall_ms=elapsed_ms)
