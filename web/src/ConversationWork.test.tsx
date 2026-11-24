import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { GroupForTurn, useConversationWork } from "./ConversationWork";
import type { ConversationTask, ConversationWorkGroup } from "./conversation-work";

const task = (over: Partial<ConversationTask> = {}): ConversationTask => ({
  id: "c1", title: "일정 조사", scope: "conversation", query: "일정", filter: null, state: "completed", revision: 3,
  question_id: null, run_id: "run-2", runs: ["run-1", "run-2"],
  events: [{ state: "queued", text: "접수됨", at: 1700000000 }, { state: "completed", text: "읽기 끝남", at: 1700000060 }],
  source_count: 2, summary: "출장 일정은 3월 4일입니다.", report_id: null, ...over,
});
const group = (tasks: ConversationTask[], id = "g1", thread = "t1"): ConversationWorkGroup => ({ id, thread_id: thread, turn_id: "turn-" + id, message: "m", tasks });
const page = (groups: ConversationWorkGroup[], thread = "t1") => Response.json({ thread_id: thread, groups });

let calls: { url: string; method: string; body: any }[];
let route: (url: string, method: string, body: any) => Response | Promise<Response>;
const expired = vi.fn();
const openRun = vi.fn();
const newGroup = vi.fn();
beforeEach(() => {
  calls = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    const call = { url: url.replace("/api/v1", ""), method: init.method ?? "GET", body: init.body ? JSON.parse(String(init.body)) : null };
    calls.push(call);
    return route(call.url, call.method, call.body);
  }));
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.clearAllMocks(); });

function Harness({ thread = "t1", ids = ["g1"] }: { thread?: string; ids?: string[] }) {
  const work = useConversationWork({ accountId: "member-a", thread, enabled: true, receiptIds: ids, expired, onNewGroup: newGroup });
  return <>{ids.map(id => <GroupForTurn key={id} requestId={id} work={work} openRun={openRun} />)}</>;
}
const reads = () => calls.filter(c => c.method === "GET");
const posts = () => calls.filter(c => c.method === "POST");

it("reads each thread once, shows the live summary, and opens a child's run detail and history", async () => {
  route = () => page([group([task(), task({ id: "m1", title: "준비물 조사", scope: "memory", query: "준비물", summary: "전자기기와 여권", run_id: "run-m", runs: ["run-m"] })])]);
  const user = userEvent.setup();
  render(<Harness />);
  expect(await screen.findByText("출장 일정은 3월 4일입니다.")).toBeVisible();
  expect(screen.getByText("전자기기와 여권")).toBeVisible();
  const first = screen.getByRole("listitem", { name: "일정 조사" });
  await user.click(within(first).getByRole("button", { name: "상세·근거 보기" }));
  expect(openRun).toHaveBeenCalledWith("run-2");
  await user.click(within(first).getByRole("button", { name: "진행 기록" }));
  expect(within(first).getByText("접수됨")).toBeVisible();
  await user.click(within(first).getByRole("button", { name: "1번째 실행 상세" }));
  expect(openRun).toHaveBeenLastCalledWith("run-1");
  expect(reads()).toHaveLength(1);
  expect(reads()[0].url).toBe("/personal/tasks/conversations/t1");
});

it("polls only while a child is queued, then stops, and loads an older group once by request_id", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  let latest = 0;
  route = (url) => {
    if (url.includes("request_id=g0")) return page([group([task({ id: "o1", title: "예전 검색" })], "g0")]);
    latest += 1;
    return page([group([latest === 1 ? task({ id: "c2", title: "진행 검색", state: "queued", revision: 1, summary: null }) : task({ id: "c2", title: "진행 검색", state: "completed", revision: 2, summary: "다 읽었습니다" })])]);
  };
  render(<Harness ids={["g0", "g1"]} />);
  expect(await screen.findByText("예전 검색")).toBeVisible();
  expect(await screen.findByText("확인 대기·진행 중")).toBeVisible();
  await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
  expect(await screen.findByText("다 읽었습니다")).toBeVisible();
  const settled = calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(15000); });
  expect(calls).toHaveLength(settled);
  expect(calls.filter(c => c.url.includes("request_id=g0"))).toHaveLength(1);
});

it("answers a persisted question with the same body after a lost acknowledgement", async () => {
  const waiting = task({ id: "c3", state: "needs_input", question_id: "q-1", revision: 1, summary: null, run_id: null, runs: [], source_count: 0, query: "" });
  let fail = true;
  route = (_url, method, body) => {
    if (method === "GET") return page([group([waiting])]);
    if (fail) { fail = false; throw new TypeError("lost reply"); }
    return Response.json(group([{ ...waiting, state: "queued", revision: 2, query: body.text, question_id: null }]));
  };
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(await screen.findByRole("button", { name: "검색어 입력" }));
  await user.type(screen.getByLabelText("찾을 검색어"), "출장");
  await user.click(screen.getByRole("button", { name: "답하기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("다시 시도하면 중복 없이");
  await user.click(screen.getByRole("button", { name: "답하기" }));
  expect(await screen.findByText(/“출장”/)).toBeVisible();
  const [first, second] = posts();
  expect(first.url).toBe("/personal/tasks/conversations/t1/control");
  expect(second.body).toEqual(first.body);
  expect(first.body).toEqual({ request_id: "g1", task_id: "c3", revision: 1, action: "answer", text: "출장", question_id: "q-1" });
});

it("reuses a proven search with one stable new request id across retries and reports the new group", async () => {
  let attempts = 0;
  route = (_url, method, body) => {
    if (method === "GET") return page([group([task()])]);
    attempts += 1;
    if (attempts === 1) return Response.json({ code: "busy" }, { status: 503 });
    return Response.json(group([task({ id: "c9", state: "queued", query: body.text, revision: 1, summary: null, source_count: 0 })], body.request_id));
  };
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(await screen.findByRole("button", { name: "새 검색어로 다시" }));
  await user.type(screen.getByLabelText("새 검색어"), "서울");
  await user.click(screen.getByRole("button", { name: "새 검색 시작" }));
  await screen.findByRole("alert");
  await user.click(screen.getByRole("button", { name: "새 검색 시작" }));
  await waitFor(() => expect(newGroup).toHaveBeenCalledTimes(1));
  const [first, second] = posts();
  expect(first.body.request_id).toMatch(/^[0-9a-f-]{36}$/);
  expect(first.body.request_id).not.toBe("g1");
  expect(second.body).toEqual(first.body);
  expect(first.body).toMatchObject({ action: "reuse", task_id: "c1", revision: 3, text: "서울" });
});

it("saves a report, then reads it inline as plain text without creating links", async () => {
  const body = '정리 <a href="http://example.test">링크</a>\n- 항목';
  route = (url, method) => {
    if (url.includes("/reports/r1")) return Response.json({ id: "r1", title: "일정 보고서", body, run_id: "run-2" });
    if (method === "POST") return Response.json(group([task({ report_id: "r1", revision: 4 })]));
    return page([group([task()])]);
  };
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(await screen.findByRole("button", { name: "보고서 만들기" }));
  await user.click(await screen.findByRole("button", { name: "보고서 보기" }));
  const report = await screen.findByRole("region", { name: "보고서: 일정 보고서" });
  expect(report).toHaveTextContent("<a href=\"http://example.test\">링크</a>");
  expect(within(report).queryByRole("link")).toBeNull();
  expect(posts()[0].body).toMatchObject({ action: "save_report", request_id: "g1", revision: 3 });
});

it("fences a response that arrives after the thread changed", async () => {
  let release: (value: Response) => void = () => {};
  route = (url) => url.includes("/conversations/t1")
    ? new Promise<Response>(resolve => { release = resolve; })
    : page([], "t2");
  const view = render(<Harness thread="t1" />);
  await waitFor(() => expect(reads()).toHaveLength(1));
  view.rerender(<Harness thread="t2" />);
  await screen.findByRole("button", { name: "다시 시도" });
  await act(async () => { release(page([group([task()])])); });
  expect(screen.queryByText("출장 일정은 3월 4일입니다.")).toBeNull();
});

it("shows a stale conflict as a notice, refetches the group, and cancels a pending child", async () => {
  const live = task({ id: "q1", state: "queued", revision: 1, summary: null });
  let conflict = true;
  route = (_url, method) => {
    if (method === "POST") {
      if (conflict) { conflict = false; return Response.json({ code: "stale" }, { status: 409 }); }
      return Response.json(group([{ ...live, state: "cancelled", revision: 3 }]));
    }
    return page([group([conflict ? live : { ...live, revision: 2 }])]);
  };
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(await screen.findByRole("button", { name: "중단" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("작업 상태가 바뀌었습니다");
  await waitFor(() => expect(reads().length).toBeGreaterThan(1));
  await user.click(screen.getByRole("button", { name: "중단" }));
  expect(await screen.findByText("중단됨")).toBeVisible();
  expect(posts()[1].body).toMatchObject({ action: "cancel", revision: 2 });
});
