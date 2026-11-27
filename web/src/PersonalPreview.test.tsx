import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, cleanup, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import PersonalPreview, { PREVIEW_DRAFT_KEY } from "./PersonalPreview";
import type { ThreadGoogleRead } from "./PersonalGoogle";
import type { PersonalBrowserWorkspace } from "./personal-browser";
import { connectInKiri } from "./kiri-google-connection";

vi.mock("./kiri-google-connection", () => ({ connectInKiri: vi.fn() }));
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
let current: (ReturnType<typeof session> & { installation_admin?: boolean }) | null,
  threads: any[],
  turns: any[],
  calls: { path: string; method: string; body: any; account: string | null }[],
  paid: Map<string, any>;
let mailReads: ThreadGoogleRead[];
let connectedMailbox: string | null;
let vaultUnreachable: boolean;
let vaultReady: boolean;
let vaultOrigin: string | undefined;
let mailFailure: boolean,
  mailHang: boolean,
  readHang: boolean,
  paginate: boolean;
let restrictLogin: boolean,
  unknownFirst: boolean,
  hideHistory: boolean,
  deferSend: boolean,
  deferRefusal: boolean,
  completeSend: ((value: Response) => void) | null,
  capEnabled: boolean;
let pendingCapability: Promise<Response> | undefined;
let adminEngine: ((path: string, method: string, body: any) => Response) | undefined;
let previewRefused: boolean;
let saveFollowUp: ((body: any) => Response | Promise<Response>) | undefined;
let obsoleteCapabilityFields: boolean;
let browserRoute: ((path: string, method: string, body: any) => Response | Promise<Response>) | undefined;
let deleteThreadResponse: Response | undefined;
let inventoryFailure: boolean;
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
  connectedMailbox = "mailbox@example.test";
  vaultUnreachable = false;
  vaultReady = true;
  vaultOrigin = "https://vault.example.test";
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
  deferRefusal = false;
  completeSend = null;
  capEnabled = true;
  pendingCapability = undefined;
  adminEngine = undefined;
  saveFollowUp = undefined;
  previewRefused = false;
  obsoleteCapabilityFields = false;
  browserRoute = undefined;
  deleteThreadResponse = undefined;
  inventoryFailure = false;
  vi.mocked(connectInKiri).mockResolvedValue({ job:"11111111-1111-4111-8111-111111111111", flow: "a".repeat(64), handle: "b".repeat(64), email: "a@example.test" });
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
      if (path.startsWith("/personal/admin/") && adminEngine)
        return adminEngine(path, method, body);
      if (path === "/knowledge/memories")
        return Response.json({ items: [], next_after: null, generation: 1 });
      if (path === "/personal/google")
        return Response.json({
          connection: connectedMailbox ? {
            connection_id: tid,
            epoch: 1,
            mailbox: connectedMailbox,
          } : null,
        });
      if (path === "/personal/google/workspace") return Response.json({ connections: [] });
      if (path === "/personal/google/disconnect") {
        connectedMailbox = null;
        return new Response(null, { status: 204 });
      }
      if (path === "/personal/vault") {
        return vaultUnreachable
          ? Response.json({ code: "unavailable" }, { status: 503 })
          : Response.json({ state: vaultReady ? "ready" : "setup_required", site_origin: vaultOrigin });
      }
      if (path === "/personal/google/start" || path === "/personal/google/drive/start")
        return Response.json({ flow_id: "a".repeat(64), authorization_url: "https://accounts.google.com/o/oauth2/v2/auth" });
      if (path === "/personal/google/complete") {
        connectedMailbox = "a@example.test";
        return Response.json({
          connection_id: tid,
          epoch: 1,
          mailbox: "a@example.test",
          flow_id: "a".repeat(64),
          continuation: { thread_id: tid, request_id: bid },
        });
      }
      if (path === "/personal/google/drive/complete")
        return Response.json({
          connection_id: tid,
          epoch: 1,
          mailbox: "a@example.test",
          flow_id: "a".repeat(64),
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
        return Response.json({
          enabled: capEnabled,
          capability: "text_only_preview",
          tools: false,
          automatic_retry: false,
          ...(obsoleteCapabilityFields ? { model: "synthetic-admin-model", effort: "synthetic-effort" } : {}),
        });
      if (path === "/threads" && method === "GET")
        return inventoryFailure ? new Response(null, { status: 503 }) : Response.json({ threads });
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
      if (/^\/threads\/[^/]+\/browser(?:\/|$)/.test(path)) {
        return browserRoute?.(path, method, body) ?? Response.json({ code: "not_found" }, { status: 404 });
      }
      if (path.endsWith("/text-follow-ups") && method === "POST") return saveFollowUp?.(body);
      if (path.match(/^\/threads\/[^/]+\/text-preview$/) && method === "POST") {
        if (previewRefused)
          return Response.json({ code: "personal_text_model_unavailable" }, { status: 422 });
        if (body.text === "show my latest email")
          return Response.json(
            { code: "personal_inbox_requested" },
            { status: 422 },
          );
        if (deferRefusal) return new Promise<Response>(resolve => { completeSend = resolve; });
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
        if (deleteThreadResponse) return deleteThreadResponse;
        threads = threads.filter((t) => t.id !== path.split("/")[2]);
        return new Response(null, { status: 204 });
      }
      throw new Error(`Unexpected preview route ${method} ${path}`);
    }),
  );
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
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
async function openDrawer(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "대화 목록" }));
  return within(await screen.findByRole("dialog", { name: "대화 목록" }));
}

it.each([false, true])("focuses the task's original turn after the newest conversation page settles (older page: %s)", async older => {
  const user = userEvent.setup();
  const taskTurn = turn(aid, "Original task turn");
  const newerTurn = turn(bid, "Most recent message");
  newerTurn.run.id = "55555555-5555-4555-8555-555555555555";
  const id = taskTurn.run.id;
  const summary = { id, thread_id: tid, title: "Saved task", state: "completed", next_actor: "none", created_at: 1 };
  const originalFetch = globalThis.fetch;
  let holdNewest = false;
  let release: ((value: Response) => void) | undefined;
  let olderReads = 0;
  vi.stubGlobal("fetch", (url: string, init: RequestInit = {}) => {
    if (url.endsWith("/personal/tasks")) return Promise.resolve(Response.json({ items: [summary], next_before: null }));
    if (url.endsWith("/personal/tasks/" + id)) return Promise.resolve(Response.json({
      task: summary, request: "Look up the time", thread_title: "A private thread", history: [],
    }));
    if (url.endsWith("/messages?before=2")) {
      olderReads++;
      return Promise.resolve(Response.json({ thread_id: tid, turns: [taskTurn], cursor: 1, next_before: null }));
    }
    if (url.endsWith("/messages")) {
      if (holdNewest) return new Promise<Response>(resolve => { release = resolve; });
      return Promise.resolve(Response.json({ thread_id: tid, turns: [taskTurn], cursor: 1, next_before: null }));
    }
    return originalFetch(url, init);
  });
  mount();
  await screen.findByText("Original task turn");
  await user.click(screen.getByRole("button", { name: "맡긴 일" }));
  await user.click(await screen.findByRole("button", { name: /Saved task/ }));
  holdNewest = true;
  await user.click(await screen.findByRole("button", { name: /원래 대화 열기/ }));
  await waitFor(() => expect(release).toBeDefined());
  expect(olderReads).toBe(0);
  release!(Response.json({ thread_id: tid, cursor: 2, turns: older ? [newerTurn] : [taskTurn, newerTurn],
    next_before: older ? 2 : null }));
  await waitFor(() => expect(document.getElementById("member-turn-" + id)).toHaveFocus());
  expect(screen.getByText("Most recent message")).toBeVisible();
  expect(olderReads).toBe(older ? 1 : 0);
});

it("routes a browser request and guidance into one run and resumes the viewed tab with an idempotent receipt", async () => {
  const workspaceId = "55555555-5555-4555-8555-555555555555";
  const base = `/threads/${tid}/browser`;
  const firstUrl = "https://timetable.example.test/";
  const secondUrl = "https://timetable.example.test/morning";
  let work: PersonalBrowserWorkspace | null = null;
  let firstResume = true;
  let staleFrame = false;
  let freshFrame = 0;
  const replies = "Morning departures begin at 08:00.";
  const previewImage = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+tH1sAAAAASUVORK5CYII=";
  const state = () => Response.json({ configured: true, workspace: work });
  browserRoute = (path, method, body) => {
    if (path === base && method === "GET") return state();
    if (path === base && body.url) {
      expect(body).toMatchObject({ url: firstUrl, wait_for_user: true });
      const run = turn(body.request_id, body.text, "running");
      work = {
        id: workspaceId, request_id: body.request_id, run_id: run.run.id,
        thread_id: tid, state: "reading", revision: 1, title: "Timetable", url: firstUrl,
        active_tab_id: "main", reason: null, links: [],
        tabs: [{ id: "main", title: "Timetable", url: firstUrl }, { id: "morning", title: "Morning", url: secondUrl }],
      };
      turns = [run];
      return state();
    }
    if (path === base) {
      expect(body.workspace_id).toBe(workspaceId);
      turns[0].run.instructions = [{ request_id: body.request_id, text: body.text, state: "received" }];
      return state();
    }
    if (path === `${base}/frame` && method === "GET") {
      const tab = staleFrame ? "main" : work!.active_tab_id;
      staleFrame = false;
      return Response.json({ workspace_id: workspaceId, revision: work!.revision,
        tab_id: tab, frame_token: `frame-${++freshFrame}`, url: work!.url,
        png_base64: previewImage, width: 900, height: 600 });
    }
    if (path === `${base}/takeover`) {
      work = { ...work!, state: "user_control", revision: 2 };
      return Response.json({ workspace: work, control_token: "private-controller" });
    }
    if (path === `${base}/tabs`) {
      expect(body).toMatchObject({ workspace_id: workspaceId, control_token: "private-controller",
        action: "switch", tab_id: "morning", revision: 2 });
      work = { ...work!, active_tab_id: "morning", url: secondUrl, revision: 3 };
      staleFrame = true;
      return state();
    }
    if (path === `${base}/input`) {
      expect(body).toMatchObject({ workspace_id: workspaceId, control_token: "private-controller",
        frame_token: `frame-${freshFrame}`, revision: 3, input: { kind: "scroll", delta_x: 0, delta_y: 300 } });
      return state();
    }
    if (path === `${base}/resume`) {
      expect(body).toMatchObject({ workspace_id: workspaceId, control_token: "private-controller", revision: 3 });
      if (firstResume) {
        firstResume = false;
        return Promise.reject(new TypeError("synthetic lost receipt"));
      }
      work = { ...work!, state: "completed", revision: 4 };
      turns[0].run.state = "completed";
      turns[0].run.instructions[0].state = "applied";
      turns[0].assistant = [{ id: "browser-answer", text: replies }];
      return state();
    }
    if (path === `${base}/close`) {
      work = { ...work!, state: "closed", revision: 5 };
      return state();
    }
    throw new Error(`Unexpected browser route ${method} ${path}`);
  };
  const user = userEvent.setup();
  mount();
  const draft = await composer();
  await waitFor(() => expect(draft).toBeEnabled());
  fireEvent.change(draft, { target: { value: "브라우저에서 https://timetable.example.test 열어 주세요. 직접 조건을 바꿀게요." } });
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await user.click(await screen.findByRole("button", { name: "미리 보기 열기" }));
  const viewer = within(screen.getByRole("complementary", { name: "개인 브라우저 작업 공간" }));
  await viewer.findByRole("img");
  expect(sessionStorage.getItem(PREVIEW_DRAFT_KEY)).not.toContain(previewImage);
  expect(viewer.queryByRole("button", { name: "무수리에게 넘기기" })).not.toBeInTheDocument();
  expect(draft).toHaveAttribute("placeholder", "브라우저 작업에 추가 지시하기");
  fireEvent.change(draft, { target: { value: "가".repeat(667) } });
  expect(screen.getByRole("alert")).toHaveTextContent("추가 지시가 너무 깁니다");
  expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled();
  fireEvent.change(draft, { target: { value: "오전만 봐줘" } });
  await user.click(screen.getByRole("button", { name: "보내기" }));
  expect(await screen.findByText("추가 지시 접수됨 · 아직 반영 전")).toBeVisible();
  await user.click(viewer.getByRole("button", { name: "직접 조작 시작" }));
  await waitFor(() => expect(viewer.getByRole("button", { name: "무수리에게 넘기기" })).toBeEnabled());
  await user.click(viewer.getByRole("button", { name: "Morning" }));
  await viewer.findByText("브라우저 화면을 불러오지 못했습니다.");
  expect(viewer.queryByRole("img")).not.toBeInTheDocument();
  await user.click(viewer.getByRole("button", { name: "화면 새로고침" }));
  const image = await viewer.findByRole("img");
  vi.spyOn(image, "getBoundingClientRect").mockReturnValue({ width: 450, height: 300 } as DOMRect);
  fireEvent.touchStart(image, { touches: [{ clientX: 60, clientY: 220 }] });
  fireEvent.touchEnd(image, { changedTouches: [{ clientX: 60, clientY: 70 }] });
  await waitFor(() => expect(calls.some((call) => call.path === `${base}/input`)).toBe(true));
  await waitFor(() => expect(viewer.getByRole("button", { name: "무수리에게 넘기기" })).toBeEnabled());
  await user.click(viewer.getByRole("button", { name: "무수리에게 넘기기" }));
  await user.click(await viewer.findByRole("button", { name: "넘긴 결과 다시 확인" }));
  expect(await screen.findByText(replies)).toBeVisible();
  expect(screen.getByText("추가 지시 반영됨")).toBeVisible();
  const resumes = calls.filter((call) => call.path === `${base}/resume`);
  expect(resumes.map((call) => call.body)).toEqual([resumes[0].body, resumes[0].body]);
  expect(calls.some((call) => call.path === `/threads/${tid}/text-preview`)).toBe(false);
  expect(sessionStorage.getItem(PREVIEW_DRAFT_KEY)).not.toContain("private-controller");
  await user.click(viewer.getByRole("button", { name: "뷰어 닫기" }));
  expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "미리 보기 열기" })).toHaveFocus();
  expect(calls.some((call) => call.path === `${base}/close`)).toBe(false);
  await user.click(screen.getByRole("button", { name: "미리 보기 열기" }));
  await user.click(within(screen.getByRole("complementary", { name: "개인 브라우저 작업 공간" })).getByRole("button", { name: "작업 중지" }));
  expect(await screen.findAllByText("브라우저 작업 중지됨")).toHaveLength(2);
});

it("keeps an ordinary reference in chat and preserves a browser draft when the capability is unavailable", async () => {
  const user = userEvent.setup();
  mount();
  const draft = await composer();
  await waitFor(() => expect(draft).toBeEnabled());
  fireEvent.change(draft, { target: { value: "Translate this sentence: our address is https://example.test." } });
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText(/Synthetic reply/);
  fireEvent.change(draft, { target: { value: "Open example.test in the browser" } });
  await user.click(screen.getByRole("button", { name: "보내기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("브라우저 작업을 사용할 수 없습니다");
  expect(draft).toHaveValue("Open example.test in the browser");
  const posts = calls.filter((call) => call.method === "POST" && /\/(?:text-preview|browser)$/.test(call.path));
  expect(posts.map((call) => call.path)).toEqual([`/threads/${tid}/text-preview`, `/threads/${tid}/browser`]);
  expect(posts[1].body.url).toBe("https://example.test/");
});

it.each(["HTTP refusal", "unavailable workspace"])("restores a rejected browser start with its next draft and retries it with chat disabled (%s)", async refusal => {
  const base = `/threads/${tid}/browser`;
  let release!: (response: Response) => void;
  let posted = 0;
  const url = "https://example.test/";
  browserRoute = (path, method, body) => {
    if (path !== base) throw Error("Unexpected browser path");
    if (method === "GET") return Response.json({ configured: true, workspace: null });
    if (++posted === 1) return new Promise(resolve => { release = resolve; });
    return Response.json({ configured: true, workspace: { id: bid, thread_id: tid, request_id: body.request_id,
      run_id: aid, state: "completed", revision: 1, title: "Example", url, active_tab_id: "main", tabs: [], links: [] } });
  };
  const user = userEvent.setup();
  const first = mount();
  await user.click(await screen.findByRole("button", { name: "브라우저 작업" }));
  await user.type(await composer(), url);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(posted).toBe(1));
  await user.type(await composer(), "Next message");
  await act(async () => release(refusal === "HTTP refusal" ? new Response(null, { status: 409 })
    : Response.json({ configured: false, workspace: null })));
  await screen.findByRole("button", { name: "이전 메시지 다시 보내기" });
  first.unmount();
  capEnabled = false;
  mount();
  const retry = await screen.findByRole("button", { name: "이전 메시지 다시 보내기" });
  expect(await composer()).toHaveValue("Next message");
  expect(posted).toBe(1);
  await user.click(retry);
  await waitFor(() => expect(screen.queryByRole("button", { name: "이전 메시지 다시 보내기" })).toBeNull());
  const posts = calls.filter(call => call.path === base && call.method === "POST");
  expect(posts.map(c => c.body)).toEqual([posts[0].body, posts[0].body]);
  expect(posts[0].body.url).toBe(url);
  expect(await composer()).toHaveValue("Next message");
  expect(chatPosts()).toHaveLength(0);
});

it("sends a browser-like draft as chat when the member explicitly switches back to chat", async () => {
  browserRoute = () => Response.json({ configured: true, workspace: null });
  const user = userEvent.setup();
  mount();
  const draft = await composer();
  await user.click(await screen.findByRole("button", { name: "브라우저 작업" }));
  const text = "Open example.test in the browser";
  fireEvent.change(draft, { target: { value: text } });
  await user.click(screen.getByRole("button", { name: "일반 채팅으로 전환" }));
  expect(draft).toHaveValue(text);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText(/Synthetic reply/);
  const posts = calls.filter((call) => call.method === "POST" && /\/(?:text-preview|browser)$/.test(call.path));
  expect(posts.map((call) => call.path)).toEqual([`/threads/${tid}/text-preview`]);
  expect(posts[0].body.text).toBe(text);
});

it("renews an idle frame proof before Done, disallows old gestures, and retains control after a busy response", async () => {
  vi.useFakeTimers();
  const base = `/threads/${tid}/browser`;
  const work: PersonalBrowserWorkspace = {
    id: bid, request_id: aid, run_id: tid, thread_id: tid, state: "ready", revision: 1,
    title: "Timetable", url: "https://timetable.example.test/", active_tab_id: "main",
    tabs: [], links: [], reason: null,
  };
  const status = () => Response.json({ configured: true, workspace: work });
  let proof = 0;
  let minted = 0;
  let firstResume = true;
  browserRoute = (path) => {
    if (path === base) return status();
    if (path === `${base}/frame`) {
      const now = performance.now();
      if (!proof || now >= minted + 15000) {
        proof++;
        minted = now;
      }
      return Response.json({ workspace_id: work.id, tab_id: "main", revision: work.revision,
        frame_token: `proof-${proof}`,
        png_base64: "QUJD", width: 900, height: 600, url: work.url });
    }
    if (path === `${base}/takeover`) {
      work.state = "user_control";
      work.revision++;
      return Response.json({ workspace: work, control_token: "held-controller" });
    }
    if (path === `${base}/resume`) {
      if (firstResume) {
        firstResume = false;
        return Response.json({ code: "browser_busy" }, { status: 429 });
      }
      work.state = "completed";
      work.revision++;
      return status();
    }
    throw new Error(`Unexpected browser route ${path}`);
  };
  const tick = async (milliseconds = 0) => act(async () => {
    await vi.advanceTimersByTimeAsync(milliseconds);
  });
  mount();
  await tick();
  fireEvent.click(screen.getByRole("button", { name: "미리 보기 열기" }));
  await tick();
  fireEvent.click(screen.getByRole("button", { name: "직접 조작 시작" }));
  await tick();
  const done = () => screen.getByRole("button", { name: "무수리에게 넘기기" });
  expect(done()).toBeEnabled();
  const frame = screen.getByRole("img", { name: "개인 브라우저의 현재 화면" });
  vi.spyOn(frame, "getBoundingClientRect").mockReturnValue({ width: 450, height: 300 } as DOMRect);
  fireEvent.pointerDown(frame);
  await tick(31000);
  expect(proof).toBe(2);
  expect(calls.filter((call) => call.path === `${base}/frame`).length).toBeGreaterThan(3);
  fireEvent.click(frame, { clientX: 50, clientY: 50 });
  expect(calls.some((call) => call.path === `${base}/input`)).toBe(false);
  expect(done()).toBeEnabled();
  fireEvent.click(done());
  await tick();
  expect(screen.getByRole("alert")).toHaveTextContent("잠시 후 다시 시도해 주세요");
  expect(done()).toBeEnabled();
  expect(calls.filter((call) => call.path === `${base}/resume`)).toHaveLength(1);
  fireEvent.click(done());
  await tick();
  expect(calls.filter((call) => call.path === `${base}/resume`).map((call) => ({
    proof: call.body.frame_token, controller: call.body.control_token,
  }))).toEqual(Array(2).fill({ proof: "proof-2", controller: "held-controller" }));
  expect(calls.filter((call) => call.path === `${base}/takeover`)).toHaveLength(1);
});

it("keeps a member's draft on an engine refusal and exposes no settings entry", async () => {
  previewRefused = true;
  obsoleteCapabilityFields = true;
  const user = userEvent.setup();
  mount();
  const draft = await composer();
  await waitFor(() => expect(draft).toBeEnabled());
  await user.type(draft, "A personal request");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("입력을 유지했습니다.");
  expect(draft).toHaveValue("A personal request");
  expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled();
  expect(screen.queryByRole("button", { name: "관리" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "엔진 설정 열기" })).not.toBeInTheDocument();
  expect(screen.queryByText(/synthetic-admin-model|synthetic-effort/)).not.toBeInTheDocument();
  expect(calls.some(call => call.path.startsWith("/personal/admin/"))).toBe(false);
});

it("lets the administrator recover and save one installation model after a revision conflict", async () => {
  current = { ...session(), installation_admin: true };
  const path = "/personal/admin/engine";
  const high = { model: "synthetic-model", effort: "high" };
  const low = { ...high, effort: "low" };
  let choice = high;
  let revision = 4;
  let connection = "connection-four";
  let stale = true;
  let enabled = false;
  adminEngine = (request, method, body) => {
    if (request === "/personal/admin/members") return Response.json({ members: [] });
    if (request !== path) return Response.json({ code: "not_found" }, { status: 404 });
    if (method === "POST") {
      revision++;
      if (connection === "connection-four") {
        connection = "connection-five";
        return Response.json({ code: "conflict" }, { status: 409 });
      }
      enabled = body.enabled;
      choice = body.default_choice;
      stale = false;
    }
    return Response.json({
      configured: true, enabled, revision, connection_revision: connection, kind: "chatgpt",
      host_default: low, choices: [low, high], default_choice: choice, default_stale: stale,
    });
  };
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "관리" }));
  const beforeEngine = calls.length;
  await user.click(screen.getByRole("button", { name: "어시스턴트 엔진" }));
  async function save() {
    const editor = within(await screen.findByRole("form", { name: "설치 모델 설정" }));
    expect(editor.getByRole("status")).toHaveTextContent("엔진 연결이 바뀌었습니다.");
    await user.selectOptions(editor.getByLabelText("추론 강도"), "low");
    await user.click(editor.getByRole("checkbox", { name: "새 모델 요청 허용" }));
    await user.click(editor.getByRole("button", { name: "설치 설정 저장" }));
  }
  await save();
  expect(await screen.findByRole("alert")).toHaveTextContent("엔진 설정이 변경되었습니다.");
  await save();
  await screen.findByText(/엔진 설정을 저장했습니다/);
  expect(screen.getByText("synthetic-model · 낮음")).toBeInTheDocument();
  expect(screen.getByText("허용")).toBeInTheDocument();
  expect(screen.queryByRole("form", { name: "회원 설정" })).not.toBeInTheDocument();
  const engineCalls = calls.slice(beforeEngine).filter(call => call.path.startsWith("/personal/admin/"));
  expect(engineCalls.every(call => call.path === path && call.account === aid)).toBe(true);
  expect(engineCalls.filter(call => call.method === "POST").map(call => call.body)).toEqual([
    { revision: 4, connection_revision: "connection-four", enabled: true, default_choice: low },
    { revision: 5, connection_revision: "connection-five", enabled: true, default_choice: low },
  ]);
});
it("enables the retained draft after an administrator resumes the engine and returns via Memory", async () => {
  current = { ...session(), installation_admin: true };
  let revision = 1;
  const choice = { model: "synthetic-model", effort: "low" };
  adminEngine = (path, method, body) => {
    if (path !== "/personal/admin/engine") return Response.json({ members: [] });
    if (method === "POST") {
      capEnabled = body.enabled;
      revision++;
    }
    return Response.json({ configured: true, enabled: capEnabled, revision, connection_revision: "test",
      kind: "chatgpt", host_default: choice, choices: [choice], default_choice: choice, default_stale: false });
  };
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "Waiting for the engine");
  capEnabled = false;
  fireEvent.focus(window);
  await waitFor(() => expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled());
  expect(await composer()).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "엔진 설정 열기" }));
  const settings = within(await screen.findByRole("form", { name: "설치 모델 설정" }));
  await user.click(settings.getByRole("checkbox", { name: "새 모델 요청 허용" }));
  await user.click(settings.getByRole("button", { name: "설치 설정 저장" }));
  await screen.findByText(/엔진 설정을 저장했습니다/);
  await user.click(screen.getByRole("button", { name: "기억" }));
  await user.click(screen.getByRole("button", { name: "대화" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "보내기" })).toBeEnabled());
  expect(await composer()).toHaveValue("Waiting for the engine");
  expect(chatPosts()).toHaveLength(0);
});

it("rechecks a paused engine when an existing member tab regains focus, including an empty workspace", async () => {
  threads = [];
  const user = userEvent.setup();
  mount();
  const editor = await composer();
  await user.type(editor, "Member draft");
  capEnabled = false;
  fireEvent.focus(window);
  await waitFor(() => expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled());
  expect(editor).toBeEnabled();
  capEnabled = true;
  fireEvent.focus(window);
  await waitFor(() => expect(editor).toBeEnabled());
  expect(editor).toHaveValue("Member draft");
  expect(calls.some(call => call.path.startsWith("/personal/admin/"))).toBe(false);
  expect(chatPosts()).toHaveLength(0);
});

it("ignores an older capability response after a newer focus check enables chat", async () => {
  capEnabled = false;
  mount();
  const editor = await composer();
  await waitFor(() => expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled());
  expect(editor).toBeEnabled();
  const before = calls.filter(call => call.path === "/text-preview").length;
  let finish!: (response: Response) => void;
  pendingCapability = new Promise(resolve => { finish = resolve; });
  fireEvent.focus(window);
  await waitFor(() => expect(calls.filter(call => call.path === "/text-preview").length).toBeGreaterThan(before));
  pendingCapability = undefined;
  capEnabled = true;
  fireEvent.focus(window);
  await waitFor(() => expect(editor).toBeEnabled());
  await act(async () => { finish(Response.json({ enabled: false, capability: "text_only_preview", tools: false })); });
  expect(editor).toBeEnabled();
});

it("returns the Google setup header to the roster and keeps the explicit catalog shortcut", async () => {
  current!.installation_admin = true;
  adminEngine = path => path.endsWith("/google-setup")
    ? Response.json({ configured: true, redirect_uri: location.origin + "/oauth/google/callback" })
    : Response.json({ members: [] });
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "관리" }));
  await user.click(await screen.findByRole("button", { name: "Google Workspace 설정 열기" }));
  await user.click(await screen.findByRole("button", { name: "뒤로" }));
  await screen.findByRole("heading", { name: "회원 관리" });
  await user.click(await screen.findByRole("button", { name: "Google Workspace 설정 열기" }));
  await user.click(await screen.findByRole("button", { name: "커넥터 열기" }));
  await screen.findByRole("heading", { name: "커넥터" });
});

it("opens the Google installer inside the connector list and returns to member connectors", async () => {
  current = { ...session(), installation_admin: true };
  const path = "/personal/admin/google-setup";
  adminEngine = (request) => {
    if (request === "/personal/admin/members") return Response.json({ members: [] });
    expect(request).toBe(path);
    return Response.json({ configured: true, redirect_uri: "https://kiri.example.test/api/v1/mail/oauth/callback" });
  };
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  await screen.findByText("설치 설정 완료");
  await user.click(screen.getByRole("button", { name: "Google Workspace 설정" }));
  await screen.findByText("Google 앱 설정이 등록되어 있습니다.");
  expect(calls.filter(call => call.path === path).every(call => call.account === aid && call.method === "GET")).toBe(true);
  await user.click(screen.getByRole("button", { name: "커넥터 열기" }));
  expect(await screen.findByRole("heading", { name: "커넥터" })).toBeVisible();
});
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
  await screen.findByText("무엇을 도와드릴까요?");
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
it("keeps the next draft even when its text matches the request being acknowledged", async () => {
  deferSend = true;
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "Same question");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(completeSend).not.toBeNull());
  expect(await composer()).toHaveValue("");
  await user.type(await composer(), "Same question");
  expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled();
  await act(async () => { completeSend!(Response.json(paid.values().next().value, { status: 201 })); });
  await waitFor(() => expect(screen.getByRole("button", { name: "보내기" })).toBeEnabled());
  expect(await composer()).toHaveValue("Same question");
});
it.each(["received", "lost reply", "lost before save", "settled before save"])(
  "stores a distinct text follow-up and reconciles its original instruction (%s)", async outcome => {
    const original = { ...turn(aid, "Explain a queue", "running") };
    turns = [{ ...original, run: { ...original.run, attempt: 1, accepts_text_follow_up: true } }];
    const saved: any[] = [];
    saveFollowUp = body => {
      saved.push(body);
      if (outcome === "settled before save") {
        turns[0].run.state = "completed";
        turns[0].run.accepts_text_follow_up = false;
        return Response.json({ code: "conflict" }, { status: 409 });
      }
      if (outcome === "lost before save" && saved.length === 1) throw new TypeError("lost before admission");
      const receipt = { ...body.instruction, thread_id: tid, run_id: original.run.id, attempt: 1,
        sequence: 1, state: "closed", reason: "text_follow_up_saved" };
      turns[0].run.instructions = [receipt];
      if (outcome === "lost reply") throw new TypeError("lost after admission");
      return Response.json(receipt);
    };
    const user = userEvent.setup();
    let view = mount();
    await user.type(await composer(), "Use a bakery example");
    await user.click(await screen.findByRole("button", {name:"후속 지시 저장"}));
    if (outcome === "settled before save") {
      await user.click(await screen.findByRole("button", {name:"작성창으로 가져오기"}));
      expect(await composer()).toHaveValue("Use a bakery example");
      expect(chatPosts()).toHaveLength(0);
      return;
    }
    if (outcome === "lost before save") {
      await screen.findByRole("button", {name:"같은 지시 저장 다시 확인"});
      view.unmount(); view = mount();
      await user.click(await screen.findByRole("button", {name:"같은 지시 저장 다시 확인"}));
      expect(saved[1]).toEqual(saved[0]);
    }
    expect(await screen.findByRole("button", {name:"후속 메시지로 보내기"})).toBeDisabled();
    expect(chatPosts()).toHaveLength(0);
    await user.type(await composer(), "Another next draft");
    view.unmount();
    turns[0].run.state = "completed";
    mount();
    expect(await screen.findByText("후속 지시 저장됨 · 이 답변에는 미반영")).toBeVisible();
    expect(chatPosts()).toHaveLength(0);
    await user.click(await screen.findByRole("button", {name:"후속 메시지로 보내기"}));
    await waitFor(() => expect(chatPosts()).toHaveLength(1));
    expect(chatPosts()[0].body).toEqual({ request_id: saved[0].instruction.request_id,
      capability: "text_only_preview", text: "Use a bakery example", follow_up_run_id: original.run.id });
    expect(await composer()).toHaveValue("Another next draft");
  },
);
it("accepts a follow-up while this tab still awaits its first broker reply", async () => {
  const user = userEvent.setup();
  deferSend = true;
  mount();
  await user.type(await composer(), "Explain queues");
  await user.click(screen.getByRole("button", {name:"보내기"}));
  await waitFor(() => expect(completeSend).not.toBeNull());
  const first = chatPosts()[0].body;
  const original = turn(first.request_id, first.text, "running");
  turns = [{...original, run: {...original.run, attempt: 1, accepts_text_follow_up: true}}];
  saveFollowUp = body => {
    const receipt = { ...body.instruction, thread_id: tid, run_id: original.run.id, attempt: 1,
      sequence: 1, state: "closed", reason: "text_follow_up_saved" };
    turns[0].run.instructions = [receipt];
    return Response.json(receipt);
  };
  await user.type(await composer(), "Add a diagram");
  await user.click(await screen.findByRole("button", {name:"후속 지시 저장"}, {timeout: 5000}));
  expect(await screen.findByText("후속 지시 저장됨 · 이 답변에는 미반영")).toBeVisible();
  expect(chatPosts()).toHaveLength(1);
  turns[0].run.state = "completed";
  turns[0].run.accepts_text_follow_up = false;
  await act(async () => { completeSend!(Response.json(turns[0], {status:201})); });
  await waitFor(() => expect(screen.getByRole("button", {name:"후속 메시지로 보내기"})).toBeEnabled());
  expect(chatPosts()).toHaveLength(1);
});
function chatPosts() {
  return calls.filter(call => call.method === "POST" && call.path.endsWith("/text-preview"));
}

async function rejectWhileTyping(user: ReturnType<typeof userEvent.setup>, rejected: string, next: string,
  refusal = Response.json({ code: "rate_limited" }, { status: 429 })) {
  deferRefusal = true;
  await user.type(await composer(), rejected);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(completeSend).not.toBeNull());
  await user.type(await composer(), next);
  await act(async () => {
    completeSend!(refusal);
  });
  deferRefusal = false;
  await screen.findByRole("button", { name: "이전 메시지 다시 보내기" });
}

it("recovers a rejected first message and its separate follow-up after reload, then explicitly retries the first", async () => {
  threads = [];
  const user = userEvent.setup();
  const view = mount();
  await rejectWhileTyping(user, "First message", "Next draft");
  expect(await composer()).toHaveValue("Next draft");
  const original = chatPosts()[0];
  view.unmount();
  mount();
  const retry = await screen.findByRole("button", { name: "이전 메시지 다시 보내기" });
  expect(within(screen.getByRole("region", { name: "대화" })).getByText("First message")).toBeVisible();
  expect(await composer()).toHaveValue("Next draft");
  expect(chatPosts()).toHaveLength(1);
  expect(paid.size).toBe(0);
  await user.click(retry);
  await screen.findByText("Synthetic reply <button>opaque tool text</button>");
  expect(chatPosts().map(call => call.body)).toEqual([original.body, original.body]);
  expect(paid.size).toBe(1);
  expect(await composer()).toHaveValue("Next draft");
  expect(screen.getByRole("button", { name: "보내기" })).toBeEnabled();
});

it("keeps both messages during an engine refusal and allows retry only when the engine returns", async () => {
  const user = userEvent.setup();
  mount();
  await rejectWhileTyping(user, "Held request", "Separate draft",
    Response.json({ code: "personal_text_model_unavailable" }, { status: 422 }));
  const retry = screen.getByRole("button", { name: "이전 메시지 다시 보내기" });
  expect(retry).toHaveAttribute("aria-disabled", "true");
  await user.click(retry);
  expect(chatPosts()).toHaveLength(1);
  expect(await composer()).toHaveValue("Separate draft");
  fireEvent.focus(window);
  await waitFor(() => expect(retry).not.toHaveAttribute("aria-disabled", "true"));
  await user.click(retry);
  await screen.findByText("Synthetic reply <button>opaque tool text</button>");
  expect(await composer()).toHaveValue("Separate draft");
  expect(chatPosts().map(call => call.body.text)).toEqual(["Held request", "Held request"]);
});

it("keeps the rejected request in its thread and dismisses only that copy of an identical next draft", async () => {
  threads.push({ id: bid, title: "Second thread", updated_at: 1 });
  const user = userEvent.setup();
  mount();
  await rejectWhileTyping(user, "Identical", "Identical");
  let drawer = await openDrawer(user);
  await user.click(drawer.getByRole("button", { name: "Second thread" }));
  expect(await composer()).toHaveValue("");
  expect(screen.queryByRole("button", { name: "이전 메시지 다시 보내기" })).toBeNull();
  drawer = await openDrawer(user);
  await user.click(drawer.getByRole("button", { name: "A private thread" }));
  await user.click(await screen.findByRole("button", { name: "이전 메시지 버리기" }));
  expect(await composer()).toHaveValue("Identical");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await screen.findByText("Synthetic reply <button>opaque tool text</button>");
  const posts = chatPosts();
  expect(posts).toHaveLength(2);
  expect(posts[1].body.text).toBe(posts[0].body.text);
  expect(posts[1].body.request_id).not.toBe(posts[0].body.request_id);
});

it("drops both rejected and unsent text when the same member returns in a new session", async () => {
  const user = userEvent.setup();
  const first = mount();
  await rejectWhileTyping(user, "Previous session request", "Previous session draft");
  first.unmount();
  current = { ...session(), chat_draft_scope: "c".repeat(64) };
  mount();
  expect(await composer()).toHaveValue("");
  expect(screen.queryByRole("button", { name: "이전 메시지 다시 보내기" })).toBeNull();
  expect(sessionStorage.getItem(PREVIEW_DRAFT_KEY)).not.toContain("Previous session");
  expect(chatPosts()).toHaveLength(1);
});

it("logout fences an in-flight response and its saved draft before another account signs in", async () => {
  deferSend = true;
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "PRIVATE A request");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(completeSend).not.toBeNull());
  expect(await composer()).toHaveValue("");
  await user.type(await composer(), "A private unsent follow-up");
  expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled();
  const drawer = await openDrawer(user);
  await user.click(drawer.getByRole("button", { name: "로그아웃" }));
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
it("keeps private history visible when chat is disabled and clears it after confirmed deletion", async () => {
  capEnabled = false;
  turns = [
    turn(
      "55555555-5555-4555-8555-555555555555",
      "Saved private request",
      "outcome_unknown",
    ),
  ];
  const user = userEvent.setup();
  mount();
  await screen.findByText("Saved private request");
  expect(
    screen.getByText("응답 확인 불가 · 자동 재시도 없음"),
  ).toBeInTheDocument();
  const draft = await composer();
  await user.type(draft, "안녕하세요");
  expect(draft).toHaveValue("안녕하세요");
  expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled();
  const drawer = await openDrawer(user);
  await user.click(drawer.getByRole("button", { name: "A private thread 삭제" }));
  await user.click(drawer.getByRole("button", { name: "대화 삭제" }));
  await drawer.findByText("아직 대화가 없습니다.");
  await user.click(drawer.getByRole("button", { name: "닫기" }));
  await screen.findByText("무엇을 도와드릴까요?");
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

it("confirms deletion of a searched conversation without selecting it or losing the current draft", async () => {
  const backgroundThread = { id: bid, title: "Another conversation", created_at: 1, updated_at: 1 };
  threads.push(backgroundThread);
  const user = userEvent.setup();
  mount();
  const editor = await composer();
  await user.type(editor, "Unsaved draft");
  const drawer = await openDrawer(user);
  const search = drawer.getByRole("searchbox", { name: "대화 검색" });
  await user.type(search, "another");
  const rowDelete = drawer.getByRole("button", { name: "Another conversation 삭제" });
  await user.click(rowDelete);
  const confirm = drawer.getByRole("region", { name: "대화를 삭제할까요?" });
  expect(within(confirm).getByText(backgroundThread.title)).toBeVisible();
  expect(within(confirm).getByRole("button", { name: "취소" })).toHaveFocus();
  await user.click(within(confirm).getByRole("button", { name: "취소" }));
  expect(rowDelete).toHaveFocus();
  expect(calls.some((c) => c.method === "DELETE")).toBe(false);

  deleteThreadResponse = Response.json({ code: "unavailable" }, { status: 503 });
  await user.click(rowDelete);
  await user.click(drawer.getByRole("button", { name: "대화 삭제" }));
  await drawer.findByRole("alert");
  expect(threads).toHaveLength(2);
  deleteThreadResponse = undefined;
  inventoryFailure = false;
  await user.click(drawer.getByRole("button", { name: "대화 삭제" }));
  await drawer.findByText("일치하는 대화가 없습니다.");
  expect(search).toHaveValue("another");
  expect(search).toHaveFocus();
  await user.click(drawer.getByRole("button", { name: "닫기" }));

  expect(screen.getByRole("heading", { name: "A private thread" })).toBeVisible();
  expect(editor).toHaveValue("Unsaved draft");
  expect(calls.filter((c) => c.method === "DELETE").map(({ path, account }) => ({ path, account })))
    .toEqual(Array(2).fill({ path: `/threads/${bid}`, account: aid }));
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
it("shows actual service connections and disconnects Gmail without reading mail", async () => {
  vaultReady = false;
  vaultOrigin = undefined;
  const user = userEvent.setup();
  mount();
  const nav = await screen.findByRole("button", { name: "연결" });
  expect(screen.queryByRole("button", { name: "메일" })).not.toBeInTheDocument();
  await user.click(nav);
  expect(nav).toHaveAttribute("aria-current", "page");
  await screen.findByRole("heading", { name: "커넥터" });
  expect(screen.getByRole("button", { name: "전체" })).toHaveAttribute("aria-pressed", "true");
  await user.click(screen.getByRole("button", { name: "연결됨" }));
  expect(screen.queryByRole("button", { name: /볼트/ })).not.toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: /Gmail/ }));
  const gmail = within(screen.getByRole("region", { name: "Gmail" }));
  expect(await gmail.findByText("mailbox@example.test")).toBeInTheDocument();
  expect(gmail.getByRole("status")).toHaveTextContent("연결됨");
  expect(screen.queryByRole("button", { name: "메일 확인" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "연결 해제" }));
  expect(await gmail.findByText("연결된 계정이 없습니다.")).toBeInTheDocument();
  expect(gmail.queryByText("mailbox@example.test")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Google 연결" })).toBeDisabled();
  expect(gmail.getByText("이 설치에 등록된 볼트 주소가 없습니다. 설치 관리자에게 문의해 주세요.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(screen.getByRole("button", { name: "연결됨" })).toHaveAttribute("aria-pressed", "true");
  await screen.findByText("연결된 서비스가 없습니다.");
  await user.click(screen.getByRole("button", { name: "전체" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  const vault = within(screen.getByRole("region", { name: "볼트" }));
  expect(await vault.findByText("볼트 주소 미설정")).toBeInTheDocument();
  expect(vault.queryByText("a@example.test")).not.toBeInTheDocument();
  expect(vault.queryByRole("link")).not.toBeInTheDocument();
  expect(vault.queryByRole("button")).not.toBeInTheDocument();
  expect(calls).toContainEqual({ path: "/personal/google/disconnect", method: "POST",
    body: { connection_id: tid, epoch: 1 }, account: aid });
  for (const path of ["/personal/google", "/personal/vault"]) {
    expect(calls).toContainEqual({ path, method: "GET", body: null, account: aid });
  }
  expect(calls.some(({ path }) => path === "/personal/google/inbox")).toBe(false);
});

it("distinguishes unavailable vault status from confirmed setup requirements", async () => {
  vaultReady = false;
  vaultOrigin = undefined;
  vaultUnreachable = true;
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  const vault = within(screen.getByRole("region", { name: "볼트" }));
  await vault.findByText("연결 상태를 확인할 수 없습니다.");
  expect(vault.queryByText("볼트 주소 미설정")).not.toBeInTheDocument();
  vaultUnreachable = false;
  await user.click(screen.getByRole("button", { name: "다시 확인" }));
  await vault.findByText("볼트 주소 미설정");
  expect(vault.queryByRole("button")).not.toBeInTheDocument();
});

it("opens the configured independent HTTPS vault while keeping member setup status", async () => {
  vaultReady = false;
  vaultOrigin = "https://vault.example.test";
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  await screen.findByText("별도 로그인");
  const link = screen.getByRole("link", { name: "볼트 열기" });
  expect(link).toHaveAttribute("href", "https://vault.example.test");
  expect(link).toHaveAttribute("target", "_blank");
  expect(link).toHaveAttribute("rel", "noopener noreferrer");
  expect(screen.getByText("볼트는 새 탭에서 열립니다. 볼트 계정으로 로그인하세요.")).toBeInTheDocument();
});

it.each([
  "vault.example.test", "http://vault.example.test", location.origin,
  "https://user:password@vault.example.test", "https://vault.example.test/account",
  "https://vault.example.test?user=test", "https://vault.example.test#signin",
])("does not link to an unsafe vault origin (%s)", async (origin) => {
  vaultReady = false;
  vaultOrigin = origin;
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  await screen.findByText("볼트 주소 미설정");
  expect(screen.queryByRole("link", { name: "볼트 열기" })).not.toBeInTheDocument();
});

it("searches supported services and returns to the filtered list from details", async () => {
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  const search = screen.getByRole("searchbox", { name: "커넥터 검색" });
  await user.type(search, "slack");
  await screen.findByText("검색어와 일치하는 서비스가 없습니다.");
  await user.clear(search);
  await user.type(search, "구글 문서");
  expect(screen.queryByRole("button", { name: /Gmail/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Google Docs/ }));
  expect(screen.getByRole("heading", { name: "Google Docs" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Google Docs" })).toHaveTextContent("Docs 문서");
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(screen.getByRole("searchbox", { name: "커넥터 검색" })).toHaveValue("구글 문서");
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(await composer()).toBeInTheDocument();
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
it.each(["separate draft", "Check mail"])("recovers an abandoned Gmail handoff by its original receipt after reload and preserves the next draft (%s)", async next => {
  const user = userEvent.setup();
  const first = mount();
  connectedMailbox = null;
  deferRefusal = true;
  await user.type(await composer(), "Check mail");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(completeSend).not.toBeNull());
  await user.type(await composer(), next);
  await act(async () => completeSend!(Response.json({ code: "personal_inbox_requested" }, { status: 422 })));
  deferRefusal = false;
  await screen.findByRole("button", { name: "Google 연결하고 메일 확인" });
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  const request = chatPosts()[0].body.request_id;
  await screen.findByRole("button", { name: "메일 확인 계속하기" });
  first.unmount();
  connectedMailbox = "a@example.test";
  capEnabled = false;
  mount();
  const resume = await screen.findByRole("button", { name: "메일 확인 계속하기" });
  expect(await composer()).toHaveValue(next);
  expect(calls.filter(c => c.path === "/personal/google/inbox")).toHaveLength(0);
  await user.click(resume);
  await screen.findByText("확인한 페이지에 받은 메일이 없습니다.");
  expect(calls.filter(c => c.path === "/personal/google/inbox").map(c => c.body.read)).toEqual([{ thread_id: tid, request_id: request }]);
  expect(chatPosts()).toHaveLength(1);
  expect(await composer()).toHaveValue(next);
  expect(screen.queryByRole("button", { name: "메일 확인 계속하기" })).toBeNull();
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
  await user.click(screen.getByRole("button", { name: "연결" }));
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
  expect(await composer()).toBeEnabled();
  expect(screen.getByRole("button", { name: "보내기" })).toBeDisabled();
  await act(async () => {
    finish(Response.json({ code: "unavailable" }, { status: 503 }));
  });
  expect(
    await screen.findByText("대화 가능 상태를 확인하지 못했습니다. 상태를 새로고침해 주세요."),
  ).toBeInTheDocument();
  expect(screen.getByText("Existing conversation")).toBeInTheDocument();
});

it("restores focus when choosing history and keeps the previous thread's draft", async () => {
  const newThread = { id: bid, title: "Another conversation", created_at: 1, updated_at: 1 };
  threads.push(newThread);
  const user = userEvent.setup();
  mount();
  const editor = await composer();
  await waitFor(() => expect(editor).toBeEnabled());
  await user.type(editor, "Draft in the first conversation");

  let drawer = await openDrawer(user);
  await user.click(drawer.getByRole("button", { name: newThread.title }));
  const trigger = screen.getByRole("button", { name: "대화 목록" });
  await waitFor(() => expect(trigger).toHaveAttribute("aria-expanded", "false"));
  expect(trigger).toHaveFocus();
  expect(await composer()).toHaveValue("");

  drawer = await openDrawer(user);
  await user.click(drawer.getByRole("button", { name: "A private thread" }));
  expect(await composer()).toHaveValue("Draft in the first conversation");
});

it("does not submit an Enter that completes Korean composition", async () => {
  const user = userEvent.setup();
  mount();
  const editor = await composer();
  await waitFor(() => expect(editor).toBeEnabled());
  await user.type(editor, "한글 입력");
  fireEvent.compositionStart(editor);
  fireEvent.keyDown(editor, { key: "Enter" });
  fireEvent.compositionEnd(editor);
  expect(paid.size).toBe(0);

  await user.keyboard("{Shift>}{Enter}{/Shift}");
  expect(paid.size).toBe(0);
  await user.keyboard("{Enter}");
  await waitFor(() => expect(paid.size).toBe(1));
});

it("puts a starter in the focused composer and keeps it through navigation without sending it", async () => {
  threads = [];
  const user = userEvent.setup();
  mount();
  const example = await screen.findByRole("button", { name: "문장 다듬기" });
  await user.click(example);
  const editor = await composer();
  expect(editor).toHaveFocus();
  expect(editor).toHaveValue("다음 문장을 자연스럽게 다듬어 줘.\n");
  expect(screen.queryByRole("button", { name: "계획 세우기" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "기억" }));
  await user.click(screen.getByRole("button", { name: "대화" }));
  expect(await composer()).toHaveValue("다음 문장을 자연스럽게 다듬어 줘.\n");
  expect(calls.some(({ path, method }) => path === "/threads" && method === "POST")).toBe(false);
});

it("groups conversations by local dates and keeps a draft available from a keyboard search", async () => {
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  threads[0].updated_at = Math.floor(today.getTime() / 1000);
  threads.push({ id: bid, title: "Another conversation", updated_at: Math.floor(yesterday.getTime() / 1000) });
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "Synthetic draft");
  const drawer = await openDrawer(user);
  expect(drawer.getByRole("heading", { name: "오늘" })).toBeVisible();
  expect(drawer.getByRole("heading", { name: "어제" })).toBeVisible();
  expect(drawer.getByRole("button", { name: "A private thread" })).toHaveAccessibleDescription("작성 중인 메시지 있음");
  const search = drawer.getByRole("searchbox", { name: "대화 검색" });
  await user.type(search, "Another");
  fireEvent.keyDown(search, { key: "Enter", isComposing: true });
  expect(search).toBeVisible();
  await user.keyboard("{Enter}");
  expect(screen.getByRole("heading", { name: "Another conversation" })).toBeVisible();
  const reopened = await openDrawer(user);
  await user.click(reopened.getByRole("button", { name: "A private thread" }));
  expect(await composer()).toHaveValue("Synthetic draft");
});

it("makes an unavailable inventory distinguishable from empty history and retries it", async () => {
  inventoryFailure = true;
  const user = userEvent.setup();
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent("대화 목록을 불러오지 못했습니다.");
  expect(await composer()).toBeDisabled();
  const drawer = await openDrawer(user);
  expect(drawer.getByRole("alert")).toHaveTextContent("대화 목록을 불러오지 못했습니다.");
  expect(drawer.queryByText("아직 대화가 없습니다.")).not.toBeInTheDocument();
  inventoryFailure = false;
  await user.click(drawer.getByRole("button", { name: "다시 시도" }));
  expect(await drawer.findByRole("button", { name: "A private thread" })).toBeVisible();
});

it("restores connector detail on reload and returns to the originating memory view", async () => {
  const user = userEvent.setup();
  const first = mount();
  await composer();
  const menu = () => within(screen.getByRole("navigation", { name: "주 메뉴" }));
  await user.click(menu().getByRole("button", { name: "기억" }));
  await user.click(menu().getByRole("button", { name: "연결" }));
  await user.click(await screen.findByRole("button", { name: /^Gmail/ }));
  expect(location.hash).toBe("#connections/gmail");
  first.unmount();
  mount();
  await screen.findByRole("heading", { name: "Gmail" });
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  await screen.findByRole("heading", { name: "커넥터" });
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  await waitFor(() => expect(menu().getByRole("button", { name: "기억" })).toHaveAttribute("aria-current", "page"));
  expect(calls.some(call => call.path === "/personal/google/inbox")).toBe(false);
});

it.each([
  ["Gmail", "memory", "gmail"],
  ["Gmail", "admin/engine", "gmail"],
  ["Google Drive", "memory", "drive"],
])("finishes %s inside Kiri and returns through the catalog to %s", async (service, destination, serviceId) => {
  const user = userEvent.setup();
  connectedMailbox = null;
  const fromAdmin = destination === "admin/engine";
  if (fromAdmin) {
    current = { ...session(), installation_admin: true };
    adminEngine = path => path === "/personal/admin/members" ? Response.json({ members: [] }) : Response.json({ configured: false });
  }
  const first = mount();
  await composer();
  await user.click(screen.getByRole("button", { name: fromAdmin ? "관리" : "기억" }));
  if (fromAdmin) await user.click(screen.getByRole("button", { name: "어시스턴트 엔진" }));
  await user.click(screen.getByRole("button", { name: "연결" }));
  await user.click(await screen.findByRole("button", { name: new RegExp("^" + service) }));
  const connect = await screen.findByRole("button", { name: "Google 연결" });
  await waitFor(() => expect(connect).toBeEnabled());
  await user.click(connect);
  await screen.findByText("Google 서비스 연결을 확인했습니다.");
  expect(connectInKiri).toHaveBeenCalledWith(expect.any(Function), aid, "a@example.test", serviceId,
    "https://vault.example.test", undefined, expect.any(AbortSignal), expect.any(Function), expect.objectContaining({persisted:expect.any(Function)}));
  expect(calls.filter(call => call.method === "POST" && call.path.endsWith("/complete"))).toEqual([{
    path: `/personal/google${serviceId === "gmail" ? "" : "/" + serviceId}/complete`, method: "POST",
    body: { handle: "b".repeat(64) }, account: aid,
  }]);
  expect(sessionStorage.getItem("moosuri.personal.google-read.v1")).toBeNull();
  expect(sessionStorage.getItem("moosuri.personal.google-workspace.v1")).toBeNull();
  first.unmount();
  mount();
  await screen.findByRole("heading", { name: service });
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  await screen.findByRole("heading", { name: "커넥터" });
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  await waitFor(() => expect(screen.getByRole("button", { name: fromAdmin ? "관리" : destination === "memory" ? "기억" : "대화" }))
    .toHaveAttribute("aria-current", "page"));
  if (fromAdmin) expect(location.hash).toBe("#admin/engine");
  expect(calls.some(call => call.path === "/personal/google/inbox")).toBe(false);
});

it("keeps the connector open and creates no grant when the vault-first Kiri flow is cancelled", async () => {
  connectedMailbox = null;
  vi.mocked(connectInKiri).mockRejectedValueOnce(new Error("볼트를 닫아 Google 연결을 취소했습니다."));
  const user = userEvent.setup();
  mount();
  await composer();
  await user.click(screen.getByRole("button", { name: "연결" }));
  await user.click(await screen.findByRole("button", { name: /^Gmail/ }));
  await user.click(await screen.findByRole("button", { name: "Google 연결" }));
  expect(await screen.findByText("볼트를 닫아 Google 연결을 취소했습니다.")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Gmail" })).toBeVisible();
  expect(calls.some(call => call.method === "POST" && call.path.endsWith("/complete"))).toBe(false);
});

it("opens an administrator bookmark as chat for an ordinary member", async () => {
  history.replaceState(null, "", "/#admin/engine");
  mount();
  await composer();
  expect(screen.queryByRole("button", { name: "관리" })).not.toBeInTheDocument();
  expect(calls.some(call => call.path.startsWith("/personal/admin/"))).toBe(false);
});
