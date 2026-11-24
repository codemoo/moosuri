import { StrictMode } from "react";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { dateChoices } from "./ConversationWork";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PersonalPreview from "./PersonalPreview";
import { resolveControl, type ConversationWorkGroup } from "./conversation-work";
import { parseInterpretation } from "./work-interpret";

const aid = "11111111-1111-4111-8111-111111111111";
const tid = "33333333-3333-4333-8333-333333333333";
const session = { user: { id: aid, name: "Member A", email: "a@example.test", role: "member", must_change_password: false }, password_change_required: false, chat_draft_scope: "a".repeat(64) };
const conv = `/personal/tasks/conversations/${tid}`;
const interpretPath = `/threads/${tid}/work-interpret`;
const mk = (over: any = {}) => ({
  id: "c1", title: "일정 조사", scope: "conversation", query: "일정", filter: null, state: "completed", revision: 2, question_id: null,
  run_id: "r1", runs: ["r1"], events: [], source_count: 2, summary: "3월 4일 출발", report_id: null, ...over,
});
const group = (id: string, tasks: any[], over: any = {}) => ({ id, thread_id: tid, turn_id: "run-" + id, message: "맡김", tasks, ...over });
let calls: { path: string; method: string; body: any }[];
let groups: Record<string, any>;
let turns: any[];
let reports: Record<string, any>;
let interpret: (body: any) => Response | Promise<Response>;
const receipt = (id: string) => ({
  sequence: 1, user_text: "맡김",
  assistant: [{ id: "a" + id, text: "검색을 맡겼습니다.", phase: "final_answer", refusal: false }],
  run: { id: "run-" + id, request_id: id, thread_id: tid, state: "completed", model: { provider_id: "member-conversation-work", function_tools: false }, can_retry: false, can_cancel: false },
});
function seed(...gs: any[]) {
  groups = Object.fromEntries(gs.map(g => [g.id, g]));
  turns = gs.map(g => receipt(g.id));
}
beforeEach(() => {
  sessionStorage.clear();
  calls = []; groups = {}; turns = []; reports = {};
  interpret = () => Response.json({ kind: "chat" });
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    const path = url.replace("/api/v1", "");
    const method = init.method ?? "GET";
    const body = init.body ? JSON.parse(String(init.body)) : null;
    calls.push({ path, method, body });
    if (path === "/auth/session") return Response.json(session);
    if (path === "/text-preview") return Response.json({ enabled: true, capability: "text_only_preview", tools: false, automatic_retry: false, work_interpret: true });
    if (path === "/threads" && method === "GET") return Response.json({ threads: [{ id: tid, title: "출장 준비", created_at: 1, updated_at: 1 }] });
    if (path.startsWith(`/threads/${tid}/messages`)) return Response.json({ thread_id: tid, cursor: turns.length, turns, next_before: null });
    if (path === "/personal/google/history") return Response.json({ reads: [] });
    if (path === interpretPath && method === "POST") return interpret(body);
    if (path === conv && method === "GET") return Response.json({ thread_id: tid, groups: Object.values(groups) });
    if (path.startsWith(`${conv}/reports/`)) {
      const report = reports[decodeURIComponent(path.split("/").pop()!)];
      return report ? Response.json(report) : Response.json({ code: "not_found" }, { status: 404 });
    }
    if (path === `${conv}/control` && method === "POST") {
      const g = groups[body.request_id];
      const found = g.tasks.find((t: any) => t.id === body.task_id);
      if (found.revision !== body.revision) return Response.json({ code: "stale" }, { status: 409 });
      if (body.action === "refine") { found.state = "queued"; found.filter = body.text; }
      found.revision += 1;
      return Response.json(g);
    }
    return Response.json({ code: "not_found" }, { status: 404 });
  }));
});
afterEach(() => { cleanup(); sessionStorage.clear(); vi.unstubAllGlobals(); });

const mount = () => render(<StrictMode><PersonalPreview /></StrictMode>);
const editor = () => screen.findByRole("textbox", { name: "메시지" });
const send = (user: ReturnType<typeof userEvent.setup>) => user.click(screen.getByRole("button", { name: "보내기" }));
const interprets = () => calls.filter(c => c.method === "POST" && c.path === interpretPath);
const controls = () => calls.filter(c => c.method === "POST" && c.path === `${conv}/control`);
const chats = () => calls.filter(c => c.method === "POST" && c.path.endsWith("/text-preview"));
const two = () => group("g1", [mk(), mk({ id: "c2", title: "준비물 조사", scope: "memory", query: "준비물", summary: "충전기", run_id: "r2", runs: ["r2"] })]);

describe("work-interpret contract helpers", () => {
  it("accepts only the closed alternatives for this thread", () => {
    const g = two();
    expect(parseInterpretation({ kind: "chat" }, tid)).toEqual({ kind: "chat" });
    expect(parseInterpretation({ kind: "work", group: g, focus: "evidence", task_id: "c2" }, tid)).toMatchObject({ kind: "work", focus: "evidence", task_id: "c2" });
    expect(parseInterpretation({ kind: "work", group: g, focus: "report", task_id: null }, tid)).toMatchObject({ task_id: null });
    expect(parseInterpretation({ kind: "work", group: g, focus: "evidence", task_id: "other" }, tid)).toBeNull();
    expect(parseInterpretation({ kind: "work", group: g, focus: "delete", task_id: null }, tid)).toBeNull();
    expect(parseInterpretation({ kind: "work", group: { ...g, thread_id: "x" }, focus: "tasks", task_id: null }, tid)).toBeNull();
    expect(parseInterpretation({ kind: "clarify", message: "" }, tid)).toBeNull();
    expect(parseInterpretation({ kind: "cancel", task_id: "c1" }, tid)).toBeNull();
    expect(parseInterpretation("chat", tid)).toBeNull();
  });
  it("keeps old group shapes unchanged and carries optional report fields only when present", () => {
    const plain = parseInterpretation({ kind: "work", group: two(), focus: "tasks", task_id: null }, tid) as any;
    expect("group_report_id" in plain.group).toBe(false);
    expect("report_requested" in plain.group).toBe(false);
    const rich = parseInterpretation({ kind: "work", group: { ...two(), group_report_id: "gr1", report_requested: true }, focus: "report", task_id: null }, tid) as any;
    expect(rich.group).toMatchObject({ group_report_id: "gr1", report_requested: true });
  });
  it("sends an ordinal to the server when more than one group could be meant", () => {
    const a = two() as ConversationWorkGroup, b = { ...two(), id: "g2" } as ConversationWorkGroup;
    expect(resolveControl({ kind: "refine", subject: "두 번째 건", term: "전자기기" }, [b, a]).kind).toBe("server");
    const one = resolveControl({ kind: "refine", subject: "두 번째 건", term: "전자기기" }, [a]);
    expect(one).toMatchObject({ kind: "act", action: "refine" });
    expect((one as any).targets[0].task.id).toBe("c2");
  });
});

describe("work interpretation through the chat composer", () => {
  it("resolves '두 번째 건은 전자기기만' to the visible second child, not its sibling", async () => {
    seed(two());
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    await user.type(await editor(), "두 번째 건은 전자기기만");
    await send(user);
    await waitFor(() => expect(controls()).toHaveLength(1));
    expect(controls()[0].body).toMatchObject({ request_id: "g1", task_id: "c2", revision: 2, action: "refine", text: "전자기기" });
    expect(interprets()).toHaveLength(0);
    expect(chats()).toHaveLength(0);
    expect(groups.g1.tasks[0].revision).toBe(2);
  });

  it("sends an ambiguous ordinal to the server and shows the returned group in this thread", async () => {
    seed(two(), { ...two(), id: "g2" });
    interpret = () => Response.json({ kind: "work", group: groups.g2, focus: "tasks", task_id: null });
    const user = userEvent.setup();
    mount();
    await screen.findAllByText("충전기");
    await user.type(await editor(), "두 번째 건은 전자기기만");
    await send(user);
    await waitFor(() => expect(interprets()).toHaveLength(1));
    expect(interprets()[0].body).toEqual({ request_id: expect.stringMatching(/^[0-9a-f-]{36}$/), text: "두 번째 건은 전자기기만" });
    expect(controls()).toHaveLength(0);
    expect(chats()).toHaveLength(0);
    await waitFor(() => expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue(""));
  });

  it("shows grounded evidence from the group itself for an evidence focus", async () => {
    seed(two());
    interpret = () => Response.json({ kind: "work", group: groups.g1, focus: "evidence", task_id: "c2" });
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    await user.type(await editor(), "준비물 조사 근거가 뭐였어?");
    await send(user);
    const region = await screen.findByRole("region", { name: "요청한 작업 결과" });
    expect(region).toHaveTextContent("충전기");
    expect(region).toHaveTextContent("근거 2건 (기록 당시)");
    expect(region).not.toHaveTextContent("3월 4일 출발"); // the other child is not mixed in
    expect(chats()).toHaveLength(0);
  });

  it("opens a stored combined report from group_report_id and rereads it after reload", async () => {
    seed(group("g1", two().tasks, { group_report_id: "gr1" }));
    reports.gr1 = { id: "gr1", title: "출장 종합 보고서", body: "종합", children: [{ title: "일정 조사", body: "3월 4일 출발" }, { title: "준비물 조사", body: "충전기" }] };
    interpret = () => Response.json({ kind: "work", group: groups.g1, focus: "report", task_id: null });
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    await user.type(await editor(), "전체를 보고서로 정리해줘");
    await send(user);
    const region = await screen.findByRole("region", { name: "요청한 작업 결과" });
    expect(await screen.findByRole("region", { name: "보고서: 출장 종합 보고서" })).toBeVisible();
    expect(region).toHaveTextContent("3월 4일 출발");
    expect(region).toHaveTextContent("충전기");
    const reads = () => calls.filter(c => c.path === `${conv}/reports/gr1`).length;
    const before = reads();
    expect(before).toBeGreaterThanOrEqual(1);
    // After reload the card itself offers the same single report, read live again.
    cleanup();
    mount();
    await user.click(await screen.findByRole("button", { name: "묶음 보고서 보기" }));
    expect(await screen.findByRole("region", { name: "묶음 보고서: 출장 종합 보고서" })).toBeVisible();
    expect(reads()).toBeGreaterThan(before);
  });

  it("shows progress, not an empty ticket, while a requested combined report is not stored", async () => {
    seed(group("g1", two().tasks, { report_requested: true }));
    interpret = () => Response.json({ kind: "work", group: groups.g1, focus: "report", task_id: null });
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    await user.type(await editor(), "전체 보고서 만들어줘");
    await send(user);
    const region = await screen.findByRole("region", { name: "요청한 작업 결과" });
    expect(region).toHaveTextContent("묶음 보고서를 준비하는 중입니다.");
    expect(calls.some(c => c.path.includes("/reports/"))).toBe(false);
    expect(screen.queryByRole("button", { name: "묶음 보고서 보기" })).toBeNull();
  });

  it("opens the genuine stored report of one child, and offers creation only through the existing action", async () => {
    seed(group("g1", [mk({ report_id: "r-1" }), mk({ id: "c2", title: "준비물 조사", run_id: "r2", runs: ["r2"], summary: "충전기" })]));
    reports["r-1"] = { id: "r-1", title: "일정 보고서", body: "3월 4일", run_id: "r1" };
    interpret = (body) => Response.json({ kind: "work", group: groups.g1, focus: "report", task_id: body.text.includes("일정") ? "c1" : "c2" });
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    await user.type(await editor(), "일정 조사 보고서 보여줘");
    await send(user);
    expect(await screen.findByRole("region", { name: "보고서: 일정 보고서" })).toBeVisible();
    await user.type(await editor(), "준비물 조사 보고서 보여줘");
    await send(user);
    await waitFor(() => expect(interprets()).toHaveLength(2));
    expect(await screen.findByText("이 검색의 보고서는 아직 없습니다.")).toBeVisible();
    expect(controls()).toHaveLength(0); // nothing is created without the member's explicit button
  });

  it("restores the draft on clarify, never mutates, and sends plain chat only on request", async () => {
    seed(two());
    interpret = () => Response.json({ kind: "clarify", message: "어느 건을 말씀하시나요?" });
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    await user.type(await editor(), "그 건 보고서 보여줘");
    await send(user);
    expect(await screen.findByText("어느 건을 말씀하시나요?")).toBeVisible();
    expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue("그 건 보고서 보여줘");
    expect(controls()).toHaveLength(0);
    expect(chats()).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "일반 메시지로 보내기" }));
    await waitFor(() => expect(chats()).toHaveLength(1));
    expect(interprets()).toHaveLength(1); // the explicit plain send is not interpreted again
  });

  it("falls back to ordinary chat when the server says chat or has no such route", async () => {
    const user = userEvent.setup();
    mount();
    await user.type(await editor(), "출장 근거 자료는 어디서 구해?");
    await send(user);
    await waitFor(() => expect(chats()).toHaveLength(1));
    expect(interprets()).toHaveLength(1);
    expect(chats()[0].body.request_id).toBe(interprets()[0].body.request_id);
    interpret = () => Response.json({ code: "not_found" }, { status: 404 });
    await user.clear(await editor());
    await user.type(await editor(), "출처 목록 정리 방법 알려줘");
    await send(user);
    await waitFor(() => expect(chats()).toHaveLength(2));
  });

  it("does not turn broker failure, invalid JSON or a foreign thread into success or a tool call", async () => {
    seed(two());
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    for (const bad of [
      () => Response.json({ code: "broker_unavailable" }, { status: 503 }),
      () => Response.json({ kind: "work", group: { ...groups.g1, thread_id: "44444444-4444-4444-8444-444444444444" }, focus: "tasks", task_id: null }),
      () => Response.json({ kind: "refine", task_id: "c1", text: "x" }),
    ]) {
      interpret = bad;
      const count = interprets().length;
      await user.type(await editor(), "두 번째 건 근거 보여줘");
      await send(user);
      await waitFor(() => expect(interprets()).toHaveLength(count + 1));
      expect(await screen.findByText(/해석/)).toBeVisible();
      expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue("두 번째 건 근거 보여줘");
      await user.clear(screen.getByRole("textbox", { name: "메시지" }));
    }
    expect(controls()).toHaveLength(0);
    expect(chats()).toHaveLength(0);
    expect(screen.queryByRole("region", { name: "요청한 작업 결과" })).toBeNull();
  });

  it("repeats the same request_id after a lost acknowledgement and shows the single returned group", async () => {
    seed(two());
    let first = true;
    interpret = () => {
      if (first) { first = false; throw new TypeError("lost ACK"); }
      return Response.json({ kind: "work", group: groups.g1, focus: "status", task_id: null });
    };
    const user = userEvent.setup();
    mount();
    await screen.findByText("충전기");
    await user.type(await editor(), "두 번째 건 지금 상태 알려줘");
    await send(user);
    await user.click(await screen.findByRole("button", { name: "동일 요청 다시 확인" }));
    await waitFor(() => expect(interprets()).toHaveLength(2));
    expect(interprets()[1].body).toEqual(interprets()[0].body);
    expect(await screen.findByRole("region", { name: "요청한 작업 결과" })).toHaveTextContent("준비물 조사");
    expect(chats()).toHaveLength(0);
  });

  it("sends everyday phrasing through the same member interpreter at phone width", async () => {
    vi.stubGlobal("innerWidth", 360);
    const user = userEvent.setup();
    mount();
    await user.type(await editor(), "오늘 점심 뭐 먹을까?");
    await send(user);
    await waitFor(() => expect(chats()).toHaveLength(1));
    expect(interprets()).toHaveLength(1);
  });
});

describe("departure evidence for dated trips", () => {
  const choose = "출발 시각 선택 필요: 날짜가 다른 기록 2건이 있어 하나로 정하지 않았습니다. 기록은 바꾸지 않았습니다. 어느 날짜인지 골라 주세요.\n선택 2026-03-04 · 출발 08:30 · 제주 출장\n선택 2026-04-10 · 출발 14:15 · 부산 출장\n선택 날짜 미상 · 출발 07:00 · 메모";
  const conflict = "출발 시각 충돌 (2026-03-04): 같은 출장 기록이 서로 다른 출발 시각 08:30 / 09:15을(를) 가리켜 하나로 정하지 않았습니다.\n  - 출발 08:30 · 출장 일정표 · 출처 m1\n  - 출발 09:15 · 출장 메모 · 출처 m2";
  const trip = (summary: string, over: any = {}) => mk({ id: "c1", title: "지난 출장 출발 시각", scope: "memory", query: "출장", summary, ...over });

  it("reads only dated choices from the summary and never offers an undated record", () => {
    expect(dateChoices(choose)).toEqual(["2026-03-04", "2026-04-10"]);
    expect(dateChoices(conflict)).toEqual([]);
    expect(dateChoices("출발 시각: 08:30 (2026-03-04) · 제주 출장")).toEqual([]);
    expect(dateChoices(null)).toEqual([]);
  });

  it("offers both dates, changes nothing until one is chosen, then shows that trip's value", async () => {
    seed(group("g1", [trip(choose)]));
    const t = groups.g1.tasks[0];
    let filter: string | null = null;
    Object.defineProperty(t, "filter", { enumerable: true, get: () => filter, set: (v: string) => {
      filter = v; t.summary = "출발 시각: 08:30 (2026-03-04) · 제주 출장 출처 m1";
    } });
    Object.defineProperty(t, "state", { enumerable: true, get: () => "completed", set: () => {} }); // the timer finishes at once
    const user = userEvent.setup();
    mount();
    expect(await screen.findByRole("button", { name: "2026-03-04 출장으로 보기" })).toBeVisible();
    expect(screen.getByRole("button", { name: "2026-04-10 출장으로 보기" })).toBeVisible();
    expect(screen.queryByRole("button", { name: /날짜 미상/ })).toBeNull();
    expect(screen.getByText(/출발 시각 선택 필요/)).toBeVisible();
    expect(controls()).toHaveLength(0);
    expect(calls.filter(c => c.method === "POST" && c.path !== "/personal/google/history")).toEqual([]);
    await user.click(screen.getByRole("button", { name: "2026-03-04 출장으로 보기" }));
    await waitFor(() => expect(controls()).toHaveLength(1));
    expect(controls()[0].body).toMatchObject({ request_id: "g1", task_id: "c1", revision: 2, action: "refine", text: "2026-03-04" });
    expect(await screen.findByText(/출발 시각: 08:30 \(2026-03-04\)/)).toBeVisible();
    expect(screen.queryByText(/14:15/)).toBeNull();
    expect(screen.queryByRole("button", { name: /출장으로 보기/ })).toBeNull();
  });

  it("shows an incompatible departure conflict with both values and sources, without choice buttons", async () => {
    seed(group("g1", [trip(conflict)]));
    interpret = () => Response.json({ kind: "work", group: groups.g1, focus: "evidence", task_id: "c1" });
    const user = userEvent.setup();
    mount();
    await screen.findAllByText(/출발 시각 충돌 \(2026-03-04\)/);
    await user.type(await editor(), "출장 근거가 뭐였어?");
    await send(user);
    const region = await screen.findByRole("region", { name: "요청한 작업 결과" });
    for (const part of ["08:30", "09:15", "출장 일정표", "출장 메모", "m1", "m2"]) expect(region).toHaveTextContent(part);
    expect(region).not.toHaveTextContent("출발 시각: ");
    expect(screen.queryByRole("button", { name: /출장으로 보기/ })).toBeNull();
    expect(controls()).toHaveLength(0);
  });

  it("offers date choices from the evidence focus too and sends the same refine", async () => {
    seed(group("g1", [trip(choose)]));
    interpret = () => Response.json({ kind: "work", group: groups.g1, focus: "evidence", task_id: "c1" });
    const user = userEvent.setup();
    mount();
    await screen.findAllByRole("button", { name: "2026-04-10 출장으로 보기" });
    await user.type(await editor(), "출장 출발 근거 보여줘");
    await send(user);
    const region = await screen.findByRole("region", { name: "요청한 작업 결과" });
    await user.click(within(region).getByRole("button", { name: "2026-04-10 출장으로 보기" }));
    await waitFor(() => expect(controls()).toHaveLength(1));
    expect(controls()[0].body).toMatchObject({ action: "refine", text: "2026-04-10" });
  });
});
