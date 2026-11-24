"""Exercise conversation handoffs through a real Kiri member gateway and timer."""
import os
from pathlib import Path
import re
import time
from uuid import uuid4

from playwright.sync_api import expect


def api(page, path, account, payload=None):
    return page.evaluate("""async ([path, account, payload]) => {
        const headers = {"X-Moosuri-Account": account};
        const init = {method: payload ? "POST" : "GET", headers, cache: "no-store"};
        if (payload) {
            headers["Content-Type"] = "application/json";
            init.body = JSON.stringify(payload);
        }
        const response = await fetch(path, init);
        return [response.status, await response.json().catch(() => null)];
    }""", [path, account, payload])


def exercise(page, members, origin, provider, proxy, *, login, logout, rows, passed):
    first, second = members
    seed = "지난 출장 일정을 알려줘."
    answer = "출장 일정은 목요일 열 시 출발입니다."
    follow = "오늘의 인사말을 적어줘."
    greeting = "좋은 아침입니다."
    decoy = "수업 일정도 알려줘."
    decoy_answer = "수업 일정은 화요일 오후입니다."
    history = [("user", seed), ("assistant", answer), ("user", decoy), ("assistant", decoy_answer)]
    provider.turns = [([("user", seed)], answer), (history[:-1], decoy_answer),
                      ([*history, ("user", follow)], greeting)]
    message = "지난 출장 대화에서 정해진 일정을 찾아주고, 내 기억에서 준비물도 찾아줘"
    editor = page.get_by_role("textbox", name="메시지", exact=True)

    def capture(name):
        directory = os.environ.get("MOOSURI_SYNTHETIC_WORK_SCREENS")
        if directory:
            target = Path(directory)
            target.mkdir(exist_ok=True, parents=True)
            page.screenshot(path=str(target / (name + ".png")), animations="disabled")

    def button(label):
        return page.get_by_role("button", name=label, exact=isinstance(label, str))

    def send(value):
        editor.fill(value)
        button("보내기").click()

    def submit(value, path_suffix):
        with page.expect_response(lambda r: r.url.endswith(path_suffix) and r.request.method == "POST") as event:
            send(value)
        response = event.value
        assert response.status == 200, f"submission HTTP {response.status}"
        sent = response.request.post_data_json
        return snapshot(sent["request_id"]), sent

    def add_memory(subject, body):
        button("기억 추가").click()
        page.get_by_label("기억 주제", exact=True).fill(subject)
        page.get_by_label("기억 내용", exact=True).fill(body)
        button("기억 저장").click()
        expect(page.get_by_role("heading", name=subject, exact=True)).to_be_visible()
        button("닫기").click()

    page.goto(origin)
    login(page, first, True)
    send(seed)
    expect(page.get_by_text(answer, exact=True)).to_be_visible(timeout=20000)
    send(decoy)
    expect(page.get_by_text(decoy_answer, exact=True)).to_be_visible(timeout=20000)
    button("기억").click()
    add_memory("준비물 전자기기", "전자기기 준비물에 보라색 충전기를 담습니다.")
    add_memory("준비물 의류", "준비물로 회색 외투를 챙깁니다.")
    add_memory("기차표", "새 기차표의 좌석은 팔 번입니다.")
    button("대화").click()

    account = rows(first, "SELECT id FROM accounts WHERE status='active'")[0][0]
    thread = rows(first, "SELECT thread_id FROM chat_runs")[0][0]
    path = "/api/v1/personal/tasks/conversations/" + thread

    def snapshot(group_id):
        status, body = api(page, path + "?request_id=" + group_id, account)
        assert status == 200 and len(body["groups"]) == 1
        return body["groups"][0]

    def wait_group(group_id, predicate):
        end = time.monotonic() + 35
        while time.monotonic() < end:
            group = snapshot(group_id)
            if predicate(group["tasks"]):
                return group
            page.wait_for_timeout(200)
        raise AssertionError("timer did not produce expected states: " + str([t["state"] for t in group["tasks"]]))

    def control(task, action, **extra):
        return {"request_id": str(uuid4()), "task_id": task["id"], "revision": task["revision"],
                "action": action, **extra}

    group, original = submit(message, path)
    gid = group["id"]
    assert [t["scope"] for t in group["tasks"]] == ["conversation", "memory"]
    assert len(set(t["id"] for t in group["tasks"])) == 2
    assert rows(first, "SELECT count(*) FROM chat_runs WHERE json_extract(model,'$.provider_id')='member-conversation-work'") == [(1,)]
    status, replay = api(page, path, account, original)
    assert status == 200 and [t["id"] for t in replay["tasks"]] == [t["id"] for t in group["tasks"]]
    conflict, _ = api(page, path, account, {**original, "message": "다른 요청"})
    assert conflict == 409
    send(follow)
    expect(page.get_by_text(greeting, exact=True)).to_be_visible(timeout=20000)
    completed = wait_group(gid, lambda tasks: all(t["state"] == "completed" for t in tasks))
    schedule, packing = completed["tasks"]
    assert schedule["source_count"] == 1 and packing["source_count"] == 2
    assert schedule["filter"] == "출장" and "수업 일정" not in schedule["summary"]
    assert "보라색 충전기" in packing["summary"] and "회색 외투" in packing["summary"]
    passed("H1/H2/H3 two durable children, one parent, ordinary chat and replay after acknowledgement")

    editor.fill("아직 보내지 않은 초안")
    page.reload()
    expect(editor).to_have_value("아직 보내지 않은 초안")
    before = rows(first, "SELECT count(*) FROM member_work_runs")
    send("둘 다 어디까지 됐어?")
    expect(page.get_by_text("지금까지의 진행", exact=True)).to_be_visible()
    assert rows(first, "SELECT count(*) FROM member_work_runs") == before and provider.calls == 3
    _, refinement = submit("준비물은 전자기기만", path + "/control")
    assert refinement["task_id"] == packing["id"] and refinement["text"] == "전자기기"
    changed = wait_group(gid, lambda tasks: tasks[1]["state"] == "completed" and len(tasks[1]["runs"]) == 2)
    assert changed["tasks"][0]["runs"] == schedule["runs"]
    narrowed = changed["tasks"][1]
    assert narrowed["source_count"] == 1 and "보라색 충전기" in narrowed["summary"] and "회색 외투" not in narrowed["summary"]
    status, old_work = api(page, "/api/v1/personal/tasks/" + packing["run_id"], account)
    assert status == 200 and "회색 외투" in old_work["result"]
    passed("S1/S2/S4/R1 targeted filter, read-only status, previous occurrence and checked source counts")

    status, missing = api(page, path + "/reports/" + str(uuid4()), account)
    assert status == 404 and (not missing or "body" not in missing)
    card = page.locator("#member-turn-" + group["turn_id"]).get_by_role("listitem", name=narrowed["title"], exact=True)
    expect(card).to_contain_text("보라색 충전기")
    card.scroll_into_view_if_needed()
    capture("conversation-work-desktop")
    page.set_viewport_size({"width": 360, "height": 780})
    card.get_by_role("button", name="진행 기록", exact=True).click()
    expect(card.get_by_role("button", name="2번째 실행 상세")).to_be_visible()
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
    capture("conversation-work-mobile")
    with page.expect_response(lambda r: r.url.endswith(path + "/control") and r.request.method == "POST") as received:
        card.get_by_role("button", name="보고서 만들기").click()
    assert received.value.status == 200
    saved = received.value.request.post_data_json
    current = snapshot(saved["request_id"])
    card.get_by_role("button", name="보고서 보기").click()
    expect(card.get_by_role("region", name=re.compile("보고서"))).to_contain_text("보라색 충전기")
    page.set_viewport_size({"width": 1200, "height": 840})
    report_id = current["tasks"][1]["report_id"]
    report_path = path + "/reports/" + report_id
    status, report = api(page, report_path, account)
    assert status == 200 and report["run_id"] == narrowed["run_id"] and "보라색 충전기" in report["body"]
    assert rows(first, "SELECT count(*) FROM member_conversation_reports") == [(1,)]
    status, duplicate = api(page, path + "/control", account, saved)
    assert status == 200 and duplicate["tasks"][1]["report_id"] == report_id
    passed("R2 only an actual stored receipt opens a report, identical submission opens the same one")

    proven = snapshot(gid)["tasks"][1]
    card.get_by_role("button", name="새 검색어로 다시").click()
    card.get_by_label("새 검색어", exact=True).fill("기차표")
    with page.expect_response(lambda r: r.url.endswith(path + "/control") and r.request.method == "POST") as received:
        card.get_by_role("button", name="새 검색 시작").click()
    assert received.value.status == 200
    again = received.value.request.post_data_json
    reused = snapshot(again["request_id"])
    assert reused["id"] != gid and len(reused["tasks"]) == 1
    fresh = wait_group(reused["id"], lambda tasks: tasks[0]["state"] == "completed")["tasks"][0]
    assert fresh["scope"] == proven["scope"] and fresh["query"] == "기차표"
    assert "팔 번" in fresh["summary"] and "보라색 충전기" not in fresh["summary"]
    status, same_reuse = api(page, path + "/control", account, again)
    assert status == 200 and same_reuse["id"] == reused["id"]
    passed("P1/P2/P4 new search executes against fresh input and replay preserves its own group")

    status, _ = api(page, path + "/control", account,
                    {**control(proven, "refine", text="충전기"), "request_id": gid})
    assert status == 200
    wait_group(gid, lambda ts: ts[1]["state"] == "completed" and len(ts[1]["runs"]) == 3)
    page.reload()
    button("맡긴 일").click()
    ledger = page.get_by_role("region", name="맡긴 일 목록", exact=True)
    packing_row = ledger.locator(".task-row").filter(has_text=packing["title"])
    expect(packing_row).to_have_count(1)
    packing_row.click()
    detail = page.get_by_role("region", name="맡긴 일 상세", exact=True)
    detail.get_by_role("button", name="2번째 실행 · 근거 보기").click()
    detail.get_by_role("button", name="이 회차의 보고서 보기").click()
    expect(detail.get_by_role("region", name="저장한 보고서")).to_contain_text("보라색 충전기")
    detail.get_by_role("button", name=re.compile("원래 대화 열기")).click()
    expect(card).to_be_visible()
    passed("S4/R2 persistent single ledger row and saved report reachable from an older occurrence")

    queries = {"request_id": str(uuid4()), "message": "조회 두 가지", "items": [
        {"title": "어느 일정", "scope": "conversation", "query": ""},
        {"title": "승차권", "scope": "memory", "query": "기차표"}]}
    status, pending = api(page, path, account, queries)
    assert status == 200
    both = wait_group(pending["id"], lambda ts: ts[0]["state"] == "needs_input" and ts[1]["state"] == "completed")
    waiting = both["tasks"][0]
    assert waiting["question_id"] and not waiting["runs"]
    page.reload()
    status, _ = api(page, path + "/control", account, control(waiting, "reuse", text="일정"))
    assert status in (400, 409, 422)
    assert snapshot(pending["id"])["tasks"][0]["question_id"] == waiting["question_id"]
    waiting_card = page.get_by_role("listitem", name=waiting["title"], exact=True)
    expect(waiting_card).to_contain_text("어떤 검색어로 찾을까요?")
    waiting_card.get_by_role("button", name="검색어 입력").click()
    waiting_card.get_by_label("찾을 검색어", exact=True).fill("일정")
    with page.expect_response(lambda r: r.url.endswith(path + "/control") and r.request.method == "POST") as received:
        waiting_card.get_by_role("button", name="답하기").click()
    assert received.value.status == 200
    action = received.value.request.post_data_json
    status, _ = api(page, path + "/control", account, action)
    assert status == 200
    status, _ = api(page, path + "/control", account, {**action, "question_id": str(uuid4())})
    assert status == 409
    resumed = wait_group(pending["id"], lambda ts: ts[0]["state"] == "completed")
    assert len(resumed["tasks"][0]["runs"]) == 1 and resumed["tasks"][1]["runs"] == both["tasks"][1]["runs"]
    passed("Q1/Q2/Q3/Q4 persisted input, independent sibling, stale answer refusal and one resumed run")

    button("기억").click()
    button(re.compile("준비물 전자기기")).click()
    button("기억 수정").click()
    memory_editor = page.get_by_role("dialog", name="기억 작성", exact=True)
    memory_editor.get_by_role("textbox", name=re.compile("^기억 내용")).fill("전자기기 준비물은 초록색 어댑터입니다.")
    button("기억 저장").click()
    expect(page.get_by_text("전자기기 준비물은 초록색 어댑터입니다.", exact=True).first).to_be_visible()
    status, revised = api(page, report_path, account)
    assert status == 200 and "더 이상 볼 수 없습니다" in revised["body"]
    assert "보라색 충전기" not in revised["body"] and "초록색 어댑터" not in revised["body"]
    button("기억 잊기").click()
    button("기억과 이력 지우기").click()
    expect(button(re.compile("준비물 전자기기"))).to_have_count(0)
    status, withdrawn = api(page, report_path, account)
    assert status == 200 and "보라색 충전기" not in withdrawn["body"] and "더 이상 볼 수 없습니다" in withdrawn["body"]
    status, _ = api(page, path + "/control", account, control(snapshot(gid)["tasks"][1], "reuse", text="새 입력"))
    assert status in (400, 404, 409, 422)
    passed("R3/P3 editing and forgetting the source withdraw its old excerpt and reusable proof")

    button("대화").click()
    expect(card).to_contain_text("더 이상 볼 수 없습니다")
    expect(card).not_to_contain_text("보라색 충전기")
    expect(card.get_by_role("button", name="새 검색어로 다시")).to_have_count(0)
    pending_group, _ = submit("대화 검색: ; 기억 검색:", path)
    send("그거 멈춰")
    chooser = page.locator(".work-clarify")
    expect(chooser).to_be_visible()
    assert all(t["state"] == "needs_input" for t in snapshot(pending_group["id"])["tasks"])
    chooser.get_by_role("button", name=re.compile("기억")).click()
    after_stop = wait_group(pending_group["id"], lambda ts: [t["state"] for t in ts] == ["needs_input", "cancelled"])
    status, _ = api(page, path + "/control", account, control(after_stop["tasks"][1], "reuse", text="새 검색"))
    assert status == 409 and not after_stop["tasks"][1]["runs"]
    _, pending_answer = submit("질문 답은 일정", path + "/control")
    assert pending_answer["question_id"] == pending_group["tasks"][0]["question_id"]
    wait_group(pending_group["id"], lambda ts: [t["state"] for t in ts] == ["completed", "cancelled"])
    passed("S3 ambiguous follow-up asks which child, selected child alone is cancelled")

    def failed_submission(req):
        return req.url == origin + path and req.method == "POST"

    groups_for_message = "SELECT id,turn_id FROM member_conversation_groups WHERE message=?"
    retry_message = "지난 답사 대화에서 일정을 찾아주고, 내 기억에서 환승표도 찾아줘"
    proxy.drop_ack_path = path
    try:
        with page.expect_event("requestfailed", predicate=failed_submission) as lost:
            send(retry_message)
    finally:
        proxy.drop_ack_path = None
    original = lost.value.post_data_json
    [(group_id, turn_id)] = rows(first, groups_for_message, (retry_message,))
    assert original["request_id"] == group_id and original["items"][0]["filters"] == ["답사"]
    page.reload()
    recovered_turn = page.locator("#member-turn-" + turn_id)
    expect(recovered_turn).to_contain_text("답사 일정 조사", timeout=15000)
    assert rows(first, groups_for_message, (retry_message,)) == [(group_id, turn_id)]
    assert snapshot(group_id)["tasks"][0]["filter"] == "답사"
    passed("H3 committed scoped request survives a dropped HTTP acknowledgement and browser reload")

    unsubmitted = "지난 견학 대화에서 장소를 찾아주고, 내 기억에서 승차표도 찾아줘"
    pattern = "**" + path
    def block_send(route):
        if failed_submission(route.request):
            route.abort()
        else:
            route.continue_()

    page.route(pattern, block_send)
    try:
        with page.expect_event("requestfailed", predicate=failed_submission) as blocked:
            send(unsubmitted)
        expect(button("동일 요청 다시 확인")).to_be_visible()
    finally:
        page.unroute(pattern, block_send)
    page.reload()
    expect(button("동일 요청 다시 확인")).to_be_visible()
    assert not rows(first, groups_for_message, (unsubmitted,))
    with page.expect_response(lambda r: failed_submission(r.request)) as retried:
        button("동일 요청 다시 확인").click()
    response = retried.value
    assert response.status == 200 and response.request.post_data_json == blocked.value.post_data_json
    retried_group = snapshot(response.request.post_data_json["request_id"])
    assert retried_group["tasks"][0]["filter"] == "견학"
    assert rows(first, groups_for_message, (unsubmitted,)) == [(retried_group["id"], retried_group["turn_id"])]
    passed("H6 request lost before admission retains its ID and filters on manual retry after reload")

    notify_group, _ = submit("대화 검색: 비행기표; 기억 검색: 공항버스", path)
    button("새 대화").click()
    editor.fill("다른 대화에서 쓰는 초안")
    completed_notice = page.get_by_role("complementary", name="완료된 조사")
    wait_group(notify_group["id"], lambda ts: all(t["state"] == "completed" for t in ts))
    expect(editor).to_have_value("다른 대화에서 쓰는 초안")
    expect(editor).to_be_focused()
    completed_notice.get_by_role("button", name=re.compile("완료 .*건")).click()
    for task in notify_group["tasks"]:
        notice = completed_notice.get_by_text(task["title"] + " 완료", exact=True)
        expect(notice).to_be_visible(timeout=35000)
        expect(notice).to_have_count(1)
    expect(editor).to_have_value("다른 대화에서 쓰는 초안")
    passed("R4 background completions appear once per run and preserve the other chat's draft and focus")

    logout(page)
    login(page, second, True)
    second_account = rows(second, "SELECT id FROM accounts WHERE status='active'")[0][0]
    for resource in (report_path, path):
        status, body = api(page, resource, second_account)
        assert status in (403, 404) and (not body or "body" not in body)
    status, _ = api(page, path + "/control", second_account, action)
    assert status in (403, 404)
    assert rows(second, "SELECT count(*) FROM member_work_runs") == [(0,)]
    passed("H5/Q4/R5 second member cannot read, answer or open the other member's report")
