#!/usr/bin/env python3
"""Real Chromium -> TLS -> installed gateway/Vault in a marked disposable guest.

Run on the Linux VM host after clean-installed.py. No API response is mocked.
Only the two synthetic origins can traverse the local CONNECT proxy; no Google
or model provider is called. Root-private synthetic fixture credentials are read
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
    try:
        main()
    except Exception as error:
        frames = traceback.extract_tb(error.__traceback__)
        frame = next((frame for frame in reversed(frames)
                      if Path(frame.filename).name == Path(__file__).name), frames[-1])
        print(f'FAIL installed-browser: {type(error).__name__} at {Path(frame.filename).name}:{frame.lineno}', file=sys.stderr)
        raise SystemExit(1)
