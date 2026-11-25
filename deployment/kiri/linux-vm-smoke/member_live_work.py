"""Follow a real request from the composer to its sources, combined report and per-task history."""
import json
import re
import time
import uuid

from playwright.sync_api import expect

# Fixed finite deadlines (milliseconds).
GROUP_MS = 180_000
TASKS_MS = 240_000
REPORT_MS = 180_000
UI_MS = 30_000
RERUN_MS = 240_000

DEPARTURE = "05:50"


def exercise(j):
    page = j.page
    rows = j.rows
    mark = "ZQ-" + uuid.uuid4().hex[:8].upper()      # opaque synthetic marker for the wanted item
    decoy = "XD-" + uuid.uuid4().hex[:8].upper()     # opaque marker for the item the narrowing must drop

    seed = f"합성 출장 기록이야. 이번 출장 출발은 목요일 {DEPARTURE}이야. 알았다고만 짧게 답해 줘."
    delegate = ("지난 출장 대화에서 몇 시에 출발하기로 했는지 찾아 주고, 내 기억에서 ‘준비물’도 찾아 줘. "
                "두 가지 결과를 각각 따로 확인할 수 있게 하고 묶어서 보고서도 만들어 줘.")

    def poll(read, ok, ms, what):
        """Re-read SQLite until ok(value) holds; fixed deadline; failure names only the step."""
        deadline = time.monotonic() + ms / 1000
        while time.monotonic() < deadline:
            if j.provider.claude_error:
                raise RuntimeError(j.provider.claude_error)
            last = read()
            if ok(last):
                return last
            page.wait_for_timeout(250)
        raise AssertionError(f"deadline reached: {what}")

    def groups():
        return rows("SELECT id, thread_id FROM member_conversation_groups ORDER BY admitted_order")

    def tasks(gid):
        return rows("SELECT id, scope, state, revision, position FROM member_conversation_tasks "
                    "WHERE group_id=? ORDER BY position", (gid,))

    def runs(task_id):
        result = []
        for run, stored in rows("SELECT id,sources FROM member_work_runs WHERE workflow_id=? ORDER BY admitted_order", (task_id,)):
            sources = json.loads(stored)
            assert isinstance(sources, list), "work sources must be a JSON array"
            result.append((run, {(s["id"], s["revision"]) for s in sources}))
        return result

    def current_report(gid):
        return rows("SELECT r.id, (SELECT count(*) FROM member_conversation_group_report_items i WHERE i.report_id=r.id) "
                    "FROM member_conversation_group_reports r WHERE r.group_id=? AND r.superseded=0 AND r.state='ready'",
                    (gid,))

    def card(scope_label):
        return page.locator(".work-task").filter(has_text=f"{scope_label} 검색 ·")

    def open_group_report():
        opener = page.get_by_role("button", name=re.compile("묶음 보고서 (보기|열기)")).first
        expect(opener).to_be_visible(timeout=UI_MS)
        opener.click()
        region = page.get_by_role("region", name=re.compile("묶음 보고서")).first
        expect(region).to_be_visible(timeout=UI_MS)
        return region

    # ---------------------------------------------------------------- WORK-01: seed chat fact + memory items
    reply = j.chat(seed)
    assert reply.strip(), "seed chat gave an empty reply"
    assert rows("SELECT count(*) FROM chat_runs WHERE state='completed' AND user_text=?", (seed,)) == [(1,)]
    j.add_memory("출장 전자기기 준비물", f"출장 준비물로 어댑터와 보조배터리를 챙깁니다. 확인 표식은 {mark}입니다.")
    j.add_memory("출장 의류 준비물", f"출장 준비물로 회색 외투를 챙깁니다. 확인 표식은 {decoy}입니다.")
    assert rows("SELECT count(*) FROM member_conversation_groups") == [(0,)], "seeding created work before delegation"
    expected = {subject: (mid, rev) for mid, rev, subject, _ in j.memories()}
    seed_run = j.last_run
    j.case("WORK-01")

    # ---------------------------------------------------------------- WORK-02: one request, independent tasks
    j.new_chat()
    j.send(delegate)
    found = poll(groups, lambda g: len(g) >= 1, GROUP_MS, "delegated request was not recorded")
    assert len(found) == 1, f"delegation produced {len(found)} groups"
    gid, thread = found[0]
    recorded = poll(lambda: tasks(gid), lambda t: len(t) == 2, GROUP_MS, "two tasks were not recorded")
    assert sorted(t[1] for t in recorded) == ["conversation", "memory"], [t[1] for t in recorded]
    assert len({t[0] for t in recorded}) == 2
    done = poll(lambda: tasks(gid), lambda t: len(t) == 2 and all(x[2] == "completed" for x in t),
                TASKS_MS, "tasks did not both complete")
    by_scope = {t[1]: t for t in done}
    conv_id, mem_id = by_scope["conversation"][0], by_scope["memory"][0]
    conv_runs, mem_runs = runs(conv_id), runs(mem_id)
    assert len(conv_runs) == 1 and len(mem_runs) == 1, (len(conv_runs), len(mem_runs))
    assert seed_run in {mid for mid, _ in conv_runs[0][1]}, "departure source differs from the original chat"
    assert mem_runs[0][1] == set(expected.values()), "expected both original memory versions"
    expect(card("대화")).to_have_count(1, timeout=UI_MS)
    expect(card("기억")).to_have_count(1)
    expect(card("기억")).to_contain_text(mark, timeout=UI_MS)
    expect(card("대화")).to_contain_text(DEPARTURE, timeout=UI_MS)
    expect(card("기억")).to_contain_text(re.compile("근거 \\d+건"))
    expect(card("대화")).to_contain_text(re.compile("근거 \\d+건"))
    conv_text_before = card("대화").locator(".work-summary").inner_text()
    j.case("WORK-02")

    # ---------------------------------------------------------------- WORK-03: report and original worklog after refresh
    ready = poll(lambda: current_report(gid), lambda r: len(r) == 1, REPORT_MS, "combined report did not become ready")
    assert ready[0][1] == 2, f"report has {ready[0][1]} items"
    page.reload()
    region = open_group_report()
    expect(region).to_contain_text(mark, timeout=UI_MS)
    expect(region).to_contain_text(DEPARTURE)
    assert set(rows("SELECT task_id,run_id FROM member_conversation_group_report_items WHERE report_id=?", (ready[0][0],))) == {
        (mem_id, mem_runs[0][0]), (conv_id, conv_runs[0][0])}
    # The original worklog: the ledger lists the delegated task once, and its detail returns to the same thread.
    j.button("맡긴 일").click()
    ledger = page.get_by_role("region", name="맡긴 일 목록", exact=True)
    expect(ledger).to_be_visible(timeout=UI_MS)
    row = ledger.locator(f'button[data-task-id="{mem_id}"]')
    expect(row).to_have_count(1, timeout=UI_MS)
    row.click()
    detail = page.get_by_role("region", name="맡긴 일 상세", exact=True)
    expect(detail).to_be_visible(timeout=UI_MS)
    detail.get_by_role("button", name=re.compile("원래 대화 열기")).click()
    expect(card("기억")).to_have_count(1, timeout=UI_MS)
    expect(card("대화")).to_have_count(1)
    expect(card("기억")).to_contain_text(mark)
    assert groups() == [(gid, thread)], "reopening changed the group set"
    assert [t[0] for t in tasks(gid)] == [t[0] for t in done], "reopening changed the task ids"
    assert len(current_report(gid)) == 1
    assert rows("SELECT count(*) FROM member_conversation_group_reports WHERE group_id=?", (gid,)) == [(1,)]
    assert len(runs(conv_id)) == 1 and len(runs(mem_id)) == 1
    j.case("WORK-03")

    # ---------------------------------------------------------------- WORK-04: narrow one search, the other stays
    conv_before = by_scope["conversation"]
    memory_card = card("기억")
    memory_card.get_by_role("button", name="조건 좁히기", exact=True).click()
    memory_card.get_by_label("추가 조건(모두 포함)", exact=True).fill("전자기기")
    memory_card.get_by_role("button", name="좁혀서 다시 찾기", exact=True).click()
    after = poll(lambda: runs(mem_id), lambda r: len(r) == 2, RERUN_MS, "narrowed search did not record a second run")
    final = poll(lambda: tasks(gid), lambda t: len(t) == 2 and all(x[2] == "completed" for x in t),
                 RERUN_MS, "tasks did not settle after narrowing")
    now = {t[1]: t for t in final}
    assert [t[0] for t in final] == [t[0] for t in done], "narrowing changed the task set"
    assert now["conversation"] == conv_before, "the untouched task changed"
    assert runs(conv_id) == conv_runs, "the untouched task gained a run"
    assert after[0] == mem_runs[0], "the earlier occurrence of the narrowed task changed"
    assert after[1][1] == {expected["출장 전자기기 준비물"]}, "narrowed search used an unexpected source"
    expect(card("기억")).to_contain_text(mark, timeout=UI_MS)
    expect(card("기억")).not_to_contain_text(decoy)
    assert card("대화").locator(".work-summary").inner_text() == conv_text_before
    page.reload()
    expect(card("기억")).to_have_count(1, timeout=UI_MS)
    expect(card("대화")).to_have_count(1)
    expect(card("기억")).to_contain_text(mark)
    expect(card("기억")).not_to_contain_text(decoy)
    expect(card("대화")).to_contain_text(DEPARTURE)
    assert groups() == [(gid, thread)] and len(tasks(gid)) == 2 and len(runs(mem_id)) == 2 and len(runs(conv_id)) == 1
    updated = poll(lambda: current_report(gid), lambda reports: len(reports) == 1, REPORT_MS, "revised report missing")
    assert updated[0][0] != ready[0][0] and updated[0][1] == 2
    assert set(rows("SELECT task_id,run_id FROM member_conversation_group_report_items WHERE report_id=?", (updated[0][0],))) == {
        (mem_id, after[1][0]), (conv_id, conv_runs[0][0])}
    region = open_group_report()
    expect(region).to_contain_text(mark, timeout=UI_MS)
    expect(region).to_contain_text(DEPARTURE)
    expect(region).not_to_contain_text(decoy)
    j.case("WORK-04")
