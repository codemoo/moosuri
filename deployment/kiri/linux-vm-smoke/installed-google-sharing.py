#!/usr/bin/env python3
"""Installed shared-app distribution, replay and recovery with synthetic secrets.

Run as root only in the marked disposable two-member installation. This registers
a synthetic app permanently in that fixture; it never contacts Google, starts
consent, imports tokens or claims real OAuth success. Existing app state is refused.
Only verdicts and binary hashes leave the private output directory. Failure keeps
evidence and the created fixture; never reset the installation to retry.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import time
import traceback

BASE = 'personal/admin/google-setup'
DOCKER = ['docker', '--host', 'unix:///run/kiri/docker.sock']


def main():
    assert os.geteuid() == 0
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip() == 'SYNTHETIC_DISPOSABLE_VM'
    os.umask(0o077)
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--resume', action='store_true',
                        help='Resume only the sixth journey after five recorded passes; preserve prior failure')
    args = parser.parse_args()
    args.output_dir.mkdir(mode=0o700, exist_ok=args.resume)
    prior = None
    if args.resume:
        old = args.output_dir / 'result.json'
        prior = json.loads(old.read_text())
        assert len(prior['passed']) == 5 and prior.get('failure', {}).get('type') == 'TypeError'
        assert prior.get('registration_checkpoint'), 'legacy_run_has_no_cross_run_state_proof'
        assert prior['synthetic_app'] and prior['fault_permissions_restored']
        assert prior['provider_authentication'] is False and prior['real_mail_or_tokens'] is False
        (args.output_dir / ('result-prior-' + str(time.time_ns()) + '.json')).write_bytes(old.read_bytes())
    spec = importlib.util.spec_from_file_location('fixture', args.fixture)
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    members = json.loads(fixture.REBOOT_STATE.read_text())['members']
    assert len(members) == 2 and members[0]['email'] == 'install-test@example.com'
    config = json.loads(Path('/etc/moosuri/kiri-members.json').read_text())
    client_file = Path(config['registry_directory']) / 'google-web-client.json'
    assert args.resume or not client_file.exists(), 'preserve_existing_app'
    report = {'passed': [], 'provider_authentication': False, 'model_inference': False,
              'synthetic_app': True, 'real_mail_or_tokens': False}

    def command(argv):
        result = subprocess.run(argv, capture_output=True, timeout=45)
        assert result.returncode == 0, 'fixture_command_failed'
        return result.stdout

    def binary_hash(unit):
        pid = command(['systemctl', 'show', unit, '-p', 'MainPID', '--value']).decode().strip()
        with open('/proc/' + pid + '/exe', 'rb') as stream:
            return hashlib.file_digest(stream, 'sha256').hexdigest()

    report['harness_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    report['fixture_sha256'] = hashlib.sha256(args.fixture.read_bytes()).hexdigest()
    report['gateway_sha256'] = binary_hash('moosuri-kiri-gateway')
    report['gws_relay_sha256'] = binary_hash('moosuri-kiri-gws')
    jars = [fixture.login(m['email'], m['password'])[1] for m in members]
    secret = (json.loads(client_file.read_text())['web']['client_secret']
              if args.resume else secrets.token_urlsafe(32))
    callback = fixture.ORIGIN + '/api/v1/mail/oauth/callback'
    client = {'web': {'client_id': 'synthetic-sharing.apps.googleusercontent.com',
                      'client_secret': secret, 'redirect_uris': [callback]}}
    upload = {'web_client_json': json.dumps(client)}
    expected = {'configured': True, 'redirect_uri': callback}
    paths, names = [], []
    for member in members:
        name = 'moosuri-member-' + member['account']
        data = json.loads(command(DOCKER + ['inspect', name]))[0]
        source = Path(next(m['Source'] for m in data['Mounts'] if m['Destination'] == '/data'))
        assert source.is_relative_to(Path(config['mounts_root']))
        assert args.resume or not list(source.rglob('gws-client.json')), 'preserve_member_registration'
        paths.append(source)
        names.append(name)
    report['member_image_ids'] = [json.loads(command(DOCKER + ['inspect', name]))[0]['Image'] for name in names]
    if prior:
        for key in ('gateway_sha256', 'gws_relay_sha256', 'member_image_ids', 'fixture_sha256'):
            assert report[key] == prior[key], 'resume_runtime_changed'
        report['passed'] = prior['passed']
        report['resumed_failure'] = prior['failure']
        report['prior_harness_sha256'] = prior['harness_sha256']
    restored = True

    def save():
        (args.output_dir / 'result.json').write_text(json.dumps(report))

    def passed(name):
        report['passed'].append(name)
        save()
        print('PASS ' + name, flush=True)

    def app(index, path, body=None):
        result = fixture.app(path, 'GET' if body is None else 'POST', body,
                             jars[index], members[index]['account'])
        body = result[1]
        encoded = body if isinstance(body, bytes) else json.dumps(body).encode()
        assert secret.encode() not in encoded, 'secret_in_public_response'
        return result

    def registered():
        files = [list(path.rglob('gws-client.json')) for path in paths]
        if any(len(items) != 1 for items in files):
            return False
        return [items[0] for items in files]

    def disconnected(index):
        return (app(index, 'personal/google') == (200, {'connection': None})
                and app(index, 'personal/google/workspace') == (200, {'connections': []}))

    def checkpoint(files):
        return {str(p): {'sha256': hashlib.sha256(p.read_bytes()).hexdigest(),
                         'inode': p.stat().st_ino, 'uid': p.stat().st_uid,
                         'mode': p.stat().st_mode & 0o777} for p in files}

    try:
        def unchanged():
            assert all((p.read_bytes(), p.stat().st_ino) == value for p, value in before.items())

        if args.resume:
            files = registered()
            assert files and json.loads(client_file.read_text()) == client
            assert all(json.loads(p.read_text())['web_client'] == client for p in files)
            assert all(p.stat().st_mode & 0o777 == 0o600 for p in files + [client_file])
            assert checkpoint(files + [client_file]) == prior['registration_checkpoint'], 'registration_changed_since_prior_run'
            before = {p: (p.read_bytes(), p.stat().st_ino) for p in files + [client_file]}
            report['registration_checkpoint'] = prior['registration_checkpoint']
        else:
            assert fixture.app(BASE)[0] == 401
            assert app(1, BASE)[0] == 403 and app(1, BASE, upload)[0] == 403
            assert fixture.app(BASE, jar=jars[1], account=members[0]['account'])[0] == 401
            assert app(0, BASE) == (200, {'configured': False, 'redirect_uri': callback})
            invalid = {'web': dict(client['web'], redirect_uris=[callback + '?wrong'])}
            assert app(0, BASE, {'web_client_json': json.dumps(invalid)}) == (400, {'code': 'google_redirect_uri_mismatch'})
            assert not client_file.exists()
            passed('GA01-only-admin-and-exact-callback-can-register')

            assert app(0, BASE, upload) == (200, expected)
            files = fixture.until(registered, 40)
            records = [json.loads(p.read_text()) for p in files]
            assert all(r['web_client'] == client and r['redirect_uri'] == callback for r in records)
            assert records[0]['capability_key'] != records[1]['capability_key']
            assert records[0]['app_revision'] != records[1]['app_revision']
            assert files[0].stat().st_uid != files[1].stat().st_uid
            assert all(p.stat().st_mode & 0o777 == 0o600 for p in files + [client_file])
            assert disconnected(0) and disconnected(1), 'app_registration_must_not_claim_member_consent'
            before = {p: (p.read_bytes(), p.stat().st_ino) for p in files + [client_file]}
            report['registration_checkpoint'] = checkpoint(files + [client_file])
            passed('GA02-running-members-receive-shared-app-with-distinct-private-authority')

            # Treat the successful first POST as a lost reply: reconcile GET then replay.
            assert app(0, BASE) == (200, expected)
            assert app(0, BASE, upload) == (200, expected)
            for field, value in [('client_id', 'conflicting.apps.googleusercontent.com'),
                                 ('client_secret', secrets.token_urlsafe(32))]:
                conflict = {'web': dict(client['web'], **{field: value})}
                assert app(0, BASE, {'web_client_json': json.dumps(conflict)}) == (409, {'code': 'google_setup_already_configured'})
            unchanged()
            passed('GA03-lost-reply-replay-and-conflicts-preserve-both-members')

            command(DOCKER + ['stop', names[0]])
            try:
                assert disconnected(1), 'sibling_service_was_interrupted'
            finally:
                command(DOCKER + ['start', names[0]])
            fixture.until(lambda: disconnected(0), 35)
            unchanged()
            passed('GA04-one-member-restart-retains-registration-and-sibling-service')

            # Deliberately make the source app unreadable to its strict private-file
            # contract; never replace its bytes or generate another app.
            client_file.chmod(0o640)
            restored = False
            try:
                assert app(0, BASE) == (503, {'code': 'google_setup_storage_unavailable'})
                assert app(0, BASE, upload)[0] == 503
                assert disconnected(0) and disconnected(1)
            finally:
                client_file.chmod(0o600)
                restored = True
            assert app(0, BASE) == (200, expected)
            unchanged()
            passed('GA05-invalid-source-permissions-preserve-existing-members-and-recover')

        command(['systemctl', 'restart', 'moosuri-kiri-gws', 'moosuri-kiri-gateway'])
        fixture.until(lambda: app(0, BASE) == (200, expected), 35)
        fixture.until(lambda: disconnected(0) and disconnected(1), 35)
        unchanged()
        assert fixture.app('personal/google', jar=jars[0], account=members[1]['account'])[0] == 401
        assert fixture.app('personal/google', jar=jars[1], account=members[0]['account'])[0] == 401
        journal = command(['journalctl', '--no-pager', '-u', 'moosuri-*'])
        argv = command(['ps', '-eo', 'args'])
        assert secret.encode() not in journal and secret.encode() not in argv
        passed('GA06-service-restart-retains-private-registration-without-false-consent')
    except Exception as error:
        frame = next((f for f in reversed(traceback.extract_tb(error.__traceback__))
                      if f.filename == __file__), None)
        report['failure'] = {'type': type(error).__name__, 'line': frame.lineno if frame else None}
        raise
    finally:
        report['fault_permissions_restored'] = restored
        save()


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print('FAIL installed-google-sharing: ' + type(error).__name__, flush=True)
        raise SystemExit(1)
