#!/usr/bin/env python3
"""Installed Kiri Claude journeys without entering an actual provider account.

Run only in clean-installed.py's marked guest. Never prints/stores sessions,
URLs, frames, typed text or codes. A visible login page is not authentication.
"""
import argparse
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
ROOT = Path('/root/moosuri-installed-claude-kiri')


def main():
    assert os.geteuid() == 0
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip() == 'SYNTHETIC_DISPOSABLE_VM'
    os.umask(0o077)
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture', required=True, type=Path)
    parser.add_argument('--recover', action='store_true')
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('fixture', args.fixture)
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    members = json.loads(fixture.REBOOT_STATE.read_text())['members']
    assert [m['email'] for m in members] == ['install-test@example.com', 'second-install@example.com']
    ROOT.mkdir(mode=0o700, exist_ok=args.recover)
    if (ROOT/'result.json').exists():
        (ROOT/('result-prior-'+str(time.time_ns())+'.json')).write_bytes((ROOT/'result.json').read_bytes())
    report = {'checks': [], 'provider_authentication': False, 'real_callback': False, 'inference': False}
    pid = subprocess.check_output(['systemctl', 'show', 'moosuri-kiri-gateway', '-p', 'MainPID', '--value'], text=True).strip()
    with open('/proc/'+pid+'/exe', 'rb') as binary:
        report['candidate_sha256'] = hashlib.file_digest(binary, 'sha256').hexdigest()
    _, jar = fixture.login(members[0]['email'], members[0]['password'])
    _, foreign = fixture.login(members[1]['email'], members[1]['password'])
    _, sibling = fixture.login(members[0]['email'], members[0]['password'])
    def call(op=None, session=jar, account=members[0]['account']):
        result = fixture.app(BASE, 'GET' if op is None else 'POST', op, session, account)
        # The private URL and callback are never part of this public API.
        body = json.dumps(result[1])
        assert 'authorization_url' not in body and 'oauth/authorize?' not in body
        assert 'oauth/code/callback?' not in body
        return result
    def record(name):
        report['checks'].append({'journey': name, 'status': 'passed'})
        (ROOT/'result.json').write_text(json.dumps(report))
        print('PASS '+name, flush=True)
    def poll(ident, states):
        end = time.monotonic()+240
        while time.monotonic() < end:
            status, data = call({'op': 'status', 'request_id': ident})
            assert status == 200
            op = data['operation']
            if op['state'] in states:
                if op['state'] != 'awaiting_code' or op.get('browser_available'): return op
            assert op['state'] not in ['failed', 'cancelled'], 'unexpected_terminal'
            time.sleep(1)
        raise AssertionError('connection_deadline')
    def start():
        ident = str(uuid.uuid4())
        body = {'op': 'start', 'request_id': ident, 'email': 'synthetic-owner@example.test',
                'model': 'claude-sonnet-5-5', 'effort': 'medium', 'hosting_terms_accepted': True}
        assert call(body)[0] == 200
        (ROOT/'request.json').write_text(json.dumps({'request_id': ident}))
        poll(ident, ['awaiting_code'])
        return ident
    def frame(ident):
        end = time.monotonic()+20
        while time.monotonic() < end:
            status, data = call({'op': 'frame', 'request_id': ident})
            if status == 200 and data.get('frame'):
                f = data['frame']
                assert f['width'] == 1280 and f['height'] == 900
                assert len(f['png_base64']) > 100 and f['frame_token']
                from urllib.parse import urlsplit
                loc = urlsplit(f['url'])
                assert loc.scheme == 'https' and loc.netloc and not loc.query and not loc.fragment
                assert loc.path in ('', '/')
                return f
            assert status in (200, 409, 503)
            time.sleep(0.5)
        raise AssertionError('frame_deadline')
    config = json.loads(Path('/etc/moosuri/google-browser.json').read_text())
    prefix = 'moosuri-google-'+config['owner'].replace('-', '')+'-'
    def no_browser():
        result = subprocess.run(['docker', '--host', 'unix:///run/kiri/docker.sock', 'container', 'ls', '--all',
                                 '--format', '{{.Names}}', '--filter', 'name=^/'+prefix+'[01]$'],
                                capture_output=True, timeout=10, check=True)
        return not result.stdout.strip()
    setup = Path('/var/lib/moosuri-control/model-setup')
    before = {p.name: p.read_bytes() for p in setup.iterdir() if p.is_file() and 'claude-subscription' not in p.name}
    def unchanged():
        assert {p.name: p.read_bytes() for p in setup.iterdir() if p.is_file() and 'claude-subscription' not in p.name} == before
    try:
        assert no_browser(), 'existing_browser_must_be_preserved'
        if args.recover:
            status, data = call(); assert status == 200
            op = data.get('operation')
            if op and op['state'] not in ['ready', 'failed', 'cancelled', 'disconnected']:
                assert call({'op': 'cancel', 'request_id': op['request_id']})[0] == 200
                poll(op['request_id'], ['cancelled', 'failed'])
        ident = start()
        assert call({'op': 'code', 'request_id': ident, 'code': 'synthetic-code#state'})[0] in (400, 404)
        frame(ident)
        record('K01-private-Kiri-frame-no-external-URL-or-code-entry')
        assert call()[1]['operation']['request_id'] == ident
        assert not call({'op': 'status', 'request_id': ident}, sibling)[1]['operation'].get('browser_available')
        for payload in [{'op': 'frame', 'request_id': ident},
                        {'op': 'input', 'request_id': ident, 'frame_token': 'synthetic-stale', 'input': {'kind': 'scroll', 'delta_x': 0, 'delta_y': 30}}]:
            assert call(payload, sibling)[0] == 404
            assert call(payload, foreign, members[1]['account'])[0] in (403, 404)
            assert call(payload, None)[0] == 401
        frame(ident)
        record('K02-same-session-reopen-and-other-session-member-denial')
        stale = {'op': 'input', 'request_id': ident, 'frame_token': 'synthetic-stale', 'input': {'kind': 'scroll', 'delta_x': 0, 'delta_y': 30}}
        assert call(stale)[0] == 409
        current = frame(ident)
        gesture = dict(stale, frame_token=current['frame_token'])
        assert call(gesture)[0] == 200
        assert call(gesture)[0] == 409
        frame(ident)
        record('K03-stale-proof-and-repeated-gesture-refused-without-replay')
        assert call({'op': 'cancel', 'request_id': ident})[0] == 200
        poll(ident, ['cancelled']); fixture.until(no_browser, 40); unchanged()
        record('K05a-explicit-cancel-confirms-browser-and-CLI-cleanup')
        ident = start(); frame(ident)
        subprocess.run(['systemctl', 'restart', 'moosuri-kiri-gateway'], check=True, timeout=45, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        fixture.until(lambda: call()[0] == 200, 30)
        assert call()[1]['operation']['state'] == 'interrupted'
        assert call({'op': 'cancel', 'request_id': ident})[0] == 200
        poll(ident, ['cancelled']); fixture.until(no_browser, 40); unchanged()
        record('K05b-gateway-restart-and-confirmed-recovery')
        ident = start(); frame(ident)
        assert fixture.app('auth/logout', 'POST', {}, jar, members[0]['account'])[0] in (200, 204)
        fixture.until(no_browser, 90)
        assert call({'op': 'frame', 'request_id': ident})[0] == 401
        unchanged(); record('K05c-logout-revokes-frame-and-closes-browser')
    except Exception as error:
        last = traceback.extract_tb(error.__traceback__)[-1]
        report['failure'] = {'type': type(error).__name__, 'line': last.lineno}
        (ROOT/'result.json').write_text(json.dumps(report))
        print('FAIL Kiri Claude journey: '+type(error).__name__+' line '+str(last.lineno), flush=True)
        raise SystemExit(1)


if __name__ == '__main__': main()
