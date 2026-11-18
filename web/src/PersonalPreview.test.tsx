import { StrictMode } from "react";
import { act, render, screen, waitFor, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import PersonalPreview, { PREVIEW_DRAFT_KEY } from "./PersonalPreview";
import type { ThreadGoogleRead } from "./PersonalGoogle";
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
let mailReads: ThreadGoogleRead[];
let mailFailure: boolean,
  mailHang: boolean,
  readHang: boolean,
  paginate: boolean;
let restrictLogin: boolean,
  unknownFirst: boolean,
  hideHistory: boolean,
  deferSend: boolean,
  completeSend: ((value: Response) => void) | null,
  capEnabled: boolean;
let pendingCapability: Promise<Response> | undefined;
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
  mailReads = [];
  mailFailure = false;
  mailHang = false;
  readHang = false;
  paginate = false;
  calls = [];
  paid = new Map();
  restrictLogin = false;
  unknownFirst = false;
  hideHistory = false;
  deferSend = false;
  completeSend = null;
  capEnabled = true;
  pendingCapability = undefined;
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
      if (path === "/personal/google")
        return Response.json({
          connection: {
            connection_id: tid,
            epoch: 1,
            mailbox: "a@example.test",
          },
        });
      if (path === "/personal/google/complete")
        return Response.json({
          connection_id: tid,
          epoch: 1,
          mailbox: "a@example.test",
          flow_id: "a".repeat(64),
          continuation: { thread_id: tid, request_id: bid },
        });
      if (path === "/personal/google/history") {
        if (mailHang) return await new Promise<Response>(() => {});
        if (mailFailure)
          return Response.json({ code: "unavailable" }, { status: 503 });
        return Response.json({
          reads: mailReads.filter((r) => r.thread_id === body.thread_id),
        });
      }
      if (path === "/personal/google/inbox") {
        const result = {
          mailbox: "a@example.test",
          messages: [],
          has_more: false,
          scope: "one_inbox_page_metadata_only",
        };
        if (body.read) {
          const saved = {
            ...body.read,
            state: "completed",
            created_at: 1,
            source: {
              connection_id: tid,
              epoch: 1,
              app_revision: aid,
              mailbox: "a@example.test",
            },
            result,
          };
          mailReads = [saved];
          if (readHang) return await new Promise<Response>(() => {});
          return Response.json(saved);
        }
        return Response.json(result);
      }
      if (path === "/text-preview" && pendingCapability)
        return pendingCapability;
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
          next_before: paginate && !path.includes("?before=") ? 1 : null,
        });
      if (path.match(/^\/threads\/[^/]+\/text-preview$/) && method === "POST") {
        if (body.text === "show my latest email")
          return Response.json(
            { code: "personal_inbox_requested" },
            { status: 422 },
          );
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
  history.replaceState(null, "", "/");
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
  expect(screen.getByText(/모델에 전달되지 않습니다/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "대화 만들기" }));
  const input = await composer();
  await user.type(input, "Explain a queue");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText("Synthetic reply <button>opaque tool text</button>");
  expect(screen.queryByRole("button", { name: "opaque tool text" })).toBeNull();
  expect(paid.size).toBe(1);
  const posts = calls.filter(
    (c) => c.method === "POST" && c.path !== "/personal/google/history",
  );
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
  expect(
    calls.filter(
      (c) => c.method === "POST" && c.path !== "/personal/google/history",
    ),
  ).toHaveLength(0);
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
  expect(
    calls.filter(
      (c) => c.method === "POST" && c.path !== "/personal/google/history",
    ),
  ).toHaveLength(0);
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

it("returns a recognized read to its original thread and replays stored history after reload", async () => {
  const user = userEvent.setup();
  const first = mount();
  await user.type(await composer(), "show my latest email");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText("확인한 페이지에 받은 메일이 없습니다.");
  expect(
    screen.getByRole("heading", { name: "A private thread" }),
  ).toBeInTheDocument();
  expect(
    screen.getByText(/전체 메일에서 가장 최근인 항목을 뜻하지 않습니다/),
  ).toBeInTheDocument();
  expect(calls.filter((c) => c.path === "/personal/google/inbox")).toHaveLength(
    1,
  );
  expect(paid.size).toBe(0);
  first.unmount();
  mount();
  await screen.findByText("확인한 페이지에 받은 메일이 없습니다.");
  expect(calls.filter((c) => c.path === "/personal/google/inbox")).toHaveLength(
    1,
  );
});

it("keeps canonical conversation visible while optional mail history is hung", async () => {
  mailHang = true;
  turns = [
    turn(
      "55555555-5555-4555-8555-555555555555",
      "Canonical conversation survives",
    ),
  ];
  mount();
  await screen.findByText("Canonical conversation survives");
  expect(screen.getByText(/Synthetic reply/)).toBeInTheDocument();
});
it("clears stale mail on a history failure without hiding the conversation", async () => {
  turns = [turn("55555555-5555-4555-8555-555555555555", "Canonical message")];
  mailReads = [
    {
      thread_id: tid,
      request_id: aid,
      state: "completed",
      created_at: 1,
      source: {
        connection_id: tid,
        epoch: 1,
        app_revision: aid,
        mailbox: "a@example.test",
      },
      result: {
        mailbox: "a@example.test",
        messages: [
          {
            id: "one",
            subject: "Private saved mail",
            from: "sender",
            date: "date",
          },
        ],
        has_more: false,
        scope: "one_inbox_page_metadata_only",
      },
    },
  ];
  mount();
  await screen.findByText("Private saved mail");
  mailFailure = true;
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
  });
  await screen.findByText(/저장된 메일 확인 기록을 불러오지 못했습니다/);
  expect(screen.queryByText("Private saved mail")).toBeNull();
  expect(screen.getByText("Canonical message")).toBeInTheDocument();
});
it("loading older canonical messages does not fetch the private mail list again", async () => {
  paginate = true;
  turns = [turn("55555555-5555-4555-8555-555555555555", "Canonical page")];
  const user = userEvent.setup();
  mount();
  await screen.findByText("Canonical page");
  const count = calls.filter(
    (c) => c.path === "/personal/google/history",
  ).length;
  await user.click(screen.getByRole("button", { name: "이전 메시지" }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "이전 메시지" })).toBeNull(),
  );
  expect(
    calls.filter((c) => c.path === "/personal/google/history"),
  ).toHaveLength(count);
});
it("clears only the exact original first-message draft after a mail result", async () => {
  threads = [];
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "show my latest email");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText("확인한 페이지에 받은 메일이 없습니다.");
  expect(await composer()).toHaveValue("");
  expect(threads).toHaveLength(1);
  expect(
    calls.find((c) => c.path === "/personal/google/inbox")?.body.read.thread_id,
  ).toBe(threads[0].id);
  expect(paid.size).toBe(0);
});
it("an uncertain old read cannot clear a newer draft when returning to the original thread", async () => {
  readHang = true;
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "show my latest email");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() =>
    expect(
      calls.filter((c) => c.path === "/personal/google/inbox"),
    ).toHaveLength(1),
  );
  await user.click(screen.getByRole("button", { name: "대화" }));
  const input = await composer();
  await user.clear(input);
  await user.type(input, "A newer unsent draft");
  await user.click(screen.getByRole("button", { name: "메일" }));
  await waitFor(() =>
    expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue(
      "A newer unsent draft",
    ),
  );
  expect(calls.filter((c) => c.path === "/personal/google/inbox")).toHaveLength(
    1,
  );
});
it.each(["Original request", "A newer draft"])(
  "OAuth returns to the original thread and clears only the exact snapshot: %s",
  async (draft) => {
    threads = [
      { id: aid, title: "Another recent thread", created_at: 2, updated_at: 2 },
      ...threads,
    ];
    sessionStorage.setItem(
      PREVIEW_DRAFT_KEY,
      JSON.stringify({
        version: 1,
        account: aid,
        scope: "a".repeat(64),
        saved_at: Date.now(),
        selected: aid,
        slots: [{ id: tid, draft }],
      }),
    );
    sessionStorage.setItem(
      "moosuri.personal.google-read.v1",
      JSON.stringify({
        flow: "a".repeat(64),
        account: aid,
        session: "a".repeat(64),
        read: { thread_id: tid, request_id: bid },
        draft: "Original request",
        phase: "consent",
        expires: Date.now() + 60000,
      }),
    );
    history.replaceState(
      null,
      "",
      "/#google_handle=" + "b".repeat(64) + "&flow=" + "a".repeat(64),
    );
    mount();
    await screen.findByText("확인한 페이지에 받은 메일이 없습니다.");
    expect(
      screen.getByRole("heading", { name: "A private thread" }),
    ).toBeInTheDocument();
    expect(await composer()).toHaveValue(
      draft === "Original request" ? "" : draft,
    );
    expect(
      calls.filter((c) => c.path === "/personal/google/inbox"),
    ).toHaveLength(1);
  },
);

it("opens a saved conversation while model discovery stalls and keeps it when discovery fails", async () => {
  let finish!: (r: Response) => void;
  pendingCapability = new Promise((resolve) => {
    finish = resolve;
  });
  turns = [
    turn("55555555-5555-4555-8555-555555555555", "Existing conversation"),
  ];
  mount();
  await screen.findByText("Existing conversation");
  expect(await composer()).toBeDisabled();
  await act(async () => {
    finish(Response.json({ code: "unavailable" }, { status: 503 }));
  });
  expect(
    await screen.findByText("모델 연결 상태를 확인할 수 없습니다."),
  ).toBeInTheDocument();
  expect(screen.getByText("Existing conversation")).toBeInTheDocument();
});
