import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import PersonalPreview from "./PersonalPreview";

/* Journeys for the one typed Gmail inbox-read intent of POST /threads/{thread}/work-interpret. Fetch is stubbed at
 * the wire. The interpretation reply is only an instruction: the read itself is the member's own authenticated
 * /personal/google/inbox call, under the request id the interpretation already holds. Everything is synthetic. */
const aid = "11111111-1111-4111-8111-111111111111", tid = "33333333-3333-4333-8333-333333333333";
const oid = "77777777-7777-4777-8777-777777777777", connId = "55555555-5555-4555-8555-555555555555";
const scopeId = "a".repeat(64);
const member = { user: { id: aid, name: "Member A", email: "a@example.test", role: "member", must_change_password: false },
  password_change_required: false, chat_draft_scope: scopeId };
const conn = { connection_id: connId, epoch: 1, mailbox: "a@example.test" };
const EMPTY = "확인한 페이지에 받은 메일이 없습니다.";
const ASK = "최근 받은 메일 5개 확인해줘";

type Call = { path: string; method: string; body: any };
type Script = (body: any) => Response | Promise<Response>;
let calls: Call[], threads: any[], connection: typeof conn | null, mailReads: any[], script: Script[],
  inboxFault: null | "lost" | "mismatch" | "unauthorized" | "unknown", historyDown: boolean, who: typeof member;
const post = (path: string) => calls.filter(c => c.method === "POST" && c.path === path);
const interpretPosts = () => post(`/threads/${tid}/work-interpret`);
const inboxReads = () => post("/personal/google/inbox").map(c => c.body.read);
const inboxReply = (extra: object = {}) => ({ kind: "inbox_read", thread_id: tid, scope: "one_inbox_page_metadata_only", limit: 5, ...extra });
const reply = (value: unknown): Script => () => Response.json(value);
const chatTurn = (body: any, thread: string) => ({ sequence: 1, user_text: body.text, assistant: [], run: {
  id: "66666666-6666-4666-8666-666666666666", request_id: body.request_id, thread_id: thread, state: "completed",
  model: { provider_id: "deployment-text", provider_name: "Deployment text-only preview", function_tools: false },
  can_retry: false, can_cancel: false } });

beforeEach(() => {
  sessionStorage.clear(); history.replaceState(null, "", "/");
  calls = []; threads = [{ id: tid, title: "A private thread", created_at: 2, updated_at: 2 }]; connection = conn;
  mailReads = []; script = []; inboxFault = null; historyDown = false; who = member;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    const path = url.replace("/api/v1", ""), method = init.method ?? "GET", body = init.body ? JSON.parse(String(init.body)) : null;
    calls.push({ path, method, body });
    const json = (v: unknown, status = 200) => Response.json(v, { status });
    if (path === "/auth/session") return json(who);
    if (path === "/text-preview") return json({ enabled: true, capability: "text_only_preview", tools: false, automatic_retry: false, work_interpret: true });
    if (path === "/threads" && method === "GET") return json({ threads });
    if (path === "/threads" && method === "POST") {
      const created={id:body.request_id,title:body.title,created_at:2,updated_at:2};
      threads.push(created);return json(created,201);
    }
    if (/^\/threads\/[^/]+\/messages/.test(path)) return json({ thread_id: path.split("/")[2], cursor: 0, turns: [], next_before: null });
    if (/^\/threads\/[^/]+\/browser/.test(path)) return json({ code: "not_found" }, 404);
    if (/^\/threads\/[^/]+\/work-interpret$/.test(path)) {
      const next = script.shift();
      if (!next) throw new Error("unscripted work-interpret");
      return next(body);
    }
    if (/^\/threads\/[^/]+\/text-preview$/.test(path)) return json(chatTurn(body, path.split("/")[2]), 201);
    if (path === "/personal/google") return json({ connection });
    if (path === "/personal/google/workspace") return json({ connections: [] });
    if (path === "/personal/vault") return json({ state: "ready", site_origin: "https://vault.example.test" });
    if (path === "/personal/google/history")
      return historyDown ? json({ code: "unavailable" }, 503) : json({ reads: mailReads.filter(r => r.thread_id === body.thread_id) });
    if (path === "/personal/google/inbox") {
      const saved = { ...body.read, state: "completed", created_at: 1, source: { ...conn, app_revision: aid },
        result: { mailbox: "a@example.test", messages: [], has_more: false, scope: "one_inbox_page_metadata_only" } };
      if (inboxFault === "unauthorized") return json({ code: "unauthorized" }, 401);
      if (inboxFault === "unknown") return json({ ...saved, state: "unknown", result: undefined });
      if (inboxFault === "mismatch") return json({ ...saved, request_id: "99999999-9999-4999-8999-999999999999" });
      mailReads = [saved]; // the server's receipt, replayed by request id
      if (inboxFault === "lost") { inboxFault = null; historyDown = true; throw new TypeError("synthetic lost response"); }
      return json(saved);
    }
    return json({ code: "not_found" }, 404);
  }));
});
afterEach(() => { cleanup(); sessionStorage.clear(); history.replaceState(null, "", "/"); vi.unstubAllGlobals(); });

const mount = () => render(<StrictMode><PersonalPreview /></StrictMode>);
const composer = () => screen.findByRole("textbox", { name: "메시지" });
const alerts = () => screen.queryAllByRole("alert").map(a => a.textContent ?? "").join(" ");
async function say(text: string, user = userEvent.setup()) {
  await user.type(await composer(), text);
  await user.click(screen.getByRole("button", { name: "보내기" }));
  return user;
}

it("connected: one interpretation then the member's own inbox read under the same request id, with no chat answer", async () => {
  script.push(reply(inboxReply()));
  mount();
  await say(ASK);
  expect(await screen.findByText(EMPTY)).toBeVisible();
  const [asked] = interpretPosts();
  expect(asked.body).toEqual({ request_id: expect.any(String), text: ASK });
  // The target is the current authenticated selection from /personal/google, nothing from the reply.
  expect(post("/personal/google/inbox").map(c => c.body)).toEqual([
    { connection_id: connId, epoch: 1, read: { thread_id: tid, request_id: asked.body.request_id } }]);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
  expect(await composer()).toHaveValue("");
  expect(screen.queryByRole("button", { name: "메일 확인 계속하기" })).toBeNull();
});

it("no connection: nothing is read, the Vault connect continuation keeps the request id through reload, then reads once", async () => {
  connection = null;
  script.push(reply(inboxReply()));
  const first = mount();
  const user = await say("받은 메일 확인해줘");
  await screen.findByRole("button", { name: "Google 연결하고 메일 확인" });
  const id = interpretPosts()[0].body.request_id;
  expect(post("/personal/google/inbox")).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  await screen.findByRole("button", { name: "메일 확인 계속하기" });
  first.unmount();
  connection = conn; // connected meanwhile; the carrier survives the reload without any new interpretation
  mount();
  const resume = await screen.findByRole("button", { name: "메일 확인 계속하기" });
  expect(post("/personal/google/inbox")).toHaveLength(0);
  await user.click(resume);
  expect(await screen.findByText(EMPTY)).toBeVisible();
  expect(inboxReads()).toEqual([{ thread_id: tid, request_id: id }]);
  expect(interpretPosts()).toHaveLength(1);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
});

it("an uncertain interpretation response is re-checked with the same request id and reads the inbox exactly once", async () => {
  script.push(() => { throw new TypeError("synthetic lost response"); }, reply(inboxReply()));
  mount();
  const user = await say(ASK);
  expect(post("/personal/google/inbox")).toHaveLength(0);
  await user.click(await screen.findByRole("button", { name: "동일 요청 다시 확인" }));
  expect(await screen.findByText(EMPTY)).toBeVisible();
  const ids = interpretPosts().map(c => c.body.request_id);
  expect(ids).toHaveLength(2);
  expect(ids[0]).toBe(ids[1]);
  expect(inboxReads()).toEqual([{ thread_id: tid, request_id: ids[0] }]);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
});

it("a lost inbox response keeps the carrier; after reload the saved receipt is reused and the provider is not asked again", async () => {
  inboxFault = "lost";
  script.push(reply(inboxReply()));
  const first = mount();
  await say(ASK);
  await screen.findByRole("button", { name: "메일 확인 계속하기" });
  expect(alerts()).toContain("메일 읽기 결과를 확인하지 못했습니다");
  const id = interpretPosts()[0].body.request_id;
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
  first.unmount();
  historyDown = false;
  mount();
  expect(await screen.findByText(EMPTY)).toBeVisible();
  await waitFor(() => expect(screen.queryByRole("button", { name: "메일 확인 계속하기" })).toBeNull());
  expect(inboxReads()).toEqual([{ thread_id: tid, request_id: id }]);
  expect(interpretPosts()).toHaveLength(1);
});

it("switching threads while interpreting still reads for the original thread and leaves the other thread untouched", async () => {
  threads = [{ id: oid, title: "Other", created_at: 3, updated_at: 3 }, ...threads];
  let release!: () => void;
  script.push(() => new Promise<Response>(resolve => { release = () => resolve(Response.json(inboxReply())); }));
  mount();
  // The newest thread is selected first; move the request to the older one by choosing it before sending.
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "대화 목록" }));
  await user.click(await screen.findByRole("button", { name: "A private thread" }));
  await say(ASK, user);
  await waitFor(() => expect(interpretPosts()).toHaveLength(1));
  await user.click(screen.getByRole("button", { name: "대화 목록" }));
  await user.click(await screen.findByRole("button", { name: "Other" }));
  release();
  await waitFor(() => expect(inboxReads()).toHaveLength(1));
  expect(inboxReads()[0]).toEqual({ thread_id: tid, request_id: interpretPosts()[0].body.request_id });
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText(EMPTY)).toBeNull();
  expect(await composer()).toHaveValue("");
  await user.click(screen.getByRole("button", { name: "대화 목록" }));
  await user.click(await screen.findByRole("button", { name: "A private thread" }));
  expect(await screen.findByText(EMPTY)).toBeVisible();
});

it("a forged or widened reply, a mismatched read response and a revoked session never become a successful mail read", async () => {
  mount();
  const user = userEvent.setup();
  const notice = "요청을 업무로 해석한 결과를 확인할 수 없습니다. 입력을 유지했습니다.";
  const forged = [inboxReply({ thread_id: oid }), inboxReply({ limit: 50 }), inboxReply({ scope: "all_mail" }),
    inboxReply({ connection_id: connId }), inboxReply({ account: "other@example.test" })];
  await user.type(await composer(), ASK);
  for (const [i, bad] of forged.entries()) {
    script.push(reply(bad));
    await user.click(screen.getByRole("button", { name: "보내기" }));
    await waitFor(() => expect(interpretPosts()).toHaveLength(i + 1));
    expect(await screen.findByText(notice)).toBeVisible();
    await waitFor(() => expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue(ASK));
  }
  expect(post("/personal/google/inbox")).toHaveLength(0);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
  // A read that answers for another request is not a success and keeps the same-id continuation.
  inboxFault = "mismatch";
  script.push(reply(inboxReply()));
  await user.click(screen.getByRole("button", { name: "보내기" }));
  expect(await screen.findByRole("button", { name: "메일 확인 계속하기" })).toBeVisible();
  expect(alerts()).toContain("메일을 읽지 못했습니다");
  expect(screen.queryByText(EMPTY)).toBeNull();
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
  // A revoked session ends the flow at the login screen without another read or any chat fallback.
  await user.click(screen.getByRole("button", { name: "이전 메시지 버리기" }));
  inboxFault = "unauthorized";
  script.push(reply(inboxReply()));
  await say(ASK, user);
  expect(await screen.findByRole("heading", { name: "내 무수리에 로그인" })).toBeVisible();
  expect(post("/personal/google/inbox")).toHaveLength(2);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
});

it("a negated request is clarified and a hypothetical one is ordinary chat; neither reads mail", async () => {
  script.push(reply({ kind: "clarify", message: "하지 말라는 뜻으로 이해했어요. 아무 작업도 바꾸지 않았어요. 원하는 작업을 하나의 문장으로 다시 알려 주세요." }));
  mount();
  const user = await say("받은 메일 5개 확인하지 마");
  expect(await screen.findByText(/하지 말라는 뜻으로 이해했어요/)).toBeVisible();
  expect(await composer()).toHaveValue("받은 메일 5개 확인하지 마");
  script.push(reply({ kind: "chat" }));
  await user.clear(await composer());
  await say("받은 메일은 어떻게 확인하는 거야", user);
  await waitFor(() => expect(post(`/threads/${tid}/text-preview`)).toHaveLength(1));
  expect(post("/personal/google/inbox")).toHaveLength(0);
});

it("an unknown provider outcome is not repeated automatically; only the member's explicit action reads again under a new id with the same text", async () => {
  inboxFault = "unknown";
  script.push(reply(inboxReply()));
  mount();
  const user = await say(ASK);
  const again = await screen.findByRole("button", { name: "새로 메일 읽기" });
  expect(alerts()).toContain("메일을 읽었는지 확인하지 못했습니다");
  const firstId = interpretPosts()[0].body.request_id;
  expect(inboxReads()).toEqual([{ thread_id: tid, request_id: firstId }]);
  expect(screen.queryByText(EMPTY)).toBeNull();
  inboxFault = null;
  await user.click(again);
  expect(await screen.findByText(EMPTY)).toBeVisible();
  const reads = inboxReads();
  expect(reads).toHaveLength(2);
  expect(reads[1].thread_id).toBe(tid);
  expect(reads[1].request_id).not.toBe(firstId);
  expect(interpretPosts()).toHaveLength(1);
  expect(screen.queryByRole("button", { name: "새로 메일 읽기" })).toBeNull();
});

it("a missing connection after a thread switch neither navigates away nor touches the selected thread", async () => {
  connection = null;
  threads = [{ id: oid, title: "Other", created_at: 3, updated_at: 3 }, ...threads];
  let release!: () => void;
  script.push(() => new Promise<Response>(resolve => { release = () => resolve(Response.json(inboxReply())); }));
  mount();
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "대화 목록" }));
  await user.click(await screen.findByRole("button", { name: "A private thread" }));
  await say(ASK, user);
  await waitFor(() => expect(interpretPosts()).toHaveLength(1));
  await user.click(screen.getByRole("button", { name: "대화 목록" }));
  await user.click(await screen.findByRole("button", { name: "Other" }));
  release();
  await waitFor(() => expect(calls.some(c => c.path === "/personal/google" && c.method === "GET")).toBe(true));
  expect(screen.queryByRole("button", { name: "Google 연결하고 메일 확인" })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(post("/personal/google/inbox")).toHaveLength(0);
  expect(await composer()).toBeVisible();
});

it("another member on the same browser never inherits a held inbox request", async () => {
  connection = null;
  script.push(reply(inboxReply()));
  const first = mount();
  await say(ASK);
  await screen.findByRole("button", { name: "Google 연결하고 메일 확인" });
  first.unmount();
  who = { ...member, user: { ...member.user, id: "88888888-8888-4888-8888-888888888888", email: "b@example.test", name: "Member B" },
    chat_draft_scope: "b".repeat(64) };
  connection = conn;
  // Account B starts at the same ordinary chat view. Navigation is browser-wide,
  // while the held inbox carrier must remain scoped to A's authenticated session.
  history.replaceState(null, "", "/");
  mount();
  await composer();
  expect(screen.queryByRole("button", { name: "메일 확인 계속하기" })).toBeNull();
  expect(post("/personal/google/inbox")).toHaveLength(0);
  expect(interpretPosts()).toHaveLength(1);
});

it("an unsupported read scope reports the supported scope and keeps the input without reading or chatting", async () => {
  const scope = "지금은 연결된 Gmail 받은편지함의 첫 페이지에서 최대 5건의 보낸 사람·제목·날짜만 확인할 수 있어요. 검색, 전체 메일, 본문 요약, 다른 계정은 아직 지원하지 않아요.";
  script.push(reply({ kind: "clarify", message: scope }));
  mount();
  await say("받은 메일 전체에서 청구서 찾아서 요약해줘");
  expect(await screen.findByText(/최대 5건/)).toBeVisible();
  expect(await composer()).toHaveValue("받은 메일 전체에서 청구서 찾아서 요약해줘");
  expect(screen.queryByText(EMPTY)).toBeNull();
  expect(post("/personal/google/inbox")).toHaveLength(0);
  expect(post(`/threads/${tid}/text-preview`)).toHaveLength(0);
});

it.each([null,"A newer draft",ASK])("first conversation preserves the live request and newer draft: %s", async newer => {
  threads=[];connection=newer===null?conn:null;
  script.push(()=>Response.json(inboxReply({thread_id:threads[0].id})));
  const first=mount(),user=await say(ASK);
  if(newer===null)await screen.findByText(EMPTY);
  else await screen.findByRole("button",{name:"Google 연결하고 메일 확인"});
  expect(post("/threads")).toHaveLength(1);
  const thread=threads[0].id;
  const interpreted=post(`/threads/${thread}/work-interpret`);
  expect(interpreted).toHaveLength(1);
  const expected={thread_id:thread,request_id:interpreted[0].body.request_id};
  if(newer!==null) {
    expect(inboxReads()).toEqual([]);
    await user.click(screen.getByRole("button",{name:"뒤로"}));
    const draft=await composer();await user.clear(draft);await user.type(draft,newer);
    first.unmount();connection=conn;mount();
    const resume=await screen.findByRole("button",{name:"메일 확인 계속하기"});
    expect(inboxReads()).toEqual([]); // A reload never authorizes a new read.
    expect(await composer()).toHaveValue(newer);
    await user.click(resume);await screen.findByText(EMPTY);
    expect(await composer()).toHaveValue(newer);
  } else expect(await composer()).toHaveValue("");
  expect(inboxReads()).toEqual([expected]);
  expect(post(`/threads/${thread}/work-interpret`)).toHaveLength(1);
});
