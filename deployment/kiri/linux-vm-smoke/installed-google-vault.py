"""Actual installed Vault preflight journeys; no provider credential submission.

Only the existing two-member marked guest is used. The real gateway prepares its
recipient and the real Vault WASM encrypts/saves. A deliberate browser transport
fault stops each live Deliver request before it reaches the gateway; the cancelled
packet is submitted once to test stale replay rejection. This is evidence
of preflight, ciphertext retry and cancellation, never Google login or consent.
No passwords, envelopes, sessions, request bodies or page traces are recorded.
"""
import hashlib
import json
from pathlib import Path
import re
import secrets
import subprocess
import time
import traceback
import uuid
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'
VAULT = 'https://vault.example.com'
BASE = ORIGIN + '/api/v1/personal/google/kiri'
FIRST = 'vault-journey-first@example.test'
SECOND = 'vault-journey-second@example.test'
DECOY = 'vault-journey-decoy@example.test'
PRIMARY_TITLE = 'Google'
SECOND_TITLE = 'Synthetic Google journey second'
DECOY_TITLE = 'Synthetic Google journey lookalike'


def run(browser, args, ssh, target, members):
    def guest(code):
        prelude = "from pathlib import Path\nassert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'\n"
        result = subprocess.run([*ssh, target, 'sudo', '-n', 'python3', '-B', '-'],
                                input=prelude+code, text=True, capture_output=True, timeout=30)
        assert result.returncode == 0, 'private_guest_check_failed'
        return json.loads(result.stdout)

    def inventory():
        return guest("""import json,subprocess
c=json.loads(Path('/etc/moosuri/google-browser.json').read_text())
prefix='moosuri-google-'+c['owner'].replace('-','')+'-'
r=subprocess.run(['docker','--host','unix:///run/kiri/docker.sock','container','ls','--all','--format','{{.Names}}','--filter','name=^/'+prefix+'[01]$'],capture_output=True,text=True,check=True)
chrome=0
for p in Path('/proc').glob('[0-9]*/comm'):
 try:
  if p.read_text().strip() in ('chrome','chromium','Xvfb'):chrome+=1
 except (FileNotFoundError,PermissionError,ProcessLookupError):pass
print(json.dumps({'capsules':len(r.stdout.splitlines()),'chrome':chrome}))
""")

    baseline = inventory()
    assert baseline == {'capsules': 0, 'chrome': 0}, 'existing_browser_must_be_preserved'
    evidence = {'passed': [], 'provider_authentication': False, 'delivery_boundary': 'browser_abort_before_gateway'}
    report = args.output_dir/'google-vault-result.json'
    assert not report.exists(), 'inspect_prior_run_and_created_items_before_recovery'
    evidence['gateway_sha256'] = guest("""import json,hashlib,subprocess
p=subprocess.check_output(['systemctl','show','moosuri-kiri-gateway','-p','MainPID','--value'],text=True).strip()
print(json.dumps(hashlib.sha256(Path('/proc/'+p+'/exe').read_bytes()).hexdigest()))
""")
    if args.google_vault_resume:
        prior = json.loads(args.google_vault_resume.read_text())
        expected = ['GV01-', 'GV02-', 'GV03-', 'GV04-']
        assert len(prior['passed']) == 4 and all(name.startswith(prefix) for name, prefix in zip(prior['passed'], expected))
        assert prior['gateway_sha256'] == evidence['gateway_sha256'], 'resume_gateway_changed'
        assert prior['delivery_boundary'] == evidence['delivery_boundary'] and prior['provider_authentication'] is False
        evidence['passed'] = prior['passed'].copy()
        evidence['resumed_prior_sha256'] = hashlib.sha256(args.google_vault_resume.read_bytes()).hexdigest()
        evidence['prior_failure'] = prior.get('failure')
    def passed(name):
        assert not plaintext, 'plaintext_secret_in_request'
        evidence['passed'].append(name)
        report.write_text(json.dumps(evidence))
        print('PASS '+name, flush=True)

    context = browser.new_context(ignore_https_errors=True, service_workers='block',
                                  viewport={'width': 1280, 'height': 900})
    other_context = browser.new_context(ignore_https_errors=True, service_workers='block',
                                        viewport={'width': 390, 'height': 844})
    packets, jobs, started, plaintext = [], [], [], []
    private = [f'Vault{chr(65+i)}42' for i in range(2)]
    private.extend(secrets.token_urlsafe(24) for _ in range(3))
    def guard(route):
        body = route.request.post_data_json
        if body.get('op') == 'start':
            jobs.append(body['job']); started.append(time.monotonic())
        if body.get('op') == 'deliver':
            assert set(body) == {'op', 'job', 'envelope', 'google_email'}
            assert set(body['envelope']) == {'key', 'iv', 'body'}
            packets.append(body)
            route.abort('failed')
        else:
            route.continue_()
    context.route(BASE, guard)
    other_context.route(BASE, lambda route: route.abort('failed') if route.request.post_data_json.get('op') == 'deliver' else route.continue_())
    def inspect(request):
        if request.url.startswith((ORIGIN+'/', VAULT+'/')) and request.method in ('POST', 'PUT'):
            body = request.post_data_buffer or b''
            if any(value.encode() in body for value in private):
                plaintext.append(True)
    context.on('request', inspect)
    other_context.on('request', inspect)

    def login(ctx, member):
        page = ctx.new_page(); page.goto(ORIGIN+'/app#connections/gmail')
        page.get_by_label('이메일', exact=True).fill(member['email'])
        page.get_by_label('비밀번호', exact=True).fill(member['password'])
        page.get_by_role('button', name='로그인', exact=True).click()
        expect(page.locator('.member-workspace')).to_be_visible(timeout=30000)
        return page

    def until(page, condition, seconds=30):
        end = time.monotonic()+seconds
        while time.monotonic() < end:
            if condition(): return
            page.wait_for_timeout(200)
        raise AssertionError('bounded_condition_failed')

    def request(page, member, body):
        return page.request.post(BASE, data=body, headers={
            'Origin': ORIGIN, 'Sec-Fetch-Site': 'same-origin', 'X-Moosuri-Account': member['account']})

    def no_chrome():
        assert inventory()['chrome'] == 0, 'preflight_started_chrome'

    def settled(page):
        until(page, lambda: inventory() == baseline, 45)

    def open_google(page):
        # Respect the product's four starts/minute admission rule.
        until(page, lambda: sum(time.monotonic()-at < 62 for at in started) < 4, 65)
        page.goto(ORIGIN+'/app#connections/gmail')
        with context.expect_page() as opened:
            page.get_by_role('button', name='Google 연결', exact=True).click()
        popup = opened.value
        expect(popup.get_by_label('볼트 비밀번호', exact=True)).to_be_visible(timeout=60000)
        assert inventory() == {'capsules':1, 'chrome':0}, 'prepared_key_positive_control'
        return popup, jobs[-1]

    def unlock(page, index=0):
        page.get_by_label('볼트 비밀번호', exact=True).fill(private[index])
        page.get_by_role('button', name='열기', exact=True).click()

    def cancel(page, popup):
        page.get_by_role('button', name='취소', exact=True).click()
        until(page, popup.is_closed)
        expect(page.get_by_role('button', name='Google 연결', exact=True)).to_be_enabled(timeout=20000)
        settled(page)

    def normal_vault(page, ctx):
        page.goto(ORIGIN+'/app#connections/vault')
        with ctx.expect_page() as opened:
            page.get_by_role('button', name='볼트 열기', exact=True).click()
        vault = opened.value
        expect(vault.get_by_label('볼트 비밀번호', exact=True)).to_be_visible(timeout=30000)
        return vault

    def add(vault, title, email, address, secret):
        vault.get_by_role('button', name='새 항목', exact=True).click()
        for label, value in [('이름', title), ('사용자 이름', email), ('주소', address), ('비밀번호 또는 키', secret)]:
            vault.get_by_label(label, exact=True).fill(value)
        vault.get_by_role('button', name='저장', exact=True).click()
        expect(vault.get_by_role('button', name=title, exact=True)).to_be_visible(timeout=60000)

    app = other = None
    try:
        app = login(context, members[0])
        if not args.google_vault_resume:
            popup, job = open_google(app)
            popup.get_by_label('볼트 비밀번호', exact=True).fill('synthetic-wrong-password')
            popup.get_by_role('button', name='열기', exact=True).click()
            expect(popup.get_by_text('비밀번호 또는 복구 키를 확인해 주세요.', exact=True)).to_be_visible(timeout=60000)
            assert not packets
            unlock(popup)
            expect(popup.get_by_label('Google 계정 비밀번호', exact=True)).to_be_visible(timeout=60000)
            assert not packets; no_chrome()
            passed('GV01-empty-Google-Vault-wrong-unlock-and-no-browser-before-account')

            popup.get_by_label('Google 계정 이메일', exact=True).fill(FIRST)
            popup.get_by_label('Google 계정 비밀번호', exact=True).fill(private[2])
            popup.get_by_role('button', name='볼트에 저장하고 계속', exact=True).click()
            until(app, lambda: bool(packets), 60)
            packet = packets[0]
            assert packet['google_email'] == FIRST and FIRST != members[0]['email']
            assert not plaintext
            assert request(app, members[0], {'op': 'status', 'job': job}).json()['state'] == 'vault'
            no_chrome(); cancel(app, popup)
            passed('GV02-different-Google-email-encrypted-save-and-sealed-handoff-only')

            before = len(packets)
            popup, job = open_google(app); unlock(popup)
            until(app, lambda: len(packets) >= before+2, 60)
            assert popup.get_by_label('Google 계정 비밀번호', exact=True).count() == 0
            current = [p for p in packets[before:] if p['job'] == job]
            assert len(current) >= 2 and all(p == current[0] for p in current)
            assert all(p['google_email'] == FIRST for p in current)
            assert jobs.count(job) == 1 and not popup.is_closed()
            no_chrome(); cancel(app, popup)
            replay = request(app, members[0], current[0])
            evidence['cancelled_replay_status'] = replay.status
            assert replay.status == 410
            no_chrome()
            passed('GV03-one-account-reuse-identical-ciphertext-retry-and-cancelled-replay-denial')

            vault = normal_vault(app, context); unlock(vault)
            expect(vault.get_by_role('button', name=PRIMARY_TITLE, exact=True)).to_be_visible(timeout=60000)
            add(vault, SECOND_TITLE, SECOND, 'https://accounts.google.com', private[3])
            add(vault, DECOY_TITLE, DECOY, 'https://accounts.google.com.evil.example.test', private[4])
            vault.close()
            before = len(packets)
            popup, job = open_google(app); unlock(popup)
            expect(popup.get_by_text('Google 계정이 여러 개 있습니다. 연결할 계정을 선택하세요.', exact=True)).to_be_visible(timeout=60000)
            expect(popup.get_by_role('button', name=re.compile('^'+re.escape(FIRST)))).to_be_visible()
            expect(popup.get_by_text(DECOY, exact=False)).to_have_count(0)
            app.wait_for_timeout(1800); assert len(packets) == before; no_chrome()
            popup.set_viewport_size({'width': 390, 'height': 844})
            assert popup.evaluate('document.documentElement.scrollWidth <= innerWidth'), 'Google_chooser_mobile_overflow'
            popup.screenshot(path=str(args.output_dir/'google-vault-chooser-mobile.png'), full_page=True)
            popup.get_by_role('button', name=re.compile('^'+re.escape(SECOND))).click()
            until(app, lambda: len(packets) > before)
            assert packets[before]['google_email'] == SECOND
            no_chrome(); cancel(app, popup)
            passed('GV04-explicit-second-account-selection-lookalike-exclusion-and-mobile-layout')

        before = len(packets)
        popup, job = open_google(app); unlock(popup)
        expect(popup.get_by_text('Google 계정이 여러 개 있습니다. 연결할 계정을 선택하세요.', exact=True)).to_be_visible(timeout=60000)
        receipt = request(app, members[0], {'op': 'status', 'job': job}).json()
        other = login(other_context, members[1])
        other_vault = normal_vault(other, other_context); unlock(other_vault, 1)
        expect(other_vault.get_by_role('button', name='새 항목', exact=True)).to_be_visible(timeout=60000)
        for name in (PRIMARY_TITLE, SECOND_TITLE, DECOY_TITLE):
            expect(other_vault.get_by_role('button', name=name, exact=True)).to_have_count(0)
        recipient_url = VAULT+'/session/kiri-recipient?challenge='+receipt['challenge']
        def own_recipient():
            response = popup.request.get(recipient_url, headers={
                'X-Moosuri-Vault-Account':members[0]['account'], 'Sec-Fetch-Site':'same-origin'})
            assert response.status == 200, 'same_owner_positive_control_failed'
            value = response.json()
            assert value['protocol'] == 'moosuri-google-runtime-v1'
            assert value['binding']['account'] == members[0]['account']
            assert value['binding']['challenge'] == receipt['challenge']
        own_recipient()
        denied = other_vault.request.get(recipient_url,
                    headers={'X-Moosuri-Vault-Account':members[1]['account'], 'Sec-Fetch-Site':'same-origin'})
        evidence['foreign_recipient_status'] = denied.status
        report.write_text(json.dumps(evidence))
        # Adapter::post currently maps the gateway's missing foreign recipient
        # (410) to fixed Unavailable. A generic outage cannot satisfy both controls.
        assert denied.status == 503
        assert denied.json() == {'code':'vault_unavailable'}
        own_recipient()
        unknown = popup.request.get(VAULT+'/session/kiri-recipient?challenge='+str(uuid.uuid4()),
            headers={'X-Moosuri-Vault-Account':members[0]['account'], 'Sec-Fetch-Site':'same-origin'})
        assert unknown.status == 503 and unknown.json() == denied.json()
        evidence['unknown_recipient_status'] = unknown.status
        forged = other_vault.request.get(recipient_url, headers={
            'X-Moosuri-Vault-Account':members[0]['account'], 'Sec-Fetch-Site':'same-origin'})
        evidence['forged_header_recipient_status'] = forged.status
        report.write_text(json.dumps(evidence))
        # This session endpoint takes authority from the cookie, never this header.
        assert forged.status == 503 and forged.json() == {'code':'vault_unavailable'}
        own_recipient()
        assert request(other, members[1], {'op': 'status', 'job': job}).status in (404, 410)
        popup.close()
        expect(app.get_by_text('볼트를 닫아 Google 연결을 취소했습니다.', exact=True)).to_be_visible(timeout=15000)
        settled(app); assert len(packets) == before
        passed('GV05-foreign-member-vault-recipient-denial-and-popup-close-cleanup')

        popup, job = open_google(app); unlock(popup)
        expect(popup.get_by_text('Google 계정이 여러 개 있습니다. 연결할 계정을 선택하세요.', exact=True)).to_be_visible(timeout=60000)
        app.get_by_role('button', name='대화 목록', exact=True).click()
        app.get_by_role('button', name='로그아웃', exact=True).click()
        expect(app.get_by_role('button', name='로그인', exact=True)).to_be_visible(timeout=30000)
        until(app, popup.is_closed)
        settled(app)
        assert request(app, members[0], {'op': 'status', 'job': job}).status == 401
        assert not plaintext
        passed('GV06-owner-logout-closes-selection-and-revokes-prepared-key')
        evidence['created_synthetic_items'] = [PRIMARY_TITLE, SECOND_TITLE, DECOY_TITLE]
        evidence['notes'] = 'Synthetic encrypted items retained for evidence; inspect before rerunning empty-state journey.'
        report.write_text(json.dumps(evidence))
    except Exception as error:
        frame = next((f for f in reversed(traceback.extract_tb(error.__traceback__)) if f.filename.endswith('installed-google-vault.py')), None)
        evidence['failure'] = {'type':type(error).__name__, 'line':frame.lineno if frame else None}
        report.write_text(json.dumps(evidence))
        raise
    finally:
        if app:
            for job in jobs:
                try: request(app, members[0], {'op': 'cancel', 'job': job})
                except Exception: pass
        try:
            deadline = time.monotonic()+45
            while time.monotonic() < deadline and inventory() != baseline:
                time.sleep(0.2)
            evidence['cleanup_clean'] = inventory() == baseline
            report.write_text(json.dumps(evidence))
            assert evidence['cleanup_clean'], 'owned_preflight_cleanup_unconfirmed'
        finally:
            context.close(); other_context.close()
