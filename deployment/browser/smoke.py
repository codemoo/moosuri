"""Owned-project Linux Docker smoke test. Never modifies the host firewall.

This module is import-safe for pure harness unit tests. Docker runs only in main.
"""
import argparse
import base64
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import selectors
import re
import tempfile
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
    try:
        result = subprocess.run(args, env=env, text=True, capture_output=True, timeout=timeout, check=True)
    except subprocess.CalledProcessError as error:
        # This harness contains synthetic data only. Preserve bounded daemon
        # diagnostics so an unsupported engine option is not mistaken for a wait.
        print("synthetic engine command failed: " + error.stderr[-4096:], file=sys.stderr)
        raise
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
    def __init__(self, engine="docker", image=None):
        endpoints = {"docker": "unix:///var/run/docker.sock", "kiri": "unix:///run/kiri/docker.sock"}
        if engine not in endpoints or (image is not None and not re.fullmatch(r"sha256:[0-9a-f]{64}", image)):
            raise ValueError("select a local engine and an exact prebuilt image ID")
        if engine == "kiri" and image is None:
            raise ValueError("build the test image and load it into Kiri before this smoke")
        self.engine, self.image = engine, image
        self.config_dir = tempfile.TemporaryDirectory(prefix="moosuri-browser-engine-")
        self.docker = ["/usr/bin/docker", "--host=" + endpoints[engine], "--config=" + self.config_dir.name]
        self.override = None
        self.owner = str(uuid.uuid4())
        self.project = "moosuri-browser-smoke-" + self.owner
        self.env = dict({k:v for k,v in os.environ.items() if not k.startswith(("DOCKER_", "COMPOSE_"))}, MOOSURI_BROWSER_OWNER=self.owner,
                        MOOSURI_BROWSER_TENANT=str(uuid.uuid4()), MOOSURI_BROWSER_JAVASCRIPT="enabled")
        if engine == "kiri":
            self.env["DOCKER_API_VERSION"] = "1.47"
        self.compose = self.docker + ["compose", "--env-file", "/dev/null", "-p", self.project, "-f", str(HERE / "compose.yaml"), "-f", str(HERE / "compose.test.yaml")]
        self.workers = []
        self.volumes = []

    def profile_volume(self):
        name = self.project + "-profile-" + uuid.uuid4().hex
        run_output(self.docker + [ "volume", "create", "--label", f"{OWNER_LABEL}={self.owner}", "--label", f"{PROJECT_LABEL}={self.project}", name], self.env)
        self.volumes.append(name)
        self.verify_owned("volume", [name])
        return name

    def run(self, *args, timeout=60):
        return run_output(self.compose + list(args), self.env, timeout)

    def resource_ids(self, kind):
        command = self.docker + [ "ps", "-aq"] if kind == "container" else self.docker + [ "network", "ls", "-q"]
        return run_output(command + ["--filter", f"label={PROJECT_LABEL}={self.project}"], self.env).split()

    def verify_owned(self, kind, ids):
        if not ids:
            return
        rows = json.loads(run_output(self.docker + [ kind, "inspect", *ids], self.env))
        for row in rows:
            labels = row.get("Config", {}).get("Labels", {}) if kind == "container" else row.get("Labels", {})
            if labels.get(PROJECT_LABEL) != self.project or labels.get(OWNER_LABEL) != self.owner:
                raise RuntimeError("ownership label mismatch; refusing cleanup or exec")

    def exec_owned(self, container, *args, user=None):
        self.verify_owned("container", [container])
        options = [] if user is None else ["--user", user]
        return run_output(self.docker + [ "exec", *options, container, *args], self.env)

    def proxy_exec(self, code):
        ids = self.run("ps", "-q", "proxy").split()
        if len(ids) != 1:
            raise RuntimeError("expected one owned proxy")
        return self.exec_owned(ids[0], "python3", "-c", code)

    def fixture_get(self, path):
        # Literal control paths only, hosted on the owned proxy loopback alias.
        assert path in {"counts", "rebind", "reset-counts", "positive-control"}
        return self.proxy_exec("import urllib.request;u=urllib.request.build_opener(urllib.request.ProxyHandler({}));print(u.open('http://93.184.216.34/" + path + "',timeout=2).read(4096).decode())")

    def check_launcher(self, binary):
        if not Path(binary).is_absolute():
            raise ValueError("launcher binary path must be absolute")
        ids = self.run("ps", "-q", "proxy").split()
        assert len(ids) == 1
        self.verify_owned("container", ids)
        proxy = json.loads(run_output(self.docker + ["inspect", ids[0]], self.env))[0]
        network = next(n for n in proxy["NetworkSettings"]["Networks"].values()
                       if n["IPAddress"] == self.env["MOOSURI_BROWSER_PROXY_IP"])
        self.verify_owned("network", [network["NetworkID"]])
        seccomp = Path(self.config_dir.name) / "seccomp.json"
        seccomp.write_bytes((HERE / "seccomp.json").read_bytes())
        seccomp.chmod(0o600)
        config = dict(engine=self.engine, image_id=proxy["Image"], network_id=network["NetworkID"],
                      proxy_container_id=proxy["Id"], proxy_ip=self.env["MOOSURI_BROWSER_PROXY_IP"],
                      owner=self.owner, project=self.project, seccomp_file=str(seccomp),
                      verified=True, smoke_evidence_sha256=hashlib.sha256(self.engine.encode() + proxy["Image"].encode() + Path(__file__).read_bytes()).hexdigest(),
                      javascript_enabled=True)
        subprocess.run([binary], input=json.dumps(config), text=True, env=self.env, check=True, timeout=180)
        # Real launcher cleanup must leave only the harness's original proxy.
        remaining = self.resource_ids("container")
        assert len(remaining) == 1 and proxy["Id"].startswith(remaining[0]), "launcher left a worker behind"

    def configure(self):
        if self.engine == "kiri":
            version = json.loads(run_output(self.docker + ["version", "--format", "{{json .Server}}"], self.env))
            components = [p for p in version.get("Components", []) if p.get("Name") == "Kiri"]
            if version.get("Platform", {}).get("Name") != "Kiri" or len(components) != 1 or components[0].get("Version") != "0.2.2":
                raise RuntimeError("this capsule requires Kiri 0.2.2 with custom seccomp enforcement")
        if self.image:
            observed = run_output(self.docker + ["image", "inspect", self.image, "--format", "{{.Id}}"], self.env).strip()
            if observed != self.image:
                raise RuntimeError("prebuilt test image identity mismatch")
            self.override = tempfile.NamedTemporaryFile(mode="w", suffix=".json")
            json.dump({"services": {key: {"image": self.image, "pull_policy": "never"} for key in ("proxy", "browser")}}, self.override)
            self.override.flush()
            self.compose += ["-f", self.override.name]
        if self.resource_ids("container") or self.resource_ids("network"):
            raise RuntimeError("generated project name already exists")
        network_ids = run_output(self.docker + [ "network", "ls", "-q"], self.env).split()
        occupied = []
        if network_ids:
            for row in json.loads(run_output(self.docker + [ "network", "inspect", *network_ids], self.env)):
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
        for kind, command in (("container", self.docker + [ "rm", "-f"]),
                              ("network", self.docker + [ "network", "rm"])):
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
        for name in self.volumes:
            try:
                self.verify_owned("volume", [name])
                run_output(self.docker + [ "volume", "rm", name], self.env)
            except BaseException as error:
                errors.append(error)
        return errors


class Worker:
    def __init__(self, harness, workspace=False, profile=None):
        self.harness = harness
        self.name = harness.project + "-worker-" + uuid.uuid4().hex[:12]
        command = harness.compose + ["run", "--rm", "-T", "--no-deps", "--name", self.name]
        if profile:
            assert workspace and profile in harness.volumes
            harness.verify_owned("volume", [profile])
            command += ["--volume", f"{profile}:/profiles/persistent", "--env", "MOOSURI_BROWSER_PROFILE=bounded"]
        command += ["browser"]
        if workspace:
            command.append("--workspace")
        self.p = subprocess.Popen(command,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=harness.env)
        harness.workers.append(self)  # ready failure is still cleaned up
        self.pipe = JsonPipe(self.p)
        ready = self.pipe.read(45)
        assert ready.get("ready") and ready.get("javascript_enabled"), ready
        assert bool(ready.get("workspace")) == workspace, ready
        assert bool(ready.get("profile_persistence")) == bool(profile), ready
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

    def apparmor_evidence(self):
        # Only labels/executable classes from this owned synthetic container.
        # Never emit command lines, page data or environment values.
        code = '''import json,os
from pathlib import Path
labels={"worker":set(),"chromium":set()}
for p in Path("/proc").iterdir():
 if not p.name.isdigit(): continue
 try:
  name=(p/"comm").read_text().strip()
  kind="worker" if name=="browser-worker-" else "chromium" if name=="chromium" else None
  if kind: labels[kind].add((p/"attr/current").read_text()[:256].strip())
 except (PermissionError,FileNotFoundError,ProcessLookupError): continue
assert all(labels.values()), "missing live worker/Chromium AppArmor evidence"
print(json.dumps({k:sorted(v) for k,v in labels.items()}))'''
        result = json.loads(self.harness.exec_owned(self.name, "python3", "-c", code, user="10001:10001"))
        print(f"EVIDENCE engine={self.harness.engine} apparmor=" + json.dumps(result), flush=True)


def check_page(reply):
    assert reply.get("ok"), reply
    result = reply["result"]
    assert "SCRIPT_RENDERED_OK" in result["text"], result
    assert "not refundable" in result["text"] and "PRIVATE_CANARY" not in result["text"], result
    return result


def exercise_member_tabs(harness):
    worker = Worker(harness, workspace=True)
    start = "https://93.184.216.34/kiri-schedule"
    detail = "https://93.184.216.34/kiri-detail"

    def ask(op, **args):
        reply = worker.ask(dict(op="workspace_" + op, **args))
        assert reply.get("ok"), (op, reply.get("code"))
        return reply["result"]

    def rejected(op, code, **args):
        reply = worker.ask(dict(op="workspace_" + op, **args))
        assert not reply.get("ok") and reply.get("code") == code, (op, reply.get("code"))

    try:
        opened = ask("start_tabs", url=start)
        first = opened["tab_id"]
        assert any(link["url"] == detail and link["text"].strip()
                   for link in opened["observation"]["page"]["links"])
        rejected("follow", "forbidden_destination", tab_id=first, url=start + "/unobserved")
        ask("tab_observe", tab_id=first)
        old = ask("tab_frame", tab_id=first)["frame_token"]
        second = ask("follow", tab_id=first, url=detail)["tab_id"]
        assert second != first
        assert ask("tabs")["active_tab"] == second
        ask("switch", tab_id=second, to_tab_id=first)
        click = {"kind": "click", "x": 80, "y": 125}
        rejected("tab_input", "stale_page", tab_id=first, frame_token=old, input=click)
        fresh = ask("tab_frame", tab_id=first)["frame_token"]
        ask("tab_input", tab_id=first, frame_token=fresh, input=click)
        # The site itself opens this third tab through a trusted human gesture.
        deadline = time.monotonic() + 5
        while True:
            tabs = ask("tabs")["tabs"]
            if len(tabs) == 3:
                break
            assert time.monotonic() < deadline, "site-opened tab missing"
            time.sleep(0.1)
        popup = next(t["tab_id"] for t in tabs if t["tab_id"] not in (first, second))
        ask("switch", tab_id=first, to_tab_id=popup)
        assert ask("tab_frame", tab_id=popup)["url"] == detail
        rejected("follow", "forbidden_destination", tab_id=popup, url=start)
        rejected("tab_observe", "forbidden_destination", tab_id=popup)
        state = ask("close_tab", tab_id=popup)
        for n in (2, 1):
            assert len(state["tabs"]) == n
            state = ask("close_tab", tab_id=state["active_tab"])
        assert state == {"active_tab": None, "tabs": []}
    finally:
        worker.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", choices=["docker", "kiri"], default="docker")
    parser.add_argument("--image", help="exact ID of an already loaded public test-target image")
    parser.add_argument("--launcher", help="absolute path to the built browser_capsule_smoke example")
    args = parser.parse_args()
    if args.engine == "kiri" and not args.launcher:
        parser.error("Kiri acceptance requires --launcher")
    harness = Harness(args.engine, args.image)
    configured = False
    try:
        harness.configure()
        configured = True
        # Build output is inherited (not buffered without a bound by this script).
        if not harness.image:
            subprocess.run(harness.compose + ["build"], env=harness.env, check=True, timeout=1200)
        harness.run("up", "--no-build", "-d", "proxy")
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
        if args.launcher:
            harness.check_launcher(args.launcher)
        first = Worker(harness)
        result = check_page(first.ask({"op": "open", "url": "http://93.184.216.34/"}))
        first.apparmor_evidence()
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
        # Explicit human mode reuses one page/profile through form submission,
        # refreshes navigation for observation, and omits password field values.
        # CA/system trust is active only in the test image; this validates the
        # synthetic leaf SAN/chain before Chromium uses its private NSS database.
        harness.proxy_exec("import urllib.request;u=urllib.request.build_opener(urllib.request.ProxyHandler({}));assert b'Workspace login fixture' in u.open('https://93.184.216.34/workspace-login',timeout=2).read(4096)")
        exercise_member_tabs(harness)
        workspace = Worker(harness, workspace=True)
        for changing_page, action in [
            ("workspace-shift", {"kind": "click", "x": 50, "y": 75}),
            ("workspace-overlay", {"kind": "click", "x": 50, "y": 75}),
            ("workspace-focus", {"kind": "insert_text", "text": "SHOULD_NOT_BE_INSERTED"}),
        ]:
            opened = workspace.ask({"op": "workspace_open", "url": "https://93.184.216.34/" + changing_page})
            assert opened["ok"], opened
            before_change = workspace.ask({"op": "workspace_frame"})
            assert before_change["ok"], before_change
            time.sleep(2.5)
            rejected_change = workspace.ask({"op": "workspace_input", "expected_frame_token": before_change["result"]["frame_token"], "input": action})
            assert not rejected_change["ok"] and rejected_change["code"] == "stale_page", (changing_page, rejected_change)
        login = workspace.ask({"op": "workspace_open", "url": "https://93.184.216.34/workspace-login"})
        assert login["ok"], login
        observation = login["result"]["page"]
        assert "not refundable" in " ".join(observation["text"].split()), observation
        assert "FORM_DEFAULT_SECRET" not in json.dumps(observation), observation
        def human_input(action, delay=0):
            frame = workspace.ask({"op": "workspace_frame"})
            assert frame["ok"], frame
            image = frame["result"]
            assert (image["width"], image["height"]) == (1280, 900), image.keys()
            if delay:
                time.sleep(delay)  # caret blink and color animation preserve geometry
            response = workspace.ask({"op": "workspace_input", "expected_frame_token": image["frame_token"], "input": action})
            assert response == {"ok": True, "result": {"accepted": True}}, response
            return image["frame_token"]
        token = human_input({"kind": "click", "x": 50, "y": 75})
        stale_input = workspace.ask({"op": "workspace_input", "expected_frame_token": token, "input": {"kind": "key", "key": "enter"}})
        assert not stale_input["ok"] and stale_input["code"] == "stale_page", stale_input
        human_input({"kind": "insert_text", "text": "TYPED_SYNTHETIC_SECRET"}, delay=0.65)
        after_typing = workspace.ask({"op": "workspace_observe"})
        assert after_typing["ok"], after_typing
        assert "TYPED_SYNTHETIC_SECRET" not in json.dumps(after_typing), after_typing
        human_input({"kind": "click", "x": 50, "y": 125})
        time.sleep(0.5)
        resumed = workspace.ask({"op": "workspace_observe"})
        assert resumed["ok"] and "COOKIE_SESSION_OK" in resumed["result"]["page"]["text"], resumed
        assert json.loads(harness.fixture_get("counts"))["workspace_posts"] == 1, "HTTPS form POST was not observed"
        same_profile = workspace.ask({"op": "workspace_open", "url": "https://93.184.216.34/workspace-home"})
        assert same_profile["ok"] and "COOKIE_SESSION_OK" in same_profile["result"]["page"]["text"], same_profile
        fresh_workspace = Worker(harness, workspace=True)
        fresh_page = fresh_workspace.ask({"op": "workspace_open", "url": "https://93.184.216.34/workspace-home"})
        assert fresh_page["ok"] and "FRESH_PROFILE_NO_COOKIE" in fresh_page["result"]["page"]["text"], fresh_page
        profile = harness.profile_volume()
        workspace = Worker(harness, workspace=True, profile=profile)
        assert workspace.ask({"op":"workspace_open","url":"https://93.184.216.34/workspace-persistent-login"})["ok"]
        human_input({"kind":"click","x":50,"y":75})
        human_input({"kind":"insert_text","text":"TYPED_SYNTHETIC_SECRET"})
        old_token = human_input({"kind":"click","x":50,"y":125})
        time.sleep(0.5)
        assert "COOKIE_SESSION_OK" in workspace.ask({"op":"workspace_observe"})["result"]["page"]["text"]
        before_recovery = json.loads(harness.fixture_get("counts"))
        assert workspace.loopback_hits() == 0
        workspace.stop()
        workspace = Worker(harness, workspace=True, profile=profile)
        assert workspace.ask({"op":"workspace_blank"}) == {"ok":True,"result":{"blank":True}}
        blank = workspace.ask({"op":"workspace_frame"})
        assert blank["ok"] and blank["result"]["url"] == "about:blank", blank
        old_input = workspace.ask({"op":"workspace_input","expected_frame_token":old_token,"input":{"kind":"key","key":"enter"}})
        assert not old_input["ok"] and old_input["code"] == "stale_page", old_input
        after_recovery = json.loads(harness.fixture_get("counts"))
        for counter in ("persistent_posts", "persistent_authenticated"):
            assert before_recovery[counter] == after_recovery[counter], "recovery revisited a prior site or replayed a POST"
        current = workspace.ask({"op":"workspace_open","url":"https://93.184.216.34/workspace-persistent-home"})
        assert current["ok"] and "COOKIE_SESSION_OK" in current["result"]["page"]["text"], current
        assert json.loads(harness.fixture_get("counts"))["persistent_posts"] == 1
        assert workspace.loopback_hits() == 0
        workspace.stop()
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
            if worker.p.poll() is not None:
                continue  # Explicitly stopped workers had their canary checked before shutdown.
            assert worker.loopback_hits() == 0, "browser request bypassed proxy to loopback canary"
        assert first.ask({"op": "close", "handle": handle})["ok"]
        assert not first.ask({"op": "read", "handle": handle})["ok"]
        print(f"PASS engine={harness.engine} launcher={'passed' if args.launcher else 'SKIPPED'}: JS/negation/PNG, scoped/stale handles, persistent cookie workspace/human input/form omission, owned live canaries, proxy DNS rebind, direct-egress deny")
    finally:
        errors = harness.cleanup() if configured else []
        if harness.override:
            harness.override.close()
        harness.config_dir.cleanup()
        for error in errors:
            print(f"owned smoke cleanup error: {error}", file=sys.stderr)
        if errors and sys.exc_info()[0] is None:
            raise RuntimeError("owned smoke cleanup incomplete")


if __name__ == "__main__":
    main()
