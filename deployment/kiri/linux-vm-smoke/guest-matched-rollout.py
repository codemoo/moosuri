#!/usr/bin/env python3
"""Move the two-member installed fixture to a different matched source bundle.

This is a disposable-guest acceptance harness, not a production updater. It uses
the public owned-member rollout command and the actual browser admission probe.
It retains old artifacts/configs and the original installer receipt. Run the
installed browser harness and reboot acceptance after this command succeeds.
No provider is called and no real account or token may be added to this fixture.
"""
import argparse
import hashlib
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import socket
import ssl
import subprocess
import sys
import time
import traceback
from uuid import uuid4

ENV = dict(os.environ, DOCKER_API_VERSION='1.47')
DOCKER = ['docker', '--host=unix:///run/kiri/docker.sock']
EVIDENCE = None


def checkpoint(phase, **values):
    if EVIDENCE is not None:
        (EVIDENCE / 'phase.json').write_text(json.dumps({'phase': phase, **values}))


def run(args, timeout=120, body=None):
    result = subprocess.run([str(a) for a in args], input=body, text=True,
                            capture_output=True, env=ENV, timeout=timeout)
    if result.returncode:
        codes = re.findall(r'\b(?:kiri|personal|browser)_[a-z0-9_]+\b', result.stderr)
        operation = Path(str(args[0])).name
        raise RuntimeError(f'command_failed_{operation}_exit_{result.returncode}: ' + ','.join(codes[:3]))
    return result.stdout.strip()


def docker(*args, timeout=120):
    return run(DOCKER + list(args), timeout=timeout)


def digest(path):
    with path.open('rb') as stream:
        value = hashlib.sha256()
        for block in iter(lambda: stream.read(1 << 20), b''):
            value.update(block)
    return value.hexdigest()


def inspect(account):
    return json.loads(docker('container', 'inspect', 'moosuri-member-' + account))[0]


def snapshot(root):
    result = {}
    for path in root.rglob('*'):
        if not path.is_file() or path.name.endswith(('-wal', '-shm')):
            continue
        relative = str(path.relative_to(root))
        with path.open('rb') as stream:
            is_sqlite = stream.read(16) == b'SQLite format 3\0'
        if is_sqlite:
            with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as db:
                db.execute('PRAGMA query_only=ON')
                value = hashlib.sha256('\n'.join(db.iterdump()).encode()).hexdigest()
        else:
            value = digest(path)
        result[relative] = [path.stat().st_uid, path.stat().st_mode, value]
    assert result, 'fixture_snapshot_empty'
    return result


def atomic_copy(source, target, mode):
    temporary = target.with_name(target.name + '.rollout-' + uuid4().hex)
    shutil.copyfile(source, temporary)
    os.chmod(temporary, mode)
    os.replace(temporary, target)


def capsule(role, image, previous, seccomp, probe, evidence_dir, used):
    project = 'moosuri-rollout-' + uuid4().hex
    owner = previous['owner']
    # Preserve the names even if a create reply is lost. Failure leaves this
    # isolated guest for inspection; never restart lifecycle with a mixed image.
    checkpoint('preparing_' + role, project=project, owner=owner)
    labels = ['--label', 'io.moosuri.browser.owner=' + owner,
              '--label', 'com.docker.compose.project=' + project]
    subnet = next(ipaddress.ip_network(f'10.178.{i}.0/24') for i in range(70, 200)
                  if not any(ipaddress.ip_network(f'10.178.{i}.0/24').overlaps(n) for n in used))
    used.append(subnet)
    proxy_ip = str(subnet.network_address + 2)
    internal = docker('network', 'create', '--internal', '--subnet', str(subnet),
                      *labels, project + '_browser')
    public = docker('network', 'create', *labels, project + '_public')
    proxy = docker('container', 'create', '--name', project + '-proxy-1',
                   '--entrypoint', '/usr/bin/tini', '--user', '10001:10001',
                   '--read-only', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true',
                   '--network', public, '--pids-limit', '64',
                   '--memory', '128m', '--cpus', '1', '--restart', 'unless-stopped',
                   *labels, '--label', 'com.docker.compose.service=proxy', image,
                   '--', '/usr/local/bin/browser-proxy')
    docker('network', 'connect', '--ip', proxy_ip, internal, proxy)
    docker('container', 'start', proxy)
    config = dict(engine='kiri', image_id=image, network_id=internal,
                  proxy_container_id=proxy, proxy_ip=proxy_ip, owner=owner,
                  project=project, seccomp_file=str(seccomp), verified=False,
                  smoke_evidence_sha256='', javascript_enabled=True)
    (evidence_dir / (role + '-input.json')).write_text(json.dumps(config))
    result = json.loads(run([probe] + (['--google'] if role == 'google_auth' else []),
                            body=json.dumps(config)))
    assert result['config']['verified']
    assert result['evidence']['browser_started'] and result['evidence']['owned_cleanup']
    evidence_bytes = json.dumps(result['evidence'], sort_keys=True, indent=2).encode()
    assert hashlib.sha256(evidence_bytes).hexdigest() == result['config']['smoke_evidence_sha256'], 'probe_evidence_hash_mismatch'
    (evidence_dir / (role + '-smoke.json')).write_bytes(evidence_bytes)
    (evidence_dir / (role + '-result.json')).write_text(json.dumps(result, indent=2))
    print('PASS T02 ' + role + ': new image starts confined; actual admission and cleanup', flush=True)
    return result['config']


def service_health(manifest, brokers_only=False):
    names = ['model', 'gws', 'browser']
    if not brokers_only:
        names.extend(['vault', 'lifecycle', 'gateway'])
    units = ['moosuri-kiri-' + name + '.service' for name in names]
    previous = None
    stable_since = None
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        current = []
        for unit in units:
            state = dict(line.split('=', 1) for line in run([
                'systemctl', 'show', unit, '-p', 'ActiveState', '-p', 'SubState',
                '-p', 'MainPID', '-p', 'NRestarts']).splitlines())
            if state['ActiveState'] != 'active' or state['SubState'] != 'running' or int(state['MainPID']) <= 1:
                current = []
                break
            binary = 'moosuri-vault-service' if unit == 'moosuri-kiri-vault.service' else 'moosuri'
            # Type=simple can report a PID before exec completes. Only the
            # matching executable counts toward the stable interval.
            try:
                matching = digest(Path('/proc') / state['MainPID'] / 'exe') == manifest['files'][binary]
            except OSError:
                matching = False
            if not matching:
                current = []
                break
            current.append(state)
        if current and current == previous:
            if stable_since is not None and time.monotonic() - stable_since >= 10:
                break
        else:
            stable_since = time.monotonic() if current else None
        previous = current
        time.sleep(2)
    else:
        raise RuntimeError('matched_services_not_stable')
    if brokers_only:
        return
    run(['systemctl', 'is-active', '--quiet', 'moosuri-kiri-onboard.timer'])
    tls = ssl.create_default_context(cafile='/root/moosuri-install-tls/cert.pem')
    for host in ['assistant.example.com', 'vault.example.com']:
        connection = http.client.HTTPSConnection(host, context=tls, timeout=15)
        connection.sock = tls.wrap_socket(socket.create_connection(('127.0.0.1', 443), timeout=15), server_hostname=host)
        try:
            connection.request('GET', '/', headers={'Host': host})
            response = connection.getresponse()
            assert response.status == 200 and response.read(262144), 'installed_https_health_failed'
        finally:
            connection.close()
    final = [dict(line.split('=', 1) for line in run([
        'systemctl', 'show', unit, '-p', 'ActiveState', '-p', 'SubState',
        '-p', 'MainPID', '-p', 'NRestarts']).splitlines()) for unit in units]
    assert final == previous, 'services_changed_during_https_health'


def main():
    global EVIDENCE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', required=True, type=Path)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--probe', required=True, type=Path)
    parser.add_argument('--evidence-dir', required=True, type=Path)
    args = parser.parse_args()
    assert os.geteuid() == 0 and sys.platform == 'linux'
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip() == 'SYNTHETIC_DISPOSABLE_VM'
    os.umask(0o077)
    state = json.loads(Path('/root/moosuri-clean-acceptance-state.json').read_text())
    assert {m['email'] for m in state['members']} == {
        'install-test@example.com', 'second-install@example.com'}
    manifest = json.loads((args.bundle / 'manifest.json').read_text())
    assert manifest['format'] == 1 and manifest['source_revision'] == args.revision
    assert not (args.bundle / '.incomplete').exists()
    assert set(manifest['files']) == {'moosuri', 'moosuri-vault-service', 'member.tar',
                                     'browser.tar', 'google-auth.tar', 'seccomp.json'}
    for name, expected in manifest['files'].items():
        assert digest(args.bundle / name) == expected, 'bundle_integrity_failed'
    config_path = Path('/etc/moosuri/kiri-members.json')
    config = json.loads(config_path.read_text())
    enrolled_path = Path(config['enrollment_directory']) / 'members.json'
    enrolled = json.loads(enrolled_path.read_text())
    members = enrolled['members']
    assert {m['email'] for m in members} == {m['email'] for m in state['members']}
    assert not config['members']
    assert manifest['images']['member'] != config['image'], 'different_image_required'
    version = json.loads(docker('version', '--format', '{{json .Server}}'))
    assert version['Platform']['Name'] == 'Kiri'
    assert any(c['Name'] == 'Kiri' and c['Version'] == '0.2.2' for c in version['Components'])
    args.evidence_dir.mkdir(mode=0o700)
    EVIDENCE = args.evidence_dir
    checkpoint('validated_bundle', source_revision=args.revision)
    backup = args.evidence_dir / 'before'
    backup.mkdir(mode=0o700)
    for path in [config_path, enrolled_path, Path('/etc/moosuri/kiri-install.json'),
                 Path('/etc/moosuri/browser.json'), Path('/etc/moosuri/google-browser.json'),
                 Path('/etc/moosuri/browser-smoke.json'), Path('/etc/moosuri/google-browser-smoke.json'),
                 Path('/usr/local/bin/moosuri'), Path('/usr/local/bin/moosuri-vault-service')]:
        shutil.copyfile(path, backup / path.name)
    receipt = digest(Path('/etc/moosuri/kiri-install.json'))
    allocations = {p.name: digest(p) for p in Path(config['allocations_root']).glob('uid-*.json')}
    key_hashes = {p: digest(Path(p)) for p in state['files']}
    key_hashes['/etc/moosuri/model-fingerprint.key'] = digest(Path('/etc/moosuri/model-fingerprint.key'))
    for archive, role in [('member.tar', 'member'), ('browser.tar', 'browser'), ('google-auth.tar', 'google_auth')]:
        docker('load', '-i', str(args.bundle / archive), timeout=1200)
        assert docker('image', 'inspect', '--format', '{{.Id}}', manifest['images'][role]) == manifest['images'][role]
    print('PASS T01: source revision, all bundle hashes and all three Kiri image identities verified', flush=True)
    # ProtectHome hides /root from the installed services. Keep runtime inputs
    # under /etc while retaining the same hash-verified bundle bytes.
    seccomp = Path('/etc/moosuri') / ('rollout-seccomp-' + manifest['files']['seccomp.json'] + '.json')
    atomic_copy(args.bundle / 'seccomp.json', seccomp, 0o600)
    used = []
    for identifier in docker('network', 'ls', '-q').split():
        row = json.loads(docker('network', 'inspect', identifier))[0]
        used.extend(ipaddress.ip_network(n['Subnet']) for n in row.get('IPAM', {}).get('Config', []) if n.get('Subnet'))
    used.extend(ipaddress.ip_network(a['local'] + '/' + str(a['prefixlen']), strict=False)
                for link in json.loads(run(['ip', '-j', '-4', 'address', 'show']))
                for a in link.get('addr_info', []) if a.get('family') == 'inet')
    configs = {}
    for role, filename in [('browser', 'browser.json'), ('google_auth', 'google-browser.json')]:
        configs[filename] = capsule(role, manifest['images'][role],
                                   json.loads((backup / filename).read_text()), seccomp,
                                   args.probe, args.evidence_dir, used)
    # Stop every registry consumer before the new binary can migrate its schema.
    # Old brokers cannot read a newer schema, even while members remain on the
    # old image. Restart matching brokers before validating replacement mounts.
    run(['systemctl', 'stop', 'moosuri-kiri-onboard.timer', 'moosuri-kiri-onboard.service',
         'moosuri-kiri-lifecycle.service', 'moosuri-kiri-gateway.service', 'moosuri-kiri-vault.service',
         'moosuri-kiri-model.service', 'moosuri-kiri-gws.service', 'moosuri-kiri-browser.service'])
    checkpoint('services_stopped_keep_lifecycle_stopped_until_config_matches', old_image=config['image'],
               new_image=manifest['images']['member'])
    old = {m['account_id']: inspect(m['account_id']) for m in members}
    roots = {account: Path(next(p['Source'] for p in row['Mounts'] if p['Destination'] == '/data'))
             for account, row in old.items()}
    snapshots = {account: snapshot(path) for account, path in roots.items()}
    # The separate encrypted Vault store must also survive the service update.
    vault_root = Path('/var/lib/moosuri-vault')
    vault_before = snapshot(vault_root)
    (args.evidence_dir / 'data-before.json').write_text(json.dumps(snapshots))
    (args.evidence_dir / 'vault-before.json').write_text(json.dumps(vault_before))
    (args.evidence_dir / 'containers-before.json').write_text(json.dumps(old))
    for filename, value in configs.items():
        checkpoint('installing_matching_' + filename)
        staged = args.evidence_dir / filename
        staged.write_text(json.dumps(value))
        atomic_copy(staged, Path('/etc/moosuri') / filename, 0o600)
        role = 'browser' if filename == 'browser.json' else 'google_auth'
        atomic_copy(args.evidence_dir / (role + '-smoke.json'),
                    Path('/etc/moosuri') / filename.replace('.json', '-smoke.json'), 0o600)
    for name in ['moosuri', 'moosuri-vault-service']:
        checkpoint('installing_matching_binary_' + name)
        atomic_copy(args.bundle / name, Path('/usr/local/bin') / name, 0o755)
    checkpoint('matching_brokers_installed_starting_before_member_rollout')
    run(['systemctl', 'start', 'moosuri-kiri-model.service', 'moosuri-kiri-gws.service',
         'moosuri-kiri-browser.service'])
    service_health(manifest, brokers_only=True)
    replaced = {}
    for member in members:
        account = member['account_id']
        checkpoint('replacing_member_keep_lifecycle_stopped', account=account,
                   old_image=config['image'], new_image=manifest['images']['member'])
        command = [args.bundle / 'moosuri', 'kiri-rollout', '--experimental', '--email', member['email'],
                   '--uid', member['uid'], '--lifecycle-state-directory', '/var/lib/moosuri-kiri-lifecycle',
                   '--expected-container-id', old[account]['Id'], '--old-image', config['image'],
                   '--new-image', manifest['images']['member'], '--start']
        for key in ['registry_directory', 'mounts_root', 'ipc_root', 'allocations_root', 'max_volume_bytes',
                    'memory_bytes', 'nano_cpus', 'pids']:
            if key in config:
                command.extend(['--' + key.replace('_', '-'), config[key]])
        for prefix in ['old', 'new']:
            for key in ['model_sockets_root', 'google_sockets_root', 'browser_sockets_root']:
                if config.get(key):
                    command.extend(['--' + prefix + '-' + key.replace('_', '-'), config[key]])
            vault_origin = member.get('vault_origin') or config.get('vault_origin')
            if vault_origin:
                command.extend(['--' + prefix + '-vault-origin', vault_origin])
            if member.get('enable_local_gws') or config.get('public_origin'):
                command.append('--' + prefix + '-enable-local-gws')
            if member.get('enable_text_preview'):
                command.append('--' + prefix + '-enable-text-preview')
        result = json.loads(run(command))
        assert result['replaced'] and result['readiness'] == 'owner-identified-health-ready'
        after = inspect(account)
        assert after['Id'] != old[account]['Id'] and after['Image'] == manifest['images']['member']
        assert after['Config']['Labels'] == old[account]['Config']['Labels']
        assert after['Config']['User'] == old[account]['Config']['User']
        assert after['Mounts'] == old[account]['Mounts']
        assert snapshot(roots[account]) == snapshots[account], 'member_data_changed'
        assert {p.name: digest(p) for p in Path(config['allocations_root']).glob('uid-*.json')} == allocations
        retry = json.loads(run(command))
        assert not retry['replaced'] and inspect(account) == after, 'replay_replaced_target'
        replaced[account] = after
    print('PASS T03/T04: both different-image replacements preserve UID, keys, database, mounts; retry keeps ID/PID', flush=True)
    checkpoint('members_replaced_installing_matching_lifecycle_config_keep_lifecycle_stopped')
    config['image'] = manifest['images']['member']
    staged = args.evidence_dir / 'kiri-members.json'
    staged.write_text(json.dumps(config))
    atomic_copy(staged, config_path, 0o600)
    assert snapshot(vault_root) == vault_before, 'encrypted_vault_changed_before_service_start'
    checkpoint('matched_config_and_binaries_installed')
    # Existing units/paths remain intact; all now execute the matched bundle.
    run(['systemctl', 'start', 'moosuri-kiri-vault.service'])
    run(['systemctl', 'start', 'moosuri-kiri-lifecycle.service', 'moosuri-kiri-gateway.service',
         'moosuri-kiri-onboard.timer'])
    checkpoint('services_started_health_not_yet_verified')
    service_health(manifest)
    checkpoint('health_verified_checking_preserved_data')
    assert digest(Path('/etc/moosuri/kiri-install.json')) == receipt, 'installer_receipt_changed'
    assert snapshot(vault_root) == vault_before, 'encrypted_vault_changed'
    assert {p: digest(Path(p)) for p in key_hashes} == key_hashes, 'identity_key_changed'
    assert digest(enrolled_path) == digest(backup / 'members.json'), 'enrollment_changed'
    for member in members:
        account = member['account_id']
        assert inspect(account) == replaced[account], 'member_changed_after_lifecycle_resume'
        assert snapshot(roots[account]) == snapshots[account], 'member_data_changed_after_lifecycle_resume'
    (args.evidence_dir / 'complete.json').write_text(json.dumps({
        'source_revision': args.revision, 'members': len(members), 'different_image': True,
        'provider_calls_issued_by_harness': False, 'probe_sha256': digest(args.probe), 'receipt_unchanged': True, 'vault_unchanged': True,
        'bundle_files': manifest['files'], 'member_image': manifest['images']['member'],
        'browser_ui_and_reboot': 'required separately'}, indent=2))
    checkpoint('complete_browser_ui_and_reboot_required_separately')
    print('PASS T05: matched services resume; original receipt, identities and encrypted Vault retained', flush=True)


if __name__ == '__main__':
    if not __debug__:
        raise SystemExit('Run without Python optimization; acceptance assertions are required')
    try:
        main()
    except BaseException as error:
        frames = traceback.extract_tb(error.__traceback__)
        frame = next((f for f in reversed(frames) if Path(f.filename).name == Path(__file__).name), frames[-1])
        reason = str(error) if isinstance(error, RuntimeError) or re.fullmatch('[a-z0-9_]+', str(error)) else ''
        if EVIDENCE is not None:
            (EVIDENCE / 'failed.json').write_text(json.dumps({'line': frame.lineno, 'reason': reason,
                'last_phase': json.loads((EVIDENCE / 'phase.json').read_text()),
                'recovery': 'Inspect phase.json and both member images before starting lifecycle. '
                            'Created proxy/network names are retained for owned inspection. No automatic rollback.'}))
        print(f'FAIL matched-rollout: {type(error).__name__} at {frame.lineno}: ' + reason, file=sys.stderr)
        raise SystemExit(1)
