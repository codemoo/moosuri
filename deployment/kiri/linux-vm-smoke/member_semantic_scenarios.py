"""Synthetic counterparts of the member's everyday requests, through the HTTPS composer."""
import re
import time
from playwright.sync_api import expect
from member_conversation_scenarios import api

TRAVEL = "지난 출장 때 몇 시에 출발하기로 했는지랑 챙길 것, 한 장으로 정리해서 저장해 줘."
SEED = "합성 출장 기록이야. 출장 출발은 목요일 06:10이야. 확인이라고 답해 줘."
EVIDENCE = "방금 찾은 전자기기 준비물의 확인 표식은 뭐야? 근거에 적힌 대로 답해 줘."
REOPEN = "방금 정리한 것들 한 장으로 다시 보여 줘."
PARAPHRASE = "출장 때 언제 출발하기로 했는지 전에 나눈 얘기에서 확인하고, 챙길 건 내가 적어 둔 기억에서 꺼내 줘."
READ_ONLY = "지난 출장 대화에서 일정을 찾아만 줘. 달력 등록은 하지 마."
TOKEN = "MORU-718"


def exercise(page, members, origin, provider, *, login, rows, passed):
    member = members[0]
    items = [{"title": "출장 출발", "scope": "conversation", "query": "출발"},
             {"title": "출장 준비물", "scope": "memory", "query": "준비물"}]
    provider.turns = [([("user", SEED)], "확인")]
    provider.work_intents = {TRAVEL: {"intent": "create", "items": items, "report": "combined"},
                             PARAPHRASE: {"intent": "create", "items": items},
                             READ_ONLY: {"intent": "create", "items": [items[0]]},
                             EVIDENCE: {"intent": "evidence", "target": 2},
                             REOPEN: {"intent": "report", "report": "combined"}}
    page.goto(origin)
    login(page, member, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    def button(label):
        return page.get_by_role("button", name=label, exact=True)
    def send(text):
        editor.fill(text)
        button("보내기").click()
    admission_times = {}
    def interpret(text):
        began = time.monotonic()
        def mine(r):
            # The composer may also reopen an earlier receipt read-only after a reload. Such a late response
            # must never be taken for the request just sent, so match the exact text and exclude read_only.
            if r.request.method != "POST" or not r.url.endswith("/work-interpret"):
                return False
            try:
                body = r.request.post_data_json
            except Exception:
                return False
            return isinstance(body, dict) and body.get("text") == text and not body.get("read_only")
        with page.expect_response(mine, timeout=90000) as pending:
            send(text)
        response = pending.value
        admission_times[text] = time.monotonic() - began
        assert response.ok, (response.status, response.status_text)
        # A newly created thread can replace Chromium's response object before its body finishes.
        # Check the canonical committed receipt via the side-effect-free read, including for a replay.
        account_id = rows(member, "select id from accounts where status='active'")[0][0]
        code, result = api(page, response.url.removeprefix(origin), account_id,
                           {**response.request.post_data_json, "read_only": True})
        assert code == 200, ("missing committed receipt", code)
        assert response.ok and result["kind"] == "work", (response.status, result.get("code"), result.get("kind"))
        return result, response.request.post_data_json, response.url.removeprefix(origin)
    def wait_rows(sql, test):
        until = time.monotonic() + 40
        while time.monotonic() < until:
            result = rows(member, sql)
            if test(result):
                return result
            page.wait_for_timeout(125)
        raise AssertionError(f"SQLite condition did not settle: {sql}")
    def add(subject, body):
        button("기억 추가").click()
        page.get_by_label("기억 주제", exact=True).fill(subject)
        page.get_by_label("기억 내용", exact=True).fill(body)
        button("기억 저장").click()
        expect(page.get_by_role("heading", name=subject, exact=True)).to_be_visible()
        button("닫기").click()

    send(SEED)
    wait_rows("select state from chat_runs", lambda result: result == [("completed",)])
    button("기억").click()
    add("지난 출장 전자기기 준비물", f"출장 때 챙길 준비물은 어댑터입니다. 확인 표식은 {TOKEN}입니다.")
    add("출장 의류 준비물", "출장 준비물로 회색 외투를 챙겨요.")
    button("대화").click()
    began = time.monotonic()
    reply, request, path = interpret(TRAVEL)
    gid = reply["group"]["id"]
    def interpretation_result(group):
        return {"searches": [(t["scope"], t["query"]) for t in group["tasks"]],
                "report_requested": group.get("report_requested", False)}
    assert len(reply["group"]["tasks"]) == 2 and reply["group"].get("report_requested"), interpretation_result(reply["group"])
    account = rows(member, "select id from accounts where status='active'")[0][0]
    before = provider.interpretations
    status, replay = api(page, path, account, request)
    assert status == 200 and replay["group"]["id"] == gid and provider.interpretations == before
    assert api(page, path, account, {**request, "text": PARAPHRASE})[0] == 409
    passed(f"SW-01/SW-02 plain request and lost acknowledgement produce one durable group (admitted in {admission_times[TRAVEL]:.1f}s)")
    reports = wait_rows("select id from member_conversation_group_reports where state='ready'", lambda x: len(x) == 1)
    # The report row and its item rows are separate reads: settle on the durable item count, same expectation.
    wait_rows("select count(*) from member_conversation_group_report_items", lambda found: found == [(2,)])
    page.reload()
    opener = page.get_by_role("button", name=re.compile("묶음 보고서 (보기|열기)"))
    opener.first.click()
    report = page.get_by_role("region", name=re.compile("묶음 보고서")).first
    expect(report).to_contain_text(TOKEN)
    expect(report).to_contain_text("06:10")
    passed(f"SW-03 reopened combined result contains both sources ({time.monotonic()-began:.1f}s)")
    reply, _, _ = interpret(EVIDENCE)
    assert reply["focus"] == "evidence"
    focused = page.get_by_role("region", name="요청한 작업 결과")
    expect(focused).to_contain_text(TOKEN)
    expect(focused.get_by_role("button", name="상세·근거 보기").first).to_be_visible()
    passed("SW-04 same-thread follow-up exposes the marker and the source")
    reply, _, _ = interpret(REOPEN)
    assert reply["focus"] == "report" and reply["group"]["group_report_id"] == reports[0][0]
    assert rows(member, "select count(*) from member_conversation_group_reports") == [(1,)]
    passed("SW-05 report can be requested again without duplicating the saved result")
    button("새 대화").click()
    reply, _, _ = interpret(PARAPHRASE)
    assert reply["group"]["id"] != gid and sorted(t["scope"] for t in reply["group"]["tasks"]) == ["conversation", "memory"], interpretation_result(reply["group"])
    passed("SW-06 equivalent wording starts both searches")
    button("새 대화").click()
    reply, _, _ = interpret(READ_ONLY)
    assert len(reply["group"]["tasks"]) == 1 and reply["group"]["tasks"][0]["scope"] == "conversation"
    assert rows(member, "select count(*) from member_conversation_interpretations where outcome is null") == [(0,)]
    passed("SW-07 explicitly requested read with prohibited calendar write")
    watch = "기억에서 ‘준비물’이 바뀌면 30분마다 알려줘 7일 동안"
    brief = "매일 아침 8시에 기억에서 ‘준비물’ 브리핑해줘 7일 동안"
    for message, kind in [(watch, "watch"), (brief, "brief")]:
        with page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith("/personal/tasks/monitors")) as event:
            send(message)
        response = event.value
        data = response.json()
        assert response.ok and data["state"] == "active" and data["kind"] == kind, (response.status, data.get("code"))
        args = response.request.post_data_json
        assert rows(member, "select count(*) from member_workflows where thread_id is not null and message is not null and id in (select id from member_work_monitors)")[0][0] >= 1
        before_count = rows(member, "select count(*) from member_work_monitors")
        assert api(page, "/api/v1/personal/tasks/monitors", account, args)[0] == 200
        assert rows(member, "select count(*) from member_work_monitors") == before_count
    passed("MW-01/MW-02 a change watch and a local-time brief are registered from chat, with the same request on replay")
    page.reload()
    expect(page.get_by_text(re.compile("브리핑을 등록했습니다|브리핑을 등록했어요")).first).to_be_visible()
    send("내 감시 목록 보여줘")
    board = page.get_by_role("region", name=re.compile("감시|브리핑"))
    expect(board.get_by_text("준비물 브리핑", exact=True).first).to_be_visible()
    record = rows(member, "select id from member_work_monitors where kind='watch'")[0][0]
    with page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith(f"/{record}/stop")):
        send("‘준비물 변화 감시’ 감시 그만해줘")
    assert rows(member, "select state from member_work_monitors where kind='watch'") == [("stopped",)]
    assert rows(member, "select state from member_work_monitors where kind='brief'") == [("active",)]
    passed("MW-03/MW-04 reopen work history and stop just the named recurring job")
    print(f"Semantic routes {provider.interpretations}, chat turns {provider.calls}, provider {provider.model}", flush=True)
