#!/usr/bin/env python3
"""Installed disconnect/recovery acceptance with a SYNTHETIC Active fixture.

The public API prepares a real isolated login environment and official URL.
With the gateway stopped, this harness marks that unauthenticated environment
Active and supplies a synthetic binding. No account login, inference or real
credential is involved. Never run outside the marked disposable installation.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time
import traceback
import uuid

BASE = 'personal/admin/engine/subscription'
SETUP = Path('/var/lib/moosuri-control/model-setup')
CONFIG = SETUP / 'runtime-model.json'
RECEIPT = SETUP / 'claude-subscription-operation.json'
RECORDS = Path('/var/lib/moosuri-claude-control/connections')
ROOT = Path('/root/moosuri-installed-claude-lifecycle')
UNIT = 'moosuri-kiri-gateway'


def durable(path, data):
    value = data if isinstance(data, bytes) else json.dumps(data).encode()
    temp = path.with_name(path.name + '.fixture-next')
    with open(temp, 'xb') as stream:
        stream.write(value)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temp, path)
    fd = os.open(path.parent, os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def service(action):
    subprocess.run(['systemctl', action, UNIT], check=True, timeout=45,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def main():
    assert os.geteuid() == 0
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip() == 'SYNTHETIC_DISPOSABLE_VM'
    os.umask(0o077)
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture', required=True, type=Path)
    parser.add_argument('--stage-active-only', action='store_true')
    parser.add_argument('--resume-held', action='store_true', help='Continue D04-D06 from the retained corrupt-config failure')
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('fixture', args.fixture)
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    members = json.loads(fixture.REBOOT_STATE.read_text())['members']
    assert len(members) == 2 and members[0]['email'] == 'install-test@example.com'
    ROOT.mkdir(mode=0o700, exist_ok=True)
    if (ROOT/'result.json').exists():
        (ROOT/('prior-'+str(time.time_ns())+'.json')).write_bytes((ROOT/'result.json').read_bytes())
    report = {'checks': [], 'synthetic_active_fixture': True,
              'provider_login': False, 'inference': False}
    pid = subprocess.check_output(['systemctl','show',UNIT,'-p','MainPID','--value'], text=True).strip()
    with open('/proc/'+pid+'/exe', 'rb') as stream:
        report['candidate_sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
    _, jar = fixture.login(members[0]['email'], members[0]['password'])
    _, member_jar = fixture.login(members[1]['email'], members[1]['password'])
    account = members[0]['account']

    def call(body=None, cookies=None, owner=None):
        return fixture.app(BASE, 'GET' if body is None else 'POST', body,
                           jar if cookies is None else cookies, owner or account)

    def passed(name):
        report['checks'].append({'journey': name, 'status': 'passed'})
        durable(ROOT/'result.json', report)
        print('PASS '+name, flush=True)

    def poll(ident, states, seconds=240):
        end = time.monotonic()+seconds
        while time.monotonic() < end:
            status, reply = call({'op':'status', 'request_id':ident})
            assert status == 200, 'status_http_failure'
            if reply['operation']['state'] in states:
                return reply['operation']
            time.sleep(1)
        raise AssertionError('lifecycle_deadline')

    def own_record():
        found = [(p, json.loads(p.read_text())) for p in RECORDS.glob('*.json')]
        found = [(p, r) for p, r in found if r['owner']['account_id'] == account]
        assert len(found) == 1
        return found[0]

    def synthetic_config(rec):
        ident = rec['attempt']['request_id']
        owner = rec['owner']
        uid = rec['evidence']['runtime']['service']['uid']
        config = {'version':1, 'connection_revision':ident,
                  'provider':{'kind':'anthropic', 'base_url':None, 'allowed_ips':[],
                              'router_providers':[], 'zero_data_retention':False},
                  'model':'claude-sonnet-5-5', 'effort':'medium', 'models':[],
                  'key_file':None, 'subscription':'claude_code',
                  'bindings':[dict(owner, peer_uid=250001,
                      budget={'max_calls':100, 'max_tokens':500000, 'reservation_tokens':16384},
                      worker={'socket':f'/run/moosuri-claude-worker-{uid}/engine.sock','worker_uid':uid})]}
        return json.dumps(config).encode()

    def active_fixture():
        assert not CONFIG.exists(), 'prior_config_present'
        ident = str(uuid.uuid4())
        payload = {'op':'start', 'request_id':ident, 'email':'synthetic-owner@example.test',
                   'model':'claude-sonnet-5-5', 'effort':'medium', 'hosting_terms_accepted':True}
        assert call(payload)[0] == 200
        op = poll(ident, ['awaiting_code'])
        assert op.get('authorization_url', '').startswith('https://claude.com/cai/oauth/authorize?')
        service('stop')
        path, rec = own_record()
        assert rec['attempt']['request_id'] == ident and rec['attempt']['phase'] == 'login_issued'
        # This explicit fixture transition is NOT proof of authentication or a
        # live model worker. Actual installed cleanup still uses protected inputs.
        rec['attempt']['phase'] = 'active'
        durable(path, rec)
        config_bytes = synthetic_config(rec)
        receipt = json.loads(RECEIPT.read_text())
        assert receipt['request_id'] == ident
        receipt.update(state='ready', config_sha256=hashlib.sha256(config_bytes).hexdigest())
        durable(CONFIG, config_bytes)
        durable(RECEIPT, receipt)
        service('start')
        fixture.until(lambda: call()[0] == 200, 30)
        return ident, path, rec, config_bytes

    def payload(target):
        return {'op':'disconnect', 'request_id':str(uuid.uuid4()), 'replace_revision':target}

    def retired(path, rec):
        actual = json.loads(path.read_text())
        assert actual['attempt']['request_id'] == rec['attempt']['request_id']
        assert actual['attempt']['phase'] == 'retired'
        assert actual['runtime_request_id'] == rec['runtime_request_id']
        assert not CONFIG.exists()
        env = rec['attempt']['environment_id']
        inputs = Path('/var/lib/moosuri-claude-config')/env
        assert not inputs.exists() or not list(inputs.iterdir())
        assert json.loads(RECEIPT.read_text())['state'] == 'disconnected'
        status, engine = fixture.app('personal/admin/engine', 'GET', None, jar, account)
        assert status == 200 and engine['configured'] is False

    try:
        if args.resume_held:
            path, rec = own_record()
            saved = json.loads(RECEIPT.read_text())
            assert saved.get('mode') == 'disconnect' and saved['state'] in ['disconnecting', 'cleanup_pending']
            assert rec['attempt']['phase'] == 'active'
            assert saved['target_revision'] == rec['attempt']['request_id']
            initial = synthetic_config(rec)
            assert hashlib.sha256(initial).hexdigest() == saved['config_sha256']
            assert CONFIG.read_bytes() == b'{invalid synthetic config'
            op = {'request_id':saved['request_id']}
            report['resumed_from_retained_failure'] = True
        else:
            target, path, rec, initial = active_fixture()
            if args.stage_active_only:
                passed('D00-synthetic-active-fixture-prepared-for-ui')
                return
            op = payload(target)
            assert call(op, member_jar, members[1]['account'])[0] == 403
            assert call(dict(op, replace_revision=str(uuid.uuid4())))[0] == 409
            assert CONFIG.read_bytes() == initial
            assert json.loads(path.read_text())['attempt']['phase'] == 'active'
            passed('D01-member-and-stale-target-refused-without-mutation')
            assert call(op)[0] == 200
            # Deliberately discard the first operation body; replay the same ID.
            assert call(op)[0] == 200
            poll(op['request_id'], ['disconnected'])
            retired(path, rec)
            passed('D02-disconnect-lost-response-replay-and-exact-retirement')
            old_runtime = rec['runtime_request_id']
            old_env = rec['attempt']['environment_id']
            target, path, rec, initial = active_fixture()
            assert rec['runtime_request_id'] == old_runtime
            assert rec['attempt']['environment_id'] != old_env
            passed('D03-reconnect-reuses-owner-runtime-with-fresh-login-environment')
            # A real external flock interrupts the root mark after durable unpublish.
            lease = open(path.with_suffix('.lock'), 'r+')
            fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
            op = payload(target)
            assert call(op)[0] == 200
            fixture.until(lambda: not CONFIG.exists(), 30)
            assert json.loads(RECEIPT.read_text()).get('config_sha256')
            assert json.loads(path.read_text())['attempt']['phase'] == 'active'
            assert call({'op':'start','request_id':str(uuid.uuid4()),'email':'synthetic-owner@example.test',
                         'model':'claude-sonnet-5-5','effort':'medium','hosting_terms_accepted':True})[0] == 409
            service('stop')
            lease.close()
            # At restart, an unreadable config must hold rather than delete inputs.
            durable(CONFIG, b'{invalid synthetic config')
            service('start')
        fixture.until(lambda: call()[0] == 200, 30)
        _, jar = fixture.login(members[0]['email'], members[0]['password'])
        assert call({'op':'cancel','request_id':op['request_id']})[0] == 200
        poll(op['request_id'], ['cleanup_pending'], 30)
        # Observe beyond the entire bounded drive retry window, not just the
        # orphan's initial cleanup_pending view before its first step executes.
        time.sleep(14)
        assert call({'op':'status','request_id':op['request_id']})[1]['operation']['state'] == 'cleanup_pending'
        assert CONFIG.read_bytes() == b'{invalid synthetic config'
        held = json.loads(RECEIPT.read_text())
        assert held['state'] == 'cleanup_pending' and held['target_revision'] == rec['attempt']['request_id']
        assert held['config_sha256'] == hashlib.sha256(initial).hexdigest()
        assert json.loads(path.read_text())['attempt']['phase'] == 'active'
        passed('D04-restart-and-corrupt-config-hold-owned-environment')
        service('stop')
        durable(CONFIG, initial)
        service('start')
        fixture.until(lambda: call()[0] == 200, 30)
        assert call({'op':'cancel','request_id':op['request_id']})[0] == 200
        poll(op['request_id'], ['disconnected'])
        retired(path, rec)
        passed('D05-new-session-recovers-original-target-after-exact-config-restored')
        service('restart')
        fixture.until(lambda: call()[0] == 200, 30)
        assert call()[1]['operation']['state'] == 'disconnected'
        retired(path, rec)
        passed('D06-disconnected-state-survives-gateway-restart')
    except Exception as error:
        frame = traceback.extract_tb(error.__traceback__)[-1]
        report['failure'] = {'type':type(error).__name__, 'line':frame.lineno}
        durable(ROOT/'result.json', report)
        print('FAIL lifecycle '+type(error).__name__+' line '+str(frame.lineno), flush=True)
        raise SystemExit(1)


if __name__ == '__main__':
    main()
