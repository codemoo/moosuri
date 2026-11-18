import { StrictMode } from "react";
import { act, render, screen, waitFor, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import PersonalPreview, { PREVIEW_DRAFT_KEY } from "./PersonalPreview";
const aid = "11111111-1111-4111-8111-111111111111",
  bid = "22222222-2222-4222-8222-222222222222",
  tid = "33333333-3333-4333-8333-333333333333";
const session = (id = aid, restricted = false) => ({
  user: {
    id,
    name: id === aid ? "Member A" : "Member B",
    email: id === aid ? "a@example.test" : "b@example.test",
    role: "member",
    must_change_password: restricted,
  },
  password_change_required: restricted,
  chat_draft_scope: (id === aid ? "a" : "b").repeat(64),
});
let current: ReturnType<typeof session> | null,
  threads: any[],
  turns: any[],
  calls: { path: string; method: string; body: any; account: string | null }[],
  paid: Map<string, any>;
let restrictLogin: boolean,
  unknownFirst: boolean,
  hideHistory: boolean,
  deferSend: boolean,
  completeSend: ((value: Response) => void) | null,
  capEnabled: boolean;
const turn = (id: string, text: string, state = "completed") => ({
  sequence: 1,
  user_text: text,
  assistant:
    state === "completed"
      ? [
          {
            id: "answer",
            text: "Synthetic reply <button>opaque tool text</button>",
            phase: "final_answer",
            refusal: false,
          },
        ]
      : [],
  run: {
    id: "44444444-4444-4444-8444-444444444444",
    request_id: id,
    thread_id: threads[0]?.id ?? tid,
    state,
    model: { function_tools: false },
    can_retry: false,
    can_cancel: false,
  },
});
beforeEach(() => {
  sessionStorage.clear();
  current = session();
  threads = [
    { id: tid, title: "A private thread", created_at: 1, updated_at: 1 },
  ];
  turns = [];
  calls = [];
  paid = new Map();
  restrictLogin = false;
  unknownFirst = false;
  hideHistory = false;
  deferSend = false;
  completeSend = null;
  capEnabled = true;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit = {}) => {
      const path = url.replace("/api/v1", "");
      const method = init.method ?? "GET";
      const body = init.body ? JSON.parse(String(init.body)) : null;
      const account = new Headers(init.headers).get("X-Moosuri-Account");
      calls.push({ path, method, body, account });
      if (path === "/auth/session")
        return current
          ? Response.json(current)
          : Response.json({ code: "unauthorized" }, { status: 401 });
      if (path === "/auth/login") {
        current = session(
          body.email.startsWith("b") ? bid : aid,
          restrictLogin,
        );
        return Response.json(current);
      }
      if (path === "/auth/password") {
        current = session(current!.user.id);
        return Response.json(current);
      }
      if (path === "/auth/logout") {
        current = null;
        return new Response(null, { status: 204 });
      }
      if (path === "/knowledge/memories")
        return Response.json({ items: [], next_after: null, generation: 1 });
      if (path === "/text-preview")
        return capEnabled
          ? Response.json({
              enabled: true,
              capability: "text_only_preview",
              model: "synthetic-text",
              tools: false,
              automatic_retry: false,
            })
          : Response.json({ code: "not_found" }, { status: 404 });
      if (path === "/threads" && method === "GET")
        return Response.json({ threads });
      if (path === "/threads" && method === "POST") {
        let t = threads.find((t) => t.id === body.request_id);
        if (!t) {
          t = {
            id: body.request_id,
            title: body.title,
            created_at: 1,
            updated_at: 1,
          };
          threads.push(t);
        }
        return Response.json(t, { status: 201 });
      }
      if (path.match(/^\/threads\/[^/]+\/messages/))
        return Response.json({
          thread_id: path.split("/")[2],
          cursor: turns.length,
          turns: hideHistory ? [] : turns,
          next_before: null,
        });
      if (path.match(/^\/threads\/[^/]+\/text-preview$/) && method === "POST") {
        let result = paid.get(body.request_id);
        if (!result) {
          result = turn(body.request_id, body.text);
          result.run.thread_id = path.split("/")[2];
          paid.set(body.request_id, result);
        }
        if (deferSend)
          return await new Promise<Response>((resolve) => {
            completeSend = resolve;
          });
        if (unknownFirst) {
          unknownFirst = false;
          throw new TypeError("synthetic lost ACK");
        }
        turns = [result];
        hideHistory = false;
        return Response.json(result, { status: 201 });
      }
      if (path.match(/^\/threads\/[^/]+$/) && method === "DELETE") {
        threads = threads.filter((t) => t.id !== path.split("/")[2]);
        return new Response(null, { status: 204 });
      }
      throw new Error(`Unexpected preview route ${method} ${path}`);
    }),
  );
});
afterEach(() => {
  cleanup();
  sessionStorage.clear();
  vi.unstubAllGlobals();
});
function mount() {
  return render(
    <StrictMode>
      <PersonalPreview />
    </StrictMode>,
  );
}
async function composer() {
  return await screen.findByRole("textbox", { name: "메시지" });
}
it("logs in, replaces a temporary password and sends through the explicit text-only route", async () => {
  current = null;
  restrictLogin = true;
  threads = [];
  const user = userEvent.setup();
  mount();
  await screen.findByRole("heading", { name: "내 무수리에 로그인" });
  await user.type(
    screen.getByRole("textbox", { name: "이메일" }),
    "a@example.test",
  );
  await user.type(screen.getByLabelText("비밀번호"), "Temporary-password-42");
  await user.click(screen.getByRole("button", { name: "로그인" }));
  await screen.findByRole("heading", { name: "임시 비밀번호 변경" });
  await user.type(screen.getByLabelText("새 비밀번호"), "Changed-password-42");
  await user.click(screen.getByRole("button", { name: "비밀번호 변경" }));
  await screen.findByRole("heading", { name: "내 대화" });
  expect(screen.getByText(/메일 조회·발송/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "대화 만들기" }));
  const input = await composer();
  await user.type(input, "Explain a queue");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText("Synthetic reply <button>opaque tool text</button>");
  expect(screen.queryByRole("button", { name: "opaque tool text" })).toBeNull();
  expect(paid.size).toBe(1);
  const posts = calls.filter((c) => c.method === "POST");
  expect(posts.map((c) => c.path)).toEqual([
    "/auth/login",
    "/auth/password",
    "/threads",
    expect.stringMatching(/\/text-preview$/),
  ]);
  expect(posts.at(-1)!.body).toEqual({
    request_id: expect.any(String),
    text: "Explain a queue",
    capability: "text_only_preview",
  });
  expect(posts.slice(1).every((c) => c.account === aid)).toBe(true);
  expect(
    calls.some(
      (c) =>
        c.path === "/system" ||
        c.path.includes("/mail/") ||
        (c.path.endsWith("/messages") && c.method === "POST"),
    ),
  ).toBe(false);
});
it("recovers an unconfirmed send after refresh without an automatic POST and reuses its immutable request", async () => {
  unknownFirst = true;
  hideHistory = true;
  const user = userEvent.setup();
  let view = mount();
  const input = await composer();
  await user.type(input, "Keep this exact request");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByRole("button", { name: "동일 요청 다시 확인" });
  const original = calls.find(
    (c) => c.path.endsWith("/text-preview") && c.method === "POST",
  )!;
  await waitFor(() =>
    expect(sessionStorage.getItem(PREVIEW_DRAFT_KEY)).toContain(
      original.body.request_id,
    ),
  );
  expect(paid.size).toBe(1);
  view.unmount();
  view = mount();
  await screen.findByRole("button", { name: "동일 요청 다시 확인" });
  expect(
    calls.filter(
      (c) => c.method === "POST" && c.path.endsWith("/text-preview"),
    ),
  ).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "동일 요청 다시 확인" }));
  await screen.findByText("Synthetic reply <button>opaque tool text</button>");
  const posted = calls.filter(
    (c) => c.method === "POST" && c.path.endsWith("/text-preview"),
  );
  expect(posted).toHaveLength(2);
  expect(posted[1].body).toEqual(original.body);
  expect(paid.size).toBe(1);
  expect(
    screen.queryByRole("button", { name: "동일 요청 다시 확인" }),
  ).toBeNull();
  expect(await composer()).toHaveValue("");
  view.unmount();
});
it("logout fences an in-flight response and its saved draft before another account signs in", async () => {
  deferSend = true;
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "PRIVATE A request");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(completeSend).not.toBeNull());
  await user.click(screen.getByRole("button", { name: "로그아웃" }));
  await screen.findByRole("heading", { name: "내 무수리에 로그인" });
  expect(sessionStorage.getItem(PREVIEW_DRAFT_KEY)).toBeNull();
  const result = paid.values().next().value;
  await act(async () => {
    completeSend!(Response.json(result, { status: 201 }));
  });
  expect(screen.queryByText(/PRIVATE A request/)).toBeNull();
  expect(sessionStorage.getItem(PREVIEW_DRAFT_KEY)).toBeNull();
  threads = [
    { id: tid, title: "B private thread", created_at: 1, updated_at: 1 },
  ];
  turns = [];
  deferSend = false;
  await user.type(
    screen.getByRole("textbox", { name: "이메일" }),
    "b@example.test",
  );
  await user.type(screen.getByLabelText("비밀번호"), "B-password-42");
  await user.click(screen.getByRole("button", { name: "로그인" }));
  await screen.findByRole("heading", { name: "B private thread" });
  expect(await composer()).toHaveValue("");
  expect(screen.queryByText(/Synthetic reply/)).toBeNull();
  expect(
    calls.filter(
      (c) => c.path.endsWith("/text-preview") && c.method === "POST",
    ),
  ).toHaveLength(1);
  expect(calls.filter((c) => c.path === "/auth/logout")).toHaveLength(1);
});
it("keeps private history visible when the preview is disabled and deletes only the selected thread", async () => {
  capEnabled = false;
  turns = [
    turn(
      "55555555-5555-4555-8555-555555555555",
      "Saved private request",
      "outcome_unknown",
    ),
  ];
  const user = userEvent.setup();
  vi.stubGlobal(
    "confirm",
    vi.fn(() => true),
  );
  mount();
  await screen.findByText("Saved private request");
  expect(
    screen.getByText("응답 확인 불가 · 자동 재시도 없음"),
  ).toBeInTheDocument();
  expect(await composer()).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "대화 삭제" }));
  await screen.findByRole("heading", { name: "내 텍스트 대화를 시작하세요" });
  expect(
    calls
      .filter((c) => c.method === "DELETE")
      .map((c) => ({ path: c.path, account: c.account })),
  ).toEqual([{ path: `/threads/${tid}`, account: aid }]);
  expect(calls.filter((c) => c.method === "POST")).toHaveLength(0);
});

it("opens manual memories in the authenticated account and returns to the same conversation", async () => {
  const user = userEvent.setup();
  mount();
  await composer();
  await user.click(screen.getByRole("button", { name: "기억" }));
  await screen.findByRole("button", { name: "기억 추가" });
  expect(
    calls
      .filter((c) => c.path === "/knowledge/memories")
      .every((c) => c.account === aid),
  ).toBe(true);
  expect(
    calls.some(
      (c) =>
        c.path.includes("/knowledge/collections") ||
        c.path.includes("/extraction"),
    ),
  ).toBe(false);
  await user.click(screen.getByRole("button", { name: "대화" }));
  expect(await composer()).toHaveValue("");
  expect(calls.filter((c) => c.method === "POST")).toHaveLength(0);
});

it("sends a first message from the empty workspace with one thread and one model call", async () => {
  current = session();
  threads = [];
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "Where is my list?");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(paid.size).toBe(1));
  expect(threads).toHaveLength(1);
  expect(threads[0].title).toBe("Where is my list?");
  expect(
    calls.filter((c) => c.path === "/threads" && c.method === "POST"),
  ).toHaveLength(1);
});
