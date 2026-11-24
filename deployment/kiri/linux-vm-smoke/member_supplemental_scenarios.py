"""Supplemental Kiri acceptance: the ordinary composer's long-term preferences and durable local state.

Invoked by member-web-fullstack.py with ``--supplemental-work`` (needs no model login, no broker and no
Google grant). Everything is synthetic: two isolated member volumes (only the first is exercised), the
gateway, and the deterministic provider fixture. ``provider.work_intents`` supplies the *bounded* classifier
decisions that the Rust protocol accepts (crates/moosuri-server/src/personal_text/interpret.rs and
preferences.rs), so these cases isolate the member runtime's deterministic gates, SQLite effects and replay
projections from model quality. Live Claude is tested separately (``--claude``).

Contracts checked on every call (crates/moosuri-core/src/member_interpret.rs):
  request  {request_id, text, [group_id], [read_only]}; request_id/group_id canonical lowercase UUID.
  reply    chat {kind}; clarify {kind,message}; work {kind,group,focus,task_id}; preference {kind,thread_id,view}
  focus    tasks | status | evidence | report; task_id is always present (null or a task of the group)
  view     remembered{operation create|edit,memory_id,revision,current_revision,subject,text,superseded}
           withdrawn{memory_id}; no_change{reason,candidates<=5{memory_id,revision,subject}}; stale{}
  reason   one_time negated not_a_request needs_literal no_match stale ambiguous exists unchanged

Cases (each prints its own PASS line):
  SUP-01 ordinary chat still reaches the text preview after the classifier says "chat"
  SUP-02 remember: one durable memory, typed receipt, UI receipt, exact replay with no second write/model call
  SUP-03 only-this-work exception: exclusion is stored on that task alone; memory and another group untouched
  SUP-04 one-time preference (member's words and classifier flag) changes no memory
  SUP-05a..d negated / hypothetical / reported / invented requests change nothing
  SUP-06a..b ambiguous and duplicate references change nothing and name the candidates
  SUP-07 update: new revision; older receipts show current text; stale excerpts, report and worklog hide old text
  SUP-08 forget: nothing readable remains anywhere; every old receipt is withdrawn; repeating it is a no-op
  SUP-09 shopping and application outcomes made from the composer with no browser seed, replayed exactly once
  SUP-10 task list and worklog navigation after those replays (no duplicates, ids match SQLite, round trip)

Budget: 17 classifier calls plus 1 chat call, well under the 32-call member reservation of the semantic suite.
Only aggregate and synthetic identifiers are printed; member text is never printed.
"""
import json
import re
import time
from urllib.parse import urlparse

from playwright.sync_api import expect
from member_conversation_scenarios import api

UUID = r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
THREAD_PATH = re.compile(r".*/threads/(" + UUID + r")/work-interpret")
REQUEST_KEYS = {"request_id", "text", "group_id", "read_only"}
FOCUSES = {"tasks", "status", "evidence", "report"}
REASONS = {"one_time", "negated", "not_a_request", "needs_literal", "no_match", "stale", "ambiguous", "exists", "unchanged"}
OUTCOME_KEYS = {"request_id", "kind", "message", "criteria", "browser_runs", "mail_reads"}
STALE_NOTICE = "더 이상 볼 수 없습니다"

# Synthetic member data. The two tokens let every check prove where a stale excerpt would have appeared.
SUBJECT, OTHER = "여행 준비물", "여행 숙소"
OLD, NEW = "SUPP-4417", "SUPP-8821"
VALUE1 = f"충전 어댑터와 회색 외투 그리고 확인 표식 {OLD}"
VALUE2 = f"보조 배터리와 여권 사본 그리고 확인 표식 {NEW}"
VALUE_OTHER = "호텔보다 한옥"
TITLE_A, TITLE_B = "출장 준비물 정리", "여행 준비물 확인"

CHAT, ANSWER = "오늘 컨디션 어때? 한마디로 답해 줘.", "좋아요"
CREATE = f"{SUBJECT}: {VALUE1}. 기억해 줘"
SECOND = f"{OTHER}: {VALUE_OTHER}. 기억해 줘"
EDIT = f"{SUBJECT}을 ‘{VALUE2}’로 바꿔 줘"
FORGET = f"{SUBJECT} 기억 지워 줘"
FORGET_AGAIN = f"{SUBJECT} 기억을 다시 지워 줘"
ONE1 = f"이번 출장만 {SUBJECT}을 ‘여권만’으로 바꿔 줘"
ONE2 = f"{SUBJECT}을 그 여행에서만 ‘여권만’으로 바꿔 줘"
NEG = f"{SUBJECT} 기억을 지우지 마"
INFO = f"{SUBJECT} 기억을 지우면 어떻게 돼?"
REPORTED = f"친구가 ‘{SUBJECT} 기억 지워 줘’라고 말했어"
INVENTED = f"{SUBJECT}을 바꿔 줘"
AMBIG = "여행 기억 지워 줘"
EXISTS = f"{SUBJECT}: 다른 내용 적어 둘게. 기억해 줘"
GROUP_A = "다음 출장 때 챙길 것을 한 장으로 정리해서 저장해 줘."
GROUP_B = "다음 출장 때 챙길 것을 내 기억에서 확인해 줘."
EXCLUDE = "이번 조사에서만 어댑터는 빼 줘."
SHOP = "이번 달 안에 도착하는 노트 가격과 배송비를 비교해 줘"
APPLY = "지원금 공고 지원서 초안을 준비해 줘"

MEMORIES = ("select json_extract(v.draft,'$.subject'), json_extract(v.draft,'$.text'), m.revision "
            "from knowledge_memories m join knowledge_memory_versions v "
            "on v.tenant_id=m.tenant_id and v.memory_id=m.id and v.revision=m.revision "
            "where m.status='active' order by 1")

# What the composer shows for each no_change reason (web/src/MemberMemoryReceipt.tsx).
UI_REASON = {"one_time": "이번 한 번만", "negated": "하지 말라는 뜻", "not_a_request": "기억을 바꾸라는 요청이 아니라서",
             "needs_literal": "기억할 이름과 내용을 직접 적어", "no_match": "같은 이름의 기억을 찾지 못했어요",
             "ambiguous": "하나로 정할 수 없어요", "exists": "같은 이름의 기억이 이미 있어요"}


def canonical(value):
    return isinstance(value, str) and re.fullmatch(UUID, value) is not None and value.strip("0-") != ""


def pref(operation, subject=None, value=None, lasting=None):
    """The closed classifier object: {intent:"preference", preference:{operation,subject,value,lasting}}."""
    body = {"operation": operation}
    for key, item in (("subject", subject), ("value", value), ("lasting", lasting)):
        if item is not None:
            body[key] = item
    return {"intent": "preference", "preference": body}


def check_view(view):
    kind = view["kind"]
    if kind == "remembered":
        assert set(view) == {"kind", "operation", "memory_id", "revision", "current_revision", "subject", "text",
                             "superseded"}, "remembered keys"
        assert view["operation"] in ("create", "edit") and canonical(view["memory_id"])
        assert 0 < view["revision"] <= view["current_revision"] and isinstance(view["superseded"], bool)
        assert view["superseded"] == (view["revision"] != view["current_revision"])
        assert view["subject"] and len(view["subject"].encode()) <= 512 and len(view["text"].encode()) <= 8192
    elif kind == "withdrawn":
        assert set(view) == {"kind", "memory_id"} and canonical(view["memory_id"]), "withdrawn keys"
    elif kind == "no_change":
        assert set(view) == {"kind", "reason", "candidates"}, "no_change keys"
        assert view["reason"] in REASONS and len(view["candidates"]) <= 5, view["reason"]
        for item in view["candidates"]:
            assert set(item) == {"memory_id", "revision", "subject"} and canonical(item["memory_id"]) and item["revision"] > 0
    else:
        assert view == {"kind": "stale"}, "unknown preference view kind"


def check_reply(reply, thread):
    kind = reply["kind"]
    if kind == "chat":
        assert reply == {"kind": "chat"}
    elif kind == "clarify":
        assert set(reply) == {"kind", "message"} and reply["message"].strip() and len(reply["message"]) <= 600
    elif kind == "preference":
        assert set(reply) == {"kind", "thread_id", "view"} and reply["thread_id"] == thread, "preference keys or thread"
        check_view(reply["view"])
    elif kind == "work":
        assert set(reply) == {"kind", "group", "focus", "task_id"}, "work keys (task_id is always serialized)"
        group = reply["group"]
        assert reply["focus"] in FOCUSES and group["thread_id"] == thread and group["tasks"]
        assert reply["task_id"] is None or reply["task_id"] in [task["id"] for task in group["tasks"]]
    else:
        raise AssertionError("unknown work-interpret reply kind")


def exercise(page, members, origin, provider, *, login, rows, passed):
    member = members[0]
    outside = []
    page.on("request", lambda r: outside.append(r.url) if not r.url.startswith(origin + "/") else None)
    provider.turns = [([("user", CHAT)], ANSWER)]
    provider.work_intents = {
        CREATE: pref("create", SUBJECT, VALUE1), SECOND: pref("create", OTHER, VALUE_OTHER),
        EXISTS: pref("create", SUBJECT, "다른 내용 적어 둘게"),
        EDIT: pref("edit", SUBJECT, VALUE2), INVENTED: pref("edit", SUBJECT, "땅콩 알레르기"),
        ONE1: pref("edit", SUBJECT, "여권만", "persistent"),  # the member's own words mark it one-time
        ONE2: pref("edit", SUBJECT, "여권만", "one_time"),     # only the classifier flag marks it one-time
        FORGET: pref("forget", SUBJECT), FORGET_AGAIN: pref("forget", SUBJECT), NEG: pref("forget", SUBJECT),
        INFO: pref("forget", SUBJECT), REPORTED: pref("forget", SUBJECT), AMBIG: pref("forget", "여행"),
        GROUP_A: {"intent": "create", "report": "combined",
                  "items": [{"title": TITLE_A, "scope": "memory", "query": "준비물"}]},
        GROUP_B: {"intent": "create", "items": [{"title": TITLE_B, "scope": "memory", "query": "준비물"}]},
        EXCLUDE: {"intent": "exclude", "target": 1, "text": "어댑터"},
    }
    page.goto(origin)
    login(page, member, True)
    account = rows(member, "select id from accounts where status='active'")[0][0]
    editor = page.get_by_role("textbox", name="메시지", exact=True)

    def button(label):
        return page.get_by_role("button", name=label, exact=True)

    def region(label):
        return page.get_by_role("region", name=label, exact=True)

    def receipt():
        return page.get_by_role("status", name="기억 변경 결과", exact=True)

    def send(text):
        editor.fill(text)
        button("보내기").click()

    def wait_rows(sql, test, params=()):
        until, last = time.monotonic() + 40, None
        while time.monotonic() < until:
            last = rows(member, sql, params)
            if test(last):
                return last
            page.wait_for_timeout(125)
        raise AssertionError(f"SQLite condition did not settle ({len(last or [])} rows): {sql}")

    def interpret(text, expected):
        def mine(r):
            if r.request.method != "POST" or not r.url.endswith("/work-interpret"):
                return False
            try:  # a late read-only reopen of an earlier receipt must never stand in for this request
                body = r.request.post_data_json
            except Exception:
                return False
            return isinstance(body, dict) and body.get("text") == text and not body.get("read_only")
        with page.expect_response(mine, timeout=90000) as pending:
            send(text)
        response = pending.value
        assert response.ok, (response.status, response.status_text)
        request, path = response.request.post_data_json, response.url.removeprefix(origin)
        match = THREAD_PATH.fullmatch(path)
        assert match, "unexpected work-interpret route"
        thread = match.group(1)
        assert {"request_id", "text"} <= set(request) <= REQUEST_KEYS and request["text"] == text
        assert canonical(request["request_id"]) and (request.get("group_id") is None or canonical(request["group_id"]))
        assert not request.get("read_only") and text.strip() and len(text.encode()) <= 8192
        # The navigator can replace a completed response while entering its new thread.
        # Always inspect the committed receipt, exactly as a member does after a reload.
        code, reply = api(page, path, account, {**request, "read_only": True})
        assert code == 200, ("missing committed receipt", code)
        check_reply(reply, thread)
        assert reply["kind"] == expected, (reply["kind"], expected)
        return reply, request, path, thread

    def replay(path, request):
        before = provider.interpretations
        code, view = api(page, path, account, {**request, "read_only": True})
        assert code == 200 and provider.interpretations == before, ("read-only replay", code)
        return view

    def snapshot():
        return (rows(member, "select id,revision,status from knowledge_memories order by id"),
                rows(member, "select memory_id,revision,status,draft from knowledge_memory_versions "
                             "order by memory_id,revision"))

    def settled():
        return rows(member, "select count(*) from member_conversation_interpretations where outcome is not null")[0][0]

    def decide(text, expected):
        """One classifier call, one durable receipt; returns the checked reply."""
        calls, done = provider.interpretations, settled()
        result = interpret(text, expected)
        assert provider.interpretations == calls + 1 and settled() == done + 1, "one classification, one receipt"
        assert rows(member, "select count(*) from member_conversation_interpretations where outcome is null") == [(0,)]
        return result

    def refuse(text, reason, names=()):
        """Non-mutating preference request: nothing in SQLite changes, the reason is the typed one."""
        before = snapshot()
        reply, request, path, thread = decide(text, "preference")
        view = reply["view"]
        assert view["kind"] == "no_change" and view["reason"] == reason, view.get("reason", view["kind"])
        assert sorted(item["subject"] for item in view["candidates"]) == sorted(names), reason
        assert snapshot() == before, reason
        expect(receipt()).to_contain_text(UI_REASON[reason])
        if names:
            expect(page.get_by_role("list", name="해당할 수 있는 기억").get_by_role("listitem")).to_have_count(len(names))
        assert replay(path, request) == reply
        return reply, thread

    def leaked(token):
        pattern = f"%{token}%"
        return rows(member, "select (select count(*) from member_conversation_interpretations "
                            "where coalesce(plan,'') like ?1 or coalesce(outcome,'') like ?1)"
                            " + (select count(*) from member_conversation_events where text like ?1)", (pattern,))[0][0]

    # ------------------------------------------------------------------ SUP-01 ordinary chat
    reply, _, _, thread1 = decide(CHAT, "chat")
    sql = ("SELECT r.user_text,o.text,r.state FROM chat_runs r JOIN chat_output o "
           "ON o.run_id=r.id AND o.tenant_id=r.tenant_id ORDER BY r.sequence")
    wait_rows(sql, lambda found: found == [(CHAT, ANSWER, "completed")])
    assert provider.calls == 1 and rows(member, "select count(*) from knowledge_memories") == [(0,)]
    passed("SUP-01 ordinary chat: classifier said chat, one committed reply, no memory or work created")

    # ------------------------------------------------------------------ SUP-02 remember
    reply1, request1, path1, thread = decide(CREATE, "preference")
    assert thread == thread1
    view = reply1["view"]
    assert (view["kind"], view["operation"], view["revision"], view["current_revision"], view["superseded"]) == \
        ("remembered", "create", 1, 1, False)
    assert (view["subject"], view["text"]) == (SUBJECT, VALUE1)
    memory = view["memory_id"]
    assert rows(member, MEMORIES) == [(SUBJECT, VALUE1, 1)]
    assert rows(member, "select count(*) from knowledge_memories where id=?", (memory,)) == [(1,)]
    expect(receipt()).to_contain_text(VALUE1)
    assert replay(path1, request1) == reply1
    calls = provider.interpretations
    code, again = api(page, path1, account, request1)  # a lost acknowledgement: the very same request
    assert code == 200 and again == reply1 and provider.interpretations == calls
    assert rows(member, MEMORIES) == [(SUBJECT, VALUE1, 1)] and leaked(OLD) == 0
    receipt().get_by_role("button", name="기억 목록 보기").click()
    expect(page.get_by_role("button").filter(has_text=SUBJECT)).to_be_visible()
    button("대화").click()
    passed("SUP-02 remember: one memory at revision 1, receipt shown, exact replay writes nothing and skips the model")

    # Group A: a saved combined report that reads the memory, kept as the stale-excerpt witness.
    reply, request_a, path_a, thread = decide(GROUP_A, "work")
    group_a = reply["group"]
    assert thread == thread1 and reply["focus"] == "report" and reply["task_id"] is None and group_a["report_requested"]
    assert [(t["scope"], t["query"]) for t in group_a["tasks"]] == [("memory", "준비물")]
    gid_a, task_a = group_a["id"], group_a["tasks"][0]["id"]
    wait_rows("select state from member_conversation_tasks where group_id=?", lambda found: found == [("completed",)], (gid_a,))
    ready = wait_rows("select id from member_conversation_group_reports where group_id=? and state='ready' and superseded=0",
                      lambda found: len(found) == 1, (gid_a,))
    report_id = ready[0][0]
    report_path = f"/api/v1/personal/tasks/conversations/{thread1}/reports/{report_id}"
    detail_path = f"/api/v1/personal/tasks/{task_a}"
    first = replay(path_a, request_a)
    assert first["group"]["group_report_id"] == report_id and first["group"]["tasks"][0]["source_count"] == 1
    assert OLD in json.dumps(first, ensure_ascii=False), "positive control: the excerpt is visible while current"
    code, document = api(page, report_path, account)
    assert code == 200 and document["id"] == report_id and OLD in document["body"]

    # ------------------------------------------------------------------ SUP-03 only-this-work exception
    button("새 대화").click()
    reply, request_b, path_b, thread2 = decide(GROUP_B, "work")
    group_b = reply["group"]
    assert thread2 != thread1 and reply["focus"] == "tasks" and not group_b.get("report_requested")
    gid_b, task_b = group_b["id"], group_b["tasks"][0]["id"]
    wait_rows("select state from member_conversation_tasks where id=?", lambda found: found == [("completed",)], (task_b,))
    assert replay(path_b, request_b)["group"]["tasks"][0]["source_count"] == 1
    revision = rows(member, "select revision from member_conversation_tasks where id=?", (task_b,))[0][0]
    before = snapshot()
    reply, request_x, path_x, _ = decide(EXCLUDE, "work")
    assert reply["focus"] == "status" and reply["task_id"] == task_b and reply["group"]["id"] == gid_b
    assert rows(member, "select word from member_conversation_exclusions where task_id=?", (task_b,)) == [("어댑터",)]
    assert rows(member, "select count(*) from member_conversation_exclusions") == [(1,)]
    wait_rows("select state,revision from member_conversation_tasks where id=?",
              lambda found: found[0][0] == "completed" and found[0][1] > revision, (task_b,))
    assert snapshot() == before and rows(member, MEMORIES) == [(SUBJECT, VALUE1, 1)]
    excluded = replay(path_x, request_x)["group"]["tasks"][0]
    assert "어댑터 제외" in (excluded.get("filter") or "") and excluded["source_count"] == 0 and excluded["query"] == "준비물"
    untouched = replay(path_a, request_a)["group"]["tasks"][0]
    assert not untouched.get("filter") and untouched["source_count"] == 1, "the exception never reached the other work"
    passed("SUP-03 only-this-work exception: one exclusion row on that task, memory revision and other group untouched")

    # ------------------------------------------------------------------ SUP-04 one-time preference
    refuse(ONE1, "one_time")
    passed("SUP-04a one-time wording in the member's own words is refused even when the classifier says persistent")
    refuse(ONE2, "one_time")
    assert rows(member, MEMORIES) == [(SUBJECT, VALUE1, 1)]
    passed("SUP-04b classifier one-time flag is refused; no revision was created")

    # ------------------------------------------------------------------ SUP-05 non-mutating rephrases
    refuse(NEG, "negated")
    passed("SUP-05a negated request ('지우지 마') leaves the memory intact")
    refuse(INFO, "not_a_request")
    passed("SUP-05b hypothetical question about forgetting is not a request")
    refuse(REPORTED, "not_a_request")
    passed("SUP-05c reported and quoted speech is not a request")
    refuse(INVENTED, "needs_literal")
    passed("SUP-05d a value the member never wrote is not accepted")

    # ------------------------------------------------------------------ SUP-06 conflicting references
    reply, _, _, _ = decide(SECOND, "preference")
    assert reply["view"]["kind"] == "remembered" and reply["view"]["operation"] == "create"
    other = reply["view"]["memory_id"]
    assert rows(member, MEMORIES) == sorted([(SUBJECT, VALUE1, 1), (OTHER, VALUE_OTHER, 1)])
    refuse(AMBIG, "ambiguous", (SUBJECT, OTHER))
    passed("SUP-06a a partial name matching two memories asks which one and deletes neither")
    refuse(EXISTS, "exists", (SUBJECT,))
    assert rows(member, "select count(*) from knowledge_memories where status='active'") == [(2,)]
    passed("SUP-06b a duplicate name is not a second memory and names the existing one")

    # ------------------------------------------------------------------ SUP-07 update and stale excerpts
    reply3, request3, path3, _ = decide(EDIT, "preference")
    view = reply3["view"]
    assert (view["kind"], view["operation"], view["revision"], view["current_revision"], view["superseded"]) == \
        ("remembered", "edit", 2, 2, False) and view["memory_id"] == memory
    assert (view["subject"], view["text"]) == (SUBJECT, VALUE2)
    assert rows(member, MEMORIES) == sorted([(SUBJECT, VALUE2, 2), (OTHER, VALUE_OTHER, 1)])
    assert rows(member, "select revision from knowledge_memory_versions where memory_id=? order by revision", (memory,)) == [(1,), (2,)]
    assert rows(member, "select count(*) from knowledge_memories where id=?", (memory,)) == [(1,)]
    expect(receipt()).to_contain_text(NEW)
    old_receipt = replay(path1, request1)
    assert old_receipt["view"]["kind"] == "remembered" and old_receipt["view"]["operation"] == "create"
    assert (old_receipt["view"]["revision"], old_receipt["view"]["current_revision"], old_receipt["view"]["superseded"]) == (1, 2, True)
    assert old_receipt["view"]["text"] == VALUE2 and OLD not in json.dumps(old_receipt, ensure_ascii=False)
    assert leaked(OLD) == 0 and leaked(NEW) == 0

    def stale(hidden):
        view = replay(path_a, request_a)
        dumped = json.dumps(view, ensure_ascii=False)
        task = view["group"]["tasks"][0]
        assert task["state"] == "completed" and task["source_count"] == 1 and STALE_NOTICE in dumped
        assert all(token not in dumped for token in hidden), "group excerpt"
        code, doc = api(page, report_path, account)
        assert code == 200 and STALE_NOTICE in doc["body"] and all(token not in doc["body"] for token in hidden), "report"
        code, detail = api(page, detail_path, account)
        assert code == 200 and all(token not in json.dumps(detail, ensure_ascii=False) for token in hidden), "worklog"

    def show_report(hidden):
        page.reload()
        expect(editor).to_be_visible()
        page.get_by_role("button", name=re.compile("묶음 보고서 (보기|열기)")).first.click()
        saved = page.get_by_role("region", name=re.compile("묶음 보고서")).first
        expect(saved).to_contain_text(STALE_NOTICE)
        for token in hidden:
            expect(saved).not_to_contain_text(token)

    stale([OLD])
    button("대화 목록").click()
    dialog = page.get_by_role("dialog")
    dialog.get_by_role("searchbox").fill(CHAT)
    dialog.get_by_role("button", name=CHAT, exact=True).click()
    show_report([OLD])
    passed("SUP-07 update: revision 2, older receipt shows the current text, edited excerpt hidden in group, report, worklog and UI")

    # ------------------------------------------------------------------ SUP-08 forget
    reply4, request4, path4, thread = decide(FORGET, "preference")
    assert thread == thread1 and reply4["view"] == {"kind": "withdrawn", "memory_id": memory}
    expect(receipt()).to_contain_text("이 기억은 잊었어요")
    assert rows(member, MEMORIES) == [(OTHER, VALUE_OTHER, 1)]
    assert rows(member, "select count(*) from knowledge_memories where id=? and status='active'", (memory,)) == [(0,)]
    assert rows(member, "select count(*) from knowledge_memory_versions where memory_id=?", (memory,)) == [(0,)]
    assert rows(member, "select count(*) from knowledge_memories where id=? and status='active'", (other,)) == [(1,)]
    for path, request in ((path1, request1), (path3, request3), (path4, request4)):
        withdrawn = replay(path, request)
        assert withdrawn["view"] == {"kind": "withdrawn", "memory_id": memory}
        assert OLD not in json.dumps(withdrawn, ensure_ascii=False) and NEW not in json.dumps(withdrawn, ensure_ascii=False)
    assert leaked(OLD) == 0 and leaked(NEW) == 0
    stale([OLD, NEW])
    show_report([OLD, NEW])
    button("기억").click()
    expect(page.get_by_role("button").filter(has_text=OTHER)).to_be_visible()
    expect(page.get_by_role("button").filter(has_text=SUBJECT)).to_have_count(0)
    button("대화").click()
    refuse(FORGET_AGAIN, "no_match")
    assert rows(member, MEMORIES) == [(OTHER, VALUE_OTHER, 1)]
    passed("SUP-08 forget: memory, versions, old receipts, excerpts, report and worklog show nothing; repeat is a no-op")

    # ------------------------------------------------------------------ SUP-09 shopping and application, no broker seed
    def outcome(text):
        def mine(r):
            if r.request.method != "POST" or "/personal/tasks/research-outcomes/" not in r.url or r.url.endswith("/control"):
                return False
            try:
                body = r.request.post_data_json
            except Exception:
                return False
            return isinstance(body, dict) and body.get("message") == text
        with page.expect_response(mine, timeout=30000) as pending:
            send(text)
        response = pending.value
        assert response.ok, response.status
        body, path = response.request.post_data_json, response.url.removeprefix(origin)
        assert set(body) == OUTCOME_KEYS and canonical(body["request_id"]) and body["browser_runs"] == [] and body["mail_reads"] == []
        assert path.endswith("/personal/tasks/research-outcomes/" + thread1), "outcome created in the visible thread"
        return body, path

    shop, shop_path = outcome(SHOP)
    apply, apply_path = outcome(APPLY)
    assert (shop["kind"], apply["kind"]) == ("shopping", "application")
    listing = api(page, shop_path, account)[1]
    assert listing["thread_id"] == thread1
    made = {item["id"]: item for item in listing["outcomes"]}
    for body in (shop, apply):
        item = made[body["request_id"]]
        assert item["kind"] == body["kind"] and item["state"] == "needs_input" and item["thread_id"] == thread1
        assert item["next"]["kind"] == "ask_url" and canonical(item["question"]["id"])
        assert item["offers"] == [] and item["citations"] == [] and item["receipts"] == [] and item["draft"] is None
    assert rows(member, "select count(*) from personal_browser_jobs") == [(0,)], "nothing was read: no broker seed"
    for body, path in ((shop, shop_path), (apply, apply_path)):
        first_view = api(page, path, account, body)
        assert first_view[0] == 200 and first_view[1]["id"] == body["request_id"] and first_view[1]["revision"] == made[body["request_id"]]["revision"]
        assert api(page, path, account, body)[1] == first_view[1]  # the lost acknowledgement, twice
    assert api(page, shop_path, account, {**shop, "message": shop["message"] + " 다시"})[0] == 409
    for bad in ("http://shop.example/", "https://127.0.0.1/", "file:///etc/passwd"):
        code, _ = api(page, shop_path + "/control", account,
                      {"request_id": shop["request_id"], "revision": made[shop["request_id"]]["revision"], "action": "answer",
                       "text": bad, "question_id": made[shop["request_id"]]["question"]["id"]})
        assert code == 400, "non-public answers are refused"
    assert rows(member, "select kind from member_research_outcomes order by kind") == [("application",), ("shopping",)]
    assert rows(member, "select kind,count(*) from member_linked_work group by kind order by kind") == [("application", 1), ("shopping", 1)]
    assert rows(member, "select count(*) from personal_browser_jobs") == [(0,)]
    card = region("비교와 지원 준비")
    expect(card).to_be_visible()
    expect(card.locator("li.outcome")).to_have_count(2)
    passed("SUP-09 shopping and application from the composer: needs_input, exact replay, 409 on a changed body, one task each")

    # ------------------------------------------------------------------ SUP-10 list and worklog navigation
    page.reload()
    expect(editor).to_be_visible()
    button("맡긴 일").click()
    code, tasks = api(page, "/api/v1/personal/tasks", account)
    kinds = sorted(item["kind"] for item in tasks["items"] if item["kind"] != "text")
    assert code == 200 and kinds == ["application", "conversation_work", "conversation_work", "shopping"], kinds
    listing_view = region("맡긴 일 목록")
    row = lambda title: listing_view.locator(".task-row").filter(has_text=title)
    detail_view = region("맡긴 일 상세")
    for title, kind, text in (("구매 조건 비교", "shopping", SHOP), ("지원 신청 준비", "application", APPLY)):
        expect(row(title)).to_have_count(1)
        assert row(title).get_attribute("data-task-id") in {item["id"] for item in tasks["items"] if item["kind"] == kind}
        expect(row(title)).to_contain_text("답변 필요")
        row(title).click()
        expect(detail_view.get_by_role("heading", name=title, exact=True)).to_be_visible()
        expect(detail_view).to_contain_text(text)
        if kind == "shopping":
            page.get_by_role("button", name=re.compile("원래 대화 열기")).click()
            expect(editor).to_be_visible()
            expect(card).to_be_visible()
            button("맡긴 일").click()
            expect(row(title)).to_have_count(1)
        else:
            button("맡긴 일 목록").click()
            expect(row(title)).to_be_focused()
    for title, task, hidden in ((TITLE_A, task_a, [OLD, NEW]), (TITLE_B, task_b, [OLD, NEW])):
        expect(row(title)).to_have_count(1)
        assert row(title).get_attribute("data-task-id") == task
        row(title).click()
        expect(detail_view.get_by_role("heading", name=title, exact=True)).to_be_visible()
        for token in hidden:
            expect(detail_view).not_to_contain_text(token)
        button("맡긴 일 목록").click()
        expect(row(title)).to_be_focused()
    assert rows(member, "select kind,count(*) from member_linked_work group by kind order by kind") == [("application", 1), ("shopping", 1)]
    button("대화").click()
    expect(editor).to_be_visible()
    passed("SUP-10 task list and worklog: one row per outcome after replay, ids match SQLite, detail and chat round trips")

    assert not [url for url in outside if "google" in (urlparse(url).hostname or "")], "no request left the origin for Google"
    assert provider.calls == 1 and rows(member, "select count(*) from member_conversation_interpretations where outcome is null") == [(0,)]
    print(f"Supplemental routes {provider.interpretations}, chat turns {provider.calls}, provider {provider.model}", flush=True)
