#!/usr/bin/python3
"""Test target only: owned-target network probes and a real local canary."""
import ipaddress
import json
import os
import socket
import subprocess
import sys
import time
import urllib.request

REAL = "/usr/local/bin/browser-worker-real"
args = sys.argv[1:]
if args == ["--validate-proxy"]:
    os.execv(REAL, [REAL, *args])
proxy = str(ipaddress.IPv4Address(os.environ["MOOSURI_BROWSER_PROXY_IP"]))
# The derived seccomp profile permits namespaced chroot, not host capability.
status = dict(line.split(":", 1) for line in open("/proc/self/status") if ":" in line)
assert os.geteuid() == 10001
for field in ("CapEff", "CapPrm", "CapInh", "CapAmb", "CapBnd"):
    assert int(status[field].strip(), 16) == 0, field
assert status["NoNewPrivs"].strip() == "1"
assert status["Seccomp"].strip() == "2"
try:
    os.chroot("/")
except PermissionError as error:
    assert error.errno == 1
else:
    raise SystemExit("non-namespaced chroot unexpectedly allowed")

if args == ["--network-probe"]:
    # Both listeners are owned by this test project's proxy namespace. No host
    # gateway, metadata endpoint or external address is contacted by this probe.
    for target in [(proxy, 8081), ("93.184.216.34", 80)]:
        try:
            connection = socket.create_connection(target, timeout=0.5)
        except OSError:
            continue
        connection.close()
        raise SystemExit("direct egress to owned fixture was possible")
    with socket.create_connection((proxy, 3128), timeout=2):
        pass
    print(json.dumps({"direct_egress_blocked": True, "proxy_reachable": True}))
    raise SystemExit(0)
if args not in ([], ["--workspace"]):
    raise SystemExit("unsupported test worker argument")
canary = subprocess.Popen(
    [sys.executable, "/capsule/loopback-canary.py"],
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
)
# This runs as the same unprivileged user, after namespace firewall setup and
# before browser launch. An actual successful HTTP response is the positive
# control; absence of a listener can never pass this test.
local = urllib.request.build_opener(urllib.request.ProxyHandler({}))
try:
    for attempt in range(40):
        try:
            with local.open("http://127.0.0.1:8081/positive-control", timeout=0.25) as r:
                assert r.read(128) == b"LOOPBACK_CANARY_SHOULD_NEVER_BE_READ"
            with local.open("http://127.0.0.1:8081/reset", timeout=0.25) as r:
                assert r.read(128) == b"reset"
            break
        except OSError:
            if canary.poll() is not None or attempt == 39:
                raise
            time.sleep(0.05)
except BaseException:
    canary.terminate()
    canary.wait(timeout=2)
    raise
os.execv(REAL, [REAL, *args])
