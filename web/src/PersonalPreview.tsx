import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import type {
  ChatPage,
  ChatTurn,
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
import PersonalGoogle, {
  clearPersonalGoogle,
  googleReturn,
} from "./PersonalGoogle";
import PersonalAdmin from "./PersonalAdmin";
import { PersonalMemories } from "./PersonalMemories";
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
      <img src="/moosuri-icon-192.png" alt="" width="32" height="32" />
      Moosuri <small>텍스트 미리보기</small>
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
    clearPersonalGoogle();
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
  return (
    <ScopeRegistry.Provider value={scopes}>
      <div className="preview-shell">
        <header className="preview-header">
          <Brand />
          {session && (
            <div>
              <span>{session.user.name}</span>
              <button onClick={() => void signOut()}>로그아웃</button>
            </div>
          )}
        </header>
        <main>
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
          {phase === "ready" &&
            session &&
            (session.password_change_required ||
            session.user.must_change_password ? (
              <Credentials
                key={`password-${epoch}`}
                session={session}
                done={(v) => {
                  signedIn(v);
                  announce();
                }}
                expired={expired}
              />
            ) : (
              <Workspace
                key={`${session.user.id}-${epoch}`}
                session={session}
                recovery={draftSession}
                expired={expired}
                changedSession={signedIn}
              />
            ))}
        </main>
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
}: {
  session?: SessionView;
  done: (v: SessionView) => void;
  expired?: () => void;
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
      <p>
        {session
          ? "개인 대화를 시작하기 전에 새 비밀번호를 정해 주세요."
          : "설치자가 초대한 계정으로 로그인하세요."}
      </p>
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
    </section>
  );
}
type Capability = {
  enabled: boolean;
  capability: string;
  model: string;
  tools: boolean;
  automatic_retry: boolean;
};
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
}: {
  session: SessionView;
  recovery: ChatDraftSession | null;
  expired: () => void;
  changedSession: (v: SessionView) => void;
}) {
  const scope = useScope(session.user.id),
    [threads, setThreads] = useState<ThreadSummary[]>([]),
    [selected, setSelected] = useState<string | null>(null),
    [memory, setMemory] = useState<ChatMemory>({ drafts: {}, pending: {} }),
    [page, setPage] = useState<ChatPage | null>(null),
    [cap, setCap] = useState<Capability | null>(null),
    [loading, setLoading] = useState(true),
    [error, setError] = useState(""),
    [reading, setReading] = useState(false),
    [mutating, setMutating] = useState(false),
    [title, setTitle] = useState("새 텍스트 대화");
  const [view, setView] = useState<"chat" | "memory" | "mail" | "admin">(() =>
      googleReturn() ? "mail" : "chat",
    ),
    [refresh, setRefresh] = useState(0);
  const [openInbox, setOpenInbox] = useState(false);
  const [admin, setAdmin] = useState(session.installation_admin === true);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const memoryRef = useRef(memory);
  memoryRef.current = memory;
  const initialized = useRef(false),
    mutation = useRef(false),
    historyVersion = useRef(0);
  const pending = selected ? memory.pending[selected] : undefined,
    draft = memory.drafts[selected ?? "new"] ?? "",
    ready =
      cap?.enabled &&
      cap.capability === "text_only_preview" &&
      cap.tools === false;
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
  async function inventory() {
    const result = await scope.request<ThreadsResponse>("/threads");
    setThreads(result.threads);
    return result.threads;
  }
  useEffect(() => {
    let live = true;
    setLoading(true);
    setError("");
    void Promise.all([
      scope.request<ThreadsResponse>("/threads"),
      scope.request<Capability>("/text-preview").catch((e) => {
        if (e instanceof ApiFailure && e.status === 404) return null;
        throw e;
      }),
    ])
      .then(([result, cap]) => {
        if (!live) return;
        setThreads(result.threads);
        setCap(cap);
        if (!initialized.current) {
          const restored = recovery?.load(result.threads.map((t) => t.id));
          const created =
            restored?.memory.threadCreation &&
            result.threads.find(
              (t) => t.id === restored.memory.threadCreation!.request_id,
            );
          if (restored) {
            const value = created
              ? moveNewDraft(restored.memory, created.id)
              : restored.memory;
            setMemory(value);
            memoryRef.current = value;
          }
          setSelected(
            created?.id ?? restored?.selected ?? result.threads[0]?.id ?? null,
          );
          initialized.current = true;
        }
      })
      .catch((e) => {
        if (live) fail(e);
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
    const check = () => {
      void scope
        .request<SessionView>("/auth/session")
        .then((v) => {
          if (
            v.user.id !== session.user.id ||
            v.chat_draft_scope !== session.chat_draft_scope ||
            v.password_change_required !== session.password_change_required ||
            v.installation_admin !== session.installation_admin
          )
            changedSession(v);
        })
        .catch(fail);
    };
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  }, [scope, session]);
  function reconcile(thread: string, turns: ChatTurn[]) {
    const value = memoryRef.current.pending[thread];
    if (!value) return;
    const found = turns.find((t) => t.run.request_id === value.request_id);
    if (found && !active(found))
      change((m) => {
        const pending = { ...m.pending },
          drafts = { ...m.drafts };
        delete pending[thread];
        if (drafts[thread] === value.text) delete drafts[thread];
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
  useEffect(() => {
    ++historyVersion.current;
    setPage(null);
    setError("");
    if (selected) void history(selected);
  }, [selected, scope, refresh]);
  useEffect(() => {
    if (!selected || reading || !page?.turns.some(active)) return;
    const timer = window.setInterval(() => void history(selected), 2000);
    return () => window.clearInterval(timer);
  }, [selected, page?.turns.some(active), reading, scope]);
  async function createThread(proposedTitle: string) {
    setError("");
    const value: PendingThreadCreation = memoryRef.current.threadCreation ?? {
      request_id: crypto.randomUUID(),
      title: proposedTitle.trim() || "새 텍스트 대화",
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
  async function create() {
    if (mutation.current) return;
    mutation.current = true;
    setMutating(true);
    try {
      await createThread(title);
    } finally {
      mutation.current = false;
      setMutating(false);
    }
  }
  async function send(event?: FormEvent, replay = false) {
    event?.preventDefault();
    if (!ready || mutation.current || loading) return;
    let thread = selected;
    const old = thread ? memoryRef.current.pending[thread] : undefined;
    const text = memoryRef.current.drafts[thread ?? "new"] ?? "";
    if (old && !replay) return;
    if (!old && page?.turns.some(active)) return;
    if (!old && (!text.trim() || new TextEncoder().encode(text).length > 8192))
      return;
    const value: PendingSend = old ?? {
      request_id: crypto.randomUUID(),
      text,
      status: "sending",
    };
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
    change((m) => ({
      ...m,
      pending: { ...m.pending, [destination]: { ...value, status: "sending" } },
    }));
    try {
      const turn = await scope.request<ChatTurn>(
        `/threads/${encodeURIComponent(thread)}/text-preview`,
        "POST",
        {
          request_id: value.request_id,
          text: value.text,
          capability: "text_only_preview",
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
      void inventory().catch(fail);
    } catch (e) {
      if (!isAborted(e)) {
        if (
          e instanceof ApiFailure &&
          [400, 401, 403, 404, 422, 429].includes(e.status)
        ) {
          change((m) => {
            const pending = { ...m.pending };
            delete pending[destination];
            return { ...m, pending };
          });
          if (e.status === 422 && e.code === "personal_inbox_requested") {
            setOpenInbox(true);
            setView("mail");
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
  async function remove() {
    if (!selected || mutation.current) return;
    const thread = selected;
    if (!window.confirm("이 대화를 삭제할까요?")) return;
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
      change((m) => {
        const drafts = { ...m.drafts },
          pending = { ...m.pending };
        delete drafts[thread];
        delete pending[thread];
        return { ...m, drafts, pending };
      });
      const remaining = await inventory();
      setSelected(remaining[0]?.id ?? null);
    } catch (e) {
      fail(e);
    } finally {
      mutation.current = false;
      setMutating(false);
    }
  }
  return (
    <>
      <nav className="preview-view-nav" aria-label="개인 공간">
        <button aria-pressed={view === "chat"} onClick={() => setView("chat")}>
          대화
        </button>
        <button
          aria-pressed={view === "memory"}
          onClick={() => setView("memory")}
        >
          기억
        </button>
        <button
          aria-pressed={view === "mail"}
          onClick={() => {
            setOpenInbox(false);
            setView("mail");
          }}
        >
          메일
        </button>
        <button
          disabled={mutating || loading}
          onClick={() => setRefresh((v) => v + 1)}
        >
          연결 새로고침
        </button>
        {admin && (
          <button
            aria-pressed={view === "admin"}
            onClick={() => setView("admin")}
          >
            관리
          </button>
        )}
      </nav>
      {view === "admin" && admin ? (
        <PersonalAdmin
          account={session.user.id}
          expired={expired}
          unavailable={() => {
            setAdmin(false);
            setView("chat");
          }}
        />
      ) : view === "mail" ? (
        <PersonalGoogle
          session={session}
          expired={expired}
          readOnOpen={openInbox}
          consumed={() => setOpenInbox(false)}
        />
      ) : view === "memory" ? (
        <section className="preview-memory-panel">
          <p className="preview-memory-note">
            직접 저장한 기억을 편집하고 수정 이력을 확인할 수 있습니다.
          </p>
          <PersonalMemories accountId={session.user.id} onExpired={expired} />
        </section>
      ) : (
        <div className="preview-workspace">
          <aside aria-label="대화 목록">
            <h1>내 대화</h1>
            <label>
              새 대화 제목
              <input
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                maxLength={200}
                disabled={mutating || !!memory.threadCreation}
              />
            </label>
            <button onClick={() => void create()} disabled={mutating}>
              {memory.threadCreation
                ? "같은 대화 만들기 다시 확인"
                : "대화 만들기"}
            </button>
            <ul>
              {threads.map((t) => (
                <li key={t.id}>
                  <button
                    aria-current={selected === t.id ? "page" : undefined}
                    onClick={() => setSelected(t.id)}
                  >
                    {t.title}
                  </button>
                </li>
              ))}
            </ul>
            {loading && <p role="status">대화 확인 중…</p>}
          </aside>
          <section className="preview-conversation" aria-label="텍스트 대화">
            <div className="preview-capability">
              <strong>텍스트 미리보기</strong>
              <p>
                메일 조회·발송이나 예약이 필요한 작업은 현재 운영 비서에서
                이용하세요.
              </p>
              {cap && <span>모델: {cap.model}</span>}
              {!loading && !ready && (
                <p>이 설치에 텍스트 미리보기가 연결되어 있지 않습니다.</p>
              )}
            </div>
            {error && (
              <p role="alert" className="preview-error">
                {error}
              </p>
            )}
            {selected ? (
              <>
                <header className="preview-thread-header">
                  <h2>
                    {threads.find((t) => t.id === selected)?.title ?? "대화"}
                  </h2>
                  <button disabled={mutating} onClick={() => void remove()}>
                    대화 삭제
                  </button>
                </header>
                <div className="preview-turns" aria-live="polite">
                  {page?.next_before && (
                    <button
                      disabled={reading}
                      onClick={() =>
                        void history(selected, page.next_before ?? undefined)
                      }
                    >
                      이전 메시지
                    </button>
                  )}
                  {reading && !page && <p role="status">대화 불러오는 중…</p>}
                  {page?.turns.map((turn) => (
                    <article key={turn.run.id}>
                      <div className="preview-user">
                        <strong>나</strong>
                        <p>{turn.user_text}</p>
                      </div>
                      <div className="preview-assistant">
                        <strong>무수리 · 텍스트</strong>
                        {turn.assistant.map((m) => (
                          <p key={m.id} className="preview-output">
                            {m.text}
                          </p>
                        ))}
                        <span className="preview-run-state">
                          {stateLabel(turn)}
                        </span>
                      </div>
                    </article>
                  ))}
                </div>
                {pending && (
                  <div className="preview-pending" role="status">
                    <p>
                      {pending.status === "sending"
                        ? "답변을 기다리는 중…"
                        : pending.status === "accepted"
                          ? "저장된 요청의 답변을 확인하고 있습니다."
                          : "아직 전송 결과를 확인하지 못했습니다. 새 요청은 만들지 않았습니다."}
                    </p>
                    {pending.status !== "sending" && (
                      <>
                        <button
                          disabled={reading || mutating}
                          onClick={() => void history(selected)}
                        >
                          결과 확인
                        </button>
                        <button
                          disabled={mutating}
                          onClick={() => void send(undefined, true)}
                        >
                          동일 요청 다시 확인
                        </button>
                      </>
                    )}
                  </div>
                )}
              </>
            ) : (
              !loading && <h2>내 텍스트 대화를 시작하세요</h2>
            )}
            {!loading && (
              <form className="preview-composer" onSubmit={(e) => void send(e)}>
                <label htmlFor="preview-message">메시지</label>
                <textarea
                  id="preview-message"
                  value={draft}
                  onChange={(e) =>
                    change((m) => ({
                      ...m,
                      drafts: {
                        ...m.drafts,
                        [selected ?? "new"]: e.target.value,
                      },
                    }))
                  }
                  disabled={
                    !ready || loading || !!pending || page?.turns.some(active)
                  }
                  maxLength={8192}
                  rows={4}
                />
                {new TextEncoder().encode(draft).length > 8192 && (
                  <p role="alert">메시지가 너무 깁니다. 내용을 줄여 주세요.</p>
                )}
                <button
                  disabled={
                    !ready ||
                    mutating ||
                    !!pending ||
                    page?.turns.some(active) ||
                    !draft.trim() ||
                    new TextEncoder().encode(draft).length > 8192
                  }
                >
                  보내기
                </button>
              </form>
            )}
          </section>
        </div>
      )}
    </>
  );
}
function stateLabel(turn: ChatTurn) {
  return {
    queued: "대기 중",
    running: "답변 작성 중",
    completed: "답변 완료",
    failed: "응답 실패",
    cancelled: "취소됨",
    incomplete: "일부 응답 · 자동 재시도 없음",
    refused: "응답 거절",
    outcome_unknown: "응답 확인 불가 · 자동 재시도 없음",
  }[turn.run.state];
}
