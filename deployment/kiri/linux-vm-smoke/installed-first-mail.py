"""Installed UI first-conversation journeys with explicit synthetic service replies.

App login, thread creation and browser UI are real in the marked guest. Model
capability/interpretation and Gmail status/history/read are intercepted: no model
call, provider credential, real mail or OAuth request is involved. This tests the
UI orchestration and original draft/request boundary, not actual Gmail success.
"""
import json
import re
import subprocess
import traceback
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'
ASK = '최근 받은 메일 5개 확인해줘'
EMPTY = '확인한 페이지에 받은 메일이 없습니다.'
CONNECTION = {'connection_id':'55555555-5555-4555-8555-555555555555',
              'epoch':1, 'mailbox':'synthetic-mailbox@example.test'}


def run(browser, args, ssh, target, members):
    report=args.output_dir/'first-mail-result.json'
    assert not report.exists(), 'preserve_previous_evidence'
    evidence={'passed':[], 'provider_authentication':False, 'model_inference':False,
              'synthetic_replies':['capability','interpretation','Gmail status/history/read']}
    check="""from pathlib import Path
import subprocess,hashlib
assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'
p=subprocess.check_output(['systemctl','show','moosuri-kiri-gateway','-p','MainPID','--value'],text=True).strip()
print(hashlib.sha256(Path('/proc/'+p+'/exe').read_bytes()).hexdigest())
"""
    result=subprocess.run([*ssh,target,'sudo','-n','python3','-B','-'],input=check,text=True,capture_output=True,timeout=30)
    assert result.returncode==0 and re.fullmatch('[a-f0-9]{64}',result.stdout.strip())
    evidence['gateway_sha256']=result.stdout.strip()
    context=browser.new_context(ignore_https_errors=True, service_workers='block')
    connected=True
    reads, interpretations, history, created=[],[],[],[]

    def fulfill(route, value, status=200):
        route.fulfill(status=status,content_type='application/json',body=json.dumps(value))

    def service(route):
        nonlocal history
        path=route.request.url.removeprefix(ORIGIN+'/api/v1')
        body=route.request.post_data_json if route.request.method=='POST' else None
        if path=='/text-preview':
            fulfill(route,{'enabled':True,'capability':'text_only_preview','tools':False,'automatic_retry':False,'work_interpret':True})
        elif re.fullmatch('/threads/[^/]+/work-interpret',path):
            thread=path.split('/')[2]
            interpretations.append({'thread_id':thread,'request_id':body['request_id']})
            fulfill(route,{'kind':'inbox_read','thread_id':thread,'scope':'one_inbox_page_metadata_only','limit':5})
        elif path=='/personal/google':
            fulfill(route,{'connection':CONNECTION if connected else None})
        elif path=='/personal/google/history':
            fulfill(route,{'reads':[r for r in history if r['thread_id']==body['thread_id']]})
        elif path=='/personal/google/inbox':
            assert connected
            reads.append(body['read'])
            saved={**body['read'],'state':'completed','created_at':1,
                   'source':{**CONNECTION,'app_revision':'11111111-1111-4111-8111-111111111111'},
                   'result':{'mailbox':CONNECTION['mailbox'],'messages':[],'has_more':False,'scope':'one_inbox_page_metadata_only'}}
            history.append(saved);fulfill(route,saved)
        elif path=='/personal/google/kiri':
            route.abort('failed')
            raise AssertionError('no_provider_connection_authorized_in_UI_fixture')
        elif path=='/threads' and route.request.method=='POST':
            created.append(body['request_id']);route.continue_()
        else: route.continue_()
    context.route(ORIGIN+'/api/v1/**',service)
    try:
        page=context.new_page();page.goto(ORIGIN+'/app')
        page.get_by_label('이메일',exact=True).fill(members[0]['email'])
        page.get_by_label('비밀번호',exact=True).fill(members[0]['password'])
        page.get_by_role('button',name='로그인',exact=True).click()
        expect(page.locator('.member-workspace')).to_be_visible(timeout=30000)
        for number,newer in enumerate([None,'A newer synthetic draft',ASK],1):
            connected=newer is None
            page.get_by_role('button',name='대화 목록',exact=True).click()
            page.get_by_role('dialog',name='대화 목록',exact=True).get_by_role('button',name='새 대화',exact=True).click()
            composer=page.get_by_role('textbox',name='메시지',exact=True)
            expect(composer).to_have_value('')
            before=(len(created),len(interpretations),len(reads))
            composer.fill(ASK)
            page.get_by_role('button',name='보내기',exact=True).click()
            if newer is None:
                expect(page.get_by_text(EMPTY,exact=True)).to_be_visible(timeout=30000)
                expect(page.get_by_role('textbox',name='메시지',exact=True)).to_have_value('')
            else:
                expect(page.get_by_role('button',name='Google 연결하고 메일 확인',exact=True)).to_be_visible(timeout=30000)
                assert len(reads)==before[2]
                page.get_by_role('button',name='뒤로',exact=True).click()
                page.get_by_role('textbox',name='메시지',exact=True).fill(newer)
                page.reload()
                resume=page.get_by_role('button',name='메일 확인 계속하기',exact=True)
                expect(resume).to_be_visible(timeout=30000)
                assert len(reads)==before[2], 'reload_must_not_read'
                expect(page.get_by_role('textbox',name='메시지',exact=True)).to_have_value(newer)
                connected=True
                resume.click()
                expect(page.get_by_text(EMPTY,exact=True)).to_be_visible(timeout=30000)
                expect(page.get_by_role('textbox',name='메시지',exact=True)).to_have_value(newer)
            assert len(created)==before[0]+1 and len(interpretations)==before[1]+1
            assert len(reads)==before[2]+1 and reads[-1]==interpretations[-1]
            assert reads[-1]['thread_id']==created[-1]
            if number==3:
                page.set_viewport_size({'width':390,'height':844})
                assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
                page.screenshot(path=str(args.output_dir/'first-mail-draft-mobile.png'),full_page=True)
            name=['FM01-first-live-message-reads-once-in-created-thread',
                  'FM02-reload-needs-explicit-resume-and-preserves-new-draft',
                  'FM03-identical-new-draft-is-not-cleared-by-old-request'][number-1]
            evidence['passed'].append(name);report.write_text(json.dumps(evidence));print('PASS '+name,flush=True)
    except Exception as error:
        frame=next((f for f in reversed(traceback.extract_tb(error.__traceback__)) if f.filename.endswith('installed-first-mail.py')),None)
        evidence['failure']={'type':type(error).__name__,'line':frame.lineno if frame else None}
        raise
    finally:
        report.write_text(json.dumps(evidence));context.close()
