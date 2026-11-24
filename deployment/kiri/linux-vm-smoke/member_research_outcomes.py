"""Linux user-path acceptance for Kiri member research outcomes, connected work and short corrections.

Invoked by member-web-fullstack.py (``--research-work`` or ``--short-corrections``). Everything is synthetic
and isolated: owner volumes, gateway, model and a private same-UID browser broker socket that is passed to
the member process only by a ``test-fixtures`` build (``serve --browser-test-socket``). Nothing here
reaches a network, a Google provider, a shop or an agency; nothing is bought, submitted or sent.

The broker speaks the member browser framing of ``account_runtime/browser_wire.rs`` (one JSON line in, one
``{"ok":..,"result":..}`` line out, ``v``/``generation``/``workspace_id``/``op`` present) and serves two fixed
synthetic HTML documents. ``read_page`` is an ordinary gateway action: POST /api/v1/threads/<t>/browser. The
member service opens the page through the broker, the (synthetic) model reads it, and the member store saves
a real job + observation. This script never writes to a database; it only reads SQLite read-only.

Only aggregate results are printed (case ids, PASS/SKIP/FAIL, counts); page and member text is never printed.
"""
from datetime import date, timedelta
from html.parser import HTMLParser
import json
import re
import socket
import threading
import time
from uuid import uuid4

from playwright.sync_api import expect

SHOP_URL = "https://shop.example/offers"
NOTICE_URL = "https://grants.example/notice"
TERMINAL = ("completed", "ready", "failed", "closed")


# ----------------------------------------------------------------------------- synthetic documents
def shop_html(first_day, second_day):
    return (
        "<html><head><title>Synthetic Shop</title></head><body>"
        "<section><h1>Notebook offers</h1></section>"
        f"<section><p>Offer A: Alpha Shop</p><p>Price: $40.00</p><p>Shipping: $5.00</p><p>Delivery by: {first_day}</p></section>"
        f"<section><p>Offer B: Beta Mart</p><p>Price: $38.00</p><p>Shipping: $9.50</p><p>Delivery by: {second_day}</p></section>"
        "<section><button>Buy now</button></section></body></html>"
    )


def notice_html(deadline):
    return (
        "<html><head><title>Synthetic Notice</title></head><body>"
        "<section><h1>Funding call</h1></section>"
        "<section><p>Title: Open Science Fund</p><p>Issuer: Kfund Foundation</p>"
        f"<p>Deadline: {deadline}</p><p>Award: $20,000</p>"
        "<p>Eligibility: Registered nonprofit research groups</p>"
        "<p>Required documents: CV, proposal</p></section>"
        "<section><button>Submit application</button></section></body></html>"
    )


class _Projection(HTMLParser):
    """Reduce HTML to the 'visible layout text' the real worker projects (blocks separated by blank lines)."""

    def __init__(self):
        super().__init__()
        self.parts, self.title, self._title = [], "", False

    def handle_starttag(self, tag, attrs):
        self._title = self._title or tag == "title"

    def handle_endtag(self, tag):
        if tag == "title":
            self._title = False
        elif tag in ("p", "h1", "button"):
            self.parts.append("\n")
        elif tag == "section":
            self.parts.append("\n\n")

    def handle_data(self, data):
        if self._title:
            self.title += data
        else:
            self.parts.append(data)


def project(html):
    parser = _Projection()
    parser.feed(html)
    text = re.sub(r"\n{3,}", "\n\n", "".join(parser.parts)).strip()
    return parser.title.strip(), "Accessibility:\n{\"role\":\"document\"}\n\nVisible layout text:\n" + text


# ----------------------------------------------------------------------------- broker
def synthetic_frame_png():
    """A plain 1280×900 RGB surface for the fixture browser's read-only thumbnail."""
    import base64
    import struct
    import zlib
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    header = struct.pack(">2I5B", 1280, 900, 8, 2, 0, 0, 0)
    pixels = (b"\0" + bytes((244, 246, 248)) * 1280) * 900
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b"")
    return base64.b64encode(png).decode("ascii")


class Broker:
    """Private same-UID Unix socket broker for ONE member; serves only the fixed documents it was given."""

    OPS = {"open", "tabs", "frame", "stop"}

    def __init__(self, path, documents):
        self.path, self.documents = path, dict(documents)
        self.ops, self.rejected, self.tabs = [], 0, {}
        self.rejection_kinds = []
        self.opened_documents = []
        self.frame = synthetic_frame_png()
        self._lock = threading.Lock()
        self._listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._listener.bind(str(path))  # caller keeps umask 077 and a 0700 parent
        self._listener.listen(4)
        self._listener.settimeout(0.2)
        self._stop = False
        self._thread = threading.Thread(target=self._serve, daemon=True)
        self._thread.start()

    def close(self):
        self._stop = True
        self._thread.join(timeout=2)
        self._listener.close()

    def set_document(self, url, html):
        with self._lock:
            self.documents[url] = html

    def count(self, op):
        with self._lock:
            return self.ops.count(op)

    def _serve(self):
        while not self._stop:
            try:
                connection, _ = self._listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with connection:
                connection.settimeout(5)
                try:
                    self._handle(connection)
                except (OSError, ValueError) as error:
                    self.rejection_kinds.append(type(error).__name__)
                    with self._lock:
                        self.rejected += 1

    @staticmethod
    def _reply(connection, ok, result=None, code=None):
        body = {"ok": ok}
        if ok:
            body["result"] = result
        else:
            body["code"] = code
        connection.sendall(json.dumps(body).encode() + b"\n")

    def _handle(self, connection):
        data = b""
        while not data.endswith(b"\n"):
            chunk = connection.recv(4096)
            if not chunk:
                break
            data += chunk
        request = json.loads(data)
        valid = (data.endswith(b"\n") and request.get("v") == 1 and isinstance(request.get("generation"), int)
                 and isinstance(request.get("workspace_id"), str) and request.get("op") in self.OPS)
        if not valid:
            operation = request.get("op")
            self.rejection_kinds.append(operation if operation in ("frame", "switch", "navigate", "get", "input", "health") else "invalid envelope")
            with self._lock:
                self.rejected += 1
            self._reply(connection, False, code="invalid")
            return
        op, workspace = request["op"], request["workspace_id"]
        with self._lock:
            self.ops.append(op)
            tabs = self.tabs.setdefault(workspace, [])
            if op == "open":
                html = self.documents.get(request.get("url"))
                if html is None:  # never fetch anything that is not a fixed fixture
                    self.rejection_kinds.append("unregistered fixture URL")
                    self.rejected += 1
                    self._reply(connection, False, code="invalid")
                    return
                self.opened_documents.append(request["url"])
                title, text = project(html)
                tab = str(uuid4())
                tabs[:] = [(tab, request["url"])]
                page = {"url": request["url"], "title": title, "text": text, "links": [], "truncated": False,
                        "warning": "", "retrieved_at_unix_ms": int(time.time() * 1000)}
                self._reply(connection, True, {"tab_id": tab, "observation": {"page": page}})
            elif op == "frame":
                current = next((url for tab, url in tabs if tab == request.get("tab_id")), None)
                if current is None:
                    self._reply(connection, False, code="stale")
                else:
                    self._reply(connection, True, {"frame_token": "a" * 64, "width": 1280, "height": 900,
                                                   "png_base64": self.frame, "url": current})
            elif op == "tabs":
                self._reply(connection, True, {"active_tab": tabs[-1][0] if tabs else None,
                                               "tabs": [{"tab_id": t, "url": u} for t, u in tabs]})
            else:
                self.tabs.pop(workspace, None)
                self._reply(connection, True, {"stopped": True})


# ----------------------------------------------------------------------------- reporting
class Report:
    def __init__(self, suite, passed):
        self.suite, self._passed, self.cases = suite, passed, []

    def ok(self, case, text):
        self.cases.append((case, "PASS", ""))
        self._passed(f"{case} {text}")

    def skip(self, case, reason):
        self.cases.append((case, "SKIP", reason))
        print(f"SKIP {case}: {reason}", flush=True)

    def fail(self, case, reason):
        self.cases.append((case, "FAIL", reason))
        print(f"FAIL {case}: {reason}", flush=True)

    def aggregate(self, extra=None):
        counts = {s: sum(1 for c in self.cases if c[1] == s) for s in ("PASS", "SKIP", "FAIL")}
        summary = {"suite": self.suite, **counts, "cases": [{"id": c, "status": s, "reason": r} for c, s, r in self.cases],
                   **(extra or {})}
        print("AGGREGATE " + json.dumps(summary, sort_keys=True), flush=True)
        if counts["FAIL"] or counts["SKIP"]:
            raise AssertionError(f"{counts['FAIL']} research acceptance case(s) failed")
        return summary


def api(page, path, account, payload=None):
    return page.evaluate("""async ([path, account, payload]) => {
        const headers = {"X-Moosuri-Account": account};
        const init = {method: payload ? "POST" : "GET", headers, cache: "no-store"};
        if (payload) {
            headers["Content-Type"] = "application/json";
            init.body = JSON.stringify(payload);
        }
        const response = await fetch(path, init);
        return [response.status, await response.json().catch(() => null)];
    }""", [path, account, payload])


def make_read_page(broker, rows, member):
    """Ordinary member action: open the URL through the member gateway/browser; the saved job is the proof."""

    def read_page(page, account, thread, url):
        base = f"/api/v1/threads/{thread}/browser"
        _, current = api(page, base, account)
        workspace = (current or {}).get("workspace")
        if workspace and workspace["state"] != "closed":
            api(page, base + "/close", account, {"workspace_id": workspace["id"], "revision": workspace["revision"],
                                                 "request_id": str(uuid4())})
        request = str(uuid4())
        opens = broker.count("open")
        code, _ = api(page, base, account, {"request_id": request, "text": "가격과 마감일을 읽어 줘", "url": url,
                                            "wait_for_user": False})
        assert code in (200, 202), f"browser open HTTP {code}"
        end = time.monotonic() + 45
        workspace = None
        while time.monotonic() < end:
            _, status = api(page, base, account)
            candidate = (status or {}).get("workspace")
            if candidate and candidate["request_id"] == request and candidate["state"] in TERMINAL:
                workspace = candidate
                break
            page.wait_for_timeout(150)
        assert workspace and workspace["state"] in ("completed", "ready"), "browser read did not settle"
        run = workspace["run_id"]
        # Independent proof: a real member job and saved observation exist, and the broker was actually used.
        assert rows(member, "SELECT count(*) FROM personal_browser_jobs WHERE run_id=?", (run,)) == [(1,)]
        assert rows(member, "SELECT count(*) FROM chat_output WHERE run_id=? AND item IN "
                            "('browser-answer','browser-observation')", (run,)) == [(1,)]
        assert broker.count("open") == opens + 1
        return run

    return read_page


# ----------------------------------------------------------------------------- research + connected work
def exercise(page, members, origin, provider, broker, *, login, logout, rows, passed):
    report = Report("research-work", passed)
    first, second = members
    today = date.today()

    def day(n):
        return (today + timedelta(days=n)).isoformat()

    need_by, first_day, second_day, notice_day = day(14), day(10), day(20), day(30)
    broker.set_document(SHOP_URL, shop_html(first_day, second_day))
    broker.set_document(NOTICE_URL, notice_html(notice_day))
    read_page = make_read_page(broker, rows, first)
    request_text = "이번 달 안에 도착하는 노트 가격과 배송비를 비교해 줘"
    acknowledgement = "비교할 공개 페이지 주소를 알려 주세요."
    provider.turns = [([("user", request_text)], acknowledgement)]

    page.goto(origin)
    login(page, first, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    editor.fill(request_text)
    page.get_by_role("button", name="보내기", exact=True).click()
    card = page.get_by_role("region", name="비교와 지원 준비")
    expect(card.get_by_role("button", name="URL 알려주기").first).to_be_visible(timeout=20000)
    account = rows(first, "SELECT id FROM accounts WHERE status='active'")[0][0]
    thread = rows(first, "SELECT thread_id FROM member_research_outcomes")[0][0]
    assert rows(first, "SELECT count(*) FROM member_linked_work WHERE kind='shopping'") == [(1,)]
    base = "/api/v1/personal/tasks/research-outcomes/" + thread
    report.ok("R0", "normal chat composer reaches a committed member run in SQLite")

    def create(kind, criteria, **extra):
        body = {"request_id": str(uuid4()), "kind": kind, "message": "비교해 줘", "criteria": criteria,
                "browser_runs": [], "mail_reads": [], **extra}
        status, view = api(page, base, account, body)
        return status, view, body

    def control(view, action, **extra):
        return api(page, base + "/control", account, {"request_id": view["id"], "revision": view["revision"],
                                                      "action": action, **extra})

    def case(name, text, check):
        try:
            check()
            report.ok(name, text)
        except (AssertionError, KeyError, TypeError, IndexError) as error:
            report.fail(name, type(error).__name__ + (": " + str(error)[:60] if isinstance(error, AssertionError) else ""))

    state = {}

    # R1 missing URL: ask once, reject non-public targets, then point at the existing browser.
    def missing_url():
        status, asking, _ = create("shopping", {"deadline": need_by, "currency": "USD"})
        assert status == 200 and asking["state"] == "needs_input" and asking["offers"] == []
        for bad in ("http://shop.example/", "https://127.0.0.1/", "https://localhost/x", "file:///etc/passwd"):
            assert control(asking, "answer", text=bad, question_id=asking["question"]["id"])[0] == 400
        code, asked = control(asking, "answer", text=SHOP_URL, question_id=asking["question"]["id"])
        assert code == 200 and asked["state"] == "needs_source" and asked["next"]["kind"] == "open_browser"
        assert broker.count("open") == 0, "no page may be read before the member opens it"
    case("R1", "missing URL asks once, rejects non-public targets, then points at the existing browser", missing_url)

    # R2 shopping offer by deadline from a REAL saved read through the gateway, broker and model.
    try:
        shop_run = read_page(page, account, thread, SHOP_URL)
    except AssertionError as error:
        report.fail("R2", "read_page: " + str(error)[:60])
        shop_run = None
    if shop_run:
        def offers():
            status, view, body = create("shopping", {"deadline": need_by, "currency": "USD"})
            assert status == 200 and view["state"] == "ranked", view["state"]
            by = {o["seller"]: o for o in view["offers"]}
            alpha, beta = by["Alpha Shop"], by["Beta Mart"]
            assert alpha["price"]["minor"] == 4000 and alpha["shipping"]["minor"] == 500 and alpha["total"]["minor"] == 4500
            assert beta["price"]["minor"] == 3800 and beta["shipping"]["minor"] == 950 and beta["total"]["minor"] == 4750
            assert alpha["verdict"] == "best" and beta["verdict"] == "late"
            assert alpha["delivery_by"] == first_day and beta["delivery_by"] == second_day
            assert any(c["run_id"] == shop_run and c["status"] == "current" for c in view["citations"])
            assert view["receipts"] == [] and view["draft"] is None
            assert api(page, base, account, body)[1]["id"] == view["id"]  # replay is the same outcome
            assert api(page, base, account, {**body, "message": "다른 요청"})[0] == 409
            state["shopping"] = view
        case("R2", "purchase offer by deadline: prices, fees, totals, late vs best, citation, replay and conflict", offers)

        def changed_deadline():
            view = state["shopping"]
            tight = {"deadline": day(5), "currency": "USD"}
            code, moved = control(view, "refine", criteria=tight)
            assert code == 200 and moved["revision"] == view["revision"] + 1
            assert moved["state"] == "no_match" and all(o["verdict"] != "best" for o in moved["offers"])
            assert control(view, "refine", criteria=tight)[1]["revision"] == moved["revision"]  # replay
            assert control(view, "refine", criteria={**tight, "budget_minor": 1, "currency": "USD"})[0] == 409
            assert moved["receipts"] == []
            state["moved"] = moved
        case("R3", "changed deadline re-ranks the same saved read; replay is idempotent, conflict refused", changed_deadline)

        # R4 reload re-derives the outcome from the server; the browser record is untouched.
        def reload_headings():
            page.reload()
            expect(editor).to_be_visible()
            status, listing = api(page, base, account)
            assert status == 200 and listing["thread_id"] == thread
            again = {o["id"]: o for o in listing["outcomes"]}[state["shopping"]["id"]]
            assert again["revision"] == state["moved"]["revision"]
            assert rows(first, "SELECT count(*) FROM personal_browser_jobs") == [(1,)]
        case("R4", "reload re-reads the same outcome revision from the server", reload_headings)
    else:
        for name in ("R3", "R4"):
            report.skip(name, "no saved shop read (R2 failed)")

    # R5 UI controls: the member UI shows the outcome card only if the app shell integrates it.
    region = page.get_by_role("region", name="비교와 지원 준비")
    if region.count() == 0:
        report.skip("R5", "prerequisite absent: MemberResearchOutcomes card is not mounted in the app shell")
    else:
        def ui():
            expect(region).to_be_visible()
            expect(page.get_by_text("판매 조건 비교").first).to_be_visible()
            page.get_by_role("button", name="전체 다시 읽기").click()
            page.reload()
            expect(page.get_by_role("region", name="비교와 지원 준비")).to_be_visible()
        case("R5", "outcome card is shown through real controls and survives reload", ui)

    # R6 application: eligibility/deadline from a real notice read; missing mail stated; draft is not submission.
    try:
        notice_run = read_page(page, account, thread, NOTICE_URL)
    except AssertionError as error:
        report.fail("R6", "read_page: " + str(error)[:60])
        notice_run = None
    if notice_run:
        def application():
            status, app, _ = create("application", {"terms": ["fund"]}, browser_runs=[notice_run])
            assert status == 200, status
            keys = {f["key"]: f["value"] for f in app["facts"]}
            assert {"title", "issuer", "deadline", "eligibility"} <= set(keys), sorted(keys)
            assert "nonprofit" in keys["eligibility"].lower() and "Kfund" in keys["issuer"]
            assert any(m["code"] == "mail_not_read" for m in app["missing"])
            assert not any(c["kind"] == "mail" for c in app["citations"])
            assert app["state"] not in ("submission_observed", "confirmation_observed") and app["receipts"] == []
            code, drafted = control(app, "draft")
            assert code == 200 and drafted["receipts"] == []
            if drafted["draft"]:
                assert drafted["draft"]["submitted"] is False and "제출 완료" not in drafted["draft"]["body"]
            # The page showed a 'Submit application' button but no receipt: it must never read as submitted.
            assert drafted["state"] not in ("submission_observed", "confirmation_observed")
            state["application"], state["deadline_fact"] = drafted, keys["deadline"]
        case("R6", "application eligibility, issuer and deadline are cited; missing mail stated; draft is not 'submitted'", application)

        def deadline_moves():
            broker.set_document(NOTICE_URL, notice_html(day(-2)))
            newer = read_page(page, account, thread, NOTICE_URL)
            status, app, _ = create("application", {"terms": ["fund"]}, browser_runs=[newer])
            assert status == 200 and app["state"] == "deadline_passed", app["state"]
            assert any(m["code"] == "deadline_passed" for m in app["missing"]) and app["receipts"] == []
            assert {f["key"]: f["value"] for f in app["facts"]}["deadline"] != state["deadline_fact"]
            # The older outcome stays pinned to the page it actually read.
            old = control(state["application"], "refresh")[1]
            assert {f["key"]: f["value"] for f in old["facts"]}["deadline"] == state["deadline_fact"]
        case("R7", "a changed notice deadline is a new cited read; the earlier outcome keeps its own source", deadline_moves)

        def source_closed():
            # Closing the browser workspace is not withdrawal: the saved observation remains the evidence.
            listing = api(page, base, account)[1]
            assert all(c["status"] == "current" for o in listing["outcomes"] if o["state"] != "closed"
                       for c in o["citations"])
        case("R8", "browser close leaves saved sources current (no silent loss of evidence)", source_closed)
    else:
        for name in ("R7", "R8"):
            report.skip(name, "no saved notice read (R6 failed)")


    # R10 provider-unavailable honesty through ordinary gateway routes (no Google grant in this fixture).
    def providers():
        states = {}
        probes = (("mail_status", "/api/v1/personal/google", None),
                  ("calendar_status", "/api/v1/personal/google/calendar/status", None),
                  ("mail_inbox", "/api/v1/personal/google/inbox", {"request_id": str(uuid4())}))
        for name, path, body in probes:
            code, view = api(page, path, account, body)
            text = json.dumps(view or {})
            assert not (code == 200 and '"messages"' in text and '"messages": []' not in text), name
            assert not (code == 200 and '"connected": true' in text), name
            states[name] = code
        state["providers"] = states
        listing = api(page, base, account)[1]
        assert all(not any(c["kind"] == "mail" for c in o["citations"]) for o in listing["outcomes"])
        assert any(m["code"] == "mail_not_read" for o in listing["outcomes"] if o["kind"] == "application"
                   for m in o["missing"])
    case("R10", "mail/calendar without a usable grant report non-success and never become evidence", providers)

    # R11 wrong member / thread isolation, checked in the API and in SQLite.
    def isolation():
        before = rows(first, "SELECT count(*) FROM member_research_outcomes")
        assert api(page, "/api/v1/personal/tasks/research-outcomes/" + str(uuid4()), account)[0] in (403, 404)
        logout(page)
        login(page, second, True)
        assert api(page, base, account)[0] in (401, 403, 404)  # second member's session, first member's header
        second_account = rows(second, "SELECT id FROM accounts WHERE status='active'")[0][0]
        assert api(page, base, second_account)[0] in (403, 404)
        assert api(page, f"/api/v1/threads/{thread}/browser", second_account)[0] in (403, 404)
        code, _ = api(page, f"/api/v1/threads/{thread}/browser", second_account,
                      {"request_id": str(uuid4()), "text": "읽어 줘", "url": SHOP_URL, "wait_for_user": False})
        assert code in (400, 403, 404, 503)
        assert rows(second, "SELECT count(*) FROM member_research_outcomes") == [(0,)]
        assert rows(second, "SELECT count(*) FROM personal_browser_jobs") == [(0,)]
        assert rows(first, "SELECT count(*) FROM member_research_outcomes") == before
        logout(page)
        login(page, first, False)
    case("R11", "second member cannot read, create or open anything in the first member's thread", isolation)

    # R12 withdrawal and what is stored.
    def withdraw():
        listing = api(page, base, account)[1]["outcomes"]
        target = {o["id"]: o for o in listing}[state["shopping"]["id"]]
        code, closed = control(target, "withdraw")
        assert code == 200 and closed["state"] == "closed" and closed["offers"] == []
        assert control(closed, "refresh")[0] == 409
        assert rows(first, "SELECT count(*) FROM personal_browser_jobs WHERE run_id=?", (shop_run,)) == [(1,)]
        stored = rows(first, "SELECT group_concat(title||message||criteria) FROM member_research_outcomes")[0][0] or ""
        refs = rows(first, "SELECT group_concat(kind||ref_id||coalesce(host,'')||digest) "
                           "FROM member_research_outcome_sources")[0][0] or ""
        for secret in ("Beta Mart", "Alpha Shop", "Delivery by", "nonprofit"):
            assert secret not in stored and secret not in refs, "page text persisted in outcome tables"
        assert rows(first, "SELECT count(*) FROM chat_output WHERE item LIKE '%research%'") == [(0,)]
    if shop_run and "shopping" in state:
        case("R12", "withdrawal closes the outcome, keeps the browser record; outcome tables hold references only", withdraw)
    else:
        report.skip("R12", "no shopping outcome was created (R2 failed)")

    def revoke():
        # Rotation invalidates previous-epoch source authority even when the same member signs back in.
        assert state["application"]["facts"]
        code, _ = api(page, "/api/v1/auth/password", account, {"password": "New-rotation-" + str(uuid4())})
        assert code == 200, ("rotation", code)
        status, page_after = api(page, base, account)
        assert status == 200
        again = next(o for o in page_after["outcomes"] if o["id"] == state["application"]["id"])
        assert again["state"] == "closed" and not any(again[k] for k in ("citations","facts","receipts","draft"))
    case("R9", "password rotation fences previously readable application evidence on reopening", revoke)

    # R13 the broker was used honestly: only fixtures, only read-only ops, nothing purchased or submitted.
    def broker_ledger():
        assert broker.rejected == 0, ("broker rejected", broker.rejection_kinds)
        assert set(broker.ops) <= {"open", "tabs", "frame", "stop"} and broker.count("open") >= 3
    case("R13", "broker served only fixed documents with read-only operations (no input, click or form)", broker_ledger)

    return report.aggregate({"broker_opens": broker.count("open"), "provider_probe_codes": state.get("providers", {}),
                             "model_reads": provider.browser_reads})


# ----------------------------------------------------------------------------- short corrections
def exercise_corrections(page, members, origin, provider, *, login, rows, passed):
    """Ordinary conversation only: each short correction must carry the earlier turns to the model."""
    report = Report("short-corrections", passed)
    first = members[0]
    turns = [("회의는 세 시야.", "알겠습니다. 세 시로 기억해 둘게요."),
             ("아니, 네 시.", "네 시로 고쳤습니다."),
             ("다시, 세 시 반.", "세 시 반으로 고쳤습니다.")]
    history, expected = [], []
    for prompt, answer in turns:
        expected.append(([*history, ("user", prompt)], answer))
        history += [("user", prompt), ("assistant", answer)]
    provider.turns = expected
    page.goto(origin)
    login(page, first, True)
    editor = page.get_by_role("textbox", name="메시지", exact=True)
    for prompt, answer in turns:
        editor.fill(prompt)
        editor.press("Enter")
        expect(page.get_by_text(answer, exact=True)).to_be_visible(timeout=20000)
    try:
        saved = rows(first, "SELECT r.user_text,o.text,r.state FROM chat_runs r JOIN chat_output o "
                            "ON o.run_id=r.id AND o.tenant_id=r.tenant_id ORDER BY r.sequence")
        assert saved == [(p, a, "completed") for p, a in turns]
        assert provider.calls == 3  # the provider verified the exact prior-turn history of every correction
        counts = rows(first, "SELECT p.context_messages FROM personal_text_previews p JOIN chat_runs r "
                             "ON r.tenant_id=p.tenant_id AND r.id=p.run_id ORDER BY r.sequence")
        assert counts == [(0,), (2,), (4,)], counts
        page.reload()
        expect(page.get_by_text(turns[-1][1], exact=True)).to_be_visible()
        report.ok("C1", "short corrections keep earlier turns as model context, committed in SQLite, visible after reload")
    except AssertionError as error:
        report.fail("C1", "assertion: " + str(error)[:60])
    return report.aggregate({"model_calls": provider.calls})
