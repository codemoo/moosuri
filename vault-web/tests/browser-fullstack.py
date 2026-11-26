"""Run the real assistant handoff and vault WASM UI against two private local processes."""

import argparse
from contextlib import ExitStack
import os
import re
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


def check_layout(page, screen, screenshots):
    """Check the real rendered page. Mask secret controls in optional local evidence."""
    initial = page.viewport_size
    for width, height in [(1440, 900), (390, 844), (320, 700)]:
        page.set_viewport_size({"width": width, "height": height})
        measurements = page.evaluate("""async () => {
          await document.fonts.ready;
          const main = document.querySelector("main");
          return {
            overflow: document.documentElement.scrollWidth > innerWidth,
            font: document.fonts.check('16px "Pretendard Variable"', '암호'),
            canvas: getComputedStyle(document.documentElement).backgroundColor,
            width: main.getBoundingClientRect().width
          };
        }""")
        assert not measurements["overflow"], (screen, width)
        assert measurements["font"] and measurements["canvas"] == "rgb(24, 24, 25)", (screen, width)
        if width == 1440:
            gate = screen in ("setup", "locked", "recovery")
            minimum, maximum = (380, 440) if gate else (700, 800)
            assert minimum <= measurements["width"] <= maximum, (screen, measurements["width"])
        if screenshots and width != 320:
            screenshots.mkdir(parents=True, exist_ok=True)
            page.screenshot(path=screenshots/f"{screen}-{width}.png", full_page=True, animations="disabled",
                            mask_color="#44474a",
                            mask=[page.locator('.recovery textarea, .recovery input, .otp strong, .revealed pre')])
    page.set_viewport_size(initial)


def exercise(context, origin, vault_origin, account, login_password, screenshots):
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

    check_layout(vault, "setup", screenshots)
    field = vault.get_by_label("볼트 비밀번호", exact=True)
    make = vault.get_by_role("button", name="만들기", exact=True)
    for short in ["1234567", "가나다라마바사", "🔑" * 7]:
        field.fill(short)
        expect(make).to_be_disabled()
        expect(field).to_have_attribute("aria-invalid", "true")
    field.fill("🔑" * 257)
    expect(make).to_be_disabled()
    expect(vault.get_by_text("UTF-8 기준", exact=False)).to_be_visible()
    password = "연습볼트비번🔑8"
    assert len(password) == 8
    field.fill(password)
    vault.get_by_role("button", name="만들기", exact=True).click()
    recovery = vault.get_by_label("새 복구 키").input_value()
    assert recovery and recovery != password
    check_layout(vault, "recovery", screenshots)
    verify_field = vault.get_by_label("기록한 복구 키")
    verify_field.fill("MR2-" + ("A" * 43 if recovery != "MR2-" + "A" * 43 else "B" * 42 + "A"))
    vault.get_by_role("button", name="저장하고 열기").click()
    expect(vault.get_by_text("비밀번호 또는 복구 키를 확인해 주세요.", exact=True)).to_be_visible()
    vault.get_by_label("기록한 복구 키").fill(recovery)
    vault.get_by_role("button", name="저장하고 열기").click()
    create = vault.get_by_role("button", name="새 항목", exact=True)
    create.wait_for()
    print("PASS VWEB-02 create a vault and confirm the recovery key in the WASM client", flush=True)

    check_layout(vault, "empty", screenshots)
    create.click()
    check_layout(vault, "editor", screenshots)
    title = "Synthetic vault login"
    secret = secrets.token_urlsafe(24)
    vault.get_by_label("이름", exact=True).fill(title)
    vault.get_by_label("사용자 이름", exact=True).fill("fixture-member")
    vault.get_by_label("주소", exact=True).fill("https://example.test/login")
    vault.get_by_label("비밀번호 또는 키").fill(secret)
    vault.get_by_role("button", name="저장", exact=True).click()
    vault.get_by_role("button", name=title, exact=True).click()
    expect(vault.get_by_text("fixture-member", exact=True)).to_be_visible()
    check_layout(vault, "detail", screenshots)
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
    search = vault.get_by_role("searchbox")
    search.fill("absent fixture")
    expect(vault.get_by_role("button", name=renamed)).to_have_count(0)
    search.fill("Renamed")
    expect(vault.get_by_role("button", name=renamed)).to_be_visible()
    print("PASS VWEB-04 edit and search the saved login in the live client", flush=True)

    vault.get_by_role("button", name="잠그기", exact=True).click()
    check_layout(vault, "locked", screenshots)
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

    # A private fixture exercises both the worker selector and real encrypted storage.
    def choose_subset(check_screen=False):
        vault.get_by_role("button", name="볼트 설정").click()
        vault.get_by_role("button", name="CSV 가져오기").click()
        if check_screen:
            check_layout(vault, "import", screenshots)
        vault.get_by_label("가져올 항목").select_option("any")
        vault.get_by_role("textbox", name="포함할 글자", exact=False).fill("fixture-one, fixture-two")
        vault.locator('input[type=file]').set_input_files({
            "name": "synthetic.csv", "mimeType": "text/csv", "buffer": (
                "Title,URL,Username,Password,Notes,OTPAuth\n"
                "Fixture A,https://a.example.test,FIXTURE-ONE-user,synthetic a,,otpauth://totp/?secret=JBSWY3DPEHPK3PXP\n"
                "Fixture B,https://b.example.test,user-fixture-two,synthetic b,,\n"
                "Excluded,https://x.example.test,other,synthetic x,,invalid\n"
            ).encode(),
        })
    choose_subset(check_screen=True)
    save = vault.get_by_role("button", name="선택한 2개 항목 저장")
    expect(save).to_be_visible()
    expect(vault.get_by_label("가져올 항목")).to_be_disabled()
    save.click()
    expect(vault.get_by_role("heading", name="볼트 보호 설정")).to_be_visible(timeout=30000)
    vault.get_by_role("button", name="뒤로", exact=True).click()
    expect(vault.get_by_role("button", name="Fixture A", exact=False)).to_be_visible(timeout=30000)
    expect(vault.get_by_role("button", name="Fixture B", exact=False)).to_be_visible()
    expect(vault.get_by_role("button", name="Excluded", exact=False)).to_have_count(0)
    vault.reload()
    vault.get_by_label("볼트 비밀번호", exact=True).fill(password)
    vault.get_by_role("button", name="열기", exact=True).click()
    choose_subset()
    expect(vault.get_by_role("button", name="선택한 0개 항목 저장")).to_be_disabled()
    print("PASS VWEB-07 filtered import, excluded row, reload and duplicate preview", flush=True)

    vault.get_by_role("button", name="가져오기 닫기").click()
    vault.get_by_role("button", name="뒤로", exact=True).click()
    check_layout(vault, "list", screenshots)
    vault.get_by_role("button", name="Fixture A", exact=True).click()
    expect(vault.locator(".otp strong")).to_have_text(re.compile(r"^\d{6}$"))
    vault.get_by_role("button", name="뒤로", exact=True).click()
    settings = vault.get_by_role("button", name="볼트 설정")
    settings.click()
    check_layout(vault, "settings", screenshots)
    replacement = vault.get_by_label("새 볼트 비밀번호")
    change = vault.get_by_role("button", name="비밀번호 변경", exact=True)
    rotate = vault.get_by_role("button", name="모든 항목의 보호 키 교체", exact=True)
    for short in ["1234567", "🔑" * 7, "🔑" * 257]:
        replacement.fill(short)
        expect(change).to_be_disabled()
        expect(rotate).to_be_disabled()
    changed = secrets.token_hex(4)
    replacement.fill(changed)
    change.click()
    expect(vault.get_by_text("저장했습니다.", exact=True)).to_be_visible(timeout=30000)

    def unlock(value, *, use_key=False, succeeds=True):
        vault.get_by_label("복구 키로 열기").set_checked(use_key)
        vault.get_by_label("복구 키" if use_key else "볼트 비밀번호", exact=True).fill(value)
        vault.get_by_role("button", name="열기", exact=True).click()
        if succeeds:
            expect(vault.get_by_role("button", name="Fixture A", exact=True)).to_be_visible(timeout=30000)
        else:
            expect(vault.get_by_text("비밀번호 또는 복구 키를 확인해 주세요.", exact=True)).to_be_visible()
            expect(settings).to_have_count(0)

    def lock():
        vault.get_by_role("button", name="잠그기", exact=True).click()
        expect(vault.get_by_role("heading", name="볼트 열기", exact=True)).to_be_visible()

    lock()
    unlock(password, succeeds=False)
    unlock(changed)
    lock()
    unlock(recovery, use_key=True)
    print("PASS VWEB-08 change to eight ASCII characters, reject old password and keep recovery and TOTP", flush=True)

    settings.click()
    renew = vault.get_by_role("button", name="복구 키 새로 만들기")
    renew.click()
    vault.get_by_label("새 복구 키").wait_for()
    vault.get_by_role("button", name="취소", exact=True).click()
    lock()
    unlock(recovery, use_key=True)
    settings.click()
    renew.click()
    renewed = vault.get_by_label("새 복구 키").input_value()
    assert renewed != recovery
    verify_field.fill(renewed)
    vault.get_by_role("button", name="저장하고 열기").click()
    expect(renew).to_be_visible()
    lock()
    unlock(recovery, use_key=True, succeeds=False)
    unlock(renewed, use_key=True)
    print("PASS VWEB-09 cancel and confirm a recovery-key replacement, reject superseded key", flush=True)

    settings.click()
    with vault.expect_download() as download:
        vault.get_by_role("button", name="암호화 파일 내보내기").click()
    archive = download.value.path()
    rotated = "🔐" * 8
    replacement.fill(rotated)
    rotate.click()
    rotation_key = vault.get_by_label("새 복구 키").input_value()
    assert rotation_key != renewed
    verify_field.fill(rotation_key)
    vault.get_by_role("button", name="저장하고 열기").click()
    expect(renew).to_be_visible()
    vault.reload()
    unlock(changed, succeeds=False)
    unlock(renewed, use_key=True, succeeds=False)
    unlock(rotated)
    lock()
    unlock(rotation_key, use_key=True)
    print("PASS VWEB-10 rotate all keys with eight supplementary characters, reject old keys, reopen items", flush=True)

    settings.click()
    vault.get_by_role("button", name="암호화 파일 가져오기").click()
    vault.get_by_label("암호화 볼트 파일").set_input_files(archive)
    vault.get_by_label("파일의 볼트 비밀번호").fill(changed)
    vault.get_by_role("button", name="파일 전체 확인").click()
    expect(vault.get_by_text("이미 있는 중복 2개", exact=False)).to_be_visible(timeout=30000)
    expect(vault.get_by_role("button", name="선택한 0개 항목 저장")).to_be_disabled()
    print("PASS VWEB-11 reopen an archive with its pre-rotation password, both entries remain duplicates", flush=True)

    vault.get_by_role("button", name="가져오기 닫기").click()
    context.route(vault_origin + "/session/logout", lambda route: route.fulfill(status=503), times=1)
    vault.get_by_role("button", name="연결 끊기").click()
    expect(vault.get_by_text("볼트를 잠갔습니다. 연결 끊기를 완료하지 못했습니다. 다시 시도해 주세요.", exact=True)).to_be_visible()
    expect(vault.get_by_role("heading", name="볼트 열기", exact=True)).to_be_visible()
    expect(vault.get_by_role("button", name="Fixture A")).to_have_count(0)
    vault.get_by_role("button", name="연결 끊기").click()
    expect(vault.get_by_role("heading", name="무수리에서 볼트를 열어 주세요")).to_be_visible()
    expect(vault.get_by_role("link", name="무수리로 돌아가기")).to_be_visible()
    vault.reload()
    expect(vault.get_by_role("heading", name="무수리에서 볼트를 열어 주세요")).to_be_visible()
    expect(vault.get_by_role("link", name="무수리로 돌아가기")).to_have_attribute("href", origin + "/app")
    expect(vault.get_by_label("볼트 비밀번호", exact=True)).to_have_count(0)
    print("PASS VWEB-12 failed disconnect locks locally, retry revokes the session across reload", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assistant", type=Path)
    parser.add_argument("vault", type=Path)
    parser.add_argument("--screenshots", type=Path, help="Save masked synthetic screens at desktop and mobile sizes")
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
                exercise(context, origin, vault_origin, account, login_password, args.screenshots)
            finally:
                browser.close()


if __name__ == "__main__":
    main()
