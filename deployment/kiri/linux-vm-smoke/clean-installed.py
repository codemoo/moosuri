#!/usr/bin/env python3
"""Exercise an actual setup-kiri result in a marked disposable Ubuntu guest.

No service configuration is fabricated here. The source installer must have
created the installation, first member, Vault, browser and invitation timer.
Synthetic test password is read from the fixture's root-private file only.
"""
import hashlib
import http.client
from http.cookies import SimpleCookie
import json
import os
from pathlib import Path
import secrets
import socket
import ssl
import subprocess
import sys
import time
import traceback
from uuid import uuid4

ETC = Path('/etc/moosuri')
CONTROL = Path('/var/lib/moosuri-control')
BIN = '/usr/local/bin/moosuri'
ORIGIN = 'https://assistant.example.com'
VAULT = 'https://vault.example.com'
REBOOT_STATE = Path('/root/moosuri-clean-acceptance-state.json')


def run(args, secret=None, timeout=90):
    return subprocess.run([str(x) for x in args], input=secret, text=True,
                          capture_output=True, timeout=timeout)


def until(check, seconds=90):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = check()
        if value:
            return value
        time.sleep(1)
    raise AssertionError('bounded_wait_failed')


class LocalTLS(http.client.HTTPSConnection):
    def connect(self):
        self.sock = self._context.wrap_socket(
            socket.create_connection(('127.0.0.1', 443), timeout=20),
            server_hostname=self.host)


def request(host, path, method='GET', body=None, jar=None, account=None, form=False):
    headers = {'Host': host, 'Sec-Fetch-Site': 'same-origin'}
    if jar:
        headers['Cookie'] = jar.output(header='', sep=';').strip()
    if account:
        headers['X-Moosuri-Account' if host.startswith('assistant.') else 'X-Moosuri-Vault-Account'] = account
    if method == 'POST':
        headers['Origin'] = ORIGIN if form or host.startswith('assistant.') else VAULT
        headers['Content-Type'] = 'application/x-www-form-urlencoded' if form else 'application/json'
        body = body if form else json.dumps(body)
    tls = ssl.create_default_context(cafile='/root/moosuri-install-tls/cert.pem')
    c = LocalTLS(host, context=tls, timeout=20)
    try:
        c.request(method, path, body=body, headers=headers)
        response = c.getresponse()
        if jar is not None:
            for name, value in response.getheaders():
                if name.lower() == 'set-cookie':
                    for k, v in SimpleCookie(value).items():
                        jar[k] = v.value
        raw = response.read(2 << 20)
        return response.status, json.loads(raw) if raw.startswith(b'{') else raw
    finally:
        c.close()


def app(path, method='GET', body=None, jar=None, account=None):
    return request('assistant.example.com', '/api/v1/' + path, method, body, jar, account)


def login(email, password):
    jar = SimpleCookie()
    code, data = app('auth/login', 'POST', {'email': email, 'password': password}, jar)
    assert code == 200, ('login', code)
    return data, jar


def vault_session(jar, account):
    code, handoff = app('personal/vault/handoff', 'POST', {}, jar, account)
    assert code == 200
    vault = SimpleCookie()
    code, _ = request('vault.example.com', '/session', 'POST',
                      'handoff=' + handoff['token'], vault, form=True)
    assert code == 303
    code, state = request('vault.example.com', '/session/current', jar=vault)
    assert code == 200 and state['owner']['account'] == account
    assert request('vault.example.com', '/session', 'POST',
                   'handoff=' + handoff['token'], form=True)[0] == 401
    return vault


def passed(text):
    print('PASS ' + text, flush=True)


def fixture_only():
    assert os.geteuid() == 0 and sys.platform == 'linux'
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip() == 'SYNTHETIC_DISPOSABLE_VM'
    os.umask(0o077)


def after_reboot():
    fixture_only()
    state = json.loads(REBOOT_STATE.read_text())
    assert Path('/proc/sys/kernel/random/boot_id').read_text().strip() != state['boot_id']
    for path, digest in state['files'].items():
        assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest
    for service in ('moosuri-kiri-model.service', 'moosuri-kiri-onboard.timer', 'nginx.service'):
        assert run(['systemctl', 'is-active', '--quiet', service]).returncode == 0
    accounts = []
    for member in state['members']:
        view, jar = login(member['email'], member['password'])
        account = view['user']['id']
        assert account == member['account']
        accounts.append(account)
        vault_session(jar, account)
        assert app('personal/admin/members', jar=jar, account=account)[0] == member['admin_status']
    assert len(set(accounts)) == 2
    assert not (CONTROL / 'model-setup/runtime-model.json').exists()
    passed('CI-07 actual reboot retains both member identities, Vault handoff, admin boundary and unconfigured model')


def main(bundle):
    fixture_only()
    receipt = json.loads((ETC / 'kiri-install.json').read_text())
    assert receipt['complete'] and receipt['admin_email'] == 'install-test@example.com'
    assert receipt['public_origin'] == ORIGIN and receipt['vault_origin'] == VAULT
    password = Path('/root/moosuri-install-test.password').read_text().strip()
    for host in ('assistant.example.com', 'vault.example.com'):
        assert request(host, '/')[0] == 200
    admin, jar = login(receipt['admin_email'], password)
    assert not admin['password_change_required'] and not admin['user']['must_change_password']
    account = admin['user']['id']
    assert app('personal/admin/members', jar=jar, account=account)[0] == 200
    code, model_setup = app('personal/admin/engine/setup', jar=jar, account=account)
    assert code == 200 and model_setup == {'supported': True, 'saved': False}
    assert run(['systemctl', 'is-active', '--quiet', 'moosuri-kiri-model.service']).returncode == 0
    assert not (CONTROL / 'model-setup/runtime-model.json').exists()
    first_vault = vault_session(jar, account)
    passed('CI-02 HTTPS assistant/Vault, authenticated administrator and single-use Vault handoff')

    setup = [BIN, 'setup-kiri', '--bundle', bundle, '--public-origin', ORIGIN,
             '--vault-origin', VAULT, '--admin-name', receipt['admin_name'],
             '--admin-email', receipt['admin_email'], '--member-capacity', str(receipt['member_capacity']),
             '--member-volume-gib', str(receipt['member_volume_bytes'] >> 30),
             '--tls-certificate', '/root/moosuri-install-tls/cert.pem',
             '--tls-key', '/root/moosuri-install-tls/key.pem', '--password-stdin']
    before = {p: p.read_bytes() for p in (ETC / 'kiri-install.json', ETC / 'vault-adapter.key',
                                          CONTROL / 'enrolled/members.json')}
    assert run(setup, password + '\n').returncode == 0
    assert all(p.read_bytes() == value for p, value in before.items())
    changed = setup.copy()
    changed[changed.index(ORIGIN)] = 'https://changed.example.com'
    denied = run(changed, password + '\n')
    assert denied.returncode != 0 and 'conflicting_existing_installation' in denied.stderr
    assert all(p.read_bytes() == value for p, value in before.items())
    passed('CI-03/04 repeat retains identities and authority; changed origin is refused without mutation')

    # The installed timer must prepare the new member without any service edits.
    invitation = str(uuid4())
    code, response = app('personal/admin/invitations', 'POST',
        {'request_id': invitation, 'name': 'Second Install Tester', 'email': 'second-install@example.com'}, jar, account)
    assert code == 200 and response['request_id'] == invitation
    record = CONTROL / 'invitations' / (invitation + '.json')
    until(lambda: record.exists() and json.loads(record.read_text())['stage'] == 'password_required', 155)
    second_password = secrets.token_urlsafe(24)
    result = run([BIN, 'kiri-onboard-worker', '--experimental', '--config', ETC / 'kiri-members.json',
        '--policy', ETC / 'kiri-onboard-policy.json', '--operations-root', CONTROL / 'invitations',
        '--finish-request', invitation, '--password-stdin'], second_password + '\n', 180)
    assert result.returncode == 0 and json.loads(result.stdout)['ready']
    second, jar2 = login('second-install@example.com', second_password)
    account2 = second['user']['id']
    assert second['password_change_required'] and account2 != account
    replacement = secrets.token_urlsafe(24)
    assert app('auth/password', 'POST', {'password': replacement}, jar2, account2)[0] == 200
    second_vault = vault_session(jar2, account2)
    enrolled = json.loads((CONTROL / 'enrolled/members.json').read_text())['members']
    assert len(enrolled) == 2 and len({m['uid'] for m in enrolled}) == 2
    for m in enrolled:
        name = 'moosuri-member-' + m['account_id']
        result = run(['docker', '--host=unix:///run/kiri/docker.sock', 'exec',
                      '--user', f"{m['uid']}:{m['uid']}", name, '/usr/local/bin/gws', '--version'])
        assert result.returncode == 0 and result.stdout.splitlines()[0] == 'gws 0.22.0'
    assert app('personal/admin/members', jar=jar2, account=account2)[0] == 403
    assert app('personal/admin/engine/setup', jar=jar2, account=account2)[0] == 403
    assert app('personal/admin/engine/setup', 'POST', {
        'request_id': str(uuid4()), 'api_key': 'synthetic-never-dispatched',
        'model': 'claude-sonnet-5-5', 'effort': 'medium',
    }, jar2, account2)[0] == 403
    assert app('personal/admin/engine/setup', jar=jar, account=account)[1] == {'supported': True, 'saved': False}
    assert not (CONTROL / 'model-setup/runtime-model.json').exists()
    passed('CI-05 installed invitation timer, independent member UID and GWS; second member is not admin')
    passed('I02/M01 model service starts unconfigured; only administrator can open setup; member writes denied')

    sessions = [(jar, account, first_vault), (jar2, account2, second_vault)]
    jobs = [str(uuid4()), str(uuid4())]
    def google(i, op, **fields):
        j, a, _ = sessions[i]
        return app('personal/google/kiri', 'POST', dict(op=op, job=jobs[i], **fields), j, a)
    for i in range(2):
        assert google(i, 'start', service='gmail')[0] == 200
    recipients = []
    routes = []
    for i in range(2):
        def prepared():
            code, state = google(i, 'status')
            assert code == 200 and state['state'] not in {'unavailable', 'blocked', 'cancelled'}
            return state if state['state'] == 'vault' else None
        ready = until(prepared, 60)
        route = '/session/kiri-recipient?challenge=' + ready['challenge']
        code, recipient = request('vault.example.com', route, jar=sessions[i][2])
        assert code == 200 and recipient['binding']['account'] == sessions[i][1]
        recipients.append(recipient)
        routes.append(route)
    assert recipients[0]['spki'] != recipients[1]['spki']
    for i in range(2):
        assert request('vault.example.com', routes[i], jar=sessions[1-i][2])[0] == 503
    assert google(0, 'cancel')[0] == 200
    assert request('vault.example.com', routes[0], jar=first_vault)[0] == 503
    assert request('vault.example.com', routes[1], jar=second_vault)[0] == 200
    assert app('auth/logout', 'POST', {}, jar2, account2)[0] == 204
    assert request('vault.example.com', routes[1], jar=second_vault)[0] == 401
    passed('CI-06 actual separate Kiri recipients, cross-member denial, cancellation and logout')

    journal = run(['journalctl', '--no-pager', '-u', 'moosuri-*'], timeout=30).stdout
    argv = run(['ps', '-eo', 'args']).stdout
    metadata = b'\n'.join(p.read_bytes() for p in ETC.glob('*.json'))
    for secret in (password, second_password, replacement):
        assert secret not in journal and secret not in argv and secret.encode() not in metadata
    for name in ('browser', 'google-browser'):
        cfg = json.loads((ETC / (name + '.json')).read_text())
        evidence = (ETC / (name + '-smoke.json')).read_bytes()
        assert cfg['verified'] and cfg['smoke_evidence_sha256'] == hashlib.sha256(evidence).hexdigest()
    passed('CI-08 no synthetic secret in argv/journal/receipt; actual browser evidence matches configs')
    # Synthetic fixture credentials only, retained root-private for the explicit
    # post-reboot phase. Never usable on an unmarked installation or printed.
    state = {
        'boot_id': Path('/proc/sys/kernel/random/boot_id').read_text().strip(),
        'files': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in before},
        'members': [
            {'email': receipt['admin_email'], 'password': password, 'account': account, 'admin_status': 200},
            {'email': 'second-install@example.com', 'password': replacement, 'account': account2, 'admin_status': 403},
        ],
    }
    fd = os.open(REBOOT_STATE, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'w') as output:
        json.dump(state, output)
        output.flush()
        os.fsync(output.fileno())


if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['--after-reboot']:
            after_reboot()
        else:
            main(Path(sys.argv[1]).resolve())
    except Exception as error:
        # Do not render HTTP response bodies, cookies, process logs or credentials.
        frame = traceback.extract_tb(error.__traceback__)[-1]
        print(f'FAIL clean-install scenario: {type(error).__name__} at {Path(frame.filename).name}:{frame.lineno}', file=sys.stderr)
        raise SystemExit(1)
