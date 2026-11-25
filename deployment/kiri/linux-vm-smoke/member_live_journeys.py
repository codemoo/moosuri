"""Real Claude journeys in fresh Linux member volumes. Retain only digests and counts in the harness."""
import hashlib
import importlib
import json
import re
import time

from playwright.sync_api import TimeoutError as PlaywrightTimeout, expect

import claude_cli


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


class Receipts:
    def __init__(self):
        self.events = []
        self.outcomes = []

    def interpretation(self, request, status, payload):
        # Record only a digest and typed outcome after the gateway has finished its response.
        # Chromium can discard a response body on navigation, even after receiving HTTP 200.
        data = json.loads(request)
        if data.get("read_only") or not isinstance(data.get("text"), str):
            return
        try:
            body = json.loads(payload)
        except (ValueError, UnicodeDecodeError):
            body = {}
        view = body.get("view") or {}
        self.outcomes.append((digest(data["text"]), status, body.get("kind"), view.get("kind")))


    def record(self, classifier, messages, metrics):
        # This runs only after the official CLI verified the requested model's usage receipt.
        entry = {"kind": "interpret" if classifier else "answer", "tokens": metrics["output_tokens"]}
        if not classifier:
            text = messages[-1][1]
            try:
                body = json.loads(text)
            except ValueError:
                body = None
            memories = []
            if isinstance(body, dict) and "member_memories" in body:
                memories = [digest(m["text"]) for m in body["member_memories"]]
                text = body["current_user_request"]
            entry.update(request=digest(text), memories=memories,
                         history=tuple(digest(m[1]) for m in messages[1:-1]))
        self.events.append(entry)


MEMORIES = """select m.id,m.revision,json_extract(v.draft,'$.subject'),json_extract(v.draft,'$.text')
              from knowledge_memories m join knowledge_memory_versions v on
              v.tenant_id=m.tenant_id and v.memory_id=m.id and v.revision=m.revision
              where m.status='active'"""
ANSWERS = """select r.id,r.thread_id,r.state,o.text from chat_runs r
             join personal_text_previews p on p.tenant_id=r.tenant_id and p.run_id=r.id
             left join chat_output o on o.tenant_id=r.tenant_id and o.run_id=r.id and o.item='personal-text-preview-answer'
             where r.user_text=?"""


class Journey:
    def __init__(self, page, members, provider, proxy, login, logout, rows):
        self.proxy = proxy
        self.page, self.members, self.provider = page, members, provider
        self.login, self.logout, self.read = login, logout, rows
        self.member = members[0]
        self.entered = {0}
        self.events = provider.live.events
        self.outcomes = provider.live.outcomes
        self.cases = []
        self.sent = self.last_sent = self.last_receipt = 0
        self.last_run = self.last_thread = self.last_wire = None

    def button(self, label):
        return self.page.get_by_role("button", name=label, exact=True)

    def rows(self, sql, params=()):
        return self.read(self.member, sql, params)

    def memories(self):
        return self.rows(MEMORIES)

    def new_chat(self):
        self.button("새 대화").click()
        expect(self.page.get_by_role("textbox", name="메시지", exact=True)).to_have_value("")

    def switch(self, index):
        self.logout(self.page)
        if self.member != self.members[index]:
            self.provider.live.select_member(index)
        self.member = self.members[index]
        self.login(self.page, self.member, index not in self.entered)
        self.entered.add(index)

    def send(self, text):
        self.page.get_by_role("textbox", name="메시지", exact=True).fill(text)
        self.button("보내기").click()
        self.sent += 1

    def wait(self, probe, error, since=None, text=None, route=None):
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            if self.provider.claude_error:
                raise RuntimeError(self.provider.claude_error)
            if since is not None:
                for key, status, kind, view in self.outcomes[since:]:
                    if key == digest(text) and (status != 200 or kind != route):
                        raise AssertionError(f"{error} (HTTP {status}, route={kind}, view={view})")
            result = probe()
            if result:
                return result
            self.page.wait_for_timeout(160)
        raise AssertionError(error)

    def chat(self, text, memories=None):
        before = {row[0] for row in self.rows(ANSWERS, (text,))}
        receipt, since = len(self.events), len(self.outcomes)
        self.send(text)
        def completed():
            if self.rows("SELECT count(*) FROM member_work_monitors") != [(0,)]:
                raise AssertionError("ordinary chat unexpectedly created a recurring monitor")
            rows = [r for r in self.rows(ANSWERS, (text,)) if r[0] not in before and r[2] not in ("running", "queued")]
            if rows and rows[0][2] != "completed":
                raise AssertionError(f"ordinary reply state {rows[0][2]}")
            return rows[0] if rows and rows[0][3] else None
        run, thread, _, output = self.wait(completed, "ordinary answer missing", since, text, "chat")
        visible = self.page.locator(f"#member-turn-{run} .member-reply > p").last
        expect(visible).to_be_visible()
        assert visible.text_content() == output, "visible answer differs from SQLite"
        answers = [e for e in self.events[receipt:] if e["kind"] == "answer" and e["request"] == digest(text)]
        assert len(answers) == 1 and answers[0]["tokens"] > 0, "one verified real answer required"
        self.last_wire, self.last_run, self.last_thread = answers[0], run, thread
        if memories is not None:
            assert sorted(answers[0]["memories"]) == sorted(map(digest, memories)), "unexpected memories at model input"
            for original in memories:
                active = self.rows(MEMORIES)
                assert any(t == original and self.rows(
                    "select count(*) from agent_dependencies where run_id=? and kind='memory' and object_id=? and revision=?",
                    (run, mid, rev)) == [(1,)] for mid, rev, _, t in active), "answer lacks the current member memory version"
        return output

    def remember(self, text, expected_revision=1):
        since, receipt = len(self.outcomes), len(self.events)
        self.send(text)
        def saved():
            return next((r for r in self.rows(MEMORIES) if r[3] == text and r[1] == expected_revision), None)
        found = self.wait(saved, "standing preference missing or wrong revision", since, text, "preference")
        expect(self.page.get_by_role("status", name="기억 변경 결과", exact=True).filter(has_text=text).last).to_be_visible()
        assert sum(e["kind"] == "interpret" for e in self.events[receipt:]) == 1
        return found[2]

    def memory_detail(self, subject):
        self.button("기억").click()
        self.page.get_by_role("button").filter(has_text=subject).click()
        detail = self.page.get_by_role("dialog", name="기억 읽기", exact=True)
        expect(detail.get_by_role("heading", name=subject, exact=True)).to_be_visible()
        return detail

    def add_memory(self, subject, text):
        self.button("기억").click()
        self.button("기억 추가").click()
        self.page.get_by_label("기억 주제", exact=True).fill(subject)
        self.page.get_by_label("기억 내용", exact=True).fill(text)
        self.button("기억 저장").click()
        expect(self.page.get_by_role("heading", name=subject, exact=True)).to_be_visible()
        self.button("닫기").click()
        self.button("대화").click()
        assert any((s, t) == (subject, text) for _, _, s, t in self.rows(MEMORIES))

    def reopen(self, thread, run):
        title = self.rows("select title from threads where id=?", (thread,))[0][0].strip() or "제목 없는 대화"
        stored = self.rows("select text from chat_output where run_id=? and item='personal-text-preview-answer'", (run,))[0][0]
        count = len(self.events)
        self.page.reload()
        self.button("대화 목록").click()
        drawer = self.page.get_by_role("dialog", name="대화 목록", exact=True)
        drawer.get_by_role("button", name=title, exact=True).first.click()
        visible = self.page.locator(f"#member-turn-{run} .member-reply > p").last
        expect(visible).to_be_visible()
        assert visible.text_content() == stored and len(self.events) == count, "reopening changed the answer or used the model"

    def run_cases(self, cases):
        failures = []
        for code, exercise in cases:
            try:
                self.button("새 대화").click()
                exercise()
            except (AssertionError, PlaywrightTimeout) as exc:
                failures.append(code)
                print(f"FAIL {code} {str(exc).splitlines()[0] or type(exc).__name__}", flush=True)
                self.last_receipt, self.last_sent = len(self.events), self.sent
            else:
                self.case(code)
        assert not failures, "failed member scenarios " + ", ".join(failures)

    def case(self, code):
        events = self.events[self.last_receipt:]
        result = {"id": code, "messages": self.sent - self.last_sent,
                  "interpretations": sum(e["kind"] == "interpret" for e in events),
                  "answers": sum(e["kind"] == "answer" for e in events)}
        self.cases.append(result)
        self.last_receipt, self.last_sent = len(self.events), self.sent
        print("PASS " + json.dumps(result), flush=True)


def choice(text, wanted, opposite):
    normalized = re.sub(r"[\s*_`]", "", text)
    wanted = any(w in normalized for w in wanted)
    other = any(w in normalized for w in opposite)
    assert wanted and not other, "answer did not give the requested single choice"


COFFEE = "난 커피는 따뜻하게 마시는 게 좋아"
COFFEE_ASK = "카페에서 주문할 커피 온도는 뜨겁게랑 차갑게 중 뭐로 할까? 고른 온도만 한 단어로 답해줘."
HOT, COLD = ("따뜻", "뜨겁", "뜨거", "핫", "온커피", "핫커피"), ("차갑", "차가", "아이스", "시원", "냉")
MEETING_ASK = "다음 팀 회의 시간대로 오전과 오후 중 하나 골라줘. 고른 시간대만 한 단어로 답해줘."


def daily(j):
    j.remember(COFFEE)
    j.new_chat()
    choice(j.chat(COFFEE_ASK, [COFFEE]), HOT, COLD)
    j.case("DAY-01")
    choice(j.chat("오늘 한 잔만 아이스로 할래. 오늘 주문할 온도만 답해줘.", ()), COLD, HOT)
    choice(j.chat("그럼 다음 주에 주문할 커피 온도도 하나 골라줘. 온도만 말해줘.", [COFFEE]), HOT, COLD)
    assert [(r[1], r[3]) for r in j.rows(MEMORIES)] == [(1, COFFEE)]
    j.case("DAY-02")
    meeting = "저는 회의는 오전이 편해요."
    j.new_chat()
    j.remember(meeting)
    j.new_chat()
    choice(j.chat(MEETING_ASK, [meeting]), ("오전",), ("오후",))
    j.case("DAY-03")
    j.new_chat()
    choice(j.chat(COFFEE_ASK, [COFFEE]), HOT, COLD)
    j.case("DAY-04")
    corrected = "앞으로 커피는 차갑게 마시는 게 편해"
    j.new_chat()
    j.remember(corrected, 2)
    j.new_chat()
    choice(j.chat(COFFEE_ASK, [corrected]), COLD, HOT)
    assert len(j.rows(MEMORIES)) == 2
    j.case("DAY-05")


def one_time_choices(j):
    j.remember(COFFEE)
    saved = j.memories()

    def ask(text, expected_memories, wanted, other):
        choice(j.chat(text, expected_memories), wanted, other)
        assert j.memories() == saved, "this order changed the standing preference"

    def this_order():
        previous = j.chat(COFFEE_ASK, [COFFEE])
        ask("오늘 커피는 아이스로 할게, 이번 주문 온도만 한 단어로 답해줘.", (), COLD, HOT)
        assert digest(previous) not in j.last_wire["history"], "old preference answer entered this order"

    def opt_out():
        previous = j.chat(COFFEE_ASK, [COFFEE])
        ask("내 커피 취향은 쓰지 마. 이번 한 잔은 아이스로 정했어. 오늘 온도만 답해줘.", (), COLD, HOT)
        assert digest(previous) not in j.last_wire["history"], "opted-out preference entered answer history"

    def exclude_drink():
        ask("녹차 말고 커피로 할래. 내 평소 커피 취향에 맞게 온도만 한 단어로 추천해줘.", [COFFEE], HOT, COLD)
        j.new_chat()
        ask("내 커피 취향을 놓치지 말고 온도를 추천해줘. 온도만 한 단어로.", [COFFEE], HOT, COLD)

    j.run_cases([
        ("CHOICE-01", this_order),
        ("CHOICE-02", lambda: ask("이번 커피는 아이스로 할게. 평소 내가 고르는 커피 온도는 뭐였지? 평소 온도만 한 단어로.", [COFFEE], HOT, COLD)),
        ("CHOICE-03", lambda: ask("카페에서 커피를 주문하려고 해. 내 취향 잊지 말고 온도를 추천해줘. 한 단어만.", [COFFEE], HOT, COLD)),
        ("CHOICE-04", opt_out),
        ("CHOICE-05", exclude_drink),
    ])


def privacy(j):
    early, late = "나는 회의는 오전을 선호해.", "저는 회의는 오후가 편해요."
    j.remember(early)
    j.new_chat()
    choice(j.chat(MEETING_ASK, [early]), ("오전",), ("오후",))
    j.case("OWN-01")
    j.switch(1)
    assert not j.rows(MEMORIES)
    j.chat(MEETING_ASK, ())
    assert not j.rows("select object_id from agent_dependencies where kind='memory'")
    j.case("OWN-02")
    j.new_chat()
    j.remember(late)
    j.new_chat()
    choice(j.chat(MEETING_ASK, [late]), ("오후",), ("오전",))
    j.switch(0)
    j.new_chat()
    choice(j.chat(MEETING_ASK, [early]), ("오전",), ("오후",))
    j.case("OWN-03")
    before = j.rows(MEMORIES)
    j.new_chat()
    for text in ("친구는 회의가 늦을수록 좋대. 그냥 친구 얘기야.", "만약 내가 밤에 일한다면 오후 회의도 편하겠지?"):
        j.chat(text, ())
    assert j.rows(MEMORIES) == before
    j.case("OWN-04")
    j.new_chat()
    previous = j.chat(MEETING_ASK, [early])
    generic = j.chat("이번 답은 내 선호를 반영하지 말고 일반적으로만 답해줘. 회의 전에 확인할 준비물 두 가지는?", ())
    assert generic and digest(previous) not in j.last_wire["history"]
    assert not j.rows("select object_id from agent_dependencies where kind='memory' and run_id=?", (j.last_run,))
    j.case("OWN-05")


def library(j):
    subject = "주말 운동"
    before = "평소 운동은 수영을 선호해."
    after = "주말 운동은 배드민턴을 선호해."
    ask = "주말 운동은 수영과 배드민턴 중 하나만 추천해줘. 운동 이름만 말해줘."
    j.add_memory(subject, before)
    j.new_chat()
    choice(j.chat(ask, [before]), ("수영",), ("배드민턴",))
    j.case("LIB-01")
    j.memory_detail(subject).get_by_role("button", name="기억 수정").click()
    editor = j.page.get_by_role("dialog", name="기억 작성", exact=True)
    editor.get_by_role("textbox", name=re.compile("^기억 내용")).fill(after)
    j.button("기억 저장").click()
    expect(j.page.get_by_role("dialog", name="기억 읽기")).to_contain_text(after)
    j.button("닫기").click()
    j.button("대화").click()
    j.new_chat()
    choice(j.chat(ask, [after]), ("배드민턴",), ("수영",))
    assert [(r[1], r[3]) for r in j.rows(MEMORIES)] == [(2, after)]
    j.case("LIB-02")
    def confirm():
        j.memory_detail(subject).get_by_role("button", name="기억 잊기").click()
        return j.page.get_by_role("dialog", name="기억 잊기", exact=True)
    confirm().get_by_role("button", name="취소", exact=True).click()
    detail = j.page.get_by_role("dialog", name="기억 읽기", exact=True)
    expect(detail).to_contain_text(after)
    detail.get_by_role("button", name="닫기", exact=True).click()
    j.button("대화").click()
    j.new_chat()
    choice(j.chat(ask, [after]), ("배드민턴",), ("수영",))
    j.case("LIB-03")
    confirm().get_by_role("button", name="기억과 이력 지우기").click()
    expect(j.page.get_by_text("아직 기록한 기억이 없습니다.", exact=True)).to_be_visible()
    j.button("대화").click()
    j.switch(0)
    j.new_chat()
    j.chat(ask, ())
    assert not j.rows(MEMORIES) and j.rows("select count(*) from knowledge_memory_versions") == [(0,)]
    j.case("LIB-04")



def boundaries(j):
    # An unpunctuated question and conditional advice must not become standing preferences.
    j.chat("커피는 어떤 걸 고르는 게 좋아", ())
    j.new_chat()
    j.chat("회의는 늦으면 먼저 시작하는 게 좋아", ())
    assert not j.memories()
    j.case("CHK-01")

    j.new_chat()
    answer = j.chat("안내 문서가 변경되면 무엇을 확인해야 하는지 알려줘. 수정일과 작성자를 한 문장으로 짚어줘.", ())
    assert "수정" in answer and "작성" in answer, "the explanation omitted a requested check"
    assert not j.rows("SELECT id FROM member_work_monitors")
    j.case("CHK-02")

    j.new_chat()
    with j.page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith("/personal/tasks/monitors")) as made:
        j.send("기억에서 ‘준비물’이 바뀌면 바로 알려줘.")
    assert made.value.ok
    mid = made.value.json()["id"]
    assert j.rows("SELECT id,state,every_seconds,days FROM member_work_monitors") == [(mid, "active", 3600, 7)]
    j.page.reload()
    j.send("내 감시 목록 보여줘")
    board = j.page.get_by_role("region", name=re.compile("감시|브리핑"))
    expect(board.get_by_text("준비물 변화 감시", exact=True)).to_be_visible()
    with j.page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith(f"/{mid}/stop")) as stopped:
        j.send("‘준비물 변화 감시’ 감시 그만해줘")
    assert stopped.value.ok
    assert j.rows("SELECT id,state,next_due FROM member_work_monitors") == [(mid, "stopped", None)]
    j.case("CHK-03")

# One registry for the CLI choices, implementation and expected completed cases.
SUITES = {
    "groceries": ("member_live_grocery", "exercise", 8), "timetable": ("member_live_timetable", "exercise", 6),
    "daily": (__name__, "daily", 5), "continuity": ("member_live_trip", "exercise", 4),
    "privacy": (__name__, "privacy", 5), "library": (__name__, "library", 4),
    "work": ("member_live_work", "exercise", 4), "boundaries": (__name__, "boundaries", 3),
    "routing": ("member_live_composer", "routing", 8), "timing": ("member_live_timing", "exercise", 10),
    "choices": (__name__, "one_time_choices", 5),
    "acknowledgement": ("member_live_ack", "exercise", 5), "drafts": ("member_live_tabs", "exercise", 4),
}


def exercise(group, page, members, origin, provider, proxy, *, login, logout, rows):
    page.goto(origin)
    login(page, members[0], True)
    j = Journey(page, members, provider, proxy, login, logout, rows)
    module, name, total = SUITES[group]
    run = getattr(importlib.import_module(module), name)
    try:
        run(j)
    finally:
        print("JOURNEY " + json.dumps({"group": group, "model": claude_cli.MODEL, "effort": claude_cli.EFFORT,
              "passed": len(j.cases), "planned": total, "messages": j.sent, "cases": j.cases,
              "interpretations": sum(e["kind"] == "interpret" for e in j.events),
              "answers": sum(e["kind"] == "answer" for e in j.events)}, ensure_ascii=False), flush=True)
    assert len({c["id"] for c in j.cases}) == total
    assert len(j.events) == provider.calls + provider.interpretations, "model call without a verified receipt"
