"""Actual installed reentry before provider submission; retained synthetic Vault.

The guest must already have the two synthetic member Vaults and multiple Google
choices from installed-google-vault.py. Live Deliver is always aborted. The
accepted-provider/completion branches have separate synthetic coverage, not a
passing provider claim here. Only sanitized results and a blank-selection UI
screenshot are recorded; credentials and request bodies remain private.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import time
import traceback
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'
VAULT = 'https://vault.example.com'
BASE = ORIGIN + '/api/v1/personal/google/kiri'
PENDING = 'moosuri.google-kiri-pending.v1:'


def run(browser, args, ssh, target, members):
    def guest(code):
        prelude = "from pathlib import Path\nassert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'\n"
        r = subprocess.run([*ssh, target, 'sudo', '-n', 'python3', '-B', '-'],
                           input=prelude+code, text=True, capture_output=True, timeout=30)
        assert r.returncode == 0, 'guest_check_failed'
        return json.loads(r.stdout)

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

    baseline = {'capsules':0, 'chrome':0}
    assert inventory() == baseline, 'preserve_existing_browser'
    report = args.output_dir/'google-reentry-result.json'
    assert not report.exists(), 'preserve_prior_evidence'
    evidence = {'passed':[], 'provider_authentication':False,
                'delivery_boundary':'browser_abort_before_gateway'}
    evidence['gateway_sha256'] = guest("""import json,hashlib,subprocess
p=subprocess.check_output(['systemctl','show','moosuri-kiri-gateway','-p','MainPID','--value'],text=True).strip()
print(json.dumps(hashlib.sha256(Path('/proc/'+p+'/exe').read_bytes()).hexdigest()))
""")
    contexts = []
    ctx = browser.new_context(ignore_https_errors=True, service_workers='block')
    contexts.append(ctx)
    jobs, starts, delivered = [], [], []
    faults = {'start':False, 'status':False}

    def route_request(route):
        data = route.request.post_data_json
        if data['op'] == 'deliver':
            delivered.append(data['job'])
            route.abort('failed')
            return
        if data['op'] == 'start':
            jobs.append(data['job']); starts.append(time.monotonic())
            if faults['start']:
                faults['start'] = False
                response = route.fetch()
                assert response.status == 200, 'lost_start_must_have_reached_gateway'
                route.abort('failed')
                return
        if data['op'] == 'status' and faults['status']:
            route.abort('failed')
            return
        route.continue_()
    ctx.route(BASE, route_request)

    def until(page, condition, seconds=30):
        deadline = time.monotonic()+seconds
        while time.monotonic() < deadline:
            if condition(): return
            page.wait_for_timeout(200)
        raise AssertionError('bounded_condition_failed')

    def call(page, member, op, job):
        return page.request.post(BASE, data={'op':op, 'job':job}, headers={
            'Origin':ORIGIN, 'Sec-Fetch-Site':'same-origin', 'X-Moosuri-Account':member['account']})

    def login(context, member):
        page = context.new_page(); page.goto(ORIGIN+'/app#connections/gmail')
        page.get_by_label('이메일', exact=True).fill(member['email'])
        page.get_by_label('비밀번호', exact=True).fill(member['password'])
        page.get_by_role('button', name='로그인', exact=True).click()
        expect(page.locator('.member-workspace')).to_be_visible(timeout=30000)
        return page

    def stored(page):
        rows = page.evaluate('(prefix) => Object.entries(sessionStorage).filter(([k]) => k.startsWith(prefix))', PENDING)
        for _, raw in rows:
            data = json.loads(raw)
            assert set(data) <= {'job','account','scope','service','created','updated','read'}
            assert isinstance(data.get('job'), str) and len(data['job']) == 36
        return rows

    def popup_for(page):
        return next((p for p in ctx.pages if p != page and not p.is_closed() and p.url.startswith(VAULT)), None)

    def selection(page):
        until(page, lambda: popup_for(page) is not None)
        popup = popup_for(page)
        expect(popup.get_by_label('볼트 비밀번호', exact=True)).to_be_visible(timeout=30000)
        popup.get_by_label('볼트 비밀번호', exact=True).fill('VaultA42')
        popup.get_by_role('button', name='열기', exact=True).click()
        expect(popup.get_by_text('Google 계정이 여러 개 있습니다. 연결할 계정을 선택하세요.', exact=True)).to_be_visible(timeout=60000)
        assert inventory() == {'capsules':1, 'chrome':0}
        return popup

    def begin(page):
        until(page, lambda: sum(time.monotonic()-at < 62 for at in starts) < 4, 65)
        page.goto(ORIGIN+'/app#connections/gmail')
        page.get_by_role('button', name='Google 연결', exact=True).click()
        return selection(page), jobs[-1]

    def reload_pending(page):
        page.reload()
        expect(page.get_by_role('button', name='이어서 진행', exact=True)).to_be_visible(timeout=30000)
        expect(page.get_by_role('button', name='Google 연결', exact=True)).to_have_count(0)

    def cancelled(page, job):
        until(page, lambda: inventory() == baseline, 45)
        assert call(page, members[0], 'status', job).status == 410
        expect(page.get_by_role('button', name='Google 연결', exact=True)).to_be_enabled(timeout=30000)
        assert not stored(page)
        for popup in list(ctx.pages):
            if popup != page: popup.close()

    def cancel_active(page, job):
        page.get_by_role('button', name='취소', exact=True).click()
        cancelled(page, job)

    def passed(name):
        assert not delivered, 'unexpected_account_selection_or_delivery'
        evidence['passed'].append(name)
        report.write_text(json.dumps(evidence))
        print('PASS '+name, flush=True)

    app = None
    try:
        app = login(ctx, members[0])
        popup, job = begin(app)
        before = len(jobs)
        reload_pending(app)
        app.set_viewport_size({'width':390, 'height':844})
        assert app.evaluate('document.documentElement.scrollWidth <= innerWidth')
        app.screenshot(path=str(args.output_dir/'google-reentry-mobile.png'), full_page=True)
        app.get_by_role('button', name='이어서 진행', exact=True).click()
        selection(app)
        assert len(jobs) == before and call(app, members[0], 'status', job).json()['state'] == 'vault'
        cancel_active(app, job)
        passed('RE01-reload-before-delivery-resumes-same-job-and-key')

        faults['start'] = True
        before = len(jobs)
        app.get_by_role('button', name='Google 연결', exact=True).click()
        until(app, lambda: len(jobs) > before and not faults['start'])
        job = jobs[-1]
        until(app, lambda: call(app, members[0], 'status', job).json().get('state') == 'vault')
        reload_pending(app)
        app.get_by_role('button', name='이어서 진행', exact=True).click()
        selection(app)
        assert len(jobs) == before+1
        cancel_active(app, job)
        passed('RE02-lost-Start-response-resumes-without-second-Start')

        popup, job = begin(app)
        before = len(jobs)
        reload_pending(app)
        faults['status'] = True
        app.get_by_role('button', name='이어서 진행', exact=True).click()
        until(app, lambda: app.get_by_role('button', name='이어서 진행', exact=True).is_visible())
        assert stored(app) and call(app, members[0], 'status', job).json()['state'] == 'vault'
        faults['status'] = False
        app.get_by_role('button', name='이어서 진행', exact=True).click()
        selection(app)
        assert len(jobs) == before
        cancel_active(app, job)
        passed('RE03-preflight-Status-loss-keeps-recoverable-owned-job')

        popup, job = begin(app)
        reload_pending(app)
        app.get_by_role('button', name='저장된 연결 취소', exact=True).click()
        cancelled(app, job)
        popup, job = begin(app)
        reload_pending(app)
        assert call(app, members[0], 'cancel', job).status == 200
        app.get_by_role('button', name='이어서 진행', exact=True).click()
        cancelled(app, job)
        passed('RE04-saved-cancel-and-already-gone-receipt-return-fresh-choice')

        popup, job = begin(app)
        rows = stored(app)
        assert len(rows) == 1
        for member in members:
            other_ctx = browser.new_context(ignore_https_errors=True, service_workers='block')
            contexts.append(other_ctx)
            other_ctx.route(BASE, lambda route: route.abort('failed') if route.request.post_data_json['op'] in ('start','deliver') else route.continue_())
            other = login(other_ctx, member)
            other.evaluate('(rows) => { for (const [k,v] of rows) sessionStorage.setItem(k,v); }', rows)
            other.reload()
            expect(other.locator('.member-workspace')).to_be_visible(timeout=30000)
            expect(other.get_by_role('button', name='이어서 진행', exact=True)).to_have_count(0)
            assert call(other, member, 'status', job).status == 410
            other_ctx.close(); contexts.remove(other_ctx)
        assert call(app, members[0], 'status', job).json()['state'] == 'vault'
        cancel_active(app, job)
        passed('RE05-other-login-and-member-cannot-adopt-copied-metadata')
        evidence['not_executed'] = ['RE03-accepted-provider-delivery', 'RE06-real-completion-and-mail-continuation']
    except Exception as error:
        frame = next((f for f in reversed(traceback.extract_tb(error.__traceback__)) if f.filename.endswith('installed-google-reentry.py')), None)
        evidence['failure'] = {'type':type(error).__name__, 'line':frame.lineno if frame else None}
        raise
    finally:
        faults['status'] = False
        if app:
            for job in jobs:
                try: call(app, members[0], 'cancel', job)
                except Exception: pass
        try:
            deadline = time.monotonic()+45
            while time.monotonic() < deadline and inventory() != baseline:
                time.sleep(0.2)
            evidence['cleanup_clean'] = inventory() == baseline
            report.write_text(json.dumps(evidence))
            assert evidence['cleanup_clean'], 'cleanup_unconfirmed'
        finally:
            for context in contexts: context.close()
