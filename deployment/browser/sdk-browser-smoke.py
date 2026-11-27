#!/usr/bin/env python3
"""Real Chrome callback acceptance; no Google access, credentials or provider writes.

Requires a built moosuri-connectors test binary and the pinned Google-auth image.
Uses its unmodified isolation entrypoint except for the final test executable.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
from uuid import uuid4

p = argparse.ArgumentParser()
p.add_argument('--image', required=True)
p.add_argument('--test-binary', required=True)
p.add_argument('--claude', action='store_true', help='Private Claude callback instead of Google SDK')
p.add_argument('--docker-host', default='unix:///var/run/docker.sock',
               choices=('unix:///var/run/docker.sock', 'unix:///run/kiri/docker.sock'))
p.add_argument('--case')
a = p.parse_args()
cases = (("valid", "wrong_state", "iframe", "popup", "subresource", "cancel") if a.claude else
         ("valid", "wrong_state", "iframe", "popup", "subresource", "burst", "retry_after_error"))
if a.case and a.case not in cases:
    p.error('case must be one of: '+', '.join(cases))
test = ('browser::workspace::claude::browser_tests::actual_chrome_binds_private_claude_callback'
        if a.claude else 'browser::google_auth::sdk::tests::actual_chrome_intercepts_only_the_bound_top_level_callback')
case_env = 'MOOSURI_CLAUDE_TEST_CASE' if a.claude else 'MOOSURI_SDK_TEST_CASE'
root = Path(__file__).resolve().parent
binary = Path(a.test_binary).resolve(strict=True)
assert binary.is_file() and os.access(binary, os.X_OK)
assert a.image.startswith('sha256:') and len(a.image) == 71
with tempfile.TemporaryDirectory(prefix='moosuri-sdk-browser-') as directory:
    entry = (root / 'entrypoint.sh').read_text()
    old = '/usr/local/bin/browser-worker "$@"'
    assert entry.count(old) == 1
    script = Path(directory) / 'entrypoint.sh'
    script.write_text(entry.replace(old, '/usr/local/bin/sdk-browser-tests "$@"'))
    script.chmod(0o555)
    docker = ['docker', '--host=' + a.docker_host]
    for scenario in (a.case,) if a.case else cases:
        name = 'moosuri-sdk-browser-test-' + uuid4().hex
        try:
            result = subprocess.run(docker + [
                'run', '--rm', '--name', name, '--network=none', '--cgroupns=private',
                '--read-only', '--cap-drop=ALL', '--cap-add=NET_ADMIN', '--cap-add=SETUID',
                '--cap-add=SETGID', '--cap-add=SETPCAP', '--security-opt=no-new-privileges:true',
                '--security-opt=seccomp=' + str(root / 'seccomp.json'),
                '--memory=768m', '--memory-swap=768m', '--pids-limit=256', '--cpus=1',
                '--tmpfs=/profiles:rw,noexec,nosuid,size=128m,uid=10001,gid=10001,mode=0700',
                '--tmpfs=/tmp:rw,noexec,nosuid,size=64m,mode=1777',
                '--tmpfs=/run:rw,noexec,nosuid,size=1m,mode=0755', '--shm-size=256m',
                '--env=MOOSURI_BROWSER_PROXY_IP=10.42.17.2',
                '--env=MOOSURI_BROWSER_TENANT=' + str(uuid4()),
                '--env=' + case_env + '=' + scenario,
                '--mount=type=bind,src=' + str(binary) + ',dst=/usr/local/bin/sdk-browser-tests,readonly',
                '--mount=type=bind,src=' + str(script) + ',dst=/capsule/sdk-test-entry.sh,readonly',
                '--entrypoint=/usr/bin/tini', a.image, '--', '/capsule/sdk-test-entry.sh',
                '--ignored', '--exact', test,
                '--nocapture',
            ], timeout=90)
            if result.returncode != 0:
                raise SystemExit(result.returncode)
            print("PASS actual Chrome: " + scenario, flush=True)
        finally:
            subprocess.run(docker + ['rm', '--force', name], capture_output=True, timeout=15)
