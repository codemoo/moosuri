#!/usr/bin/env python3
"""Exercise the built Kiri web client in Chrome and WebKit with synthetic APIs."""

import argparse
import json
import re
import time
import traceback
from copy import deepcopy
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
from urllib.parse import parse_qs, urlencode, urlsplit
from uuid import uuid4

from playwright.sync_api import expect, sync_playwright

OWNER = "10000000-0000-4000-8000-000000000001"
MEMBER = "10000000-0000-4000-8000-000000000002"
THREAD = "20000000-0000-4000-8000-000000000001"
TASK = "30000000-0000-4000-8000-000000000001"
OLD_TASK = "30000000-0000-4000-8000-000000000002"
GRANT = "40000000-0000-4000-8000-000000000001"
FLOW = "a" * 64
HANDLE = "b" * 64
CONSENT = "https://accounts.google.com/o/oauth2/v2/auth"
VAULT = "https://vault.example.test"
GOOGLE = {"gmail": "Gmail", "drive": "Google Drive", "docs": "Google Docs",
          "slides": "Google Slides", "sheets": "Google Sheets"}
ALIASES = {"gmail": "지메일", "drive": "구글 드라이브", "docs": "구글 문서",
           "slides": "구글 슬라이드", "sheets": "구글 시트"}
CLAUDE = "https://claude.ai/settings/usage"
PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+tH1sAAAAASUVORK5CYII="
ANSWER = "합성 응답입니다."
ORIGINAL = "브라우저에서 오전 출발 시각을 찾아 줘."
RESULT = "합성 시간표에서 08:15 출발을 확인했습니다."
PAGE_URL = "https://timetable.example.test/"
MAIL_PROMPT = "최근 받은 메일을 확인해 줘."
MAIL_SUBJECT = "합성 주간 회의 공지"
FOLLOW_UP = "주말 운행도 확인해 줘."
NEXT_DRAFT = "다음에 비교할 다른 시간표 초안"
QUOTA_PATH = "/personal/admin/engine/quota"


class StaticFiles(SimpleHTTPRequestHandler):
    def translate_path(self, path):
        entrypoint = {"/app": "/personal.html", "/demo": "/demo.html"}.get(urlsplit(path).path)
        return super().translate_path(entrypoint or path)

    def log_message(self, _format, *args):
        pass


def roster_member(account_id, name, admin=False, ready=True):
    email = "owner@example.test" if admin else "member@example.test"
    return dict(account_id=account_id, email=email, display_name=name, generation=1,
                revision=1, admin=admin, eligible=True, suspended=False, ready=ready)


def turn(thread, request_id, message, sequence=1, state="completed", answer=ANSWER):
    return dict(sequence=sequence, user_text=message,
                assistant=[dict(id=str(uuid4()), text=answer, phase="final_answer", refusal=False)]
                if state == "completed" else [],
                run=dict(id=str(uuid4()), request_id=request_id, thread_id=thread, state=state,
                         model=dict(function_tools=False), can_retry=False, can_cancel=False))


class World:
    def __init__(self, base, admin=False, signed_in=True):
        self.base = base
        self.admin = admin
        self.account = OWNER if admin else MEMBER
        self.signed_in = signed_in
        self.restricted = False
        self.calls = []
        self.errors = []
        self.threads = []
        self.turns = {}
        self.defer_rejection = False
        self.rejected_route = None
        self.lose_invite = False
        self.lose_change = False
        self.invites = {}
        self.changes = {}
        self.members = [roster_member(OWNER, "최관리", admin=True), roster_member(MEMBER, "이온유")]
        self.conflict = False
        self.follow_up_outcome = None
        self.quota_checks = []
        self.now = int(time.time())
        self.choices = [dict(model="claude-sonnet-5-5", effort=effort) for effort in ("medium", "high")]
        self.engine = dict(configured=True, enabled=True, revision=1, kind="anthropic",
                           connection_mode="claude_code", connection_revision="synthetic-one",
                           host_default=self.choices[0], choices=self.choices, default_choice=None,
                           default_stale=False)
        self.google_configured = True
        self.google_grants = {}
        self.mail_reads = []
        self.vault = dict(state="setup_required", site_origin=VAULT)
        self.memory = []
        self.browser = None
        self.browser_request = None
        self.frame_no = 0
        self.task_reads = []
        self.message_reads = []
        self.tasks_visible = False

    def session(self):
        return dict(user=dict(id=self.account, name="서윤" if self.admin else "이온유",
                              email="owner@example.test" if self.admin else "member@example.test",
                              role="member", must_change_password=self.restricted),
                    password_change_required=self.restricted, chat_draft_scope="c" * 64,
                    installation_admin=self.admin)

    def seed_conversation(self):
        self.threads.append(dict(id=THREAD, title="합성 시간표 원래 대화", created_at=self.now, updated_at=self.now))
        active = turn(THREAD, OLD_TASK, ORIGINAL, answer=RESULT)
        active["run"]["id"] = OLD_TASK
        recent = turn(THREAD, TASK, "저녁 운행도 확인해 줘.", 2, answer="합성 저녁 열차입니다.")
        recent["run"]["id"] = TASK
        self.turns[THREAD] = [active, recent]

    def seed_active_answer(self, outcome):
        self.seed_conversation()
        self.follow_up_outcome = outcome
        self.turns[THREAD].pop()
        answer = self.turns[THREAD][0]
        answer["assistant"] = []
        answer["run"].update(state="running", attempt=1, accepts_text_follow_up=True)

    def finish_active_answer(self):
        answer = self.turns[THREAD][0]
        answer["assistant"] = [dict(id="original-answer", text=RESULT, phase="final_answer", refusal=False)]
        answer["run"].update(state="completed", accepts_text_follow_up=False)

    def configure_openrouter(self, *checks):
        model = dict(model="synthetic/model", effort=None)
        self.engine.update(kind="open_router", connection_mode="api", host_default=model, choices=[model])
        self.quota_checks = list(checks)

    def task(self, task_id):
        return dict(id=task_id, thread_id=THREAD, created_at=self.now, state="completed", next_actor="none",
                    title="오전 시간표 확인" if task_id == OLD_TASK else "저녁 시간표 확인")

    def serve(self, route):
        request = route.request
        url = urlsplit(request.url)
        if request.url.startswith(self.base + "/"):
            if not url.path.startswith("/api/v1/"):
                route.continue_()
                return
            path = url.path.removeprefix("/api/v1")
            body = request.post_data_json if request.post_data else None
            self.calls.append((request.method, path, deepcopy(body)))
            if path != "/auth/session" and not path.startswith("/auth/login"):
                if request.headers.get("x-moosuri-account") != self.account:
                    self.errors.append(f"Missing or wrong current account on {request.method} {path}")
            if self.defer_rejection and request.method == "POST" and re.fullmatch(r"/threads/[^/]+/text-preview", path):
                self.defer_rejection = False
                self.rejected_route = route
                return
            try:
                status, data = self.respond(request.method, path, parse_qs(url.query), body)
                if status == 0:
                    route.abort("failed")
                elif status == 204:
                    route.fulfill(status=204)
                else:
                    route.fulfill(status=status, json=data)
            except Exception as error:
                self.errors.append(f"{request.method} {path}: {error}")
                route.fulfill(status=500, json={"code": "unexpected_fixture_request"})
            return
        if request.url.startswith(CONSENT + "?") and url.path == "/o/oauth2/v2/auth":
            service = parse_qs(url.query)["synthetic_service"][0]
            suffix = "" if service == "gmail" else "&" + urlencode({"google_service": service})
            return_url = self.base + "/personal.html#"
            success = return_url + urlencode({"google_handle": HANDLE, "flow": FLOW}) + suffix
            denied = return_url + urlencode({"google_denied": FLOW}) + suffix
            route.fulfill(content_type="text/html", body=f'<!doctype html><title>Synthetic OAuth</title>'
                          f'<h1>Synthetic Google consent</h1><a href="{success}">Allow</a> '
                          f'<a href="{denied}">Deny</a>')
        elif request.url.rstrip("/") == VAULT:
            route.fulfill(content_type="text/html", body="<h1>Synthetic separate vault</h1>")
        else:
            self.errors.append(f"Blocked unexpected network request to {url.scheme}://{url.netloc}{url.path}")
            route.abort("blockedbyclient")

    def respond(self, method, path, query, body):
        if path == "/auth/session":
            return (200, self.session()) if self.signed_in else (401, {"code": "unauthorized"})
        if path == "/auth/login":
            assert body["email"] == "member@example.test" and body["password"] == "synthetic-temporary"
            self.signed_in, self.restricted = True, True
            return 200, self.session()
        if path == "/auth/password":
            assert body["password"] == "synthetic-permanent"
            self.restricted = False
            return 200, self.session()
        if path == "/auth/logout":
            self.signed_in = False
            return 204, None
        if not self.signed_in or self.restricted:
            raise AssertionError("private API before an unrestricted session")
        if path == "/text-preview":
            return 200, dict(enabled=self.engine["enabled"], capability="text_only_preview", tools=False)
        if path == "/threads":
            if method == "GET":
                return 200, dict(threads=self.threads)
            thread = dict(id=body["request_id"], title=body["title"], created_at=self.now, updated_at=self.now)
            if not any(entry["id"] == thread["id"] for entry in self.threads):
                self.threads.append(thread)
            return 201, thread
        if path.startswith("/threads/"):
            return self.chat(method, path, query, body)
        if path.startswith("/knowledge/memories"):
            return self.memories(method, path, body)
        if path.startswith("/personal/admin/"):
            assert self.admin, "ordinary member attempted an admin API"
            return self.installation(method, path.removeprefix("/personal/admin/"), body)
        if path == "/personal/vault":
            return 200, self.vault
        if path.startswith("/personal/google"):
            return self.google(method, path, body)
        if path == "/personal/tasks":
            if not self.tasks_visible:
                return 200, dict(items=[], next_before=None)
            before = query.get("before")
            self.task_reads.append(before)
            if before:
                assert before == ["42"]
                return 200, dict(items=[self.task(OLD_TASK)], next_before=None)
            return 200, dict(items=[self.task(TASK)], next_before="42")
        if path.startswith("/personal/tasks/"):
            if not self.tasks_visible:
                return 404, {"code": "not_found"}
            assert path.endswith(OLD_TASK)
            return 200, dict(task=self.task(OLD_TASK), request=ORIGINAL, thread_title=self.threads[0]["title"],
                             result=RESULT, observation=None, reason=None,
                             history=[dict(state="starting", at=self.now), dict(state="completed", at=self.now)])
        raise AssertionError("route missing in fixture")

    def chat(self, method, path, query, body):
        _, _, thread, *operation = path.split("/")
        entries = self.turns.setdefault(thread, [])
        if operation == ["messages"]:
            before = query.get("before")
            self.message_reads.append((thread, before))
            if self.tasks_visible and thread == THREAD:
                if before:
                    assert before == ["2"]
                    return 200, dict(thread_id=thread, cursor=2, turns=entries[:1], next_before=None)
                return 200, dict(thread_id=thread, cursor=2, turns=entries[1:], next_before=2)
            return 200, dict(thread_id=thread, cursor=len(entries), turns=entries, next_before=None)
        if operation == ["text-preview"]:
            assert body["capability"] == "text_only_preview" and "model" not in body and "effort" not in body
            if body["text"] == MAIL_PROMPT:
                return 422, {"code": "personal_inbox_requested"}
            saved = next((entry for entry in entries if entry["run"]["request_id"] == body["request_id"]), None)
            if saved is None:
                if "follow_up_run_id" in body:
                    original = next(entry["run"] for entry in entries if entry["run"]["id"] == body["follow_up_run_id"])
                    assert original["state"] == "completed"
                    receipt, = original["instructions"]
                    assert (body["request_id"], body["text"]) == (receipt["request_id"], receipt["text"])
                    assert receipt["reason"] == "text_follow_up_saved"
                    receipt["reason"] = "text_follow_up_sent"
                saved = turn(thread, body["request_id"], body["text"], len(entries) + 1)
                entries.append(saved)
            return 201, saved
        if operation == ["text-follow-ups"] and method == "POST":
            return self.save_follow_up(thread, body)
        if not operation and method == "DELETE":
            self.threads = [entry for entry in self.threads if entry["id"] != thread]
            self.turns.pop(thread, None)
            return 204, None
        if operation[:1] == ["browser"]:
            return self.browser_request_route(method, thread, operation[1:], body)
        raise AssertionError("thread operation missing in fixture")

    def save_follow_up(self, thread, body):
        assert thread == THREAD and body["run_id"] == OLD_TASK
        data = body["instruction"]
        run = self.turns[THREAD][0]["run"]
        assert data["expected_attempt"] == run["attempt"] and data["text"] == FOLLOW_UP
        outcome, self.follow_up_outcome = self.follow_up_outcome, None
        if outcome == "lost":
            return 0, None
        if outcome == "settled":
            self.finish_active_answer()
            return 409, {"code": "conflict"}
        assert run["state"] == "running" and not run.get("instructions")
        receipt = dict(request_id=data["request_id"], text=data["text"], thread_id=thread,
                       run_id=run["id"], attempt=run["attempt"], sequence=1, state="closed",
                       reason="text_follow_up_saved")
        run["instructions"] = [receipt]
        return 200, receipt

    def browser_request_route(self, method, thread, operation, body):
        if not operation and method == "POST":
            if body.get("url"):
                assert body["url"].startswith(PAGE_URL)
                self.browser_request = turn(thread, body["request_id"], body["text"], state="running")
                self.turns[thread].append(self.browser_request)
                self.browser = dict(id=TASK, thread_id=thread, request_id=body["request_id"],
                                    run_id=self.browser_request["run"]["id"], revision=1, state="reading",
                                    url=PAGE_URL, active_tab_id="main", title="합성 시간표", links=[], reason=None,
                                    tabs=[dict(id="main", title="시간표", url=PAGE_URL),
                                          dict(id="morning", title="오전", url=PAGE_URL + "morning")])
            else:
                assert body["workspace_id"] == TASK
                self.browser_request["run"]["instructions"] = [dict(request_id=body["request_id"],
                    text=body["text"], state="received")]
        if operation == ["frame"]:
            self.frame_no += 1
            return 200, dict(workspace_id=TASK, revision=self.browser["revision"],
                             tab_id=self.browser["active_tab_id"], url=self.browser["url"],
                             frame_token=f"synthetic-frame-{self.frame_no}", png_base64=PNG, width=900, height=600)
        if operation:
            assert body["workspace_id"] == TASK and body["revision"] == self.browser["revision"]
        if operation == ["takeover"]:
            self.browser.update(state="user_control", revision=self.browser["revision"] + 1)
            return 200, dict(workspace=self.browser, control_token="synthetic-controller")
        if operation in (["tabs"], ["input"], ["resume"]):
            assert body["control_token"] == "synthetic-controller"
        if operation == ["tabs"]:
            assert body["action"] == "switch" and body["tab_id"] == "morning"
            self.browser.update(active_tab_id="morning", url=PAGE_URL + "morning", revision=self.browser["revision"] + 1)
        if operation == ["input"]:
            assert body["frame_token"] == f"synthetic-frame-{self.frame_no}"
            assert body["input"] == {"kind": "scroll", "delta_x": 0, "delta_y": 500}
        if operation == ["resume"]:
            self.browser.update(state="completed", revision=self.browser["revision"] + 1)
            self.browser_request["run"]["state"] = "completed"
            self.browser_request["run"]["instructions"][0]["state"] = "applied"
            self.browser_request["assistant"] = [dict(id="browser-result", text=RESULT, phase="final_answer", refusal=False)]
        if operation == ["close"]:
            self.browser.update(state="closed", revision=self.browser["revision"] + 1)
        return 200, dict(configured=True, workspace=self.browser)

    def memories(self, method, path, body):
        if method == "POST":
            assert not self.memory
            self.memory.append(dict(id="synthetic-memory", revision=1, status="active", draft=body["draft"],
                                    created_at=self.now, updated_at=self.now))
            return 201, self.memory[-1]
        if path.endswith("/history"):
            return 200, dict(items=list(reversed(self.memory)), next_before=None)
        if method == "PUT":
            assert body["expected_revision"] == self.memory[-1]["revision"]
            self.memory.append(dict(self.memory[-1], revision=body["expected_revision"] + 1,
                                    draft=body["draft"], updated_at=self.now + 5))
            return 200, self.memory[-1]
        if path.endswith("/synthetic-memory"):
            return 200, self.memory[-1]
        assert path == "/knowledge/memories"
        return 200, dict(items=self.memory[-1:], next_after=None, generation=1)

    def installation(self, method, path, body):
        if path == "members":
            return 200, dict(members=self.members)
        if path == "invitations":
            request_id = body["request_id"]
            if request_id not in self.invites:
                invited = roster_member(str(uuid4()), body["name"], ready=False)
                invited.update(email=body["email"], invitation_request_id=request_id, invitation_name=body["name"])
                self.members.append(invited)
                self.invites[request_id] = dict(request_id=request_id, email=body["email"])
                if self.lose_invite:
                    return 0, None
            return 201, self.invites[request_id]
        if path == "members/change":
            request_id = body["request_id"]
            if request_id not in self.changes:
                member = next(entry for entry in self.members if entry["account_id"] == body["account_id"])
                assert body["revision"] == member["revision"] and body["generation"] == member["generation"]
                assert body["action"] in ("suspend", "resume") and member["account_id"] == MEMBER
                member.update(suspended=body["action"] == "suspend", revision=member["revision"] + 1)
                self.changes[request_id] = dict(member)
                if self.lose_change:
                    return 0, None
            return 200, self.changes[request_id]
        if path == "engine":
            if method == "POST":
                assert body["revision"] == self.engine["revision"]
                assert body["connection_revision"] == self.engine["connection_revision"]
                if self.conflict:
                    self.conflict = False
                    self.engine.update(revision=self.engine["revision"] + 1, connection_revision="synthetic-two")
                    return 409, {"code": "conflict"}
                self.engine.update(revision=self.engine["revision"] + 1, enabled=body["enabled"],
                                   default_choice=body["default_choice"])
            return 200, self.engine
        if path == "engine/quota":
            assert method == "POST" and self.engine["kind"] == "open_router"
            return self.engine_quota(body)
        if path == "google-setup":
            redirect = "https://assistant.example.test/oauth/google/callback"
            if method == "POST":
                client = json.loads(body["web_client_json"])["web"]
                if client["redirect_uris"] != [redirect]:
                    return 422, {"code": "google_redirect_uri_mismatch"}
                self.google_configured = True
            return 200, dict(configured=self.google_configured, redirect_uri=redirect)
        raise AssertionError("administrator operation missing in fixture")

    def engine_quota(self, body):
        assert body["revision"] == self.engine["revision"]
        assert body["connection_revision"] == self.engine["connection_revision"]
        expected_probe, probe, outcome = self.quota_checks.pop(0)
        assert body["probe_id"] == expected_probe
        if outcome == "pending":
            return 200, dict(probe_id=probe, report=None)
        metrics = [
            dict(id="credit", label="합성 키 사용 한도", unit="usd", used=3, limit=3, remaining=0,
                 reset_at=None, reset_interval="weekly"),
            dict(id="requests", label="합성 요청 수", unit="requests", used=2, limit=None, remaining=None,
                 reset_at=self.now + 3600, reset_interval=None),
        ]
        available = outcome != "unavailable"
        report = dict(provider_id="old-binding" if outcome == "stale" else self.engine["connection_revision"],
                      provider_revision=self.engine["revision"], status="known" if available else "unavailable",
                      metrics=metrics if available else [], observed_at=self.now if available else None)
        return 200, dict(probe_id=probe, report=report)

    def google(self, method, path, body):
        parts = path.removeprefix("/personal/google").strip("/").split("/")
        if parts == [""]:
            return 200, dict(connection=self.google_grants.get("gmail"))
        if parts == ["workspace"]:
            return 200, dict(connections=[dict(grant, service=service) for service, grant in self.google_grants.items()
                                          if service != "gmail"])
        if parts == ["history"]:
            return 200, dict(reads=[entry for entry in self.mail_reads if entry["thread_id"] == body["thread_id"]])
        if parts == ["inbox"]:
            read = body["read"]
            grant = self.google_grants["gmail"]
            assert body["connection_id"] == grant["connection_id"] and body["epoch"] == grant["epoch"]
            assert any(thread["id"] == read["thread_id"] for thread in self.threads)
            assert read["request_id"] == chat_posts(self)[0]["request_id"]
            message = {"id": "synthetic-mail", "subject": MAIL_SUBJECT,
                       "from": "notices@example.test", "date": "20 Nov 2026"}
            receipt = dict(read, state="completed", created_at=self.now, source=dict(grant, app_revision=GRANT),
                           result=dict(mailbox=grant["mailbox"], has_more=False,
                                       scope="one_inbox_page_metadata_only", messages=[message]))
            self.mail_reads.append(receipt)
            return 200, receipt
        service, action = parts if len(parts) == 2 else ("gmail", parts[0])
        assert method == "POST" and service in GOOGLE
        if action == "start":
            return 200, dict(flow_id=FLOW, authorization_url=CONSENT + "?" + urlencode({"synthetic_service": service}))
        if action == "complete":
            assert body["handle"] == HANDLE
            grant = dict(connection_id=GRANT, epoch=1, mailbox=f"{service}@example.test")
            self.google_grants[service] = grant
            return 200, dict(grant, flow_id=FLOW)
        if action == "disconnect":
            assert body == {"connection_id": GRANT, "epoch": 1}
            del self.google_grants[service]
            return 204, None
        raise AssertionError("unexpected Google operation, including any implicit inbox read")


def button(parent, name):
    return parent.get_by_role("button", name=name, exact=isinstance(name, str))


def heading(parent, name):
    return parent.get_by_role("heading", name=name, exact=True)


def nav(page, name):
    return button(page.get_by_role("navigation", name="주 메뉴"), name)


def posts(world, path):
    return [body for method, actual, body in world.calls if method == "POST" and
            (actual == path if isinstance(path, str) else path.fullmatch(actual))]


def chat_posts(world):
    return posts(world, re.compile(r"/threads/[^/]+/text-preview"))


def composer(page):
    return page.get_by_role("textbox", name="메시지", exact=True)


def check_layout(page):
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), "document overflows horizontally"
    workspace = page.locator(".member-workspace")
    if workspace.count():
        assert workspace.evaluate("e => getComputedStyle(e).display === 'grid'"), "workspace CSS not loaded"
    assert page.locator(".member-panel:visible").evaluate_all(
        "panels => panels.every(panel => panel.scrollWidth <= panel.clientWidth + 1)"
    ), "member panel overflows horizontally"


def sign_in(page, world):
    expect(heading(page, "내 무수리에 로그인")).to_be_visible()
    page.get_by_label("이메일", exact=True).fill("member@example.test")
    page.get_by_label("비밀번호", exact=True).fill("synthetic-temporary")
    button(page, "로그인").click()
    expect(heading(page, "임시 비밀번호 변경")).to_be_visible()
    assert not any(path == "/threads" for _, path, _ in world.calls)
    page.get_by_label("새 비밀번호", exact=True).fill("synthetic-permanent")
    button(page, "비밀번호 변경").click()
    expect(composer(page)).to_be_enabled()
    page.reload()
    expect(composer(page)).to_be_enabled()
    button(page, "대화 목록").click()
    button(page.get_by_role("dialog", name="대화 목록"), "로그아웃").click()
    expect(heading(page, "내 무수리에 로그인")).to_be_visible()
    page.reload()
    expect(heading(page, "내 무수리에 로그인")).to_be_visible()


def author_and_restore(page, world):
    expect(composer(page)).to_be_enabled()
    examples = page.get_by_role("group", name="메시지 작성 예시")
    examples.get_by_role("button").first.click()
    expect(composer(page)).to_be_focused()
    assert composer(page).input_value() and chat_posts(world) == []
    composer(page).fill("")
    composer(page).press_sequentially("What is 2 + 2?")
    nav(page, "기억").click()
    nav(page, "대화").click()
    expect(composer(page)).to_have_value("What is 2 + 2?")
    composer(page).press("Enter")
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()
    assert len(world.threads) == 1 and len(chat_posts(world)) == 1
    page.reload()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()
    assert len(chat_posts(world)) == 1
    button(page, "대화 목록").click()
    drawer = page.get_by_role("dialog", name="대화 목록")
    expect(heading(drawer, "오늘")).to_be_visible()
    drawer.get_by_role("searchbox", name="대화 검색").fill("2 + 2")
    expect(button(drawer, "What is 2 + 2?")).to_be_visible()
    drawer.get_by_role("searchbox", name="대화 검색").press("Enter")
    expect(drawer).to_be_hidden()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()


def reject_and_retry(page, world):
    expect(composer(page)).to_be_enabled()
    world.defer_rejection = True
    composer(page).press_sequentially("First message A")
    with page.expect_request(re.compile(r"/threads/[^/]+/text-preview$")):
        button(page, "보내기").click()
    expect(composer(page)).to_have_value("")
    composer(page).press_sequentially("Later draft B")
    assert world.rejected_route is not None
    world.rejected_route.fulfill(status=422, json={"code": "invalid_request"})
    retry = button(page, "이전 메시지 다시 보내기")
    expect(retry).to_be_visible()
    expect(composer(page)).to_have_value("Later draft B")
    page.reload()
    expect(retry).to_be_enabled()
    expect(page.get_by_role("region", name="대화", exact=True).get_by_text("First message A", exact=True)).to_be_visible()
    expect(composer(page)).to_have_value("Later draft B")
    first, = chat_posts(world)
    retry.click()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()
    assert chat_posts(world) == [first, first]
    expect(composer(page)).to_have_value("Later draft B")
    page.reload()
    expect(composer(page)).to_have_value("Later draft B")
    expect(retry).to_have_count(0)
    assert len(chat_posts(world)) == 2


def submit_follow_up(page):
    expect(page.get_by_text(ORIGINAL, exact=True)).to_be_visible()
    expect(button(page, "후속 지시 저장")).to_be_disabled()
    composer(page).fill(FOLLOW_UP)
    expect(button(page, "보내기")).to_be_disabled()
    button(page, "후속 지시 저장").click()


def recover_and_send_follow_up(page, world):
    submit_follow_up(page)
    retry = button(page, "같은 지시 저장 다시 확인")
    expect(retry).to_be_enabled()
    composer(page).fill(NEXT_DRAFT)
    page.reload()
    expect(retry).to_be_enabled()
    expect(composer(page)).to_have_value(NEXT_DRAFT)
    expect(button(page, "작성창으로 가져오기")).to_be_disabled()
    path = f"/threads/{THREAD}/text-follow-ups"
    original, = posts(world, path)
    retry.click()
    send = button(page, "후속 메시지로 보내기")
    expect(page.get_by_text("후속 지시 저장됨 · 이 답변에는 미반영", exact=True)).to_be_visible()
    expect(send).to_be_disabled()
    assert posts(world, path) == [original, original] and not chat_posts(world)
    world.finish_active_answer()
    page.reload()
    expect(page.get_by_text(RESULT, exact=True)).to_be_visible()
    expect(send).to_be_enabled()
    assert not chat_posts(world)
    send.click()
    expect(page.get_by_text("후속 메시지로 보냄", exact=True)).to_be_visible()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()
    assert chat_posts(world) == [dict(request_id=original["instruction"]["request_id"],
        text=FOLLOW_UP, capability="text_only_preview", follow_up_run_id=OLD_TASK)]
    page.reload()
    expect(page.get_by_text("후속 메시지로 보냄", exact=True)).to_be_visible()
    expect(composer(page)).to_have_value(NEXT_DRAFT)
    assert len(world.turns[THREAD]) == 2 and len(chat_posts(world)) == 1


def restore_follow_up_after_completion(page, world):
    submit_follow_up(page)
    expect(page.get_by_text("이 답변에 후속 지시를 저장하지 못했습니다.", exact=True)).to_be_visible()
    expect(page.get_by_text(RESULT, exact=True)).to_be_visible()
    composer(page).fill(NEXT_DRAFT)
    page.reload()
    restore = button(page, "작성창으로 가져오기")
    expect(restore).to_be_disabled()
    expect(composer(page)).to_have_value(NEXT_DRAFT)
    composer(page).clear()
    restore.click()
    expect(composer(page)).to_have_value(FOLLOW_UP)
    expect(restore).to_have_count(0)
    assert not chat_posts(world)
    assert len(posts(world, f"/threads/{THREAD}/text-follow-ups")) == 1
    assert len(world.turns[THREAD]) == 1


def memory_editor(page, world):
    nav(page, "기억").click()
    button(page, "기억 추가").click()
    editor = page.get_by_role("dialog", name="기억 작성")
    editor.get_by_label("기억 주제").fill("아침차")
    editor.get_by_label("기억 내용").fill("아삼")
    page.keyboard.press("Escape")
    nav(page, "대화").click()
    nav(page, "기억").click()
    button(page, "기억 작성 이어가기").click()
    expect(editor.get_by_label("기억 주제")).to_have_value("아침차")
    expect(editor.get_by_label("기억 내용")).to_have_value("아삼")
    assert world.memory == []
    button(editor, "기억 저장").click()
    row = button(page, re.compile("아침차"))
    expect(row).to_be_visible()
    detail = page.get_by_role("dialog", name="기억 읽기")
    if not detail.is_visible():
        row.click()
    expect(detail.get_by_text("아삼", exact=True)).to_be_visible()
    button(detail, "기억 수정").click()
    editor.get_by_label("기억 내용").fill("실론")
    button(editor, "기억 저장").click()
    expect(detail.get_by_text("실론", exact=True)).to_be_visible()
    button(detail, "수정 이력").click()
    expect(detail.get_by_text("저장 버전 1", exact=False)).to_be_visible()
    expect(detail.get_by_text("저장 버전 2", exact=False)).to_be_visible()
    expect(detail.get_by_text("아삼", exact=True)).to_be_visible()
    assert detail.locator("time[datetime]").count() >= 3
    assert [entry["revision"] for entry in world.memory] == [1, 2]


def consent(page, world, service, decision):
    button(page, "Google 연결").click()
    expect(heading(page, "Synthetic Google consent")).to_be_visible()
    page.get_by_role("link", name=decision, exact=True).click()
    expect(heading(page, GOOGLE[service])).to_be_visible()
    expect(page).to_have_url(re.compile(rf"#connections/{service}$"))


def connector(page, world, service, action):
    name = GOOGLE[service]
    if world.admin:
        nav(page, "관리").click()
        button(page, "어시스턴트 엔진").click()
    else:
        nav(page, "기억").click()
    if action == "disconnect":
        world.google_grants[service] = dict(connection_id=GRANT, epoch=1, mailbox=f"{service}@example.test")
    nav(page, "연결").click()
    expect(heading(page, "커넥터")).to_be_visible()
    page.get_by_role("searchbox", name="커넥터 검색").fill(ALIASES[service])
    button(page, re.compile(r"^" + name)).click()
    detail = page.get_by_role("region", name=name, exact=True)
    expect(detail).to_be_visible()
    check_layout(page)
    if action == "success":
        consent(page, world, service, "Allow")
        expect(detail.get_by_text(f"{service}@example.test", exact=True)).to_be_visible()
        page.reload()
        expect(heading(page, name)).to_be_visible()
        expect(detail.get_by_text(f"{service}@example.test", exact=True)).to_be_visible()
    elif action == "cancel":
        consent(page, world, service, "Deny")
        expect(page.get_by_text("Google 연결이 취소됐습니다.", exact=True)).to_be_visible()
        expect(button(page, "Google 연결")).to_be_enabled()
    else:
        expect(detail.get_by_text(f"{service}@example.test", exact=True)).to_be_visible()
        button(page, "연결 해제").click()
        expect(detail.get_by_text("연결된 계정이 없습니다.", exact=True)).to_be_visible()
        assert service not in world.google_grants
    button(page, "뒤로").click()
    expect(heading(page, "커넥터")).to_be_visible()
    button(page, "뒤로").click()
    expect(nav(page, "관리" if world.admin else "기억")).to_have_attribute("aria-current", "page")
    if world.admin:
        expect(page.get_by_role("form", name="설치 모델 설정")).to_be_visible()
    assert not any(path == "/personal/google/inbox" for _, path, _ in world.calls)
    complete = "/personal/google/complete" if service == "gmail" else f"/personal/google/{service}/complete"
    assert len(posts(world, complete)) == (1 if action == "success" else 0)


def gmail_read(page, world):
    world.google_grants["gmail"] = dict(connection_id=GRANT, epoch=1, mailbox="gmail@example.test")
    nav(page, "연결").click()
    button(page, re.compile(r"^Gmail")).click()
    expect(page.get_by_text("gmail@example.test", exact=True)).to_be_visible()
    assert not posts(world, "/personal/google/inbox")
    nav(page, "대화").click()
    expect(composer(page)).to_be_enabled()
    composer(page).fill(MAIL_PROMPT)
    button(page, "보내기").click()
    expect(heading(page, MAIL_PROMPT)).to_be_visible()
    history = page.get_by_role("region", name="이 대화의 메일 확인 기록")
    expect(history.get_by_text(MAIL_SUBJECT, exact=True)).to_be_visible()
    assert len(world.mail_reads) == len(world.threads) == 1
    assert not world.turns[world.threads[0]["id"]]
    page.reload()
    expect(history.get_by_text(MAIL_SUBJECT, exact=True)).to_be_visible()
    assert len(posts(world, "/personal/google/inbox")) == 1


def browser_history(page, world):
    nav(page, "기억").click()
    nav(page, "연결").click()
    button(page, re.compile("^Gmail")).click()
    expect(heading(page, "Gmail")).to_be_visible()
    page.go_back()
    expect(heading(page, "커넥터")).to_be_visible()
    page.go_back()
    expect(nav(page, "기억")).to_have_attribute("aria-current", "page")
    page.go_forward()
    expect(heading(page, "커넥터")).to_be_visible()
    page.go_forward()
    expect(heading(page, "Gmail")).to_be_visible()
    page.reload()
    expect(heading(page, "Gmail")).to_be_visible()
    check_layout(page)


def consent_history_from_task(page, world):
    world.seed_conversation()
    world.tasks_visible = True
    nav(page, "맡긴 일").click()
    button(page, "이전 작업 더 보기").click()
    button(page, re.compile("오전 시간표 확인")).click()
    expect(heading(page, "진행 기록")).to_be_visible()
    task_url = page.url
    nav(page, "연결").click()
    button(page, re.compile(r"^Gmail")).click()
    consent(page, world, "gmail", "Deny")
    expect(page.get_by_text("Google 연결이 취소됐습니다.", exact=True)).to_be_visible()
    page.go_back()
    expect(heading(page, "커넥터")).to_be_visible()
    page.go_back()
    expect(page).to_have_url(task_url)
    expect(heading(page, "진행 기록")).to_be_visible()
    page.go_forward()
    expect(heading(page, "커넥터")).to_be_visible()
    page.go_forward()
    expect(heading(page, "Gmail")).to_be_visible()
    assert not posts(world, "/personal/google/complete")
    assert not posts(world, "/personal/google/inbox")


def vault_tab(page, world):
    nav(page, "연결").click()
    button(page, re.compile("^볼트")).click()
    expect(page.get_by_text("별도 로그인", exact=True)).to_be_visible()
    with page.context.expect_page() as popup:
        page.get_by_role("link", name="볼트 열기").click()
    vault = popup.value
    expect(heading(vault, "Synthetic separate vault")).to_be_visible()
    expect(heading(page, "볼트")).to_be_visible()
    vault.close()


def admin_invite(page, world):
    world.lose_invite = True
    nav(page, "관리").click()
    expect(heading(page, "회원 관리")).to_be_visible()
    button(page, "회원 초대").click()
    form = page.get_by_role("form", name="초대 요청")
    form.get_by_label("이름", exact=True).fill("한새론")
    form.get_by_label("이메일", exact=True).fill("new@example.test")
    button(form, "초대 요청 등록").click()
    expect(page.get_by_role("alert")).to_contain_text("초대 접수 결과를 받지 못했습니다")
    original, = posts(world, "/personal/admin/invitations")
    nav(page, "기억").click()
    nav(page, "관리").click()
    page.reload()
    expect(form.get_by_label("이메일", exact=True)).to_have_value("new@example.test")
    button(form, "초대 접수 확인").click()
    receipt = page.get_by_role("region", name="최근 초대")
    expect(receipt).to_contain_text("new@example.test")
    expect(receipt).to_contain_text(original["request_id"])
    assert posts(world, "/personal/admin/invitations") == [original, original]
    assert len(world.invites) == 1
    page.reload()
    expect(receipt).to_contain_text(original["request_id"])
    check_layout(page)


def admin_change(page, world):
    world.lose_change = True
    nav(page, "관리").click()
    page.get_by_role("searchbox", name="회원 검색", exact=True).fill("이온유")
    expect(page.get_by_text("member@example.test", exact=True)).to_be_visible()
    expect(page.get_by_text("owner@example.test", exact=True)).not_to_be_visible()
    page.get_by_role("searchbox", name="회원 검색", exact=True).fill("member@example.test")
    page.get_by_label("member@example.test 계정 관리", exact=True).click()
    suspend = button(page, "계정 정지")
    suspend.click()
    dialog = page.get_by_role("dialog")
    expect(dialog).to_be_visible()
    page.keyboard.press("Escape")
    expect(dialog).to_be_hidden()
    expect(suspend).to_be_focused()
    assert not world.changes
    suspend.click()
    button(dialog, "계정 정지").click()
    expect(page.get_by_role("alert")).to_contain_text("변경 결과를 받지 못했습니다")
    original, = posts(world, "/personal/admin/members/change")
    page.reload()
    button(page, "변경 결과 확인").click()
    expect(page.get_by_text("member@example.test · 계정을 정지했습니다.", exact=True)).to_be_visible()
    button(page.get_by_role("group", name="회원 상태 필터"), re.compile(r"^정지 ")).click()
    expect(page.get_by_text("member@example.test", exact=True)).to_be_visible()
    assert posts(world, "/personal/admin/members/change") == [original, original]
    assert len(world.changes) == 1


def google_setup(page, world):
    world.google_configured = False
    nav(page, "관리").click()
    button(page, "Google Workspace 설정 열기").click()
    expect(heading(page, "Google Workspace 설정")).to_be_visible()
    button(page, "이미 받은 JSON 가져오기").click()
    callback = page.get_by_label("이 설치의 콜백 주소", exact=True).input_value()
    upload = page.get_by_label("Google 웹 클라이언트 JSON", exact=True)
    for redirect in ("https://different.example.test/oauth/google/callback", callback):
        file = {"web": {"client_id": "synthetic-id", "client_secret": "synthetic-placeholder", "redirect_uris": [redirect]}}
        upload.set_input_files({"name": "synthetic.json", "mimeType": "application/json", "buffer": json.dumps(file).encode()})
        button(page, "이 설치에 저장").click()
        if redirect != callback:
            expect(page.get_by_role("alert")).to_contain_text("콜백 주소가 다릅니다")
    expect(heading(page, "Google 앱 설정을 저장했습니다.")).to_be_visible()
    button(page, "뒤로").click()
    expect(heading(page, "회원 관리")).to_be_visible()
    assert world.google_configured


def google_from_chat(page, world):
    for text, title in (("구글 로그인하고 GWS까지 등록해줘", "커넥터"),
                        ("지메일 연결해 주세요", "Gmail"), ("구글 드라이브 연동해줘", "Google Drive")):
        composer(page).fill(text)
        composer(page).press("Enter")
        expect(heading(page, title)).to_be_visible()
        assert not posts(world, "/personal/google/start") and not posts(world, "/personal/google/drive/start")
        assert not posts(world, "/personal/google/inbox") and not chat_posts(world)
        page.go_back()
        expect(composer(page)).to_be_visible()


def google_setup_to_catalog(page, world):
    nav(page, "관리").click()
    button(page, "Google Workspace 설정 열기").click()
    expect(heading(page, "Google 앱 설정이 등록되어 있습니다.")).to_be_visible()
    button(page, "커넥터 열기").click()
    expect(heading(page, "커넥터")).to_be_visible()
    assert not posts(world, "/personal/admin/google-setup")


def google_setup_from_connector(page, world):
    nav(page, "연결").click()
    button(page, re.compile(r"^Gmail")).click()
    button(page, "Google Workspace 설정").click()
    expect(heading(page, "Google 앱 설정이 등록되어 있습니다.")).to_be_visible()
    page.reload()
    expect(heading(page, "Google 앱 설정이 등록되어 있습니다.")).to_be_visible()
    button(page, "뒤로").click()
    expect(heading(page, "Gmail")).to_be_visible()
    assert not posts(world, "/personal/admin/google-setup")


def engine(page, world):
    expect(composer(page)).to_be_disabled()
    nav(page, "관리").click()
    button(page, "어시스턴트 엔진").click()
    form = page.get_by_role("form", name="설치 모델 설정")
    expect(form).to_be_visible()
    quota = page.get_by_role("region", name="공급자 잔여 한도")
    expect(quota).to_contain_text("잔여량 확인 불가 · 초기화 시각 확인 불가")
    expect(quota.get_by_role("link", name="Claude 구독 사용량 열기")).to_have_attribute("href", CLAUDE)
    quota.get_by_text("무수리 회원별 사용 집계", exact=True).click()
    expect(quota.get_by_role("link", name="서버에서 회원별 집계 확인하기")).to_be_visible()
    world.conflict = True
    form.get_by_role("combobox", name="모델", exact=True).select_option("claude-sonnet-5-5")
    form.get_by_role("combobox", name="추론 강도", exact=True).select_option("high")
    expect(form.get_by_text("저장하면", exact=True)).to_be_visible()
    button(form, "설치 설정 저장").click()
    expect(page.get_by_role("alert")).to_contain_text("엔진 설정이 변경되었습니다")
    expect(form.get_by_role("combobox", name="모델", exact=True)).to_have_value("")
    form.get_by_role("combobox", name="모델", exact=True).select_option("claude-sonnet-5-5")
    form.get_by_role("combobox", name="추론 강도", exact=True).select_option("high")
    form.get_by_label("새 모델 요청 허용", exact=True).check()
    button(form, "설치 설정 저장").click()
    expect(page.get_by_text("엔진 설정을 저장했습니다. 새 요청부터 적용됩니다.", exact=True)).to_be_visible()
    first, second = posts(world, "/personal/admin/engine")
    assert first["revision"] + 1 == second["revision"] and first["connection_revision"] != second["connection_revision"]
    assert world.engine["default_choice"] == {"model": "claude-sonnet-5-5", "effort": "high"}
    nav(page, "기억").click()
    nav(page, "대화").click()
    expect(composer(page)).to_be_enabled()
    composer(page).fill("엔진을 다시 켰습니다.")
    button(page, "보내기").click()
    expect(page.get_by_text(ANSWER, exact=True)).to_be_visible()


def admin_bookmark_member(page, world):
    page.goto(world.base + "/personal.html#admin/engine")
    expect(composer(page)).to_be_enabled()
    expect(nav(page, "관리")).to_have_count(0)
    expect(page.get_by_role("form", name="설치 모델 설정")).to_have_count(0)
    assert not any(path.startswith("/personal/admin/") for _, path, _ in world.calls)


def open_quota(page, world):
    nav(page, "관리").click()
    button(page, "어시스턴트 엔진").click()
    quota = page.get_by_role("region", name="공급자 잔여 한도")
    expect(quota).to_be_visible()
    expect(quota).to_contain_text("잔여량 확인 불가 · 초기화 시각 확인 불가")
    assert not posts(world, QUOTA_PATH)
    return quota


def read_openrouter_quota(page, world):
    world.configure_openrouter((None, "first", "pending"), ("first", "first", "known"),
                               (None, "second", "unavailable"))
    quota = open_quota(page, world)
    button(quota, "남은 사용량 확인").click()
    expect(button(quota, "사용량 확인 중…")).to_be_disabled()
    credit = quota.get_by_text("합성 키 사용 한도", exact=True).locator("..")
    used, zero = page.evaluate('() => [3, 0].map(value => new Intl.NumberFormat(undefined, {style:"currency", currency:"USD"}).format(value))')
    expect(credit).to_contain_text(f"사용 {used} · 한도 {used} · 남은 양 {zero} · 리셋 주기 매주 (시각 미제공)")
    requests = quota.get_by_text("합성 요청 수", exact=True).locator("..")
    expect(requests).to_contain_text("사용 2회 · 잔여량 미제공 · 리셋")
    expect(requests).not_to_contain_text("리셋 시각 미제공")
    expect(quota.get_by_text(re.compile("기준$"))).to_be_visible()
    button(quota, "사용량 다시 확인").click()
    expect(quota).to_contain_text("제공사에서 한도 정보를 받지 못했습니다.")
    expect(credit).to_have_count(0)
    expect(quota.get_by_text(re.compile("기준$"))).to_have_count(0)
    assert not world.quota_checks


def retry_stale_openrouter_quota(page, world):
    world.configure_openrouter((None, "old-binding-probe", "stale"), (None, "current-binding-probe", "known"))
    quota = open_quota(page, world)
    check = button(quota, "남은 사용량 확인")
    check.click()
    expect(quota.get_by_role("status")).to_have_text("사용량을 새로 확인하지 못했습니다.")
    expect(quota.get_by_text("합성 키 사용 한도", exact=True)).to_have_count(0)
    assert len(posts(world, QUOTA_PATH)) == 1
    check.click()
    expect(quota.get_by_text("합성 키 사용 한도", exact=True)).to_be_visible()
    expect(quota.get_by_role("status")).to_have_count(0)
    assert not world.quota_checks


def demo_member_entry(page, world):
    expect(page).to_have_url(world.base + "/demo")
    expect(heading(page, "예시 대화")).to_be_visible()
    examples = page.get_by_role("navigation", name="예시 선택")
    expect(button(examples, "시간표 확인")).to_have_attribute("aria-pressed", "true")
    expect(page.get_by_text("오후 3시 40분, 7번 승강장에서 출발해요.", exact=True)).to_be_visible()
    for title, evidence in (
        ("기억 정정", "예시 기억 · 보리차 → 현미차"),
        ("맡긴 일", "예시 작업 기록 · 요청 → 시간표 확인 → 완료"),
    ):
        example = button(examples, title)
        example.press("Space")
        expect(example).to_have_attribute("aria-pressed", "true")
        expect(examples.locator('[aria-pressed="true"]')).to_have_count(1)
        expect(page.get_by_text(evidence, exact=True)).to_be_visible()
    draft = page.get_by_role("textbox", name="입력 연습")
    draft.press_sequentially("Synthetic practice prompt")
    expect(draft).to_have_value("Synthetic practice prompt")
    check_layout(page)
    assert world.calls == []
    page.get_by_role("link", name="회원 공간 열기").click()
    expect(page).to_have_url(world.base + "/app")
    expect(heading(page, "내 무수리에 로그인")).to_be_visible()
    assert world.calls == [("GET", "/auth/session", None)]


def demo_draft_reset(page, world):
    open_examples = page.get_by_role("link", name="예시 대화 둘러보기")
    expect(heading(page, "내 무수리에 로그인")).to_be_visible()
    previous_calls = list(world.calls)
    open_examples.click()
    expect(heading(page, "예시 대화")).to_be_visible()
    draft = page.get_by_role("textbox", name="입력 연습")
    draft.fill("Synthetic draft only on this screen")
    button(page, "맡긴 일").click()
    expect(draft).to_have_value("Synthetic draft only on this screen")
    assert world.calls == previous_calls
    page.get_by_role("link", name="회원 공간 열기").click()
    expect(heading(page, "내 무수리에 로그인")).to_be_visible()
    previous_calls = list(world.calls)
    open_examples.click()
    expect(heading(page, "예시 대화")).to_be_visible()
    expect(button(page, "시간표 확인")).to_have_attribute("aria-pressed", "true")
    expect(draft).to_have_value("")
    assert world.calls == previous_calls and all(path == "/auth/session" for _, path, _ in world.calls)


def browser_takeover(page, world):
    expect(composer(page)).to_be_enabled()
    composer(page).fill(f"브라우저에서 {PAGE_URL} 열어 주세요. 직접 조건을 바꿀게요.")
    button(page, "보내기").click()
    preview = button(page, "미리 보기 열기")
    expect(preview).to_be_visible()
    composer(page).fill("오전 출발만 봐줘")
    button(page, "보내기").click()
    expect(page.get_by_text("추가 지시 접수됨 · 아직 반영 전", exact=True)).to_be_visible()
    preview.click()
    viewer = page.get_by_role("dialog" if page.viewport_size["width"] <= 860 else "complementary",
                              name="개인 브라우저 작업 공간")
    expect(viewer.get_by_role("img", name="개인 브라우저의 현재 화면")).to_be_visible()
    expect(button(viewer, "무수리에게 넘기기")).to_have_count(0)
    button(viewer, "직접 조작 시작").click()
    button(viewer, "오전").click()
    expect(button(viewer, "오전")).to_have_attribute("aria-current", "true")
    button(viewer, "브라우저 아래로 스크롤").click()
    button(viewer, "무수리에게 넘기기").click()
    expect(page.get_by_text(RESULT, exact=True)).to_be_visible()
    expect(page.get_by_text("추가 지시 반영됨", exact=True)).to_be_visible()
    button(viewer, "뷰어 닫기").click()
    expect(viewer).to_be_hidden()
    expect(preview).to_be_focused()
    assert all(not path.endswith("/close") for _, path, _ in world.calls)
    assert not chat_posts(world)
    assert "synthetic-controller" not in page.evaluate("JSON.stringify(sessionStorage)")


def tasks_to_original(page, world):
    nav(page, "맡긴 일").click()
    expect(heading(page, "맡긴 일")).to_be_visible()
    button(page, "이전 작업 더 보기").click()
    button(page, re.compile("오전 시간표 확인")).click()
    expect(heading(page, "진행 기록")).to_be_visible()
    expect(page.get_by_text(ORIGINAL, exact=True)).to_be_visible()
    expect(page.get_by_text(RESULT, exact=True)).to_be_visible()
    detail_url = page.url
    page.reload()
    expect(page).to_have_url(detail_url)
    expect(page.get_by_text(RESULT, exact=True)).to_be_visible()
    button(page, re.compile("원래 대화 열기")).click()
    expect(heading(page, "합성 시간표 원래 대화")).to_be_visible()
    expect(page.get_by_text(ORIGINAL, exact=True)).to_be_visible()
    expect(page.get_by_text(RESULT, exact=True)).to_be_visible()
    expect(page.locator(f"#member-turn-{OLD_TASK}")).to_be_focused()
    assert (THREAD, ["2"]) in world.message_reads
    assert ["42"] in world.task_reads and not chat_posts(world)


SCENARIOS = {
    "B-AUTH-01": (False, sign_in),
    "B-CHAT-01": (False, author_and_restore),
    "B-CHAT-02": (False, reject_and_retry),
    "B-FTU-01": (False, recover_and_send_follow_up),
    "B-FTU-02": (False, restore_follow_up_after_completion),
    "B-MEM-01": (False, memory_editor),
    "B-NAV-01": (False, browser_history),
    "B-NAV-02": (False, consent_history_from_task),
    "B-VAULT-01": (False, vault_tab),
    "B-ADM-01": (True, admin_invite),
    "B-ADM-02": (True, admin_change),
    "B-ADM-03": (False, admin_bookmark_member),
    "B-GWS-01": (True, google_setup),
    "B-GWS-02": (True, google_setup_to_catalog),
    "B-GWS-03": (True, google_setup_from_connector),
    "B-GWS-04": (True, google_from_chat),
    "B-ENG-01": (True, engine),
    "B-ENG-02": (True, read_openrouter_quota),
    "B-ENG-03": (True, retry_stale_openrouter_quota),
    "B-DEMO-01": (False, demo_member_entry),
    "B-DEMO-02": (False, demo_draft_reset),
    "B-BR-01": (False, browser_takeover),
    "B-TASK-01": (False, tasks_to_original),
    "B-GMAIL-04": (False, gmail_read),
}
for service in GOOGLE:
    for index, action in enumerate(("success", "cancel", "disconnect"), 1):
        SCENARIOS[f"B-{service.upper()}-{index:02}"] = (action == "cancel", partial(connector, service=service, action=action))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dist", type=Path, help="path to the built web/dist directory")
    parser.add_argument("--browser", choices=("chrome", "chromium", "webkit"), default="chrome")
    parser.add_argument("--width", type=int, choices=(360, 390, 1440), action="append")
    parser.add_argument("--scenario", choices=sorted(SCENARIOS), action="append")
    parser.add_argument("--headed", action="store_true")
    args = parser.parse_args()
    if not (args.dist / "personal.html").is_file():
        parser.error("the dist directory must contain the built personal.html entry")
    server = ThreadingHTTPServer(("127.0.0.1", 0), partial(StaticFiles, directory=str(args.dist.resolve())))
    server_thread = Thread(target=server.serve_forever, daemon=True)
    server_thread.start()
    base = f"http://127.0.0.1:{server.server_port}"
    heights = {360: 780, 390: 844, 1440: 900}
    failures = 0
    try:
        with sync_playwright() as playwright:
            if args.browser == "webkit":
                browser = playwright.webkit.launch(headless=not args.headed)
            else:
                browser = playwright.chromium.launch(channel=args.browser, headless=not args.headed)
            print(f"{args.browser} {browser.version}", flush=True)
            for scenario in args.scenario or SCENARIOS:
                admin, run = SCENARIOS[scenario]
                for width in args.width or heights:
                    world = World(base, admin=admin, signed_in=scenario not in ("B-AUTH-01", "B-DEMO-01", "B-DEMO-02"))
                    if scenario == "B-TASK-01":
                        world.seed_conversation()
                        world.tasks_visible = True
                    if scenario == "B-ENG-01":
                        world.engine["enabled"] = False
                    if scenario in ("B-FTU-01", "B-FTU-02"):
                        world.seed_active_answer("lost" if scenario == "B-FTU-01" else "settled")
                    context = browser.new_context(viewport={"width": width, "height": heights[width]},
                                                  service_workers="block")
                    context.route("**/*", world.serve)
                    page = context.new_page()
                    page.on("pageerror", lambda error: world.errors.append(str(error)))
                    page.set_default_timeout(5_000)
                    page.set_default_navigation_timeout(15_000)
                    try:
                        entrypoint = {"B-DEMO-01": "/demo", "B-DEMO-02": "/app"}.get(scenario, "/personal.html")
                        page.goto(base + entrypoint, wait_until="domcontentloaded")
                        run(page, world)
                        check_layout(page)
                        assert not world.errors, world.errors
                        print(f"PASS {scenario} {width}px", flush=True)
                    except Exception:
                        failures += 1
                        print(f"FAIL {scenario} {width}px {page.url}\n{traceback.format_exc()}", flush=True)
                        if world.errors:
                            print("Fixture errors:", *world.errors, sep="\n", flush=True)
                    finally:
                        context.close()
            browser.close()
    finally:
        server.shutdown()
        server_thread.join()
        server.server_close()
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
