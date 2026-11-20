#!/usr/bin/env python3
"""Run against a built moosuri binary with the standard web assets embedded.

Usage: python3 web/tests/standard-web-fullstack.py /path/to/moosuri
Requires Python Playwright and its Chromium browser.
"""

import argparse
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
from secrets import token_urlsafe
import socket
import subprocess
from tempfile import TemporaryDirectory
from threading import Thread
import time
from urllib.error import URLError
from urllib.parse import urlsplit
from urllib.request import ProxyHandler, build_opener

from playwright.sync_api import expect, sync_playwright


EMAIL = "standard-smoke@example.test"
MODEL = "standard-smoke-model"
QUESTION = "What color is the synthetic teacup?"
ANSWER = "The synthetic teacup is cobalt blue."
DRAFT = "Unsent standard web smoke draft"


class ModelFixture(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if request["model"] != MODEL or request.get("tools") or request.get("stream") is not True:
            self.send_error(400)
            return
        prompt = request["messages"][-1]["content"]
        if prompt not in ("Reply with the single word OK.", QUESTION):
            self.send_error(400)
            return
        self.server.prompts.append(prompt)
        text = ANSWER if prompt == QUESTION else "OK"
        frames = [
            {"choices": [{"index": 0, "delta": {"role": "assistant", "content": text}}]},
            {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
            {"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 7}},
        ]
        body = "".join(
            f"data: {json.dumps(dict(frame, id='smoke-response', model=MODEL))}\n\n"
            for frame in frames
        ) + "data: [DONE]\n\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body.encode())))
        self.end_headers()
        self.wfile.write(body.encode())


@contextmanager
def standard_server(binary, root, password):
    password_file = root / "operator-password"
    password_file.write_text(password + "\n", encoding="utf-8")
    password_file.chmod(0o600)
    data = root / "installation"
    env = {"PATH": os.defpath, "HOME": str(root), "TMPDIR": str(root)}
    subprocess.run(
        [binary, "init", "--data-dir", data, "--name", "Synthetic Operator",
         "--email", EMAIL, "--password-file", password_file],
        env=env, check=True, capture_output=True, timeout=20,
    )
    password_file.unlink()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        address = f"127.0.0.1:{listener.getsockname()[1]}"
    origin = f"http://{address}"
    with (root / "server.log").open("w") as log, subprocess.Popen(
        [binary, "serve", "--data-dir", data, "--bind", address, "--origin", origin, "--dev"],
        env=env, stdout=log, stderr=subprocess.STDOUT,
    ) as process:
        try:
            deadline = time.monotonic() + 15
            opener = build_opener(ProxyHandler({}))
            while time.monotonic() < deadline and process.poll() is None:
                try:
                    with opener.open(origin + "/health/ready", timeout=1):
                        break
                except URLError:
                    time.sleep(0.1)
            else:
                raise RuntimeError("The isolated standard server did not become ready")
            yield origin
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


def configure_provider(context, origin, account, model_port):
    """Use the real admin API to provision the synthetic provider."""
    headers = {"Origin": origin, "X-Moosuri-Account": account}

    def post(path, data):
        response = context.request.post(origin + "/api/v1/admin/providers" + path,
                                        headers=headers, data=data)
        if not response.ok:
            raise AssertionError(f"Provider fixture setup {path or '/'} returned {response.status}: "
                                 f"{response.text()[:160]}")
        return response.json()

    provider = post("", {
        "name": "Synthetic standard smoke provider",
        "enabled": True,
        "config": {"kind": "compatible", "base_url": f"http://127.0.0.1:{model_port}/v1",
                   "allowed_ips": ["127.0.0.1"], "router_providers": [],
                   "zero_data_retention": False},
        "credential": {"action": "clear"},
    })
    post(f"/{provider['id']}/probe", {"revision": provider["revision"], "model": MODEL})
    post("/default", {"selection": {
        "source": "shared", "provider_id": provider["id"],
        "provider_revision": provider["revision"], "model": MODEL,
    }})


def exercise(context, origin, password, model):
    blocked = set()

    def only_local(route):
        if route.request.url.startswith(origin + "/"):
            route.continue_()
        else:
            blocked.add(urlsplit(route.request.url).hostname)
            route.abort()

    context.route("**/*", only_local)
    page = context.new_page()
    page.goto(origin + "/app")
    expect(page.get_by_role("heading", name="로그인")).to_be_visible()
    page.get_by_label("Email").fill(EMAIL)
    page.get_by_label("Password").fill(password)
    with page.expect_response(lambda response: response.url == origin + "/api/v1/auth/login") as login:
        page.get_by_role("button", name="Sign in").click()
    assert login.value.ok, "The real browser sign-in failed"
    menu = page.get_by_role("navigation", name="주 메뉴")
    expect(menu).to_be_visible()
    print("PASS standard browser sign-in", flush=True)

    configure_provider(context, origin, login.value.json()["user"]["id"], model.server_port)
    page.reload()
    composer = page.get_by_role("textbox", name="메시지")
    composer.fill(QUESTION)
    page.get_by_role("button", name="메시지 전송").click()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible(timeout=15_000)
    expect(composer).to_have_value("")
    print("PASS send and stream through the synthetic loopback model", flush=True)

    page.reload()
    menu.get_by_role("button", name="대화 검색").click()
    search = page.get_by_role("dialog", name="대화 검색")
    field = search.get_by_role("searchbox", name="대화 제목")
    field.fill(QUESTION)
    field.press("Enter")
    expect(search).to_be_hidden()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()
    expect(page.get_by_role("heading", name=QUESTION, level=1)).to_be_visible()
    print("PASS reload and reopen saved conversation from search", flush=True)

    composer.fill(DRAFT)
    menu.get_by_role("button", name="소식함").click()
    page.get_by_role("button", name="예약 관리").click()
    enable_quiet = page.get_by_role("checkbox", name="조용한 시간 사용")
    quiet_timezone = page.get_by_role("textbox", name="조용한 시간 시간대")
    expect(enable_quiet).not_to_be_checked()
    enable_quiet.check()
    quiet_timezone.fill("Asia/Seoul")
    page.get_by_role("button", name="조용한 시간 저장").click()
    expect(page.get_by_role("status").filter(has_text="조용한 시간을 저장했습니다")).to_be_visible()
    page.get_by_role("button", name="채팅으로", exact=True).click()
    expect(composer).to_have_value(DRAFT)
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()
    print("PASS unsent draft survives a workspace visit and return", flush=True)

    menu.get_by_role("button", name="소식함").click()
    page.get_by_role("button", name="예약 관리").click()
    page.reload()
    expect(enable_quiet).to_be_checked()
    expect(quiet_timezone).to_have_value("Asia/Seoul")
    assert model.prompts == ["Reply with the single word OK.", QUESTION], model.prompts
    assert not blocked, f"Unexpected browser destinations: {blocked}"
    print("PASS persisted quiet-hour setting with exactly one chat inference", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--channel", help="Use an installed Chromium channel such as chrome")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    password = "Synthetic-" + token_urlsafe(24)
    with TemporaryDirectory(prefix="moosuri-standard-smoke-") as directory, ThreadingHTTPServer(
        ("127.0.0.1", 0), ModelFixture
    ) as model:
        model.prompts = []
        worker = Thread(target=model.serve_forever, daemon=True)
        worker.start()
        try:
            with standard_server(binary, Path(directory).resolve(), password) as origin, sync_playwright() as playwright:
                with playwright.chromium.launch(channel=args.channel) as browser, browser.new_context(
                    viewport={"width": 1280, "height": 900}, service_workers="block"
                ) as context:
                    exercise(context, origin, password, model)
        finally:
            model.shutdown()
            worker.join()


if __name__ == "__main__":
    main()
