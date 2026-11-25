"""Ordinary member requests with intentionally overlapping chat, shopping and calendar vocabulary."""
import json
import re

from playwright.sync_api import expect

from member_live_journeys import ANSWERS, digest


def routing(j):
    page = j.page
    def ordinary(text, content):
        receipt = len(j.events)
        j.send(text)
        def answered():
            if (page.locator(".member-calendar-chat, .member-request-echo, .work-clarify").count()
                    or page.get_by_role("alert").filter(has_text=re.compile("입력을 유지|Gmail")).count()
                    or page.get_by_role("button", name="메일 확인 계속하기", exact=True).is_visible()
                    or (not page.locator(".member-chat-heading").count() and page.get_by_role("button", name="대화", exact=True).count())
                    or j.rows("SELECT id FROM member_research_outcomes WHERE message=?", (text,))):
                raise AssertionError("ordinary question opened work or a clarification")
            rows = j.rows(ANSWERS, (text,))
            return rows[0] if rows and rows[0][2] == "completed" and rows[0][3] else None
        run, _, _, answer = j.wait(answered, "ordinary member answer missing")
        expect(page.locator(f"#member-turn-{run} .member-reply > p").last).to_have_text(answer)
        assert content.search(answer), "answer omitted the requested explanation or difference"
        assert [e["request"] for e in j.events[receipt:] if e["kind"] == "answer"] == [digest(text)]
        assert not j.rows("SELECT id FROM member_mail_work UNION ALL SELECT request_id FROM member_calendar_actions")
        assert not j.rows("SELECT id FROM member_conversation_groups")

    def shopping(text, ceiling):
        with page.expect_response(lambda r: r.request.method == "POST" and "/personal/tasks/research-outcomes/" in r.url) as post:
            j.send(text)
        assert post.value.ok, "shopping request was refused"
        result = j.rows("SELECT id,criteria FROM member_research_outcomes WHERE message=?", (text,))
        assert len(result) == 1 and json.loads(result[0][1]).get("budget_minor") == ceiling, "reference price became the ceiling"
        if ceiling is not None:
            assert json.loads(result[0][1]).get("currency") == "KRW"
        assert j.rows("SELECT count(*) FROM member_linked_work WHERE operation_id=?", (result[0][0],)) == [(1,)]
        card = page.get_by_role("region", name="비교와 지원 준비")
        expect(card).to_contain_text("구매 비교")
        page.reload()
        expect(card).to_contain_text("구매 비교")
        assert j.rows("SELECT id FROM member_research_outcomes WHERE message=?", (text,)) == [(result[0][0],)]

    def calendar():
        text = "내일 오후 3시에 30분 동안 ‘분기 점검’ 회의를 캘린더에 등록해 줘."
        calls = len(j.events)
        with page.expect_response(lambda r: r.url.endswith("/personal/google/calendar/status")) as status:
            j.send(text)
        if status.value.status in (404, 501):
            expect(page.get_by_role("alert")).to_contain_text("캘린더 기능을 사용할 수 없습니다")
            expect(page.get_by_role("textbox", name="메시지", exact=True)).to_have_value(text)
        else:
            assert status.value.ok, f"calendar status HTTP {status.value.status}"
            expect(page.get_by_role("button", name="Google 캘린더 연결", exact=True)).to_be_visible()
            expect(page.locator(".member-calendar-chat")).to_contain_text(text)
        assert not j.rows("SELECT request_id FROM member_calendar_actions"), "calendar changed before approval"
        assert len(j.events) == calls

    j.run_cases([
        ("INT-01", lambda: ordinary("첫 버스는 10시 10분, 다음 버스는 10시 25분에 도착해. 두 버스의 도착 시간을 비교해 줘. 차이는 몇 분이야?", re.compile(r"15\s*분|십오\s*분"))),
        ("INT-02", lambda: ordinary("메일 답장 초안을 어떻게 써? 인사와 맺음말을 한 문장으로 설명해 줘.", re.compile("인사|맺음|안녕|감사"))),
        ("INT-03", lambda: ordinary("캘린더에 회의를 어떻게 등록해? 제목과 시간을 입력하는 방법을 설명해 줘.", re.compile("제목|시간"))),
        ("INT-04", lambda: ordinary("“메일 답장을 써 줘”와 “회의를 등록해 줘”라는 두 문장이 각각 무슨 뜻인지 한 문장으로 설명해 줘.", re.compile("메일|회신|답장"))),
        ("INT-05", lambda: shopping("판매처별 가격과 배송을 비교해 줘. 참고 가격은 8만원이고 예산은 5만원 이하야.", 50000)),
        ("INT-06", calendar),
        ("INT-07", lambda: ordinary("제품 구매와 배송을 마쳤어. 내가 끝낸 두 가지를 다시 말해 줘.", re.compile("배송"))),
        ("INT-08", lambda: shopping("지난번 참고 가격은 8만원이었어. 이번에는 노트 가격과 배송을 비교해 줘.", None)),
    ])
