"""Owned-project Linux Docker smoke test. Never modifies the host firewall.

This module is import-safe for pure harness unit tests. Docker runs only in main.
"""
import base64
import ipaddress
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import time
import uuid

HERE = Path(__file__).resolve().parent
OWNER_LABEL = "io.moosuri.browser.owner"
PROJECT_LABEL = "com.docker.compose.project"
MAX_LINE = 4 * 1024 * 1024


def private_subnet(raw):
    network = ipaddress.IPv4Network(raw, strict=True)
    rfc1918 = [ipaddress.IPv4Network(c) for c in ("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16")]
    if not 24 <= network.prefixlen <= 28 or not any(network.subnet_of(n) for n in rfc1918):
        raise ValueError("test subnet must be canonical RFC1918 IPv4 /24 through /28")
    return network


def choose_subnet(requested, occupied):
    if requested:
        candidates = [private_subnet(requested)]
    else:
        candidates = [private_subnet(f"10.{uuid.uuid4().int % 254 + 1}.{uuid.uuid4().int % 254 + 1}.0/24") for _ in range(128)]
    for network in candidates:
        if not any(network.overlaps(other) for other in occupied):
            return network
    raise ValueError("no collision-free test subnet; configure MOOSURI_BROWSER_SUBNET explicitly")


def run_output(args, env, timeout=30):
    result = subprocess.run(args, env=env, text=True, capture_output=True, timeout=timeout, check=True)
    if len(result.stdout) > 16 * 1024 * 1024:
        raise RuntimeError("Docker inspection output exceeds bound")
    return result.stdout


class JsonPipe:
    """Bounded nonblocking request/response IO, including missing newline/EOF."""
    def __init__(self, process):
        self.process = process
        self.pending = bytearray()
        os.set_blocking(process.stdout.fileno(), False)
        os.set_blocking(process.stdin.fileno(), False)

    def read(self, timeout):
        deadline = time.monotonic() + timeout
        with selectors.DefaultSelector() as events:
            events.register(self.process.stdout, selectors.EVENT_READ)
            while True:
                newline = self.pending.find(b"\n")
                if newline >= 0:
                    if newline > MAX_LINE:
                        raise RuntimeError("worker response exceeds bound")
                    line = bytes(self.pending[:newline])
                    del self.pending[:newline + 1]
                    return json.loads(line)
                if len(self.pending) > MAX_LINE:
                    raise RuntimeError("worker response exceeds bound")
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not events.select(remaining):
                    raise TimeoutError("worker response deadline exceeded")
                chunk = os.read(self.process.stdout.fileno(), min(65536, MAX_LINE + 1 - len(self.pending)))
                if not chunk:
                    raise RuntimeError("worker closed stdout before a complete response")
                self.pending.extend(chunk)

    def write(self, data, timeout):
        raw = json.dumps(data).encode() + b"\n"
        if len(raw) > 16 * 1024:
            raise ValueError("worker command exceeds bound")
        deadline = time.monotonic() + timeout
        with selectors.DefaultSelector() as events:
            events.register(self.process.stdin, selectors.EVENT_WRITE)
            offset = 0
            while offset < len(raw):
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not events.select(remaining):
                    raise TimeoutError("worker input deadline exceeded")
                offset += os.write(self.process.stdin.fileno(), raw[offset:])


class Harness:
    def __init__(self):
        self.owner = uuid.uuid4().hex
        self.project = "moosuri-browser-smoke-" + self.owner
        self.env = dict(os.environ, MOOSURI_BROWSER_OWNER=self.owner,
                        MOOSURI_BROWSER_TENANT=str(uuid.uuid4()), MOOSURI_BROWSER_JAVASCRIPT="enabled")
        self.compose = ["docker", "compose", "-p", self.project, "-f", str(HERE / "compose.yaml"), "-f", str(HERE / "compose.test.yaml")]
        self.workers = []

    def run(self, *args, timeout=60):
        return run_output(self.compose + list(args), self.env, timeout)

    def resource_ids(self, kind):
        command = ["docker", "ps", "-aq"] if kind == "container" else ["docker", "network", "ls", "-q"]
        return run_output(command + ["--filter", f"label={PROJECT_LABEL}={self.project}"], self.env).split()

    def verify_owned(self, kind, ids):
        if not ids:
            return
        rows = json.loads(run_output(["docker", kind, "inspect", *ids], self.env))
        for row in rows:
            labels = row.get("Config", {}).get("Labels", {}) if kind == "container" else row.get("Labels", {})
            if labels.get(PROJECT_LABEL) != self.project or labels.get(OWNER_LABEL) != self.owner:
                raise RuntimeError("ownership label mismatch; refusing cleanup or exec")

    def exec_owned(self, container, *args):
        self.verify_owned("container", [container])
        return run_output(["docker", "exec", container, *args], self.env)

    def proxy_exec(self, code):
        ids = self.run("ps", "-q", "proxy").split()
        if len(ids) != 1:
            raise RuntimeError("expected one owned proxy")
        return self.exec_owned(ids[0], "python3", "-c", code)

    def fixture_get(self, path):
        # Literal control paths only, hosted on the owned proxy loopback alias.
        assert path in {"counts", "rebind", "reset-counts", "positive-control"}
        return self.proxy_exec("import urllib.request;u=urllib.request.build_opener(urllib.request.ProxyHandler({}));print(u.open('http://93.184.216.34/" + path + "',timeout=2).read(4096).decode())")

    def configure(self):
        if self.resource_ids("container") or self.resource_ids("network"):
            raise RuntimeError("generated project name already exists")
        network_ids = run_output(["docker", "network", "ls", "-q"], self.env).split()
        occupied = []
        if network_ids:
            for row in json.loads(run_output(["docker", "network", "inspect", *network_ids], self.env)):
                for config in row.get("IPAM", {}).get("Config", []) or []:
                    if config.get("Subnet"):
                        network = ipaddress.ip_network(config["Subnet"], strict=False)
                        if network.version == 4:
                            occupied.append(network)
        # Read-only route inventory prevents choosing a CIDR used by host/VPN
        # routes. Default routes are not an address allocation; all others count.
        for route in json.loads(run_output(["ip", "-j", "-4", "route", "show", "table", "all"], self.env)):
            dst = route.get("dst")
            if dst and dst != "default":
                occupied.append(ipaddress.IPv4Network(dst, strict=False))
        network = choose_subnet(os.environ.get("MOOSURI_BROWSER_SUBNET"), occupied)
        proxy = str(network.network_address + 2)
        configured = os.environ.get("MOOSURI_BROWSER_PROXY_IP")
        if configured and configured != proxy:
            raise ValueError("smoke proxy IP must equal the selected subnet's second host")
        self.env.update(MOOSURI_BROWSER_SUBNET=str(network), MOOSURI_BROWSER_PROXY_IP=proxy)

    def cleanup(self):
        errors = []
        # One failed stop must never skip another worker or owned Docker cleanup.
        for worker in self.workers:
            try:
                worker.stop()
            except BaseException as error:
                errors.append(error)
        try:
            self.verify_owned("container", self.resource_ids("container"))
            self.verify_owned("network", self.resource_ids("network"))
        except BaseException as error:
            errors.append(error)
            return errors  # A mismatch must never authorize destructive cleanup.
        try:
            # No fixed project and no --remove-orphans.
            self.run("down", "--timeout", "5", timeout=30)
        except BaseException as error:
            errors.append(error)
        # A failed Compose down still permits cleanup of individually rechecked
        # owned resources. Failure of one worker/container never skips the rest.
        for kind, command in (("container", ["docker", "rm", "-f"]),
                              ("network", ["docker", "network", "rm"])):
            try:
                ids = self.resource_ids(kind)
            except BaseException as error:
                errors.append(error)
                continue
            for resource in ids:
                try:
                    self.verify_owned(kind, [resource])
                    run_output(command + [resource], self.env)
                except BaseException as error:
                    errors.append(error)
        return errors


class Worker:
    def __init__(self, harness):
        self.harness = harness
        self.name = harness.project + "-worker-" + uuid.uuid4().hex[:12]
        self.p = subprocess.Popen(harness.compose + ["run", "--rm", "-T", "--no-deps", "--name", self.name, "browser"],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=harness.env)
        harness.workers.append(self)  # ready failure is still cleaned up
        self.pipe = JsonPipe(self.p)
        ready = self.pipe.read(45)
        assert ready.get("ready") and ready.get("javascript_enabled"), ready
        harness.verify_owned("container", [self.name])
        assert self.loopback_hits() == 0, "loopback positive control did not reset"

    def ask(self, data):
        self.pipe.write(data, 3)
        return self.pipe.read(25)

    def loopback_hits(self):
        output = self.harness.exec_owned(self.name, "python3", "-c",
            "import json,urllib.request;u=urllib.request.build_opener(urllib.request.ProxyHandler({}));print(u.open('http://127.0.0.1:8081/counts',timeout=2).read(1024).decode())")
        return json.loads(output)["hits"]

    def stop(self):
        try:
            if self.p.poll() is None:
                self.pipe.write({"op": "shutdown"}, 2)
                self.p.wait(timeout=20)
        finally:
            if self.p.poll() is None:
                self.p.terminate()
                try:
                    self.p.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    self.p.kill()
                    self.p.wait(timeout=3)
            self.p.stdin.close()
            self.p.stdout.close()


def check_page(reply):
    assert reply.get("ok"), reply
    result = reply["result"]
    assert "SCRIPT_RENDERED_OK" in result["text"], result
    assert "not refundable" in result["text"] and "PRIVATE_CANARY" not in result["text"], result
    return result


def main():
    harness = Harness()
    configured = False
    try:
        harness.configure()
        configured = True
        # Build output is inherited (not buffered without a bound by this script).
        subprocess.run(harness.compose + ["build"], env=harness.env, check=True, timeout=1200)
        harness.run("up", "-d", "proxy")
        for attempt in range(40):
            try:
                harness.fixture_get("positive-control")
                break
            except (OSError, subprocess.SubprocessError):
                if attempt == 39:
                    raise
                time.sleep(0.1)
        # Positive controls prove both private listeners are live before deny tests.
        proxy = harness.env["MOOSURI_BROWSER_PROXY_IP"]
        harness.proxy_exec("import urllib.request;u=urllib.request.build_opener(urllib.request.ProxyHandler({}));assert u.open('http://" + proxy + ":8081/positive',timeout=2).read(128)==b'PRIVATE_CANARY_SHOULD_NEVER_BE_READ';assert u.open('http://" + proxy + "/positive',timeout=2).read(128)==b'PRIVATE_CANARY_SHOULD_NEVER_BE_READ'")
        harness.fixture_get("reset-counts")
        probe = json.loads(harness.run("run", "--rm", "-T", "--no-deps", "browser", "--network-probe").strip())
        assert probe == {"direct_egress_blocked": True, "proxy_reachable": True}, probe
        first = Worker(harness)
        result = check_page(first.ask({"op": "open", "url": "http://93.184.216.34/"}))
        handle = result["handle"]
        screenshot = first.ask({"op": "screenshot", "handle": handle})
        assert screenshot["ok"], screenshot
        assert base64.b64decode(screenshot["result"]["base64"]).startswith(b"\x89PNG\r\n\x1a\n")
        second = Worker(harness)
        assert not second.ask({"op": "read", "handle": handle})["ok"], "cross-worker handle accepted"
        refreshed = first.ask({"op": "open", "url": "http://93.184.216.34/refresh"})
        assert refreshed["ok"], refreshed
        time.sleep(2)
        stale = first.ask({"op": "read", "handle": refreshed["result"]["handle"]})
        assert not stale["ok"] and "changed" in stale["error"], stale
        # Fresh workers avoid a created-tab capacity error masking the rebind test.
        domain_worker = Worker(harness)
        check_page(domain_worker.ask({"op": "open", "url": "http://fixture.example.com/"}))
        before = json.loads(harness.fixture_get("counts"))["dns_private"]
        harness.fixture_get("rebind")
        rebound_worker = Worker(harness)
        rebound = rebound_worker.ask({"op": "open", "url": "http://fixture.example.com/"})
        assert "limit" not in rebound.get("error", ""), rebound
        if rebound.get("ok"):
            assert "PRIVATE_CANARY" not in rebound["result"]["text"], rebound
            assert "SCRIPT_RENDERED_OK" not in rebound["result"]["text"], rebound
        after = json.loads(harness.fixture_get("counts"))
        assert after["dns_private"] > before, "rebind did not reach the proxy DNS policy"
        redirected = second.ask({"op": "open", "url": "http://93.184.216.34/redirect"})
        assert not redirected["ok"], redirected
        assert json.loads(harness.fixture_get("counts"))["canary"] == 0
        for worker in harness.workers:
            assert worker.loopback_hits() == 0, "browser request bypassed proxy to loopback canary"
        assert first.ask({"op": "close", "handle": handle})["ok"]
        assert not first.ask({"op": "read", "handle": handle})["ok"]
        print("PASS: JS/negation/PNG, scoped and stale handles, owned live canaries, proxy DNS rebind, direct-egress deny")
    finally:
        errors = harness.cleanup() if configured else []
        for error in errors:
            print(f"owned smoke cleanup error: {error}", file=sys.stderr)
        if errors and sys.exc_info()[0] is None:
            raise RuntimeError("owned smoke cleanup incomplete")


if __name__ == "__main__":
    main()
