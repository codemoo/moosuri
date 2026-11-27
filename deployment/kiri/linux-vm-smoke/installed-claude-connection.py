#!/usr/bin/env python3
"""Real installed gateway journeys up to official URL/cancel; no provider account login.

Only in the marked synthetic guest installed by clean-installed.py. No cookies,
authentication URLs, codes, passwords or provider output are stored or printed.
The installed candidate gateway/unit must already be selected by the operator.
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
ROOT = Path('/root/moosuri-installed-claude-connection')


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
    assert len(members) == 2 and members[0]['email'] == 'install-test@example.com'
    ROOT.mkdir(mode=0o700, exist_ok=args.recover)
    if args.recover and (ROOT/'result.json').exists():
        (ROOT/('result-prior-'+str(time.time_ns())+'.json')).write_bytes((ROOT/'result.json').read_bytes())
    report = {'checks': [], 'provider_login': False, 'inference': False}
    pid = subprocess.check_output(['systemctl','show','moosuri-kiri-gateway','-p','MainPID','--value'],text=True).strip()
    report['candidate_sha256'] = hashlib.file_digest(open('/proc/'+pid+'/exe','rb'),'sha256').hexdigest()
    account, member = members[0]['account'], members[1]['account']
    _, jar = fixture.login(members[0]['email'], members[0]['password'])
    _, other = fixture.login(members[1]['email'], members[1]['password'])
    def call(op=None, jar=jar, account=account):
        return fixture.app(BASE,'GET' if op is None else 'POST',op,jar,account)
    def save():
        (ROOT/'result.json').write_text(json.dumps(report))
    def passed(name):
        report['checks'].append({'journey':name,'status':'passed'}); save()
        print('PASS '+name,flush=True)
    def poll(ident, target):
        deadline = time.monotonic()+240
        while time.monotonic()<deadline:
            status,data=call({'op':'status','request_id':ident})
            assert status == 200
            op=data['operation']
            if op['state'] in target:return op
            assert op['state'] not in ['failed','cancelled'], 'unexpected_terminal_state'
            if target == ['awaiting_code']:
                assert op['state'] != 'cleanup_pending', 'preparation_cleanup_pending'
            time.sleep(1)
        raise AssertionError('connection_deadline')
    def new():
        ident=str(uuid.uuid4())
        payload={'op':'start','request_id':ident,'email':'synthetic-owner@example.test',
                 'model':'claude-sonnet-5-5','effort':'medium','hosting_terms_accepted':True}
        (ROOT/'request.json').write_text(json.dumps({'request_id':ident}))
        status,data=call(payload)
        assert status==200 and data['operation']['request_id']==ident
        return ident,payload
    def unchanged():
        p=Path('/var/lib/moosuri-control/model-setup/installation-model.json')
        # Derive actual config filename below from the known setup directory's
        # initial contents, rather than assume a successful model publication.
        assert {x.name:x.read_bytes() for x in p.parent.iterdir() if x.is_file() and 'claude-subscription' not in x.name} == before
    setup=Path('/var/lib/moosuri-control/model-setup')
    before={x.name:x.read_bytes() for x in setup.iterdir() if x.is_file() and 'claude-subscription' not in x.name}
    if args.recover:
        status,data=call()
        assert status==200
        op=data.get('operation')
        if op and op['state'] not in ['ready','failed','cancelled']:
            status,_=call({'op':'cancel','request_id':op['request_id']});assert status==200
            poll(op['request_id'],['cancelled','failed'])
        passed('S00-explicit-prior-interruption-recovery')
    try:
        assert call(jar=other,account=member)[0]==403
        assert call({'op':'start','request_id':str(uuid.uuid4()),'email':'a@example.test','model':'claude-sonnet-5-5','effort':'medium','hosting_terms_accepted':True},other,member)[0]==403
        assert call(jar=None)[0]==401
        assert call(jar=other,account=account)[0] in (401,403)
        passed('S01-member-anonymous-and-foreign-owner-refused')
        ident,payload=new()
        status,replay=call(payload);assert status==200 and replay['operation']['request_id']==ident
        changed=dict(payload,email='another@example.test')
        assert call(changed)[0]==409
        op=poll(ident,['awaiting_code'])
        assert op.get('authorization_url','').startswith('https://claude.com/cai/oauth/authorize?')
        assert call({'op':'code','request_id':ident,'code':'bad code'})[0]==400
        passed('S02-real-preparation-official-url-idempotence-and-invalid-code')
        status,reopen=call();assert status==200 and reopen['operation']['request_id']==ident
        assert reopen['operation'].get('authorization_url')==op['authorization_url']
        _,second_session=fixture.login(members[0]['email'],members[0]['password'])
        status,hidden=call({'op':'status','request_id':ident},second_session)
        assert status==200 and 'authorization_url' not in hidden['operation']
        assert call({'op':'cancel','request_id':ident},second_session)[0]==404
        assert call({'op':'code','request_id':ident,'code':'synthetic-code'},second_session)[0]==404
        passed('S03-reload-preserves-initiator-and-other-session-has-no-code-authority')
        assert call({'op':'cancel','request_id':ident})[0]==200
        poll(ident,['cancelled']);unchanged()
        passed('S04-cancel-cleans-owned-environment-and-preserves-model-config')
        ident,payload=new();poll(ident,['awaiting_code'])
        subprocess.run(['systemctl','restart','moosuri-kiri-gateway'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=45)
        fixture.until(lambda: call()[0]==200,30)
        status,data=call();assert status==200 and data['operation']['state']=='interrupted'
        assert call(dict(payload,request_id=str(uuid.uuid4())))[0]==409
        assert call({'op':'cancel','request_id':ident})[0]==200
        poll(ident,['cancelled']);unchanged()
        passed('S05-process-restart-blocks-new-login-until-orphan-cleanup')
        ident,_=new()
        assert call({'op':'cancel','request_id':ident})[0]==200
        poll(ident,['cancelled']);unchanged()
        passed('S06-cancel-during-preparation-finishes-before-successor')
        ident,_=new();poll(ident,['awaiting_code'])
        assert fixture.app('auth/logout','POST',{},jar,account)[0] in (200,204)
        _,jar=fixture.login(members[0]['email'],members[0]['password'])
        # Use the new current session only for non-secret progress. Original job
        # must detect revocation and clean without receiving another code.
        deadline=time.monotonic()+90
        while time.monotonic()<deadline:
            status,data=call(None,jar)
            assert status==200
            if data['operation']['state'] in ['failed','cancelled']:break
            time.sleep(1)
        else:raise AssertionError('revoked_session_cleanup_deadline')
        unchanged();passed('S07-logout-revokes-job-and-no-connection-is-published')
    except Exception as error:
        frame=traceback.extract_tb(error.__traceback__)[-1]
        report['failure']={'type':type(error).__name__,'line':frame.lineno};save()
        print('FAIL installed Claude connection: '+type(error).__name__+' line '+str(frame.lineno),flush=True)
        raise SystemExit(1)

if __name__=='__main__':main()
