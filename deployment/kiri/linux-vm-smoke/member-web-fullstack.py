#!/usr/bin/env python3
"""Run with python3 member-web-fullstack.py /path/to/moosuri (with embedded web assets).
Requires nonroot Unix, Python Playwright, openssl, and Chrome stable or a
Playwright-managed browser (--channel chromium or --channel webkit).
Use --claude for ten real answers from Claude Sonnet 5.5 at medium effort.
The default, --expect-output-limit and --history-budget need no model login.
--research-work (shopping/application outcomes from real browser reads via a private synthetic broker) needs a
binary built with `cargo build -p moosuri --features test-fixtures`; --short-corrections needs no model login.
--preference-recall checks automatic recall of a stated standing preference (no model login; add --claude for the
pinned model on the critical path, scored on its visible answer). Counts only are reported.
--live-journey GROUP uses real Claude for every interpretation and answer, with per-case PASS and a JOURNEY total.
Every invocation creates and removes a private fixture. A single OS peer UID can have one active model binding.
Live journeys switch the test broker on member login while preserving the gateway, member volumes and per-owner ledger.
"""
import argparse
import base64
import hashlib
from contextlib import ExitStack, closing, suppress
import http.client
from http.server import BaseHTTPRequestHandler, HTTPServer, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import secrets
import socket
import sqlite3
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from uuid import uuid4

from playwright.sync_api import expect, sync_playwright

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "audit"))
import claude_cli

MODEL = "synthetic-model"
PROMPT = "Fixture trip list"
ANSWER = "Pack a blue notebook."
FOLLOWUP = "Which notebook?"
SECOND_ANSWER = "The blue notebook."
DRAFT = "Unsent fixture draft"
SUBJECT = "Fixture notebook"
UPDATED = "Updated fixture notebook"
EMPTY_MEMORIES = "아직 기록한 기억이 없습니다."
LONG_PROMPTS = ["Original fixture: " + "A" * 1050, "Recent fixture: " + "B" * 750,
                "Current fixture: " + "C" * 650]
LONG_ANSWERS = ["First fixture recorded.", "Second fixture recorded.", "Recent fixture retained."]
LIMIT_PROMPT = "123 곱하기 421을 계산해 줘. 답만 적어 줘."
CLAUDE_DIALOGUES = (
    (
        ("ML-01", "23 + 19의 합만 숫자로 답해 줘.", "42"),
        ("ML-02", "예시 열차 무궁화 82호는 14:37에 출발해. 출발 시각만 답해 줘.", "14:37"),
        ("ML-03", "방금 이야기한 열차의 출발 시각을 다시 알려 줘. 시각만 답해 줘.", "14:37"),
    ),
    (
        ("ML-05", "프랑스의 수도는 어디야? 한글 지명만 적어 줘.", "파리"),
        ("ML-06", "내가 이 대화에서 예약한 좌석 번호를 알려 줘. 알려 준 적이 없으면 ‘확인 불가’라고만 답해 줘.", "확인 불가"),
        ("ML-07", "입실은 09:07이야. 입실 시각을 콜론이 들어간 HH:MM 형식으로만 써 줘.", "09:07"),
        ("ML-08", "방금 말한 입실 시각을 같은 HH:MM 형식으로만 다시 적어 줘.", "09:07"),
    ),
    (
        ("GAME-01", "가상 게임 안내에 따르면 2장 차고는 부두, 입장료는 80이고 3장 차고는 등대, 입장료는 120이야. 지금은 2장이야. 차고 위치만 답해 줘.", "부두"),
        ("GAME-02", "아니, 지금은 3장이야. 그 장의 차고 위치만 다시 알려 줘.", "등대"),
        ("GAME-03", "그럼 들어갈 때 드는 비용은? 숫자만.", "120"),
    ),
)


def provider_turns(history_budget):
    if not history_budget:
        return [([("user", PROMPT)], ANSWER),
                ([("user", PROMPT), ("assistant", ANSWER), ("user", FOLLOWUP)], SECOND_ANSWER)]
    turns = []
    for index, prompt in enumerate(LONG_PROMPTS):
        previous = [] if index == 0 else [("user", LONG_PROMPTS[index - 1]),
                                          ("assistant", LONG_ANSWERS[index - 1])]
        turns.append((previous + [("user", prompt)], LONG_ANSWERS[index]))
    return turns


class QuietHandler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass


class Provider(QuietHandler):
    def do_POST(self):
        if self.headers.get("Authorization") != "Bearer " + self.server.fixture_key:
            self.send_error(403)
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        messages = [(m["role"], m["content"]) for m in body["messages"]]
        valid = self.path == "/v1/chat/completions" and body["model"] == self.server.model and body["stream"]
        if not valid or not messages or messages[0][0] != "system":
            self.send_error(400)
            return
        canned = False
        classifier = messages[0][1].startswith("You are a strict classifier for one Korean member message.")
        if classifier:
            if len(messages) != 2 or messages[1][0] != "user":
                self.send_error(400)
                return
            try:
                context, literal = messages[1][1].split("\nmessage=", 1)
                json.loads(context.removeprefix("candidates="))
                request = json.loads(literal)
            except (TypeError, ValueError):
                self.send_error(400)
                return
            known = self.server.work_intents
            armed_chat = any(turn and turn[-1] == ("user", request) for turn, _ in self.server.turns)
            # --preference-recall only: the scenario module names the member messages it will send.
            armed_chat = armed_chat or (self.server.recall is not None and self.server.recall.knows(request))
            if request not in known and not armed_chat and self.server.live is None:
                self.send_error(400)
                return
            answer = json.dumps(known.get(request, {"intent": "chat"}), ensure_ascii=False)
            self.server.interpretations += 1
            prompt = messages[1][1]
        elif self.server.browser_reads is not None and messages[0][1].startswith("Answer the user's browser task"):
            # Research acceptance: a synthetic reading of the page the member's own browser job observed.
            # It repeats the visible page text (as a faithful summary would) and never consumes a chat turn.
            try:
                seen = json.loads(messages[1][1])["observed_page"]["visible_text"]
            except (KeyError, TypeError, ValueError):
                self.send_error(400)
                return
            answer = json.dumps({"answer": seen, "needs_user": False}, ensure_ascii=False)
            self.server.browser_reads += 1
            prompt = messages[1][1]
        elif self.server.recall is not None or self.server.live is not None:
            # Mode-scoped wire expectations live in member_preference_recall.py; answer None means "ask the model".
            answer = self.server.recall.chat(messages) if self.server.recall is not None else None
            if answer is False:
                self.send_error(400)
                return
            canned = answer is not None
            self.server.calls += 1
            prompt = json.dumps({"format": "moosuri-local-history-v1", "history_is_quoted_context": True,
                                 "history": [{"role": r, "text": t} for r, t in messages[1:-1]],
                                 "current_user_request": messages[-1][1]}, ensure_ascii=False)
        else:
            if self.server.calls >= len(self.server.turns):
                self.send_error(400)
                return
            expected, answer = self.server.turns[self.server.calls]
            if messages[1:] != expected:
                self.send_error(400)
                return
            self.server.calls += 1
            prompt = json.dumps({"format": "moosuri-local-history-v1", "history_is_quoted_context": True,
                                 "history": [{"role": r, "text": t} for r, t in messages[1:-1]],
                                 "current_user_request": messages[-1][1]}, ensure_ascii=False)
        usage = {"prompt_tokens": 9, "completion_tokens": 4 if answer else 1}
        if self.server.model == claude_cli.MODEL and not canned:
            try:
                answer, metrics = claude_cli.model(self.server.cli_cwd, prompt, 0.1, system=messages[0][1])
            except RuntimeError as error:
                self.server.claude_error = str(error)
                self.send_error(502)
                return
            usage = {"prompt_tokens": sum(metrics[k] for k in claude_cli.USAGE if k != "output_tokens"),
                     "completion_tokens": metrics["output_tokens"]}
            if self.server.live is not None:
                self.server.live.record(classifier, messages, metrics)
        frames = [
            {"choices": [{"index": 0, "delta": {"role": "assistant", "content": answer or ""},
                          "finish_reason": None}]},
            {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop" if answer else "length"}]},
            {"choices": [], "usage": usage},
        ]
        data = "".join(f"data: {json.dumps(dict(id='fixture', model=self.server.model, **frame))}\n\n"
                       for frame in frames) + "data: [DONE]\n\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.write(data.encode())


class UnixHTTP(http.client.HTTPConnection):
    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.host)


class Proxy(QuietHandler):
    def relay(self):
        if self.server.history_unavailable and self.command == "GET" and self.path.split("?")[0].endswith("/messages"):
            self.send_error(503)
            return
        allowed = ("content-type", "cookie", "origin", "sec-fetch-site", "accept", "x-moosuri-account")
        headers = {key: self.headers[key] for key in allowed if key in self.headers}
        headers.update({"Host": self.server.authority, "X-Real-IP": self.client_address[0]})
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        with closing(UnixHTTP(str(self.server.gateway), timeout=100)) as gateway:
            gateway.request(self.command, self.path, body=body, headers=headers)
            response = gateway.getresponse()
            payload = response.read()
            if self.server.live and self.command == "POST" and self.path.endswith("/work-interpret"):
                self.server.live.interpretation(body, response.status, payload)
            if self.command == "POST" and self.path == self.server.drop_ack_path and 200 <= response.status < 300:
                # The gateway has committed. Close this test connection before sending a single response byte.
                self.close_connection = True
                return
            self.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() not in ("connection", "transfer-encoding", "content-length", "date", "server"):
                    self.send_header(key, value)
            self.send_header("Content-Length", str(len(payload)))
            # A navigation may close the browser socket before the first byte is flushed.
            with suppress(BrokenPipeError, ConnectionResetError, ssl.SSLEOFError):
                self.end_headers()
                self.wfile.write(payload)

    do_GET = do_POST = do_PUT = do_DELETE = relay


def serve(stack, handler, tls=None, server_type=ThreadingHTTPServer):
    server = server_type(("127.0.0.1", 0), handler)
    stack.callback(server.server_close)
    if tls:
        server.socket = tls.wrap_socket(server.socket, server_side=True, do_handshake_on_connect=False)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    stack.callback(server.shutdown)
    return server


def directory(parent, name):
    path = parent / name
    path.mkdir(mode=0o700)
    return path


def start(stack, command, ready, env):
    process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, env=env)

    def stop():
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

    stack.callback(stop)
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"{command[1]} exited before listening ({process.returncode})")
        if ready.is_socket():
            return
        time.sleep(0.04)
    raise RuntimeError(f"{command[1]} did not start listening")


def provision(stack, binary, root, provider, output_tokens):
    env = {"PATH": os.defpath, "HOME": str(directory(root, "home")), "TMPDIR": str(root),
           "NO_PROXY": "127.0.0.1,localhost"}
    registry, ipc, gateway, model_root, ledger = (directory(root, x) for x in ("r", "i", "g", "m", "l"))
    # Research acceptance only: private same-UID broker sockets (a --features test-fixtures binary is required).
    browser_root = directory(root, "b") if provider.research or provider.public_watch_read else None
    provider.browser_socket = browser_root / "first.sock" if browser_root else None
    registry_cmd = [binary, "account-runtime", "--registry", registry]
    members = []
    brokers = []
    broker_stack = stack.enter_context(ExitStack())
    connection_revision = str(uuid4())

    def select_member(index):
        broker_stack.close()
        command, ready = brokers[index]
        start(broker_stack, command, ready, env)

    if provider.live is not None:
        provider.live.select_member = select_member
    for index, label in enumerate(("first", "second")):
        email = f"{label}@example.invalid"
        result = subprocess.run([*registry_cmd, "reserve", "--request-id", str(uuid4()), "--email", email],
                                env=env, capture_output=True, check=True)
        owner = json.loads(result.stdout)
        volume = directory(root, label)
        member = [binary, "personal-volume", "--root", volume, "--account", owner["account_id"],
                  "--tenant", owner["tenant_id"], "--volume", owner["volume_id"],
                  "--generation", str(owner["generation"])]
        temporary, password = (secrets.token_urlsafe(24) for _ in range(2))
        subprocess.run([*member, "initialize", "--name", label, "--email", email, "--password-stdin"],
                       input=temporary + "\n", text=True, env=env, capture_output=True, check=True)
        sock = directory(ipc, owner["account_id"]) / "personal.sock"
        command = [*member, "serve", "--socket", sock]
        if browser_root:  # the second member gets a path nobody listens on: its browser stays unavailable
            command.extend(["--browser-test-socket", browser_root / f"{label}.sock"])
        if index == 0 or provider.live is not None:
            # A separate fixture broker and ledger per member keeps each same-UID
            # test process bound to exactly one owner, as in the installed runtime.
            config_root = directory(root, "engine-" + label) if provider.live is not None else root
            sockets = directory(model_root, label) if provider.live is not None else model_root
            member_ledger = directory(ledger, label) if provider.live is not None else ledger
            binding = {k: owner[k] for k in ("account_id", "tenant_id", "volume_id", "generation")}
            call_limit = 80 if provider.live is not None else 40 if provider.recall is not None else 32 if provider.semantic or provider.research else 2 * (sum(map(len, CLAUDE_DIALOGUES)) if provider.model == claude_cli.MODEL else 5)
            reserve = 8192 if provider.recall is not None or provider.semantic or provider.research or provider.model == claude_cli.MODEL else 4096
            binding.update(peer_uid=os.geteuid(),
                           budget={"max_calls": call_limit, "max_tokens": reserve * call_limit, "reservation_tokens": reserve})
            fixture_key = config_root / "provider.key"
            fixture_key.write_text(provider.fixture_key)
            config = config_root / "model.json"
            config.write_text(json.dumps({
                "version": 1, "connection_revision": connection_revision, "model": provider.model,
                "provider": {"kind": "compatible", "base_url": f"http://127.0.0.1:{provider.server_port}/v1",
                             "allowed_ips": ["127.0.0.1"], "router_providers": [], "zero_data_retention": False},
                "bindings": [binding], "key_file": str(fixture_key),
            }))
            fingerprint = config_root / "fingerprint"
            fingerprint.write_bytes(secrets.token_bytes(32))
            model_socket = sockets / owner["account_id"] / "model.sock"
            brokers.append(([binary, "runtime-model", "--registry", registry, "--ledger", member_ledger,
                             "--config", config, "--fingerprint-key", fingerprint, "--socket-dir", sockets],
                            model_socket))
            if index == 0:
                select_member(0)
            preview = config_root / "preview.json"
            preview.write_text(json.dumps({"socket": str(model_socket), "broker_uid": os.geteuid(),
                                           "max_output_tokens": output_tokens}))
            command.extend(["--text-preview-config", preview])
        start(stack, command, sock, env)
        subprocess.run([*registry_cmd, "activate", "--email", email, "--ipc-root", ipc],
                       env=env, capture_output=True, check=True)
        members.append((email, temporary, password, next(volume.glob("*/assistant.sqlite"))))
    return registry, ipc, gateway, members, env


def button(page, name):
    return page.get_by_role("button", name=name, exact=True)


def login(page, member, first):
    email, temporary, password, _ = member
    page.get_by_label("이메일", exact=True).fill(email)
    page.get_by_label("비밀번호", exact=True).fill(temporary if first else password)
    button(page, "로그인").click()
    if first:
        expect(page.get_by_role("heading", name="임시 비밀번호 변경")).to_be_visible()
        page.get_by_label("새 비밀번호", exact=True).fill(password)
        button(page, "비밀번호 변경").click()
    expect(button(page, "대화 목록")).to_be_visible()


def logout(page):
    button(page, "대화 목록").click()
    button(page, "로그아웃").click()
    expect(page.get_by_label("이메일", exact=True)).to_be_visible()


def rows(member, sql, params=()):
    with closing(sqlite3.connect(member[3].as_uri() + "?mode=ro", uri=True)) as db:
        return db.execute(sql, params).fetchall()


def context_counts(member):
    return rows(member, "SELECT p.context_messages FROM personal_text_previews p "
                        "JOIN chat_runs r ON r.tenant_id=p.tenant_id AND r.id=p.run_id ORDER BY r.sequence")


def passed(case):
    print(f"PASS {case}", flush=True)


def exercise(page, members, origin, provider):
    first, second = members
    page.goto(origin)
    login(page, first, True)
    assert rows(first, "SELECT must_change_password FROM accounts") == [(0,)]
    passed("MWF-01 first login and required password rotation")
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    expect(editor).to_be_enabled()
    editor.fill(PROMPT)
    button(page, "보내기").click()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible(timeout=20000)
    editor.fill(FOLLOWUP)
    editor.press("Enter")
    expect(page.get_by_text(SECOND_ANSWER, exact=True)).to_be_visible(timeout=20000)
    sql = ("SELECT r.user_text,o.text,r.state FROM chat_runs r JOIN chat_output o "
           "ON o.run_id=r.id AND o.tenant_id=r.tenant_id ORDER BY r.sequence")
    turns = [(PROMPT, ANSWER, "completed"), (FOLLOWUP, SECOND_ANSWER, "completed")]
    assert rows(first, sql) == turns and provider.calls == 2
    assert context_counts(first) == [(0,), (2,)]
    button(page, "업무 기록 보기").last.click()
    page.get_by_role("button", name=re.compile("대화 문맥 준비")).click()
    expect(page.get_by_role("region", name="단계 상세")).to_contain_text("이전 메시지 2개")
    page.get_by_role("button", name=re.compile("원래 대화 열기")).click()
    expect(editor).to_be_visible()
    passed("MWF-02 browser chat, real broker, prior turns and two committed SQLite replies")
    editor.fill(DRAFT)
    button(page, "새 대화").click()
    expect(editor).to_have_value("")
    button(page, "대화 목록").click()
    page.get_by_role("dialog").get_by_role("searchbox").fill(PROMPT)
    button(page.get_by_role("dialog"), PROMPT).click()
    expect(editor).to_have_value(DRAFT)
    page.reload()
    expect(editor).to_have_value(DRAFT)
    expect(page.get_by_text(SECOND_ANSWER, exact=True)).to_be_visible()
    assert provider.calls == 2 and rows(first, sql) == turns
    passed("MWF-03 search and reopen a conversation, retain unsent draft and reload history")
    button(page, "기억").click()
    button(page, "기억 추가").click()
    page.get_by_label("기억 주제", exact=True).fill(SUBJECT)
    page.get_by_label("기억 내용", exact=True).fill("Use a blue cover")
    button(page, "기억 저장").click()
    expect(page.get_by_role("heading", name=SUBJECT)).to_be_visible()
    button(page, "기억 수정").click()
    page.get_by_label("기억 주제", exact=True).fill(UPDATED)
    button(page, "기억 저장").click()
    expect(page.get_by_role("heading", name=UPDATED)).to_be_visible()
    page.reload()
    page.get_by_role("button").filter(has_text=UPDATED).click()
    expect(page.get_by_role("heading", name=UPDATED)).to_be_visible()
    button(page, "닫기").click()
    assert rows(first, "SELECT revision,status FROM knowledge_memories") == [(2, "active")]
    passed("MWF-04 create and edit a memory, reopen its stored revision")
    logout(page)
    login(page, second, True)
    button(page, "대화 목록").click()
    expect(page.get_by_text("아직 대화가 없습니다.", exact=True)).to_be_visible()
    button(page, "닫기").click()
    button(page, "대화").click()
    expect(editor).to_have_value("")
    button(page, "기억").click()
    expect(page.get_by_text(EMPTY_MEMORIES, exact=True)).to_be_visible()
    assert rows(second, "SELECT id FROM threads") == [] and rows(second, "SELECT id FROM knowledge_memories") == []
    assert rows(first, sql) == turns
    passed("MWF-05 second member in the same browser has empty history, drafts and memories")
    logout(page)
    login(page, first, False)
    button(page, "기억").click()
    page.get_by_role("button").filter(has_text=UPDATED).click()
    button(page, "기억 잊기").click()
    button(page, "기억과 이력 지우기").click()
    expect(page.get_by_text(EMPTY_MEMORIES, exact=True)).to_be_visible()
    page.reload()
    expect(page.get_by_text(EMPTY_MEMORIES, exact=True)).to_be_visible()
    assert rows(first, "SELECT revision FROM knowledge_memory_versions") == []
    passed("MWF-06 sign back in with the changed password and forget memory including revisions")


def exercise_history_budget(page, member, origin, provider):
    page.goto(origin)
    login(page, member, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    for index, prompt in enumerate(LONG_PROMPTS):
        editor.fill(prompt)
        editor.press("Enter")
        expect(page.get_by_text(LONG_ANSWERS[index], exact=True)).to_be_visible(timeout=20000)
        if index == 1:
            passed("MB-01 both the preceding exchange and the current question fit the member reservation")
    assert provider.calls == 3
    sql = "SELECT user_text,state FROM chat_runs ORDER BY sequence"
    expected = [(prompt, "completed") for prompt in LONG_PROMPTS]
    assert rows(member, sql) == expected
    assert context_counts(member) == [(0,), (2,), (2,)]
    passed("MB-02 only the oldest exchange is omitted from inference and all three turns stay in history")
    too_long = "Largest fixture: " + "D" * 3800
    editor.fill(too_long)
    editor.press("Enter")
    expect(page.get_by_text("한 번에 보낼 수 있는 길이를 넘었습니다. 입력을 줄여 다시 보내세요.", exact=True)).to_be_visible()
    expect(editor).to_have_value(too_long)
    page.reload()
    expect(editor).to_have_value(too_long)
    assert provider.calls == 3 and rows(member, sql) == expected
    passed("MB-03 an oversized current question stays a draft with zero additional provider calls or saved turns")


def saved_reply(page, member, prompt, provider):
    deadline = time.monotonic() + 75
    with closing(sqlite3.connect(member[3].as_uri() + "?mode=ro", uri=True)) as db:
        while time.monotonic() < deadline:
            if provider.claude_error:
                raise RuntimeError(provider.claude_error)
            reply = db.execute(
                "SELECT r.id, r.state, o.text FROM chat_runs r LEFT JOIN chat_output o "
                "ON o.tenant_id=r.tenant_id AND o.run_id=r.id AND o.item='personal-text-preview-answer' "
                "WHERE r.user_text=? ORDER BY o.ordinal DESC LIMIT 1", (prompt,)
            ).fetchone()
            if reply and reply[1] not in ("running", "queued"):
                return reply
            page.wait_for_timeout(120)
    raise RuntimeError(f"Reply deadline exceeded (stored state {reply[1] if reply else 'absent'}, "
                       f"chat calls {provider.calls}, classification calls {provider.interpretations}).")


def exercise_output_limit(page, member, origin, provider):
    page.goto(origin)
    login(page, member, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    editor.fill(LIMIT_PROMPT)
    editor.press("Enter")
    run_id, state, text = saved_reply(page, member, LIMIT_PROMPT, provider)
    assert state == "incomplete" and not text
    notice = page.locator(f"#member-turn-{run_id}").get_by_text("답변 미완료 · 표시할 본문 없음", exact=True)
    expect(notice).to_be_visible()
    page.reload()
    expect(notice).to_be_visible()
    assert provider.calls == 1 and rows(member, "SELECT COUNT(*) FROM chat_runs") == [(1,)]
    passed("ML-04 empty output limit remains visible after reload, one synthetic response and one run")


def exercise_claude(page, member, origin, provider):
    page.goto(origin)
    login(page, member, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    failures = []
    routes = []
    page.on("response", lambda r: routes.append((r.url.split("?")[0].rsplit("/", 1)[-1], r.status))
            if r.request.method == "POST" else None)
    for index, dialogue in enumerate(CLAUDE_DIALOGUES):
        if index:
            button(page, "새 대화").click()
        history = []
        for case, prompt, expected in dialogue:
            history.append(("user", prompt))
            provider.turns.append((history.copy(), None))
            editor.fill(prompt)
            expect(button(page, "보내기")).to_be_enabled()
            editor.press("Enter")
            try:
                run_id, state, answer = saved_reply(page, member, prompt, provider)
            except RuntimeError as exc:
                raise RuntimeError(f"{case}: {exc} Recent POST route statuses: {routes[-4:]}") from None
            if state != "completed" or not answer:
                raise RuntimeError(f"{case} ended with member state {state}.")
            visible = page.locator(f"#member-turn-{run_id} .member-reply p")
            expect(visible).to_be_visible()
            if visible.inner_text() != answer or answer.strip() != expected:
                failures.append(case)
            else:
                passed(f"{case} {claude_cli.MODEL} / {claude_cli.EFFORT}, browser and SQLite")
            history.append(("assistant", answer))
    assert provider.calls == sum(map(len, CLAUDE_DIALOGUES))
    if failures:
        raise AssertionError("Answer or display mismatch in " + ", ".join(failures))


def main():
    import member_live_journeys
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--channel", choices=("chrome", "chromium", "webkit"), default="chrome",
                        help="installed Chrome stable or a Playwright browser")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--live-journey", choices=member_live_journeys.SUITES,
                      help="run an end-to-end member journey with real Claude for every classifier and chat call")
    parser.add_argument("--supplemental-work", action="store_true", help="exercise preference changes, follow-ups and source withdrawal in chat")
    parser.add_argument("--semantic-work", action="store_true", help="submit everyday work and reopen its source-backed results")
    parser.add_argument("--preference-recall", action="store_true",
                        help="automatic recall of a stated standing preference through the ordinary composer "
                             "(synthetic classifier; with --claude the pinned model decides and answers the critical path)")
    mode.add_argument("--conversation-work", action="store_true",
                      help="exercise multi-task chat, its timer, evidence and member boundaries")
    mode.add_argument("--workflows", action="store_true", help="exercise task evidence and the installation schedule timer")
    mode.add_argument("--public-watch-read", action="store_true", help="compare four fresh public-page observations with actual alerts")
    mode.add_argument("--public-watch", action="store_true", help="verify recurring public-page checks and their worklog in chat")
    mode.add_argument("--brief-work", action="store_true", help="verify connected brief requests through the normal member chat")
    mode.add_argument("--research-journey", action="store_true", help="verify shopping and application results through the member interface")
    mode.add_argument("--research-work", action="store_true",
                      help="shopping/application outcomes from real browser reads through a private synthetic "
                           "broker (needs a binary built with --features test-fixtures)")
    mode.add_argument("--short-corrections", action="store_true",
                      help="short corrections in ordinary conversation keep their earlier turns")
    mode.add_argument("--claude", action="store_true", help="conversation and short-correction scenarios using the authenticated Claude CLI")
    mode.add_argument("--expect-output-limit", action="store_true",
                      help="check a synthetic output-limit response with no visible text")
    mode.add_argument("--history-budget", action="store_true",
                      help="exercise the real broker reservation with a synthetic deterministic provider")
    args = parser.parse_args()
    if args.live_journey:
        if args.supplemental_work or args.semantic_work or args.preference_recall:
            parser.error("--live-journey runs one independent scenario group")
        args.claude = True
    if args.preference_recall and (args.supplemental_work or args.semantic_work or args.conversation_work
                                   or args.workflows or args.public_watch_read or args.public_watch
                                   or args.brief_work or args.research_journey or args.research_work
                                   or args.short_corrections or args.expect_output_limit or args.history_budget):
        parser.error("--preference-recall combines only with --claude")
    if os.geteuid() == 0:
        parser.error("run as an ordinary user")
    os.umask(0o077)
    with ExitStack() as stack:
        # Short socket paths also run on macOS's 104-byte sockaddr_un.
        root = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix="mweb-", dir="/tmp"))).resolve()
        cli_cwd = None
        if args.claude:
            cli_cwd = directory(root, "claude")
            claude_cli.require_login(cli_cwd)
        key, cert = root / "tls.key", root / "tls.pem"
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                        "-subj", "/CN=127.0.0.1", "-keyout", key, "-out", cert], capture_output=True, check=True)
        public_der = subprocess.run(["openssl", "pkey", "-in", key, "-pubout", "-outform", "DER"],
                                    check=True, capture_output=True).stdout
        spki = base64.b64encode(hashlib.sha256(public_der).digest()).decode("ascii")
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(cert, key)
        provider = serve(stack, Provider, server_type=HTTPServer)
        provider.model = claude_cli.MODEL if args.claude else MODEL
        provider.fixture_key = secrets.token_urlsafe(32)
        provider.cli_cwd = cli_cwd
        provider.claude_error = None
        provider.calls = 0
        provider.interpretations = 0
        provider.work_intents = {}
        provider.semantic = args.semantic_work or args.supplemental_work
        provider.public_watch_read = args.public_watch_read
        provider.research = args.research_work or args.research_journey
        provider.browser_reads = 0 if provider.research else None
        provider.turns = [] if args.claude or args.preference_recall else provider_turns(args.history_budget)
        provider.recall = None
        provider.live = None
        if args.live_journey:
            provider.live = member_live_journeys.Receipts()
        if args.preference_recall:
            import member_preference_recall
            provider.recall = member_preference_recall.Recall(real=args.claude)
        output_tokens = 512 if args.claude or args.preference_recall or provider.semantic or provider.research else 64
        if args.expect_output_limit:
            provider.turns = [([("user", LIMIT_PROMPT)], None)]
            output_tokens = 1
        binary = args.binary.resolve(strict=True)
        registry, ipc, gateway, members, env = provision(stack, binary, root, provider, output_tokens)
        proxy = serve(stack, Proxy, tls)
        proxy.history_unavailable = False
        proxy.live = provider.live
        proxy.drop_ack_path = None
        proxy.authority = f"127.0.0.1:{proxy.server_port}"
        proxy.gateway = gateway / "gateway.sock"
        origin = "https://" + proxy.authority
        start(stack, [binary, "personal-gateway", "--registry", registry, "--ipc-root", ipc,
                      "--socket-dir", gateway, "--public-origin", origin], proxy.gateway, env)
        with sync_playwright() as playwright:
            if args.channel == "webkit":
                browser = playwright.webkit.launch(headless=True, proxy={
                    "server": "http://127.0.0.1:0", "bypass": "127.0.0.1",
                })
            else:
                browser = playwright.chromium.launch(channel=args.channel, headless=True, args=[
                    f"--ignore-certificate-errors-spki-list={spki}", "--disable-background-networking", "--disable-quic", "--disable-component-update",
                    "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1", "--proxy-server=127.0.0.1:0",
                ])
            try:
                context = browser.new_context(service_workers="block", ignore_https_errors=args.channel == "webkit",
                                              viewport={"width": 1200, "height": 840})
                # Browser traffic for the application flows unmodified to the gateway.
                context.route(lambda url: not url.startswith(origin + "/"), lambda route: route.abort())
                page = context.new_page()
                if args.live_journey:
                    member_live_journeys.exercise(args.live_journey, page, members, origin, provider, proxy,
                                                  login=login, logout=logout, rows=rows)
                elif args.preference_recall:
                    member_preference_recall.exercise(page, members, origin, provider, login=login, logout=logout,
                                                      rows=rows, passed=passed)
                elif args.public_watch_read:
                    import member_public_watch_journey
                    from member_research_outcomes import Broker
                    broker = Broker(provider.browser_socket, {})
                    stack.callback(broker.close)
                    member_public_watch_journey.exercise_read(page, members, origin, provider, broker, login=login, rows=rows, passed=passed)
                elif args.public_watch:
                    import member_public_watch_journey
                    member_public_watch_journey.exercise(page, members, origin, provider, login=login, rows=rows, passed=passed)
                elif args.brief_work:
                    import member_brief_journey
                    member_brief_journey.exercise(page, members, origin, provider, login=login, rows=rows, passed=passed)
                elif args.supplemental_work:
                    import member_supplemental_scenarios
                    member_supplemental_scenarios.exercise(page, members, origin, provider, login=login, rows=rows, passed=passed)
                elif args.semantic_work:
                    import member_semantic_scenarios
                    member_semantic_scenarios.exercise(page, members, origin, provider, login=login, rows=rows, passed=passed)
                elif args.conversation_work:
                    import member_conversation_scenarios
                    member_conversation_scenarios.exercise(page, members, origin, provider, proxy, login=login,
                                                          logout=logout, rows=rows, passed=passed)
                elif provider.research:
                    import member_research_outcomes
                    broker = member_research_outcomes.Broker(provider.browser_socket, {})
                    stack.callback(broker.close)
                    if args.research_journey:
                        import member_research_journey
                        member_research_journey.exercise(page, members, origin, provider, broker, login=login, rows=rows, passed=passed)
                    else:
                        member_research_outcomes.exercise(page, members, origin, provider, broker, login=login,
                                                          logout=logout, rows=rows, passed=passed)
                elif args.short_corrections:
                    import member_research_outcomes
                    member_research_outcomes.exercise_corrections(page, members, origin, provider, login=login,
                                                                  rows=rows, passed=passed)
                elif args.workflows:
                    import member_workflow_scenarios
                    member_workflow_scenarios.exercise(page, members, origin, provider, login=login,
                                                       logout=logout, rows=rows, passed=passed,
                                                       prompt=PROMPT, answer=ANSWER)
                elif args.history_budget:
                    exercise_history_budget(page, members[0], origin, provider)
                elif args.expect_output_limit:
                    exercise_output_limit(page, members[0], origin, provider)
                elif args.claude:
                    exercise_claude(page, members[0], origin, provider)
                else:
                    exercise(page, members, origin, provider)
            finally:
                browser.close()


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        raise SystemExit(str(error)) from None
