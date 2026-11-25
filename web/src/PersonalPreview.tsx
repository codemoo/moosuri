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
import type { WorkReceipt } from "./PersonalWorkflows";
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
  compactMessage, parseWorkControl, resolveControl, WORK_PROVIDER_ID,
  type WorkControl, type WorkItemInput,
} from "./conversation-work";
import { clearWorkRecovery } from "./conversation-work";
import { useWorkCompletions } from "./useWorkCompletions";
import { GroupForTurn, WorkCompletionMenu, useConversationWork, WorkComposer, WorkStatusNotice } from "./ConversationWork";
import { WorkFocus, type WorkFocusState } from "./WorkFocus";
import { MemberWorkMonitors } from "./MemberWorkMonitors";
import { PublicWatchLog, PublicWatchStart, parsePublicWatchChat } from "./MemberPublicWatch";
import { startMemberPublicSearch } from "./member-research-source";
import { ConnectedBriefCard, ConnectedBriefLog } from "./MemberConnectedBrief";
import { browserTimezone, parseConnectedBriefChat, parseMonitorChat } from "./MemberWorkMonitorsChat";
import { interpretUnsupported, parseInterpretation, type WorkInterpretation } from "./work-interpret";
import "./conversation-work.css";
import { MemberMailWork } from "./MemberMailWork";
import {
  MemberMailWriteConsent, clearMailWriteConsent, isMemberConsentReturn, mailWriteReturn, rememberMailWriteConsent,
  settleMailWriteReturn, type MailWriteReturn,
} from "./MemberMailWriteConsent";
import {
  MemberCalendarConsent, calendarReturnThread, clearCalendarConsent, type CalendarConnection,
} from "./MemberCalendarConsent";
import { MemberCalendarWork } from "./MemberCalendarWork";
import { MemberMemoryReceipt, type MemberMemoryReceiptView } from "./MemberMemoryReceipt";
import {
  MemberResearchOutcomes, useResearchOutcomes, type Handoff,
} from "./MemberResearchOutcomes";
import { browserLocation } from "./browser-frame";
import type { MemberPendingRequest, MemberPendingSend } from "./chat-drafts";

import { composerWork, localZone, type MemberRoute } from "./member-work-intent";
import { monitorCommand, submitMonitor } from "./member-monitor-command";
export { memberRoute, explicitCalendarRequest, explicitMailRequest, parseMailDetails } from "./member-work-intent";
export type { MemberRoute } from "./member-work-intent";

/* Tab-session recovery for what is not a chat pending send: only what the member typed plus opaque ids.
 * No Gmail subjects/senders/bodies, calendar events, memory text or outcome contents: those are re-read. */
type MemberThreadWork = {
  publicWatch?: { request_id: string; text: string };
  brief?: { request_id: string; text: string; zone: string };
  calendar?: { request_id: string; text: string; at: number; zone: string };
  mail?: { request_id: string; text: string; to?: string; body?: string; message_id?: string };
  research?: string[];
};
type MemberWork = Record<string, MemberThreadWork>;
export const MEMBER_WORK_KEY = "moosuri.member-work.v1";
const MEMBER_WORK_TTL = 8 * 60 * 60 * 1000, MEMBER_WORK_BYTES = 64 * 1024;
const octets = (v: string) => new TextEncoder().encode(v).byteLength;
const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const only = (v: Record<string, unknown>, keys: string[]) => Object.keys(v).every(k => keys.includes(k));
const slotId = (v: string) => /^[a-zA-Z0-9_-]{1,256}$/.test(v) && !["__proto__", "prototype", "constructor"].includes(v);
const isUuid = (v: unknown): v is string => typeof v === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v);
const small = (v: unknown, max: number): v is string => typeof v === "string" && octets(v) <= max;
const zoneOk = (v: unknown): v is string => {
  if (typeof v !== "string" || !v || v.length > 64) return false;
  try { new Intl.DateTimeFormat("en", { timeZone: v }); return true; } catch { return false; }
};
function threadWork(v: unknown): MemberThreadWork | null {
  if (!isObj(v) || !only(v, ["calendar", "mail", "research", "brief", "publicWatch"])) return null;
  const out: MemberThreadWork = {};
  if (v.publicWatch !== undefined) {
    const w = v.publicWatch;
    if (!isObj(w) || !only(w, ["request_id", "text"]) || !isUuid(w.request_id)
      || !small(w.text, 8192) || !parsePublicWatchChat(w.text)?.ok) return null;
    out.publicWatch = { request_id: w.request_id, text: w.text };
  }
  if (v.brief !== undefined) {
    const b = v.brief;
    if (!isObj(b) || !only(b, ["request_id", "text", "zone"]) || !isUuid(b.request_id)
      || !small(b.text, 8192) || !zoneOk(b.zone) || !parseConnectedBriefChat(b.text)?.ok) return null;
    out.brief = { request_id: b.request_id, text: b.text, zone: b.zone as string };
  }
  if (v.calendar !== undefined) {
    const c = v.calendar;
    if (!isObj(c) || !only(c, ["request_id", "text", "at", "zone"]) || !isUuid(c.request_id) || !small(c.text, 8192) || !String(c.text).trim() ||
        typeof c.at !== "number" || !Number.isSafeInteger(c.at) || c.at < 0 || !zoneOk(c.zone)) return null;
    out.calendar = { request_id: c.request_id as string, text: c.text as string, at: c.at as number, zone: c.zone as string };
  }
  if (v.mail !== undefined) {
    const m = v.mail;
    if (!isObj(m) || !only(m, ["request_id", "text", "to", "body", "message_id"]) || !isUuid(m.request_id) || !small(m.text, 8192) || !String(m.text).trim() ||
        (m.to !== undefined && !small(m.to, 1024)) || (m.body !== undefined && !small(m.body, 8192)) ||
        (m.message_id !== undefined && !small(m.message_id, 512))) return null;
    out.mail = { request_id: m.request_id as string, text: m.text as string, ...(m.to ? { to: m.to as string } : {}),
      ...(m.body ? { body: m.body as string } : {}), ...(m.message_id ? { message_id: m.message_id as string } : {}) };
  }
  if (v.research !== undefined) {
    if (!Array.isArray(v.research) || v.research.length > 8 || !v.research.every(isUuid)) return null;
    out.research = v.research.slice() as string[];
  }
  return out.publicWatch || out.brief || out.calendar || out.mail || out.research?.length ? out : null;
}
export function clearMemberWork() {
  try { window.sessionStorage.removeItem(MEMBER_WORK_KEY); } catch { /* Storage unavailable. */ }
}
function loadMemberWork(account: string, scope: string | undefined): MemberWork {
  try {
    const raw = window.sessionStorage.getItem(MEMBER_WORK_KEY);
    if (!raw) return {};
    const v: unknown = JSON.parse(raw);
    if (octets(raw) > MEMBER_WORK_BYTES || !isObj(v) || !only(v, ["version", "account", "scope", "saved_at", "threads"]) || v.version !== 1 ||
        v.account !== account || !scope || v.scope !== scope || typeof v.saved_at !== "number" || (v.saved_at as number) > Date.now() ||
        Date.now() - (v.saved_at as number) >= MEMBER_WORK_TTL || !isObj(v.threads)) throw new Error("foreign or stale");
    const out: MemberWork = {};
    for (const [id, value] of Object.entries(v.threads as Record<string, unknown>)) {
      const work = slotId(id) ? threadWork(value) : null;
      if (work) out[id] = work;
    }
    return out;
  } catch {
    clearMemberWork();
    return {};
  }
}
function saveMemberWork(account: string, scope: string | undefined, work: MemberWork) {
  try {
    const threads = Object.fromEntries(Object.entries(work).filter(([id, w]) => slotId(id) && threadWork(w)).slice(0, 16));
    if (!scope || !Object.keys(threads).length) { window.sessionStorage.removeItem(MEMBER_WORK_KEY); return; }
    const raw = JSON.stringify({ version: 1, account, scope, saved_at: Date.now(), threads });
    if (octets(raw) > MEMBER_WORK_BYTES) window.sessionStorage.removeItem(MEMBER_WORK_KEY);
    else window.sessionStorage.setItem(MEMBER_WORK_KEY, raw);
  } catch { /* Restricted storage only costs reload recovery. */ }
}

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
    clearMemberWork();
    clearCalendarConsent();
    clearMailWriteConsent();
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
/** Exact shape of GET /personal/google/calendar/status (`{ connection: null | { connection_id, epoch, mailbox } }`). */
function parseCalendarStatus(value: unknown): CalendarConnection | null {
  const status = value as { connection?: unknown } | null;
  if (!status || typeof status !== "object" || !("connection" in status)) throw new ApiFailure(502, "calendar_status_invalid");
  if (status.connection === null) return null;
  const c = status.connection as Record<string, unknown> | undefined;
  if (!c || typeof c.connection_id !== "string" || !Number.isInteger(c.epoch) || typeof c.mailbox !== "string")
    throw new ApiFailure(502, "calendar_status_invalid");
  return { connection_id: c.connection_id, epoch: c.epoch as number, mailbox: c.mailbox };
}
/** One calendar request in a thread. The connection comes from the real calendar status route; consent (and its
 * callback) is the member's own MemberCalendarConsent, bound to this thread. The preview, approval and receipt
 * belong to MemberCalendarWork, which keeps one request id and sends this thread id with prepare and apply. */
function CalendarChat({ scope, accountId, sessionScope, threadId, item, fail, onDone, onUnavailable, onDismiss }: {
  scope: Pick<RequestScope, "request">;
  accountId: string;
  sessionScope: string | null | undefined;
  threadId: string;
  item: NonNullable<MemberThreadWork["calendar"]>;
  fail: (e: unknown) => void;
  onDone: () => void;
  onUnavailable: () => void;
  onDismiss: () => void;
}) {
  const [connection, setConnection] = useState<CalendarConnection | null | undefined>(undefined);
  const [problem, setProblem] = useState(false);
  const [lookup, setLookup] = useState(0);
  const consent = useRef<HTMLDivElement>(null);
  const unavailable = useRef(onUnavailable);
  unavailable.current = onUnavailable;
  useEffect(() => {
    let live = true;
    void scope.request<unknown>("/personal/google/calendar/status")
      .then(result => {
        if (!live) return;
        setProblem(false);
        setConnection(parseCalendarStatus(result));
      })
      .catch(e => {
        if (!live || isAborted(e)) return;
        if (e instanceof ApiFailure && (e.status === 404 || e.status === 501)) unavailable.current();
        else if (e instanceof ApiFailure && e.status === 401) fail(e);
        else setProblem(true);
      });
    return () => { live = false; };
  }, [scope, lookup]);
  useEffect(() => {
    const again = () => setLookup(n => n + 1);
    window.addEventListener("focus", again);
    return () => window.removeEventListener("focus", again);
  }, []);
  return (
    <div className="member-calendar-chat">
      <div className="member-message member-request-echo">
        <span className="sr-only">나</span>
        <p>{item.text}</p>
      </div>
      {/* Mounted from the first render so a Google callback fragment is accepted exactly once, whatever the status says. */}
      <div ref={consent}>
        <MemberCalendarConsent scope={scope} accountId={accountId} sessionScope={sessionScope} threadId={threadId}
          connection={connection ?? null}
          onConnected={next => { setProblem(false); setConnection(next); }}
          onDisconnected={() => { setConnection(null); setLookup(n => n + 1); }}
          onExpired={() => fail(new ApiFailure(401, "unauthorized"))} />
      </div>
      {problem && connection === undefined ? (
        <div className="member-chat-notice">
          <p role="alert">캘린더 연결 상태를 확인하지 못했습니다.</p>
          <button type="button" onClick={() => setLookup(n => n + 1)}>다시 확인</button>
        </div>
      ) : connection === undefined ? (
        <p className="member-chat-status" role="status">캘린더 연결 상태를 확인하는 중…</p>
      ) : (
        <MemberCalendarWork key={`${connection?.connection_id ?? "none"}:${connection?.epoch ?? 0}`} scope={scope} text={item.text}
          connection={connection} now={new Date(item.at)} zone={item.zone} threadId={threadId}
          onConnect={() => consent.current?.querySelector("button")?.focus()}
          requestId={item.request_id} onDone={onDone} onUnavailable={onUnavailable} />
      )}
      <button type="button" onClick={onDismiss}>이 요청 닫기</button>
    </div>
  );
}
type Capability = {
  enabled: boolean;
  capability: string;
  tools: boolean;
  automatic_retry: boolean;
  work_interpret?: boolean;
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
  if (!drafts[thread] || drafts[thread] === drafts.new) {
    if (drafts.new) drafts[thread] = drafts.new;
    delete drafts.new;
  }
  return { ...memory, drafts, threadCreation: undefined };
}
function consumeDraft(memory: ChatMemory, thread: string, submitted: string): ChatMemory {
  if (memory.drafts[thread] !== submitted) return memory;
  const drafts = { ...memory.drafts };
  delete drafts[thread];
  return { ...memory, drafts };
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
  // A Calendar / mail-write consent return belongs to its original conversation, not to the connector catalog.
  const consentReturn = useRef(isMemberConsentReturn());
  const writeReturn = useState<MailWriteReturn | null>(() => mailWriteReturn(session.user.id, session.chat_draft_scope))[0];
  const consentThread = useRef(writeReturn?.thread ?? calendarReturnThread(session.user.id, session.chat_draft_scope));
  const [writeThread, setWriteThread] = useState<string | null>(writeReturn?.thread ?? null);
  const [writeHandle, setWriteHandle] = useState<string | null>(writeReturn?.handle ?? null);
  useEffect(() => {
    if (writeReturn) {
      settleMailWriteReturn();
      if (!writeReturn.handle) setError("Gmail 쓰기 권한 허용이 취소되었습니다. 초안은 그대로 남아 있습니다.");
    } else if (consentReturn.current && !consentThread.current) {
      // A consent return this tab did not start: it is never completed, and its fragment must not linger.
      window.history.replaceState(window.history.state, "", location.pathname + location.search);
    }
  }, []);
  const { route, open, leaveCatalog } = useMemberRoute(session, admin, () => {
    if (consentReturn.current) return null;
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
  const [briefRevision, setBriefRevision] = useState(0);
  const [monitorList, setMonitorList] = useState<{ thread: string; key: string } | null>(null);
  const [workNotice, setWorkNotice] = useState<WorkNotice | null>(null);
  const [workFocus, setWorkFocus] = useState<WorkFocusState | null>(null);
  useEffect(() => { setWorkNotice(null); }, [selected, session.user.id]);
  useEffect(() => { setWorkFocus(null); }, [session.user.id]);
  // Everyday member work: recovery of typed requests + opaque ids, the thread's outcomes and the receipt.
  const [memberWork, setMemberWork] = useState<MemberWork>(() => loadMemberWork(session.user.id, session.chat_draft_scope));
  const memberWorkRef = useRef(memberWork);
  memberWorkRef.current = memberWork;
  function changeWork(update: (work: MemberWork) => MemberWork) {
    const value = update(memberWorkRef.current);
    memberWorkRef.current = value;
    setMemberWork(value);
  }
  function patchWork(thread: string, update: (work: MemberThreadWork) => MemberThreadWork) {
    changeWork(all => {
      const current = all[thread] ?? {}, next = update(current);
      if (next === current) return all;
      const rest = { ...all };
      if (next.publicWatch || next.brief || next.calendar || next.mail || next.research?.length) rest[thread] = next; else delete rest[thread];
      return rest;
    });
  }
  useEffect(() => { saveMemberWork(session.user.id, session.chat_draft_scope, memberWork); }, [memberWork, session]);
  // A confirmed calendar write stays visible for this tab only; reload shows the calendar itself.
  const [finishedCalendar, setFinishedCalendar] = useState<Record<string, NonNullable<MemberThreadWork["calendar"]>>>({});
  const mailIdUsed = useRef(new Set<string>()), mailKeys = useRef<Record<string, string>>({});
  const [preference, setPreference] = useState<{ thread: string; view: MemberMemoryReceiptView } | null>(null);
  useEffect(() => { setPreference(null); }, [selected, session.user.id]);
  // The thread's outcomes are always read from the server (authorized GET, derived from the member's own sources),
  // so a comparison reopens on another device or tab; tab storage only remembers which ids this tab created.
  const researchOn = view === "chat" && !!selected;
  const outcomes = useResearchOutcomes({ accountId: session.user.id, thread: selected, enabled: researchOn, expired });
  const outcomesRef = useRef(outcomes);
  outcomesRef.current = outcomes;
  // Only a thread that actually has outcomes follows browser/mail changes with extra reads.
  const outcomeWork = researchOn && (outcomes.order.length > 0 || !!(selected && memberWork[selected]?.research?.length));
  const [autoOpen, setAutoOpen] = useState<{ thread: string; url: string; text: string } | null>(null);
  const linkOutcome = useRef<{ thread: string; id: string } | null>(null);
  const watching = view === "chat" && !!selected && watchThread === selected;
  const browser = usePersonalBrowser(session.user.id, view === "chat" ? selected : null, watching, expired,
    (thread) => { if (selectedRef.current === thread) void history(thread); });
  const browserWork = browser.status?.workspace ?? null;
  const acceptsGuidance = browserNeedsGuidance(browserWork);
  const preferredMode = browserChoice?.thread === selected ? browserChoice.mode : undefined;
  const browserMode = preferredMode === "chat" ? "chat"
    : acceptsGuidance ? "guide" : preferredMode === "start" ? "start" : "chat";
  // A page the member named for a research request is opened once through the existing browser start,
  // then, once it was read, the outcome links the current source (it never buys, submits or sends).
  function startBrowser(thread: string, url: string, text: string) {
    if (memoryRef.current.pending[thread]) return;
    if (browser.status?.configured === false) { setError("이 설치에서 브라우저 작업을 사용할 수 없습니다."); return; }
    const value: PendingSend = { request_id: crypto.randomUUID(), text, status: "sending",
      browser_request: { kind: "start", url, wait_for_user: false } };
    change(m => ({ ...m, pending: { ...m.pending, [thread]: value } }));
    void send({ replay: true });
  }
  useEffect(() => {
    if (!autoOpen || selected !== autoOpen.thread || view !== "chat" || mutating || loading) return;
    setAutoOpen(null);
    startBrowser(autoOpen.thread, autoOpen.url, autoOpen.text);
  }, [autoOpen, selected, view, mutating, loading]);
  const handoffRequest = useRef<{ thread: string; workspace: string; revision: number } | null>(null);
  useEffect(() => {
    const want = handoffRequest.current;
    if (!want || !watching || want.thread !== selected || !browserWork || browserWork.id !== want.workspace) return;
    handoffRequest.current = null;
    if (browserWork.revision === want.revision && !browser.controls && !browser.busy) browser.mutate({ kind: "takeover" });
  }, [watching, selected, browserWork?.id, browserWork?.revision]);
  // Closing or reopening another conversation drops everything that was projected for the previous one.
  useEffect(() => {
    if (!initialized.current) return;
    setWorkFocus(f => (f && f.thread === selected ? f : null));
    setMonitorList(m => (m && m.thread === selected ? m : null));
    setAutoOpen(a => (a && a.thread === selected ? a : null));
    setWatchThread(t => (t === selected ? t : null));
    setWriteThread(t => (t === selected ? t : null));
    if (linkOutcome.current && linkOutcome.current.thread !== selected) linkOutcome.current = null;
    if (handoffRequest.current && handoffRequest.current.thread !== selected) handoffRequest.current = null;
  }, [selected]);
  const workspaceKey = browserWork ? `${browserWork.id}:${browserWork.revision}:${browserWork.state}` : "";
  const lastWorkspace = useRef(workspaceKey);
  const pageRead = !!browserWork && ["ready", "completed", "user_control"].includes(browserWork.state);
  useEffect(() => {
    const previous = lastWorkspace.current;
    lastWorkspace.current = workspaceKey;
    if (!outcomeWork || !workspaceKey || workspaceKey === previous) return;
    const link = linkOutcome.current;
    if (link && link.thread === selected && pageRead) return; // the link effect below refreshes this one
    outcomesRef.current.refresh();
  }, [workspaceKey, outcomeWork, selected]);
  useEffect(() => {
    const link = linkOutcome.current;
    if (!link || link.thread !== selected || !pageRead) return;
    const outcome = outcomes.outcomes[link.id];
    if (!outcome) return; // wait until the thread's outcomes are loaded
    linkOutcome.current = null;
    void outcomes.act(outcome, "refresh");
  }, [workspaceKey, pageRead, outcomes.outcomes, selected]);
  const readKey = googleReads.filter(r => r.state === "completed").map(r => r.request_id).join(",");
  useEffect(() => {
    if (!outcomeWork || !readKey) return;
    const api = outcomesRef.current;
    const waiting = Object.values(api.outcomes).filter(o => o.next.kind === "read_mail");
    if (waiting.length) for (const o of waiting) void api.act(o, "refresh"); else api.refresh();
  }, [readKey, outcomeWork]);
  const pending = (selected ? memory.pending[selected] : undefined) as MemberPendingSend | undefined,
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
            result.threads.find((t) => t.id === consentThread.current)?.id ??
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
  function reviewMonitor(thread: string, value: MemberPendingSend, message: string) {
    const nextDraft = memoryRef.current.drafts[thread];
    clearSend(thread, value);
    if (!nextDraft) change(m => ({ ...m, drafts: { ...m.drafts, [thread]: value.text } }));
    if (selectedRef.current === thread) {
      setMonitorList({ thread, key: value.request_id });
      setError(nextDraft && nextDraft !== value.text ? `${message} 원래 요청은 “${value.text}”입니다.` : message);
    }
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
  async function createThread(proposedTitle: string, confirmPrevious = false) {
    setError("");
    const previous = memoryRef.current.threadCreation;
    if (previous && !confirmPrevious) {
      setError("먼저 이전 대화의 생성 결과를 확인해 주세요. 지금 작성한 내용은 보관했습니다.");
      return null;
    }
    const value: PendingThreadCreation = previous ?? {
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
        const refused = e instanceof ApiFailure && [400, 401, 403, 404, 413, 422, 429].includes(e.status);
        change(m => ({ ...m, threadCreation: refused ? undefined : { ...value, status: "uncertain" } }));
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
      await createThread(previous.title, true);
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
    if (resolution.kind === "server") return false; // positional reference: the server maps it
    const consume = () => change(m => consumeDraft(m, thread, text));
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
  async function reopenResult(thread: string, request: string, text: string) {
    try {
      const body = await scope.request<unknown>(`/threads/${encodeURIComponent(thread)}/work-interpret`, "POST",
        { request_id: request, text, read_only: true });
      if (selectedRef.current !== thread) return;
      const reply = parseInterpretation(body, thread);
      if (reply?.kind === "work") {
        work.merge(reply.group);
        setWorkFocus({ thread, groupId: reply.group.id, focus: reply.focus, taskId: reply.task_id });
      } else if (reply?.kind === "clarify") {
        setWorkNotice({ thread, kind: "clarify", text: reply.message, draft: "" });
      } else if (reply?.kind === "preference") {
        setPreference({ thread, view: reply.view });
      }
    } catch (e) { fail(e); }
  }
  async function locked<T>(run: () => Promise<T>): Promise<T | undefined> {
    if (mutation.current) return undefined;
    mutation.current = true;
    setMutating(true);
    try { return await run(); } finally { mutation.current = false; setMutating(false); }
  }
  /** The member's actual /personal/google/inbox read for this thread; without a connection the existing
   * consent flow continues the same request id and returns here. */
  async function readInbox(thread: string, text: string): Promise<"done" | "connect" | "unavailable" | "failed"> {
    if (memoryRef.current.pending[thread]) { setError("진행 중인 요청을 먼저 마쳐 주세요."); return "failed"; }
    const id = crypto.randomUUID();
    const hold = (message: string) => {
      change(m => ({ ...m, pending: { ...m.pending, [thread]: { request_id: id, text, status: "inbox" } } }));
      setError(message);
    };
    try {
      const found = await scope.request<{ connection: { connection_id: string; epoch: number } | null }>(
        "/personal/google", "GET", undefined, undefined, { timeoutMs: 15000 });
      if (!found.connection) {
        change(m => ({ ...m, pending: { ...m.pending, [thread]: { request_id: id, text, status: "inbox" } } }));
        openInboxRequest(thread, id);
        return "connect";
      }
      const read = await scope.request<ThreadGoogleRead>("/personal/google/inbox", "POST",
        { connection_id: found.connection.connection_id, epoch: found.connection.epoch, read: { thread_id: thread, request_id: id } },
        undefined, { timeoutMs: 75000 });
      if (read.request_id !== id || read.thread_id !== thread || read.state !== "completed") {
        hold("메일을 읽지 못했습니다. ‘메일 확인 계속하기’로 같은 요청을 이어갈 수 있습니다.");
        return "failed";
      }
      await mailHistory(thread);
      return "done";
    } catch (e) {
      if (isAborted(e)) return "failed";
      if (e instanceof ApiFailure && e.status === 401) { expired(); return "failed"; }
      if (e instanceof ApiFailure && e.status === 404) {
        setError("이 설치에서는 Gmail 읽기를 사용할 수 없습니다. 입력을 유지했습니다.");
        return "unavailable";
      }
      hold("메일 읽기 결과를 확인하지 못했습니다. ‘메일 확인 계속하기’로 같은 요청을 이어갈 수 있습니다.");
      return "failed";
    }
  }
  /** Calendar and mail-reply requests do work in the thread locally (nothing is saved or sent until the
   * member approves the exact final content); only what the member typed is kept for a reload. */
  async function startMemberWork(route: Extract<MemberRoute, { kind: "calendar" | "mail" }>, text: string) {
    if (route.kind === "calendar" && !route.parsed.ok) {
      setError(`${route.parsed.message} 입력을 유지했습니다.`);
      return;
    }
    if (selected && memberWorkRef.current[selected]?.[route.kind]) {
      setError(route.kind === "calendar"
        ? "진행 중인 캘린더 요청을 먼저 마치거나 닫아 주세요. 입력을 유지했습니다."
        : "진행 중인 메일 회신 요청을 먼저 마치거나 닫아 주세요. 입력을 유지했습니다.");
      return;
    }
    await locked(async () => {
      let thread = selected;
      if (!thread) {
        thread = await createThread(Array.from(text.trim()).slice(0, 40).join(""));
        if (!thread) return;
      }
      const destination = thread;
      setError("");
      const id = crypto.randomUUID();
      patchWork(destination, w => route.kind === "calendar"
        ? { ...w, calendar: { request_id: id, text, at: Date.now(), zone: localZone() } }
        : { ...w, mail: { request_id: id, text, ...(route.to ? { to: route.to } : {}), ...(route.body ? { body: route.body } : {}) } });
      change(m => consumeDraft(m, destination, text));
      if (route.kind === "mail" && !googleReads.some(r => r.state === "completed" && r.thread_id === destination)) {
        if (await readInbox(destination, text) === "unavailable") {
          patchWork(destination, w => ({ ...w, mail: undefined }));
          change(m => ({ ...m, drafts: { ...m.drafts, [destination]: m.drafts[destination] || text } }));
        }
      }
    });
  }
  function calendarUnavailable(thread: string) {
    const item = memberWorkRef.current[thread]?.calendar ?? finishedCalendar[thread];
    if (!item) return;
    patchWork(thread, w => ({ ...w, calendar: undefined }));
    change(m => ({ ...m, drafts: { ...m.drafts, [thread]: m.drafts[thread] || item.text } }));
    setError("이 설치에서는 Google 캘린더 기능을 사용할 수 없습니다. 입력한 내용은 입력창에 남겨 두었습니다. 관리자에게 문의해 주세요.");
  }
  function openOutcomeBrowser(url: string | null, outcomeId: string) {
    if (!selected) return;
    const thread = selected, href = browserLocation(url)?.href;
    if (!href) {
      setBrowserChoice({ thread, mode: "start" });
      requestAnimationFrame(() => chatEditor.current?.focus({ preventScroll: true }));
      return;
    }
    const target = outcomes.outcomes[outcomeId];
    if (target?.thread_id === thread) linkOutcome.current = { thread, id: target.id };
    startBrowser(thread, href, `브라우저로 열기: ${href}`);
  }
  /** "브라우저에서 이어서/직접 진행": the existing viewer opens on exactly the workspace the outcome names; for a takeover
   * the viewer's own takeover control runs once, for the same workspace revision, and only after this explicit click.
   * Resuming control back to the assistant stays the viewer's button, so the member decides when to hand over. */
  function handoffToBrowser(handoff: Handoff) {
    if (!selected) return;
    browser.refresh();
    if (!browserWork || browserWork.id !== handoff.workspace_id) {
      setError("이 비교가 가리키는 브라우저 작업이 지금은 열려 있지 않습니다. ‘다시 확인’으로 최신 상태를 읽어 주세요.");
      return;
    }
    setError("");
    if (handoff.action === "takeover") handoffRequest.current = { thread: selected, workspace: handoff.workspace_id, revision: handoff.revision };
    setWatchThread(selected);
  }
  function readOutcomeMail() {
    if (!selected) return;
    const thread = selected, target = Object.values(outcomes.outcomes).find(o => o.next.kind === "read_mail");
    void locked(() => readInbox(thread, (target?.message || target?.title || "메일 확인").slice(0, 2000)));
  }
  async function send({ replay = false, followUp, batch, plain = false }: {
    replay?: boolean; followUp?: RunInstructionReceipt; batch?: WorkItemInput[]; plain?: boolean;
  } = {}) {
    if (mutation.current || loading || (inventoryError && !initialized.current)) return;
    let thread = selected;
    const old = (thread ? memoryRef.current.pending[thread] : undefined) as MemberPendingSend | undefined;
    const text = batch ? compactMessage(batch) : followUp?.text ?? memoryRef.current.drafts[thread ?? "new"] ?? "";
    if (followUp && (followUp.thread_id !== thread || followUp.reason !== "text_follow_up_saved")) return;
    if (old && !replay) return;
    if (!old && (!text.trim() || new TextEncoder().encode(text).length > 8192))
      return;
    const routeWork = !old && !followUp && !batch && !plain && browserMode === "chat";
    if (routeWork && thread) {
      const control = parseWorkControl(text);
      if (control && runControl(control, text, thread)) return;
    }
    const direct = routeWork ? composerWork(text) : null;
    if (direct?.kind === "invalid") { setError(direct.message); return; }
    if (direct?.kind === "publicWatch" || direct?.kind === "connectedBrief") {
      await locked(async () => {
        const destination = thread ?? await createThread(Array.from(text.trim()).slice(0, 40).join(""));
        if (!destination) return;
        const current = memberWorkRef.current[destination];
        const isWatch = direct.kind === "publicWatch";
        const request = isWatch ? current?.publicWatch : current?.brief;
        if (request && request.text !== text) {
          setError(isWatch ? "이 대화에 접수 중인 감시가 있어요. 먼저 그 결과를 확인해 주세요."
            : "이 대화에 확인 중인 브리핑이 있어요. 먼저 시작하거나 닫아 주세요.");
          return;
        }
        setError("");
        patchWork(destination, w => isWatch
          ? { ...w, publicWatch: w.publicWatch ?? { request_id: crypto.randomUUID(), text } }
          : { ...w, brief: w.brief ?? { request_id: crypto.randomUUID(), text, zone: localZone() } });
        change(m => consumeDraft(m, destination, text));
      });
      return;
    }
    if (direct?.kind === "calendar" || direct?.kind === "mail") { await startMemberWork(direct, text); return; }
    const memberRequest: MemberPendingRequest | undefined = old?.member_request ?? (direct?.kind === "research"
      ? { kind: "research", outcome: direct.outcome, criteria: direct.criteria } : undefined);
    const id = old?.request_id ?? followUp?.request_id ?? crypto.randomUUID();
    let monitor = old?.monitor;
    if (old?.monitor_zone && thread) {
      // Previous snapshots only recorded the zone. A stop with no stored target must never pick a new one.
      const legacy = parseMonitorChat(old.text);
      if (!legacy.ok || legacy.intent.kind === "stop") {
        reviewMonitor(thread, old, "이전 감시 요청의 대상을 확인해 주세요. 현재 목록에서 감시를 선택할 수 있어요.");
        return;
      }
      monitor = monitorCommand(legacy.intent, id, old.monitor_zone);
    } else if (direct?.kind === "monitor") {
      monitor = monitorCommand(direct.intent, id, browserTimezone());
    }
    const intent = monitor || old || followUp || plain || memberRequest || browserMode === "guide" || preferredMode === "chat"
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
    const items = batch ?? (direct?.kind === "items" ? direct.items : undefined);
    const workRequest = old?.work_request ?? (!browserRequest && items ? { items } : undefined);
    const research = old?.research_request ?? (!browserRequest && direct?.kind === "search" ? direct.search : undefined);
    if (research && new TextEncoder().encode(research.query).length > 128) { setError("검색어를 128바이트 이내로 줄여 주세요."); return; }
    const textOnly = !monitor && !browserRequest && !research && !workRequest && !memberRequest && !followUp && !old?.follow_up_run_id;
    const interpretible = textOnly && (old
      ? old.text_route === "interpret" || (!old.text_route && (!!old.interpret_group || cap?.work_interpret === true))
      : !plain && browserMode === "chat" && cap?.work_interpret === true);
    if (!monitor && !browserRequest && !research && !workRequest && !memberRequest && !ready && !interpretible) {
      if (old) setError("답변을 받을 수 없습니다. 잠시 후 같은 요청으로 다시 확인해 주세요.");
      return;
    }
    if (!old && !workRequest && !memberRequest && currentPage?.turns.some((turn) => active(turn) && turn.run.id !== browserWork?.run_id)) return;
    let value: MemberPendingSend = old ?? {
      request_id: id, text, status: "sending",
      ...(textOnly ? { text_route: interpretible ? "interpret" : "plain" } : {}),
      ...(monitor ? { monitor } : {}),
      ...(interpretible && workFocus?.thread === thread && workFocus ? { interpret_group: workFocus.groupId } : {}),
      ...(followUp ? { follow_up_run_id: followUp.run_id } : {}),
      ...(research ? { research_request: research } : {}),
      ...(workRequest ? { work_request: workRequest } : {}),
      ...(memberRequest ? { member_request: memberRequest } : {}),
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
    function rememberSend() {
      change(m => ({ ...m, pending: { ...m.pending, [destination]: { ...value, status: "sending" } } }));
      // Record the original route/target before dispatch, including a thread created on this submit.
      recovery?.save(memoryRef.current, selectedRef.current ?? destination, [...new Set([...threads.map(t => t.id), destination])]);
    }
    if (!followUp && !old) change(m => consumeDraft(m, destination, value.text));
    rememberSend();
    try {
      if (monitor) {
        const notice = await submitMonitor(scope, monitor, destination, value.request_id, value.text, command => {
          value = { ...value, monitor: command };
          rememberSend();
        });
        if (notice && selectedRef.current === destination) setError(notice);
        clearSend(destination, value);
        if (selectedRef.current === destination) {
          setMonitorList({ thread: destination, key: value.request_id });
          void history(destination);
        }
        return;
      }
      if (value.member_request) {
        // One direct create: no extra menu. The pending carrier's id is the outcome id, so a reload or a
        // lost answer retries the same request and never duplicates the comparison.
        const request = value.member_request;
        let refused: unknown = null;
        const made = await outcomesRef.current.create(request.outcome, value.text, request.criteria,
          { requestId: value.request_id, thread: destination, onError: e => { refused = e; } });
        if (!made) {
          if (refused instanceof ApiFailure && refused.status === 401) { clearSend(destination, value, true); return; }
          if (refused instanceof ApiFailure && [400, 403, 404, 405, 422, 429, 501].includes(refused.status)) {
            clearSend(destination, value, true);
            setError([404, 405, 501].includes(refused.status)
              ? "이 설치에서는 비교·지원 준비 기능을 사용할 수 없습니다. 입력을 유지했습니다."
              : "요청을 처리하지 못했습니다. 입력을 유지했습니다.");
            return;
          }
          throw refused ?? new TypeError("research outcome unconfirmed"); // unknown outcome: same id on retry
        }
        patchWork(destination, w => ({ ...w, research: [...new Set([...(w.research ?? []), made.id])].slice(-8) }));
        clearSend(destination, value);
        void inventory().catch(fail);
        const opened = made.next.kind === "open_browser" ? browserLocation(made.next.url)?.href : undefined;
        const search = made.next.kind === "ask_url"
          ? startMemberPublicSearch(value.text, [session.user.name, session.user.email]) : null;
        if (search?.ok) {
          linkOutcome.current = { thread: destination, id: made.id };
          setAutoOpen({ thread: destination, url: search.url, text: "검색 결과에서 해당 판매처나 공고의 원문을 열어 확인해 줘." });
        } else if (opened && value.text.includes(new URL(opened).hostname)) {
          linkOutcome.current = { thread: destination, id: made.id };
          setAutoOpen({ thread: destination, url: opened, text: value.text });
        }
        return;
      }
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
      if (interpretible) {
        let decision: WorkInterpretation | null = null;
        let failed = "";
        try {
          decision = parseInterpretation(await scope.request<unknown>(
            `/threads/${encodeURIComponent(destination)}/work-interpret`, "POST",
            { request_id: value.request_id, text: value.text, ...(value.interpret_group ? { group_id: value.interpret_group } : {}),
              ...(old && !old.text_route && !old.interpret_group ? { read_only: true } : {}) }, undefined, { timeoutMs: 90000 }), destination);
          if (!decision) failed = "요청을 업무로 해석한 결과를 확인할 수 없습니다.";
        } catch (e) {
          if (isAborted(e)) throw e;
          if (e instanceof ApiFailure && interpretUnsupported(e.status)) decision = { kind: "chat" }; // old server
          else if (e instanceof ApiFailure && e.status === 401) { clearSend(destination, value, true); expired(); return; }
          else if (e instanceof ApiFailure && e.status >= 400 && e.status < 600) failed = "요청을 업무로 해석하지 못했습니다.";
          else throw e; // unknown outcome: keep the same request_id for a safe re-check
        }
        if (failed) {
          // Nothing was done. Restore the draft; sending it as plain chat stays the member's choice.
          clearSend(destination, value, true);
          if (selectedRef.current === destination)
            setWorkNotice({ thread: destination, kind: "clarify", text: `${failed} 입력을 유지했습니다.`, draft: value.text });
          return;
        }
        if (decision?.kind === "clarify") {
          clearSend(destination, value, true);
          if (selectedRef.current === destination) void history(destination);
          if (selectedRef.current === destination)
            setWorkNotice({ thread: destination, kind: "clarify", text: decision.message, draft: value.text });
          return;
        }
        if (decision?.kind === "preference") {
          // The typed receipt is shown from memory only; it is never written to tab storage.
          clearSend(destination, value);
          if (selectedRef.current === destination) {
            setWorkNotice(null);
            setPreference({ thread: destination, view: decision.view });
            void history(destination);
          }
          void inventory().catch(fail);
          return;
        }
        if (decision?.kind === "work") {
          work.merge(decision.group);
          clearSend(destination, value);
          if (selectedRef.current === destination) {
            setWorkNotice(null);
            setWorkFocus({ thread: destination, groupId: decision.group.id, focus: decision.focus, taskId: decision.task_id });
            void history(destination);
          }
          void inventory().catch(fail);
          return;
        }
        if (!ready) {
          clearSend(destination, value, true);
          setError("답변을 받을 수 없어 메시지를 보내지 못했습니다. 입력을 유지했습니다.");
          return;
        }
      }
      if (interpretible) {
        const { interpret_group: _, ...original } = value;
        value = { ...original, text_route: "plain" };
        rememberSend();
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
        if (monitor && e instanceof ApiFailure && e.status === 409) {
          reviewMonitor(destination, value, "감시 상태가 바뀌었어요. 최신 목록에서 확인해 주세요.");
          return;
        }
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
            change(m => ({ ...m, pending: { ...m.pending, [destination]: { request_id: value.request_id, text: value.text, status: "inbox" } } }));
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
    patchWork(thread, () => ({}));
    setFinishedCalendar(({ [thread]: _gone, ...rest }) => rest);
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
  const mailRequest = selected ? memberWork[selected]?.mail : undefined;
  if (selected && mailRequest) mailKeys.current[selected] = mailRequest.request_id;
  const calendarRequest = selected ? memberWork[selected]?.calendar ?? finishedCalendar[selected] : undefined;
  const showOutcomes = !!selected && researchOn && (outcomes.order.length > 0 || !!memberWork[selected]?.research?.length);
  const hasMemberWork = !!selected && (!!memberWork[selected]?.publicWatch || !!memberWork[selected]?.brief || !!memberWork[selected]?.calendar || !!memberWork[selected]?.mail || !!finishedCalendar[selected] ||
    showOutcomes || preference?.thread === selected);
  const isEmpty = !busy && !inventoryError && !error && turns.length === 0 && !pending && googleReads.length === 0 && !browserWork && !hasMemberWork;
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
  const publicWatchRequest = selected ? memberWork[selected]?.publicWatch : undefined;
  const publicWatchIntent = publicWatchRequest && parsePublicWatchChat(publicWatchRequest.text);
  const briefRequest = selected ? memberWork[selected]?.brief : undefined;
  const briefIntent = briefRequest && parseConnectedBriefChat(briefRequest.text);
  const hasWork = Object.keys(work.groups).length > 0;
  // Work needs no model, so these drafts stay sendable while the chat engine is unavailable.
  const draftWorkAllowed = useMemo(() => browserMode === "chat" && (composerWork(draft) !== null ||
    (hasWork && parseWorkControl(draft) !== null) || (preferredMode !== "chat" && !!browserIntent(draft))),
    [draft, browserMode, hasWork, preferredMode]);
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
            <><PersonalTasks accountId={session.user.id} onExpired={expired} taskId={route.taskId}
              onOpenTask={taskId => open({ view: "tasks", taskId })}
              onBack={() => open({ view: "tasks" }, true)}
              onThread={(thread, run) => {
                openThread(thread);
                if (run) taskTurn.current = { thread, run };
              }} />
              {!route.taskId && <MemberWorkMonitors accountId={session.user.id} onExpired={expired} onOpenRun={id => open({ view: "tasks", taskId: id })} />}</>
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
                            <button type="button" disabled={composerBusy || !ready || browserMode !== "chat"} onClick={() => void send({ followUp: r })}>후속 메시지로 보내기</button>}
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
                    {turn.run.model?.provider_id === "member-work-result" && turn.run.request_id && selected &&
                      <button type="button" className="member-task-link" onClick={() => void reopenResult(selected, turn.run.request_id!, turn.user_text)}>결과 다시 보기</button>}
                    {turn.run.model?.provider_id === "member-linked-work" && (
                      // Calendar work linked by prepare(thread_id): the record lives on the server, never replayed from here.
                      <button className="member-task-link" type="button" onClick={() => open({ view: "tasks", taskId: turn.run.id })}>업무 기록 보기</button>
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
                            isDisabled={(!pending.browser_request && !pending.research_request && !pending.work_request && !pending.member_request && !pending.monitor && !pending.monitor_zone && pending.text_route !== "interpret" && !ready) || mutating || busy}
                            onClick={() => void send({ replay: true })} />
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
                            onClick={() => void send({ replay: true })} />
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
                            setWorkNotice(null); void send({ plain: true });
                          }}>일반 메시지로 보내기</button>
                        )}
                        <button type="button" onClick={() => setWorkNotice(null)}>닫기</button>
                      </div>
                    </div>
                  ))}
                {selected && publicWatchRequest && publicWatchIntent?.ok && <PublicWatchStart accountId={session.user.id}
                  threadId={selected} intent={publicWatchIntent.intent} requestId={publicWatchRequest.request_id} onExpired={expired}
                  onCancel={() => patchWork(selected, w => ({ ...w, publicWatch: undefined }))}
                  onDone={() => {
                    patchWork(selected, w => ({ ...w, publicWatch: undefined }));
                    void history(selected);
                    void inventory().catch(fail);
                  }} />}
                {selected && currentPage?.turns.some(t => t.run.model?.provider_id === "member-public-watch") &&
                  <PublicWatchLog accountId={session.user.id} threadId={selected} onExpired={expired} />}
                {selected && briefRequest && briefIntent?.ok && (
                  <div className="member-chat-notice">
                    <p className="member-request-echo">{briefRequest.text}</p>
                    <ConnectedBriefCard accountId={session.user.id} threadId={selected} intent={briefIntent.intent}
                      request={briefRequest} onExpired={expired} onConnect={() => open({ view: "connections" })}
                      onCancel={() => patchWork(selected, w => ({ ...w, brief: undefined }))}
                      onDone={() => {
                        patchWork(selected, w => ({ ...w, brief: undefined }));
                        setBriefRevision(n => n + 1);
                        void history(selected);
                      }} />
                  </div>
                )}
                {selected && currentPage?.turns.some(t => t.run.model?.provider_id === "member-connected-brief") && <ConnectedBriefLog accountId={session.user.id} threadId={selected} onExpired={expired} refreshKey={String(briefRevision)} />}
                {monitorList?.thread === selected && monitorList && <MemberWorkMonitors accountId={session.user.id} onExpired={expired}
                  refreshKey={monitorList.key} onOpenRun={id => open({ view: "tasks", taskId: id })} />}
                {workFocus && workFocus.thread === selected && (
                  <WorkFocus state={workFocus} group={work.groups[workFocus.groupId]} work={work}
                    openRun={(runId) => open({ view: "tasks", taskId: runId })} onClose={() => setWorkFocus(null)} />
                )}
                {mailError && <p className="member-chat-status" role="status">{mailError}</p>}
                {googleReads.length > 0 && <GoogleReadHistory reads={googleReads} />}
                {selected && mailRequest && (
                  <div className="member-message member-request-echo">
                    <span className="sr-only">나</span>
                    <p>{mailRequest.text}</p>
                    <button type="button" onClick={() => patchWork(selected, w => ({ ...w, mail: undefined }))}>이 요청 닫기</button>
                  </div>
                )}
                {googleReads.length > 0 && selected && <MemberMailWork key={`${selected}:${mailKeys.current[selected] ?? ""}`} threadId={selected}
                  read={googleReads.find(read => read.state === "completed") ?? null}
                  request={scope.request} fail={fail} disabled={composerBusy}
                  seed={mailRequest ? { text: mailRequest.text, to: mailRequest.to ?? "", body: mailRequest.body ?? "", message_id: mailRequest.message_id } : undefined}
                  newId={() => {
                    const id = mailRequest?.request_id;
                    if (id && !mailIdUsed.current.has(id)) { mailIdUsed.current.add(id); return id; }
                    return crypto.randomUUID();
                  }}
                  onPrepared={() => patchWork(selected, w => ({ ...w, mail: undefined }))}
                  onForm={form => patchWork(selected, w => {
                    const m = w.mail;
                    if (!m || (m.to ?? "") === form.to && (m.body ?? "") === form.body && (m.message_id ?? "") === form.message_id) return w;
                    return { ...w, mail: { request_id: m.request_id, text: m.text, ...(form.to ? { to: form.to } : {}),
                      ...(form.body ? { body: form.body } : {}), ...(form.message_id ? { message_id: form.message_id } : {}) } };
                  })}
                  onConnect={() => { setWriteHandle(null); setWriteThread(selected); }} />}
                {selected && writeThread === selected && (
                  <MemberMailWriteConsent request={scope.request} callbackHandle={writeHandle} fail={fail} disabled={composerBusy}
                    onChange={state => { if (state.state === "connected") settleMailWriteReturn(); }}
                    navigate={url => { rememberMailWriteConsent(session.user.id, session.chat_draft_scope, selected); window.location.assign(url); }} />
                )}
                {preference && preference.thread === selected && (
                  <div className="member-chat-notice">
                    <MemberMemoryReceipt view={preference.view} onOpenMemories={() => open({ view: "memory" })} />
                    <button type="button" onClick={() => setPreference(null)}>닫기</button>
                  </div>
                )}
                {selected && showOutcomes && (
                  <MemberResearchOutcomes api={outcomes} onOpenBrowser={openOutcomeBrowser}
                    onBrowser={handoffToBrowser} onReadMail={readOutcomeMail} />
                )}
                {selected && calendarRequest && (
                  <CalendarChat key={`${selected}:${calendarRequest.request_id}`} scope={scope} accountId={session.user.id}
                    sessionScope={session.chat_draft_scope} threadId={selected} item={calendarRequest} fail={fail}
                    onDone={() => {
                      setFinishedCalendar(all => ({ ...all, [selected]: calendarRequest }));
                      patchWork(selected, w => ({ ...w, calendar: undefined }));
                    }}
                    onUnavailable={() => calendarUnavailable(selected)}
                    onDismiss={() => {
                      patchWork(selected, w => ({ ...w, calendar: undefined }));
                      setFinishedCalendar(({ [selected]: _gone, ...rest }) => rest);
                    }} />
                )}
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
                onSubmit={(items) => void send({ batch: items })} />
            </div>
          )}
          {!loading && (
            <PersonalChatComposer key={selected ?? "new"} editor={chatEditor} draft={draft}
              followUp={selected && browserMode === "chat" && <PersonalTextFollowUp key={selected}
                thread={selected} scope={scope} draft={draft} run={working?.run.accepts_text_follow_up ? working.run : undefined}
                pending={memory.instructions?.[selected]?.pending} rejected={memory.instructions?.[selected]?.draft} change={change} expired={expired}
                refresh={() => { if (selectedRef.current === selected) void history(selected); }} />}
              disabled={inventoryError && !initialized.current}
              sendUnavailable={!ready && browserMode === "chat" && !working?.run.accepts_text_follow_up && !draftWorkAllowed} busy={composerBusy}
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
