"""Real Claude grocery-list journey: one running list per chat, corrected in short messages.

Entry point: exercise(j) with a member_live_journeys.Journey. Cases GROC-01..GROC-08.
Everything here is synthetic.  No purchase, standing preference, monitor or other durable work is requested.
"""
import json
import re

from playwright.sync_api import expect

from member_live_journeys import digest

# The user explicitly asks for a bare current list so that absence of superseded content is assertable.
ONLY_NOW = " 지금 살 품목과 수량만 보여줘."
ALIASES = {"계란": ("계란", "달걀")}


def qty(*numbers, words=""):
    """Quantity pattern for digits or Korean count words directly attached to an item."""
    digits = "|".join(f"{n}(?![0-9.])" for n in numbers)
    korean = f"[{words}](?=개|팩|통|병|봉|알|모|판)" if words else ""
    return "(?:" + "|".join(p for p in (digits, korean) if p) + ")"


def norm(text):
    return re.sub(r"[\s*_`]", "", text)


def has(text, item, quantity=None):
    n = norm(text)
    names = ALIASES.get(item, (item,))
    if quantity is None:
        return any(name in n for name in names)
    return any(re.search(re.escape(name) + r"[^0-9가-힣a-zA-Z]{0,4}(?:x|×)?" + quantity, n) for name in names)


def verify(text, present=(), absent=()):
    """Entries are an item name or an (item, quantity-pattern) pair."""
    def spec(entry):
        return entry if isinstance(entry, tuple) else (entry, None)
    missing = [spec(e)[0] for e in present if not has(text, *spec(e))]
    stale = [spec(e)[0] for e in absent if has(text, *spec(e))]
    assert not missing, "list lacks current items: " + ", ".join(missing)
    assert not stale, "list still has superseded items: " + ", ".join(stale)


MILK2, MILK3 = ("우유", qty(2, words="두둘")), ("우유", qty(3, words="세삼"))
APPLE4 = ("사과", qty(4, words="네사"))
MEAT600 = ("삼겹살", r"(?:600|0\.6|육백)")
MEAT800 = ("삼겹살", r"(?:800|0\.8|팔백)")


def no_durable_work(j):
    """Ordinary chat only: no memory, monitor, or non-chat run was created."""
    assert not j.memories(), "grocery chat created a standing memory"
    assert j.rows("select count(*) from agent_dependencies where kind='memory'") == [(0,)]
    assert j.rows("select count(*) from member_work_monitors") == [(0,)]
    total = j.rows("select count(*) from chat_runs")
    ordinary = j.rows("""select count(*) from chat_runs r join personal_text_previews p
                         on p.tenant_id=r.tenant_id and p.run_id=r.id""")
    assert total == ordinary == [(j.sent,)], "a run other than an ordinary chat answer exists"


def stored(j, thread, expected_runs, output):
    """The thread holds exactly the expected ordinary answers, and the last one is the stored text checked."""
    rows = j.rows("""select o.text from chat_runs r
                     join personal_text_previews p on p.tenant_id=r.tenant_id and p.run_id=r.id
                     join chat_output o on o.tenant_id=r.tenant_id and o.run_id=r.id
                      and o.item='personal-text-preview-answer'
                     where r.thread_id=?""", (thread,))
    assert len(rows) == expected_runs, "thread run count differs from messages sent"
    assert output in [r[0] for r in rows], "stored thread outputs lack the checked answer"


def exercise(j):
    j.new_chat()

    def ask(text, present, absent=()):
        out = j.chat(text, ())  # no member memory may reach the model
        verify(out, present, absent)
        return out

    # GROC-01: start a running list, then add to the existing list.
    first = ("이 대화에서 내 장보기 목록을 계속 관리해줘. 생각날 때마다 말할 거야. 사는 건 내가 직접 하니까 목록만 정리해줘. "
             "일단 우유 2개, 계란 한 판, 대파 한 단이야." + ONLY_NOW)
    ask(first, [MILK2, "계란", "대파"])
    home = j.last_thread
    ask("아 맞다, 두부 한 모랑 김 두 봉도 넣어줘." + ONLY_NOW, [MILK2, "계란", "대파", "두부", "김"])
    assert j.last_thread == home, "addition left the list thread"
    no_durable_work(j)
    j.case("GROC-01")

    # GROC-02: a short quantity correction replaces 2 with 3 and keeps the rest.
    ask("우유는 3개로 바꿔줘." + ONLY_NOW, [MILK3, "계란", "대파", "두부", "김"], [MILK2])
    no_durable_work(j)
    j.case("GROC-02")

    # GROC-03: replace one product with another.
    ask("대파 말고 쪽파로 해줘." + ONLY_NOW, [MILK3, "계란", "쪽파", "두부", "김"], [MILK2, "대파"])
    no_durable_work(j)
    j.case("GROC-03")

    # GROC-04: remove something already owned.
    out = ask("계란은 집에 있더라. 그건 빼줘." + ONLY_NOW, [MILK3, "쪽파", "두부", "김"], [MILK2, "대파", "계란"])
    run = j.last_run
    assert j.last_thread == home
    stored(j, home, 5, out)
    no_durable_work(j)
    j.case("GROC-04")

    # GROC-05: another chat, reopen the list, continue without restating any list content.
    j.new_chat()
    other = j.chat("책상 정리 팁 한 가지만 짧게 알려줘.", ())
    assert j.last_thread != home
    j.reopen(home, run)
    follow = "아까 그 목록에 사과 네 개만 더 넣고, 지금 목록 알려줘." + ONLY_NOW
    assert not any(w in follow for w in ("우유", "쪽파", "두부", "김", "계란", "대파"))
    out_prev, out = out, ask(follow, [MILK3, "쪽파", "두부", "김", APPLE4], [MILK2, "대파", "계란"])
    assert j.last_thread == home, "continuation did not stay in the reopened list thread"
    history = j.last_wire["history"]
    assert digest(out_prev) in history and digest(other) not in history, "reopened thread history is wrong"
    run = j.last_run
    stored(j, home, 6, out)
    no_durable_work(j)
    j.case("GROC-05")

    # GROC-06: a parallel second list stays separate, and the first list is unaffected after returning.
    j.new_chat()
    ask("이건 주말 캠핑용 별도 장보기 목록이야. 삼겹살 600그램, 상추 한 봉지, 숯 하나 넣어줘." + ONLY_NOW,
               [MEAT600, "상추", "숯"], ["우유", "쪽파", "두부", "김", "사과"])
    camp_thread = j.last_thread
    assert camp_thread not in (home, None)
    camp = ask("캠핑 목록에서 삼겹살은 800그램으로 바꾸고, 상추는 집에 있으니 빼줘." + ONLY_NOW,
               [MEAT800, "숯"], [MEAT600, "상추", "우유", "쪽파", "두부", "김", "사과"])
    correction_run = j.last_run
    stored(j, camp_thread, 2, camp)
    j.reopen(home, run)
    out = ask("장보기 목록에 식빵 하나만 더 넣고, 지금 목록 알려줘." + ONLY_NOW,
              [MILK3, "쪽파", "두부", "김", APPLE4, "식빵"],
              [MILK2, "대파", "계란", "삼겹살", "상추", "숯"])
    assert j.last_thread == home
    stored(j, home, 7, out)
    assert j.sent == 10
    no_durable_work(j)
    j.case("GROC-06")

    # Real follow-ups do not repeat list instructions or specify an answer format. The next answer
    # should name what is still needed even when the correction itself got a short acknowledgement.
    j.chat("사과 두 개만. 식빵은 빼.", ())
    receipts = len(j.events)
    out = j.chat("그럼 내가 사갈 게 뭐였지?", ())
    assert [event["kind"] for event in j.events[receipts:]] == ["answer"], "recap took an extra work interpretation"
    verify(out, [MILK3, "쪽파", "두부", "김", ("사과", qty(2, words="두둘"))],
           [APPLE4, "식빵", "계란", "대파", "삼겹살", "숯"])
    assert j.last_thread == home
    stored(j, home, 9, out)
    no_durable_work(j)
    j.case("GROC-08")

    # Search from another conversation. The task must cite the actual correction, excluding the old quantity.
    j.new_chat()
    before = len(j.events)
    request = "지난 대화에서 ‘캠핑 목록에서 삼겹살’ 찾아줘"
    with j.page.expect_response(lambda r: r.request.method == "POST" and r.url.endswith("/personal/tasks/workflows")) as accepted:
        j.send(request)
    assert accepted.value.ok
    # A new conversation can navigate as soon as the POST succeeds. Its request ID and persisted
    # work record remain available even if Chromium has already discarded the response body.
    submitted = accepted.value.request.post_data_json
    assert submitted["message"] == request and submitted["thread_id"] not in (home, camp_thread)
    work = j.rows("""SELECT r.sources FROM member_work_runs r JOIN member_workflows w ON w.id=r.workflow_id
                     WHERE w.id=? AND w.thread_id=? AND w.message=? AND w.scope='conversation'""",
                  (submitted["request_id"], submitted["thread_id"], request))
    assert len(work) == 1 and [ref["id"] for ref in json.loads(work[0][0])] == [correction_run]
    j.button("업무 기록 보기").last.click()
    detail = j.page.get_by_role("region", name="맡긴 일 상세", exact=True)
    expect(detail).to_contain_text(request)
    expect(detail).to_contain_text("800그램")
    j.page.reload()
    expect(detail).to_contain_text("800그램")
    assert len(j.events) == before and j.rows("SELECT count(*) FROM member_work_runs") == [(1,)]
    j.case("GROC-07")
