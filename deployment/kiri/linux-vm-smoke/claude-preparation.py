#!/usr/bin/env python3
"""Disposable Linux guest journeys; no credentials, provider login or inference.

Run as root inside the marked VM after placing the exact candidate at --binary.
Re-run with --resume after an actual guest reboot to verify persisted identities.
Never runs against an unmarked host or uses production data.
"""
import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

parser = argparse.ArgumentParser()
parser.add_argument('--binary', required=True)
parser.add_argument('--resume', action='store_true')
parser.add_argument('--retry', action='store_true')
parser.add_argument('--closure', action='store_true', help='Only the inode-default and reserved-login regressions')
args = parser.parse_args()
assert os.geteuid() == 0
assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip() == 'SYNTHETIC_DISPOSABLE_VM'
os.umask(0o077)
root = Path('/root/moosuri-claude-preparation-accept')
root.mkdir(mode=0o700, exist_ok=args.resume or args.retry or args.closure)
binary = str(Path(args.binary).resolve(strict=True))
report = {'no_credentials_login_or_inference': True, 'checks': [],
          'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip()}
if args.resume or args.retry or args.closure:
    report = json.loads((root / 'result.json').read_text())
if args.resume:
    assert report['boot_id'] != Path('/proc/sys/kernel/random/boot_id').read_text().strip(), 'actual_reboot_required'

def put(name, value):
    p = root / name
    p.write_text(json.dumps(value))
    p.chmod(0o600)
    return p

def check(name, action):
    started = time.monotonic()
    try:
        action()
        report['checks'].append({'journey': name, 'status': 'passed', 'candidate_sha256': hashlib.file_digest(open(binary,'rb'),'sha256').hexdigest(), 'seconds': round(time.monotonic()-started, 2)})
    except Exception as error:
        report['checks'].append({'journey': name, 'status': 'failed', 'error_type': type(error).__name__})
        put('result.json', report)
        raise
    put('result.json', report)
    print(json.dumps(report['checks'][-1]), flush=True)

def prepare(request):
    p = put('request.json', request)
    result = subprocess.run([binary, 'claude-runtime-prepare', '--request', str(p)],
                            capture_output=True, text=True, timeout=480)
    if result.returncode:
        # Preparation consumes only this synthetic request. Errors are fixed tokens.
        token = result.stderr.strip()
        assert len(token) < 256 and token.startswith('moosuri: ')
        raise RuntimeError(token)
    value = json.loads(result.stdout)
    assert value['runtime']['state'] == 'host_prepared_not_accepted'
    assert value['runtime']['owner'] == request['owner']
    assert value['runtime']['request_id'] == request['request_id']
    return value

def refuse(request, expected):
    p = put('bad-request.json', request)
    result = subprocess.run([binary, 'claude-runtime-prepare', '--request', str(p)],
                            capture_output=True, text=True, timeout=30)
    assert result.returncode and expected in result.stderr and not result.stdout

def worker(prepared, script, *values):
    service = prepared['runtime']['service']
    result = subprocess.run(['runuser', '-u', service['username'], '--', 'python3', '-c', script, *values],
                            capture_output=True, text=True, timeout=90)
    assert result.returncode == 0, 'worker_fixture_failed'
    return json.loads(result.stdout)

def admission(prepared):
    runtime, storage = prepared['runtime'], prepared['storage']
    profile = {
        'engine': runtime['engine']['path'], 'engine_sha256': runtime['engine']['sha256'],
        'engine_socket': runtime['rootless_socket'], 'image': runtime['image'],
        'official_cli': runtime['official_cli']['path'], 'cli_sha256': runtime['official_cli']['sha256'],
        'cli_version': runtime['official_cli']['version'],
        'helper': runtime['helper']['path'], 'helper_sha256': runtime['helper']['sha256'],
        'auth_root': storage['auth_root'], 'scratch_root': storage['scratch_root'],
        'isolation_acceptance_sha256': '',
        # Synthetic no-account fixture. This does not enroll provider terms for a member.
        'hosting_terms_accepted': True,
    }
    path = Path(storage['scratch_root']) / 'prospective-profile.json'
    path.write_text(json.dumps(profile)); path.chmod(0o600)
    os.chown(path, runtime['service']['uid'], runtime['service']['gid'])
    result = subprocess.run(['runuser', '-u', runtime['service']['username'], '--', binary,
                             'claude-engine-isolation', '--profile-file', str(path)],
                            capture_output=True, text=True, timeout=100)
    assert result.returncode == 0, 'native_admission_failed'
    accepted = json.loads(result.stdout)
    receipt = accepted['receipt']
    assert receipt['host_and_sibling_inaccessible'] and receipt['direct_network_denied']
    assert receipt['proxy_destination_denied'] and receipt['official_cli_version'] == '2.1.322'
    assert receipt['auth_filesystem_bytes'] <= 128*1024*1024 and receipt['auth_filesystem_inodes'] <= 32768
    admitted = json.loads(accepted['profile_json'])
    assert admitted['auth_root'] == storage['auth_root']
    assert len(admitted['isolation_acceptance_sha256']) == 64
    assert accepted['profile_digest'] == hashlib.sha256(accepted['profile_json'].encode()).hexdigest()
    put('native-admission.json', accepted)
    path.unlink()

if args.closure:
    def pinned_defaults_and_reserved_identity():
        # This marked disposable guest owns the configuration; restore it even
        # on failure. A default inode ratio of1024 exceeds the requested bound
        # unless the actual formatter is given the explicit inode count.
        config = Path('/etc/mke2fs.conf')
        original = config.read_bytes()
        import re
        altered, count = re.subn(rb'inode_ratio\s*=\s*\d+', b'inode_ratio = 1024', original)
        assert count > 0
        request_file = root/'closure-request.json'
        assert not request_file.exists(), 'closure_fresh_filesystem_required'
        request = {'version': 1, 'request_id': str(uuid.uuid4()), 'owner': {
            'account_id': str(uuid.uuid4()), 'tenant_id': str(uuid.uuid4()),
            'volume_id': str(uuid.uuid4()), 'generation': 1}}
        put('closure-request.json', request)
        try:
            config.write_bytes(altered)
            prepared = prepare(request)
        finally:
            config.write_bytes(original)
        assert config.read_bytes() == original
        assert prepared['storage']['maximum_inodes'] == 32768
        assert prepare(request) == prepared
        put('closure-prepared.json', prepared)
        admission(prepared)
        # The public login command must refuse the entire reserved namespace
        # before opening these intentionally missing private inputs or a TTY.
        reserved = str(uuid.UUID('12345678-1234-8234-8234-123456789abc'))
        refused = subprocess.run(['runuser', '-u', prepared['runtime']['service']['username'], '--',
            binary, 'claude-engine-login', '--environment-id', reserved,
            '--profile-file', '/does-not-exist-profile', '--expected-email-file', '/does-not-exist-email'],
            capture_output=True, text=True, timeout=15)
        assert refused.returncode and 'claude_worker_environment_invalid' in refused.stderr
        assert not refused.stdout
    check('P10-host-inode-default-and-reserved-login', pinned_defaults_and_reserved_identity)
    sys.exit(0)

if args.resume:
    request = json.loads((root/'original-request.json').read_text())
    original = json.loads((root/'prepared.json').read_text())
    def retained_identity():
        assert prepare(request) == original
    check('P08-reboot-same-owner-and-filesystem', retained_identity)
    check('P08-reboot-native-admission', lambda: admission(original))
    def retained_second_owner():
        second_request = json.loads((root/'second-request.json').read_text())
        second = json.loads((root/'second-prepared.json').read_text())
        assert prepare(second_request) == second
    check('P09-second-owner-after-reboot', retained_second_owner)
    sys.exit(0)

request = {'version': 1, 'request_id': str(uuid.uuid4()), 'owner': {
    'account_id': str(uuid.uuid4()), 'tenant_id': str(uuid.uuid4()), 'volume_id': str(uuid.uuid4()), 'generation': 1}}
if args.retry:
    request = json.loads((root/'original-request.json').read_text())
else:
    put('original-request.json', request)
prepared = None

def fresh():
    global prepared
    prepared = prepare(request)
    assert 0 < prepared['runtime']['service']['uid'] < 20000
    assert prepared['storage']['capacity_bytes'] == 128*1024*1024
    put('prepared.json', prepared)
check('P01-fresh-public-command', fresh)

def replay():
    assert prepare(request) == prepared
check('P02-exact-replay', replay)

def wrong_owner():
    changed = json.loads(json.dumps(request)); changed['owner']['generation'] += 1
    refuse(changed, 'claude_runtime_request_owner_mismatch')
    changed = dict(request, arbitrary_path='/etc/shadow')
    refuse(changed, 'claude_runtime_request_invalid')
check('P03-owner-and-input-conflict', wrong_owner)
check('P04-native-confinement', lambda: admission(prepared))

quota_script = r'''
import errno,json,os,sys
from pathlib import Path
root=Path(sys.argv[1]); test=root/'quota-fixture'; test.mkdir(mode=0o700)
sentinel=test/'sentinel'; sentinel.write_bytes(b'retained')
count=0; failed=False
try:
 with (test/'bytes').open('wb',buffering=0) as f:
  for _ in range(129):
   try: count+=f.write(b'x'*(1024*1024))
   except OSError as e:
    assert e.errno==errno.ENOSPC;failed=True;break
 assert failed and count<=128*1024*1024
 assert sentinel.read_bytes()==b'retained'
finally:
 (test/'bytes').unlink(missing_ok=True)
 sentinel.unlink();test.rmdir()
print(json.dumps({'bounded':failed,'bytes_written':count}))
'''
check('P05-byte-exhaustion-and-retained-data', lambda: worker(prepared, quota_script, prepared['storage']['auth_root']))

inode_script = r'''
import errno,json,os,sys
from pathlib import Path
root=Path(sys.argv[1]);test=root/'inode-fixture';test.mkdir(mode=0o700)
failed=False;count=0
try:
 for n in range(32769):
  try:
   (test/str(n)).touch(exist_ok=False);count+=1
  except OSError as e:
   assert e.errno==errno.ENOSPC;failed=True;break
 assert failed and count<32768
finally:
 for p in test.iterdir():p.unlink()
 test.rmdir()
print(json.dumps({'bounded':failed,'files_created':count}))
'''
check('P06-inode-exhaustion-and-clean-retry', lambda: worker(prepared, inode_script, prepared['storage']['auth_root']))
check('P06-admission-after-quota-recovery', lambda: admission(prepared))

def changed_unit():
    unit = Path(prepared['runtime']['service']['home'])/'.config/systemd/user/moosuri-claude-docker.service'
    # Unit filename is derived from this preparer's exact service implementation.
    assert unit.is_file()
    original = unit.read_bytes()
    try:
        unit.write_bytes(original+b'\n# synthetic conflict\n')
        refuse(request, 'claude_runtime_user_unit_conflict')
    finally:
        unit.write_bytes(original)
    assert prepare(request) == prepared
check('P07-worker-unit-change-refused', changed_unit)
def distinct_owner():
    second_request = {'version': 1, 'request_id': str(uuid.uuid4()), 'owner': {
        'account_id': str(uuid.uuid4()), 'tenant_id': str(uuid.uuid4()),
        'volume_id': str(uuid.uuid4()), 'generation': 1}}
    if (root/'second-request.json').exists():
        second_request = json.loads((root/'second-request.json').read_text())
    else:
        put('second-request.json', second_request)
    second = prepare(second_request)
    assert second['runtime']['service']['uid'] != prepared['runtime']['service']['uid']
    assert second['storage']['auth_root'] != prepared['storage']['auth_root']
    assert second['runtime']['official_cli'] == prepared['runtime']['official_cli']
    canary = Path(prepared['storage']['auth_root'])/'owner-canary'
    canary.write_bytes(b'synthetic-owner-data'); canary.chmod(0o600)
    os.chown(canary, prepared['runtime']['service']['uid'], prepared['runtime']['service']['gid'])
    try:
        code = "import json,sys\nfrom pathlib import Path\ntry: Path(sys.argv[1]).read_bytes(); raise AssertionError('foreign_read')\nexcept PermissionError: print(json.dumps({'foreign_read_denied':True}))"
        assert worker(second, code, str(canary))['foreign_read_denied']
    finally:
        canary.unlink()
    put('second-prepared.json', second)
check('P09-second-owner-separate-auth-storage', distinct_owner)
print(json.dumps({'status':'awaiting_actual_reboot','checks':len(report['checks'])}),flush=True)
