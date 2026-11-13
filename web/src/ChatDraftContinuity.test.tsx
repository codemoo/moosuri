import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, it, expect, vi } from "vitest";
import App from "./App";
import { CHAT_DRAFT_KEY, ChatDraftRecovery } from "./chat-drafts";
import type { SessionView, ChatTurn } from "../shared/api-types";
const alice: SessionView = { user: { id: "alice", name: "Alice", email: "alice@example.test", role: "admin", must_change_password: false }, password_change_required: false, chat_draft_scope: "a".repeat(64) };
const bob: SessionView = { user: { id: "bob", name: "Bob", email: "bob@example.test", role: "member", must_change_password: false }, password_change_required: false, chat_draft_scope: "b".repeat(64) };
const t1 = { id: "thread-one", title: "First conversation", created_at: 1, updated_at: 1 }, t2 = { id: "thread-two", title: "Selected conversation", created_at: 2, updated_at: 2 };
const requestId = "00000000-0000-4000-8000-000000000001", attachment = "00000000-0000-4000-8000-000000000002", target = "00000000-0000-4000-8000-000000000003";
const choice = { provider_id: "p", provider_revision: 1, provider_name: "fixture", kind: "compatible" as const, model: "fixture", effort: null, function_tools: true, verified_at: 1 };
const json = (v: unknown, status = 200) => new Response(status === 204 ? null : JSON.stringify(v), { status, headers: { "content-type": "application/json" } });
function deferred<T>() { let resolve!: (v: T) => void, reject!: (e: unknown) => void; const promise = new Promise<T>((r, j) => { resolve = r; reject = j; }); return { resolve, reject, promise }; }
function setup() {
    let onSystem: (() => Promise<Response>) | null = null;
    let session: SessionView | null = alice, threads = [t1, t2], turns: ChatTurn[] = [], onThreads: (() => Promise<Response>) | null = null, onSession: (() => Promise<Response>) | null = null, onPost: ((body: Record<string, unknown>) => Promise<Response>) | null = null, logout: Promise<Response> | null = null;
    const calls: Array<{
        path: string;
        init: RequestInit;
    }> = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
        const path = url.replace("/api/v1", "");
        calls.push({ path, init });
        if (path === "/system")
            return onSystem ? onSystem() : json({ initialized: true, version: "synthetic", stage: "preview" });
        if (path === "/auth/session")
            return onSession ? onSession() : session ? json(session) : json({}, 401);
        if (path === "/auth/login")
            return json(session);
        if (path === "/auth/logout")
            return logout ?? json({}, 204);
        if (path === "/threads" && init.method !== "POST")
            return onThreads ? onThreads() : json({ threads });
        if (path === "/settings/model")
            return json({ selection: null, effective: choice, choices: [choice] });
        if (path.endsWith("/messages")) {
            if (init.method === "POST")
                return onPost ? onPost(JSON.parse(String(init.body))) : json({}, 503);
            return json({ thread_id: path.split("/")[2], cursor: 0, turns, next_before: null });
        }
        if (path.includes("/events"))
            return new Response(new ReadableStream({ start(c) { init.signal?.addEventListener("abort", () => { try {
                    c.close();
                }
                catch { /* already closed */ } }, { once: true }); } }), { headers: { "content-type": "text/event-stream" } });
        if (path.startsWith("/threads/") && init.method === "DELETE") {
            threads = threads.filter(t => t.id !== path.split("/")[2]);
            return json({}, 204);
        }
        return json({}, 204);
    }));
    return { calls, onSystem: (handler: (() => Promise<Response>) | null) => { onSystem = handler; }, onSession: (handler: (() => Promise<Response>) | null) => { onSession = handler; }, setSession: (s: SessionView | null) => { session = s; }, setThreads: (v: typeof threads) => { threads = v; }, setTurns: (v: ChatTurn[]) => { turns = v; }, holdThreads: (p: Promise<Response>) => { onThreads = () => p; }, holdSession: (p: Promise<Response>) => { onSession = () => p; }, holdPost: (p: Promise<Response>) => { onPost = () => p; }, failLogout: () => { logout = Promise.reject(new TypeError("lost logout")); logout.catch(() => { }); } };
}
function seed() { const lease = new ChatDraftRecovery().confirm(alice)!; lease.save({ drafts: { [t1.id]: "first draft", [t2.id]: "selected next draft" }, pending: {} }, t2.id, [t1.id, t2.id]); }
const savedPending = { request_id: requestId, text: "saved uncertain request", attachments: [{ id: attachment, revision: 4 }], artifact_target: { id: target, revision: 7 }, schedule_context: { timezone: "America/New_York", target: null }, status: "sending" as const };
function seedPending() { new ChatDraftRecovery().confirm(alice)!.save({ drafts: { [t2.id]: "saved next typing" }, pending: { [t2.id]: savedPending } }, t2.id, [t1.id, t2.id]); }
async function expectPendingRecovery(srv: ReturnType<typeof setup>) {
    expect(await composer()).toHaveValue("saved next typing");
    expect(screen.getByRole("heading", { name: t2.title })).toBeInTheDocument();
    await screen.findByRole("button", { name: "같은 요청 다시 제출" });
    expect(screen.getByText(savedPending.text)).toBeInTheDocument();
    const slot = JSON.parse(window.sessionStorage.getItem(CHAT_DRAFT_KEY)!).slots.find((s: { id: string }) => s.id === t2.id);
    expect(slot.pending).toEqual({ ...savedPending, status: "uncertain" });
    expect(srv.calls.filter(c => c.init.method === "POST")).toHaveLength(0);
}
async function composer() { return screen.findByRole("textbox", { name: "메시지" }); }
const mount = () => render(<StrictMode><App /></StrictMode>);
beforeEach(() => { window.sessionStorage.clear(); window.localStorage.clear(); vi.stubGlobal("BroadcastChannel", undefined); });
describe("session-bound refresh recovery through real App/ChatPanel", () => {
    it.each(["online", "manual"] as const)("%s retry after offline boot keeps next typing and original pending carrier until exact-session/inventory confirmation", async (retry) => {
        seedPending();
        const saved = window.sessionStorage.getItem(CHAT_DRAFT_KEY);
        const srv = setup(), session = deferred<Response>(), inventory = deferred<Response>();
        srv.onSystem(async () => { throw new TypeError("offline boot"); });
        mount();
        await screen.findByRole("button", { name: "Try again" });
        expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBe(saved);
        srv.onSystem(null);
        srv.holdSession(session.promise);
        srv.holdThreads(inventory.promise);
        if (retry === "online") fireEvent(window, new Event("online"));
        else fireEvent.click(screen.getByRole("button", { name: "Try again" }));
        expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBe(saved);
        await waitFor(() => expect(srv.calls.some(c => c.path === "/auth/session")).toBe(true));
        expect(screen.queryByRole("textbox", { name: "메시지" })).toBeNull();
        await act(async () => session.resolve(json(alice)));
        await screen.findByText("Checking your conversations…");
        expect(screen.queryByRole("textbox", { name: "메시지" })).toBeNull();
        expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBe(saved);
        await act(async () => inventory.resolve(json({ threads: [t1, t2] })));
        await expectPendingRecovery(srv);
    });
    const abandonedReplies = [
        { stage: "/system", reply: "uninitialized installation", response: () => json({ initialized: false, version: "stale", stage: "preview" }) },
        { stage: "/auth/session", reply: "different account", response: () => json(bob) },
        { stage: "/auth/session", reply: "unauthorized", response: () => json({}, 401) },
        { stage: "/auth/session", reply: "network rejection", response: null },
    ];
    it.each(abandonedReplies.flatMap(test => ["before", "after"].map(order => ({ ...test, order }))))("online during loading fences abandoned $stage $reply arriving $order fresh authentication", async ({ stage, response, order }) => {
        seedPending();
        const saved = window.sessionStorage.getItem(CHAT_DRAFT_KEY);
        const srv = setup(), abandoned = deferred<Response>(), current = deferred<Response>();
        if (stage === "/system") srv.onSystem(() => abandoned.promise);
        else srv.holdSession(abandoned.promise);
        mount();
        await waitFor(() => expect(srv.calls.some(c => c.path === stage && !c.init.signal?.aborted)).toBe(true));
        const oldRequests = srv.calls.filter(c => c.path === stage);
        srv.onSystem(null);
        srv.holdSession(current.promise);
        fireEvent(window, new Event("online"));
        // Retirement is synchronous even when the old fetch ignores AbortSignal.
        expect(oldRequests.every(c => c.init.signal?.aborted)).toBe(true);
        expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBe(saved);
        await waitFor(() => expect(srv.calls.filter(c => c.path === "/auth/session" && !c.init.signal?.aborted)).toHaveLength(1));
        const finishAbandoned = async () => act(async () => {
            if (response) abandoned.resolve(response());
            else abandoned.reject(new TypeError("late offline boot"));
        });
        if (order === "before") {
            await finishAbandoned();
            expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBe(saved);
            expect(screen.queryByRole("textbox", { name: "메시지" })).toBeNull();
            expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
            expect(srv.calls.filter(c => c.path === "/threads")).toHaveLength(0);
        }
        await act(async () => current.resolve(json(alice)));
        await expectPendingRecovery(srv);
        if (order === "after") await finishAbandoned();
        await expectPendingRecovery(srv);
        expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
        expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();
    });
    it("restores draft and selected thread only after actual session and thread-list confirmation", async () => {
        seed();
        const srv = setup(), session = deferred<Response>();
        srv.holdSession(session.promise);
        const view = mount();
        expect(screen.queryByRole("textbox", { name: "메시지" })).toBeNull();
        await act(async () => session.resolve(json(alice)));
        const box = await composer();
        expect(box).toHaveValue("selected next draft");
        expect(screen.getByRole("heading", { name: t2.title })).toBeInTheDocument();
        fireEvent.change(box, { target: { value: "changed before full reload" } });
        expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toContain("changed before full reload");
        view.unmount();
        const next = setup();
        mount();
        expect(await composer()).toHaveValue("changed before full reload");
        expect(next.calls.filter(c => c.init.method === "POST")).toHaveLength(0);
        expect(window.localStorage.length).toBe(0);
    });
    it("never reveals stored text while the authenticated thread inventory is unavailable", async () => { seed(); const srv = setup(), list = deferred<Response>(); srv.holdThreads(list.promise); mount(); await screen.findByText("Checking your conversations…"); expect(screen.queryByRole("textbox", { name: "메시지" })).toBeNull(); await act(async () => list.resolve(json({ threads: [t1] }))); expect(await composer()).toHaveValue("first draft"); expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY) ?? "").not.toContain("selected next draft"); });
    it.each([bob, { ...alice, chat_draft_scope: "c".repeat(64) }, { ...alice, chat_draft_scope: undefined }])("different account, same-account replacement and old server cannot load previous drafts", async (session) => { seed(); const srv = setup(); srv.setSession(session); mount(); expect(await composer()).toHaveValue(""); expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY) ?? "").not.toContain("selected next draft"); });
    it("passive same-account session replacement synchronously fences held private responses and storage", async () => {
        const srv = setup();
        const held = deferred<Response>();
        srv.holdPost(held.promise);
        mount();
        const box = await composer();
        fireEvent.change(box, { target: { value: "old session request" } });
        await waitFor(() => expect(screen.getByRole("button", { name: "메시지 전송" })).toBeEnabled());
        fireEvent.click(screen.getByRole("button", { name: "메시지 전송" }));
        await waitFor(() => expect(srv.calls.some(c => c.init.method === "POST" && c.path.endsWith("/messages"))).toBe(true));
        fireEvent.change(box, { target: { value: "old session next draft" } });
        srv.setSession({ ...alice, chat_draft_scope: "d".repeat(64) });
        fireEvent(window, new Event("focus"));
        await waitFor(() => expect(screen.getByRole("textbox", { name: "메시지" })).toHaveValue(""));
        await act(async () => held.resolve(json({}, 503)));
        expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY) ?? "").not.toContain("old session");
    });
    it("reload reconciles pending by GET only, preserves next draft and exact original resend carrier", async () => {
        const pending = { request_id: requestId, text: "original request", attachments: [{ id: attachment, revision: 4 }], artifact_target: { id: target, revision: 7 }, schedule_context: { timezone: "America/New_York", target: null }, status: "sending" as const };
        new ChatDraftRecovery().confirm(alice)!.save({ drafts: { [t2.id]: "next unsent draft" }, pending: { [t2.id]: pending } }, t2.id, [t1.id, t2.id]);
        const srv = setup();
        mount();
        expect(await composer()).toHaveValue("next unsent draft");
        const resend = await screen.findByRole("button", { name: "같은 요청 다시 제출" });
        await waitFor(() => expect(resend).toBeEnabled());
        expect(srv.calls.filter(c => c.init.method === "POST")).toHaveLength(0);
        expect(screen.getByText("original request")).toBeInTheDocument();
        fireEvent.click(resend);
        await waitFor(() => expect(srv.calls.some(c => c.init.method === "POST")).toBe(true));
        const posts = srv.calls.filter(c => c.init.method === "POST");
        expect(posts).toHaveLength(1);
        expect(JSON.parse(String(posts[0].init.body))).toEqual({ request_id: requestId, text: "original request", attachments: pending.attachments, artifact_target: pending.artifact_target, schedule_context: pending.schedule_context });
        expect(await composer()).toHaveValue("next unsent draft");
    });
    it("a real pending send survives component reload without a new POST and retains next typing", async () => {
        const original = setup(), held = deferred<Response>();
        original.holdPost(held.promise);
        const first = mount();
        const box = await composer();
        fireEvent.change(box, { target: { value: "Original submitted request" } });
        await waitFor(() => expect(screen.getByRole("button", { name: "메시지 전송" })).toBeEnabled());
        fireEvent.click(screen.getByRole("button", { name: "메시지 전송" }));
        await waitFor(() => expect(original.calls.some(c => c.init.method === "POST" && c.path.endsWith("/messages"))).toBe(true));
        const body = JSON.parse(String(original.calls.find(c => c.init.method === "POST" && c.path.endsWith("/messages"))!.init.body));
        fireEvent.change(box, { target: { value: "Keep this next unsent text" } });
        const before = JSON.parse(window.sessionStorage.getItem(CHAT_DRAFT_KEY)!);
        expect(before.slots.find((s: {
            id: string;
        }) => s.id === t1.id).pending.request_id).toBe(body.request_id);
        first.unmount();
        const after = setup();
        mount();
        expect(await composer()).toHaveValue("Keep this next unsent text");
        await screen.findByRole("button", { name: "같은 요청 다시 제출" });
        expect(after.calls.filter(c => c.init.method === "POST")).toHaveLength(0);
        await act(async () => held.resolve(json({}, 503)));
        expect(await composer()).toHaveValue("Keep this next unsent text");
        expect(JSON.parse(window.sessionStorage.getItem(CHAT_DRAFT_KEY)!).slots.find((s: {
            id: string;
        }) => s.id === t1.id).pending.request_id).toBe(body.request_id);
    });
    it("unavailable storage keeps chat editable and never copies drafts to localStorage", async () => {
        vi.spyOn(window, "sessionStorage", "get").mockImplementation(() => { throw new DOMException("Storage denied", "SecurityError"); });
        setup();
        mount();
        const box = await composer();
        fireEvent.change(box, { target: { value: "In-memory draft still works" } });
        expect(box).toHaveValue("In-memory draft still works");
        expect(window.localStorage.length).toBe(0);
    });
    it("acknowledged failed run clears pending without replacing the next draft or resubmitting", async () => {
        new ChatDraftRecovery().confirm(alice)!.save({ drafts: { [t2.id]: "next unsent" }, pending: { [t2.id]: { request_id: requestId, text: "prior failed request", status: "uncertain" } } }, t2.id, [t1.id, t2.id]);
        const srv = setup();
        srv.setTurns([{ sequence: 1, user_text: "prior failed request", assistant: [], run: { id: "run", request_id: requestId, thread_id: t2.id, attempt: 1, state: "failed", version: 2, model: choice, error: "protocol_failed", usage: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null }, evidence: [], can_retry: true, can_cancel: false, created_at: 1 } }]);
        mount();
        expect(await composer()).toHaveValue("next unsent");
        await waitFor(() => expect(screen.queryByRole("button", { name: "같은 요청 다시 제출" })).toBeNull());
        expect(srv.calls.filter(c => c.init.method === "POST")).toHaveLength(0);
        expect(JSON.parse(window.sessionStorage.getItem(CHAT_DRAFT_KEY)!).slots.find((s: {
            id: string;
        }) => s.id === t2.id).pending).toBeUndefined();
    });
    it("clears storage immediately on failed logout and ignores old held responses", async () => {
        seed();
        const srv = setup();
        srv.failLogout();
        mount();
        expect(await composer()).toHaveValue("selected next draft");
        fireEvent.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
        await screen.findByText(/Sign-out could not be confirmed/);
        expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBeNull();
    });
    it("clears expired/restricted sessions without displaying drafts", async () => { seed(); const srv = setup(); srv.setSession(null); const view = mount(); await screen.findByRole("button", { name: "Sign in" }); expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBeNull(); view.unmount(); seed(); srv.setSession({ ...alice, password_change_required: true, user: { ...alice.user, must_change_password: true } }); mount(); await screen.findByRole("heading", { name: /password/i }); expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toBeNull(); expect(screen.queryByRole("textbox", { name: "메시지" })).toBeNull(); });
    it("deleting a conversation purges its draft without affecting the other slot", async () => { seed(); setup(); mount(); expect(await composer()).toHaveValue("selected next draft"); fireEvent.click(screen.getByRole("button", { name: `Delete ${t2.title}` })); const dialog = await screen.findByRole("dialog"); const buttons = Array.from(dialog.querySelectorAll('button')); const confirm = buttons.find(b => b.textContent?.includes("Delete") && !b.getAttribute('aria-label'))!; fireEvent.click(confirm); await waitFor(() => expect(screen.queryByRole("button", { name: `Delete ${t2.title}` })).toBeNull()); expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY) ?? "").not.toContain("selected next draft"); expect(window.sessionStorage.getItem(CHAT_DRAFT_KEY)).toContain("first draft"); });
});
