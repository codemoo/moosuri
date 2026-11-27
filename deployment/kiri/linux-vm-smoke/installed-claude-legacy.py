"""Real installed UI/HTTP with explicitly synthetic legacy configs and receipts.

Only for the marked disposable two-member installation. Provider authentication
and model responses are not mocked or attempted. Guest writes are fixture setup,
never evidence of product-owned configuration or successful provider login.
"""
import hashlib
import json
import subprocess
import traceback
import time
import uuid
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'
PREFIX = '/api/v1/personal/admin/engine/'
# Runs as root exclusively inside the existing marked guest, and never prints
# passwords, sessions or tokens. Original terminal receipt is retained privately.
FIXTURE = r'''
import base64,hashlib,json,os,subprocess,sys,time,uuid
from pathlib import Path
assert os.geteuid()==0
assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'
os.umask(0o077)
p=json.loads(sys.stdin.read())
root=Path('/root/moosuri-legacy-journeys-'+p['run'])
d=Path('/var/lib/moosuri-control/model-setup');config=d/'runtime-model.json';receipt=d/'claude-subscription-operation.json'
sha=lambda b:hashlib.sha256(b).hexdigest()
actual=sha(config.read_bytes()) if config.exists() else None
if p['action']=='resume':
 assert root.exists()
 original=json.loads((root/'original-receipt.json').read_text())
 r=json.loads(receipt.read_text());v=json.loads(config.read_text())
 assert r['mode']=='disconnect' and r['state']=='cleanup_pending' and r['config_sha256'] is None
 assert v['connection_revision']==r['target_revision']
 assert all(v['bindings'][0][k]==original[k] for k in ['account_id','tenant_id','volume_id','generation'])
 assert v['bindings'][0]['worker']=={'socket':'/run/moosuri-claude-worker-60001/engine.sock','worker_uid':60001}
 assert all(json.loads(f.read_text())['attempt']['request_id']!=r['target_revision'] for f in Path('/var/lib/moosuri-claude-control/connections').glob('*.json'))
 print(json.dumps({'config_sha256':actual,'revision':r['target_revision'],'operation':r['request_id']}));sys.exit()
assert actual==p['expected'],'fixture_config_changed' 
state=json.loads(Path('/root/moosuri-clean-acceptance-state.json').read_text())
assert [m['email'] for m in state['members']]==['install-test@example.com','second-install@example.com']
if p['action']=='inspect':
 r=json.loads(receipt.read_text())
 pid=subprocess.check_output(['systemctl','show','moosuri-kiri-gateway','-p','MainPID','--value'],text=True).strip()
 print(json.dumps({'config_sha256':actual,'state':r['state'],'intent':r.get('config_sha256') is not None,
   'binary_sha256':sha(Path('/proc',pid,'exe').read_bytes())}));sys.exit()
if not root.exists():
 assert not config.exists()
 r=json.loads(receipt.read_text());assert r['state']=='cancelled' and not r.get('browser_pending')
 assert r['account_id']==state['members'][0]['account']
 root.mkdir(mode=0o700);(root/'original-receipt.json').write_bytes(receipt.read_bytes())
original=json.loads((root/'original-receipt.json').read_text())
def write(path,value):
 data=value if isinstance(value,bytes) else json.dumps(value).encode()
 tmp=path.with_name(path.name+'.legacy-fixture');assert not tmp.exists()
 with tmp.open('xb') as f:f.write(data);f.flush();os.fsync(f.fileno())
 os.replace(tmp,path)
 fd=os.open(path.parent,os.O_DIRECTORY);os.fsync(fd);os.close(fd)
subprocess.run(['systemctl','stop','moosuri-kiri-gateway'],check=True,timeout=45)
try:
 current=json.loads(receipt.read_text())
 assert current['state'] in ['cancelled','failed','cleanup_pending']
 if p['action']=='restore':
  if config.exists():config.unlink()
  write(receipt,(root/'original-receipt.json').read_bytes())
  result={'config_sha256':None}
 else:
  target=p['revision']
  owner={k:original[k] for k in ['account_id','tenant_id','volume_id','generation']}
  records=[json.loads(f.read_text()) for f in Path('/var/lib/moosuri-claude-control/connections').glob('*.json')]
  assert all(r['attempt']['request_id']!=target for r in records)
  worker={'socket':'/run/legacy-synthetic/engine.sock' if p['kind']=='external' else '/run/moosuri-claude-worker-60001/engine.sock','worker_uid':60001}
  cfg={'version':1,'connection_revision':target,'provider':{'kind':'anthropic','base_url':None,'allowed_ips':[],'router_providers':[],'zero_data_retention':False},
    'model':'claude-sonnet-5-5','effort':'medium','models':[],'key_file':None,'subscription':'claude_code',
    'bindings':[dict(owner,peer_uid=250001,budget={'max_calls':100,'max_tokens':500000,'reservation_tokens':16384},worker=worker)]}
  data=json.dumps(cfg).encode();write(config,data)
  r=original.copy()
  if p.get('orphan'):
   digest='claude-subscription-disconnect-v1\n'+ '\n'.join(str(owner[k]) for k in ['account_id','tenant_id','volume_id','generation'])+'\n'+target
   r.update(version=3,request_id=p['operation'],mode='disconnect',target_revision=target,replace_revision=None,
      digest=base64.urlsafe_b64encode(hashlib.sha256(digest.encode()).digest()).decode().rstrip('='),
      config_sha256=sha(data) if p['orphan']=='started' else None,state='cleanup_pending',updated=int(time.time()))
  write(receipt,r)
  result={'config_sha256':sha(data),'revision':target}
finally:
 subprocess.run(['systemctl','start','moosuri-kiri-gateway'],check=True,timeout=45)
 deadline=time.monotonic()+30
 while time.monotonic()<deadline:
  ready=subprocess.run(['curl','-ksS','--noproxy','*','--resolve','assistant.example.com:443:127.0.0.1','-o','/dev/null','-w','%{http_code}','https://assistant.example.com/app'],capture_output=True,text=True,timeout=5)
  if ready.returncode==0 and ready.stdout=='200':break
  time.sleep(.2)
 else:raise RuntimeError('fixture_gateway_not_ready')
print(json.dumps(result))
'''


def run(browser, args, ssh, target, members):
    report = args.output_dir / 'claude-legacy-result.json'
    assert not report.exists(), 'preserve_previous_evidence'
    result = {'passed': [], 'synthetic_configs': True, 'api_mocking': False,
              'provider_authentication': False, 'inference': False}
    prior = None
    if args.claude_legacy_resume:
        prior=json.loads(args.claude_legacy_resume.read_text())
        assert [v.split('-')[0] for v in prior['passed']]==['LR01','LR02','LR03','LR04']
        assert prior['failure']['type']=='TimeoutError' and not prior['provider_authentication']
        ident=prior['retained_fixture_run']
        result['resumed_from_sha256']=hashlib.sha256(args.claude_legacy_resume.read_bytes()).hexdigest()
    else:
        ident = uuid.uuid4().hex
    expected = None
    contexts = []

    def guest(action, **extra):
        nonlocal expected
        import shlex
        payload = dict(run=ident, expected=expected, action=action, **extra)
        response = subprocess.run([*ssh, target, 'sudo', '-n', 'python3', '-B', '-c', shlex.quote(FIXTURE)],
                                  input=json.dumps(payload), text=True, capture_output=True, timeout=70)
        assert response.returncode == 0, 'guest_fixture_or_observation_failed'
        value = json.loads(response.stdout)
        expected = value['config_sha256']
        return value

    def passed(name):
        result['passed'].append(name)
        report.write_text(json.dumps(result))
        print('PASS '+name, flush=True)

    def login(index):
        context = browser.new_context(ignore_https_errors=True, service_workers='block',
                                      viewport={'width':390 if index else 1280,'height':844})
        contexts.append(context)
        page = context.new_page()
        page.goto(ORIGIN+'/app#admin/engine')
        page.get_by_label('이메일', exact=True).fill(members[index]['email'])
        page.get_by_label('비밀번호', exact=True).fill(members[index]['password'])
        page.get_by_role('button', name='로그인', exact=True).click()
        expect(page.locator('.member-workspace')).to_be_visible(timeout=30000)
        return page

    def settled(page, revision):
        # The fixture bypasses the product save callback. Wait for the real
        # broker to publish this exact config before opening its UI.
        end=time.monotonic()+30
        while time.monotonic()<end:
            response=page.request.get(ORIGIN+PREFIX.rstrip('/'),
                headers={'X-Moosuri-Account':members[0]['account'],'Origin':ORIGIN})
            if response.status==200 and response.json().get('connection_revision')==revision:
                return
            time.sleep(.25)
        raise AssertionError('fixture_catalog_not_published')

    try:
        if prior:
            recovered=guest('resume')
            managed=recovered['revision'];operation=recovered['operation']
            page=login(0)
        else:
            revision = str(uuid.uuid4())
            guest('stage', kind='external', revision=revision)
            page = login(0)
            settled(page,revision)
            page.reload()
            expect(page.get_by_label('Claude 계정 이메일', exact=True)).to_be_visible(timeout=30000)
            expect(page.get_by_role('button',name='구독 연결 해제',exact=True)).to_have_count(0)
            guest('inspect')
            passed('LR01-existing-external-connection-offers-replacement-without-deletion')
            other = login(1)
            expect(other.get_by_label('Claude 계정 이메일', exact=True)).to_have_count(0)
            expect(other.get_by_role('button',name='구독 연결 해제',exact=True)).to_have_count(0)
            page.reload()
            expect(page.get_by_label('Claude 계정 이메일', exact=True)).to_be_visible(timeout=30000)
            page.set_viewport_size({'width':390,'height':844})
            assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
            page.screenshot(path=str(args.output_dir/'legacy-replacement-mobile.png'),full_page=True)
            guest('inspect')
            passed('LR02-reload-mobile-and-second-member-preserve-private-connection')
            # Leave the first page showing the old target while the fixture simulates
            # another administrator having replaced that target.
            newer = str(uuid.uuid4())
            guest('stage',kind='external',revision=newer)
            settled(page,newer)
            page.get_by_label('Claude 계정 이메일',exact=True).fill('synthetic-owner@example.test')
            page.get_by_label('호스팅 조건을 확인했고, 제 구독을 이 설치에서 호스팅하는 데 동의합니다').check()
            with page.expect_response(lambda r: r.url.endswith(PREFIX+'subscription') and r.request.method=='POST') as response:
                page.get_by_role('button',name='구독 로그인 시작',exact=True).click()
            assert response.value.status==409
            expect(page.get_by_role('button',name='연결 상태 다시 확인',exact=True)).to_be_visible()
            page.get_by_role('button',name='연결 상태 다시 확인',exact=True).click()
            expect(page.get_by_label('Claude 계정 이메일',exact=True)).to_be_visible(timeout=30000)
            assert guest('inspect')['state']=='cancelled'
            passed('LR03-stale-replacement-refused-and-parent-refresh-recovers')
            managed=str(uuid.uuid4())
            guest('stage',kind='managed',revision=managed)
            settled(page,managed)
            page.reload()
            with page.expect_response(lambda r: r.url.endswith(PREFIX+'subscription') and r.request.method=='POST') as response:
                page.get_by_role('button',name='구독 연결 해제',exact=True).click(timeout=30000)
            assert response.value.status==412
            expect(page.get_by_role('alert').filter(has_text='관리 기록')).to_be_visible()
            assert guest('inspect')['state']=='cancelled'
            passed('LR04-missing-managed-record-refuses-before-reserving-or-unpublishing')
        if not prior:
            operation=str(uuid.uuid4())
            guest('stage',kind='managed',revision=managed,orphan='unstarted',operation=operation)
        page.reload()
        page.get_by_role('button',name='정리 다시 시도',exact=True).click(timeout=30000)
        expect(page.get_by_test_id('claude-subscription-setup').get_by_role('status')).to_contain_text('연결과 로그인은 삭제하지 않았습니다',timeout=30000)
        check=guest('inspect');assert check['state']=='failed' and not check['intent']
        response=page.request.get(ORIGIN+PREFIX+'setup',headers={'X-Moosuri-Account':members[0]['account'],'Origin':ORIGIN})
        assert response.status==200 and response.json().get('subscription_operation',False) is False
        passed('LR05-restarted-unstarted-disconnect-ends-and-unblocks-next-setup')
        guest('stage',kind='managed',revision=managed,orphan='started',operation=str(uuid.uuid4()))
        page.reload()
        page.get_by_role('button',name='정리 다시 시도',exact=True).click(timeout=30000)
        expect(page.get_by_test_id('claude-subscription-setup').get_by_role('status')).to_contain_text('정리',timeout=30000)
        # Observe beyond the complete bounded drive window, not its initial orphan view.
        page.wait_for_timeout(14000)
        check=guest('inspect');assert check['state']=='cleanup_pending' and check['intent']
        response=page.request.get(ORIGIN+PREFIX+'setup',headers={'X-Moosuri-Account':members[0]['account'],'Origin':ORIGIN})
        assert response.status==200 and response.json()['subscription_operation'] is True
        page.reload()
        expect(page.get_by_role('button',name='정리 다시 시도',exact=True)).to_be_visible(timeout=30000)
        result['candidate_sha256']=check['binary_sha256']
        passed('LR06-started-disconnect-retains-cleanup-obligation-across-reentry')
        guest('restore')
        expected=None
        result['fixture_restored']=True
    except Exception as error:
        frame=next((f for f in reversed(traceback.extract_tb(error.__traceback__)) if f.filename.endswith('installed-claude-legacy.py')),None)
        result['failure']={'type':type(error).__name__,'line':frame.lineno if frame else None}
        if 'page' in locals():
            page.screenshot(path=str(args.output_dir/'failure.png'),full_page=True)
        result['retained_fixture_run']=ident
        raise
    finally:
        report.write_text(json.dumps(result))
        for context in contexts:context.close()
