#!/usr/bin/env python3
"""Real Chromium -> TLS -> installed gateway/Vault in a marked disposable guest.

Run on the Linux VM host after clean-installed.py. No API response is mocked.
Only the two synthetic origins can traverse the local CONNECT proxy. By default
no provider is called. --check-model-rejection makes the installed server check
one deliberately invalid synthetic API key against Anthropic's model catalog;
it never requests inference. Root-private synthetic fixture credentials are read
through the guest SSH connection into memory and never printed or screenshotted.
"""
import argparse
import contextlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import select
import shlex
import socket
import subprocess
import sys
from threading import Thread
import time
import traceback
from playwright.sync_api import expect, sync_playwright

ORIGIN = 'https://assistant.example.com'
VAULT = 'https://vault.example.com'


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--vm-root', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--check-model-rejection', action='store_true',
                        help='Check failed model setup via UI; one invalid-key catalog call, no inference')
    parser.add_argument('--matched-rollout-dir', type=Path)
    parser.add_argument('--expected-revision')
    parser.add_argument('--invitation-journeys', action='store_true',
                        help='Run actual web joining/recovery branches on this synthetic installation')
    parser.add_argument('--post-invitation-reboot', action='store_true')
    parser.add_argument('--claude-journeys', action='store_true',
                        help='Admin UI official Claude URL/cancel only; no account authentication or inference')
    parser.add_argument('--claude-structure-only', action='store_true',
                        help='Only U01/U03; do not initiate provider login preparation')
    args = parser.parse_args()
    assert sys.platform == 'linux'
    os.umask(0o077)
    root = args.vm_root.resolve()
    ssh = ['ssh', '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
           '-o', 'UserKnownHostsFile=' + str(root / 'known_hosts'),
           '-i', str(root / 'guest_key'), '-p', (root / 'port').read_text().strip()]
    target = 'lab@127.0.0.1'
    marker = subprocess.run([*ssh, target, 'sudo', '-n', 'cat', '/etc/moosuri-kiri-lab-marker'],
                            capture_output=True, check=True, timeout=20).stdout.strip()
    assert marker == b'SYNTHETIC_DISPOSABLE_VM'
    assert bool(args.matched_rollout_dir) == bool(args.expected_revision)
    if args.matched_rollout_dir:
        check = '''import hashlib,json,sys,subprocess
from pathlib import Path
e=json.loads((Path(sys.argv[1])/'complete.json').read_text())
assert e['source_revision']==sys.argv[2]
for unit,binary in [('moosuri-kiri-gateway','moosuri'),('moosuri-kiri-vault','moosuri-vault-service')]:
 p=subprocess.run(['systemctl','show',unit,'-p','MainPID','--value'],capture_output=True,text=True,check=True).stdout.strip()
 assert int(p)>1
 assert hashlib.sha256((Path('/proc')/p/'exe').read_bytes()).hexdigest()==e['bundle_files'][binary]
print('matched_running_revision_verified')
'''
        verified = subprocess.run([*ssh, target, 'sudo', '-n', 'python3', '-B', '-c',
                                   shlex.quote(check), shlex.quote(str(args.matched_rollout_dir)),
                                   shlex.quote(args.expected_revision)], capture_output=True, timeout=30)
        assert verified.returncode == 0 and verified.stdout.strip() == b'matched_running_revision_verified'
    data = subprocess.run([*ssh, target, 'sudo', '-n', 'cat', '/root/moosuri-clean-acceptance-state.json'],
                          capture_output=True, check=True, timeout=20).stdout
    members = json.loads(data)['members']
    assert [m['email'] for m in members] == ['install-test@example.com', 'second-install@example.com']
    args.output_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    tls_port = free_port()
    tunnel = subprocess.Popen([*ssh, '-N', '-o', 'ExitOnForwardFailure=yes',
                               '-L', f'127.0.0.1:{tls_port}:127.0.0.1:443', target],
                              stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    class Proxy(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_CONNECT(self):
            try:
                if self.path not in ('assistant.example.com:443', 'vault.example.com:443'):
                    self.send_error(403)
                    return
                with socket.create_connection(('127.0.0.1', tls_port), timeout=10) as upstream:
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.flush()
                    peers = [self.connection, upstream]
                    while True:
                        ready, _, _ = select.select(peers, [], [], 30)
                        if not ready:
                            return
                        for source in ready:
                            block = source.recv(65536)
                            if not block:
                                return
                            (upstream if source is self.connection else self.connection).sendall(block)
            except (OSError, ConnectionError):
                return

    proxy = ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
    proxy.daemon_threads = True
    Thread(target=proxy.serve_forever, daemon=True).start()
    try:
        for _ in range(100):
            assert tunnel.poll() is None, 'guest_tunnel_failed'
            try:
                with socket.create_connection(('127.0.0.1', tls_port), timeout=0.1):
                    break
            except OSError:
                time.sleep(0.1)
        with sync_playwright() as pw:
            browser = pw.chromium.launch(channel='chromium', headless=True,
                        proxy={'server': f'http://127.0.0.1:{proxy.server_port}'})
            try:
                if args.claude_journeys:
                    import runpy
                    module = runpy.run_path(str(Path(__file__).with_name('installed-claude-ui.py')))
                    module['run'](browser, args, members)
                    return
                if args.invitation_journeys or args.post_invitation_reboot:
                    import runpy
                    module = runpy.run_path(str(Path(__file__).with_name('installed-invitation-journeys.py')))
                    module['run'](browser, args, ssh, target, members)
                    return
                contexts, vaults, forbidden = [], [], []
                for index, member in enumerate(members):
                    context = browser.new_context(ignore_https_errors=True, service_workers='block',
                                viewport={'width': 1280 if index == 0 else 390, 'height': 900})
                    contexts.append(context)
                    page = context.new_page()
                    page.goto(ORIGIN + '/app#connections/vault')
                    page.get_by_label('이메일', exact=True).fill(member['email'])
                    page.get_by_label('비밀번호', exact=True).fill(member['password'])
                    page.get_by_role('button', name='로그인', exact=True).click()
                    expect(page.get_by_role('button', name='볼트 열기', exact=True)).to_be_visible(timeout=30000)
                    with context.expect_page() as opened:
                        page.get_by_role('button', name='볼트 열기', exact=True).click()
                    vault = opened.value
                    expect(vault.get_by_label('볼트 비밀번호', exact=True)).to_be_visible(timeout=30000)
                    private_values = [f'Vault{chr(65 + index)}42', 'Synthetic-secret-' + secrets.token_hex(12)]
                    def inspect_request(request, values=private_values):
                        if request.url.startswith(VAULT + '/') and request.method in ('POST', 'PUT'):
                            body = request.post_data_buffer or b''
                            if any(v.encode() in body for v in values):
                                forbidden.append('plaintext_vault_value_sent')
                    context.on('request', inspect_request)
                    creating = vault.get_by_role('heading', name='내 볼트 만들기', exact=True).is_visible()
                    if creating:
                        vault.get_by_label('볼트 비밀번호', exact=True).fill('1234567')
                        expect(vault.get_by_role('button', name='만들기', exact=True)).to_be_disabled()
                        vault.get_by_label('볼트 비밀번호', exact=True).fill(private_values[0])
                        vault.get_by_role('button', name='만들기', exact=True).click()
                        try:
                            expect(vault.get_by_label('새 복구 키')).to_be_visible(timeout=60000)
                        except AssertionError:
                            notices = vault.get_by_role('status').all_text_contents()
                            known = ['연결을 확인하지 못했습니다. 잠시 뒤 다시 시도해 주세요.',
                                     '입력 또는 파일 형식을 확인해 주세요.']
                            print('Vault setup failure category:',
                                  next((text for text in notices if text in known), 'unclassified'), flush=True)
                            raise
                        recovery = vault.get_by_label('새 복구 키').input_value()
                        private_values.append(recovery)
                        vault.get_by_label('기록한 복구 키', exact=True).fill(recovery)
                        vault.get_by_role('button', name='저장하고 열기', exact=True).click()
                    else:
                        vault.get_by_label('볼트 비밀번호', exact=True).fill(private_values[0])
                        vault.get_by_role('button', name='열기', exact=True).click()
                    expect(vault.get_by_role('button', name='새 항목', exact=True)).to_be_visible(timeout=60000)
                    title = f'Synthetic {chr(65 + index)} item'
                    other = f'Synthetic {chr(66 - index)} item'
                    expect(vault.get_by_text(other, exact=True)).to_have_count(0)
                    if vault.get_by_text(title, exact=True).count() == 0:
                        vault.get_by_role('button', name='새 항목', exact=True).click()
                        vault.get_by_label('이름', exact=True).fill(title)
                        vault.get_by_label('사용자 이름', exact=True).fill(member['email'])
                        vault.get_by_label('주소', exact=True).fill('https://example.com')
                        vault.get_by_label('비밀번호 또는 키', exact=True).fill(private_values[1])
                        vault.get_by_role('button', name='저장', exact=True).click()
                    expect(vault.get_by_text(title, exact=True)).to_be_visible(timeout=30000)
                    vault.get_by_role('button', name='잠그기', exact=True).click()
                    vault.get_by_label('볼트 비밀번호', exact=True).fill('wrong-vault-password')
                    vault.get_by_role('button', name='열기', exact=True).click()
                    expect(vault.get_by_text('비밀번호 또는 복구 키를 확인해 주세요.', exact=True)).to_be_visible(timeout=60000)
                    expect(vault.get_by_role('button', name='새 항목', exact=True)).to_have_count(0)
                    vault.get_by_label('볼트 비밀번호', exact=True).fill(private_values[0])
                    vault.get_by_role('button', name='열기', exact=True).click()
                    expect(vault.get_by_text(title, exact=True)).to_be_visible(timeout=60000)
                    expect(vault.get_by_text(other, exact=True)).to_have_count(0)
                    assert vault.evaluate('document.documentElement.scrollWidth <= innerWidth'), 'vault_overflow'
                    vault.screenshot(path=str(args.output_dir / f'vault-member-{index + 1}.png'), full_page=True)
                    vaults.append(vault)
                    print(f'PASS installed browser member {index + 1}: UI login/handoff, '
                          f'{"new 8-character Vault/recovery" if creating else "existing synthetic Vault reopen"}, '
                          'encrypted save, wrong-password denial, reopen, member isolation/layout', flush=True)
                    if args.check_model_rejection:
                        page.goto(ORIGIN + '/app#admin/engine')
                        if index == 0:
                            key = page.get_by_label('Anthropic API 키', exact=True)
                            expect(key).to_be_visible(timeout=30000)
                            expect(page.get_by_label('모델 ID', exact=True)).to_have_value('claude-sonnet-5-5')
                            setup = page.locator('form').filter(has=key)
                            expect(setup.get_by_label('추론 강도', exact=False)).to_have_value('medium')
                            key.fill('synthetic-invalid-not-an-api-key')
                            with page.expect_response(lambda r: r.url.endswith('/personal/admin/engine/setup')
                                                      and r.request.method == 'POST') as failed:
                                page.get_by_role('button', name='연결 확인 후 저장', exact=True).click()
                            reply = failed.value
                            assert reply.status == 502 and reply.json() == {
                                'code': 'model_provider_verification_failed'}, 'model_failure_not_reported'
                            expect(key).to_have_value('')
                            expect(page.get_by_role('alert').filter(has_text='연결을 확인하지 못했습니다.')).to_be_visible(timeout=30000)
                            page.reload()
                            expect(page.get_by_label('Anthropic API 키', exact=True)).to_be_visible(timeout=30000)
                            expect(page.get_by_text('설치에 연결 설정이 등록되어 있습니다.', exact=False)).to_have_count(0)
                            print('PASS M03 partial: installed UI reports provider verification failure, clears input, '
                                  'remains unconfigured after reload; invalid-key vs network failure, inference '
                                  'and valid-key replacement are not distinguished/tested', flush=True)
                        else:
                            # Direct entry to the management route cannot expose the setup form.
                            expect(page.locator('.member-workspace')).to_be_visible(timeout=30000)
                            expect(page.get_by_label('Anthropic API 키', exact=True)).to_have_count(0)
                            expect(page.get_by_role('button', name='연결 확인 후 저장', exact=True)).to_have_count(0)
                            print('PASS installed non-admin: direct engine route exposes no credential form', flush=True)
                assert not forbidden, 'plaintext_vault_value_sent'
                for context in contexts:
                    context.close()
            finally:
                browser.close()
    finally:
        proxy.shutdown()
        proxy.server_close()
        stop(tunnel)


if __name__ == '__main__':
    if not __debug__:
        raise SystemExit('Run without Python optimization; acceptance assertions are required')
    try:
        main()
    except Exception as error:
        frames = traceback.extract_tb(error.__traceback__)
        frame = next((frame for frame in reversed(frames)
                      if Path(frame.filename).name in (Path(__file__).name, 'installed-invitation-journeys.py')), frames[-1])
        print(f'FAIL installed-browser: {type(error).__name__} at {Path(frame.filename).name}:{frame.lineno}', file=sys.stderr)
        raise SystemExit(1)
