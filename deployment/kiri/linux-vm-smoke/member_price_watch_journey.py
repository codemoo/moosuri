"""Real Linux member UI, grant, timer, notifications and closed broker reads of a synthetic hotel."""
import json
import sqlite3
import time
from contextlib import closing
from html import escape

from playwright.sync_api import expect
from member_conversation_scenarios import api
from member_public_watch_journey import LOG, ROUTE


def exercise(page, members, origin, provider, broker, *, login, rows, passed):
    member = members[0]
    url = "https://hotel.example.test/rooms/pine"
    page.goto(origin)
    login(page, member, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    board = page.get_by_role("region", name=LOG, exact=True)
    account = rows(member, "SELECT id FROM accounts WHERE status='active'")[0][0]
    failures = []

    def result(code, ok, **observation):
        print("OBS " + json.dumps(dict(id=code, **observation), sort_keys=True), flush=True)
        if ok:
            passed(code)
        else:
            failures.append(code)
            print("FAIL " + code, flush=True)

    def set_page(content, **observation):
        broker.set_document(url, f"<html><title>솔방 호텔</title><body>{escape(content)}</body></html>", **observation)

    def send(text):
        editor.fill(text)
        page.get_by_role("button", name="보내기", exact=True).click()

    def checkpoints(watch):
        return rows(member, """SELECT state,closed,notification_id FROM member_public_watch_checks
                               WHERE watch_id=? AND state!='claimed' ORDER BY due_at""", (watch,))

    def observe(watch, content=None, **observation):
        count = 1
        if content is not None:
            count += len(checkpoints(watch))
            set_page(content, **observation)
            # Only the dedicated test member's due time is advanced. The product claims and settles each read.
            with closing(sqlite3.connect(member[3])) as clock:
                clock.execute("UPDATE member_public_watches SET next_due=last_due+1 WHERE id=? AND state='active'", (watch,))
                clock.commit()
        until = time.monotonic() + 25
        while time.monotonic() < until:
            checks = checkpoints(watch)
            if len(checks) == count and checks[-1][1]:
                state, _, notice = checks[-1]
                notices = rows(member, "SELECT id FROM local_notifications WHERE schedule_id=?", (watch,))
                assert not notice or (notice,) in notices, "check receipt has no matching member notification"
                return state, len(notices)
            page.wait_for_timeout(100)
        raise AssertionError(f"missing settled price observation {count}")

    def create(text, content):
        set_page(content)
        send(text)
        until = time.monotonic() + 15
        while time.monotonic() < until:
            found = rows(member, """SELECT w.id,w.thread_id FROM member_public_watches w
                                     JOIN chat_runs r ON w.thread_id=r.thread_id WHERE r.user_text=?
                                     AND w.state='active'""", (text,))
            if found:
                watch, thread = found[0]
                status, listing = api(page, f"/api/v1{ROUTE}?thread_id={thread}", account)
                assert status == 200
                view = next(v for v in listing["items"] if v["id"] == watch)
                return watch, view
            page.wait_for_timeout(100)
        return None, {}

    def stop(watch):
        board.get_by_role("button", name="멈추기", exact=True).click()
        expect(board).to_contain_text("중지됨")
        assert rows(member, "SELECT state,next_due FROM member_public_watches WHERE id=?", (watch,)) == [("stopped", None)]

    text = f"{url} 매시간 확인하고 가격이 바뀌어도 10만 원 이하일 때만 알려줘"
    watch, view = create(text, "객실 요금 125,000원. 체크인 오후 세 시.")
    assert watch, "explicit hourly request did not make a watch"
    initial = observe(watch)
    page.reload()
    expect(board).to_contain_text(url)
    displayed = "100,000원 이하" in board.inner_text()
    result("PRICE-01", initial == ("baseline", 0) and displayed and
           view.get("price_condition") == {"won": 100000, "inclusive": True},
           reading=initial, condition=view.get("price_condition"), displayed=displayed)

    unchanged = observe(watch, "객실 요금 125,000원. 체크인 오후 네 시.")
    result("PRICE-02", unchanged == ("unchanged", 0), reading=unchanged)

    entered = observe(watch, "객실 요금 100,000원. 조식 가능.")
    page.reload()
    expect(board).to_contain_text(url)
    entered_shown = "100,000" in board.inner_text().replace("100,000원 이하", "")
    result("PRICE-03", entered == ("changed", 1) and entered_shown, reading=entered, rate_visible=entered_shown)

    in_band = observe(watch, "객실 요금 98,000원. 조식 가능.")
    repeated = observe(watch, "객실 요금 98,000원. 조식 가능.")
    result("PRICE-04", in_band == repeated == ("unchanged", 1), readings=[in_band, repeated])

    missing = observe(watch, "객실 요금은 문의해 주세요. 보증금 9,000원.")
    ambiguous = observe(watch, "객실 요금 92,000원 / 객실 요금 115,000원. 객실을 선택하세요.")
    unavailable = observe(watch, "객실 요금 78,000원 (예약 마감)")
    mixed = observe(watch, "객실 요금 79,000원 또는 주말 USD 180")
    recovered = observe(watch, "객실 요금 97,000원.")
    result("PRICE-05", missing == ambiguous == unavailable == mixed == ("failed", 1) and recovered == ("unchanged", 1),
           readings=[missing, ambiguous, unavailable, mixed, recovered])

    outside = observe(watch, "객실 요금 113,000원.")
    reentered = observe(watch, "객실 요금 99,000원.")
    result("PRICE-06", outside[1] == 1 and reentered == ("changed", 2), readings=[outside, reentered])
    redirected = observe(watch, "객실 요금 178,000원.", final_url="https://hotel.example.test/rooms/cedar")
    partial = observe(watch, "객실 요금 135,000원.", truncated=True)
    confirmed = observe(watch, "객실 요금 96,000원.")
    result("PRICE-10", redirected == partial == ("failed", 2) and confirmed == ("unchanged", 2),
           readings=[redirected, partial, confirmed])
    stop(watch)

    page.get_by_role("button", name="새 대화", exact=True).click()
    strict, strict_view = create(f"{url} 매시간 확인하고 가격이 바뀌면 10만원 미만일 때만 알려줘", "객실 요금 100,000원.")
    assert strict
    equality = observe(strict)
    below = observe(strict, "객실 요금 99,999원.")
    stop(strict)
    result("PRICE-07", strict_view.get("price_condition") == {"won": 100000, "inclusive": False}
           and equality == ("baseline", 0) and below == ("changed", 1), readings=[equality, below])

    page.get_by_role("button", name="새 대화", exact=True).click()
    immediate, _ = create(f"{url} 예약하지 말고 객실 요금이 10만원 밑으로 내려가면 알려줘", "객실 요금 89,000원.")
    affordable = observe(immediate) if immediate else None
    if immediate:
        stop(immediate)
    result("PRICE-08", affordable == ("changed", 1), reading=affordable)

    page.get_by_role("button", name="새 대화", exact=True).click()
    before = rows(member, "SELECT count(*) FROM member_public_watches")
    alert = page.get_by_role("alert").filter(has_text="가격 기준")
    refusals = []
    for constraint in ("10만원 이하 또는 12만원 미만일 때", "10만원 이하로 안 내려가면", "10만원 이하이고 무료 취소 가능하면"):
        original = f"{url} 매시간 확인하고 가격이 바뀌면 {constraint} 알려줘"
        send(original)
        try:
            expect(alert).to_be_visible(timeout=5000)
            refusals.append(editor.input_value() == original)
        except AssertionError:
            refusals.append(False)
    after = rows(member, "SELECT count(*) FROM member_public_watches")
    result("PRICE-09", all(refusals) and before == after, retained=refusals, created=after[0][0]-before[0][0])

    print("PRICE_RECEIPTS " + json.dumps(dict(reads=broker.count("open"), closed=broker.count("stop"),
          answers=provider.calls, interpretations=provider.interpretations)), flush=True)
    assert not failures, "failed price cases " + ", ".join(failures)
    assert provider.calls == provider.interpretations == 0
    assert set(broker.ops) == {"open", "stop"} and broker.count("open") == broker.count("stop")
