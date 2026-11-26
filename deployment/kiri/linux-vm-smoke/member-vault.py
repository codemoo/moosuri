#!/usr/bin/env python3
"""Linux real-process vault enrollment scenarios; stdlib only, synthetic accounts.
Usage: python3 member-vault.py /path/to/moosuri /path/to/moosuri-vault-service
Does not test Chrome, recipient enrollment, encryption UI, or a clean installer.
"""
import contextlib
import http.client
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import time
from uuid import uuid4

ASSISTANT = "https://assistant.example.test"
VAULT = "https://vault.example.test"


class UnixHTTP(http.client.HTTPConnection):
    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.host)


def directory(parent, name):
    path = parent / name
    path.mkdir(mode=0o700)
    return path


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def main(binary, vault_binary):
    assert sys.platform == "linux" and os.geteuid() != 0
    with tempfile.TemporaryDirectory(prefix="moosuri-member-vault-") as tmp, contextlib.ExitStack() as stack:
        root = Path(tmp).resolve()
        registry, ipc, gateway = (directory(root, n) for n in ("registry", "ipc", "gateway"))
        env = {"PATH": os.defpath, "TMPDIR": str(root)}

        def run(args, password=None):
            result = subprocess.run([str(a) for a in args], input=password, text=True, capture_output=True, env=env, timeout=30)
            assert result.returncode == 0, f"fixture command failed: {args[1]}"
            return json.loads(result.stdout) if result.stdout.startswith("{") else None

        def launch(args, ready):
            process = subprocess.Popen([str(a) for a in args], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            stack.callback(stop, process)
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline and process.poll() is None:
                try:
                    if ready():
                        return process
                except (OSError, http.client.HTTPException):
                    pass
                time.sleep(0.05)
            raise AssertionError(f"fixture service failed: {args[0]}")

        def request_http(conn, method, path, body=None, headers=None):
            with contextlib.closing(conn) as c:
                c.request(method, path, body, headers or {})
                response = c.getresponse()
                raw = response.read()
                cookies = [v.split(";", 1)[0] for k, v in response.getheaders() if k.lower() == "set-cookie"]
                return response.status, json.loads(raw) if raw.startswith(b"{") else None, "; ".join(cookies)

        account_by_cookie = {}

        def app(method, path, body=None, cookie=None):
            headers = {"Host": "assistant.example.test", "X-Real-IP": "192.0.2.1", "Sec-Fetch-Site": "same-origin"}
            if cookie:
                headers["Cookie"] = cookie
                headers["X-Moosuri-Account"] = account_by_cookie[cookie]
            if method == "POST":
                headers.update({"Origin": ASSISTANT, "Content-Type": "application/json"})
            result = request_http(UnixHTTP(str(gateway / "gateway.sock"), timeout=10), method, path,
                                  json.dumps(body) if body is not None else None, headers)
            if result[2] and result[1] and "user" in result[1]:
                account_by_cookie[result[2]] = result[1]["user"]["id"]
            return result

        def provision(label):
            email = label + "@example.test"
            allocation = run([binary, "account-runtime", "--registry", registry, "reserve", "--request-id", uuid4(), "--email", email])
            base = [binary, "personal-volume", "--root", directory(root, label), "--account", allocation["account_id"],
                    "--tenant", allocation["tenant_id"], "--volume", allocation["volume_id"], "--generation", allocation["generation"]]
            password = secrets.token_urlsafe(24)
            run([*base, "initialize", "--name", label, "--email", email, "--password-stdin"], password + "\n")
            sock = directory(ipc, allocation["account_id"]) / "personal.sock"
            launch([*base, "serve", "--socket", sock, "--vault-origin", VAULT], sock.is_socket)
            run([binary, "account-runtime", "--registry", registry, "activate", "--email", email, "--ipc-root", ipc])
            return allocation, email, password

        key = directory(root, "keys") / "adapter.key"
        key.write_text(secrets.token_hex(32))
        key.chmod(0o400)
        adapter_port, vault_port = port(), port()
        first = provision("first")
        command = [binary, "personal-gateway", "--registry", registry, "--ipc-root", ipc, "--socket-dir", gateway,
                   "--public-origin", ASSISTANT, "--vault-site-origin", VAULT, "--vault-all-members",
                   "--vault-adapter-listen", f"127.0.0.1:{adapter_port}", "--vault-adapter-key-file", key]
        gateway_process = launch(command, (gateway / "gateway.sock").is_socket)
        second = provision("second")  # No gateway reconfiguration/restart.

        def vault(method, path, body=None, cookie=None, account=None):
            headers = {"Host": "vault.example.test"}
            if cookie:
                headers["Cookie"] = cookie
            if account:
                headers["X-Moosuri-Vault-Account"] = account
            if method == "POST":
                headers.update({"Origin": ASSISTANT, "Content-Type": "application/x-www-form-urlencoded"})
            return request_http(http.client.HTTPConnection("127.0.0.1", vault_port, timeout=10), method, path, body, headers)

        launch([vault_binary, "--data-dir", root / "vault-data", "--bind", f"127.0.0.1:{vault_port}",
                "--origin", VAULT, "--assistant-origin", ASSISTANT, "--adapter-url", f"http://127.0.0.1:{adapter_port}/internal/v1/vault",
                "--adapter-key-file", key], lambda: vault("GET", "/health/live")[0] == 204)
        sessions = []
        for allocation, email, password in (first, second):
            status, _, cookie = app("POST", "/api/v1/auth/login", {"email": email, "password": password})
            assert status == 200
            status, _, changed_cookie = app("POST", "/api/v1/auth/password", {"password": secrets.token_urlsafe(24)}, cookie)
            assert status == 200, ("password change", status)
            cookie = changed_cookie or cookie
            assert app("GET", "/api/v1/personal/vault", cookie=cookie)[1]["state"] == "ready"
            assert app("POST", "/api/v1/personal/google/kiri", {"op": "start", "job": str(uuid4()), "service": "gmail"}, cookie)[0] == 403
            status, handoff, _ = app("POST", "/api/v1/personal/vault/handoff", {}, cookie)
            assert status == 200
            status, _, vault_cookie = vault("POST", "/session", "handoff=" + handoff["token"])
            assert status == 303 and vault_cookie
            assert vault("GET", "/session/current", cookie=vault_cookie)[1]["owner"]["account"] == allocation["account_id"]
            assert vault("POST", "/session", "handoff=" + handoff["token"])[0] == 401
            sessions.append((cookie, vault_cookie, allocation["account_id"]))
        print("PASS MV-01 two real member stores enroll, including a member added after gateway startup", flush=True)
        print("PASS MV-02 separate vault sessions resolve to their owners; handoffs are single-use", flush=True)
        for i, (_, vc, account) in enumerate(sessions):
            assert vault("GET", "/api/state", cookie=vc, account=account)[0] == 200
            assert vault("GET", "/api/state", cookie=vc, account=sessions[1-i][2])[0] == 401
        print("PASS MV-03 both directions reject another member's account selector", flush=True)
        cookie_a, vault_a, _ = sessions[0]
        _, vault_b, _ = sessions[1]
        assert app("POST", "/api/v1/auth/logout", {}, cookie_a)[0] == 204
        assert vault("GET", "/session/current", cookie=vault_a)[0] == 401
        assert vault("GET", "/session/current", cookie=vault_b)[0] == 200
        print("PASS MV-04 member logout revokes their vault grant while the other remains usable", flush=True)
        stop(gateway_process)
        # Gateway intentionally owns its socket cleanup; wait for a fresh listener.
        launch(command, lambda: app("GET", "/api/v1/auth/session", cookie=sessions[1][0])[0] == 200)
        assert vault("GET", "/session/current", cookie=vault_b)[0] == 401
        status, handoff, _ = app("POST", "/api/v1/personal/vault/handoff", {}, sessions[1][0])
        assert status == 200
        status, _, refreshed = vault("POST", "/session", "handoff=" + handoff["token"])
        assert status == 303
        assert vault("GET", "/session/current", cookie=refreshed)[1]["owner"]["account"] == sessions[1][2]
        print("PASS MV-05 gateway restart invalidates grants and fresh handoff restores the same member", flush=True)
        run([binary, "account-runtime", "--registry", registry, "fence", "--email", second[1]])
        assert vault("GET", "/session/current", cookie=refreshed)[0] == 401
        print("PASS MV-06 registry fencing immediately denies an existing vault session", flush=True)


if __name__ == "__main__":
    main(*sys.argv[1:])
