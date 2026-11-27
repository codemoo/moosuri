"""Real Linux Chromium against a deliberately synthetic Active binding."""
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'


def run(browser, args, members):
    context = browser.new_context(ignore_https_errors=True, service_workers='block',
                                  viewport={'width':390, 'height':844})
    try:
        page = context.new_page()
        page.goto(ORIGIN+'/app#admin/engine')
        page.get_by_label('이메일', exact=True).fill(members[0]['email'])
        page.get_by_label('비밀번호', exact=True).fill(members[0]['password'])
        page.get_by_role('button', name='로그인', exact=True).click()
        disconnect = page.get_by_role('button', name='구독 연결 해제', exact=True)
        expect(disconnect).to_be_visible(timeout=30000)
        expect(page.get_by_label('Anthropic API 키', exact=True)).to_have_count(0)
        expect(page.get_by_role('button', name='구독 로그인 시작', exact=True)).to_have_count(0)
        assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
        disconnect.scroll_into_view_if_needed()
        page.screenshot(path=str(args.output_dir/'claude-disconnect-mobile.png'), full_page=True)
        print('PASS DU01 explicit disconnect, replacement blocked and mobile layout', flush=True)
        disconnect.click()
        expect(page.get_by_role('radio', name='API 키 사용', exact=True)).to_be_visible(timeout=180000)
        expect(page.get_by_role('radio', name='내 Claude 구독 사용', exact=True)).to_be_visible()
        expect(page.get_by_label('Anthropic API 키', exact=True)).to_be_visible()
        page.reload()
        expect(page.get_by_role('radio', name='API 키 사용', exact=True)).to_be_visible(timeout=30000)
        page.screenshot(path=str(args.output_dir/'claude-disconnected-choices.png'), full_page=True)
        print('PASS DU02 confirmed disconnect returns to connection choices across reload', flush=True)
        page.get_by_role('radio', name='내 Claude 구독 사용', exact=True).check()
        page.get_by_label('Claude 계정 이메일', exact=True).fill('synthetic-owner@example.test')
        page.get_by_label('호스팅 조건을 확인했고, 제 구독을 이 설치에서 호스팅하는 데 동의합니다').check()
        page.get_by_role('button', name='구독 로그인 시작', exact=True).click()
        expect(page.get_by_alt_text('Claude 로그인 화면', exact=True)).to_be_visible(timeout=240000)
        page.get_by_test_id('claude-subscription-setup').get_by_role('button', name='취소', exact=True).click()
        expect(page.get_by_test_id('claude-subscription-setup').get_by_role('status')).to_contain_text('취소', timeout=180000)
        print('PASS DU03 fresh login reaches Kiri screen and cancels after disconnect', flush=True)
    finally:
        context.close()
