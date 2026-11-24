import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import PersonalPreview from "./PersonalPreview";

const aid = "11111111-1111-4111-8111-111111111111";
const tid = "33333333-3333-4333-8333-333333333333";
const session = { user: { id: aid, name: "Member A", email: "a@example.test", role: "member", must_change_password: false }, password_change_required: false, chat_draft_scope: "a".repeat(64) };
const conv = `/personal/tasks/conversations/${tid}`;
const mk = (over: any = {}) => ({
  id: "c1", title: "일정 조사", scope: "conversation", query: "일정", filter: null, state: "queued", revision: 1, question_id: null,
  run_id: null, runs: [], events: [], source_count: 0, summary: null, report_id: null, ...over,
});
let calls: { path: string; method: string; body: any }[];
let groups: Record<string, any>;
let turns: any[];
let failNextWorkPost: boolean;
const receipt = (id: string, text: string) => ({
  sequence: turns.length + 1, user_text: text,
  assistant: [{ id: "a" + id, text: "검색을 맡겼습니다.", phase: "final_answer", refusal: false }],
  run: { id: "run-" + id, request_id: id, thread_id: tid, state: "completed", model: { provider_id: "member-conversation-work", function_tools: false }, can_retry: false, can_cancel: false },
});
function seed(tasks: any[]) {
  groups = { g1: { id: "g1", thread_id: tid, turn_id: "run-g1", message: "맡김", tasks } };
  turns = [receipt("g1", "지난 대화와 기억을 찾아줘")];
}
beforeEach(() => {
  sessionStorage.clear();
  calls = []; groups = {}; turns = []; failNextWorkPost = false;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    const path = url.replace("/api/v1", "");
    const method = init.method ?? "GET";
    const body = init.body ? JSON.parse(String(init.body)) : null;
    calls.push({ path, method, body });
    if (path === "/auth/session") return Response.json(session);
    if (path === "/text-preview") return Response.json({ enabled: true, capability: "text_only_preview", tools: false, automatic_retry: false });
    if (path === "/threads" && method === "GET") return Response.json({ threads: [{ id: tid, title: "출장 준비", created_at: 1, updated_at: 1 }] });
    if (path.startsWith(`/threads/${tid}/messages`)) return Response.json({ thread_id: tid, cursor: turns.length, turns, next_before: null });
    if (path === "/personal/google/history") return Response.json({ reads: [] });
    if (path === conv && method === "POST") {
      if (failNextWorkPost) { failNextWorkPost = false; groups[body.request_id] ??= { id: body.request_id, thread_id: tid, turn_id: "t", message: body.message, tasks: body.items.map((item: any, i: number) => mk({ id: `n${i}`, title: item.title, scope: item.scope, query: item.query })) }; turns = [receipt(body.request_id, body.message)]; throw new TypeError("lost ACK"); }
      if (!groups[body.request_id]) {
        groups[body.request_id] = { id: body.request_id, thread_id: tid, turn_id: "t", message: body.message, tasks: body.items.map((item: any, i: number) => mk({ id: `n${i}`, title: item.title, scope: item.scope, query: item.query })) };
        turns = [...turns, receipt(body.request_id, body.message)];
      }
      return Response.json(groups[body.request_id], { status: 201 });
    }
    if (path === conv && method === "GET") return Response.json({ thread_id: tid, groups: Object.values(groups) });
    if (path === `${conv}/control` && method === "POST") {
      const group = groups[body.request_id];
      const found = group.tasks.find((t: any) => t.id === body.task_id);
      if (found.revision !== body.revision) return Response.json({ code: "stale" }, { status: 409 });
      if (body.action === "cancel") found.state = "cancelled";
      if (body.action === "refine") { found.state = "queued"; found.filter = body.text; }
      found.revision += 1;
      return Response.json(group);
    }
    return Response.json({ code: "not_found" }, { status: 404 });
  }));
});
afterEach(() => { cleanup(); sessionStorage.clear(); vi.unstubAllGlobals(); });

const mount = () => render(<StrictMode><PersonalPreview /></StrictMode>);
const editor = () => screen.findByRole("textbox", { name: "메시지" });
const send = (user: ReturnType<typeof userEvent.setup>) => user.click(screen.getByRole("button", { name: "보내기" }));
const workPosts = () => calls.filter(c => c.method === "POST" && c.path === conv);
const controlPosts = () => calls.filter(c => c.method === "POST" && c.path === `${conv}/control`);
const chatPosts = () => calls.filter(c => c.method === "POST" && c.path.endsWith("/text-preview"));

it("sends the natural two-part sentence as two independent children and not as chat", async () => {
  const user = userEvent.setup();
  mount();
  const text = "지난 출장 대화에서 정해진 일정을 찾아주고, 내 기억에서 준비물도 찾아줘";
  await user.type(await editor(), text);
  await send(user);
  await waitFor(() => expect(workPosts()).toHaveLength(1));
  expect(workPosts()[0].body).toMatchObject({
    message: text,
    items: [{ scope: "conversation", query: "일정" }, { scope: "memory", query: "준비물" }],
  });
  expect(workPosts()[0].body.request_id).toMatch(/^[0-9a-f-]{36}$/);
  expect(await screen.findByRole("listitem", { name: "출장 일정 조사" })).toBeVisible();
  expect(screen.getByRole("listitem", { name: "준비물 조사" })).toBeVisible();
  expect(chatPosts()).toHaveLength(0);
  expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue("");
});

it("keeps one-query searches on the existing route and sends the compact two-item form to the group route", async () => {
  const user = userEvent.setup();
  mount();
  await user.type(await editor(), "대화 검색: 출장; 기억 검색: 준비물");
  await send(user);
  await waitFor(() => expect(workPosts()).toHaveLength(1));
  expect(workPosts()[0].body.items.map((i: any) => [i.scope, i.query])).toEqual([["conversation", "출장"], ["memory", "준비물"]]);
  await screen.findByRole("listitem", { name: "출장 조사" });
  await user.type(await editor(), "기억 검색: 여권");
  await send(user);
  await waitFor(() => expect(calls.some(c => c.path === "/personal/tasks/workflows" && c.method === "POST")).toBe(true));
  expect(workPosts()).toHaveLength(1);
});

it("reuses the same request id when the first acknowledgement is lost", async () => {
  failNextWorkPost = true;
  const user = userEvent.setup();
  mount();
  await user.type(await editor(), "대화 검색: 출장; 기억 검색: 준비물");
  await send(user);
  await user.click(await screen.findByRole("button", { name: "동일 요청 다시 확인" }));
  await waitFor(() => expect(workPosts()).toHaveLength(2));
  expect(workPosts()[1].body).toEqual(workPosts()[0].body);
  expect(Object.keys(groups)).toHaveLength(1);
});

it("composes independent work items explicitly and keeps the unsent draft", async () => {
  const user = userEvent.setup();
  mount();
  await user.type(await editor(), "아직 보내지 않은 초안");
  await user.click(screen.getByRole("button", { name: "여러 검색 한꺼번에 맡기기" }));
  await user.type(screen.getByLabelText("1번 검색어"), "출장");
  await user.type(screen.getByLabelText("2번 검색어"), "준비물");
  await user.click(screen.getByRole("button", { name: "2개 맡기기" }));
  await waitFor(() => expect(workPosts()).toHaveLength(1));
  expect(workPosts()[0].body.message).toBe("대화 검색: 출장; 기억 검색: 준비물");
  expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue("아직 보내지 않은 초안");
});

it("answers a status question as a read: no dispatch, no chat turn, results stay visible", async () => {
  seed([mk(), mk({ id: "m1", title: "준비물 조사", scope: "memory", query: "준비물", state: "completed", revision: 2, run_id: "r", summary: "여권", source_count: 1 })]);
  const user = userEvent.setup();
  mount();
  expect(await screen.findByText("여권")).toBeVisible();
  await user.type(await editor(), "둘 다 어디까지 됐어?");
  await send(user);
  expect(await screen.findByText("지금까지의 진행")).toBeVisible();
  expect(controlPosts()).toHaveLength(0);
  expect(workPosts()).toHaveLength(0);
  expect(chatPosts()).toHaveLength(0);
  expect(screen.getByText("여권")).toBeVisible();
});

it("refines the matching child and stops the only running one", async () => {
  seed([mk(), mk({ id: "m1", title: "준비물 조사", scope: "memory", query: "준비물", state: "completed", revision: 2, run_id: "r", summary: "여권", source_count: 1 })]);
  const user = userEvent.setup();
  mount();
  await screen.findByText("여권");
  await user.type(await editor(), "준비물은 전자기기만");
  await send(user);
  await waitFor(() => expect(controlPosts()).toHaveLength(1));
  expect(controlPosts()[0].body).toEqual({ request_id: "g1", task_id: "m1", revision: 2, action: "refine", text: "전자기기" });
  expect(await screen.findByText(/그리고 “전자기기”/)).toBeVisible();
  // Both are queued now, so "그거 멈춰" must ask which one instead of guessing.
  await user.type(screen.getByRole("textbox", { name: "메시지" }), "그거 멈춰");
  await send(user);
  expect(await screen.findByText("어느 검색에 적용할까요?")).toBeVisible();
  expect(controlPosts()).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: /준비물 조사 \(“준비물”\)/ }));
  await waitFor(() => expect(controlPosts()).toHaveLength(2));
  expect(controlPosts()[1].body).toMatchObject({ action: "cancel", task_id: "m1" });
  expect(chatPosts()).toHaveLength(0);
});

it("explains when no child matches, keeps the draft, and sends it as chat only on request", async () => {
  seed([mk({ state: "completed", revision: 2, run_id: "r", summary: "3월 4일", source_count: 1 })]);
  const user = userEvent.setup();
  mount();
  await screen.findByText("3월 4일");
  await user.type(await editor(), "배터리는 충전기만");
  await send(user);
  expect(await screen.findByText(/찾지 못했습니다/)).toBeVisible();
  expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue("배터리는 충전기만");
  expect(controlPosts()).toHaveLength(0);
  expect(chatPosts()).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "일반 메시지로 보내기" }));
  await waitFor(() => expect(chatPosts()).toHaveLength(1));
  expect(chatPosts()[0].body.text).toBe("배터리는 충전기만");
});

it("treats a stop request in a thread without work as ordinary chat", async () => {
  const user = userEvent.setup();
  mount();
  await user.type(await editor(), "그거 멈춰");
  await send(user);
  await waitFor(() => expect(chatPosts()).toHaveLength(1));
  expect(controlPosts()).toHaveLength(0);
});
