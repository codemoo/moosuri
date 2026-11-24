"""Linux browser acceptance for a synthetic recurring public-page watch.

PW-01 checks a single durable grant and reload. PW-02 exercises one-off, negated,
quoted and private requests. PW-03 advances the isolated grant to a due slot
with no reader and checks its failed occurrence. PW-04 verifies stop, task history
and the original thread. Only the fixture's next due time is adjusted.
"""
import json
import re
import sqlite3
import time
from contextlib import closing

from playwright.sync_api import expect

from member_conversation_scenarios import api

URL = "https://status.example.test/service"
COMMAND = f"{URL} 를 매시간 확인하고 바뀔 때만 알려줘"
ONE_OFF = f"{URL} 지금 한 번 확인해줘"
NEGATED = f"{URL} 를 매시간 확인하고 바뀔 때만 알려주지 마"
QUOTED = f"“{URL} 를 매시간 확인하고 바뀔 때만 알려줘”라는 말은 무슨 뜻이야?"
PRIVATE = "http://192.168.0.10/status 를 매시간 확인하고 바뀔 때만 알려줘"
TOKEN = f"{URL}?token=abc123 를 매시간 확인하고 바뀔 때만 알려줘"
ORDINARY = "오늘 아침 인사만 짧게 해 줘."
ANSWERS = {ORDINARY: "좋은 아침이에요", ONE_OFF: "알겠어요 한번만이군요", NEGATED: "알겠어요 하지 않을게요", QUOTED: "알겠어요 인용으로 볼게요"}
ROUTE = "/personal/tasks/public-watches"
LOG = "이 대화의 공개 페이지 감시"
REFUSED = "감시를 만들지 않았어요"


def exercise(page, members, origin, provider, *, login, rows, passed):
    first = members[0]
    seen = {"post": [], "stop": []}

    def on_request(request):
        path = request.url.removeprefix(origin).split("?")[0].removeprefix("/api/v1")
        if request.method == "GET" or not path.startswith(ROUTE):
            return
        body = json.loads(request.post_data or "null")
        (seen["stop"] if path.endswith("/stop") else seen["post"]).append((path, body))

    page.on("request", on_request)

    turns = [ORDINARY, NEGATED, QUOTED]
    provider.turns = []
    history = []
    for text in turns:
        history.append(("user", text))
        provider.turns.append((history.copy(), ANSWERS[text]))
        history.append(("assistant", ANSWERS[text]))
    provider.work_intents = {text: {"intent": "chat"} for text in turns}
    chat_before = provider.calls

    editor = page.get_by_role("textbox", name="메시지", exact=True)
    log = page.get_by_role("region", name=LOG, exact=True)

    def button(name):
        return page.get_by_role("button", name=name, exact=True)

    def send(text):
        editor.fill(text)
        button("보내기").click()

    def tables():
        names = [r[0] for r in rows(first, "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%public_watch%'")]
        assert names, "no public-watch table exists"
        return names

    def watch_table():
        found = [n for n in tables() if not re.search("check|run|slot", n)]
        assert len(found) == 1, f"cannot identify the watch table among {len(tables())} tables"
        return found[0]

    def watches(thread):
        table = watch_table()
        columns = [c[1] for c in rows(first, f"PRAGMA table_info({table})")]
        for needed in ("state", "thread_id", "url", "every_seconds", "revision"):
            assert needed in columns, f"watch table lacks column {needed}"
        return [dict(zip(columns, r)) for r in rows(first, f"SELECT {','.join(columns)} FROM {table} WHERE thread_id=?", (thread,))]

    def total_watches():
        return rows(first, f"SELECT count(*) FROM {watch_table()}")[0][0]

    def notifications():
        return rows(first, "SELECT count(*) FROM local_notifications")[0][0]

    def account_id():
        return rows(first, "SELECT id FROM accounts WHERE status='active'")[0][0]

    def listed(thread):
        status, body = api(page, f"/api/v1{ROUTE}?thread_id={thread}", account)
        assert status == 200 and isinstance(body, dict), f"watch list answered {status}"
        return body["items"]

    page.goto(origin)
    login(page, first, True)
    expect(editor).to_be_visible()
    account = account_id()

    # ---------------------------------------------------------------- PW-01
    send(ORDINARY)
    expect(page.get_by_text(ANSWERS[ORDINARY]).first).to_be_visible(timeout=60000)
    thread = rows(first, "SELECT thread_id FROM chat_runs WHERE user_text=?", (ORDINARY,))[0][0]
    assert provider.calls == chat_before + 1, "the opening turn did not reach the model once"
    assert total_watches() == 0, "a watch exists before any command"
    send(COMMAND)
    expect(log).to_be_visible(timeout=30000)
    expect(log).to_contain_text(URL)
    expect(log).to_contain_text("1시간마다")
    expect(log).to_contain_text("바뀐 경우에만")
    expect(log).to_contain_text("진행 중")
    expect(log.get_by_role("button", name="멈추기", exact=True)).to_be_visible()
    assert provider.calls == chat_before + 1, "the command consumed a model chat turn"
    assert len(seen["post"]) == 1, f"expected exactly one create POST, saw {len(seen['post'])}"
    path, body = seen["post"][0]
    assert path == ROUTE, "create POST went to the wrong route"
    assert sorted(body) == ["days", "every_seconds", "message", "request_id", "thread_id", "url"], "unexpected create body keys"
    assert body["thread_id"] == thread and body["url"] == URL and body["every_seconds"] == 3600 and body["days"] == 7
    assert body["message"] == COMMAND and body["request_id"], "create body lost the command or request id"
    found = watches(thread)
    assert len(found) == 1 and found[0]["url"] == URL and found[0]["every_seconds"] == 3600 and found[0]["state"] == "active", \
        "the watch row does not match the command"
    assert total_watches() == 1
    page.reload()
    expect(editor).to_be_visible()
    expect(log).to_contain_text(URL, timeout=30000)
    assert len(seen["post"]) == 1, "reload reposted the command"
    assert len(watches(thread)) == 1
    items = listed(thread)
    assert len(items) == 1 and items[0]["thread_id"] == thread and items[0]["url"] == URL
    watch_id, revision = items[0]["id"], items[0]["revision"]
    passed("PW-01 hourly change-only command creates one thread-bound watch: one POST, one row, log shows the exact "
           "URL, no model turn, no repost after reload")

    # ---------------------------------------------------------------- PW-02
    posts_before, rows_before = len(seen["post"]), total_watches()
    send(ONE_OFF)
    expect(page.get_by_role("alert").filter(has_text="브라우저 작업을 사용할 수 없습니다")).to_be_visible(timeout=15000)
    send(NEGATED)
    expect(page.get_by_text(ANSWERS[NEGATED]).first).to_be_visible(timeout=60000)
    send(QUOTED)
    expect(page.get_by_text(ANSWERS[QUOTED]).first).to_be_visible(timeout=60000)
    assert provider.calls == chat_before + 3, "one-off/negated/quoted were not plain chat"
    for text in (PRIVATE, TOKEN):
        send(text)
        expect(page.get_by_role("alert").filter(has_text=REFUSED)).to_be_visible(timeout=15000)
    assert provider.calls == chat_before + 3, "a refused address reached the model"
    assert len(seen["post"]) == posts_before, "a refused or plain message posted a watch"
    assert total_watches() == rows_before, "a refused or plain message created a watch row"
    assert len(listed(thread)) == 1
    assert not re.search("192\\.168|abc123", " ".join(str(r) for r in rows(first, f"SELECT * FROM {watch_table()}"))), \
        "a refused address was stored"
    passed("PW-02 one-off uses normal browser, negated and quoted text stay ordinary chat; private and token addresses are refused locally; "
           "no POST and no new row")

    # ---------------------------------------------------------------- PW-03
    notes_before = notifications()
    # Only this synthetic grant is moved forward. The real installation timer claims the slot.
    with closing(sqlite3.connect(first[3])) as timer:
        timer.execute("UPDATE member_public_watches SET next_due=? WHERE id=? AND state='active'", (int(time.time()), watch_id))
        timer.commit()
    page.reload()
    expect(editor).to_be_visible()
    expect(log).to_contain_text("확인하지 못했어요", timeout=60000)
    expect(log).not_to_contain_text("바뀌었어요")
    checks = listed(thread)[0]["checks"]
    states = [c["state"] for c in checks]
    assert checks and any(state in ("failed", "unavailable") for state in states), f"no unavailable check recorded ({len(checks)} checks)"
    assert "changed" not in states, "an unreadable page was recorded as changed"
    assert all(not c.get("excerpt") for c in checks if c["state"] in ("failed", "unavailable")), "an unavailable check carries text"
    assert notifications() == notes_before, "an unreadable page produced a change notification"
    passed("PW-03 no active reader: the due check is recorded unavailable, never changed; no notification, no excerpt, "
           "UI says it could not read")

    # ---------------------------------------------------------------- PW-04
    revision = listed(thread)[0]["revision"]
    task_status, detail = api(page, "/api/v1/personal/tasks/" + watch_id, account)
    assert task_status == 200 and detail["task"]["kind"] == "public_watch" and detail["request"] == COMMAND
    assert rows(first, "SELECT request_id FROM chat_runs WHERE id=? AND thread_id=?", (detail["origin_turn_id"], thread)) == [(watch_id,)]
    assert any(step["state"] in ("failed", "unavailable") for step in detail["history"])
    stops_before = len(seen["stop"])
    log.get_by_role("button", name="멈추기", exact=True).click()
    expect(log).to_contain_text("중지됨", timeout=30000)
    expect(log.get_by_role("button", name="멈추기", exact=True)).to_have_count(0)
    assert len(seen["stop"]) == stops_before + 1, "stop did not post exactly once"
    stop_path, stop_body = seen["stop"][-1]
    assert stop_path == f"{ROUTE}/{watch_id}/stop" and stop_body["revision"] == revision and stop_body["request_id"], \
        "stop body does not carry the shown revision"
    assert sorted(stop_body) == ["request_id", "revision"]
    assert watches(thread)[0]["state"] == "stopped", "the row is not stopped"
    page.reload()
    expect(editor).to_be_visible()
    expect(log).to_contain_text("중지됨", timeout=30000)
    expect(log.get_by_role("button", name="멈추기", exact=True)).to_have_count(0)
    page.get_by_role("button", name="대화 목록").click()
    dialog = page.get_by_role("dialog", name="대화 목록")
    dialog.get_by_role("searchbox").fill(ORDINARY)
    dialog.get_by_role("button", name=ORDINARY, exact=True).click()
    expect(dialog).not_to_be_visible()
    expect(log).to_contain_text("중지됨", timeout=30000)
    expect(log.get_by_role("button", name="멈추기", exact=True)).to_have_count(0)
    items = listed(thread)
    assert len(items) == 1 and items[0]["state"] == "stopped" and items[0]["id"] == watch_id
    assert len(seen["stop"]) == stops_before + 1 and len(seen["post"]) == posts_before, "reopening posted again"
    assert total_watches() == 1
    passed("PW-04 stop posts once with the shown revision; reload and reopening the conversation show it stopped with "
           "no stop button and no new request")
    page.remove_listener("request", on_request)
    print(f"Public watch routes: chat turns {provider.calls - chat_before}, create posts {len(seen['post'])}, "
          f"stop posts {len(seen['stop'])}", flush=True)


def exercise_read(page, members, origin, provider, broker, *, login, rows, passed):
    """Run the real installation timer and member reader against a changing synthetic public page."""
    member = members[0]
    broker.set_document(URL, "<html><title>Service</title><body>정상 운영</body></html>")
    page.goto(origin)
    login(page, member, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    before = provider.calls
    editor.fill(f"{URL} 바뀌면 알려줘")
    page.get_by_role("button", name="보내기", exact=True).click()
    log = page.get_by_role("region", name=LOG, exact=True)
    expect(log).to_contain_text(URL, timeout=30000)
    watch, cadence, days = rows(member, "SELECT id,every_seconds,days FROM member_public_watches")[0]
    assert (cadence, days) == (3600, 7)
    expect(log).to_contain_text("1시간마다")

    def wait_state(state, count):
        until = time.monotonic() + 45
        query = "SELECT state FROM member_public_watch_checks WHERE watch_id=? ORDER BY due_at"
        while time.monotonic() < until:
            states = [r[0] for r in rows(member, query, (watch,))]
            if len(states) == count and states[-1] == state:
                return
            page.wait_for_timeout(120)
        raise AssertionError(f"expected public check {count}/{state}, observed {states}")

    def make_due():
        with closing(sqlite3.connect(member[3])) as clock:
            clock.execute("UPDATE member_public_watches SET next_due=last_due+1 WHERE id=? AND state='active'", (watch,))
            clock.commit()

    def alerts():
        return rows(member, "SELECT count(*) FROM local_notifications WHERE schedule_id=?", (watch,))[0][0]

    wait_state("baseline", 1)
    page.reload()
    expect(log).to_contain_text("정상 운영")
    assert broker.count("open") == broker.count("stop") == 1 and alerts() == 0
    passed("PW-05 first timer read stores a baseline, closes its private workspace and shows the source without an alert")
    make_due()
    wait_state("unchanged", 2)
    assert alerts() == 0 and broker.count("open") == broker.count("stop") == 2
    passed("PW-06 a fresh read of the same content adds one check and zero notifications")
    broker.set_document(URL, "<html><title>Service</title><body>서비스 점검 중</body></html>")
    make_due()
    wait_state("changed", 3)
    page.reload()
    expect(log).to_contain_text("서비스 점검 중")
    make_due()
    wait_state("unchanged", 4)
    assert alerts() == 1 and broker.count("open") == broker.count("stop") == 4
    assert set(broker.ops) == {"open", "stop"} and provider.calls == before
    assert rows(member, "SELECT count(DISTINCT workspace_id), sum(closed) FROM member_public_watch_checks WHERE watch_id=?", (watch,)) == [(4,4)]
    passed("PW-07 a changed page sends one notification; next identical observation leaves it at one; four private workspaces closed")
