"""Exercise the shipped onboarding timer in a fresh, marked Kiri Linux guest.

Install the candidate as /usr/local/bin/moosuri, load its matching runtime image,
and install the repository's onboarding service and timer. Pass the image ID.
"""

import argparse
import http.client
from http.cookies import SimpleCookie
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import time
import uuid


BINARY = "/usr/local/bin/moosuri"
UNIT = "moosuri-kiri-onboard"
CONTROL = Path("/var/lib/moosuri-control")
OPS = CONTROL / "invitations"
CONFIG = Path("/etc/moosuri/kiri-members.json")
POLICY = CONFIG.with_name("kiri-onboard-policy.json")
STORAGE = Path("/srv/moosuri-storage")
GATEWAY = CONTROL / "gateway"
REGISTRY = CONTROL / "registry"
ORIGIN = "https://personal.internal"
VOLUME_BYTES = 64 << 20


def run(*args, secret=None):
    data = None if secret is None else (secret + "\n") if secret else ""
    result = subprocess.run(
        args,
        input=data,
        stdin=subprocess.DEVNULL if data is None else None,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if result.returncode:
        raise RuntimeError(f"{Path(args[0]).name} {args[1]} exited with {result.returncode}")
    return result.stdout


def until(check, seconds, failure):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.25)
    raise RuntimeError(failure)


class Gateway(http.client.HTTPConnection):
    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(15)
        self.sock.connect(str(GATEWAY / "gateway.sock"))


def post(path, body, cookies, account=None):
    headers = {
        "Content-Type": "application/json",
        "Host": "personal.internal",
        "Origin": ORIGIN,
        "X-Real-IP": "192.0.2.50",
        "Cookie": cookies.output(header="", sep=";").strip(),
    }
    if account:
        headers["X-Moosuri-Account"] = account
    client = Gateway("personal.internal")
    try:
        client.request("POST", "/api/v1/" + path, json.dumps(body), headers)
        response = client.getresponse()
        for key, value in response.getheaders():
            if key.lower() == "set-cookie":
                received = SimpleCookie(value)
                for name, morsel in received.items():
                    cookies[name] = morsel.value
        raw = response.read()
        return response.status, json.loads(raw) if raw else {}
    finally:
        client.close()


def login(email, password):
    cookies = SimpleCookie()
    status, body = post("auth/login", {"email": email, "password": password}, cookies)
    return status, body, cookies


def passed(message):
    print("PASS " + message, flush=True)


def create_fixture(image):
    if (
        os.geteuid() != 0
        or os.uname().sysname != "Linux"
        or Path("/etc/moosuri-kiri-lab-marker").read_text().strip() != "SYNTHETIC_DISPOSABLE_VM"
    ):
        raise RuntimeError("This fixture requires a fresh disposable Kiri Linux guest")
    if any(p.exists() or p.is_symlink() for p in (CONFIG.parent, CONTROL, STORAGE)):
        raise RuntimeError("The guest installation directories already exist")
    if run("systemctl", "show", UNIT + ".timer", "-P", "LoadState").strip() != "loaded":
        raise RuntimeError("Install the repository's onboarding timer and service first")
    os.umask(0o077)
    CONFIG.parent.mkdir()
    CONTROL.mkdir()
    CONTROL.chmod(0o711)
    for path in (REGISTRY, OPS, CONTROL / "enrolled", GATEWAY, STORAGE):
        path.mkdir(mode=0o700)
    CONFIG.write_text(
        json.dumps(
            {
                "version": 1,
                "registry_directory": str(REGISTRY),
                "mounts_root": str(STORAGE / "mounts"),
                "ipc_root": str(STORAGE / "ipc"),
                "allocations_root": str(STORAGE / "assignments"),
                "enrollment_directory": str(CONTROL / "enrolled"),
                "image": image,
                "memory_bytes": 128 << 20,
                "nano_cpus": 250000000,
                "pids": 64,
                "max_volume_bytes": VOLUME_BYTES,
                "members": [],
            }
        )
    )
    POLICY.write_text(json.dumps({"uid_pool": [41001, 41002], "max_total_volume_bytes": VOLUME_BYTES}))


def claims():
    record = OPS / "web-onboarding.json"
    return json.loads(record.read_text())["claims"] if record.exists() else {}


def waiting(request):
    record = OPS / (request + ".json")
    return record.exists() and json.loads(record.read_text())["stage"] == "password_required"


def exercise(owner_email, owner_password, common):
    status, user, jar = login(owner_email, owner_password)
    assert status == 200 and user["password_change_required"]
    owner = user["user"]["id"]
    replacement = secrets.token_urlsafe(24)
    assert post("auth/password", {"password": replacement}, jar, owner)[0] == 200
    run(
        BINARY,
        "installation-admin",
        "--registry",
        REGISTRY,
        "bootstrap",
        "--request-id",
        str(uuid.uuid4()),
        "--email",
        owner_email,
        "--ipc-root",
        STORAGE / "ipc",
        "--member-uid",
        "40001",
        "--password-stdin",
        secret=replacement,
    )
    invitations = {}
    for email in ("one@example.test", "two@example.test"):
        request = str(uuid.uuid4())
        status, receipt = post(
            "personal/admin/invitations",
            {"request_id": request, "name": "Linux Fixture", "email": email},
            jar,
            owner,
        )
        assert status == 200 and receipt["request_id"] == request
        invitations[request] = email
    passed("authenticated administrator reserved two invitations")

    run("systemctl", "start", UNIT + ".timer")
    try:
        first = until(
            lambda: next((r for r in invitations if waiting(r)), None),
            155,
            "No invitation reached password_required through the installed timer",
        )
    finally:
        run("systemctl", "stop", UNIT + ".timer")
    until(
        lambda: run("systemctl", "show", UNIT + ".service", "-P", "ActiveState").strip() == "inactive",
        30,
        "The onboarding service did not finish",
    )
    assert run("systemctl", "show", UNIT + ".service", "-P", "Result").strip() == "success"
    (other,) = set(invitations) - {first}
    pending = claims()
    assert set(pending) == {first} and not waiting(other)
    assert pending[first]["profile"]["uid"] == 41001
    passed("stock timer prepared one member and kept the other within the storage budget")
    backing = STORAGE / "images" / (pending[first]["invitation"]["volume_id"] + ".img")
    inode = backing.stat().st_ino
    worker = [BINARY, "kiri-onboard-worker", *common, "--policy", POLICY]
    run(*worker)
    assert claims() == pending and inode == backing.stat().st_ino
    passed("repeat scan preserved the pending member identity and volume")

    password = secrets.token_urlsafe(24)
    assert login(invitations[first], password)[0] != 200
    finish = ["--finish-request", first, "--password-stdin"]
    assert json.loads(run(*worker, *finish, secret=password))["ready"]
    status, user, member = login(invitations[first], password)
    account = pending[first]["invitation"]["account_id"]
    assert status == 200 and user["password_change_required"] and user["user"]["id"] == account
    changed = secrets.token_urlsafe(24)
    assert post("auth/password", {"password": changed}, member, account)[0] == 200
    assert login(invitations[first], password)[0] != 200
    passed("first member logged in and rotated the required initial password")
    assert json.loads(run(*worker, *finish, secret=""))["ready"]
    status, user, _ = login(invitations[first], changed)
    assert status == 200 and user["user"]["id"] == account and not user["password_change_required"]
    assert login(invitations[other], changed)[0] != 200
    passed("retry preserved the changed credential and member isolation")

    policy = json.loads(POLICY.read_text())
    policy["max_total_volume_bytes"] *= 2
    POLICY.write_text(json.dumps(policy))
    for _ in range(2):
        run(*worker)
        if waiting(other):
            break
    expanded = claims()
    assert waiting(other) and set(expanded) == set(invitations) and expanded[first] == pending[first]
    assert expanded[other]["profile"]["uid"] != expanded[first]["profile"]["uid"]
    assert expanded[other]["invitation"]["volume_id"] != expanded[first]["invitation"]["volume_id"]
    second_password = secrets.token_urlsafe(24)
    assert json.loads(
        run(*worker, "--finish-request", other, "--password-stdin", secret=second_password)
    )["ready"]
    status, second, _ = login(invitations[other], second_password)
    assert status == 200 and second["user"]["id"] == expanded[other]["invitation"]["account_id"]
    passed("increased capacity admitted the second member with a separate UID and volume")

    alternate = CONFIG.with_name("other-installation.json")
    config = json.loads(CONFIG.read_text())
    config["enrollment_directory"] = str(CONTROL / "other-enrolled")
    (CONTROL / "other-enrolled").mkdir()
    alternate.write_text(json.dumps(config))
    other_args = [alternate if a == CONFIG else a for a in worker]
    refused = subprocess.run([*other_args, *finish], input="", capture_output=True, text=True, timeout=60)
    assert (
        refused.returncode
        and "onboard_worker_operations_belong_to_another_installation" in refused.stderr
    )
    assert claims() == expanded and inode == backing.stat().st_ino
    passed("a changed installation path preserved both existing member claims")
    journal = run("journalctl", "-u", UNIT, "--no-pager", "-o", "cat")
    assert "stage=capacity_waiting" in journal
    queue = (OPS / "web-onboarding.json").read_text()
    assert all(
        s not in journal and s not in queue
        for s in (owner_password, replacement, password, changed, second_password)
    )
    passed("the service journal and host queue excluded all synthetic passwords")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", help="loaded sha256 ID of the matching member runtime image")
    image = parser.parse_args().image
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", image):
        parser.error("expected the loaded image SHA-256 ID")
    create_fixture(image)
    common = ["--experimental", "--config", CONFIG, "--operations-root", OPS]
    owner, password = "admin@example.test", secrets.token_urlsafe(24)
    ready = run(
        BINARY,
        "kiri-onboard",
        *common,
        "--request-id",
        str(uuid.uuid4()),
        "--first-name",
        "Owner",
        "--first-email",
        owner,
        "--uid",
        "40001",
        "--password-stdin",
        secret=password,
    )
    assert json.loads(ready)["ready"]
    gateway = subprocess.Popen(
        [
            BINARY,
            "personal-gateway",
            "--public-origin",
            ORIGIN,
            "--registry",
            REGISTRY,
            "--ipc-root",
            STORAGE / "ipc",
            "--socket-dir",
            GATEWAY,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        until(lambda: (GATEWAY / "gateway.sock").is_socket(), 15, "Gateway did not start")
        exercise(owner, password, common)
    finally:
        gateway.terminate()
        gateway.wait(timeout=15)


if __name__ == "__main__":
    main()
