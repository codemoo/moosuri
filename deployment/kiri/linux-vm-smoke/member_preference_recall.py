"""Check nine preference-recall scenarios through the Kiri member composer on disposable accounts.

Run member-web-fullstack.py with --preference-recall. Add --claude to use the pinned model
for classification and answers. The nine greetings used to fill the history window have fixed
replies in both modes. Compare each posted answer and the memory sent to the model with that
member's SQLite rows. Report case results and counts.
"""
import json
import re
import time

from playwright.sync_api import expect
import claude_cli

PREF = "기차는 통로 쪽이 편해"
ASK = "부산행 좌석 하나 추천해줘"
OVERRIDE = "이번엔 창가로"
NEXT_TRIP = "다음 주에 탈 기차 좌석도 하나 추천해줘"
UNRELATED = "파스타 면은 몇 분 삶아야 해?"
CORRECT = "기차는 이제 창가 쪽이 편해"
FORGET = "기차 기억 지워 줘"
SUBJECT = "기차"
GREETINGS = ("안녕", "안녕하세요", "고마워", "고마워요", "감사합니다", "네", "좋아", "hello", "hi")
EMPTY_MEMORIES = "아직 기록한 기억이 없습니다."
RECENT_MESSAGES = 16  # eight ordinary exchanges

MEMORIES = ("select json_extract(v.draft,'$.subject'), json_extract(v.draft,'$.text'), m.revision "
            "from knowledge_memories m join knowledge_memory_versions v "
            "on v.tenant_id=m.tenant_id and v.memory_id=m.id and v.revision=m.revision "
            "where m.status='active' order by 1")


def recommended_seat(text):
    # Score the primary choice. A later comparison can legitimately name the other seat.
    for sentence in re.split(r"[.!?。\n]+", text):
        aisle = "통로" in sentence or "복도" in sentence
        window = "창가" in sentence or "창문" in sentence
        if aisle or window:
            if aisle and window or any(phrase in sentence for phrase in ("비추천", "추천하지", "피하세요")):
                return None
            return "aisle" if aisle else "window"
    return None


def aisle(text):
    return recommended_seat(text) == "aisle"


def window(text):
    return recommended_seat(text) == "window"


class Recall:
    """Mode-scoped wire checks for the fixture provider. Reports reasons and counts, never text."""

    def __init__(self, real):
        self.real = real
        self.expect = None
        self.error = None
        self.texts = set()
        self.stats = {"matched": 0, "literal": 0, "trimmed": 0}

    def knows(self, text):
        return text in self.texts

    def fail(self, reason):
        self.error = self.error or reason
        return False

    def chat(self, messages):
        """None: the real model answers. str: the synthetic answer. False: wire mismatch (HTTP 400)."""
        exp = self.expect
        if exp is None:
            return self.fail("unexpected text-model call")
        roles = [role for role, _ in messages[1:]]
        if messages[-1][0] != "user" or any(role not in ("user", "assistant") for role in roles):
            return self.fail("message roles")
        history, last = messages[1:-1], messages[-1][1]
        want = exp["history"]
        if len(history) > RECENT_MESSAGES or len(history) % 2:
            return self.fail("history is not at most eight whole exchanges")
        if history != want[-RECENT_MESSAGES:]:
            return self.fail("older history is not the literal recent tail")
        if len(history) < len(want):
            self.stats["trimmed"] += 1
        if exp["memories"] is None:
            if last != exp["text"]:
                return self.fail("expected the literal member text")
            self.stats["literal"] += 1
        else:
            try:
                body = json.loads(last)
            except ValueError:
                return self.fail("expected the encoded recall request, got literal text")
            if not isinstance(body, dict) or set(body) != {"current_user_request", "member_memories"}:
                return self.fail("recall request keys")
            entries = body["member_memories"]
            if (body["current_user_request"] != exp["text"] or not isinstance(entries, list)
                    or any(not isinstance(e, dict) or set(e) != {"subject", "text"} for e in entries)
                    or sorted((e["subject"], e["text"]) for e in entries) != sorted(exp["memories"])):
                return self.fail("recall request does not carry exactly the stored memories")
            self.stats["matched"] += 1
        return exp["answer"]


def exercise(page, members, origin, provider, *, login, logout, rows, passed):
    recall, real = provider.recall, provider.recall.real
    first, second = members
    tally = {"pass": 0, "skip": 0}
    outside = []
    page.on("request", lambda r: outside.append(1) if not r.url.startswith(origin + "/") else None)

    def done(case):
        tally["pass"] += 1
        passed(f"{case} [{claude_cli.MODEL} / {claude_cli.EFFORT}]" if real else case)

    texts = {PREF, ASK, OVERRIDE, NEXT_TRIP, UNRELATED, CORRECT, FORGET, *GREETINGS}
    recall.texts = texts
    if not real:
        pref = lambda op, value=None: {"intent": "preference", "preference": dict(
            {"operation": op, "subject": SUBJECT, "topic": "좌석", "lasting": "stated"}, **({"value": value} if value else {}))}
        provider.work_intents = {
            PREF: pref("create", PREF), CORRECT: pref("edit", CORRECT), FORGET: {"intent": "preference", "preference": {
                "operation": "forget", "subject": SUBJECT}},
            ASK: {"intent": "chat", "recall": ["좌석", "복도", "창가"]}, OVERRIDE: {"intent": "chat"},
            NEXT_TRIP: {"intent": "chat", "recall": ["기차", "좌석"]},
            UNRELATED: {"intent": "chat", "recall": ["면"]},
        }

    page.goto(origin)
    login(page, first, True)
    account_ok = rows(first, "select count(*) from accounts where status='active'")
    assert account_ok == [(1,)]
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    expect(editor).to_be_enabled()
    history = []

    def button(label):
        return page.get_by_role("button", name=label, exact=True)

    def receipt():
        return page.get_by_role("status", name="기억 변경 결과", exact=True)

    def wait_for_memory(expected):
        until = time.monotonic() + 100
        while time.monotonic() < until:
            if provider.claude_error or recall.error:
                raise AssertionError(provider.claude_error or recall.error)
            if receipt().is_visible():
                assert expected in receipt().inner_text(), "the visible memory receipt did not confirm the requested change"
                return
            page.wait_for_timeout(150)
        raise AssertionError("no memory receipt appeared")

    def new_chat():
        button("새 대화").click()
        expect(editor).to_have_value("")
        history.clear()

    def memories():
        return rows(first, MEMORIES)

    def wire(found=None):
        return [(s, t) for s, t, _ in (memories() if found is None else found)]

    def counts():
        return (rows(first, "select count(*) from personal_text_previews")[0][0], provider.calls, provider.interpretations)

    def quiet(active):
        """No work, no browser job and exactly the expected active memories."""
        assert rows(first, "select count(*) from member_conversation_tasks") == [(0,)]
        assert rows(first, "select count(*) from personal_browser_jobs") == [(0,)]
        assert rows(first, "select count(*) from knowledge_memories where status='active'") == [(active,)]

    def wait_reply(text, before):
        until = time.monotonic() + 100
        while time.monotonic() < until:
            if provider.claude_error:
                raise RuntimeError(provider.claude_error)
            if recall.error:
                raise AssertionError("wire mismatch: " + recall.error)
            found = [r for r in rows(first,
                "SELECT r.id, r.state, o.text FROM chat_runs r LEFT JOIN chat_output o "
                "ON o.tenant_id=r.tenant_id AND o.run_id=r.id AND o.item='personal-text-preview-answer' "
                "WHERE r.user_text=? ORDER BY o.ordinal DESC", (text,)) if r[0] not in before]
            if found and found[0][1] not in ("running", "queued"):
                return found[0]
            page.wait_for_timeout(120)
        raise RuntimeError("Reply deadline exceeded.")

    def turn(text, stored, answer, classified, score=None, fixed=False):
        """One ordinary composer turn. stored: None (literal wire) or the memories the wire must carry."""
        before = {r[0] for r in rows(first, "select id from chat_runs where user_text=?", (text,))}
        runs, calls, classifications = counts()
        recall.expect = {"text": text, "memories": stored, "history": list(history),
                         "answer": answer if fixed or not real else None}
        editor.fill(text)
        expect(button("보내기")).to_be_enabled()
        button("보내기").click()
        run, state, output = wait_reply(text, before)
        recall.expect = None
        assert recall.error is None, recall.error
        assert state == "completed" and output, f"member state {state}"
        assert counts() == (runs + 1, calls + 1, classifications + classified), (counts(), (runs + 1, calls + 1, classifications + classified))
        reply = page.locator(f"#member-turn-{run} .member-reply > p").last
        expect(reply).to_be_visible()
        shown = reply.text_content()
        assert shown == output, "displayed answer differs from the stored answer"
        if score and not score(shown):
            raise AssertionError("visible answer failed the scenario score")
        history.extend([("user", text), ("assistant", output)])
        return run, shown

    # ------------------------------------------------------------------ REC-01 baseline
    turn(ASK, None, "원하시는 좌석 유형을 알려 주세요.", 1)
    assert memories() == []
    quiet(0)
    done("REC-01 baseline: without a memory the model sees the literal question; no memory, no work")

    # ------------------------------------------------------------------ REC-02 stated preference
    new_chat()
    runs, calls, classifications = counts()
    recall.expect = None
    editor.fill(PREF)
    expect(button("보내기")).to_be_enabled()
    button("보내기").click()
    wait_for_memory("통로")
    stored = rows(first, MEMORIES)
    assert len(stored) == 1 and stored[0][2] == 1, "one memory at revision 1"
    subject, value, _ = stored[0]
    assert subject and 0 < len(value.encode()) <= 8192
    if real:
        assert value in PREF and "통로" in value, "the stored value comes from the statement"
    else:
        assert (subject, value) == (SUBJECT, PREF)
    assert rows(first, "select count(*) from knowledge_memory_versions") == [(1,)]
    assert counts() == (runs, calls, classifications + 1) and recall.error is None, "no chat call, one classification"
    quiet(1)
    button("기억").click()
    page.get_by_role("button").filter(has_text=subject).click()
    expect(page.get_by_role("heading", name=subject)).to_be_visible()
    expect(button("기억 수정")).to_be_visible()
    button("닫기").click()
    button("대화").click()
    done("REC-02 stated preference: one revision-1 memory, editable receipt, no chat call, no work")

    # ------------------------------------------------------------------ REC-06 beyond eight exchanges
    for greeting in GREETINGS:
        turn(greeting, None, f"{greeting} 응답이에요.", 0, fixed=True)
    assert len(history) == 18
    before = recall.stats["trimmed"]
    turn(ASK, wire(), "통로 쪽 좌석을 추천해요.", 1, aisle)
    assert recall.stats["trimmed"] == before + 1, "older exchanges were trimmed from the model request"
    original = rows(first, "SELECT thread_id FROM chat_runs WHERE user_text=?", (PREF,))[0][0]
    assert rows(first, "SELECT count(*) FROM chat_runs WHERE thread_id=? AND user_text=?", (original, ASK)) == [(1,)]
    quiet(1)
    done("REC-06 same thread beyond eight exchanges: oldest trimmed, history literal, memory still recalled")

    # ------------------------------------------------------------------ REC-03 recall in a new chat
    new_chat()
    run, shown = turn(ASK, wire(), "통로 쪽 좌석을 추천해요.", 1, aisle)
    seen = counts()
    page.reload()
    expect(editor).to_be_visible()
    again = page.locator(f"#member-turn-{run} .member-reply > p").last
    expect(again).to_be_visible()
    assert again.text_content() == shown and counts() == seen, "reload leaves the answer and call counts unchanged"
    assert rows(first, MEMORIES) == stored
    assert rows(first, "select count(*) from knowledge_memory_versions") == [(1,)]
    quiet(1)
    done("REC-03 new chat: wire carries the memory, visible answer is aisle, reload leaves state and calls unchanged")

    # Keep the same thread after the aisle recommendation: the short reply relies on it.
    turn(OVERRIDE, None, "이번에는 창가 쪽 좌석을 추천해요.", 1, window)
    assert rows(first, MEMORIES) == stored
    assert rows(first, "select count(*) from knowledge_memory_versions") == [(1,)]
    quiet(1)
    turn(NEXT_TRIP, wire(), "다음 여행에는 통로 쪽 좌석을 추천해요.", 1, aisle)
    assert rows(first, MEMORIES) == stored
    quiet(1)
    done("REC-04 short follow-up selects window for this trip; the same thread recalls aisle for the next trip")

    # ------------------------------------------------------------------ REC-05 greeting
    new_chat()
    turn("안녕하세요", None, "안녕하세요.", 0)
    quiet(1)
    done("REC-05 greeting: no classifier call, literal wire, no read")

    # ------------------------------------------------------------------ REC-07 unrelated
    new_chat()
    turn(UNRELATED, None, "보통 8분 정도 삶아요.", 1)
    assert rows(first, MEMORIES) == stored
    quiet(1)
    done("REC-07 unrelated question reaches the model without the seat preference")

    # ------------------------------------------------------------------ REC-08 another member
    new_chat()
    page.reload()
    expect(editor).to_be_visible()
    logout(page)
    login(page, second, True)
    button("기억").click()
    expect(page.get_by_text(EMPTY_MEMORIES, exact=True)).to_be_visible()
    assert rows(second, "select count(*) from knowledge_memories") == [(0,)]
    assert rows(second, "select count(*) from chat_runs") == [(0,)]
    button("대화").click()
    logout(page)
    login(page, first, False)
    button("기억").click()
    expect(page.get_by_role("button").filter(has_text=subject)).to_have_count(1)
    button("대화").click()
    assert rows(first, MEMORIES) == stored and recall.error is None
    done("REC-08 another isolated member has no memory or history; the first member's memory is intact")

    # ------------------------------------------------------------------ REC-09 correction and forget
    new_chat()
    runs, calls, classifications = counts()
    editor.fill(CORRECT)
    button("보내기").click()
    wait_for_memory("창가")
    revised = rows(first, MEMORIES)
    assert revised == [(subject, CORRECT, 2)]
    assert counts() == (runs, calls, classifications + 1)
    new_chat()
    turn(ASK, wire(revised), "창가 쪽 좌석을 추천해요.", 1, window)
    assert "통로" not in json.dumps(revised, ensure_ascii=False)
    quiet(1)
    new_chat()
    runs, calls, classifications = counts()
    editor.fill(FORGET)
    button("보내기").click()
    wait_for_memory("이 기억은 잊었어요")
    assert rows(first, MEMORIES) == [] and rows(first, "select count(*) from knowledge_memory_versions") == [(0,)]
    assert counts() == (runs, calls, classifications + 1)
    new_chat()
    turn(ASK, None, "원하시는 좌석 유형을 알려 주세요.", 1)
    quiet(0)
    done("REC-09 correction recalls only the new text; forget leaves nothing to recall")

    assert not outside, "no request left the private origin"
    assert recall.error is None
    runs, calls, classifications = counts()
    print("AGGREGATE " + json.dumps({
        "mode": f"{claude_cli.MODEL}/{claude_cli.EFFORT}" if real else "synthetic",
        "passed": tally["pass"], "skipped": tally["skip"], "classifier_calls": classifications,
        "chat_calls": calls, "chat_runs": runs, "wire_matched": recall.stats["matched"],
        "wire_literal": recall.stats["literal"], "history_trimmed": recall.stats["trimmed"]}), flush=True)
