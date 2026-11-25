"""Conversation tabs: per-thread drafts, an async thread creation, and returning from a task to its own chat."""
import re
import uuid

from playwright.sync_api import expect


def exercise(j):
    page = j.page
    rows = j.rows
    run = uuid.uuid4().hex[:6].upper()
    editor = page.get_by_role("textbox", name="메시지", exact=True)

    def title_of(thread):
        return rows("select title from threads where id=?", (thread,))[0][0]

    def heading(title):
        return page.get_by_role("heading", level=1).filter(has_text=title)

    def open_thread(title, draft=None):
        j.button("대화 목록").click()
        drawer = page.get_by_role("dialog", name="대화 목록", exact=True)
        if draft is not None:
            row = drawer.locator(".member-thread-row").filter(has_text=title)
            expect(row.get_by_text("초안", exact=True)).to_be_visible()
        drawer.get_by_role("button", name=title, exact=True).first.click()
        expect(heading(title)).to_be_visible()
        if draft is not None:
            expect(editor).to_have_value(draft)

    def counts():
        return (rows("select count(*) from chat_runs")[0][0], rows("select count(*) from threads")[0][0],
                len(j.events), j.sent)

    def two_chats(tag):
        """Two real titled conversations; returns [(thread, title), ...] with the second one open."""
        found = []
        for name in ("가", "나"):
            j.new_chat()
            j.chat(f"탭 {tag}{run} {name} 대화, 알겠다고만 답해줘")
            found.append((j.last_thread, title_of(j.last_thread)))
        assert found[0][0] != found[1][0] and found[0][1] != found[1][1]
        return found

    # ------------------------------------------------------------ TAB-01: drafts follow their own conversation
    (ta, title_a), (tb, title_b) = two_chats("A")
    draft_a, draft_b = f"A 대화에 이어 쓸 초안 {run}", f"B 대화에 이어 쓸 초안 {run}\n둘째 줄"
    editor.fill(draft_b)                       # B is open
    open_thread(title_a)
    expect(editor).to_have_value("")
    editor.fill(draft_a)
    before = counts()
    for title, draft in ((title_b, draft_b), (title_a, draft_a), (title_b, draft_b), (title_a, draft_a)):
        open_thread(title, draft)
    page.reload()
    expect(heading(title_a)).to_be_visible()
    expect(editor).to_have_value(draft_a)
    open_thread(title_b, draft_b)
    open_thread(title_a, draft_a)
    assert counts() == before, "switching or reloading sent a message, created a chat or called the model"
    assert rows("select count(*) from chat_runs where user_text in (?,?)", (draft_a, draft_b)) == [(0,)]
    assert {r[0] for r in rows("select id from threads where id in (?,?)", (ta, tb))} == {ta, tb}
    j.case("TAB-01")

    # ------------------------------------------------------------ TAB-03: task detail returns to its own chat
    # Runs before TAB-02 and builds its own conversations, so it is independent of the async-creation case.
    mark = "TB-" + uuid.uuid4().hex[:8].upper()
    j.add_memory(f"탭 시험 자료 {run}", f"탭 시험 자료 표식은 {mark}입니다.")
    # Both conversations keep their identities when work is added to A.
    open_thread(title_a)
    command = f"기억 검색: 탭 시험 자료 {run}; 기억 검색: 표식 {mark}"
    j.send(command)
    def settled():
        groups = rows("select id from member_conversation_groups where thread_id=?", (ta,))
        if len(groups) != 1:
            return None
        tasks = rows("select id,state from member_conversation_tasks where group_id=? order by position", (groups[0][0],))
        return tasks if len(tasks) == 2 and all(t[1] == "completed" for t in tasks) else None
    tasks = j.wait(settled, "two private searches did not complete in conversation A")
    cards = page.locator(".work-task")
    expect(cards).to_have_count(2, timeout=30_000)
    expect(cards.filter(has=page.locator(".work-task-head", has_text=f"탭 시험 자료 {run}"))).to_have_count(1)
    task_id = tasks[0][0]
    draft_a, draft_b = f"작업 뒤에 남길 A 초안 {run}", f"다른 대화의 B 초안 {run}"
    editor.fill(draft_a)
    open_thread(title_b)
    editor.fill(draft_b)
    before = counts(), rows("select count(*) from member_conversation_groups")[0][0], len(tasks)
    j.button("맡긴 일").click()
    ledger = page.get_by_role("region", name="맡긴 일 목록", exact=True)
    row = ledger.locator(f'button[data-task-id="{task_id}"]')
    expect(row).to_have_count(1, timeout=30_000)
    row.click()
    detail = page.get_by_role("region", name="맡긴 일 상세", exact=True)
    back = detail.get_by_role("button", name=re.compile("원래 대화 열기"))
    expect(back).to_contain_text(title_a)
    assert title_a not in title_b
    back.click()
    expect(heading(title_a)).to_be_visible()
    expect(editor).to_have_value(draft_a)
    expect(page.locator(".work-task")).to_have_count(2, timeout=30_000)
    expect(page.locator(".work-task").filter(has=page.locator(".work-task-head", has_text=f"탭 시험 자료 {run}"))).to_have_count(1)
    page.reload()
    expect(heading(title_a)).to_be_visible()
    expect(editor).to_have_value(draft_a)
    expect(page.locator(".work-task")).to_have_count(2, timeout=30_000)
    open_thread(title_b, draft_b)
    open_thread(title_a, draft_a)
    after = counts(), rows("select count(*) from member_conversation_groups")[0][0], \
        rows("select count(*) from member_conversation_tasks where group_id in "
             "(select id from member_conversation_groups where thread_id=?)", (ta,))[0][0]
    assert after == before, "returning from the task detail created work, a message or a model call"
    assert rows("select count(*) from member_conversation_groups where thread_id=?", (tb,)) == [(0,)]
    j.case("TAB-03")

    # A definite refusal must release the creation ID, leaving the draft editable.
    j.new_chat()
    first = "회의 자료에 관해 한 문장으로 답해 줘."
    revised = "‘회람 자료’ 기억이 바뀌면 알려줘."
    def matcher(url):
        return url.split("?")[0].endswith("/threads")

    def refuse_create(route):
        if route.request.method == "POST":
            route.fulfill(status=422, json={"code": "invalid_input"})
        else:
            route.fallback()
    page.route(matcher, refuse_create)
    try:
        with page.expect_response(lambda r: r.request.method == "POST" and matcher(r.url)) as rejected:
            j.send(first)
        assert rejected.value.status == 422
        expect(editor).to_have_value(first)
    finally:
        page.unroute(matcher, refuse_create)
    with page.expect_response(lambda r: r.request.method == "POST" and matcher(r.url)) as accepted:
        j.send(revised)
    assert accepted.value.ok and accepted.value.request.post_data_json["title"] == revised
    assert accepted.value.request.post_data_json["request_id"] != rejected.value.request.post_data_json["request_id"]
    expect(page.get_by_role("heading", name="회람 자료 변화 감시")).to_be_visible()
    assert rows("select count(*) from threads where title=?", (first,)) == [(0,)]
    assert rows("select count(*) from threads where title=?", (revised,)) == [(1,)]
    # An unknown outcome is different: recover the original thread and keep the edited text unsent.
    j.new_chat()
    shared_title = "주말 나들이 때 이동 시간과 준비물을 함께 정리하려고 해. 이번 주말 일정으로 "
    lost_title = shared_title + "처음 계획을 세워 줘."
    edited = shared_title + "시간부터 정해 줘."
    assert lost_title[:40] == edited[:40]
    j.proxy.drop_ack_path = "/api/v1/threads"
    try:
        with page.expect_event("requestfailed", predicate=lambda r: r.method == "POST" and matcher(r.url)) as lost:
            j.send(lost_title)
    finally:
        j.proxy.drop_ack_path = None
    recover = j.button("같은 대화 만들기 다시 확인")
    expect(recover).to_be_visible()
    j.send(edited)
    expect(page.get_by_role("alert")).to_contain_text("이전 대화의 생성 결과")
    with page.expect_response(lambda r: r.request.method == "POST" and matcher(r.url)) as checked:
        recover.click()
    assert checked.value.ok and checked.value.request.post_data_json == lost.value.post_data_json
    recovered = checked.value.json()["id"]
    expect(heading(lost_title[:40])).to_be_visible()
    expect(editor).to_have_value(edited)
    assert rows("select count(*) from threads where title=?", (lost_title[:40],)) == [(1,)]
    assert not rows("select id from chat_runs where thread_id=?", (recovered,))
    j.case("TAB-04")

    # ------------------------------------------------------------ TAB-02: next draft while a new chat is being created
    previous_title = title_a
    j.new_chat()
    watch = "https://status.example.test/service 를 매시간 확인하고 바뀔 때만 알려줘"
    next_draft = f"감시를 만드는 동안 쓴 다음 초안 {run}"
    new_title = watch.strip()[:40]
    threads_before = rows("select count(*) from threads")[0][0]
    held = []

    def hold(route):
        if route.request.method == "POST" and route.request.url.split("?")[0].endswith("/threads"):
            held.append(route)
        else:
            route.fallback()

    page.route(matcher, hold)
    try:
        editor.fill(watch)
        j.sent += 1
        j.button("보내기").evaluate("el => el.click()")   # the click must not wait for the held POST
        j.wait(lambda: held, "thread creation request was not issued")
        assert len(held) == 1
        expect(editor).to_be_enabled()
        expect(j.button("보내기")).to_be_disabled()
        expect(heading("새 대화")).to_be_visible()          # still the empty new chat while creation is pending
        editor.press("Control+A")
        editor.press_sequentially(next_draft)
        expect(editor).to_have_value(next_draft)
        assert rows("select count(*) from threads")[0][0] == threads_before
    finally:
        for route in held:
            route.continue_()
        page.unroute(matcher, hold)
    expect(heading(new_title)).to_be_visible(timeout=30_000)
    expect(page.locator(".member-message").filter(has_text=previous_title)).to_have_count(0)
    new = j.wait(lambda: rows("select id from threads where title=?", (new_title,)) or None, "new thread missing")
    assert len(new) == 1 and rows("select count(*) from threads")[0][0] == threads_before + 1
    thread = new[0][0]
    watches = j.wait(lambda: rows("select url,state from member_public_watches where thread_id=?", (thread,)),
                     "accepted public watch missing")
    assert watches == [("https://status.example.test/service", "active")]
    log = page.get_by_role("region", name="이 대화의 공개 페이지 감시", exact=True)
    expect(log).to_contain_text("https://status.example.test/service", timeout=30_000)
    expect(log.locator("article")).to_have_count(1)
    expect(editor).to_have_value(next_draft)             # the draft typed during creation belongs to the new thread
    page.reload()
    expect(heading(new_title)).to_be_visible()
    expect(editor).to_have_value(next_draft)
    expect(log.locator("article")).to_have_count(1, timeout=30_000)
    assert rows("select count(*) from threads where title=?", (new_title,)) == [(1,)]
    assert next_draft not in {r[0] for r in rows("select user_text from chat_runs")}
    j.case("TAB-02")
