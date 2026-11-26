"""Exercise owned profile replacement on the existing synthetic invitation fixture.

Run only in its marked disposable guest, with a candidate binary already copied.
The profile gains its own Google socket and Vault origin. No provider is contacted.
"""
import argparse
import fcntl
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import socket
import sqlite3
import subprocess


def engine(path):
    c = http.client.HTTPConnection("localhost", timeout=20)
    c.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    c.sock.settimeout(20)
    c.sock.connect("/run/kiri/docker.sock")
    try:
        c.request("GET", "/v1.47" + path)
        r = c.getresponse()
        assert r.status == 200, "synthetic engine read failed"
        return json.loads(r.read(1 << 20))
    finally:
        c.close()


def run(args, expected_error=None):
    r = subprocess.run([str(a) for a in args], capture_output=True, text=True, timeout=120)
    if expected_error:
        assert r.returncode and expected_error in r.stderr, "expected refusal missing"
        return
    if r.returncode:
        codes = re.findall(r"\b(?:kiri|personal)_[a-z0-9_]+\b", r.stderr)
        raise RuntimeError("candidate command failed: " + ",".join(codes[:3]))
    return json.loads(r.stdout)


def data_snapshot(container):
    data = Path(next(m["Source"] for m in container["Mounts"] if m["Destination"] == "/data"))
    files = {}
    for p in data.rglob("*"):
        if not p.is_file() or p.name.endswith(("-wal", "-shm")):
            continue
        if p.name.endswith(".sqlite"):
            with sqlite3.connect(p.as_uri() + "?mode=ro", uri=True) as c:
                c.execute("PRAGMA query_only=ON")
                digest = hashlib.sha256("\n".join(c.iterdump()).encode()).hexdigest()
        else:
            digest = hashlib.sha256(p.read_bytes()).hexdigest()
        files[str(p.relative_to(data))] = (p.stat().st_uid, p.stat().st_mode, digest)
    assert files, "fixture data missing"
    return files


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("binary", type=Path)
    args = p.parse_args()
    assert os.geteuid() == 0
    assert Path("/etc/moosuri-kiri-lab-marker").read_text().strip() == "SYNTHETIC_DISPOSABLE_VM"
    os.umask(0o077)
    config_path = Path("/etc/moosuri/kiri-members.json")
    config = json.loads(config_path.read_text())
    enrolled_path = Path(config["enrollment_directory"]) / "members.json"
    enrolled = json.loads(enrolled_path.read_text())
    members = enrolled["members"]
    assert len(members) == 3 and {m["email"] for m in members} == {
        "admin@example.test", "one@example.test", "two@example.test"}
    member = next(m for m in members if m["email"] == "one@example.test")
    assert not any(member.get(k) for k in ("enable_local_gws", "enable_google_readonly", "vault_origin"))
    def inventory():
        rows = [engine("/containers/moosuri-member-" + m["account_id"] + "/json") for m in members]
        assert all(r["State"]["Running"] for r in rows)
        return rows
    before = inventory()
    old = next(r for r in before if r["Config"]["Labels"]["org.moosuri.account"] == member["account_id"])
    state = Path("/var/lib/moosuri-control/recovery-check")
    assert state.is_dir()
    google = Path("/var/lib/moosuri-control/rollout-google")
    google.mkdir(mode=0o700)
    child = google / member["account_id"]
    child.mkdir(mode=0o711)
    child.chmod(0o711)
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.bind(str(child / "google.sock"))
    os.chown(child / "google.sock", member["uid"], member["uid"])
    os.chmod(child / "google.sock", 0o600)
    command = [args.binary, "kiri-rollout", "--experimental", "--email", member["email"],
               "--uid", str(member["uid"]), "--lifecycle-state-directory", state,
               "--expected-container-id", old["Id"], "--old-image", config["image"],
               "--new-image", config["image"], "--new-google-sockets-root", google,
               "--new-enable-local-gws", "--new-vault-origin", "https://vault.example.test", "--start"]
    for key in ("registry_directory", "mounts_root", "ipc_root", "allocations_root",
                "memory_bytes", "nano_cpus", "pids", "max_volume_bytes"):
        command.extend(["--" + key.replace("_", "-"), str(config[key])])
    lock = os.open(state, os.O_RDONLY | os.O_DIRECTORY)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        run(command, "kiri_rollout_lifecycle_worker_running")
    finally:
        os.close(lock)
    assert inventory() == before
    print("PASS U01: active lifecycle lock refuses replacement without stopping a member", flush=True)
    wrong = command.copy()
    wrong[wrong.index("--expected-container-id") + 1] = next(r["Id"] for r in before if r["Id"] != old["Id"])
    run(wrong, "kiri_rollout_container_id_mismatch")
    assert inventory() == before
    print("PASS U02: another member's container ID is refused without changes", flush=True)
    saved = data_snapshot(old)
    reservations = {p.name: p.read_bytes() for p in Path(config["allocations_root"]).glob("uid-*.json")}
    result = run(command)
    assert result["replaced"] and result["readiness"] == "owner-identified-health-ready"
    after = inventory()
    updated = next(r for r in after if r["Config"]["Labels"]["org.moosuri.account"] == member["account_id"])
    assert updated["Id"] != old["Id"]
    assert updated["Config"]["Labels"] == old["Config"]["Labels"]
    assert updated["Config"]["User"] == old["Config"]["User"]
    assert all(m in updated["Mounts"] for m in old["Mounts"])
    addition = [m for m in updated["Mounts"] if m not in old["Mounts"]]
    assert len(addition) == 1 and addition[0]["Destination"] == "/google" and not addition[0]["RW"]
    assert data_snapshot(updated) == saved
    assert {p.name:p.read_bytes() for p in Path(config["allocations_root"]).glob("uid-*.json")} == reservations
    print("PASS U03: owned replacement preserves UID, volume, keys and database; adds only own read-only Google mount", flush=True)
    for old_row, new_row in zip(before, after):
        if old_row["Id"] != old["Id"]:
            assert old_row == new_row
    print("PASS U04: both sibling containers retain their exact profiles and running PIDs", flush=True)
    replay = run(command)
    assert not replay["replaced"] and replay["container_id"] == updated["Id"]
    assert inventory() == after and data_snapshot(updated) == saved
    print("PASS U05: same-target retry preserves container ID/PID/data without replacement", flush=True)
    member.update(enable_local_gws=True, vault_origin="https://vault.example.test")
    config["google_sockets_root"] = str(google)
    enrolled_path.write_text(json.dumps(enrolled))
    config_path.write_text(json.dumps(config))
    run([args.binary, "kiri-lifecycle", "--experimental", "--config", config_path,
         "--state-directory", state, "--once"])
    assert inventory() == after and data_snapshot(updated) == saved
    print("PASS U06: matched lifecycle resumes all three members without another replacement", flush=True)
    sock.close()


if __name__ == "__main__":
    main()
