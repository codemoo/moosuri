"""Browser acceptance of member work against real private fixtures."""
from pathlib import Path
import re
import os
import sqlite3
import time
from contextlib import closing
from playwright.sync_api import expect


def exercise(page, members, origin, provider, *, login, logout, rows, passed, prompt, answer):
    first, second = members
    output=os.environ.get("MOOSURI_SYNTHETIC_WORK_SCREENS")
    def capture(name):
        if output:
            directory=Path(output)
            directory.mkdir(parents=True,exist_ok=True)
            page.screenshot(path=str(directory/(name+".png")),animations="disabled")
    button = lambda label: page.get_by_role("button", name=label, exact=True)
    region = lambda label: page.get_by_role("region", name=label, exact=True)
    page.goto(origin)
    login(page, first, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    editor.fill(prompt)
    editor.press("Enter")
    expect(page.get_by_text(answer, exact=True)).to_be_visible(timeout=20000)
    button("업무 기록 보기").click()
    expect(region("맡긴 일 상세").get_by_text(answer, exact=True)).to_be_visible()
    button(re.compile("대화 문맥 준비")).click()
    expect(region("단계 상세")).to_contain_text("이전 메시지 0개")
    passed("TLOG-01 chat receipt, selected context and committed answer")
    button("기억").click()
    button("기억 추가").click()
    page.get_by_label("기억 주제", exact=True).fill("합성 준비물")
    page.get_by_label("기억 내용", exact=True).fill("오전 점검에는 초록 수첩을 준비합니다.")
    button("기억 저장").click()
    expect(page.get_by_role("heading",name="합성 준비물")).to_be_visible()
    button("닫기").click()
    button("대화").click()
    editor.fill("기억에서 수첩 찾아줘")
    expect(button("보내기")).to_be_enabled()
    with page.expect_response(lambda response: response.url.endswith("/personal/tasks/workflows") and response.request.method=="POST") as accepted:
        button("보내기").click()
    assert accepted.value.status==200, f"Task submission HTTP {accepted.value.status}"
    assert rows(first,"SELECT json_array_length(sources) FROM member_work_runs")==[(1,)], "Stored source count differs"

    expect(page.get_by_text(re.compile("기억 검색 결과 1건을 확인했습니다"))).to_be_visible()
    button("업무 기록 보기").last.click()
    source = region("맡긴 일 상세").get_by_role("button",name=re.compile("합성 준비물"))
    source.click()
    expect(region("단계 상세")).to_contain_text("초록 수첩")
    link=page.url
    assert rows(first,"SELECT count(*) FROM member_work_runs")==[(1,)] and provider.calls==1
    passed("TWORK-01 natural-language research reads actual memory in the originating conversation")
    page.set_viewport_size({"width":360,"height":780})
    page.reload()
    source.click()
    expect(region("단계 상세")).to_be_visible()
    assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
    capture("member-work-step-mobile")
    button("진행 기록으로").click()
    expect(source).to_be_focused()
    button(re.compile("원래 대화 열기")).click()
    expect(editor).to_be_visible()
    passed("TLOG-02 mobile deep link, evidence selection, focus return and original chat")
    button("맡긴 일").click()
    expect(page.get_by_role("heading",name="오늘",exact=True)).to_be_visible()
    button("자료 조사 맡기기").click()
    page.get_by_label("찾을 곳").select_option("conversation")
    page.get_by_label("검색어",exact=True).fill("notebook")
    button("조사 실행").click()
    expect(region("맡긴 일 상세").get_by_text(re.compile("대화 1건을 확인"))).to_be_visible()
    region("맡긴 일 상세").get_by_role("button",name=re.compile(re.escape(prompt))).click()
    expect(region("단계 상세")).to_contain_text(answer)
    passed("TWORK-02 search earlier conversation and open original question and reply")
    button("맡긴 일 목록").click()
    button("자료 조사 맡기기").click()
    page.get_by_label("찾을 곳").select_option("memory")
    page.get_by_label("검색어",exact=True).fill("수첩")
    page.get_by_label("업무 이름",exact=False).fill("수첩 점검")
    page.get_by_label("실행 주기").select_option("1800")
    button("예약 추가").click()
    scheduled=page.locator(".task-schedule")
    scheduled.get_by_role("button",name=re.compile("수첩 점검")).click()
    expect(scheduled).to_contain_text("다음 실행")
    assert rows(first,"SELECT period,enabled FROM member_workflows WHERE period>0")==[(1800,1)]
    passed("TSCHED-01 create a recurring search and show its real next run")
    scheduled.get_by_role("button",name="일시정지",exact=True).click()
    expect(scheduled.get_by_role("button",name="예약 재개",exact=True)).to_be_visible()
    scheduled.get_by_role("button",name="지금 실행",exact=True).click()
    expect(region("맡긴 일 상세")).to_contain_text("기억 1건")
    assert rows(first,"SELECT count(*) FROM member_work_runs WHERE due_at IS NOT NULL")==[(0,)]
    button("맡긴 일 목록").click()
    scheduled.get_by_role("button",name="예약 재개",exact=True).click()
    expect(scheduled.get_by_role("button",name="일시정지",exact=True)).to_be_visible()
    passed("TSCHED-02 pause, run once while paused and resume without replay")
    logout(page)
    # Advance only the test fixture's clock boundary. The gateway timer and
    # member process must perform the actual work while no browser is logged in.
    with closing(sqlite3.connect(first[3])) as db:
        db.execute("UPDATE member_workflows SET next_due=? WHERE period=1800 AND enabled=1", (int(time.time())-3601,))
        db.commit()
    deadline=time.monotonic()+30
    while not rows(first,"SELECT id FROM member_work_runs WHERE due_at IS NOT NULL") and time.monotonic()<deadline:
        time.sleep(.25)
    assert rows(first,"SELECT count(*) FROM member_work_runs WHERE due_at IS NOT NULL")==[(1,)]
    login(page,second,True)
    button("맡긴 일").click()
    expect(region("맡긴 일 목록")).to_contain_text("아직 맡긴 일이 없습니다")
    page.goto(link)
    expect(page.get_by_role("alert")).to_contain_text("이 작업을 찾을 수 없습니다")
    assert rows(second,"SELECT count(*) FROM member_work_runs")==[(0,)]
    passed("TLOG-03 second member cannot see another member's list, schedule or direct link")
    logout(page)
    login(page,first,False)
    button("맡긴 일").click()
    button(re.compile("수첩 점검.*기억 1건")).first.click()
    expect(region("맡긴 일 상세")).to_contain_text("예약 실행 시작")
    passed("TSCHED-03 due work executed offline exactly once and retained its own evidence")
    button("맡긴 일 목록").click()
    scheduled.get_by_role("button",name=re.compile("수첩 점검")).click()
    scheduled.get_by_role("button",name="예약 해제",exact=True).click()
    scheduled.get_by_role("button",name="예약 해제 확인",exact=True).click()
    expect(page.locator(".task-schedule")).to_have_count(0)
    assert rows(first,"SELECT count(*) FROM member_work_runs WHERE due_at IS NOT NULL")==[(1,)]
    capture("member-work-list-mobile")
    passed("TSCHED-04 retiring a schedule leaves completed occurrence receipts")
    button("기억").click()
    button(re.compile("합성 준비물")).click()
    button("기억 잊기").click()
    button("기억과 이력 지우기").click()
    page.goto(link)
    button(re.compile("확인한 기억 1")).click()
    expect(region("단계 상세")).to_contain_text("더 이상 볼 수 없습니다")
    assert "초록 수첩" not in region("맡긴 일 상세").inner_text() and provider.calls==1
    passed("TWORK-03 deleted memory is withdrawn from a reopened historical task")
