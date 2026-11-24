import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { Button } from "@astryxdesign/core/Button";
import type {
  ChatPage,
  ChatTurn,
  RunInstructionReceipt,
  SessionView as BaseSessionView,
  ThreadSummary,
  ThreadsResponse,
} from "../shared/api-types";
import type {
  ChatMemory,
  PendingSend,
  PendingThreadCreation,
} from "./ChatPanel";
import { ApiFailure, errorText, isAborted, RequestScope } from "./api";
import { ScopeRegistry, useScope } from "./account-scope";
import { accountPasswordValid } from "./account-password";
import { ChatDraftRecovery, type ChatDraftSession } from "./chat-drafts";
import "./personal-preview.css";
import {
  clearPersonalGoogle,
  googleReturn,
  googleReadDestination,
  GoogleReadHistory,
  type GoogleReadIntent,
  type ThreadGoogleRead,
} from "./PersonalGoogle";
import { clearWorkspaceGoogle, workspaceGoogleReturn } from "./personal-google-workspace";
import { useMemberRoute } from "./member-route";
import { PersonalConnections } from "./PersonalConnections";
import PersonalAdmin from "./PersonalAdmin";
import { clearAdminRecovery } from "./PersonalAdminRecovery";
import { personalResearchIntent, type WorkReceipt } from "./PersonalWorkflows";
import { PersonalTasks } from "./PersonalTasks";
import { PersonalEngineSettings } from "./PersonalEngineSettings";
import { PersonalMemories } from "./PersonalMemories";
import { initialKnowledgeDrafts } from "./KnowledgeState";
import { PersonalChatComposer } from "./PersonalChatComposer";
import { PersonalWorkspaceChrome, threadDisplayTitle, type MemberView } from "./PersonalWorkspaceChrome";
import { PersonalBrowserCard, PersonalBrowserViewer } from "./PersonalBrowser";
import { browserEndpoint, browserIntent, browserNeedsGuidance, browserStateLabel, type PersonalBrowserStatus } from "./personal-browser";
import { usePersonalBrowser } from "./usePersonalBrowser";
import { InstructionReceipts } from "./RunInstructions";
import { PersonalTextFollowUp, clearSavedFollowUp, isTextFollowUp } from "./PersonalTextFollowUp";
import { sameInstruction } from "./run-instructions";
import {
  compactMessage, parseWorkControl, parseWorkItems, resolveControl, WORK_PROVIDER_ID,
  type WorkControl, type WorkItemInput,
} from "./conversation-work";
import { clearWorkRecovery } from "./conversation-work";
import { useWorkCompletions } from "./useWorkCompletions";
import { GroupForTurn, WorkCompletionMenu, useConversationWork, WorkComposer, WorkStatusNotice } from "./ConversationWork";
import "./conversation-work.css";
type WorkNotice = { thread: string; kind: "status"; groupId: string }
  | { thread: string; kind: "clarify" | "choose"; text: string; draft: string; choices?: { label: string; run: () => void }[] };
type SessionView = BaseSessionView & { installation_admin?: boolean };
export const PREVIEW_DRAFT_KEY = "moosuri.personal-preview-drafts.v1";
function draftStorage() {
  try {
    const s = window.sessionStorage;
    return {
      getItem: () => s.getItem(PREVIEW_DRAFT_KEY),
      setItem: (_key: string, value: string) =>
        s.setItem(PREVIEW_DRAFT_KEY, value),
      removeItem: () => s.removeItem(PREVIEW_DRAFT_KEY),
    };
  } catch {
    return null;
  }
}
function Brand() {
  return (
    <span className="preview-brand">
      <img src="/moosuri-icon-192.png" alt="" width="48" height="48" />
      Moosuri
    </span>
  );
}
export default function PersonalPreview() {
  const scopes = useMemo(() => new Set<RequestScope>(), []),
    recovery = useMemo(() => new ChatDraftRecovery(draftStorage), []);
  const [epoch, setEpoch] = useState(0),
    [session, setSession] = useState<SessionView | null>(null),
    [draftSession, setDraftSession] = useState<ChatDraftSession | null>(null);
  const [phase, setPhase] = useState<"boot" | "login" | "ready" | "logout">(
      "boot",
    ),
    [error, setError] = useState("");
  const logout = useRef<{ account: string; scope: RequestScope } | null>(null),
    channel = useRef<BroadcastChannel | null>(null);
  function fence() {
    for (const s of scopes) s.close();
    recovery.clear();
    clearWorkRecovery();
    clearAdminRecovery();
    clearPersonalGoogle();
    clearWorkspaceGoogle();
    setDraftSession(null);
    setSession(null);
    setEpoch((v) => v + 1);
  }
  function expired() {
    fence();
    setPhase("login");
    setError("로그인이 만료되었습니다. 다시 로그인해 주세요.");
  }
  function signedIn(value: SessionView) {
    for (const s of scopes) s.close();
    setDraftSession(recovery.confirm(value));
    setSession(value);
    setPhase("ready");
    setError("");
    setEpoch((v) => v + 1);
  }
  function announce() {
    try {
      channel.current?.postMessage({ type: "auth-changed", retired: true });
    } catch {
      /* Optional cross-tab fence. */
    }
  }
  useEffect(() => {
    if (typeof BroadcastChannel === "undefined") return;
    const c = new BroadcastChannel("moosuri-auth");
    channel.current = c;
    c.onmessage = (e) => {
      if (e.data?.type === "auth-changed") {
        fence();
        setError("");
        setPhase("boot");
      }
    };
    return () => {
      c.close();
      channel.current = null;
    };
  }, []);
  useEffect(
    () => () => {
      for (const s of scopes) s.close();
      logout.current?.scope.close();
    },
    [scopes],
  );
  async function signOut() {
    const account = session?.user.id ?? logout.current?.account;
    if (!account || (logout.current && phase === "logout")) return;
    logout.current?.scope.close();
    const scope = new RequestScope(account);
    logout.current = { account, scope };
    fence();
    setPhase("logout");
    setError("");
    announce();
    try {
      await scope.request("/auth/logout", "POST", {}, undefined, {
        timeoutMs: 15000,
      });
      if (logout.current?.scope !== scope) return;
      logout.current = null;
      scope.close();
      setPhase("login");
      announce();
    } catch (e) {
      if (!isAborted(e) && logout.current?.scope === scope) {
        setError("로그아웃 결과를 확인하지 못했습니다. 다시 확인해 주세요.");
        setPhase("login");
      }
    }
  }
  const signedInMember = phase === "ready" && session &&
    !session.password_change_required && !session.user.must_change_password;
  return (
    <ScopeRegistry.Provider value={scopes}>
      <div className={`preview-shell ${signedInMember ? "member-signed-in" : "member-auth"}`}>
        {signedInMember ? (
          <Workspace
            key={`${session.user.id}-${epoch}`}
            session={session}
            recovery={draftSession}
            expired={expired}
            changedSession={signedIn}
            onSignOut={() => void signOut()}
          />
        ) : (
          <>
            <main>
              <Brand />
              {error && (
                <p role="alert" className="preview-error">
                  {error}
                </p>
              )}
              {phase === "boot" && (
                <Boot
                  key={`boot-${epoch}`}
                  ready={signedIn}
                  login={() => setPhase("login")}
                  failed={(e) => {
                    setError(e);
                    setPhase("login");
                  }}
                />
              )}
              {phase === "logout" && <p role="status">로그아웃 확인 중…</p>}
              {phase === "login" &&
                (logout.current ? (
                  <section className="preview-gate">
                    <h1>로그아웃 확인</h1>
                    <button onClick={() => void signOut()}>
                      로그아웃 다시 확인
                    </button>
                  </section>
                ) : (
                  <Credentials
                    key={`login-${epoch}`}
                    done={(v) => {
                      signedIn(v);
                      announce();
                    }}
                  />
                ))}
              {phase === "ready" && session && (
                  <Credentials
                    key={`password-${epoch}`}
                    session={session}
                    done={(v) => {
                      signedIn(v);
                      announce();
                    }}
                    expired={expired}
                    onSignOut={() => void signOut()}
                  />
                )}
            </main>
          </>
        )}
      </div>
    </ScopeRegistry.Provider>
  );
}
function Boot({
  ready,
  login,
  failed,
}: {
  ready: (v: SessionView) => void;
  login: () => void;
  failed: (e: string) => void;
}) {
  const scope = useScope();
  useEffect(() => {
    let live = true;
    void scope
      .request<SessionView>("/auth/session")
      .then((v) => {
        if (live) ready(v);
      })
      .catch((e) => {
        if (!live || isAborted(e)) return;
        if (e instanceof ApiFailure && e.status === 401) login();
        else
          failed(
            "연결 상태를 확인하지 못했습니다. 로그인으로 다시 연결할 수 있습니다.",
          );
      });
    return () => {
      live = false;
    };
  }, [scope]);
  return <p role="status">로그인 확인 중…</p>;
}
function Credentials({
  session,
  done,
  expired,
  onSignOut,
}: {
  session?: SessionView;
  done: (v: SessionView) => void;
  expired?: () => void;
  onSignOut?: () => void;
}) {
  const scope = useScope(session?.user.id),
    [email, setEmail] = useState(""),
    [password, setPassword] = useState(""),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const sending = useRef(false);
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (sending.current) return;
    if (session && !accountPasswordValid(password)) {
      setError("비밀번호는 8자 이상, UTF-8 256바이트 이하로 입력해 주세요.");
      return;
    }
    sending.current = true;
    setBusy(true);
    setError("");
    try {
      const v = await scope.request<SessionView>(
        session ? "/auth/password" : "/auth/login",
        "POST",
        session ? { password } : { email, password },
        undefined,
        { timeoutMs: 15000 },
      );
      setPassword("");
      done(v);
    } catch (e) {
      if (isAborted(e)) return;
      if (session && e instanceof ApiFailure && e.status === 401) expired?.();
      else
        setError(
          e instanceof ApiFailure && e.status === 401
            ? "이메일 또는 비밀번호를 확인해 주세요."
            : errorText(e),
        );
      setPassword("");
    } finally {
      sending.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="preview-gate">
      <h1>{session ? "임시 비밀번호 변경" : "내 무수리에 로그인"}</h1>
      {session && <p>개인 대화를 시작하기 전에 새 비밀번호를 정해 주세요.</p>}
      <form onSubmit={(e) => void submit(e)}>
        {!session && (
          <label>
            이메일
            <input
              type="email"
              autoComplete="username"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              disabled={busy}
            />
          </label>
        )}
        <label>
          {session ? "새 비밀번호" : "비밀번호"}
          <input
            type="password"
            autoComplete={session ? "new-password" : "current-password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            disabled={busy}
          />
        </label>
        {error && (
          <p role="alert" className="preview-error">
            {error}
          </p>
        )}
        <button disabled={busy}>
          {busy ? "확인 중…" : session ? "비밀번호 변경" : "로그인"}
        </button>
      </form>
      {!session && <a className="member-demo-link" href="/demo">예시 대화 둘러보기</a>}
      {onSignOut && <Button variant="ghost" label="로그아웃" onClick={onSignOut} />}
    </section>
  );
}
type Capability = {
  enabled: boolean;
  capability: string;
  tools: boolean;
  automatic_retry: boolean;
};
const chatStarters = [
  { label: "기억 검색", prompt: "기억 검색: " },
  { label: "대화·기억 함께 검색", prompt: "대화 검색: ; 기억 검색: " },
  { label: "문장 다듬기", prompt: "다음 문장을 자연스럽게 다듬어 줘.\n" },
  { label: "계획 세우기", prompt: "다음 일을 작은 단계로 나눠서 계획을 세워 줘.\n" },
];
const active = (turn: ChatTurn) =>
  ["queued", "running"].includes(turn.run.state);
function moveNewDraft(memory: ChatMemory, thread: string): ChatMemory {
  const drafts = { ...memory.drafts };
  if (drafts.new && !drafts[thread]) drafts[thread] = drafts.new;
  delete drafts.new;
  return { ...memory, drafts, threadCreation: undefined };
}
function Workspace({
  session,
  recovery,
  expired,
  changedSession,
  onSignOut,
}: {
  session: SessionView;
  recovery: ChatDraftSession | null;
  expired: () => void;
  changedSession: (v: SessionView) => void;
  onSignOut: () => void;
}) {
  const scope = useScope(session.user.id),
    [threads, setThreads] = useState<ThreadSummary[]>([]),
    [selected, setSelected] = useState<string | null>(null),
    [memory, setMemory] = useState<ChatMemory>({ drafts: {}, pending: {} }),
    [page, setPage] = useState<ChatPage | null>(null),
    [cap, setCap] = useState<Capability | null>(),
    [capError, setCapError] = useState(""),
    [loading, setLoading] = useState(true),
    [inventoryError, setInventoryError] = useState(false),
    [error, setError] = useState(""),
    [reading, setReading] = useState(false),
    [mutating, setMutating] = useState(false);
  const [memoryDrafts, setMemoryDrafts] = useState(initialKnowledgeDrafts);
  const [refresh, setRefresh] = useState(0);
  const [admin, setAdmin] = useState(session.installation_admin === true);
  const onAdminUnavailable = useCallback(() => setAdmin(false), []);
  const { route, open, leaveCatalog } = useMemberRoute(session, admin, () => {
    const workspace = workspaceGoogleReturn(session);
    if (workspace) return { view: "connections", service: workspace.service ?? undefined };
    return googleReturn() ? { view: "connections", service: "gmail" } : null;
  });
  const { view } = route;
  const adminTab = route.engine ? "engine" : "members";
  const lastEngineTab = useRef(false);
  useEffect(() => {
    if (view === "admin") lastEngineTab.current = !!route.engine;
  }, [view, route.engine]);
  const [watchThread, setWatchThread] = useState<string | null>(null);
  const [browserChoice, setBrowserChoice] = useState<{
    thread: string | null; mode: "chat" | "start" | "guide";
  } | null>(null);
  const browserOpenButton = useRef<HTMLElement | null>(null);
  const [openInbox, setOpenInbox] = useState<GoogleReadIntent | null>(null);
  useEffect(() => {
    if (view !== "connections" || route.service !== "gmail") setOpenInbox(null);
  }, [view, route.service]);
  const [googleReads, setGoogleReads] = useState<ThreadGoogleRead[]>([]);
  const [mailError, setMailError] = useState("");
  const mailVersion = useRef(0);
  const googleThread = useRef(
    googleReadDestination(session)?.thread_id ?? null,
  );
  const googleIntent = useRef(googleReadDestination(session));
  const completedGoogle = useRef<{
    intent: GoogleReadIntent;
    legacyDraft?: string;
    verified: boolean;
  } | null>(null);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const memoryRef = useRef(memory);
  memoryRef.current = memory;
  const initialized = useRef(false),
    mutation = useRef(false),
    threadInventoryVersion = useRef(0),
    historyVersion = useRef(0);
  const chatScroll = useRef<HTMLDivElement>(null);
  const chatEditor = useRef<HTMLTextAreaElement>(null);
  const followTail = useRef(true);
  const taskTurn = useRef<{ thread: string; run: string; lastCursor?: number } | null>(null);
  const olderScroll = useRef<{ top: number; height: number } | null>(null);
  const currentPage = page?.thread_id === selected ? page : null;
  const receiptIds = (currentPage?.turns ?? []).filter(t => t.run.model?.provider_id === WORK_PROVIDER_ID).map(t => t.run.request_id).filter((id): id is string => typeof id === "string" && !!id);
  const completions = useWorkCompletions(session.user.id, session.chat_draft_scope, view === "chat" ? selected : null);
  const work = useConversationWork({
    observe: completions.observe, sessionScope: session.chat_draft_scope,
    accountId: session.user.id, thread: selected, enabled: view === "chat", receiptIds, expired,
    onNewGroup: () => { if (selectedRef.current) void history(selectedRef.current); },
  });
  const [workNotice, setWorkNotice] = useState<WorkNotice | null>(null);
  const workBypass = useRef<string | null>(null);
  useEffect(() => { setWorkNotice(null); }, [selected, session.user.id]);
  const watching = view === "chat" && !!selected && watchThread === selected;
  const browser = usePersonalBrowser(session.user.id, view === "chat" ? selected : null, watching, expired,
    (thread) => { if (selectedRef.current === thread) void history(thread); });
  const browserWork = browser.status?.workspace ?? null;
  const acceptsGuidance = browserNeedsGuidance(browserWork);
  const preferredMode = browserChoice?.thread === selected ? browserChoice.mode : undefined;
  const browserMode = preferredMode === "chat" ? "chat"
    : acceptsGuidance ? "guide" : preferredMode === "start" ? "start" : "chat";
  const pending = selected ? memory.pending[selected] : undefined,
    draft = memory.drafts[selected ?? "new"] ?? "",
    ready =
      cap?.enabled &&
      cap.capability === "text_only_preview" &&
      cap.tools === false;
  useEffect(() => {
    if (selected && pending?.browser_request?.kind === "start" &&
        browserWork?.request_id === pending.request_id) clearSend(selected, pending);
  }, [selected, pending, browserWork?.request_id]);
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) expired();
    else setError(errorText(e));
  }
  function change(update: (m: ChatMemory) => ChatMemory) {
    const value = update(memoryRef.current);
    memoryRef.current = value;
    setMemory(value);
  }
  function clearCompletedGoogleDraft(m: ChatMemory): ChatMemory {
    const done = completedGoogle.current;
    if (
      !done ||
      !done.verified ||
      googleIntent.current?.thread_id !== done.intent.thread_id ||
      googleIntent.current?.request_id !== done.intent.request_id
    )
      return m;
    const request = m.pending[done.intent.thread_id];
    if (request?.status === "inbox" && request.request_id === done.intent.request_id) {
      const pending = { ...m.pending };
      delete pending[done.intent.thread_id];
      return { ...m, pending };
    }
    // Older OAuth flows kept their original text in the editor. New handoffs
    // carry their own request slot, even when the next draft is identical.
    if (done.legacyDraft === undefined || m.drafts[done.intent.thread_id] !== done.legacyDraft) return m;
    const drafts = { ...m.drafts };
    delete drafts[done.intent.thread_id];
    return { ...m, drafts };
  }
  async function inventory() {
    const version = ++threadInventoryVersion.current;
    const result = await scope.request<ThreadsResponse>("/threads");
    if (version === threadInventoryVersion.current) {
      setThreads(result.threads);
      setInventoryError(false);
    }
  }
  const capabilityVersion = useRef(0);
  const checkCapability = useCallback(async () => {
    const version = ++capabilityVersion.current;
    try {
      const capability = await scope.request<Capability>("/text-preview");
      if (version === capabilityVersion.current) {
        setCap(capability);
        setCapError("");
      }
    } catch (e) {
      if (version !== capabilityVersion.current || isAborted(e)) return;
      setCap(null);
      if (e instanceof ApiFailure && e.status === 401) expired();
      else setCapError(e instanceof ApiFailure && e.status === 404 ? ""
        : "대화 가능 상태를 확인하지 못했습니다. 상태를 새로고침해 주세요.");
    }
  }, [scope, expired]);
  useEffect(() => {
    if (view !== "chat") return;
    setCap(undefined);
    setCapError("");
    void checkCapability();
    return () => { ++capabilityVersion.current; };
  }, [view, refresh, checkCapability]);
  useEffect(() => {
    let live = true;
    setLoading(true);
    setInventoryError(false);
    setError("");
    const inventoryVersion = ++threadInventoryVersion.current;
    void scope
      .request<ThreadsResponse>("/threads")
      .then((result) => {
        if (!live || inventoryVersion !== threadInventoryVersion.current) return;
        setThreads(result.threads);
        if (!initialized.current) {
          const restored = recovery?.load(result.threads.map((t) => t.id));
          const created =
            restored?.memory.threadCreation &&
            result.threads.find(
              (t) => t.id === restored.memory.threadCreation!.request_id,
            );
          if (restored) {
            const value = clearCompletedGoogleDraft(
              created
                ? moveNewDraft(restored.memory, created.id)
                : restored.memory,
            );
            setMemory(value);
            memoryRef.current = value;
          }
          setSelected(
            result.threads.find((t) => t.id === googleThread.current)?.id ??
              created?.id ??
              restored?.selected ??
              (restored?.memory.drafts.new || restored?.memory.threadCreation
                ? null
                : result.threads[0]?.id ?? null),
          );
          initialized.current = true;
        }
      })
      .catch((e) => {
        if (!live || inventoryVersion !== threadInventoryVersion.current || isAborted(e)) return;
        if (e instanceof ApiFailure && e.status === 401) expired();
        else setInventoryError(true);
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [scope, recovery, refresh]);
  useEffect(() => {
    if (initialized.current)
      recovery?.save(
        memory,
        selected,
        threads.map((t) => t.id),
      );
  }, [memory, selected, threads, recovery]);
  useEffect(() => {
    let live = true;
    const check = () => {
      setGoogleReads([]);
      void scope
        .request<SessionView>("/auth/session")
        .then((v) => {
          if (!live) return;
          if (
            v.user.id !== session.user.id ||
            v.chat_draft_scope !== session.chat_draft_scope ||
            v.password_change_required !== session.password_change_required ||
            v.installation_admin !== session.installation_admin
          )
            changedSession(v);
          else if (view === "chat") {
            void checkCapability();
            if (selectedRef.current) {
              void history(selectedRef.current);
              void mailHistory(selectedRef.current);
            }
          }
        })
        .catch(e => { if (live) fail(e); });
    };
    window.addEventListener("focus", check);
    return () => { live = false; window.removeEventListener("focus", check); };
  }, [scope, session, view, checkCapability]);
  function reconcile(thread: string, turns: ChatTurn[]) {
    const followUp = memoryRef.current.instructions?.[thread]?.pending;
    if (followUp && turns.some(t => t.run.instructions?.some(r => isTextFollowUp(r) && sameInstruction(r, followUp))))
      change(m => clearSavedFollowUp(m, followUp));
    const value = memoryRef.current.pending[thread];
    if (!value) return;
    if (value.browser_request?.kind === "guide" && turns.some((turn) =>
      turn.run.instructions?.some((instruction) => instruction.request_id === value.request_id))) {
      clearSend(thread, value);
      return;
    }
    const found = turns.find((t) => t.run.request_id === value.request_id);
    if (found && (!active(found) || value.browser_request?.kind === "start")) clearSend(thread, value);
  }
  function clearSend(thread: string, value: PendingSend, rejected = false) {
    change((m) => {
      if (m.pending[thread]?.request_id !== value.request_id) return m;
      // Keep the rejected text alongside the next draft if the editor is occupied.
      if (rejected && m.drafts[thread])
        return { ...m, pending: { ...m.pending, [thread]: { ...value, status: "rejected" } } };
      const pending = { ...m.pending }, drafts = { ...m.drafts };
      delete pending[thread];
      if (rejected && !drafts[thread]) drafts[thread] = value.text;
      return { ...m, pending, drafts };
    });
  }
  async function history(thread: string, before?: number) {
    const version = ++historyVersion.current;
    setReading(true);
    try {
      const result = await scope.request<ChatPage>(
        `/threads/${encodeURIComponent(thread)}/messages${before ? `?before=${before}` : ""}`,
      );
      if (version !== historyVersion.current || selectedRef.current !== thread)
        return;
      if (before && chatScroll.current) {
        const { scrollTop, scrollHeight } = chatScroll.current;
        olderScroll.current = { top: scrollTop, height: scrollHeight };
      }
      setPage((current) =>
        before && current?.thread_id === thread
          ? {
              ...result,
              turns: [
                ...result.turns,
                ...current.turns.filter(
                  (t) => !result.turns.some((old) => old.run.id === t.run.id),
                ),
              ],
            }
          : result,
      );
      reconcile(thread, result.turns);
      setError("");
    } catch (e) {
      if (version === historyVersion.current) fail(e);
    } finally {
      if (version === historyVersion.current) setReading(false);
    }
  }
  async function mailHistory(thread: string) {
    const version = ++mailVersion.current;
    setGoogleReads([]);
    setMailError("");
    try {
      const result = await scope.request<{ reads: ThreadGoogleRead[] }>(
        "/personal/google/history",
        "POST",
        { thread_id: thread },
        undefined,
        { timeoutMs: 15000 },
      );
      if (version !== mailVersion.current || selectedRef.current !== thread)
        return;
      const reads = result.reads.filter((r) => r.thread_id === thread);
      setGoogleReads(reads);
      const handoff = memoryRef.current.pending[thread];
      if (handoff?.status === "inbox" && reads.some(r => r.request_id === handoff.request_id && r.state === "completed"))
        clearSend(thread, handoff);
      const candidate = completedGoogle.current;
      if (
        candidate &&
        result.reads.some(
          (r) =>
            r.state === "completed" &&
            r.thread_id === candidate.intent.thread_id &&
            r.request_id === candidate.intent.request_id,
        )
      ) {
        candidate.verified = true;
        change(clearCompletedGoogleDraft);
      }
    } catch (e) {
      if (version !== mailVersion.current) return;
      setGoogleReads([]);
      if (e instanceof ApiFailure && e.status === 401) expired();
      else if (!isAborted(e) && !(e instanceof ApiFailure && e.status === 404))
        setMailError(
          "저장된 메일 확인 기록을 불러오지 못했습니다. 대화 기록은 계속 이용할 수 있습니다.",
        );
    }
  }
  useEffect(() => {
    ++historyVersion.current;
    setPage(null);
    ++mailVersion.current;
    setGoogleReads([]);
    setMailError("");
    setError("");
    if (selected && view === "chat") {
      void history(selected);
      void mailHistory(selected);
    } else {
      setReading(false);
    }
  }, [selected, scope, refresh, view]);
  useLayoutEffect(() => {
    followTail.current = true;
    olderScroll.current = null;
  }, [selected, view]);
  useLayoutEffect(() => {
    const element = chatScroll.current;
    if (!element) return;
    if (olderScroll.current) {
      element.scrollTop = olderScroll.current.top + element.scrollHeight - olderScroll.current.height;
      olderScroll.current = null;
    } else if (followTail.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [currentPage, pending, googleReads, view]);
  // A task can refer to an earlier conversation page. Follow only the
  // cursors returned for that thread and stop if a page fails to advance.
  useEffect(() => {
    const target = taskTurn.current;
    if (!target || view !== "chat" || selected !== target.thread || !currentPage || reading) return;
    const turn = document.getElementById(`member-turn-${target.run}`);
    if (turn) {
      taskTurn.current = null;
      followTail.current = false;
      turn.focus({ preventScroll: true });
      turn.scrollIntoView?.({ block: "center" });
    } else if (currentPage.next_before != null && target.lastCursor !== currentPage.next_before) {
      target.lastCursor = currentPage.next_before;
      void history(target.thread, currentPage.next_before);
    } else {
      taskTurn.current = null;
    }
  }, [currentPage, reading, selected, view]);
  useEffect(() => {
    const element = chatScroll.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const resize = new ResizeObserver(() => {
      if (followTail.current) element.scrollTop = element.scrollHeight;
    });
    resize.observe(element);
    return () => resize.disconnect();
  }, [view]);
  useEffect(() => {
    const waitingForText = pending && !pending.browser_request && ["sending", "accepted"].includes(pending.status);
    if (view !== "chat" || !selected || reading || (!waitingForText && !page?.turns.some(active))) return;
    const timer = window.setInterval(() => void history(selected), 2000);
    return () => window.clearInterval(timer);
  }, [selected, page?.turns.some(active), pending?.status, pending?.browser_request, view, reading, scope]);
  async function createThread(proposedTitle: string) {
    setError("");
    const value: PendingThreadCreation = memoryRef.current.threadCreation ?? {
      request_id: crypto.randomUUID(),
      title: proposedTitle.trim() || "새 대화",
      status: "sending",
    };
    change((m) => ({ ...m, threadCreation: value }));
    try {
      const created = await scope.request<ThreadSummary>(
        "/threads",
        "POST",
        { title: value.title, request_id: value.request_id },
        undefined,
        { timeoutMs: 15000 },
      );
      ++threadInventoryVersion.current;
      setThreads((current) => [
        created,
        ...current.filter((t) => t.id !== created.id),
      ]);
      change((m) => moveNewDraft(m, created.id));
      setSelected(created.id);
      return created.id;
    } catch (e) {
      if (!isAborted(e)) {
        change((m) => ({
          ...m,
          threadCreation: { ...value, status: "uncertain" },
        }));
        fail(e);
      }
      return null;
    }
  }
  async function confirmThreadCreation() {
    const previous = memoryRef.current.threadCreation;
    if (!previous || mutation.current) return;
    mutation.current = true;
    setMutating(true);
    try {
      await createThread(previous.title);
    } finally {
      mutation.current = false;
      setMutating(false);
    }
  }
  function openInboxRequest(thread: string, requestId: string) {
    const intent = { thread_id: thread, request_id: requestId };
    googleIntent.current = intent;
    setOpenInbox(intent);
    open({ view: "connections", service: "gmail" });
  }
  // Deterministic follow-ups on existing work (status / cancel / refine). Returns true when
  // the text was consumed here; false leaves it to ordinary chat.
  function runControl(control: WorkControl, text: string, thread: string): boolean {
    const shown = receiptIds.slice().reverse().map(id => work.groups[id]).filter(Boolean);
    const extra = Object.values(work.groups).filter(g => !receiptIds.includes(g.id));
    const resolution = resolveControl(control, [...shown, ...extra]);
    const consume = () => change(m => {
      const drafts = { ...m.drafts };
      if (drafts[thread] === text) delete drafts[thread];
      return { ...m, drafts };
    });
    switch (resolution.kind) {
      case "pass":
        if (!receiptIds.length) return false;
        work.refresh();
        setWorkNotice({ thread, kind: "clarify", text: "업무 기록을 불러오고 있습니다. 잠시 후 다시 확인해 주세요.", draft: text });
        return true;
      case "status":
        consume(); work.refresh();
        setWorkNotice({ thread, kind: "status", groupId: resolution.group.id });
        return true;
      case "clarify":
        setWorkNotice({ thread, kind: "clarify", text: resolution.message, draft: text });
        return true;
      case "choose": {
        const { action, text: term } = resolution;
        setWorkNotice({
          thread, kind: "choose", draft: text, text: "어느 검색에 적용할까요?",
          choices: resolution.candidates.map(({ group, task }) => ({
            label: `${task.title} (“${task.query}”)`,
            run: () => { consume(); setWorkNotice(null); void work.act(group, task, action, term !== undefined ? { text: term, question_id: action === "answer" ? task.question_id ?? undefined : undefined } : {}); },
          })),
        });
        return true;
      }
      case "act":
        consume(); setWorkNotice(null);
        for (const { group, task } of resolution.targets)
          void work.act(group, task, resolution.action, resolution.text !== undefined ? { text: resolution.text, question_id: resolution.action === "answer" ? task.question_id ?? undefined : undefined } : {});
        return true;
    }
  }
  async function send(event?: FormEvent, replay = false, followUp?: RunInstructionReceipt, batch?: WorkItemInput[]) {
    event?.preventDefault();
    if (mutation.current || loading || (inventoryError && !initialized.current)) return;
    let thread = selected;
    const old = thread ? memoryRef.current.pending[thread] : undefined;
    const text = batch ? compactMessage(batch) : followUp?.text ?? memoryRef.current.drafts[thread ?? "new"] ?? "";
    if (followUp && (followUp.thread_id !== thread || followUp.reason !== "text_follow_up_saved")) return;
    if (old && !replay) return;
    if (!old && (!text.trim() || new TextEncoder().encode(text).length > 8192))
      return;
    const bypass = workBypass.current === text;
    workBypass.current = null;
    if (!old && !followUp && !batch && !bypass && thread && browserMode === "chat") {
      const control = parseWorkControl(text);
      if (control && runControl(control, text, thread)) return;
    }
    const intent = old || followUp || browserMode === "guide" || preferredMode === "chat"
      ? null : browserIntent(text, browserMode === "start");
    if (intent && !intent.url) {
      setError("브라우저로 열 http 또는 https 주소를 메시지에 넣어 주세요.");
      return;
    }
    const browserRequest = old ? old.browser_request : followUp ? undefined : (
      browserMode === "guide" && browserWork
        ? { kind: "guide" as const, workspace_id: browserWork.id }
        : intent?.url ? { kind: "start" as const, url: intent.url, wait_for_user: intent.wait } : undefined
    );
    if (!old && browserRequest?.kind === "start" && browser.status?.configured === false) {
      setError("이 설치에서 브라우저 작업을 사용할 수 없습니다.");
      return;
    }
    if (browserRequest?.kind === "guide" && new TextEncoder().encode(old?.text ?? text).length > 2000) {
      setError("추가 지시가 너무 깁니다. 내용을 줄여 주세요.");
      return;
    }
    const parsedWork = old || followUp || browserRequest || browserMode !== "chat" ? null
      : batch ? { kind: "items" as const, items: batch } : parseWorkItems(text);
    if (parsedWork?.kind === "invalid") { setError(parsedWork.message); return; }
    const workRequest = old ? old.work_request : parsedWork ? { items: parsedWork.items } : undefined;
    const research = old ? old.research_request : (!followUp && !browserRequest && !workRequest && browserMode==="chat" ? personalResearchIntent(text) : null);
    if (research && new TextEncoder().encode(research.query).length>128){setError("검색어를 128바이트 이내로 줄여 주세요.");return;}
    if (!browserRequest && !research && !workRequest && !ready) return;
    if (!old && !workRequest && currentPage?.turns.some((turn) => active(turn) && turn.run.id !== browserWork?.run_id)) return;
    const value: PendingSend = old ?? {
      request_id: followUp?.request_id ?? crypto.randomUUID(),
      text,
      status: "sending",
      ...(followUp ? { follow_up_run_id: followUp.run_id } : {}),
      ...(research ? { research_request: research } : {}),
      ...(workRequest ? { work_request: workRequest } : {}),
      ...(browserRequest ? { browser_request: browserRequest } : {}),
    };
    followTail.current = true;
    mutation.current = true;
    setMutating(true);
    if (!thread) {
      thread = await createThread(
        Array.from(text.trim()).slice(0, 40).join(""),
      );
      if (!thread) {
        mutation.current = false;
        setMutating(false);
        return;
      }
    }
    const destination = thread;
    setError("");
    change((m) => {
      const drafts = { ...m.drafts };
      if (!followUp && !old && drafts[destination] === value.text) delete drafts[destination];
      return { ...m, drafts, pending: { ...m.pending, [destination]: { ...value, status: "sending" } } };
    });
    try {
      if (value.work_request) {
        try {
          await work.post(destination, { request_id: value.request_id, message: value.text, items: value.work_request.items });
        } catch (e) {
          if (!isAborted(e) && e instanceof ApiFailure && [400, 403, 404, 409, 422, 429].includes(e.status)) {
            clearSend(destination, value, true);
            setError(e.status === 429 ? "한꺼번에 맡길 수 있는 검색 수를 넘었습니다. 잠시 후 다시 시도해 주세요."
              : "검색 요청을 처리하지 못했습니다. 입력을 유지했습니다.");
            return;
          }
          throw e;
        }
        clearSend(destination, value);
        if (selectedRef.current === destination) void history(destination);
        void inventory().catch(fail);
        return;
      }
      if (value.research_request) {
        await scope.request<WorkReceipt>("/personal/tasks/workflows","POST",{
          request_id:value.request_id, thread_id:destination, message:value.text, title:value.research_request.query+" 조사",
          ...value.research_request,period:0,first_due:null,
        },undefined,{timeoutMs:30000});
        clearSend(destination,value);
        if (selectedRef.current===destination) void history(destination);
        void inventory().catch(fail);
        return;
      }
      if (value.browser_request) {
        const body = value.browser_request.kind === "start"
          ? { url: value.browser_request.url, wait_for_user: value.browser_request.wait_for_user }
          : { workspace_id: value.browser_request.workspace_id };
        const status = await scope.request<PersonalBrowserStatus>(
          browserEndpoint(destination), "POST", { request_id: value.request_id, text: value.text, ...body },
          undefined, { timeoutMs: 90000 },
        );
        browser.publish(destination, status);
        if (!status.configured || !status.workspace) {
          clearSend(destination, value, true);
          setError("이 설치에서 브라우저 작업을 사용할 수 없습니다.");
          return;
        }
        clearSend(destination, value);
        setBrowserChoice(null);
        if (selectedRef.current === destination) void history(destination);
        void inventory().catch(fail);
        return;
      }
      const turn = await scope.request<ChatTurn>(
        `/threads/${encodeURIComponent(thread)}/text-preview`,
        "POST",
        {
          request_id: value.request_id,
          text: value.text,
          capability: "text_only_preview",
          ...(value.follow_up_run_id ? { follow_up_run_id: value.follow_up_run_id } : {}),
        },
        undefined,
        { timeoutMs: 90000 },
      );
      if (selectedRef.current === thread)
        setPage((current) => ({
          thread_id: thread,
          cursor: current?.cursor ?? 0,
          next_before: current?.next_before ?? null,
          turns: [
            ...(current?.thread_id === thread ? current.turns : []).filter(
              (t) => t.run.id !== turn.run.id,
            ),
            turn,
          ].sort((a, b) => a.sequence - b.sequence),
        }));
      if (active(turn))
        change((m) => ({
          ...m,
          pending: {
            ...m.pending,
            [destination]: { ...value, status: "accepted" },
          },
        }));
      else reconcile(thread, [turn]);
      if (value.follow_up_run_id && selectedRef.current === thread) void history(thread);
      void inventory().catch(fail);
    } catch (e) {
      if (!isAborted(e)) {
        if (value.browser_request && e instanceof ApiFailure &&
            [400, 401, 403, 404, 409, 422, 429].includes(e.status)) {
          clearSend(destination, value, true);
          if (e.status === 401) expired();
          else if ([403, 404].includes(e.status)) setError("이 대화에서 브라우저 작업을 사용할 수 없습니다.");
          else if (e.status === 409) setError("브라우저 상태가 바뀌었습니다. 현재 작업을 확인해 주세요.");
          else setError("브라우저 요청을 처리하지 못했습니다. 입력을 유지했습니다.");
          return;
        }
        if (
          e instanceof ApiFailure &&
          [400, 401, 403, 404, 422, 429].includes(e.status)
        ) {
          const inbox = e.status === 422 && e.code === "personal_inbox_requested";
          if (inbox) {
            change(m => ({ ...m, pending: { ...m.pending, [destination]: { ...value, status: "inbox" } } }));
            openInboxRequest(destination, value.request_id);
            return;
          }
          clearSend(destination, value, !value.follow_up_run_id);
          if (e.status === 422 && e.code === "personal_text_input_budget_exceeded") {
            if (selectedRef.current === destination) {
              setError(value.follow_up_run_id
                ? "원래 질문과 후속 지시가 한 번에 보낼 수 있는 길이를 넘었습니다. 저장한 지시를 확인해 주세요."
                : "한 번에 보낼 수 있는 길이를 넘었습니다. 입력을 줄여 다시 보내세요.");
            }
            return;
          }
          if (value.follow_up_run_id && e.status !== 401 && selectedRef.current === destination) {
            setError("후속 메시지를 보내지 못했습니다. 저장한 지시에서 다시 보낼 수 있습니다.");
            return;
          }
          if (e.code === "personal_text_model_unavailable") {
            ++capabilityVersion.current;
            setCap(null);
            setError("답변을 받을 수 없어 메시지를 보내지 못했습니다. 입력을 유지했습니다.");
          } else fail(e);
        } else {
          change((m) => ({
            ...m,
            pending: {
              ...m.pending,
              [destination]: { ...value, status: "uncertain" },
            },
          }));
          setError(
            "전송 결과를 확인하지 못했습니다. 결과를 확인하거나 같은 요청으로 다시 확인하세요.",
          );
        }
      }
    } finally {
      mutation.current = false;
      setMutating(false);
    }
  }
  async function remove(thread: string) {
    if (mutation.current || !threads.some((item) => item.id === thread)) return false;
    mutation.current = true;
    setMutating(true);
    try {
      await scope.request(
        `/threads/${encodeURIComponent(thread)}`,
        "DELETE",
        undefined,
        undefined,
        { timeoutMs: 15000 },
      );
    } catch (e) {
      if (e instanceof ApiFailure && e.status === 401) expired();
      if (!(e instanceof ApiFailure && e.status === 404)) return false;
    } finally {
      mutation.current = false;
      setMutating(false);
    }
    ++threadInventoryVersion.current;
    setThreads((current) => current.filter((item) => item.id !== thread));
    const fallback = threads.find((item) => item.id !== thread)?.id ?? null;
    setSelected((current) => current === thread ? fallback : current);
    setWatchThread((current) => current === thread ? null : current);
    setBrowserChoice((current) => current?.thread === thread ? null : current);
    change((m) => {
      const drafts = { ...m.drafts },
        pending = { ...m.pending };
      delete drafts[thread];
      delete pending[thread];
      return { ...m, drafts, pending };
    });
    return true;
  }
  function leaveManagement() {
    setAdmin(false);
    open({ view: "chat" });
  }
  function navigate(destination: MemberView, back = false) {
    taskTurn.current = null;
    if (destination === "connections" && view !== "connections") {
      setOpenInbox(null);
    }
    open({ view: destination, engine: destination === "admin" && lastEngineTab.current }, back);
  }
  function openThread(id: string | null) {
    setSelected(id);
    navigate("chat");
  }
  function openBrowser(trigger: HTMLElement) {
    if (!selected) return;
    browserOpenButton.current = trigger;
    setWatchThread(selected);
  }
  function closeBrowser() {
    setWatchThread(null);
    browserOpenButton.current?.focus({ preventScroll: true });
    browserOpenButton.current = null;
  }
  const turns = currentPage?.turns ?? [];
  const hasActiveTurn = turns.some((turn) => active(turn) && turn.run.id !== browserWork?.run_id);
  const optimistic = pending && !turns.some((turn) => turn.run.request_id === pending.request_id);
  const busy = loading || (selected !== null && reading && !currentPage);
  const composerBusy = loading || busy || mutating || !!pending || hasActiveTurn;
  const isEmpty = !busy && !inventoryError && !error && turns.length === 0 && !pending && googleReads.length === 0 && !browserWork;
  const working = turns.find((turn) => active(turn) && turn.run.id !== browserWork?.run_id);
  const chatActivity = browserWork && ["starting", "reading", "ready", "user_control", "resuming", "stopping"].includes(browserWork.state)
    ? browserStateLabel[browserWork.state]
    : working ? stateLabel(working)
    : pending?.status === "inbox" ? "메일 확인 대기"
    : pending?.status === "rejected" ? "이전 메시지 전송 실패"
    : pending?.status === "uncertain" ? "전송 결과 확인 필요"
    : pending?.browser_request ? "브라우저 요청을 확인하는 중"
    : pending ? "답변을 기다리는 중" : undefined;
  function useStarter(prompt: string) {
    const key = selected ?? "new";
    if (memoryRef.current.drafts[key] || (!ready && !/^(?:기억|대화) 검색: /u.test(prompt)) || composerBusy) return;
    change((value) => ({ ...value, drafts: { ...value.drafts, [key]: prompt } }));
    if (chatEditor.current?.disabled) requestAnimationFrame(()=>chatEditor.current?.focus({preventScroll:true}));
    else chatEditor.current?.focus({preventScroll:true});
  }
  const hasWork = Object.keys(work.groups).length > 0;
  // Work needs no model, so these drafts stay sendable while the chat engine is unavailable.
  const draftWorkAllowed = browserMode === "chat" && (parseWorkItems(draft) !== null || (hasWork && parseWorkControl(draft) !== null));
  const thread = threads.find((item) => item.id === selected);
  const heading = view === "chat" ? thread ? threadDisplayTitle(thread.title) : "새 대화"
    : view === "tasks" ? "맡긴 일"
    : view === "memory" ? "기억"
    : view === "connections" ? "커넥터"
    : "관리";
  return (
    <PersonalWorkspaceChrome view={view} title={heading} admin={admin} threads={threads}
      selected={selected} loading={loading} busy={mutating} user={session.user}
      drafts={memory.drafts} inventoryError={inventoryError} chatActivity={chatActivity}
      onRefreshThreads={() => setRefresh((value) => value + 1)}
      onNavigate={navigate} onThread={openThread} onNew={() => openThread(null)}
      onDelete={remove} onSignOut={onSignOut}
      completionMenu={completions.completed.length > 0 && <WorkCompletionMenu items={completions.completed}
        onDismiss={completions.dismiss} onOpen={run => { completions.dismiss(run); open({ view: "tasks", taskId: run }); }} />}>
      {view !== "chat" && (
        <div key={view} className="member-panel">
          {view === "admin" && admin ? (
            <div className="member-settings-layout">
              <nav className="member-settings-tabs" aria-label="설치 관리">
                <button aria-pressed={adminTab === "members"} onClick={() => open({ view: "admin" })}>회원 관리</button>
                <button aria-pressed={adminTab === "engine"} onClick={() => open({ view: "admin", engine: true })}>어시스턴트 엔진</button>
              </nav>
              {adminTab === "members" ? (
                <PersonalAdmin account={session.user.id} recoveryScope={session.chat_draft_scope}
                  openGoogleSetup={() => open({ view: "connections", setup: true })} expired={expired} unavailable={leaveManagement} />
              ) : (
                <PersonalEngineSettings account={session.user.id} expired={expired} unavailable={leaveManagement} />
              )}
            </div>
          ) : view === "connections" ? (
            <PersonalConnections
              navigation={{ route, open }}
              installationAdmin={admin}
              onAdminUnavailable={onAdminUnavailable}
              onBack={leaveCatalog}
              session={session}
              expired={expired}
              readIntent={openInbox}
              returned={(intent, uncertain, draftText) => {
                if (
                  googleIntent.current?.thread_id === intent.thread_id &&
                  googleIntent.current?.request_id === intent.request_id
                ) {
                  completedGoogle.current = {
                    intent,
                    legacyDraft: draftText,
                    verified: !uncertain,
                  };
                  if (!uncertain) change(clearCompletedGoogleDraft);
                }
                googleThread.current = intent.thread_id;
                setSelected(intent.thread_id);
                setOpenInbox(null);
                open({ view: "chat" });
                setRefresh((v) => v + 1);
                if (uncertain)
                  setError(
                    "저장된 메일 조회 결과만 확인합니다. 자동으로 다시 읽지 않았습니다.",
                  );
              }}
            />
          ) : view === "tasks" ? (
            <PersonalTasks accountId={session.user.id} onExpired={expired} taskId={route.taskId}
              onOpenTask={taskId => open({ view: "tasks", taskId })}
              onBack={() => open({ view: "tasks" }, true)}
              onThread={(thread, run) => {
                openThread(thread);
                if (run) taskTurn.current = { thread, run };
              }} />
          ) : view === "memory" ? (
            <PersonalMemories accountId={session.user.id} onExpired={expired}
              drafts={memoryDrafts} updateDrafts={setMemoryDrafts} />
          ) : null}
        </div>
      )}
      {view === "chat" && (
        <div className="member-chat-layout">
        <section className="member-chat" aria-label="대화">
          <div className="member-chat-scroll" ref={chatScroll} tabIndex={0} aria-label="대화 기록"
            onScroll={(event) => {
              const { scrollTop, scrollHeight, clientHeight } = event.currentTarget;
              followTail.current = scrollHeight - scrollTop - clientHeight < 96;
            }}>
            <div className="member-chat-content" data-empty={isEmpty || undefined}>
              {(capError || error || inventoryError) && (
                <div className="member-chat-notice">
                  {inventoryError && <p role="alert">대화 목록을 불러오지 못했습니다.</p>}
                  {capError && <p role="alert">{capError}</p>}
                  {error && <p role="alert">{error}</p>}
                  {(capError || inventoryError || !pending || pending.status === "rejected") && (
                    <Button label="상태 새로고침" variant="ghost"
                      isDisabled={loading || mutating} onClick={() => {
                        browser.refresh();
                        setRefresh((v) => v + 1);
                      }} />
                  )}
                </div>
              )}
              {!loading && cap !== undefined && !ready && (
                <div className="member-chat-notice">
                  <p role="status">{admin
                    ? "어시스턴트 엔진의 연결과 설정을 확인해 주세요."
                    : "지금 답변을 받을 수 없습니다. 관리자에게 문의해 주세요."}</p>
                  {!draft && <button type="button" disabled={composerBusy} onClick={()=>useStarter("기억 검색: ")}>저장된 기억 검색</button>}
                  {admin && (
                    <Button label="엔진 설정 열기" variant="ghost" onClick={() => {
                      open({ view: "admin", engine: true });
                    }} />
                  )}
                </div>
              )}
              {busy && <p className="member-chat-status" role="status">대화 불러오는 중…</p>}
              {isEmpty && (
                <section className="member-conversation-start" aria-labelledby="member-conversation-start-title">
                  <img src="/moosuri-icon-192.png" width="64" height="64" alt="" />
                  <h2 id="member-conversation-start-title">무엇을 도와드릴까요?</h2>
                  {ready && browserMode === "chat" && !draft && (
                    <div className="member-chat-starters" role="group" aria-label="메시지 작성 예시">
                      {chatStarters.map(({ label, prompt }) => (
                        <button key={label} type="button" onClick={() => useStarter(prompt)} disabled={composerBusy}>
                          {label}
                        </button>
                      ))}
                    </div>
                  )}
                </section>
              )}
              {!selected && memory.threadCreation?.status === "uncertain" && (
                <div className="member-chat-notice">
                  <p role="status">새 대화가 만들어졌는지 확인하지 못했습니다.</p>
                  <Button label="같은 대화 만들기 다시 확인" variant="ghost"
                    isDisabled={mutating} onClick={() => void confirmThreadCreation()} />
                </div>
              )}
              {selected && currentPage?.next_before && (
                <Button className="member-older" label="이전 메시지" variant="ghost" isDisabled={reading}
                  onClick={() => void history(selected, currentPage.next_before ?? undefined)} />
              )}
              <div className="member-chat-turns" aria-live="polite">
                {turns.map((turn) => (
                  <article className="member-chat-turn" id={`member-turn-${turn.run.id}`} tabIndex={-1} key={turn.run.id}>
                    <div className="member-message">
                      <span className="sr-only">나</span>
                      <p>{turn.user_text}</p>
                    </div>
                    {turn.run.instructions && <>
                      <InstructionReceipts receipts={turn.run.instructions.filter(r => !isTextFollowUp(r))} />
                      {turn.run.instructions.filter(isTextFollowUp).map(r =>
                        <div className="member-chat-notice" key={r.request_id}>
                          <p>{r.text}</p>
                          <span role="status">{r.reason === "text_follow_up_sent" || turns.some(t => t.run.request_id === r.request_id)
                            ? "후속 메시지로 보냄" : "후속 지시 저장됨 · 이 답변에는 미반영"}</span>
                          {r.reason === "text_follow_up_saved" && !turns.some(t => t.run.request_id === r.request_id) &&
                            <button type="button" disabled={composerBusy || !ready || browserMode !== "chat"} onClick={() => void send(undefined, false, r)}>후속 메시지로 보내기</button>}
                        </div>)}
                    </>}
                    {(turn.assistant.length > 0 || turn.run.id !== browserWork?.run_id) && (
                      <div className="member-reply">
                        <span className="sr-only">무수리</span>
                        {turn.assistant.map((message) => <p key={message.id}>{message.text}</p>)}
                        {turn.run.id !== browserWork?.run_id && turn.run.state !== "completed" && (
                          <span className="member-chat-status" data-working={active(turn) || undefined}>
                            {stateLabel(turn)}
                          </span>
                        )}
                      </div>
                    )}
                    {turn.run.model?.provider_id === WORK_PROVIDER_ID && (
                      <GroupForTurn requestId={turn.run.request_id} work={work}
                        openRun={(runId) => open({ view: "tasks", taskId: runId })} />
                    )}
                    {(turn.run.model?.provider_id==="personal-browser" || turn.run.model?.provider_id==="member-workflow" || ["Deployment text-only preview","Deployment browser answer"].includes(turn.run.model?.provider_name)) && <button className="member-task-link" type="button" onClick={()=>open({view:"tasks",taskId:turn.run.id})}>업무 기록 보기</button>}
                    {browserWork?.run_id === turn.run.id && (
                      <PersonalBrowserCard browser={browser} pending={false} onOpen={openBrowser} />
                    )}
                  </article>
                ))}
                {optimistic && (
                  <div className="member-message">
                    <span className="sr-only">나</span>
                    <p>{pending.text}</p>
                  </div>
                )}
                {selected && pending && (optimistic || pending.status !== "sending") && (
                  <div className="member-pending">
                    {optimistic && (
                      <p className="member-chat-status" data-working={pending.status === "sending" || undefined} role="status">
                        {pending.status === "uncertain"
                          ? "아직 전송 결과를 확인하지 못했습니다. 새 요청은 만들지 않았습니다."
                          : pending.status === "inbox" ? "Gmail에서 메일 확인을 이어갈 수 있습니다."
                          : pending.status === "rejected" ? "이 메시지를 보내지 못했습니다."
                          : pending.browser_request ? "브라우저 요청을 확인하는 중…" : "답변을 기다리는 중…"}
                      </p>
                    )}
                    {(pending.status === "rejected" || pending.status === "inbox") && (
                      <div className="member-retry-actions">
                        {pending.status === "inbox" ? (
                          <Button label="메일 확인 계속하기" variant="ghost" isDisabled={mutating || busy}
                            onClick={() => openInboxRequest(selected, pending.request_id)} />
                        ) : (
                          <Button label="이전 메시지 다시 보내기" variant="ghost"
                            isDisabled={(!pending.browser_request && !pending.research_request && !pending.work_request && !ready) || mutating || busy}
                            onClick={() => void send(undefined, true)} />
                        )}
                        <Button label="이전 메시지 버리기" variant="ghost" isDisabled={mutating}
                          onClick={() => clearSend(selected, pending)} />
                      </div>
                    )}
                    {(pending.status === "uncertain" || pending.status === "accepted") && (
                      <div className="member-retry-actions">
                        <Button label="결과 확인" variant="ghost" isDisabled={reading || mutating}
                          onClick={() => {
                            if (pending.browser_request) browser.refresh();
                            void history(selected);
                          }} />
                        {pending.status === "uncertain" && (
                          <Button label="동일 요청 다시 확인" variant="ghost" isDisabled={mutating}
                            onClick={() => void send(undefined, true)} />
                        )}
                      </div>
                    )}
                  </div>
                )}
                {workNotice && workNotice.thread === selected && (workNotice.kind === "status"
                  ? <WorkStatusNotice group={work.groups[workNotice.groupId]} onClose={() => setWorkNotice(null)} />
                  : draft === workNotice.draft && (
                    <div className="member-chat-notice work-clarify" role="status">
                      <p>{workNotice.text}</p>
                      <div className="member-retry-actions">
                        {workNotice.choices?.map(choice => <button key={choice.label} type="button" onClick={choice.run}>{choice.label}</button>)}
                        {workNotice.kind === "clarify" && (
                          <button type="button" disabled={composerBusy || !ready} onClick={() => {
                            workBypass.current = workNotice.draft; setWorkNotice(null); void send();
                          }}>일반 메시지로 보내기</button>
                        )}
                        <button type="button" onClick={() => setWorkNotice(null)}>닫기</button>
                      </div>
                    </div>
                  ))}
                {mailError && <p className="member-chat-status" role="status">{mailError}</p>}
                {googleReads.length > 0 && <GoogleReadHistory reads={googleReads} />}
                {!turns.some((turn) => turn.run.id === browserWork?.run_id) && (
                  <PersonalBrowserCard browser={browser} pending={pending?.browser_request?.kind === "start"}
                    onOpen={openBrowser} />
                )}
              </div>
            </div>
          </div>
          {!loading && browserMode === "chat" && (
            <div className="work-composer-wrap">
              <WorkComposer disabled={mutating || !!pending || (inventoryError && !initialized.current)}
                onSubmit={(items) => void send(undefined, false, undefined, items)} />
            </div>
          )}
          {!loading && (
            <PersonalChatComposer key={selected ?? "new"} editor={chatEditor} draft={draft}
              followUp={selected && browserMode === "chat" && <PersonalTextFollowUp key={selected}
                thread={selected} scope={scope} draft={draft} run={working?.run.accepts_text_follow_up ? working.run : undefined}
                pending={memory.instructions?.[selected]?.pending} rejected={memory.instructions?.[selected]?.draft} change={change} expired={expired}
                refresh={() => { if (selectedRef.current === selected) void history(selected); }} />}
              disabled={(inventoryError && !initialized.current) || (!ready && browserMode === "chat" && !working?.run.accepts_text_follow_up && !/^(?:기억|대화) 검색[:：]/u.test(draft) && !personalResearchIntent(draft) && !draftWorkAllowed)} busy={composerBusy}
              waiting={busy ? "대화 불러오는 중…"
                : pending?.status === "rejected" || pending?.status === "inbox" ? "다음 메시지를 작성하세요."
                : pending?.status === "uncertain" ? "전송 결과를 확인해 주세요."
                : pending?.browser_request ? "브라우저 요청을 확인하는 중…" : "답변을 기다리는 중…"}
              mode={browserMode}
              browserMode={
                (browserMode !== "chat" || acceptsGuidance || browser.status?.configured || !selected) &&
                <div className="member-browser-mode">
                  {browserMode !== "chat" && <span>{browserMode === "guide" ? "브라우저에 추가 지시" : "브라우저 작업"}</span>}
                  <button type="button" disabled={composerBusy}
                    onClick={() => setBrowserChoice({ thread: selected, mode: browserMode === "chat"
                      ? (acceptsGuidance ? "guide" : "start") : "chat" })}>
                    {browserMode !== "chat" ? "일반 채팅으로 전환" : acceptsGuidance ? "브라우저 작업에 지시" : "브라우저 작업"}
                  </button>
                </div>
              }
              onDraft={(value) => change((m) => ({
                ...m, drafts: { ...m.drafts, [selected ?? "new"]: value },
              }))}
              onSend={() => void send()} />
          )}
        </section>
        {watching && <PersonalBrowserViewer key={selected} browser={browser} onClose={closeBrowser} />}
        </div>
      )}
    </PersonalWorkspaceChrome>
  );
}

function stateLabel(turn: ChatTurn) {
  return {
    queued: "대기 중",
    running: "답변 작성 중",
    completed: "답변 완료",
    failed: "응답 실패",
    cancelled: "취소됨",
    incomplete: turn.assistant.some(message => message.text.length > 0)
      ? "일부 응답 · 자동 재시도 없음" : "답변 미완료 · 표시할 본문 없음",
    refused: "응답 거절",
    outcome_unknown: "응답 확인 불가 · 자동 재시도 없음",
  }[turn.run.state];
}
