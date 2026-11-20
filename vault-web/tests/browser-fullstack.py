"""Run the real assistant handoff and vault WASM UI against two private local processes."""

import argparse
from contextlib import ExitStack
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
from urllib.error import URLError
from urllib.request import ProxyHandler, build_opener

from playwright.sync_api import expect, sync_playwright


def listen_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def start(stack, args, ready):
    process = subprocess.Popen(
        args,
        env={"PATH": os.defpath},
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    def stop():
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

    stack.callback(stop)
    http = build_opener(ProxyHandler({}))
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline and process.poll() is None:
        try:
            with http.open(ready, timeout=1) as response:
                if response.status == 204:
                    return
        except URLError:
            time.sleep(0.1)
    raise RuntimeError(f"{Path(args[0]).name} did not become ready")


def exercise(context, origin, vault_origin, account, login_password):
    page = context.new_page()
    page.goto(origin + "/app")
    page.get_by_label("Email").fill(account)
    page.get_by_label("Password").fill(login_password)
    page.get_by_role("button", name="Sign in").click()
    page.get_by_role("button", name="Open conversations").click()
    with page.expect_popup() as popup:
        page.get_by_role("dialog").get_by_role("button", name="볼트 열기", exact=True).click()
    vault = popup.value
    vault.set_default_timeout(20000)
    vault.get_by_role("heading", name="내 볼트 만들기").wait_for()
    assert vault.url.startswith(vault_origin + "/")
    print("PASS VWEB-01 signed-in assistant opens a separate authenticated vault window", flush=True)

    password = secrets.token_urlsafe(28)
    vault.get_by_label("볼트 비밀번호", exact=True).fill(password)
    vault.get_by_role("button", name="만들기", exact=True).click()
    recovery = vault.get_by_label("새 복구 키").input_value()
    assert recovery and recovery != password
    vault.get_by_label("기록한 복구 키").fill(recovery)
    vault.get_by_role("button", name="저장하고 열기").click()
    create = vault.get_by_role("button", name="새 항목", exact=True)
    create.wait_for()
    print("PASS VWEB-02 create a vault and confirm the recovery key in the WASM client", flush=True)

    create.click()
    title = "Synthetic vault login"
    secret = secrets.token_urlsafe(24)
    vault.get_by_label("이름", exact=True).fill(title)
    vault.get_by_label("사용자 이름", exact=True).fill("fixture-member")
    vault.get_by_label("주소", exact=True).fill("https://example.test/login")
    vault.get_by_label("비밀번호 또는 키").fill(secret)
    vault.get_by_role("button", name="저장", exact=True).click()
    vault.get_by_role("button", name=title, exact=True).click()
    expect(vault.get_by_text("fixture-member", exact=True)).to_be_visible()
    assert not vault.locator("pre").count()
    vault.get_by_role("button", name="비밀번호 보기", exact=True).click()
    assert vault.locator("pre").inner_text() == secret
    vault.get_by_role("button", name="지금 닫기").click()
    expect(vault.locator("pre")).to_have_count(0)
    print("PASS VWEB-03 save an encrypted login and explicitly reveal and hide its secret", flush=True)

    renamed = "Renamed Linux fixture"
    vault.get_by_role("button", name="수정", exact=True).click()
    vault.get_by_label("이름", exact=True).fill(renamed)
    vault.get_by_role("button", name="저장", exact=True).click()
    vault.get_by_role("heading", name=renamed).wait_for()
    vault.get_by_role("button", name="뒤로", exact=True).click()
    vault.get_by_role("button", name="항목 찾기").click()
    search = vault.get_by_role("searchbox")
    search.fill("absent fixture")
    expect(vault.get_by_role("button", name=renamed)).to_have_count(0)
    search.fill("Renamed")
    expect(vault.get_by_role("button", name=renamed)).to_be_visible()
    print("PASS VWEB-04 edit and search the saved login in the live client", flush=True)

    vault.get_by_role("button", name="잠그기", exact=True).click()
    vault.get_by_label("볼트 비밀번호", exact=True).fill(secrets.token_urlsafe(22))
    vault.get_by_role("button", name="열기", exact=True).click()
    expect(vault.get_by_text("비밀번호 또는 복구 키를 확인해 주세요.", exact=True)).to_be_visible(
        timeout=20000
    )
    expect(vault.get_by_role("button", name=renamed)).to_have_count(0)
    vault.get_by_label("복구 키로 열기").check()
    vault.get_by_label("복구 키", exact=True).fill(recovery)
    vault.get_by_role("button", name="열기", exact=True).click()
    vault.get_by_role("button", name=renamed).click()
    print("PASS VWEB-05 lock, reject a wrong password and recover the same item with its key", flush=True)

    vault.get_by_role("button", name="삭제", exact=True).click()
    vault.get_by_role("button", name="취소", exact=True).click()
    expect(vault.get_by_role("heading", name=renamed)).to_be_visible()
    vault.get_by_role("button", name="삭제", exact=True).click()
    vault.get_by_role("button", name="항목 삭제", exact=True).click()
    create.wait_for()
    expect(vault.get_by_role("button", name=renamed)).to_have_count(0)
    vault.reload()
    vault.get_by_label("볼트 비밀번호", exact=True).fill(password)
    vault.get_by_role("button", name="열기", exact=True).click()
    create.wait_for()
    expect(vault.get_by_role("button", name=renamed)).to_have_count(0)
    print("PASS VWEB-06 cancel deletion then confirm and verify it after reload and unlock", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assistant", type=Path)
    parser.add_argument("vault", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="moosuri-vault-ui-") as tmp, ExitStack() as stack:
        root = Path(tmp)
        account = "vault-owner@example.test"
        login_password = secrets.token_urlsafe(24)
        owner_file, adapter_file = root / "password", root / "adapter"
        for path, value in ((owner_file, login_password), (adapter_file, secrets.token_hex(32))):
            path.write_text(value + "\n")
            path.chmod(0o600)
        app = args.assistant.resolve(strict=True)
        service = args.vault.resolve(strict=True)
        ports = set()
        while len(ports) < 3:
            ports.add(listen_port())
        app_port, vault_port, adapter_port = sorted(ports)
        origin, vault_origin = f"http://127.0.0.1:{app_port}", f"http://localhost:{vault_port}"
        loopback = f"127.0.0.1:{adapter_port}"
        subprocess.run(
            [
                app,
                "init",
                "--data-dir",
                root / "assistant",
                "--name",
                "Vault Fixture",
                "--email",
                account,
                "--password-file",
                owner_file,
            ],
            env={"PATH": os.defpath},
            check=True,
            capture_output=True,
        )
        owner_file.unlink()
        start(
            stack,
            [
                app,
                "serve",
                "--data-dir",
                root / "assistant",
                "--dev",
                "--bind",
                f"127.0.0.1:{app_port}",
                "--origin",
                origin,
                "--vault-origin",
                vault_origin,
                "--vault-adapter-bind",
                loopback,
                "--vault-adapter-key-file",
                adapter_file,
            ],
            origin + "/health/ready",
        )
        start(
            stack,
            [
                service,
                "--data-dir",
                root / "vault",
                "--dev",
                "--bind",
                f"127.0.0.1:{vault_port}",
                "--origin",
                vault_origin,
                "--assistant-origin",
                origin,
                "--adapter-url",
                "http://" + loopback + "/internal/v1/vault",
                "--adapter-key-file",
                adapter_file,
            ],
            vault_origin + "/health/live",
        )
        with sync_playwright() as pw:
            browser = pw.chromium.launch(channel="chromium", args=["--disable-background-networking"])
            try:
                context = browser.new_context(viewport={"width": 1440, "height": 900})
                context.route(
                    "**/*",
                    lambda route: route.continue_()
                    if route.request.url.startswith(
                        (origin + "/", vault_origin + "/", "blob:" + vault_origin + "/")
                    )
                    else route.abort(),
                )
                exercise(context, origin, vault_origin, account, login_password)
            finally:
                browser.close()


if __name__ == "__main__":
    main()
