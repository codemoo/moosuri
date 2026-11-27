#!/usr/bin/env python3
"""Linux process journeys using a synthetic engine, never provider credentials.

Run as a non-root user with --binary <built moosuri>. This exercises the actual
private-config reader, CLI dispatch, pipes, catalog and process exit. It does not
prove official login, container isolation, provisioning or the Kiri UI.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import selectors
import socket
import subprocess
import tempfile
import time
import uuid


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Journey:
    def __init__(self, binary, behavior="ok", additional=None):
        self.temp = tempfile.TemporaryDirectory(prefix="moosuri-bridge-process-")
        self.root = Path(self.temp.name).resolve()
        self.binary = binary
        self.child = None
        self.pending = bytearray()
        for name in ("auth", "scratch", "socket"):
            (self.root / name).mkdir(mode=0o700)
        fixture = Path(__file__).resolve().parents[2] / "crates/moosuri-connectors/src/claude_code/fixture_engine.py"
        engine = self.root / "engine"
        engine.write_text(fixture.read_text().replace("FIXTURE_ROOT", repr(str(self.root))))
        engine.chmod(0o500)
        for name in ("official", "helper"):
            p = self.root / name
            p.write_bytes(b"synthetic executable placeholder")
            p.chmod(0o500)
        self.listener = socket.socket(socket.AF_UNIX)
        self.listener.bind(str(self.root / "engine.sock"))
        # Keep the production profile's serialization order for its digest.
        profile = dict(
            engine=str(engine), engine_sha256=sha(engine),
            engine_socket=str(self.root / "engine.sock"),
            image="example.invalid/synthetic@sha256:" + "a" * 64,
            official_cli=str(self.root / "official"), cli_sha256=sha(self.root / "official"),
            cli_version="2.1.322", helper=str(self.root / "helper"),
            helper_sha256=sha(self.root / "helper"),
            auth_root=str(self.root / "auth"), scratch_root=str(self.root / "scratch"),
            isolation_acceptance_sha256="b" * 64, hosting_terms_accepted=True,
        )
        packed = json.dumps(profile, separators=(",", ":"))
        self.private("profile.json", packed)
        self.private("email", "owner@example.test\n")
        self.config = dict(
            version=1, socket_directory=str(self.root / "socket"),
            owner={**{k: str(uuid.uuid4()) for k in ("account_id", "tenant_id", "volume_id")}, "generation": 1},
            connection_revision=str(uuid.uuid4()), model="claude-fixture-v1", effort="medium",
            models=additional or [],
            adapter={"type": "claude_code", "settings": {
                "profile_file": str(self.root / "profile.json"),
                "profile_digest": hashlib.sha256(packed.encode()).hexdigest(),
                "environment_id": str(uuid.uuid4()), "expected_email_file": str(self.root / "email"),
            }},
        )
        self.private("worker.json", json.dumps(self.config))
        self.private("behavior", behavior)

    def private(self, name, value):
        p = self.root / name
        p.write_text(value)
        p.chmod(0o600)

    def start(self, output_file=False):
        self.output_file = open(self.root / "rejected-output", "wb") if output_file else None
        self.child = subprocess.Popen(
            [self.binary, "claude-engine-login-bridge", "--config", str(self.root / "worker.json")],
            stdin=subprocess.PIPE, stdout=self.output_file or subprocess.PIPE,
            stderr=subprocess.PIPE, bufsize=0,
        )

    def event(self):
        deadline = time.monotonic() + 45
        with selectors.DefaultSelector() as poll:
            poll.register(self.child.stdout, selectors.EVENT_READ)
            while b"\n" not in self.pending:
                assert poll.select(max(0, deadline - time.monotonic())), "event_deadline"
                chunk = os.read(self.child.stdout.fileno(), 8192)
                assert chunk, "unexpected_protocol_eof"
                self.pending.extend(chunk)
                assert len(self.pending) <= 16384, "output_limit"
            line, _, remaining = self.pending.partition(b"\n")
            self.pending = remaining
            return json.loads(line)

    def send(self, value):
        self.child.stdin.write(value)
        self.child.stdin.flush()

    def finish(self, result=None, error=None):
        if result is not None:
            assert self.event() == {"event": "result", "result": result}, "wrong_result"
        # Intentionally leave the parent's stdin open: successful process exit
        # must not depend on closing Tokio's blocking stdin reader.
        code = self.child.wait(timeout=15)
        assert (code == 0) == (result == "ready"), "wrong_exit"
        stderr = self.child.stderr.read(1024)
        expected = error or ("claude_login_" + result if result != "ready" else None)
        assert stderr == (f"moosuri: {expected}\n".encode() if expected else b""), "unbounded_stderr"
        if self.child.stdout:
            assert not self.pending and self.child.stdout.read(1) == b"", "extra_protocol_output"
        for forbidden in ("bound-user", "usage-command", "runtime-model.json", "socket/engine.sock"):
            assert not (self.root / forbidden).exists(), "unexpected_inference_or_registration"
        self.child.stdin.close()

    def close(self):
        if self.child:
            if self.child.poll() is None:
                self.child.kill()
                self.child.wait(timeout=5)
            for channel in (self.child.stdin, self.child.stdout, self.child.stderr):
                if channel:
                    channel.close()
        if getattr(self, "output_file", None):
            self.output_file.close()
        self.listener.close()
        self.temp.cleanup()


def run(binary):
    cases = ("existing_ready", "fresh_login", "cancel", "parent_eof", "partial_eof",
             "duplicate_code", "additional_model", "invalid_generation", "config_permissions",
             "profile_revision", "file_output", "cancel_preflight_lock")
    for name in cases:
        needs_login = name in {"fresh_login", "cancel", "parent_eof", "partial_eof", "duplicate_code"}
        extra = [{"model": "claude-unverified", "effort": "medium"}] if name == "additional_model" else None
        j = Journey(binary, "bridge-login" if needs_login else "ok", extra)
        lock = None
        try:
            if name == "invalid_generation":
                j.config["owner"]["generation"] = 0
                j.private("worker.json", json.dumps(j.config))
            if name == "profile_revision":
                j.config["adapter"]["settings"]["profile_digest"] = "0" * 64
                j.private("worker.json", json.dumps(j.config))
            if name == "config_permissions":
                (j.root / "worker.json").chmod(0o640)
            if name == "cancel_preflight_lock":
                environment = j.config["adapter"]["settings"]["environment_id"]
                lock = open(j.root / "auth" / (environment + ".lock"), "wb")
                os.fchmod(lock.fileno(), 0o600)
                fcntl.flock(lock, fcntl.LOCK_EX)
            j.start(output_file=name == "file_output")
            if name == "cancel_preflight_lock":
                started = time.monotonic()
                j.send(b'{"command":"cancel"}\n')
                j.finish(result="cancelled")
                assert time.monotonic() - started < 3, "cancel_waited_for_lock"
                assert not (j.root / "launches").exists()
            elif name in {"invalid_generation", "config_permissions"}:
                j.finish(error="engine_config_invalid")
            elif name == "profile_revision":
                j.finish(error="claude_worker_profile_changed")
            elif name == "file_output":
                j.finish(error="claude_login_private_pipe_required")
                assert (j.root / "rejected-output").stat().st_size == 0
            elif needs_login:
                event = j.event()
                assert event["event"] == "authorization_url"
                assert event["url"].startswith("https://claude.com/cai/oauth/authorize?")
                if name == "fresh_login":
                    j.send(b'{"command":"code","code":"synthetic-code#state"}\n')
                    outcome = "ready"
                elif name == "cancel":
                    j.send(b'{"command":"cancel"}\n')
                    outcome = "cancelled"
                elif name == "duplicate_code":
                    j.send(b'{"command":"code","code":"synthetic-code#state"}\n' * 2)
                    outcome = "protocol_error"
                else:
                    if name == "partial_eof":
                        j.send(b'{"command":"code","code":"unfinished')
                    j.child.stdin.close()
                    outcome = "cancelled"
                j.finish(result=outcome)
                assert (j.root / "removed").exists(), "cleanup_missing"
            else:
                j.finish(result="model_unavailable" if name == "additional_model" else "ready")
            print(json.dumps({"journey": name, "result": "passed"}), flush=True)
        finally:
            if lock:
                lock.close()
            j.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    args = parser.parse_args()
    assert os.geteuid() != 0 and os.uname().sysname == "Linux", "nonroot_linux_required"
    run(str(args.binary.resolve(strict=True)))
