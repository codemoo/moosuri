"""Lose acknowledgements at the browser boundary and recover through the same member controls."""
from urllib.parse import urlsplit

from playwright.sync_api import expect

from member_live_journeys import ANSWERS, digest


def exercise(j):
    page, button = j.page, j.button
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    monitors = "/personal/tasks/monitors"
    retry = button("동일 요청 다시 확인")

    def posted(response, suffix):
        return response.request.method == "POST" and urlsplit(response.url).path.endswith(suffix)

    def lose(suffix, submit, *, after_commit):
        if after_commit:
            j.proxy.drop_ack_path = "/api/v1" + suffix
            try:
                with page.expect_event("requestfailed", predicate=lambda r: r.method == "POST" and
                                       urlsplit(r.url).path == j.proxy.drop_ack_path) as lost:
                    submit()
                expect(retry).to_be_visible(timeout=20000)
                return lost.value.post_data_json
            finally:
                j.proxy.drop_ack_path = None
        observed = []
        def drop(route):
            if route.request.method != "POST":
                route.fallback()
                return
            observed.append(route.request.post_data_json)
            route.abort("connectionreset")
        with_route = "**" + suffix
        page.route(with_route, drop)
        try:
            submit()
            expect(retry).to_be_visible(timeout=20000)
        finally:
            page.unroute(with_route, drop)
        assert len(observed) == 1
        return observed[0]

    def answer(text, request, receipt):
        def completed():
            found = j.rows(ANSWERS, (text,))
            assert len(found) <= 1, "retry duplicated the chat run"
            return found[0] if found and found[0][2] == "completed" and found[0][3] else None
        run, _, _, reply = j.wait(completed, "recovered text never produced a completed reply")
        expect(page.locator(f"#member-turn-{run} .member-reply > p").last).to_have_text(reply)
        assert j.rows("SELECT request_id FROM chat_runs WHERE id=?", (run,)) == [(request,)]
        assert sum(e["kind"] == "answer" and e["request"] == digest(text) for e in j.events[receipt:]) == 1

    def committed_create():
        text = "‘이번 주 준비물’ 기억이 바뀌면 바로 알려줘."
        before = len(j.events)
        # The write and the parallel history read can acknowledge the same turn. Keep both
        # acknowledgements unavailable until the member has pressed the manual retry control.
        j.proxy.history_unavailable = True
        try:
            original = lose(monitors, lambda: j.send(text), after_commit=True)
            mid = original["request_id"]
            with page.expect_response(lambda r: posted(r, monitors)) as repeated:
                retry.click()
        finally:
            j.proxy.history_unavailable = False
        assert repeated.value.ok and repeated.value.request.post_data_json == original
        expect(page.get_by_role("heading", name="이번 주 준비물 변화 감시")).to_be_visible()
        page.reload()
        assert j.rows("SELECT id,state FROM member_work_monitors WHERE id=?", (mid,)) == [(mid, "active")]
        assert j.rows("SELECT count(*) FROM chat_runs WHERE request_id=?", (mid,)) == [(1,)]
        assert len(j.events) == before

    def rejected():
        text, next_draft = "삼각형의 꼭짓점은 몇 개야? 숫자로 답해 줘.", "다음에는 사각형을 물어볼게."
        start = len(j.events)
        held = []
        def hold(route):
            if route.request.method == "POST":
                held.append(route)
            else:
                route.fallback()
        pattern = "**/threads/*/text-preview"
        page.route(pattern, hold)
        try:
            j.send(text)
            j.wait(lambda: held, "text request never reached the fault boundary")
            editor.fill(next_draft)
            expect(editor).to_have_value(next_draft)
            request = held[0].request.post_data_json
            held[0].fulfill(status=422, json={"code": "personal_text_input_budget_exceeded"})
        finally:
            page.unroute(pattern, hold)
        again = button("이전 메시지 다시 보내기")
        expect(again).to_be_visible()
        expect(editor).to_have_value(next_draft)
        page.reload()
        expect(editor).to_have_value(next_draft)
        with page.expect_response(lambda r: posted(r, "/text-preview")) as repeated:
            again.click()
        assert repeated.value.ok and repeated.value.request.post_data_json == request
        answer(text, request["request_id"], start)
        expect(editor).to_have_value(next_draft)
        assert not j.rows("SELECT id FROM chat_runs WHERE user_text=?", (next_draft,))

    def plain_choice():
        text = "빨강과 파랑을 섞으면 무슨 색이야? 한 단어로 알려줘."
        classifier = "**/threads/*/work-interpret"
        def unavailable(route):
            route.fulfill(status=503, json={"code": "unavailable"})
        page.route(classifier, unavailable)
        try:
            j.send(text)
            plain = button("일반 메시지로 보내기")
            expect(plain).to_be_visible()
            original = lose("/threads/*/text-preview", plain.click, after_commit=False)
        finally:
            page.unroute(classifier, unavailable)
        route_calls = []
        def capture(req):
            if req.method == "POST" and req.url.endswith("/work-interpret"):
                route_calls.append(req.post_data_json)
        page.on("request", capture)
        try:
            start = len(j.events)
            page.reload()
            expect(retry).to_be_visible()
            with page.expect_response(lambda r: posted(r, "/text-preview")) as repeated:
                retry.click()
            assert repeated.value.ok and repeated.value.request.post_data_json == original
            answer(text, original["request_id"], start)
            assert not route_calls, "plain choice was interpreted as work again after reload"
        finally:
            page.remove_listener("request", capture)

    def stopped():
        text = "‘요약 마감’ 기억이 바뀌면 바로 알려줘."
        stop = "‘요약 마감 변화 감시’ 감시 그만해줘"
        with page.expect_response(lambda r: posted(r, monitors)) as made:
            j.send(text)
        first = made.value.json()["id"]
        path = f"{monitors}/{first}/stop"
        original = lose(path, lambda: j.send(stop), after_commit=True)
        assert j.rows("SELECT state,stop_request_id FROM member_work_monitors WHERE id=?", (first,)) == [
            ("stopped", original["request_id"])]
        other = page.context.new_page()
        try:
            other.goto(page.url)
            other.get_by_role("button", name="새 대화", exact=True).click()
            other.get_by_role("textbox", name="메시지", exact=True).fill(text)
            with other.expect_response(lambda r: posted(r, monitors)) as another:
                other.get_by_role("button", name="보내기", exact=True).click()
            j.sent += 1
            second = another.value.json()["id"]
            assert second != first
        finally:
            other.close()
        calls = len(j.events)
        page.reload()
        with page.expect_response(lambda r: posted(r, "/stop"), timeout=12000) as repeated:
            retry.click()
        assert repeated.value.ok and posted(repeated.value, path) and repeated.value.request.post_data_json == original, \
            "retry switched the monitor ID or revision"
        listing = page.get_by_role("region", name="내 감시와 브리핑")
        expect(listing).to_contain_text("중지됨")
        assert j.rows("SELECT id FROM member_work_monitors WHERE stop_request_id=?", (original["request_id"],)) == [(first,)]
        assert j.rows("SELECT state,stop_request_id FROM member_work_monitors WHERE id=?", (second,)) == [("active", None)]
        assert len(j.events) == calls

    def conflict():
        text = "‘교대 공지’ 기억이 바뀌면 알려줘."
        with page.expect_response(lambda r: posted(r, monitors)) as made:
            j.send(text)
        mid = made.value.json()["id"]
        command = "‘교대 공지 변화 감시’ 감시 그만해줘"
        pattern = "**" + monitors + "/" + mid + "/stop"
        page.route(pattern, lambda r: r.fulfill(status=409, json={"code": "conflict"}), times=1)
        j.send(command)
        expect(page.get_by_role("alert")).to_contain_text("상태가 바뀌")
        expect(editor).to_have_value(command)
        expect(retry).to_have_count(0)
        assert j.rows("SELECT state FROM member_work_monitors WHERE id=?", (mid,)) == [("active",)]
        card = page.get_by_role("article").filter(has=page.get_by_role("heading", name="교대 공지 변화 감시"))
        with page.expect_response(lambda r: posted(r, f"/{mid}/stop")) as confirmed:
            card.get_by_role("button", name="멈추기").click()
        assert confirmed.value.ok
        expect(card).to_contain_text("중지됨")
        assert j.rows("SELECT state FROM member_work_monitors WHERE id=?", (mid,)) == [("stopped",)]

    j.run_cases([("ACK-01", committed_create), ("ACK-02", rejected),
                    ("ACK-03", plain_choice), ("ACK-04", stopped), ("ACK-05", conflict)])
