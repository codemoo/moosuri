"""Real installed UI after synthetic app registration; no API mocking or consent."""
import json
import re
import traceback
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'


def run(browser, args, ssh, target, members):
    report = args.output_dir / 'google-sharing-ui-result.json'
    assert not report.exists(), 'preserve_previous_evidence'
    evidence = {'passed': [], 'provider_authentication': False, 'api_mocking': False}
    writes = []
    try:
        for index, member in enumerate(members):
            context = browser.new_context(ignore_https_errors=True, service_workers='block',
                                          viewport={'width': 390 if index else 1280, 'height': 844})
            try:
                # Neither this verification nor opening a detail authorizes a
                # provider operation; observe any unexpected Google POST.
                context.on('request', lambda req: writes.append(req.url) if
                           '/api/v1/personal/google' in req.url and req.method != 'GET' else None)
                page = context.new_page()
                page.goto(ORIGIN + '/app#connections')
                page.get_by_label('이메일', exact=True).fill(member['email'])
                page.get_by_label('비밀번호', exact=True).fill(member['password'])
                page.get_by_role('button', name='로그인', exact=True).click()
                expect(page.get_by_role('heading', name='커넥터', exact=True)).to_be_visible(timeout=30000)
                setup = page.get_by_role('region', name='Google Workspace 설치 설정', exact=True)
                if index == 0:
                    expect(setup.get_by_text('설치 설정 완료', exact=True)).to_be_visible()
                    setup.get_by_role('button', name='Google Workspace 설정', exact=True).click()
                    expect(page.get_by_role('heading', name='Google 앱 설정이 등록되어 있습니다.', exact=True)).to_be_visible()
                    expect(page.get_by_label('이 설치의 콜백 주소')).to_have_value(ORIGIN + '/api/v1/mail/oauth/callback')
                    page.get_by_role('button', name='커넥터 열기', exact=True).click()
                    evidence['passed'].append('GA02-ui-admin-sees-shared-app-registered')
                else:
                    expect(setup).to_have_count(0)
                    expect(page.get_by_role('button', name='Google Workspace 설정', exact=True)).to_have_count(0)
                page.get_by_role('button', name=re.compile(r'^Gmail\b')).click()
                expect(page.get_by_role('button', name='Google 연결', exact=True)).to_be_enabled(timeout=30000)
                expect(page.get_by_role('button', name='연결 해제', exact=True)).to_have_count(0)
                page.reload()
                expect(page.get_by_role('button', name='Google 연결', exact=True)).to_be_enabled(timeout=30000)
                expect(page.get_by_role('button', name='연결 해제', exact=True)).to_have_count(0)
                assert not writes, 'opening_or_reload_must_not_start_consent_or_read'
                assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
                page.screenshot(path=str(args.output_dir / f'google-sharing-member-{index}.png'), full_page=True)
                evidence['passed'].append('GA06-ui-' + ('admin' if index == 0 else 'member') + '-reload-requires-own-consent')
                report.write_text(json.dumps(evidence))
            finally:
                context.close()
        for name in evidence['passed']:
            print('PASS ' + name, flush=True)
    except Exception as error:
        frame = next((f for f in reversed(traceback.extract_tb(error.__traceback__))
                      if f.filename.endswith('installed-google-sharing-ui.py')), None)
        evidence['failure'] = {'type': type(error).__name__, 'line': frame.lineno if frame else None}
        raise
    finally:
        report.write_text(json.dumps(evidence))
