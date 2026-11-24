"""Linux user-path acceptance of whole Korean research journeys through the ordinary member chat UI.

Run with member-web-fullstack.py --research-journey. Each step uses the member chat, a visible
control, SQLite or the read-only fixture ledger. The shopping card is also exercised at 390px.

Fixed synthetic public URLs and documents come from member_research_outcomes (SHOP_URL / NOTICE_URL); the broker only
serves those and only answers open/tabs/frame/stop. Nothing is bought, submitted or sent; no Google or shop is contacted.
SQLite is only opened read-only (``rows``). Only ids, counts and PASS/FAIL are printed, never page or member text.

Journeys (each named, each in its own conversation):
  J1 shopping_ask_opens_and_links_page   natural ask with URL + date; page opened by the UI; offers/deadlines at 390px
  J2 worklog_exact_task_and_return       worklog row is the exact linked task; reload; back to the original turn
  J3 grant_facts_missing_mail_draft_only explicit notice URL; cited facts; mailbox honestly missing; draft not submitted
  J4 missing_url_repair_and_refine       no URL: bad URLs refused, answer in the same card, open, refine currency/budget/expiry
  J6 public_search_scope               only the requested search topic leaves the chat; a snippet cannot confirm a price
  J5 read_only_ledger                    broker ops, model reads and provider-side tables over the whole run

J1 checks that all offer details fit the mobile viewport. J2 checks that task history uses
localized event labels and can return to the original chat turn.
"""
import json
import re
import time
from datetime import date, timedelta
from uuid import uuid4

from playwright.sync_api import Error as PlaywrightError, expect

from member_research_outcomes import NOTICE_URL, SHOP_URL, Report, api, notice_html, shop_html

READ_ONLY_OPS = {"open", "tabs", "frame", "stop"}
LINKED_STATES = {"ready", "needs_input", "refresh", "refine", "answer", "draft", "withdraw"}
FINAL_CLAIMS = ("제출 완료", "접수 완료", "주문 완료", "결제 완료", "구매 완료", "발송 완료")
ACTION_BUTTONS = re.compile("구매|결제|주문|제출|전송|발송|신청하기")
DESKTOP = {"width": 1200, "height": 840}
PHONE = {"width": 390, "height": 844}


def exercise(page, members, origin, provider, broker, *, login, rows, passed):
    report = Report("research-journey", passed)
    first = members[0]
    today = date.today()

    def day(n):
        return today + timedelta(days=n)

    need_by, first_day, second_day, notice_day = (day(n).isoformat() for n in (14, 10, 20, 30))
    broker.set_document(SHOP_URL, shop_html(first_day, second_day))
    broker.set_document(NOTICE_URL, notice_html(notice_day))
    # Research messages take the client research route (no model chat turn). Armed only as a safety net;
    # the number of chat/classifier calls actually made is reported at the end instead of assumed.
    provider.turns = []

    page.goto(origin)
    login(page, first, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    region = page.get_by_role("region", name="비교와 지원 준비")
    account = rows(first, "SELECT id FROM accounts WHERE status='active'")[0][0]
    state = {"used": False, "last_thread": None, "opens": 0}

    # ------------------------------------------------------------------ small helpers
    def button(name):
        return page.get_by_role("button", name=name, exact=True)

    def tally():
        return {"ops": len(broker.ops), "opens": broker.count("open"), "reads": provider.browser_reads,
                "rejected": broker.rejected}

    def release(thread):
        """Hygiene only: free the previous journey's browser workspace via the ordinary close route."""
        path = f"/api/v1/threads/{thread}/browser"
        _, current = api(page, path, account)
        workspace = (current or {}).get("workspace")
        if workspace and workspace["state"] != "closed":
            api(page, path + "/close", account, {"workspace_id": workspace["id"], "revision": workspace["revision"],
                                                 "request_id": str(uuid4())})

    def new_conversation():
        if state["last_thread"]:
            release(state["last_thread"])
        if state["used"]:
            button("새 대화").click()
        state["used"] = True
        expect(editor).to_be_visible()
        expect(editor).to_have_value("")

    def send(text):
        editor.fill(text)
        button("보내기").click()

    def title_of(kind, text):
        head = text.strip()[:40].replace("\n", " ").replace("\r", " ").replace("\t", " ")
        return ("구매 비교 · " if kind == "shopping" else "지원 준비 · ") + head

    def card_of(kind, text):
        return region.get_by_role("listitem", name=title_of(kind, text), exact=True)

    def outcome(text):
        found = rows(first, "SELECT id,thread_id,kind,revision,criteria,pending_url,question_id,withdrawn,draft_requested "
                            "FROM member_research_outcomes WHERE message=?", (text,))
        assert len(found) == 1, f"{len(found)} outcome rows for the journey message (want exactly 1)"
        keys = ("id", "thread_id", "kind", "revision", "criteria", "pending_url", "question_id", "withdrawn", "draft")
        out = dict(zip(keys, found[0]))
        out["criteria"] = json.loads(out["criteria"])
        out["message"] = text
        state["last_thread"] = out["thread_id"]
        return out

    def linked(outcome_id):
        found = rows(first, "SELECT id,origin_turn_id,thread_id,kind FROM member_linked_work WHERE operation_id=?",
                     (outcome_id,))
        assert len(found) == 1, f"{len(found)} linked work rows for the outcome (want exactly 1)"
        events = [e for (e,) in rows(first, "SELECT state FROM member_linked_work_events WHERE work_id=? ORDER BY sequence",
                                     (found[0][0],))]
        return {"id": found[0][0], "turn": found[0][1], "thread": found[0][2], "kind": found[0][3], "events": events}

    def actions(outcome_id):
        return rows(first, "SELECT from_revision,action FROM member_research_outcome_actions WHERE outcome_id=? "
                           "ORDER BY from_revision", (outcome_id,))

    def sources(outcome_id):
        return rows(first, "SELECT kind,ref_id,host FROM member_research_outcome_sources WHERE outcome_id=? "
                           "ORDER BY position", (outcome_id,))

    def jobs(thread):
        return rows(first, "SELECT run_id,json_extract(view,'$.url'),research_reading IS NOT NULL "
                           "FROM personal_browser_jobs WHERE thread_id=?", (thread,))

    def saved_reading(run):
        return rows(first, "SELECT group_concat(text,char(10)) FROM chat_output WHERE run_id=? AND item IN "
                           "('browser-answer','browser-observation')", (run,))[0][0] or ""

    def check_invariants(o):
        """Fresh read: revision == 1 + applied actions; linked events only the known research ones."""
        now = outcome(o["message"])
        applied = actions(o["id"])
        assert now["revision"] == 1 + len(applied), f"revision {now['revision']} != 1+{len(applied)} actions"
        events = linked(o["id"])["events"]
        assert set(events) <= LINKED_STATES, "unknown linked-work state in SQLite"
        return now

    def effects_none():
        for table in ("mail_actions", "member_mail_work", "member_calendar_actions"):
            assert rows(first, f"SELECT count(*) FROM {table}") == [(0,)], f"{table} is not empty"

    def ledger_ok(before, opens):
        after = tally()
        assert broker.rejected == 0, "broker saw a malformed/unknown frame"
        assert set(broker.ops) <= READ_ONLY_OPS, "broker saw a non-read-only op"
        assert after["opens"] - before["opens"] == opens, f"broker opens +{after['opens'] - before['opens']} want +{opens}"
        assert after["reads"] - before["reads"] == opens, "model page reads differ from page opens"
        state["opens"] += opens

    def card_text(item):
        return item.inner_text()

    def offer_cells(item, seller):
        row = item.locator("table.outcome-offers tbody tr").filter(has_text=seller)
        expect(row).to_have_count(1)
        return [c.strip() for c in row.locator("td").all_inner_texts()], row.get_attribute("data-verdict")

    def wait_state(item, label, timeout=60000):
        expect(item.locator(".outcome-state")).to_have_text(label, timeout=timeout)

    def fits(item):
        """Every element of the card must lie inside the viewport (no horizontal overflow or clipped price)."""
        page.set_viewport_size(PHONE)
        try:
            item.scroll_into_view_if_needed()
            page.wait_for_timeout(250)
            result = item.evaluate("""(el) => {
                const width = window.innerWidth, off = [];
                for (const node of el.querySelectorAll('*')) {
                    const r = node.getBoundingClientRect();
                    if (r.width > 0 && (r.right > width + 0.5 || r.left < -0.5)) off.push(node.tagName.toLowerCase());
                }
                return {width, doc: document.documentElement.scrollWidth, card: el.scrollWidth - el.clientWidth, off};
            }""")
        finally:
            page.set_viewport_size(DESKTOP)
        assert result["width"] == PHONE["width"], "viewport did not become 390px"
        assert result["doc"] <= result["width"], f"page scrolls horizontally ({result['doc']} > {result['width']}px)"
        assert result["card"] <= 1, f"card content wider than its box by {result['card']}px"
        assert not result["off"], "elements outside 390px: " + ",".join(sorted(set(result["off"]))[:8])

    def journey(name, summary, body):
        try:
            body()
        except AssertionError as error:
            report.fail(name, str(error).splitlines()[0][:160] if str(error) else "assertion")
        except PlaywrightError as error:
            report.fail(name, type(error).__name__ + ": " + (str(error).splitlines() or [""])[0][:100])
        except (KeyError, TypeError, IndexError, ValueError) as error:
            report.fail(name, type(error).__name__)
        else:
            report.ok(name, summary)

    def prerequisite(name):
        if name not in state:
            raise AssertionError(f"prerequisite journey {name} did not complete")
        return state[name]

    # ------------------------------------------------------------------ J1
    shop_text = (f"{SHOP_URL} 페이지에서 노트 가격과 배송비를 비교해서 "
                 f"{day(14).year}년 {day(14).month}월 {day(14).day}일까지 도착하는 가장 유리한 곳을 정리해 줘")

    def shopping():
        before = tally()
        new_conversation()
        send(shop_text)
        item = card_of("shopping", shop_text)
        wait_state(item, "비교 완료")  # URL in the message: opened by the UI, read, then linked by 'refresh'
        o = outcome(shop_text)
        assert o["kind"] == "shopping" and o["pending_url"] == SHOP_URL and o["question_id"] is None
        assert o["criteria"].get("deadline") == need_by and "budget_minor" not in o["criteria"], "stated date not parsed"
        link = linked(o["id"])
        assert link["kind"] == "shopping" and link["thread"] == o["thread_id"] and link["events"][0] == "ready"
        assert check_invariants(o)["revision"] >= 2, "the page link never advanced the outcome"
        ledger_ok(before, 1)

        # The page the member named is a real saved browser job, and the outcome pins exactly that run.
        job = jobs(o["thread_id"])
        assert len(job) == 1 and job[0][1] == SHOP_URL and job[0][2] == 1, "no single saved read of the named page"
        assert sources(o["id"]) == [("browser", job[0][0], "shop.example")], "outcome not linked to the observed page"
        citation = item.get_by_role("list", name="근거").locator("li")
        expect(citation).to_have_count(1)
        expect(citation).to_contain_text("Synthetic Shop")
        expect(citation).to_contain_text("shop.example")
        expect(citation).to_have_attribute("data-status", "current")

        # Actual prices and deadlines, compared with the fixture and with the saved reading in SQLite.
        saved = saved_reading(job[0][0])
        expected = {"Alpha Shop": ("40.00 USD", "5.00 USD", "45.00 USD", first_day, "$40.00", "$5.00"),
                    "Beta Mart": ("38.00 USD", "9.50 USD", "47.50 USD", second_day, "$38.00", "$9.50")}
        for seller, (price, ship, total, arrive, raw_price, raw_ship) in expected.items():
            cells, _ = offer_cells(item, seller)
            assert cells[1:5] == [price, ship, total, arrive], f"{seller} row differs from the fixture"
            assert seller in saved and raw_price in saved and raw_ship in saved and arrive in saved, \
                f"{seller} not in the saved reading"
        alpha, alpha_verdict = offer_cells(item, "Alpha Shop")
        beta, beta_verdict = offer_cells(item, "Beta Mart")
        assert alpha_verdict == "best" and "가장 유리" in alpha[5]
        assert beta_verdict == "late" and "기한 초과" in beta[5] and f"기한 {need_by} 보다 늦음" in beta[5]
        meta = item.locator(".outcome-meta")
        expect(meta).to_contain_text(f"도착 기한 {need_by}")
        expect(meta).to_contain_text("여유 4일")
        assert not any(claim in card_text(item) for claim in FINAL_CLAIMS), "card claims a final action"
        assert item.get_by_role("button", name=ACTION_BUTTONS).count() == 0, "card offers a purchase/submit action"

        fits(item)  # 390px
        state["J1"] = {"outcome": o, "link": link, "text": shop_text}

    journey("J1 shopping_ask_opens_and_links_page",
            "natural Korean ask with URL+date opened and linked the observed page; prices/deadlines match fixture "
            "and SQLite; 390px has no horizontal overflow", shopping)

    # ------------------------------------------------------------------ J2
    def worklog():
        j1 = prerequisite("J1")
        o, link = j1["outcome"], j1["link"]
        turn = page.locator("#member-turn-" + link["turn"])
        origin_text = rows(first, "SELECT user_text FROM chat_runs WHERE id=?", (link["turn"],))[0][0]
        assert origin_text == j1["text"], "origin turn is not the member's exact message"
        assert rows(first, "SELECT json_extract(model,'$.provider_id') FROM chat_runs WHERE id=?",
                    (link["turn"],)) == [("member-linked-work",)]
        soft = []
        detail = page.get_by_role("region", name="맡긴 일 상세", exact=True)

        def detail_get(identifier):
            return page.expect_response(lambda r: r.request.method == "GET"
                                        and r.url.split("?")[0].endswith("/personal/tasks/" + identifier))

        def verify(response):
            assert response.status == 200, f"task detail HTTP {response.status}"
            # Chromium may discard a response body when opening the detail replaces the view.
            status, body = api(page, response.url.removeprefix(origin), account)
            assert status == 200
            assert body["task"]["id"] == link["id"] and body["task"]["kind"] == "shopping", "not the linked task"
            assert body["origin_turn_id"] == link["turn"] and body["request"] == origin_text
            assert body["task"]["thread_id"] == link["thread"]

        def ensure_detail():
            """After a reload the detail is expected to survive; if the route does not, reopen it from the ledger."""
            try:
                expect(detail).to_be_visible(timeout=8000)
                return True
            except AssertionError:
                button("맡긴 일").click()
                ledger = page.get_by_role("region", name="맡긴 일 목록", exact=True)
                row = ledger.locator(f'[data-task-id="{link["id"]}"]')
                expect(row).to_have_count(1)
                row.click()
                expect(detail).to_be_visible()
                return False

        # 1 ordinary entry: the button on the original turn itself
        expect(turn).to_be_visible()
        with detail_get(link["turn"]) as received:
            turn.get_by_role("button", name="업무 기록 보기", exact=True).click()
        verify(received.value)
        expect(detail).to_be_visible()
        expect(detail.locator(".task-result").first).to_have_text(origin_text)
        expect(detail.locator(".task-meta")).to_contain_text("구매 비교")
        events = linked(o["id"])["events"]
        steps = detail.locator("ol.task-history li")
        expect(steps).to_have_count(len(events))
        raw = [t for t in steps.all_inner_texts() if re.search(r"[A-Za-z]{3,}", t)]
        if raw:
            soft.append(f"H2: {len(raw)} of {len(events)} worklog steps show untranslated English state tokens")

        # 2 reload on the worklog detail, then the list row is exactly this task
        page.reload()
        kept = ensure_detail()
        button("맡긴 일 목록").click()
        ledger = page.get_by_role("region", name="맡긴 일 목록", exact=True)
        row = ledger.locator(f'[data-task-id="{link["id"]}"]')
        expect(row).to_have_count(1)
        shopping_rows = rows(first, "SELECT count(*) FROM member_linked_work WHERE kind='shopping'")[0][0]
        assert ledger.locator(".task-row").filter(has_text="구매 조건 비교").count() == shopping_rows
        with detail_get(link["id"]) as received:
            row.click()
        verify(received.value)

        # 3 reload again, then "원래 대화 열기" lands on the original turn (focused and on screen)
        page.reload()
        ensure_detail()
        detail.get_by_role("button", name=re.compile("^원래 대화 열기")).click()
        expect(turn).to_be_visible(timeout=15000)
        page.wait_for_function("id => document.activeElement && document.activeElement.id === id",
                               arg="member-turn-" + link["turn"], timeout=15000)
        assert turn.evaluate("e => {const r = e.getBoundingClientRect(); return r.bottom > 0 && r.top < innerHeight}")
        expect(card_of("shopping", j1["text"]).locator(".outcome-state")).to_have_text("비교 완료", timeout=30000)
        assert rows(first, "SELECT count(*) FROM member_research_outcomes WHERE message=?", (j1["text"],)) == [(1,)]
        state["detail_survived_reload"] = kept
        if soft:
            raise AssertionError("; ".join(soft))

    journey("J2 worklog_exact_task_and_return",
            "worklog opened the exact linked task (id, kind, request, event count match SQLite), survived reloads and "
            "returned to the original turn", worklog)

    # ------------------------------------------------------------------ J3
    grant_text = f"{NOTICE_URL} 공고를 읽고 지원 자격과 마감일을 확인해서 지원서 초안을 써 줘"

    def grant():
        before = tally()
        new_conversation()
        send(grant_text)
        item = card_of("application", grant_text)
        expect(item.locator(".outcome-state")).to_have_text(re.compile("^(초안 준비됨|공고 확인됨)$"), timeout=60000)
        o = outcome(grant_text)
        assert o["kind"] == "application" and o["pending_url"] == NOTICE_URL and o["draft"] == 1
        link = linked(o["id"])
        assert link["kind"] == "application" and link["events"][0] == "ready"
        check_invariants(o)
        ledger_ok(before, 1)

        job = jobs(o["thread_id"])
        assert len(job) == 1 and job[0][1] == NOTICE_URL and job[0][2] == 1, "no single saved read of the notice"
        assert sources(o["id"]) == [("browser", job[0][0], "grants.example")]
        saved = saved_reading(job[0][0])
        facts = item.locator("dl.outcome-facts")
        expect(facts).to_be_visible()
        for value in ("Open Science Fund", "Kfund Foundation", notice_day, "nonprofit"):
            assert value.lower() in facts.inner_text().lower(), "a fact is missing from the card"
            assert value.lower() in saved.lower(), "a fact is not in the saved reading"
        assert facts.locator("dd").count() >= 4 and all("[1]" in t for t in facts.locator("dd").all_inner_texts()), \
            "a fact has no citation"
        citation = item.get_by_role("list", name="근거").locator("li")
        expect(citation).to_have_count(1)
        expect(citation).to_contain_text("Synthetic Notice")
        expect(citation).to_contain_text("grants.example")
        assert "Gmail" not in citation.inner_text()
        expect(item.locator(".outcome-meta")).to_contain_text(re.compile("마감까지 (29|30|31)일"))

        # The mailbox was never read: say so, cite no mail, and do not pretend a mail check happened.
        expect(item.get_by_role("list", name="확인되지 않은 조건")).to_contain_text("메일을 아직 읽지 않았습니다")
        assert rows(first, "SELECT count(*) FROM member_research_outcome_sources WHERE outcome_id=? AND kind='mail'",
                    (o["id"],)) == [(0,)]
        mail_offered = item.get_by_role("button", name="받은편지함 읽기", exact=True).count() == 1  # offered, never clicked

        # A draft exists and says so; nothing reads as submitted, sent or received.
        draft = item.get_by_role("region", name="지원서 초안")
        expect(draft).to_be_visible()
        expect(draft).to_contain_text("제출되지 않음")
        text = card_text(item)
        assert not any(claim in text for claim in FINAL_CLAIMS), "card claims a final action"
        expect(item.get_by_role("region", name="관찰된 확인 번호")).to_have_count(0)
        assert item.get_by_role("button", name=ACTION_BUTTONS).count() == 0, "card offers a submit/send action"
        effects_none()
        page.reload()
        expect(card_of("application", grant_text).get_by_role("region", name="지원서 초안")).to_contain_text("제출되지 않음")
        effects_none()
        assert set(linked(o["id"])["events"]) <= LINKED_STATES
        state["mail_offered"] = mail_offered

    journey("J3 grant_facts_missing_mail_draft_only",
            "notice facts are cited to the read page, missing mailbox stated, draft shown as not submitted, "
            "no provider-side action rows", grant)

    # ------------------------------------------------------------------ J4
    repair_text = "이번 달 안에 도착하는 노트 가격과 배송비를 비교해 줘"

    def repair():
        before = tally()
        new_conversation()
        send(repair_text)
        item = card_of("shopping", repair_text)
        wait_state(item, "URL이 필요합니다", 30000)
        expect(item.locator(".outcome-next")).to_contain_text("이 질문은 한 번만 합니다")
        o = outcome(repair_text)
        question = o["question_id"]
        assert question and o["pending_url"] is None and o["revision"] == 1
        assert linked(o["id"])["events"] == ["needs_input"]
        assert tally()["opens"] == before["opens"], "a page was opened before the member gave a URL"

        # Unsafe/unusable answers are refused in the same card and change nothing.
        item.get_by_role("button", name="URL 알려주기", exact=True).click()
        field = item.get_by_label("공개 페이지 URL", exact=True)
        for bad in ("http://shop.example/offers", "https://127.0.0.1/offers"):
            field.fill(bad)
            item.get_by_role("button", name="알려주기", exact=True).click()
            expect(item.get_by_role("alert")).to_contain_text("처리할 수 없습니다")
            now = outcome(repair_text)
            assert now["revision"] == 1 and now["question_id"] == question and now["pending_url"] is None
            assert actions(o["id"]) == []
        assert tally()["opens"] == before["opens"]

        # The real answer arrives through the same card: nothing is read until the member opens it.
        field.fill(SHOP_URL)
        item.get_by_role("button", name="알려주기", exact=True).click()
        wait_state(item, "읽을 페이지가 필요합니다", 30000)
        now = outcome(repair_text)
        assert now["question_id"] is None and now["pending_url"] == SHOP_URL and now["revision"] == 2
        assert actions(o["id"]) == [(1, "answer")] and linked(o["id"])["events"] == ["needs_input", "answer"]
        assert tally()["opens"] == before["opens"], "answering read the page without being asked to open it"
        item.get_by_role("button", name="브라우저로 열기", exact=True).click()
        wait_state(item, "비교 완료")
        ledger_ok(before, 1)
        done = check_invariants(now)
        assert [a for _, a in actions(o["id"])][0] == "answer" and set(a for _, a in actions(o["id"])) <= {"answer", "refresh"}
        job = jobs(o["thread_id"])
        assert len(job) == 1 and job[0][1] == SHOP_URL and sources(o["id"]) == [("browser", job[0][0], "shop.example")]
        _, alpha_verdict = offer_cells(item, "Alpha Shop")
        _, beta_verdict = offer_cells(item, "Beta Mart")
        assert (alpha_verdict, beta_verdict) == ("best", "eligible"), "no-deadline ranking is not cheapest total"
        expect(item.get_by_role("list", name="확인되지 않은 조건")).to_contain_text("도착 기한이 없어")

        # Reload: same outcome, same revision, no second outcome or second page read.
        page.reload()
        item = card_of("shopping", repair_text)
        wait_state(item, "비교 완료", 30000)
        again = outcome(repair_text)
        assert again["revision"] == done["revision"] and tally()["opens"] == before["opens"] + 1
        assert rows(first, "SELECT count(*) FROM member_linked_work WHERE operation_id=?", (o["id"],)) == [(1,)]

        # Refine through the real form. Currency defaults to KRW in the form, so a budget without USD is refused honestly.
        def refine(deadline, budget, currency):
            item.get_by_role("button", name="조건 변경", exact=True).click()
            form = item.get_by_role("form", name="조건 변경")
            form.get_by_label("도착 기한", exact=True).fill(deadline)
            form.get_by_label(re.compile("^예산")).fill(str(budget))
            form.get_by_label("통화", exact=True).fill(currency)
            form.get_by_role("button", name="조건 적용", exact=True).click()
            expect(form).to_have_count(0)
            now = check_invariants(again)
            stored = {k: now["criteria"].get(k) for k in ("deadline", "budget_minor", "currency")}
            assert stored == {"deadline": deadline, "budget_minor": budget * (1 if currency == "KRW" else 100), "currency": currency}, \
                "criteria in SQLite differ from the form"
            return now

        refine(need_by, 4600, "KRW")
        wait_state(item, "조건에 맞는 항목 없음", 30000)
        assert offer_cells(item, "Alpha Shop")[1] == "currency_mismatch" and "통화 다름" in offer_cells(item, "Alpha Shop")[0][5]
        refine(need_by, 46, "USD")
        wait_state(item, "비교 완료", 30000)
        assert offer_cells(item, "Alpha Shop")[1] == "best" and offer_cells(item, "Beta Mart")[1] == "late"
        refine(need_by, 40, "USD")  # budget below the best total
        wait_state(item, "조건에 맞는 항목 없음", 30000)
        assert offer_cells(item, "Alpha Shop")[1] == "over_budget" and "예산 초과" in offer_cells(item, "Alpha Shop")[0][5]
        assert item.locator('tr[data-verdict="best"]').count() == 0
        refine(day(5).isoformat(), 50, "USD")  # expiry earlier than any delivery
        wait_state(item, "조건에 맞는 항목 없음", 30000)
        cells, verdict = offer_cells(item, "Alpha Shop")
        assert verdict == "late" and f"기한 {day(5).isoformat()} 보다 늦음" in cells[5]
        final = refine(need_by, 46, "USD")  # repaired: the same saved read ranks again
        wait_state(item, "비교 완료", 30000)
        assert offer_cells(item, "Alpha Shop")[1] == "best"
        names = [a for _, a in actions(o["id"])]
        assert names.count("refine") == 5 and names[0] == "answer", f"unexpected action log ({len(names)} actions)"
        assert final["revision"] == 1 + len(names)
        assert tally()["opens"] == before["opens"] + 1, "a refinement re-read the page"
        assert rows(first, "SELECT count(*) FROM member_research_outcomes WHERE thread_id=?", (o["thread_id"],)) == [(1,)]
        effects_none()

    journey("J4 missing_url_repair_and_refine",
            "missing URL asked once, unsafe URLs refused unchanged, answer and open from the same card, "
            "reload idempotent, currency/budget/expiry refinements re-ranked the same read", repair)

    def public_search():
        before = tally()
        new_conversation()
        message = "‘fixture notebook’을 웹에서 검색해줘. 가격과 배송비를 비교해 줘. 내 메일은 쓰지 마."
        search = "https://duckduckgo.com/?q=fixture+notebook"
        broker.set_document(search, shop_html(first_day, second_day))
        send(message)
        item = card_of("shopping", message)
        expect(item).to_be_visible(timeout=30000)
        work = outcome(message)
        until = time.monotonic() + 35
        while time.monotonic() < until:
            opened = jobs(work["thread_id"])
            if len(opened) == 1 and opened[0][2]:
                break
            page.wait_for_timeout(100)
        assert broker.opened_documents[before["opens"]:] == [search], "only the public topic may reach the browser"
        assert len(opened) == 1 and opened[0][1:] == ("https://duckduckgo.com/", 1), "the recorded job must omit the query"
        assert all(kind == "browser" and host == "duckduckgo.com" for kind, _, host in sources(work["id"])), "a private source entered public shopping"
        expect(item.get_by_role("region", name="관찰된 확인 번호")).to_have_count(0)
        expect(item.locator(".outcome-offers")).to_have_count(0)
        ledger_ok(before, 1)
        effects_none()

    journey("J6 public_search_scope", "only the public search phrase was opened; listing copy did not become a verified seller or a receipt", public_search)

    # ------------------------------------------------------------------ J5
    def final_ledger():
        assert broker.rejected == 0, "broker saw a malformed/unknown frame"
        assert set(broker.ops) <= READ_ONLY_OPS, "broker saw a non-read-only op"
        assert broker.count("open") == state["opens"] == provider.browser_reads, \
            f"opens {broker.count('open')} / expected {state['opens']} / model reads {provider.browser_reads}"
        effects_none()
        assert rows(first, "SELECT count(*) FROM member_research_outcomes WHERE withdrawn=1") == [(0,)]
        assert rows(first, "SELECT count(*) FROM member_research_outcome_sources WHERE kind='mail'") == [(0,)]
        assert rows(first, "SELECT kind FROM member_linked_work ORDER BY admitted_order") == \
            [("shopping",), ("application",), ("shopping",), ("shopping",)], "linked work rows differ from the four conversations"

    journey("J5 read_only_ledger",
            "broker saw only open/tabs/frame/stop for exactly the journeys' pages, model reads match, no mail/calendar "
            "action rows, no withdrawn outcomes", final_ledger)

    return report.aggregate({
        "broker_opens": broker.count("open"), "broker_ops": sorted(set(broker.ops)), "broker_rejected": broker.rejected,
        "model_reads": provider.browser_reads, "model_chat_calls": provider.calls,
        "classifier_calls": provider.interpretations, "detail_survived_reload": state.get("detail_survived_reload"),
        "mail_read_offered_not_clicked": state.get("mail_offered"),
    })
