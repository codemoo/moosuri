"""The same timing words must mean the same thing on local and public member watches."""
import re

from playwright.sync_api import expect



def exercise(j):
    page = j.page
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    monitor_path = "/personal/tasks/monitors"
    def monitor_rows():
        return j.rows("SELECT id FROM member_work_monitors ORDER BY id")

    def refusal(text, reason="간격|주기"):
        watches = "SELECT id FROM member_work_monitors UNION ALL SELECT id FROM member_public_watches ORDER BY id"
        before = j.rows(watches)
        j.send(text)
        expect(page.get_by_role("alert").filter(has_text=re.compile(reason))).to_be_visible(timeout=3500)
        expect(editor).to_have_value(text)
        assert j.rows(watches) == before, "a refused schedule was registered"

    def local(text, query, interval, days):
        with page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith(monitor_path)) as created:
            j.send(text)
        response = created.value
        assert response.ok, f"monitor create HTTP {response.status}"
        mid = response.request.post_data_json["request_id"]
        assert response.json()["id"] == mid
        assert j.rows("SELECT m.id,m.every_seconds,m.days,m.state FROM member_work_monitors m JOIN member_workflows w ON w.id=m.id WHERE w.query=?", (query,)) == [
            (mid, interval, days, "active")], "stored cadence, lifetime, query or request ID differs"
        card = page.get_by_role("region", name="내 감시와 브리핑").get_by_role("article").filter(has_text=query+" 변화 감시")
        expect(card).to_contain_text({1800: "30분", 3600: "1시간", 86400: "하루"}[interval])
        expect(card).to_contain_text(f"{days}일간")
        return mid

    def public():
        url = "https://status.example.test/timing"
        text = f"{url} 를 반 시간마다 이틀 동안 확인하고 바뀔 때만 알려줘"
        path = "/personal/tasks/public-watches"
        with page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith(path)) as created:
            j.send(text)
        response = created.value
        assert response.ok
        wid = response.request.post_data_json["request_id"]
        assert response.json()["id"] == wid
        sql = "SELECT id,every_seconds,days,state FROM member_public_watches WHERE url=?"
        assert j.rows(sql, (url,)) == [(wid, 1800, 2, "active")], "public watch used different timing"
        board = page.get_by_role("region", name="이 대화의 공개 페이지 감시")
        page.reload()
        expect(board).to_contain_text(url)
        expect(board).to_contain_text("30분마다 확인 · 2일 동안")
        with page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith(f"/{wid}/stop")) as stop:
            board.get_by_role("button", name="멈추기").click()
        assert stop.value.ok and j.rows(sql, (url,)) == [(wid, 1800, 2, "stopped")]
        expect(board.get_by_role("button", name="멈추기")).to_have_count(0)

    def conflicting():
        refusal("‘업무 공지’ 기억을 30분마다 1시간마다 확인하고 바뀔 때만 알려줘")
        for text, message in [
            ("‘정기 점검’ 기억이 바뀌면 매시간 이틀 동안 사흘 동안 알려줘", "기간"),
            ("https://status.example.test/conflict 를 30분마다 1시간마다 확인하고 바뀔 때만 알려줘", "간격"),
            ("매일 아침 8시에 중요한 메일을 30분마다 요약해서 알려줘", "간격"),
        ]:
            before = j.rows("SELECT id FROM member_work_monitors UNION ALL SELECT id FROM member_public_watches ORDER BY id")
            j.send(text)
            expect(page.get_by_role("alert").filter(has_text=message)).to_be_visible()
            expect(editor).to_have_value(text)
            expect(page.get_by_role("region", name="메일·일정 브리핑 요청 확인")).to_have_count(0)
            assert j.rows("SELECT id FROM member_work_monitors UNION ALL SELECT id FROM member_public_watches ORDER BY id") == before

    def repair():
        before = monitor_rows()
        refusal("기억에서 ‘전시 안내’가 바뀌면 매주 알려줘")
        local("기억에서 ‘전시 안내’가 바뀌면 하루마다 사흘 동안 알려줘", "전시 안내", 86400, 3)
        assert len(monitor_rows()) == len(before) + 1

    def clarify_frequency_and_end():
        refusal("https://status.example.test/news 를 하루에 두 번 확인하고 바뀌면 알려줘")
        refusal("‘학사 공지’ 기억이 바뀌면 매시간 12월 25일까지 알려줘", "기간")
        local("‘학사 공지’ 기억이 바뀌면 하루 한 번 일주일 동안 알려줘", "학사 공지", 86400, 7)

    j.run_cases([
        ("TIME-01", lambda: refusal("기억에서 ‘교통 공지’가 바뀌면 두 시간마다 알려줘")),
        ("TIME-02", lambda: local("‘점검 일정’ 기억이 바뀌면 반 시간마다 이틀 동안 알려줘", "점검 일정", 1800, 2)),
        ("TIME-03", lambda: local("기억에서 ‘10분마다 점검 90일 동안’이 바뀌면 매시간 알려줘", "10분마다 점검 90일 동안", 3600, 7)),
        ("TIME-04", public),
        ("TIME-05", conflicting),
        ("TIME-06", repair),
        ("TIME-07", lambda: refusal("기억에서 ‘당직 교대’가 바뀌면 1시간 30분마다 알려줘")),
        ("TIME-08", lambda: refusal("https://status.example.test/short 를 30분마다 2시간 동안 확인하고 바뀔 때만 알려줘", "기간")),
        ("TIME-09", lambda: local("‘출고 공지’ 기억이 바뀌면 매 30분 간격으로 48시간 동안 알려줘", "출고 공지", 1800, 2)),
        ("TIME-10", clarify_frequency_and_end),
    ])
    assert not j.events, "a watch control unexpectedly used the model"
