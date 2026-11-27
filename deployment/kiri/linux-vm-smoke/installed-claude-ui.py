"""Installed UI assertions, called by the existing isolated Linux browser harness."""
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'


def run(browser, args, members):
    contexts = []
    try:
        for index, member in enumerate(members):
            context = browser.new_context(ignore_https_errors=True, service_workers='block',
                        viewport={'width': 1280 if index == 0 else 390, 'height': 900})
            contexts.append(context)
            page = context.new_page()
            page.goto(ORIGIN + '/app#admin/engine')
            page.get_by_label('이메일', exact=True).fill(member['email'])
            page.get_by_label('비밀번호', exact=True).fill(member['password'])
            page.get_by_role('button', name='로그인', exact=True).click()
            expect(page.locator('.member-workspace')).to_be_visible(timeout=30000)
            if index:
                expect(page.get_by_role('radio', name='내 Claude 구독 사용', exact=True)).to_have_count(0)
                expect(page.get_by_label('Anthropic API 키', exact=True)).to_have_count(0)
                print('PASS U03 ordinary member cannot access either credential form', flush=True)
                continue
            expect(page.get_by_role('radio', name='내 Claude 구독 사용', exact=True)).to_be_visible(timeout=30000)
            page.get_by_role('radio', name='내 Claude 구독 사용', exact=True).check()
            expect(page.get_by_label('Claude 계정 이메일', exact=True)).to_be_visible()
            expect(page.get_by_label('구독 모델 ID', exact=True)).to_have_value('claude-sonnet-5-5')
            expect(page.get_by_label('구독 추론 강도', exact=True)).to_have_value('medium')
            expect(page.get_by_role('button', name='구독 로그인 시작', exact=True)).to_be_disabled()
            page.set_viewport_size({'width': 390, 'height': 844})
            assert page.evaluate('document.documentElement.scrollWidth <= innerWidth'), 'subscription_form_overflow'
            page.screenshot(path=str(args.output_dir/'claude-form-mobile.png'), full_page=True)
            page.get_by_role('button', name='구독 로그인 시작', exact=True).scroll_into_view_if_needed()
            page.screenshot(path=str(args.output_dir/'claude-form-mobile-bottom.png'), full_page=True)
            page.set_viewport_size({'width': 1280, 'height': 900})
            print('PASS U01 mode choice, exact defaults, consent gate and mobile layout', flush=True)
            if args.claude_structure_only:
                print('NOT RUN U02 structure-only selection', flush=True)
                continue
            page.get_by_label('Claude 계정 이메일', exact=True).fill('synthetic-owner@example.test')
            page.get_by_label('호스팅 조건을 확인했고, 제 구독을 이 설치에서 호스팅하는 데 동의합니다').check()
            page.get_by_role('button', name='구독 로그인 시작', exact=True).click()
            expect(page.get_by_role('link', name='Claude 로그인 열기', exact=True)).to_be_visible(timeout=240000)
            expect(page.get_by_label('로그인 코드', exact=True)).to_have_attribute('type','password')
            expect(page.get_by_label('로그인 코드', exact=True)).to_have_value('')
            page.reload()
            expect(page.get_by_role('link', name='Claude 로그인 열기', exact=True)).to_be_visible(timeout=30000)
            # Screenshots contain the link label, never its one-use URL or code.
            page.screenshot(path=str(args.output_dir/'claude-awaiting-code.png'), full_page=True)
            page.get_by_test_id('claude-subscription-setup').get_by_role('button', name='취소', exact=True).click()
            expect(page.get_by_test_id('claude-subscription-setup').get_by_role('status')).to_contain_text('취소',timeout=180000)
            page.screenshot(path=str(args.output_dir/'claude-cancelled.png'), full_page=True)
            print('PASS U02 actual URL handoff, private code field, reload and confirmed cancellation', flush=True)
    finally:
        for context in contexts:
            context.close()
