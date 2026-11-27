#!/usr/bin/env python3
"""J01–J08 against installed Linux services; loaded by clean-installed-browser.

Only a SYNTHETIC_DISPOSABLE_VM is admitted. No Google/model calls. Existing two
fixture users are preserved. Two extra synthetic users exercise web admission;
root changes only capacity and targeted expiry/failure injection in this fixture.
Never print tokens, passwords, request bodies, HTML or browser traces.
"""
import json
from pathlib import Path
import secrets
import subprocess
import time
from playwright.sync_api import expect

ORIGIN = 'https://assistant.example.com'
BASE = '/api/v1/personal/'


def run(browser, args, ssh, target, members):
    def guest(code, timeout=30):
        prelude = "from pathlib import Path\nassert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'\n"
        r = subprocess.run([*ssh, target, 'sudo', '-n', 'python3', '-B', '-'],
                           input=prelude + code, text=True, capture_output=True, timeout=timeout)
        assert r.returncode == 0, 'guest_fixture_operation_failed'
        return r.stdout.strip()

    state_file = args.output_dir / 'invitation-fixture.json'
    progress = args.output_dir / 'invitation-progress.json'
    def passed(code):
        completed.append(code)
        progress.write_text(json.dumps({'passed': completed}))
        print('PASS installed invitation ' + code, flush=True)

    def context(width=1280):
        return browser.new_context(ignore_https_errors=True, service_workers='block',
                                   viewport={'width': width, 'height': 900})

    def login(ctx, email, password, admin=False):
        page = ctx.new_page()
        page.goto(ORIGIN + ('/app#admin' if admin else '/app'))
        page.get_by_label('이메일', exact=True).fill(email)
        page.get_by_label('비밀번호', exact=True).fill(password)
        page.get_by_role('button', name='로그인', exact=True).click()
        expect(page.locator('.member-workspace')).to_be_visible(timeout=30000)
        expect(page.get_by_role('heading', name='임시 비밀번호 변경', exact=True)).to_have_count(0)
        return page

    completed = json.loads(progress.read_text())['passed'] if args.post_invitation_reboot else []
    if args.post_invitation_reboot:
        state = json.loads(state_file.read_text())
        assert state['completed']
        for index, item in enumerate(state['users']):
            ctx = context(390)
            try:
                page = login(ctx, item['email'], item['password'])
                assert page.request.post(ORIGIN+'/api/v1/auth/login', headers={'Origin':ORIGIN},
                    data={'email':item['email'],'password':'replacement42'}).status == 401
                assert page.request.get(ORIGIN + BASE + 'admin/members', headers={
                    'Sec-Fetch-Site': 'same-origin', 'X-Moosuri-Account': item['account']}).status == 403
                page.goto(ORIGIN+'/app#connections/vault')
                with ctx.expect_page() as opened:
                    page.get_by_role('button',name='볼트 열기',exact=True).click()
                vault=opened.value
                expect(vault.get_by_label('볼트 비밀번호',exact=True)).to_be_visible(timeout=30000)
                if 'vault_password' not in item:
                    item['vault_password']=secrets.token_urlsafe(18)
                    state_file.write_text(json.dumps(state))
                vault.get_by_label('볼트 비밀번호',exact=True).fill(item['vault_password'])
                if vault.get_by_role('heading',name='내 볼트 만들기',exact=True).is_visible():
                    vault.get_by_role('button',name='만들기',exact=True).click()
                    expect(vault.get_by_label('새 복구 키')).to_be_visible(timeout=60000)
                    vault.get_by_label('기록한 복구 키',exact=True).fill(vault.get_by_label('새 복구 키').input_value())
                    vault.get_by_role('button',name='저장하고 열기',exact=True).click()
                else:
                    vault.get_by_role('button',name='열기',exact=True).click()
                expect(vault.get_by_role('button',name='새 항목',exact=True)).to_be_visible(timeout=60000)
                title=f'Synthetic invited {index+1}'
                expect(vault.get_by_text(f'Synthetic invited {2-index}',exact=True)).to_have_count(0)
                if vault.get_by_text(title,exact=True).count()==0:
                    vault.get_by_role('button',name='새 항목',exact=True).click()
                    vault.get_by_label('이름',exact=True).fill(title)
                    vault.get_by_label('사용자 이름',exact=True).fill(item['email'])
                    vault.get_by_label('주소',exact=True).fill('https://example.com')
                    vault.get_by_label('비밀번호 또는 키',exact=True).fill(secrets.token_urlsafe(18))
                    vault.get_by_role('button',name='저장',exact=True).click()
                expect(vault.get_by_text(title,exact=True)).to_be_visible(timeout=30000)
            finally:
                ctx.close()
        passed('J01/J04/J07 real reboot retained both web accounts; original passwords, replacement denial, separate Vaults and member roles')
        return

    assert not state_file.exists(), 'inspect_existing_invitation_fixture_before_resuming'
    state = {'completed': False, 'users': []}
    state_file.write_text(json.dumps(state))
    admin_ctx, member_ctx = context(), context(390)
    admin = login(admin_ctx, members[0]['email'], members[0]['password'], True)
    expect(admin.get_by_role('button', name='회원 초대', exact=True)).to_be_visible()

    def roster():
        admin.get_by_role('button', name='목록 새로고침', exact=True).click()
        expect(admin.get_by_role('button', name='목록 새로고침', exact=True)).to_be_enabled()

    def row(email):
        return admin.locator('li.management-person').filter(has_text=email)

    def open_row(email):
        current = row(email)
        details = current.locator('details')
        if details.get_attribute('open') is None:
            details.locator('summary').click()
        return current

    def invite(email, name):
        admin.get_by_role('button', name='회원 초대', exact=True).click()
        admin.get_by_label('이름', exact=True).fill(name)
        admin.get_by_label('이메일', exact=True).fill(email)
        admin.get_by_role('button', name='초대 요청 등록', exact=True).click()
        receipt = admin.get_by_role('region', name='최근 초대', exact=True)
        expect(receipt).to_contain_text(email)
        admin.get_by_role('button', name='접수 내역 닫기', exact=True).click()
        return issue(email)

    def issue(email):
        current = open_row(email)
        button = current.get_by_role('button', name='새 초대 링크 만들기', exact=True)
        if button.count() == 0:
            button = current.get_by_role('button', name='초대 링크 만들기', exact=True)
        button.click()
        link = current.get_by_label('초대 링크', exact=True)
        expect(link).to_be_visible(timeout=30000)
        result = link.input_value()
        assert result.startswith(ORIGIN + '/join#') and len(result.split('#')[1]) == 43
        return result

    def change(email, action, question):
        open_row(email).get_by_role('button', name=action, exact=True).click()
        dialog = admin.get_by_role('dialog', name=question, exact=True)
        dialog.get_by_role('button', name=action, exact=True).click()
        expect(dialog).to_have_count(0)
        roster()

    def join(link):
        page = member_ctx.new_page()
        page.goto(link)
        expect(page).to_have_url(ORIGIN + '/join')
        return page

    def reject(link):
        page = join(link)
        expect(page.get_by_role('alert')).to_contain_text('새 링크', timeout=30000)
        expect(page.get_by_label('새 비밀번호', exact=True)).to_have_count(0)
        page.close()

    def submit(page, password):
        page.get_by_label('새 비밀번호', exact=True).fill(password)
        page.get_by_label('비밀번호 확인', exact=True).fill(password)
        page.get_by_role('button', name='가입 완료', exact=True).click()

    def prepare(email, capacity):
        # This is a capacity failure/recovery injection on the marked fixture,
        # not a product credential completion command.
        guest("""import json,pwd,grp,subprocess
p=Path('/etc/moosuri/kiri-onboard-policy.json'); policy=json.loads(p.read_text())
c=json.loads(Path('/etc/moosuri/kiri-members.json').read_text())
enrolled=json.loads(Path('/var/lib/moosuri-control/enrolled/members.json').read_text())['members']
used={m['uid'] for m in enrolled}|{x.pw_uid for x in pwd.getpwall()}|{x.gr_gid for x in grp.getgrall()}
for uid in range(43000,43020):
 if len(set(policy['uid_pool'])-used)>=CAPACITY-len(enrolled):break
 if uid not in used and uid not in policy['uid_pool']:policy['uid_pool'].append(uid)
policy['max_total_volume_bytes']=c['max_volume_bytes']*CAPACITY
p.write_text(json.dumps(policy))
r=subprocess.run(['systemctl','start','moosuri-kiri-onboard.service'],capture_output=True)
assert r.returncode==0
""".replace('CAPACITY', str(capacity)), 90)
        deadline = time.monotonic() + 160
        while time.monotonic() < deadline:
            ready = guest("""import json
q=json.loads(Path('/var/lib/moosuri-control/invitations/web-onboarding.json').read_text())
for req,claim in q['claims'].items():
 if claim['invitation']['email']==EMAIL:
  p=Path('/var/lib/moosuri-control/invitations')/(req+'.json')
  if p.exists() and json.loads(p.read_text())['stage']=='password_required':print('ready')
""".replace('EMAIL', repr(email)))
            if ready == 'ready':
                return
            time.sleep(2)
        raise AssertionError('installed_timer_did_not_prepare_member')

    def identity(email):
        raw = guest("""import json
q=json.loads(Path('/var/lib/moosuri-control/invitations/web-onboarding.json').read_text())
for req,c in q['claims'].items():
 if c['invitation']['email']==EMAIL:print(json.dumps({'request':req,'account':c['invitation']['account_id'],'volume':c['invitation']['volume_id'],'uid':c['profile']['uid']}))
""".replace('EMAIL', repr(email)))
        return json.loads(raw)

    try:
        email = 'web-journey@example.com'
        link = invite(email, 'Web Journey')
        # Two initial members exhaust the fresh fixture's capacity.
        guest("import subprocess\nsubprocess.run(['systemctl','start','moosuri-kiri-onboard.service'],check=True,capture_output=True)",90)
        page = join(link)
        submit(page, 'Capacity-only42')
        expect(page.get_by_role('alert')).to_contain_text('공간을 준비', timeout=30000)
        expect(page.get_by_label('새 비밀번호', exact=True)).to_have_value('')
        page.close()
        prepare(email, 3)
        before = identity(email)
        passed('J06 actual capacity wait and same-reservation preparation after capacity increase')
        old = link; link = issue(email); reject(old)
        guest("""import sqlite3,fcntl
root=Path('/var/lib/moosuri-control/registry')
import os
fd=os.open(root,os.O_RDONLY);fcntl.flock(fd,fcntl.LOCK_EX)
c=sqlite3.connect(root/'account-runtimes.sqlite')
assert c.execute("UPDATE installation_invitation_links SET expires_at=0 WHERE request_id=(SELECT request_id FROM allocations WHERE email='web-journey@example.com')").rowcount==1
c.commit();c.close();os.close(fd)
""")
        reject(link); link = issue(email)
        passed('J02 real expired/replaced links refused; allocation retained')
        change(email, '계정 정지', '계정 이용을 정지할까요?')
        reject(link)
        change(email, '정지 해제', '계정 정지를 해제할까요?')
        reject(link); link = issue(email)
        assert identity(email) == before
        passed('J05 actual member suspend/resume requires a new link and preserves allocation')
        page = join(link)
        expect(page.get_by_label('새 비밀번호', exact=True)).to_be_visible()
        page.get_by_label('새 비밀번호', exact=True).fill('1234567')
        page.get_by_label('비밀번호 확인', exact=True).fill('1234567')
        expect(page.get_by_role('button', name='가입 완료', exact=True)).to_be_disabled()
        password = secrets.token_urlsafe(18)
        state['users'].append({'email':email,'password':password,**before})
        state_file.write_text(json.dumps(state))
        observed = []
        def lost(route):
            response = route.fetch(timeout=90000)
            observed.append(response.status)
            route.abort('failed')
        page.route('**/personal/invitations/redeem', lost, times=1)
        submit(page,password)
        expect(page.get_by_role('alert')).to_contain_text('결과를 확인하지', timeout=100000)
        assert observed == [200], 'actual_redemption_did_not_complete'
        page.get_by_role('button', name='상태 다시 확인', exact=True).click()
        expect(page.get_by_role('status')).to_contain_text('이미 설정한 비밀번호', timeout=30000)
        page.close()
        account_page = login(member_ctx, email, password)
        assert identity(email) == before
        passed('J01/J03 mobile web join, real lost response, original-password login without forced change')
        replay = join(link)
        expect(replay.get_by_role('status')).to_contain_text('이미 설정한 비밀번호')
        result = replay.request.post(ORIGIN + BASE + 'invitations/redeem',
            headers={'Origin':ORIGIN},data={'token':link.split('#')[1],'password':'replacement42'})
        assert result.status == 200 and result.json()['password_set'] is False
        assert replay.request.post(ORIGIN+'/api/v1/auth/login',headers={'Origin':ORIGIN},
            data={'email':email,'password':'replacement42'}).status == 401
        assert replay.request.get(ORIGIN+BASE+'admin/members',headers={'Sec-Fetch-Site':'same-origin','X-Moosuri-Account':before['account']}).status == 403
        assert replay.request.post(ORIGIN+BASE+'invitations/status',headers={'Origin':'https://foreign.example'},data={'token':link.split('#')[1]}).status == 403
        assert not any(password in value or link.split('#')[1] in value for value in replay.evaluate('Object.values(localStorage).concat(Object.values(sessionStorage))'))
        replay.reload()
        expect(replay.get_by_role('alert')).to_contain_text('초대 링크를 다시')
        replay.close()
        passed('J07/J08 completed replay, wrong password, member/admin and origin isolation, reload privacy')
        # The second web member is reserved for a precise process interruption
        # after private initialization and before route activation.
        member_ctx.close()
        member_ctx = context(390)
        guest("import subprocess\nsubprocess.run(['systemctl','restart','moosuri-kiri-gateway'],check=True,capture_output=True)")
        roster()
        email2='web-interrupted@example.com'
        link2=invite(email2,'Interrupted Journey')
        prepare(email2,4)
        ident2=identity(email2)
        password2=secrets.token_urlsafe(18)
        state['users'].append({'email':email2,'password':password2,**ident2})
        state_file.write_text(json.dumps(state))
        watcher="""from pathlib import Path
import json,os,signal,subprocess,time
assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'
p=Path('/var/lib/moosuri-control/invitations/REQUEST.json')
end=time.monotonic()+55
while time.monotonic()<end:
 try:stage=json.loads(p.read_text())['stage']
 except (FileNotFoundError,json.JSONDecodeError):stage='unknown'
 if stage=='initialized':
  pid=int(subprocess.check_output(['systemctl','show','moosuri-kiri-gateway','-p','MainPID','--value']))
  os.kill(pid,signal.SIGSTOP)
  stage=json.loads(p.read_text())['stage']
  Path('/root/moosuri-invitation-cut.json').write_text(json.dumps({'stage':stage,'pid':pid}))
  os.kill(pid,signal.SIGKILL)
  break
 time.sleep(.001)
else:raise SystemExit(1)
""".replace('REQUEST',ident2['request'])
        guest("import subprocess\nsubprocess.run(['systemd-run','--unit=moosuri-invitation-cut','--property=RuntimeMaxSec=60s','python3','-B','-c',"+repr(watcher)+"],check=True,capture_output=True)")
        cut=join(link2);submit(cut,password2)
        expect(cut.get_by_role('alert')).to_be_visible(timeout=90000)
        evidence=json.loads(guest("print(Path('/root/moosuri-invitation-cut.json').read_text())"))
        assert evidence['stage']=='initialized','interruption_missed_activation_boundary'
        time.sleep(5)
        guest("import subprocess\nsubprocess.run(['systemctl','start','moosuri-kiri-onboard.service'],check=True,capture_output=True)",90)
        route=guest("import sqlite3\nc=sqlite3.connect('/var/lib/moosuri-control/registry/account-runtimes.sqlite');print(c.execute(\"SELECT route_state FROM allocations WHERE email='web-interrupted@example.com'\").fetchone()[0])")
        assert route=='reserved','timer_bypassed_link_activation'
        cut.close()
        resumed=join(link2)
        submit(resumed,'replacement42')
        expect(resumed.get_by_role('status')).to_contain_text('이미 설정한 비밀번호',timeout=90000)
        login(member_ctx,email2,password2).close()
        assert identity(email2)==ident2 and ident2['uid']!=before['uid'] and ident2['volume']!=before['volume']
        passed('J04 real gateway interruption after initialization, timer fence, original-password resume')
        state['completed']=True;state_file.write_text(json.dumps(state))
        passed('J01-J08 installed branches complete; post-reboot check remains separate')
    finally:
        admin_ctx.close();member_ctx.close()
