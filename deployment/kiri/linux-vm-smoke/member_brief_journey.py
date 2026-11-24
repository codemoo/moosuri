"""Linux user-path acceptance of the member-facing connected morning brief on the ordinary chat (no Google).

Invoked by member-web-fullstack.py as ``--brief-work`` (main wires it; this module only supplies ``exercise``):

    member_brief_journey.exercise(page, members, origin, provider, login=login, rows=rows, passed=passed)

Setup is the plain no-Google fixture: test-fixtures binary, two isolated member volumes, ordinary HTTPS UI, the
deterministic provider. No Google OAuth, no mailbox, no calendar and no network to Google exist, so the member has
NO Gmail and NO Calendar connection. Every case drives the real composer with typed Korean text, real buttons and
real reloads. The only direct HTTP calls are read-only GETs for cross-checks. SQLite is opened read-only (``rows``).
Only ids, counts and PASS/FAIL are printed, never member text.

Contract exercised (web/src/MemberWorkMonitorsChat.tsx parseConnectedBriefChat, web/src/MemberConnectedBrief.tsx):
  request   "매일 아침 8시에 오늘 일정과 중요한 메일을 알려줘"  -> card "메일·일정 브리핑 요청 확인" in the SAME thread
  card      describes 매일 08:00, mail subject/sender + today's events, 읽기 전용; checks /personal/google and
            /personal/google/calendar/status; with both missing it shows "Gmail·캘린더 연결이 필요해요", and the
            grant button "읽기 전용으로 허용하고 시작" is disabled. Nothing is granted until that button is pressed.
  ambiguous "매일 8시에 ..." (no 오전/오후/아침) -> clock question, no card, no grant
  negated   "... 알려주지 마" -> not a request: no card, no grant (ordinary chat answer)

Acceptance ids (each prints its own PASS line; a failed assertion names the id):
  BRIEF-01 request shows the thread-bound read-only card; both Gmail and Calendar are reported missing; grant disabled
  BRIEF-02 nothing was created: zero briefs/runs/notifications/monitors/mail+calendar writes, no POST to the brief
           route, no fake success wording, no request toward Google; reload + close keep it that way
  BRIEF-03 ambiguous clock asks 오전/오후 and a negated request is plain chat: no card, no grant, no POST, no rows
  BRIEF-04 a different member's brief request leaves this member's original thread, brief log, API and database clean
"""
import re

from playwright.sync_api import expect

from member_conversation_scenarios import api

BRIEF = "매일 아침 8시에 오늘 일정과 중요한 메일을 알려줘"
AMBIGUOUS = "매일 8시에 오늘 일정과 중요한 메일을 알려줘"
NEGATED = "매일 아침 8시에 일정과 메일 알려주지 마"
ORDINARY = "오늘 아침 인사만 짧게 해 줘."
ORDINARY_ANSWER = "좋은 아침이에요"
NEGATED_ANSWER = "알겠어요"
BRIEF_ROUTE = "/personal/tasks/briefs"
GOOGLE_STATUS = ("/personal/google", "/personal/google/calendar/status")
CARD = "메일·일정 브리핑 요청 확인"
LOG = "이 대화의 메일·일정 브리핑"
GRANT = "읽기 전용으로 허용하고 시작"
SUCCESS = re.compile("시작했어요|브리핑을 (등록|만들)|허용했|연결했어요|연결되었")
NO_ROWS = ("member_connected_briefs", "member_connected_brief_runs", "local_notifications", "mail_actions",
           "member_mail_work", "member_calendar_actions")


def exercise(page, members, origin, provider, *, login, rows, passed):
    first, second = members[0], members[1]
    seen = {"post": [], "status": {}, "hosts": set()}

    def on_request(request):
        url = request.url
        host = url.split("/")[2] if "://" in url else ""
        if not url.startswith(origin):
            seen["hosts"].add(host)
        if request.method != "GET" and url.split("?")[0].endswith(BRIEF_ROUTE):
            seen["post"].append(url)

    def on_response(response):
        path = response.url.removeprefix(origin).split("?")[0]
        if path.removeprefix("/api/v1") in GOOGLE_STATUS:
            seen["status"].setdefault(path.removeprefix("/api/v1"), []).append(response.status)

    page.on("request", on_request)
    page.on("response", on_response)

    provider.turns = [([("user", ORDINARY)], ORDINARY_ANSWER), ([("user", NEGATED)], NEGATED_ANSWER)]
    provider.work_intents = {ORDINARY: {"intent": "chat"}, NEGATED: {"intent": "chat"}}
    chat_before = provider.calls

    editor = page.get_by_role("textbox", name="메시지", exact=True)
    card = page.get_by_role("region", name=CARD, exact=True)
    log = page.get_by_role("region", name=LOG, exact=True)

    def button(name):
        return page.get_by_role("button", name=name, exact=True)

    def send(text):
        editor.fill(text)
        button("보내기").click()

    def zero(member, label):
        for table in NO_ROWS:
            assert rows(member, f"SELECT count(*) FROM {table}") == [(0,)], f"{label}: {table} is not empty"
        assert rows(member, "SELECT count(*) FROM member_work_monitors WHERE kind='brief'") == [(0,)], \
            f"{label}: the brief request was misrouted to a local monitor"
        assert rows(member, "SELECT count(*) FROM member_google_grants WHERE status='active'") == [(0,)], \
            f"{label}: an active Google grant exists in the no-Google fixture"

    def no_success():
        text = page.locator("body").inner_text()
        assert not SUCCESS.search(text), "the page claims a brief was started/created/connected"

    def account_of(member):
        return rows(member, "SELECT id FROM accounts WHERE status='active'")[0][0]

    page.goto(origin)
    login(page, first, True)
    expect(editor).to_be_visible()
    account = account_of(first)

    # ---------------------------------------------------------------- BRIEF-01
    send(ORDINARY)
    expect(page.get_by_text(ORDINARY_ANSWER).first).to_be_visible(timeout=60000)
    thread = rows(first, "SELECT thread_id FROM chat_runs WHERE user_text=?", (ORDINARY,))[0][0]
    assert provider.calls == chat_before + 1, "the ordinary opening turn did not reach the model once"
    send(BRIEF)
    expect(card).to_be_visible(timeout=30000)
    expect(page.get_by_text(ORDINARY_ANSWER).first).to_be_visible()  # same thread: the card sits next to the chat
    expect(card).to_contain_text("매일 08:00")
    expect(card).to_contain_text("IMPORTANT")
    expect(card).to_contain_text("오늘 일정")
    expect(card).to_contain_text("읽기 전용")
    expect(card).to_contain_text("7일 동안")
    expect(card.get_by_role("status")).to_contain_text("Gmail·캘린더 연결이 필요해요", timeout=30000)
    expect(card.get_by_role("status")).to_contain_text("브리핑도 만들어지지 않아요")
    expect(card.get_by_role("button", name=GRANT, exact=True)).to_be_disabled()
    expect(card.get_by_role("button", name="닫기", exact=True)).to_be_enabled()
    for path in GOOGLE_STATUS:
        codes = seen["status"].get(path, [])
        assert codes and all(code == 404 for code in codes), f"{path} did not report 'not connected' ({codes})"
    assert provider.calls == chat_before + 1, "the brief request consumed a model chat turn"
    assert rows(first, "SELECT count(*) FROM chat_runs WHERE thread_id=?", (thread,)) == [(1,)], \
        "the brief request created a chat run instead of staying a card in the thread"
    passed("BRIEF-01 daily 8am schedule+important-mail request shows the read-only card in its thread; Gmail and "
           "Calendar both missing; grant button disabled")

    # ---------------------------------------------------------------- BRIEF-02
    zero(first, "BRIEF-02")
    no_success()
    assert not seen["post"], f"a POST reached the brief route without the member's grant ({len(seen['post'])})"
    assert not seen["hosts"] or all("google" not in host for host in seen["hosts"]), "a request left toward Google"
    assert rows(first, "SELECT count(*) FROM member_connected_briefs WHERE thread_id=?", (thread,)) == [(0,)]
    page.reload()
    expect(editor).to_be_visible()
    expect(log).to_have_count(0)  # no brief row, so no brief log
    zero(first, "BRIEF-02 reload")
    status, body = api(page, "/api/v1" + BRIEF_ROUTE, account)
    assert status == 200 and body["items"] == [], f"brief list is not empty after reload ({status})"
    # The same pending confirmation survives reload; closing it must not grant access.
    expect(card).to_be_visible(timeout=30000)
    card.get_by_role("button", name="닫기", exact=True).click()
    expect(card).to_have_count(0)
    no_success()
    zero(first, "BRIEF-02 close")
    assert not seen["post"], "closing the card posted to the brief route"
    passed("BRIEF-02 no brief/run/notification/monitor/mail/calendar rows, no POST, no Google traffic, no success "
           "wording, also after reload and close")

    # ---------------------------------------------------------------- BRIEF-03
    send(AMBIGUOUS)
    expect(page.get_by_text(re.compile("오전인지 오후인지 알려 주세요")).first).to_be_visible(timeout=15000)
    expect(card).to_have_count(0)
    assert provider.calls == chat_before + 1, "an ambiguous clock was sent to the model"
    zero(first, "BRIEF-03 ambiguous")
    editor.fill("")
    button("새 대화").click()
    expect(editor).to_have_value("")
    cards_before = len(seen["status"].get("/personal/google", []))
    send(NEGATED)
    expect(page.get_by_text(NEGATED_ANSWER).first).to_be_visible(timeout=60000)
    expect(card).to_have_count(0)
    assert len(seen["status"].get("/personal/google", [])) == cards_before, "a negated request opened a brief card"
    assert provider.calls == chat_before + 2, "the negated request was not plain chat"
    no_success()
    zero(first, "BRIEF-03 negated")
    assert not seen["post"], "an ambiguous or negated request posted to the brief route"
    status, body = api(page, "/api/v1" + BRIEF_ROUTE, account)
    assert status == 200 and body["items"] == []
    passed("BRIEF-03 ambiguous clock asks 오전/오후, negated request stays ordinary chat; no card, grant or rows")

    # ---------------------------------------------------------------- BRIEF-04
    other_account = account_of(second)
    page.get_by_role("button", name="대화 목록").click()
    page.get_by_role("button", name="로그아웃").click()
    expect(page.get_by_label("이메일", exact=True)).to_be_visible()
    login(page, second, True)
    expect(editor).to_be_visible()
    send(BRIEF)
    expect(card).to_be_visible(timeout=30000)
    expect(card.get_by_role("status")).to_contain_text("Gmail·캘린더 연결이 필요해요", timeout=30000)
    zero(second, "BRIEF-04 other member")
    assert not seen["post"]
    page.get_by_role("button", name="대화 목록").click()
    page.get_by_role("button", name="로그아웃").click()
    expect(page.get_by_label("이메일", exact=True)).to_be_visible()
    login(page, first, False)
    # Return to the ORIGINAL thread (the one that held the first card) through the ordinary conversation list.
    if not page.get_by_text(ORDINARY_ANSWER).first.is_visible():
        page.get_by_role("button", name="대화 목록").click()
        page.get_by_text(ORDINARY).first.click()
    expect(page.get_by_text(ORDINARY_ANSWER).first).to_be_visible(timeout=30000)
    assert rows(first, "SELECT count(*) FROM chat_runs WHERE thread_id=?", (thread,)) == [(1,)]
    expect(card).to_have_count(0)
    expect(log).to_have_count(0)
    no_success()
    status, body = api(page, "/api/v1" + BRIEF_ROUTE, account)
    assert status == 200 and body["items"] == [], "this member's brief list shows rows"
    status, body = api(page, "/api/v1" + BRIEF_ROUTE, other_account)
    leaked = (body or {}).get("items") if isinstance(body, dict) else None
    assert status != 200 or leaked == [], f"the other member's account id exposed brief data ({status})"
    zero(first, "BRIEF-04 return")
    zero(second, "BRIEF-04 return other")
    assert not seen["post"], "a brief grant was posted during the cross-member return"
    assert provider.calls == chat_before + 2
    passed("BRIEF-04 other member's brief request and account id never appear in the original thread, brief log or API; "
           "both databases hold zero brief rows")
    page.remove_listener("request", on_request)
    page.remove_listener("response", on_response)
    print(f"Brief routes: chat turns {provider.calls - chat_before}, classifier calls {provider.interpretations}, "
          f"google status checks {sum(len(v) for v in seen['status'].values())}", flush=True)
