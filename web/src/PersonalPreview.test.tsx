import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, cleanup, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import PersonalPreview, { PREVIEW_DRAFT_KEY } from "./PersonalPreview";
import type { ThreadGoogleRead } from "./PersonalGoogle";
import type { PersonalBrowserWorkspace } from "./personal-browser";
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
let vaultOrigin: string | undefined;
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
let adminEngine: ((path: string, method: string, body: any) => Response) | undefined;
let previewRefused: boolean;
let obsoleteCapabilityFields: boolean;
let browserRoute: ((path: string, method: string, body: any) => Response | Promise<Response>) | undefined;
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
  vaultOrigin = undefined;
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
  adminEngine = undefined;
  previewRefused = false;
  obsoleteCapabilityFields = false;
  browserRoute = undefined;
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
          : Response.json({ state: "setup_required", site_origin: vaultOrigin });
      }
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
        return Response.json({
          enabled: capEnabled,
          capability: "text_only_preview",
          tools: false,
          automatic_retry: false,
          ...(obsoleteCapabilityFields ? { model: "synthetic-admin-model", effort: "synthetic-effort" } : {}),
        });
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
      if (/^\/threads\/[^/]+\/browser(?:\/|$)/.test(path)) {
        return browserRoute?.(path, method, body) ?? Response.json({ code: "not_found" }, { status: 404 });
      }
      if (path.match(/^\/threads\/[^/]+\/text-preview$/) && method === "POST") {
        if (previewRefused)
          return Response.json({ code: "personal_text_model_unavailable" }, { status: 422 });
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
  await user.click(screen.getByRole("button", { name: "대화 및 설정" }));
  return within(await screen.findByRole("dialog", { name: "대화 및 설정" }));
}

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
  expect(viewer.queryByRole("button", { name: "완료" })).not.toBeInTheDocument();
  fireEvent.change(draft, { target: { value: "오전만 봐줘" } });
  await user.click(screen.getByRole("button", { name: "보내기" }));
  expect(await screen.findByText("추가 지시 접수됨 · 아직 반영 전")).toBeVisible();
  await user.click(viewer.getByRole("button", { name: "브라우저 제어" }));
  await waitFor(() => expect(viewer.getByRole("button", { name: "완료" })).toBeEnabled());
  await user.click(viewer.getByRole("button", { name: "Morning" }));
  await viewer.findByText("브라우저 화면을 불러오지 못했습니다.");
  expect(viewer.queryByRole("img")).not.toBeInTheDocument();
  await user.click(viewer.getByRole("button", { name: "새로고침" }));
  const image = await viewer.findByRole("img");
  vi.spyOn(image, "getBoundingClientRect").mockReturnValue({ width: 450, height: 300 } as DOMRect);
  fireEvent.touchStart(image, { touches: [{ clientX: 60, clientY: 220 }] });
  fireEvent.touchEnd(image, { changedTouches: [{ clientX: 60, clientY: 70 }] });
  await waitFor(() => expect(calls.some((call) => call.path === `${base}/input`)).toBe(true));
  await waitFor(() => expect(viewer.getByRole("button", { name: "완료" })).toBeEnabled());
  await user.click(viewer.getByRole("button", { name: "완료" }));
  await user.click(await viewer.findByRole("button", { name: "완료 결과 다시 확인" }));
  expect(await screen.findByText(replies)).toBeVisible();
  expect(screen.getByText("추가 지시 반영됨")).toBeVisible();
  const resumes = calls.filter((call) => call.path === `${base}/resume`);
  expect(resumes.map((call) => call.body)).toEqual([resumes[0].body, resumes[0].body]);
  expect(calls.some((call) => call.path === `/threads/${tid}/text-preview`)).toBe(false);
  expect(sessionStorage.getItem(PREVIEW_DRAFT_KEY)).not.toContain("private-controller");
  await user.click(viewer.getByRole("button", { name: "뷰어 닫기" }));
  expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
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
  fireEvent.click(screen.getByRole("button", { name: "브라우저 제어" }));
  await tick();
  const done = () => screen.getByRole("button", { name: "완료" });
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

it("lets an installation administrator recover a stale engine choice and assign a bound member after a revision conflict", async () => {
  current = { ...session(), installation_admin: true };
  const path = "/personal/admin/engine";
  const high = { model: "synthetic-model", effort: "high" };
  const low = { ...high, effort: "low" };
  let memberChoice: typeof high | null = null;
  let revision = 4;
  let generation = 1;
  let connection = "connection-four";
  let stale = true;
  let enabled = false;
  adminEngine = (request, method, body) => {
    if (request === "/personal/admin/members")
      return Response.json({
        members: [{ account_id: bid, generation, email: "guest@example.test", ready: false, suspended: false }],
      });
    if (method === "POST") {
      if (request === path + "/member" && generation === 1) {
        generation++;
        revision++;
        connection = "connection-five";
        return Response.json({ code: "conflict" }, { status: 409 });
      }
      revision++;
      if (request === path) {
        enabled = body.enabled;
        stale = false;
      } else memberChoice = body.choice;
    }
    return Response.json({
      configured: true, enabled, revision, connection_revision: connection, kind: "chatgpt",
      host_default: low, choices: [low, high], default_choice: high, default_stale: stale,
      members: [{ account_id: bid, generation, choice: memberChoice, stale: false }],
    });
  };
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "관리" }));
  await user.click(screen.getByRole("button", { name: "어시스턴트 엔진" }));
  const installation = within(await screen.findByRole("form", { name: "설치 기본 설정" }));
  expect(installation.getByRole("status")).toHaveTextContent("엔진 연결이 바뀌었습니다.");
  await user.click(installation.getByRole("checkbox", { name: "어시스턴트 사용" }));
  await user.click(installation.getByRole("button", { name: "설치 설정 저장" }));
  await screen.findByText("엔진 설정을 저장했습니다.");
  expect(calls.find(call => call.path === path && call.method === "POST")?.body).toEqual({
    revision: 4, connection_revision: "connection-four", enabled: true, default_choice: high,
  });

  await user.selectOptions(screen.getByLabelText("회원"), bid);
  async function assign() {
    const editor = within(await screen.findByRole("form", { name: "회원 설정" }));
    await user.selectOptions(editor.getByLabelText("모델"), high.model);
    await user.selectOptions(editor.getByLabelText("추론 강도"), "high");
    await user.click(editor.getByRole("button", { name: "회원 설정 저장" }));
  }
  await assign();
  expect(await screen.findByRole("alert")).toHaveTextContent("회원 정보가 변경되었습니다.");
  await assign();
  await screen.findByText("엔진 설정을 저장했습니다.");
  const writes = calls.filter(call => call.path === path + "/member");
  expect(writes.map(call => call.body)).toEqual([
    { revision: 5, connection_revision: "connection-four", account_id: bid, generation: 1, choice: high },
    { revision: 6, connection_revision: "connection-five", account_id: bid, generation: 2, choice: high },
  ]);
  expect(writes.every(call => call.account === aid)).toBe(true);
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
it("logout fences an in-flight response and its saved draft before another account signs in", async () => {
  deferSend = true;
  const user = userEvent.setup();
  mount();
  await user.type(await composer(), "PRIVATE A request");
  await user.click(screen.getByRole("button", { name: "보내기" }));
  await waitFor(() => expect(completeSend).not.toBeNull());
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
  const drawer = await openDrawer(user);
  await user.click(drawer.getByRole("button", { name: "대화 삭제" }));
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
  const user = userEvent.setup();
  mount();
  const nav = await screen.findByRole("button", { name: "연결" });
  expect(screen.queryByRole("button", { name: "메일" })).not.toBeInTheDocument();
  await user.click(nav);
  expect(nav).toHaveAttribute("aria-current", "page");
  await screen.findByRole("heading", { name: "커넥터" });
  expect(screen.getByRole("button", { name: "사용 가능" })).toHaveAttribute("aria-pressed", "true");
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
  expect(screen.getByRole("button", { name: "Google 연결" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(screen.getByRole("button", { name: "연결됨" })).toHaveAttribute("aria-pressed", "true");
  await screen.findByText("연결된 서비스가 없습니다.");
  await user.click(screen.getByRole("button", { name: "사용 가능" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  const vault = within(screen.getByRole("region", { name: "볼트" }));
  expect(await vault.findByText("회원 인증 연결 필요")).toBeInTheDocument();
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
  vaultUnreachable = true;
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  const vault = within(screen.getByRole("region", { name: "볼트" }));
  await vault.findByText("연결 상태를 확인할 수 없습니다.");
  expect(vault.queryByText("회원 인증 연결 필요")).not.toBeInTheDocument();
  vaultUnreachable = false;
  await user.click(screen.getByRole("button", { name: "다시 확인" }));
  await vault.findByText("회원 인증 연결 필요");
  expect(vault.queryByRole("button")).not.toBeInTheDocument();
});

it("opens the configured independent HTTPS vault while keeping member setup status", async () => {
  vaultOrigin = "https://vault.example.test";
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  await screen.findByText("회원 인증 연결 필요");
  const link = screen.getByRole("link", { name: "볼트 사이트 보기" });
  expect(link).toHaveAttribute("href", "https://vault.example.test");
  expect(link).toHaveAttribute("target", "_blank");
  expect(link).toHaveAttribute("rel", "noopener noreferrer");
  expect(screen.getByText("현재 설치의 회원 세션과 볼트가 연결되지 않았습니다.")).toBeInTheDocument();
});

it.each([
  "vault.example.test", "http://vault.example.test", location.origin,
  "https://user:password@vault.example.test", "https://vault.example.test/account",
  "https://vault.example.test?user=test", "https://vault.example.test#signin",
])("does not link to an unsafe vault origin (%s)", async (origin) => {
  vaultOrigin = origin;
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  await user.click(screen.getByRole("button", { name: /볼트/ }));
  await screen.findByText("회원 인증 연결 필요");
  expect(screen.queryByRole("link", { name: "볼트 사이트 보기" })).not.toBeInTheDocument();
});

it("searches candidate services and returns to the filtered list from details", async () => {
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: "연결" }));
  const search = screen.getByRole("searchbox", { name: "커넥터 검색" });
  await user.type(search, "unknown");
  await screen.findByText("검색어와 일치하는 서비스가 없습니다.");
  await user.clear(search);
  await user.type(search, "slack");
  expect(screen.queryByRole("button", { name: /Gmail/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Slack/ }));
  expect(screen.getByRole("heading", { name: "Slack" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Slack" })).toHaveTextContent("도입 후보");
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(screen.getByRole("searchbox", { name: "커넥터 검색" })).toHaveValue("slack");
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
  expect(await composer()).toBeDisabled();
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
  const trigger = screen.getByRole("button", { name: "대화 및 설정" });
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
