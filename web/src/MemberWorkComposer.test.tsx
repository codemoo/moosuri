import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import PersonalPreview, { MEMBER_WORK_KEY, memberRoute } from "./PersonalPreview";
import { publicSearchUrl } from "./MemberResearchOutcomes";

/* Normal entry through the one ordinary composer: the member types a plain message and the first clear outcome
 * appears in the same thread. Fetch is stubbed at the wire level (same style as PersonalPreview.test.tsx). */
const aid = "11111111-1111-4111-8111-111111111111", tid = "33333333-3333-4333-8333-333333333333";
const connId = "55555555-5555-4555-8555-555555555555", digest = "d".repeat(64);
const scopeId = "a".repeat(64);
const member = { user: { id: aid, name: "Member A", email: "a@example.test", role: "member", must_change_password: false },
  password_change_required: false, chat_draft_scope: scopeId };

type Call = { path: string; method: string; body: any };
let calls: Call[], threads: any[], outcomes: Record<string, any[]>, linkedTurns: any[], calendarConnected: boolean, askUrl: boolean,
  mailReads: any[], workInterpret: unknown;
const post = (path: string) => calls.filter(c => c.method === "POST" && c.path === path);

const outcomeFor = (body: any, thread: string) => ({
  id: body.request_id, thread_id: thread, kind: body.kind, title: body.kind === "shopping" ? "구매 비교 · 책상" : "지원 준비 · 공고",
  message: body.message, state: askUrl ? "needs_input" : "ranked", revision: 1, criteria: body.criteria, citations: [], facts: [], offers: [],
  missing: [], conflicts: [], timing: null, draft: null, receipts: [], handoff: null, created_at: 1,
  question: askUrl ? { id: "q1", text: "URL?" } : null,
  next: askUrl ? { kind: "ask_url", text: "URL을 알려 주세요", url: null } : { kind: "none", text: "", url: null },
});
const turn = (model: string) => ({ sequence: 1, user_text: "일정", assistant: [], run: {
  id: "66666666-6666-4666-8666-666666666666", request_id: connId, thread_id: tid, state: "completed", model: { provider_id: model, function_tools: false },
  can_retry: false, can_cancel: false } });

beforeEach(() => {
  sessionStorage.clear(); history.replaceState(null, "", "/");
  calls = []; threads = [{ id: tid, title: "A private thread", created_at: 1, updated_at: 1 }]; outcomes = {}; linkedTurns = [];
  calendarConnected = true; askUrl = false; mailReads = []; workInterpret = null;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    const path = url.replace("/api/v1", ""), method = init.method ?? "GET", body = init.body ? JSON.parse(String(init.body)) : null;
    calls.push({ path, method, body });
    const json = (v: unknown, status = 200) => Response.json(v, { status });
    if (path === "/auth/session") return json(member);
    if (path === "/text-preview") return json({ enabled: true, capability: "text_only_preview", tools: false, automatic_retry: false, work_interpret: workInterpret !== null });
    if (path === "/threads" && method === "GET") return json({ threads });
    if (path === "/threads" && method === "POST") { const t = { id: body.request_id, title: body.title, created_at: 1, updated_at: 1 }; threads.push(t); return json(t, 201); }
    if (/^\/threads\/[^/]+\/messages/.test(path)) return json({ thread_id: path.split("/")[2], cursor: 0, turns: linkedTurns, next_before: null });
    if (/^\/threads\/[^/]+\/browser/.test(path)) return json({ code: "not_found" }, 404);
    if (/^\/threads\/[^/]+\/work-interpret$/.test(path)) return json(workInterpret);
    const outcomePath = path.match(/^\/personal\/tasks\/research-outcomes\/([^/]+)$/);
    if (outcomePath && method === "GET") return json({ thread_id: outcomePath[1], outcomes: outcomes[outcomePath[1]] ?? [] });
    if (outcomePath && method === "POST") {
      const made = outcomeFor(body, outcomePath[1]); outcomes[outcomePath[1]] = [...(outcomes[outcomePath[1]] ?? []), made]; return json(made, 201);
    }
    if (path === "/personal/google") return json({ connection: { connection_id: connId, epoch: 1, mailbox: "a@example.test" } });
    if (path === "/personal/google/workspace") return json({ connections: [] });
    if (path === "/personal/google/history") return json({ reads: mailReads.filter(r => r.thread_id === body.thread_id) });
    if (path === "/personal/google/inbox") {
      const saved = { ...body.read, state: "completed", created_at: 1, source: { connection_id: connId, epoch: 1, app_revision: aid, mailbox: "a@example.test" },
        result: { mailbox: "a@example.test", messages: [], has_more: false, scope: "one_inbox_page_metadata_only" } };
      mailReads = [saved]; return json(saved);
    }
    if (path === "/personal/google/mail-work/list") return json({ items: [] });
    if (path === "/personal/google/mail-write/status") return json({ state: "needs_connection", reason: "write_consent_required", next: "start_write_consent" });
    if (path === "/personal/google/mail-write/complete") return json({ state: "connected", connection_id: connId, epoch: 2, mailbox: "a@example.test" });
    if (path === "/personal/google/calendar/status")
      return json({ connection: calendarConnected ? { connection_id: connId, epoch: 3, mailbox: "a@example.test" } : null });
    if (path === "/personal/google/calendar/complete") { calendarConnected = true; return json({ connection_id: connId, epoch: 3, mailbox: "a@example.test", flow_id: "f".repeat(64) }); }
    if (path === "/personal/google/calendar/prepare")
      return json({ state: "preview", preview: { kind: "create", digest, title: "팀 회의", start: "2026-11-26T15:00:00+09:00", end: "2026-11-26T16:00:00+09:00", time_zone: "Asia/Seoul", current: null } });
    if (path === "/personal/google/calendar/apply")
      return json({ state: "confirmed", confirmed_by: "provider_response", receipt: "created",
        event: { id: "e1", title: "팀 회의", start: "2026-11-26T15:00:00+09:00", end: "2026-11-26T16:00:00+09:00", time_zone: "Asia/Seoul" } });
    throw new Error(`Unexpected route ${method} ${path}`);
  }));
});
afterEach(() => { cleanup(); sessionStorage.clear(); history.replaceState(null, "", "/"); vi.unstubAllGlobals(); });

const mount = () => render(<StrictMode><PersonalPreview /></StrictMode>);
async function say(text: string) {
  const user = userEvent.setup();
  await user.type(await screen.findByRole("textbox", { name: "메시지" }), text);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  return user;
}

it("shopping: one plain message creates the comparison in this thread and it reopens on another device without tab storage", async () => {
  mount();
  await say("12월 10일까지 도착하는 판매처 가격 비교해 줘");
  expect(await screen.findByRole("listitem", { name: "구매 비교 · 책상" })).toBeVisible();
  const [created] = post(`/personal/tasks/research-outcomes/${tid}`);
  expect(created.body).toMatchObject({ kind: "shopping", criteria: { deadline: expect.stringMatching(/-12-10$/) } });
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0); // no extra model chat for the plain request
  cleanup(); sessionStorage.clear(); calls = []; // another device: nothing in this tab's storage
  mount();
  expect(await screen.findByRole("listitem", { name: "구매 비교 · 책상" })).toBeVisible();
  expect(calls.some(c => c.method === "GET" && c.path === `/personal/tasks/research-outcomes/${tid}`)).toBe(true);
  expect(calls.filter(c => c.method === "POST" && c.path !== "/personal/google/history")).toHaveLength(0);
});

it("application: a plain grant request makes an application outcome; switching threads clears the previous projection", async () => {
  const other = "77777777-7777-4777-8777-777777777777";
  threads.push({ id: other, title: "Other", created_at: 0, updated_at: 0 });
  mount();
  await say("공모 공고와 내 메일을 비교해서 지원서 초안 써줘");
  expect(await screen.findByRole("listitem", { name: "지원 준비 · 공고" })).toBeVisible();
  expect(post(`/personal/tasks/research-outcomes/${tid}`)[0].body.kind).toBe("application");
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "대화 목록" }));
  await user.click(await screen.findByRole("button", { name: "Other" }));
  await waitFor(() => expect(screen.queryByRole("listitem", { name: "지원 준비 · 공고" })).toBeNull());
});

it("a missing URL offers only a fixed public search; nothing from the member's message reaches it", async () => {
  askUrl = true;
  mount();
  const user = await say("12월 10일까지 도착하는 판매처 가격 비교해 줘 책상");
  await user.click(await screen.findByRole("button", { name: "공개 검색 열기" }));
  await waitFor(() => expect(post(`/threads/${tid}/browser`)).toHaveLength(1));
  const start = post(`/threads/${tid}/browser`)[0].body;
  expect(start.url).toBe(publicSearchUrl("shopping"));
  expect(JSON.stringify(start)).not.toMatch(/책상|12월|가격 비교/);
});

it("calendar: the request is linked to its thread for prepare and apply, needs explicit approval, and is never re-applied on reload", async () => {
  mount();
  const user = await say("내일 오후 3시부터 4시까지 '팀 회의' 잡아줘");
  expect(await screen.findByText(/아직 Google 캘린더에 저장되지 않았습니다/)).toBeVisible();
  const [prepare] = post("/personal/google/calendar/prepare");
  expect(prepare.body).toMatchObject({ thread_id: tid, connection_id: connId, epoch: 3, kind: "create", title: "팀 회의" });
  expect(post("/personal/google/calendar/apply")).toHaveLength(0); // a preview alone never writes
  await user.click(screen.getByRole("button", { name: "Google 캘린더에 저장" }));
  expect(await screen.findByText("Google 캘린더에서 확인됨")).toBeVisible();
  const [apply] = post("/personal/google/calendar/apply");
  expect(apply.body).toMatchObject({ approved_digest: digest, request: { thread_id: tid, request_id: prepare.body.request_id } });
  expect(calls.some(c => c.path === "/personal/google/workspace")).toBe(false); // the real status route is used
  cleanup(); linkedTurns = [turn("member-linked-work")]; // what the server's linked history now holds
  mount();
  await user.click(await screen.findByRole("button", { name: "업무 기록 보기" }));
  await waitFor(() => expect(calls.some(c => c.method === "GET" && c.path === "/personal/tasks/" + linkedTurns[0].run.id)).toBe(true));
  expect(post("/personal/google/calendar/prepare")).toHaveLength(1);
  expect(post("/personal/google/calendar/apply")).toHaveLength(1);
});

it("calendar consent callback: only the flow this tab started is completed, on the original thread, then prepare resumes", async () => {
  const other = "88888888-8888-4888-8888-888888888888", req = "99999999-9999-4999-8999-999999999999";
  threads = [{ id: other, title: "Newest", created_at: 2, updated_at: 2 }, { id: tid, title: "Original", created_at: 1, updated_at: 1 }];
  calendarConnected = false;
  sessionStorage.setItem(MEMBER_WORK_KEY, JSON.stringify({ version: 1, account: aid, scope: scopeId, saved_at: Date.now(), threads: {
    [tid]: { calendar: { request_id: req, text: "내일 오후 3시부터 4시까지 '팀 회의' 잡아줘", at: Date.now(), zone: "Asia/Seoul" } } } }));
  sessionStorage.setItem("moosuri.personal.google-calendar.v1", JSON.stringify({ account: aid, session: scopeId, flow: "f".repeat(64),
    expires: Date.now() + 60000, phase: "consent", thread: tid }));
  history.replaceState(null, "", `/#google_handle=${"c".repeat(64)}&flow=${"f".repeat(64)}&google_service=calendar`);
  mount();
  expect(await screen.findByText(/아직 Google 캘린더에 저장되지 않았습니다/)).toBeVisible();
  expect(post("/personal/google/calendar/complete").map(c => c.body)).toEqual([{ handle: "c".repeat(64) }]);
  expect(post("/personal/google/calendar/prepare")[0].body).toMatchObject({ thread_id: tid, request_id: req });
  expect(location.hash).toBe(""); // the one-use handle left the address bar
});

it("mail: a plain reply request reads the inbox for this thread and prepares nothing in Gmail", async () => {
  mount();
  await say('김 대리 메일에 "확인했습니다. 감사합니다." 라고 답장 써줘');
  await waitFor(() => expect(post("/personal/google/inbox")).toHaveLength(1));
  expect(post("/personal/google/inbox")[0].body.read.thread_id).toBe(tid);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
  expect(calls.some(c => /mail-write\/(start|complete)|mail-work\/(save-draft|approve)/.test(c.path))).toBe(false);
});

it("mail-write consent callback completes once on the original thread without touching the connector catalog", async () => {
  threads = [{ id: "88888888-8888-4888-8888-888888888888", title: "Newest", created_at: 2, updated_at: 2 }, { id: tid, title: "Original", created_at: 1, updated_at: 1 }];
  sessionStorage.setItem("moosuri.member-mail-write.v1", JSON.stringify({ account: aid, session: scopeId, thread: tid, expires: Date.now() + 60000 }));
  history.replaceState(null, "", `/#google_handle=${"e".repeat(64)}&flow=${"f".repeat(64)}&google_service=mail-write`);
  mount();
  expect(await screen.findByText(/쓰기 권한이 허용되었습니다: a@example.test/)).toBeVisible();
  expect(post("/personal/google/mail-write/complete").map(c => c.body)).toEqual([{ handle: "e".repeat(64) }]);
  expect(location.hash).toBe("");
  expect(calls.some(c => c.path === "/personal/google/start" || c.path.startsWith("/personal/vault"))).toBe(false);
});

it("preference: a plain statement is interpreted once and shown as a receipt, never written to tab storage", async () => {
  workInterpret = { kind: "preference", thread_id: tid, view: { kind: "remembered", operation: "create", memory_id: "m1", revision: 1,
    current_revision: 1, subject: "답변 길이", text: "답변은 짧게", superseded: false } };
  mount();
  await say("앞으로 답변은 짧게 해줘");
  expect(await screen.findByText(/답변은 짧게/)).toBeVisible();
  expect(post(`/threads/${tid}/work-interpret`)).toHaveLength(1);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
  expect(JSON.stringify(Object.entries(sessionStorage))).not.toMatch(/답변은 짧게/);
});

it("starts a requested public source lookup directly and leaves mail and the full request out of the browser command", async () => {
  askUrl = true;
  mount();
  await say('내 메일과 “봄 연구 공고”를 검색해서 지원서 초안을 써 줘');
  await waitFor(() => expect(post(`/threads/${tid}/browser`)).toHaveLength(1));
  const command = post(`/threads/${tid}/browser`)[0].body;
  expect(new URL(command.url).searchParams.get("q")).toBe("봄 연구 공고");
  expect(JSON.stringify(command)).not.toMatch(/메일|지원서 초안/);
});


it("compares public prices with a separate no-mail instruction and respects a prohibition on comparison", () => {
  expect(memberRoute("‘공개 노트’를 검색해줘. 가격과 배송비를 비교해 줘. 내 메일은 쓰지 마.")).toMatchObject({ kind: "research", outcome: "shopping" });
  expect(memberRoute("가격과 배송비를 비교하지 마.")).toBeNull();
  expect(memberRoute("‘가격과 배송비를 비교해 줘’라는 말은 무슨 뜻이야?")).toBeNull();
});
