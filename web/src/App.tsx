import {activatePushAccount,clearBrowserPushBinding} from "./push-binding";
import {
  useEffect,
  useLayoutEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import type {
  AccountSummary,
  ManagedAccount,
  ChatEvidence,
  SessionView,
  SystemStatus,
  ThreadSummary,
  ThreadsResponse,
  UsersResponse,
} from "../shared/api-types";
import { AccountControls } from "./AccountControls";
import { ApiFailure, errorText, isAborted, RequestScope } from "./api";
import { ScopeRegistry, useScope } from "./account-scope";
import { ChatPanel, type ChatMemory } from "./ChatPanel";
import { PwaInstallButton } from "./PwaInstallButton";
import { MailConnections } from "./MailConnections";
import { GoogleSetup } from "./GoogleSetup";
import { Artifacts, type ArtifactDrafts } from "./Artifacts";
import { CommunitySkills } from "./CommunitySkills";
import { Schedules } from "./Schedules";
import { SourceConnections } from "./SourceConnections";
import { UsageView } from "./UsageView";
import { KnowledgeView } from "./KnowledgeView";
import { initialKnowledgeDrafts } from "./KnowledgeState";
import {
  AdminProviderSettings,
  PersonalModelSettings,
} from "./ProviderSettings";

function Mark({ large = false }: { large?: boolean }) {
  return (
    <span className={`mark ${large ? "mark-large" : ""}`} aria-hidden="true">
      <svg viewBox="0 0 40 40">
        <path d="M9 28V17c0-8 11-8 11 0v11m0-11c0-8 11-8 11 0v11" />
      </svg>
    </span>
  );
}
function Brand() {
  return (
    <span className="brand">
      <Mark />
      moosuri<span className="brand-dot">.</span>
    </span>
  );
}
function Notice({ children }: { children: ReactNode }) {
  return (
    <p className="notice" role="alert">
      {children}
    </p>
  );
}
function Gate({ children }: { children: ReactNode }) {
  return (
    <div className="gate">
      <header>
        <Brand />
        <span className="quiet-label">YOUR OWN SPACE</span>
      </header>
      <main className="gate-main">{children}</main>
      <footer>Self-hosted. A little more yours.</footer>
    </div>
  );
}
function Modal({
  title,
  children,
  close,
  returnFocus,
}: {
  title: string;
  children: ReactNode;
  close: () => void;
  returnFocus?: HTMLElement | null;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const element = ref.current!;
    const previous =
      returnFocus ?? (document.activeElement as HTMLElement | null);
    element.showModal();
    return () => {
      element.close();
      if (previous?.isConnected) previous.focus();
    };
  }, [returnFocus]);
  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
      aria-labelledby={titleId}
    >
      <div className="dialog-top">
        <h2 id={titleId}>{title}</h2>
        <button
          type="button"
          className="icon-button"
          onClick={close}
          aria-label="Close dialog"
        >
          ×
        </button>
      </div>
      {children}
    </dialog>
  );
}

export default function App() {
  const scopes = useMemo(() => new Set<RequestScope>(), []);
  return (
    <ScopeRegistry.Provider value={scopes}>
      <Application scopes={scopes} />
    </ScopeRegistry.Provider>
  );
}

function Application({ scopes }: { scopes: Set<RequestScope> }) {
  const [system, setSystem] = useState<SystemStatus | null>(null);
  const [session, setSession] = useState<SessionView | null>(null);
  const [epoch, setEpoch] = useState(0);
  const [boot, setBoot] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [bootFailed, setBootFailed] = useState(false);
  const [loggingOut, setLoggingOut] = useState(false);
  const logoutScope = useRef<RequestScope | null>(null);
  const bootScope = useRef<RequestScope | null>(null);
  const focusScope = useRef<RequestScope | null>(null);
  const authMutation = useRef<symbol | null>(null);
  const logoutIdentity = useRef<string | null>(null);
  const authChannel = useRef<BroadcastChannel | null>(null);
  const revalidateRef = useRef<() => void>(() => {});
  const checkSessionRef = useRef<() => void>(() => {});
  const [path, setPath] = useState(window.location.pathname);
  useEffect(() => {
    const listener = () => setPath(window.location.pathname);
    window.addEventListener("popstate", listener);
    return () => window.removeEventListener("popstate", listener);
  }, []);
  useEffect(
    () => () => {
      logoutScope.current?.close();
      bootScope.current?.close();
      focusScope.current?.close();
    },
    [],
  );
  function clearLifetimes() {
    for (const scope of scopes) scope.close();
    bootScope.current?.close();
    focusScope.current?.close();
    focusScope.current = null;
    logoutScope.current?.close();
    logoutScope.current = null;
  }
  useEffect(()=>{if(session)void activatePushAccount(session.user.id).catch(()=>{});},[session?.user.id]);
  useEffect(()=>{const listener=(event:MessageEvent)=>{if(event.data?.type==="moosuri-open-schedules"&&session)navigate("/app/schedules");};navigator.serviceWorker?.addEventListener("message",listener);return()=>navigator.serviceWorker?.removeEventListener("message",listener);},[session?.user.id]);
  function revalidate() {
    if(session)void clearBrowserPushBinding(session.user.id);
    // Synchronously fence every old response before React unmounts its UI.
    clearLifetimes();
    authMutation.current = null;
    setSession(null);
    setEpoch((value) => value + 1);
    setLoading(true);
    setLoggingOut(false);
    setBoot((value) => value + 1);
  }
  revalidateRef.current = revalidate;
  function beginMutation() {
    const owner = Symbol();
    authMutation.current = owner;
    focusScope.current?.close();
    focusScope.current = null;
    return () => {
      if (authMutation.current === owner) authMutation.current = null;
    };
  }
  checkSessionRef.current = () => {
    // Passive checks preserve a known session on network failure and cannot
    // interrupt a local cookie mutation whose response has not arrived yet.
    if (!session || loggingOut || authMutation.current || focusScope.current)
      return;
    const scope = new RequestScope();
    focusScope.current = scope;
    void scope
      .request<SessionView>("/auth/session")
      .then((current) => {
        if (
          current.user.id !== session.user.id ||
          current.user.role !== session.user.role ||
          current.password_change_required !==
            session.password_change_required ||
          current.user.must_change_password !==
            session.user.must_change_password
        )
          revalidateRef.current();
      })
      .catch((error) => {
        if (error instanceof ApiFailure && error.status === 401)
          revalidateRef.current();
      })
      .finally(() => {
        if (focusScope.current === scope) focusScope.current = null;
      });
  };
  useEffect(() => {
    if (typeof BroadcastChannel !== "undefined") {
      try {
        const channel = new BroadcastChannel("moosuri-auth");
        authChannel.current = channel;
        channel.onmessage = (event) => {
          if (event.data?.type === "auth-changed") revalidateRef.current();
        };
      } catch {
        /* The account assertion still protects requests without messaging. */
      }
    }
    const check = () => {
      if (document.visibilityState === "visible") checkSessionRef.current();
    };
    window.addEventListener("focus", check);
    document.addEventListener("visibilitychange", check);
    return () => {
      authChannel.current?.close();
      authChannel.current = null;
      window.removeEventListener("focus", check);
      document.removeEventListener("visibilitychange", check);
    };
  }, []);
  function announceTransition() {
    try {
      authChannel.current?.postMessage({ type: "auth-changed" });
    } catch {
      /* Optional cross-tab hint. */
    }
  }
  function navigate(next: string) {
    window.history.pushState(null, "", next);
    setPath(next);
  }
  useEffect(() => {
    const scope = new RequestScope();
    bootScope.current = scope;
    setLoading(true);
    setError("");
    setBootFailed(false);
    void (async () => {
      try {
        const status = await scope.request<SystemStatus>("/system");
        setSystem(status);
        if (status.initialized) {
          try {
            const current = await scope.request<SessionView>("/auth/session");
            logoutIdentity.current = null;
            setSession(current);
          } catch (e) {
            if (!(e instanceof ApiFailure && e.status === 401)) throw e;
            void clearBrowserPushBinding();
            setSession(null);
          }
        }
        setLoading(false);
      } catch (e) {
        if (!isAborted(e)) {
          setError(errorText(e));
          setBootFailed(true);
          setLoading(false);
        }
      }
    })();
    return () => scope.close();
  }, [boot]);
  function adopt(next: SessionView) {
    logoutIdentity.current = null;
    setEpoch((value) => value + 1);
    setSession(next);
    setError("");
    announceTransition();
  }
  function expire() {
    revalidate();
  }
  useEffect(() => {
    const reconnect = () => {
      if (bootFailed || loading) revalidateRef.current();
      else checkSessionRef.current();
    };
    window.addEventListener("online", reconnect);
    return () => window.removeEventListener("online", reconnect);
  }, [bootFailed, loading]);
  async function logout() {
    const expectedAccount = session?.user.id ?? logoutIdentity.current;
    if (!expectedAccount || logoutScope.current) return;
    void clearBrowserPushBinding(expectedAccount);
    logoutIdentity.current = expectedAccount;
    focusScope.current?.close();
    focusScope.current = null;
    for (const privateScope of scopes) privateScope.close();
    setSession(null);
    setEpoch((value) => value + 1);
    setLoggingOut(true);
    setError("");
    navigate("/app");
    const scope = new RequestScope(expectedAccount);
    logoutScope.current = scope;
    try {
      await scope.request("/auth/logout", "POST", {});
      logoutIdentity.current = null;
      announceTransition();
    } catch (e) {
      if (e instanceof ApiFailure && e.status === 401) {
        revalidate();
        return;
      }
      if (!isAborted(e)) {
        setError(
          "Sign-out could not be confirmed by the server. Retry sign-out before using a shared device.",
        );
        // A lost reply does not prove the cookie transition failed.
        announceTransition();
      }
    } finally {
      if (logoutScope.current === scope) {
        logoutScope.current = null;
        setLoggingOut(false);
      }
    }
  }
  if (loading)
    return (
      <Gate>
        <div className="gate-card">
          <p className="eyebrow">WELCOME TO MOOSURI</p>
          <h1>
            Opening your space<span className="brand-dot">.</span>
          </h1>
          <p className="muted" role="status">
            Checking this installation and your session…
          </p>
        </div>
      </Gate>
    );
  if (!system || bootFailed)
    return (
      <Gate>
        <div className="gate-card">
          <h1>Let’s reconnect.</h1>
          <Notice>{error}</Notice>
          <button
            className="primary"
            onClick={() => setBoot((value) => value + 1)}
          >
            Try again
          </button>
        </div>
      </Gate>
    );
  if (!system.initialized)
    return (
      <Gate>
        <div className="gate-card">
          <p className="eyebrow">A FRESH START</p>
          <h1>A space, ready to become yours.</h1>
          <p className="muted">
            This installation needs its first administrator. Ask the server
            operator to initialize moosuri in a private terminal.
          </p>
          <pre>moosuri init</pre>
          <p className="fine">
            Follow the secure password prompt. Never put a password in a command
            argument, URL, or shared log. Web setup is not available in this
            build.
          </p>
          <button
            className="primary"
            onClick={() => setBoot((value) => value + 1)}
          >
            Check again
          </button>
        </div>
      </Gate>
    );
  if (loggingOut)
    return (
      <Gate>
        <div className="gate-card">
          <h1>See you soon.</h1>
          <p role="status">Finishing sign-out…</p>
        </div>
      </Gate>
    );
  if (!session)
    return (
      <Login
        key={epoch}
        onLogin={adopt}
        beginMutation={beginMutation}
        message={error}
        onRetryLogout={error.startsWith("Sign-out") ? logout : undefined}
      />
    );
  if (session.password_change_required || session.user.must_change_password)
    return (
      <PasswordChange
        key={epoch}
        session={session}
        onChanged={adopt}
        beginMutation={beginMutation}
        onLogout={logout}
        onExpired={expire}
      />
    );
  return (
    <Workspace
      key={`${session.user.id}:${epoch}`}
      session={session}
      path={path}
      navigate={navigate}
      onLogout={logout}
      onExpired={expire}
    />
  );
}

function Login({
  onLogin,
  beginMutation,
  message,
  onRetryLogout,
}: {
  onLogin: (s: SessionView) => void;
  beginMutation: () => () => void;
  message: string;
  onRetryLogout?: () => void;
}) {
  const scope = useScope();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    const release = beginMutation();
    const credential = password;
    setPassword("");
    try {
      onLogin(
        await scope.request<SessionView>("/auth/login", "POST", {
          email,
          password: credential,
        }),
      );
    } catch (e) {
      if (!isAborted(e)) {
        setError(
          e instanceof ApiFailure && e.status === 401
            ? "The email or password is incorrect, or the invitation has expired."
            : errorText(e),
        );
        setBusy(false);
      }
    } finally {
      release();
    }
  }
  return (
    <Gate>
      <div className="gate-card">
        <p className="eyebrow">A LITTLE ROOM TO THINK</p>
        <h1>
          Welcome back<span className="brand-dot">.</span>
        </h1>
        <p className="muted">Sign in to your own moosuri space.</p>
        {(error || message) && <Notice>{error || message}</Notice>}
        {onRetryLogout && (
          <button
            className="text-button"
            disabled={busy}
            onClick={() => {
              if (!busy) onRetryLogout();
            }}
          >
            Retry sign-out
          </button>
        )}
        <form onSubmit={submit}>
          <label>
            Email
            <input
              autoFocus
              name="email"
              type="email"
              autoComplete="username"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              disabled={busy}
            />
          </label>
          <label>
            Password
            <input
              name="password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={busy}
            />
          </label>
          <button className="primary full" disabled={busy}>
            {busy ? "Signing in…" : "Sign in"}
            <span aria-hidden="true">↗</span>
          </button>
        </form>
        <p className="fine">
          New here? Your administrator can create a local invitation for you.
          Temporary passwords must be changed after sign-in.
        </p>
      </div>
      <div className="gate-aside" aria-hidden="true">
        <div className="orbit">
          <Mark large />
        </div>
        <p>
          Make room
          <br />
          for what matters.
        </p>
        <span>YOUR ASSISTANT. YOUR SPACE.</span>
      </div>
    </Gate>
  );
}

function PasswordChange({
  session,
  beginMutation,
  onChanged,
  onLogout,
  onExpired,
}: {
  session: SessionView;
  beginMutation: () => () => void;
  onChanged: (s: SessionView) => void;
  onLogout: () => void;
  onExpired: () => void;
}) {
  const scope = useScope(session.user.id);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(e: FormEvent) {
    e.preventDefault();
    if (password !== confirm) {
      setError("The passwords do not match.");
      return;
    }
    setBusy(true);
    setError("");
    const release = beginMutation();
    const next = password;
    setPassword("");
    setConfirm("");
    try {
      onChanged(
        await scope.request<SessionView>("/auth/password", "POST", {
          password: next,
        }),
      );
    } catch (e) {
      if (!isAborted(e)) {
        if (e instanceof ApiFailure && e.status === 401) onExpired();
        else {
          setError(errorText(e));
          setBusy(false);
        }
      }
    } finally {
      release();
    }
  }
  return (
    <Gate>
      <div className="gate-card">
        <p className="eyebrow">ONE STEP BEFORE YOU SETTLE IN</p>
        <h1>A password of your own.</h1>
        <p className="muted">
          Hi {session.user.name}. Replace your temporary password to open your
          space. Until then, only password change and sign-out are available.
        </p>
        {error && <Notice>{error}</Notice>}
        <form onSubmit={submit}>
          <label>
            New password
            <input
              autoFocus
              type="password"
              autoComplete="new-password"
              aria-describedby="password-help"
              minLength={12}
              required
              value={password}
              disabled={busy}
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
          <p id="password-help" className="field-help">
            Use at least 12 characters.
          </p>
          <label>
            Confirm new password
            <input
              type="password"
              autoComplete="new-password"
              minLength={12}
              required
              value={confirm}
              disabled={busy}
              onChange={(e) => setConfirm(e.target.value)}
            />
          </label>
          <button className="primary full" disabled={busy}>
            {busy ? "Saving password…" : "Save password & continue"}
          </button>
        </form>
        <button
          className="text-button"
          disabled={busy}
          onClick={() => {
            if (!busy) onLogout();
          }}
        >
          Sign out
        </button>
      </div>
    </Gate>
  );
}

function Workspace({
  session,
  path,
  navigate,
  onLogout,
  onExpired,
}: {
  session: SessionView;
  path: string;
  navigate: (path: string) => void;
  onLogout: () => void;
  onExpired: () => void;
}) {
  const scope = useScope(session.user.id);
  const [threads, setThreads] = useState<ThreadSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [knowledgeDrafts, setKnowledgeDrafts] = useState(
    initialKnowledgeDrafts,
  );
  const [artifactTarget,setArtifactTarget]=useState<{id:string;revision:number;title:string}|null>(null);
  const [artifactKind,setArtifactKind]=useState<import("../shared/api-types").ArtifactKind>("document");
  const [artifactDrafts,setArtifactDrafts]=useState<ArtifactDrafts>({});
  const [knowledgeTarget, setKnowledgeTarget] = useState<ChatEvidence | null>(null);
  const [chatMemory, setChatMemory] = useState<ChatMemory>({
    drafts: {},
    pending: {},
  });
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const pathRef = useRef(path);
  pathRef.current = path;
  const listController = useRef<AbortController | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [creating, setCreating] = useState(false);
  const [drawer, setDrawer] = useState(false);
  const [deleting, setDeleting] = useState<ThreadSummary | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const historyOpener = useRef<HTMLButtonElement>(null);
  const deleteOpener = useRef<HTMLButtonElement | null>(null);
  const admin =
    path === "/admin" || path === "/admin/providers" || path === "/admin/usage" || path === "/admin/google";
  const googleAdmin = path === "/admin/google";
  const adminUsage = path === "/admin/usage";
  const personalUsage = path === "/settings/usage";
  const knowledgePage = path === "/app/knowledge";
  const memoryPage = path === "/app/memory";
  const knowledgeOpen = knowledgePage || memoryPage;
  const providerAdmin = path === "/admin/providers";
  const personalModel = path === "/settings/model";
  const personalMail = path === "/settings/mail";
  const personalSources = path === "/settings/sources";
  const communitySkills = path === "/app/skills";
  const artifactPage=path==="/app/artifacts"||path.startsWith("/app/artifacts/");
  const schedulesPage = path === "/app/schedules";
  const allowedAdmin = session.user.role === "admin";
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(errorText(e));
  }
  useEffect(() => {
    const controller = new AbortController();
    listController.current = controller;
    setLoading(true);
    setError("");
    void scope
      .request<ThreadsResponse>("/threads", "GET", undefined, controller.signal)
      .then((result) => {
        setThreads(result.threads);
        setSelected((current) =>
          result.threads.some((t) => t.id === current)
            ? current
            : (result.threads[0]?.id ?? null),
        );
        setLoading(false);
      })
      .catch((e) => {
        if (!isAborted(e)) {
          fail(e);
          setLoading(false);
        }
      });
    return () => controller.abort();
    // The lifetime-scoped transport fences late account responses.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope, retry]);
  async function createThread() {
    setCreating(true);
    setError("");
    try {
      const thread = await scope.request<ThreadSummary>("/threads", "POST", {
        title: "New conversation",
      });
      setThreads((current) => [thread, ...current]);
      setSelected(thread.id);
      setDrawer(false);
      navigate("/app");
    } catch (e) {
      fail(e);
    } finally {
      setCreating(false);
    }
  }
  async function createForChat(): Promise<string> {
    setCreating(true);
    listController.current?.abort();
    try {
      const thread = await scope.request<ThreadSummary>("/threads", "POST", {
        title: "New conversation",
      });
      setThreads((current) => [
        thread,
        ...current.filter((item) => item.id !== thread.id),
      ]);
      if (
        selectedRef.current === null &&
        (pathRef.current === "/app" || pathRef.current === "/")
      )
        setSelected(thread.id);
      setRetry((value) => value + 1);
      return thread.id;
    } finally {
      setCreating(false);
    }
  }
  function removeMemory(id: string) {
    setChatMemory((current) => {
      const drafts = { ...current.drafts };
      const pending = { ...current.pending };
      delete drafts[id];
      delete pending[id];
      return { drafts, pending };
    });
  }
  function threadMissing(id: string) {
    setThreads((current) => current.filter((thread) => thread.id !== id));
    setSelected((current) => (current === id ? null : current));
    removeMemory(id);
  }
  async function removeThread() {
    if (!deleting) return;
    setDeleteBusy(true);
    setError("");
    try {
      await scope.request(
        `/threads/${encodeURIComponent(deleting.id)}`,
        "DELETE",
        {},
      );
      const remaining = threads.filter((t) => t.id !== deleting.id);
      setThreads(remaining);
      if (selected === deleting.id) setSelected(remaining[0]?.id ?? null);
      removeMemory(deleting.id);
      setDeleting(null);
    } catch (e) {
      fail(e);
    } finally {
      setDeleteBusy(false);
    }
  }
  const current = threads.find((t) => t.id === selected);
  const history = (
    <>
      <div className="sidebar-heading">
        <Brand />
        <button
          className="icon-button mobile-only"
          onClick={() => setDrawer(false)}
          aria-label="Close conversations"
        >
          ×
        </button>
      </div>
      <button
        className="new-chat"
        onClick={createThread}
        disabled={creating || loading}
      >
        <span aria-hidden="true">＋</span>
        {creating ? "Creating…" : "New conversation"}
      </button>
      <div className="history-title">
        YOUR CONVERSATIONS <span>{threads.length}</span>
      </div>
      <nav className="thread-list" aria-label="Conversations">
        {loading ? (
          <p className="sidebar-hint" role="status">
            Loading conversations…
          </p>
        ) : threads.length === 0 ? (
          <p className="sidebar-hint">
            A clear page.
            <br />
            Your conversations will live here.
          </p>
        ) : (
          threads.map((thread) => (
            <div
              className={`thread-row ${thread.id === selected && !admin && !personalModel && !personalUsage && !personalSources && !knowledgeOpen ? "active" : ""}`}
              key={thread.id}
            >
              <button
                className="thread-select"
                onClick={() => {
                  setSelected(thread.id);
                  navigate("/app");
                  setDrawer(false);
                }}
                aria-current={
                  thread.id === selected &&
                  !admin &&
                  !personalModel &&
                  !personalUsage &&
                  !personalSources &&
                  !knowledgeOpen
                    ? "page"
                    : undefined
                }
              >
                <span className="thread-symbol" aria-hidden="true">
                  ◌
                </span>
                <span>{thread.title}</span>
              </button>
              <button
                className="thread-delete"
                aria-label={`Delete ${thread.title}`}
                onClick={(event) => {
                  deleteOpener.current = event.currentTarget;
                  setDeleting(thread);
                  setDrawer(false);
                }}
              >
                ×
              </button>
            </div>
          ))
        )}
      </nav>
      <div className="sidebar-bottom">
        <PwaInstallButton />
        <div className="boundary-note">
          <span aria-hidden="true">⌁</span>
          <span>
            Your conversations belong
            <br />
            to your account.
          </span>
        </div>
        <button
          className={`nav-link ${knowledgePage ? "chosen" : ""}`}
          aria-current={knowledgePage ? "page" : undefined}
          onClick={() => {
            navigate("/app/knowledge");
            setDrawer(false);
          }}
        >
          자료<span aria-hidden="true">↗</span>
        </button>
        <button
          className={`nav-link ${memoryPage ? "chosen" : ""}`}
          aria-current={memoryPage ? "page" : undefined}
          onClick={() => {
            navigate("/app/memory");
            setDrawer(false);
          }}
        >
          기억<span aria-hidden="true">↗</span>
        </button>
        <button
          className={`nav-link ${personalModel ? "chosen" : ""}`}
          onClick={() => {
            navigate(personalModel ? "/app" : "/settings/model");
            setDrawer(false);
          }}
        >
          {personalModel ? "← 대화로 돌아가기" : "내 모델"}
          <span aria-hidden="true">↗</span>
        </button>
        <button
          className={`nav-link ${personalSources ? "chosen" : ""}`}
          aria-current={personalSources ? "page" : undefined}
          onClick={() => {
            navigate("/settings/sources");
            setDrawer(false);
          }}
        >
          자료 수집 연결<span aria-hidden="true">↗</span>
        </button>
        <button className={`nav-link ${artifactPage?"chosen":""}`} onClick={()=>{navigate("/app/artifacts");setDrawer(false)}}>문서 · 목표 · 요청<span aria-hidden="true">↗</span></button>
        <button className={`nav-link ${communitySkills ? "chosen" : ""}`} onClick={()=>{navigate("/app/skills");setDrawer(false)}}>제안 · 할 수 있는 일<span aria-hidden="true">↗</span></button>
        <button className={`nav-link ${schedulesPage ? "chosen" : ""}`} aria-current={schedulesPage?"page":undefined} onClick={()=>{navigate("/app/schedules");setDrawer(false)}}>예약 · 소식<span aria-hidden="true">↗</span></button>
        <button className={`nav-link ${personalMail ? "chosen" : ""}`} onClick={()=>{navigate(personalMail?"/app":"/settings/mail");setDrawer(false)}}>내 Gmail · 발송 확인<span aria-hidden="true">↗</span></button>
        <button
          className={`nav-link ${personalUsage ? "chosen" : ""}`}
          onClick={() => {
            navigate(personalUsage ? "/app" : "/settings/usage");
            setDrawer(false);
          }}
        >
          {personalUsage ? "← 대화로 돌아가기" : "사용량"}
          <span aria-hidden="true">↗</span>
        </button>
        {allowedAdmin && (
          <button
            className={`nav-link ${admin ? "chosen" : ""}`}
            onClick={() => {
              navigate(admin ? "/app" : "/admin");
              setDrawer(false);
            }}
          >
            {admin ? "← Back to your space" : "Manage installation"}
            <span aria-hidden="true">↗</span>
          </button>
        )}
        <div className="account">
          <span className="avatar">
            {session.user.name.slice(0, 1).toUpperCase()}
          </span>
          <span className="account-name">
            {session.user.name}
            <small>{session.user.email}</small>
          </span>
          <button
            className="icon-button"
            title="Sign out"
            aria-label="Sign out"
            onClick={onLogout}
          >
            ↪
          </button>
        </div>
      </div>
    </>
  );
  return (
    <div className="workspace">
      <aside className="sidebar">{history}</aside>
      <section className="work-area">
        <header className="work-header">
          <button
            className="icon-button mobile-only"
            ref={historyOpener}
            aria-label="Open conversations"
            onClick={() => setDrawer(true)}
          >
            ☰
          </button>
          <div>
            <span className="eyebrow">
              {schedulesPage ? "로컬 알림" : artifactPage ? "나의 기록" : communitySkills ? "제안" : personalSources ? "내 연결" : knowledgeOpen
                ? "나의 기록"
                : admin
                  ? "INSTALLATION"
                  : personalModel
                    ? "MY SETTINGS"
                    : "YOUR SPACE"}
            </span>
            <h1>
              {schedulesPage ? "예약 · 소식" : artifactPage ? "문서 · 목표 · 요청" : communitySkills ? "할 수 있는 일" : personalSources ? "자료 수집 연결" : knowledgeOpen
                ? memoryPage
                  ? "기억"
                  : "자료"
                : admin
                  ? googleAdmin
                    ? "Google 연결 설정"
                    : adminUsage
                    ? "계정별 사용량"
                    : providerAdmin
                      ? "모델 연결"
                      : "People & access"
                  : personalUsage
                    ? "내 사용량"
                    : personalModel
                      ? "내 모델"
                      : (current?.title ?? "A new beginning")}
            </h1>
          </div>
          <button
            className="icon-button mobile-only"
            aria-label="Sign out"
            onClick={onLogout}
          >
            ↪
          </button>
        </header>
        {error && (
          <div className="workspace-error">
            <Notice>{error}</Notice>
            <button
              className="text-button"
              disabled={creating || deleteBusy || loading}
              onClick={() => setRetry((n) => n + 1)}
            >
              Retry loading conversations
            </button>
          </div>
        )}
        {admin && allowedAdmin && (
          <nav className="settings-tabs" aria-label="설치 관리">
            <button
              aria-current={!providerAdmin && !adminUsage && !googleAdmin ? "page" : undefined}
              onClick={() => navigate("/admin")}
            >
              사용자
            </button>
            <button
              aria-current={providerAdmin ? "page" : undefined}
              onClick={() => navigate("/admin/providers")}
            >
              모델 연결
            </button>
            <button
              aria-current={adminUsage ? "page" : undefined}
              onClick={() => navigate("/admin/usage")}
            >
              사용량
            </button>
            <button aria-current={googleAdmin ? "page" : undefined} onClick={() => navigate("/admin/google")}>
              Google 연결
            </button>
          </nav>
        )}
        {schedulesPage ? <Schedules accountId={session.user.id} onExpired={onExpired} back={()=>navigate("/app")}/> : artifactPage ? <Artifacts accountId={session.user.id} initialKind={artifactKind} onKind={setArtifactKind} id={path.split("/")[3]} drafts={artifactDrafts} updateDrafts={setArtifactDrafts} navigate={navigate} onExpired={onExpired} openReference={r=>{if(r.kind==="artifact"){navigate(`/app/artifacts/${r.id}`)}else{setKnowledgeTarget({kind:r.kind,id:r.id,revision:r.revision,title:"기록의 근거"});navigate(r.kind==="source"?"/app/knowledge":"/app/memory")}}} revise={(id,revision,title)=>{setArtifactTarget({id,revision,title});setSelected(null);setChatMemory(v=>({...v,drafts:{...v.drafts,new:`선택한 ${namesForArtifact(artifactKind)}를 수정해 줘. ${v.drafts.new??""}`}}));navigate("/app")}}/> : personalSources ? (
          <SourceConnections accountId={session.user.id} onExpired={onExpired} back={() => navigate("/app")} />
        ) : communitySkills ? <CommunitySkills accountId={session.user.id} onExpired={onExpired} back={()=>navigate("/app")} connect={()=>navigate("/settings/mail")} start={(name)=>{
          setSelected(null);
          setChatMemory(current => ({
            ...current,
            drafts: {
              ...current.drafts,
              new: `${name} 절차로 도와줘: ${current.drafts.new ?? ""}`,
            },
          }));
          navigate("/app");
        }}/> : personalMail ? <MailConnections accountId={session.user.id} onExpired={onExpired} back={()=>navigate("/app")} originThreadId={selected} openGoogleSetup={allowedAdmin ? () => navigate("/admin/google") : undefined}/> : knowledgeOpen ? (
          <KnowledgeView
            accountId={session.user.id}
            mode={memoryPage ? "memory" : "source"}
            target={knowledgeTarget}
            onTargetConsumed={() => setKnowledgeTarget(null)}
            drafts={knowledgeDrafts}
            updateDrafts={setKnowledgeDrafts}
            onExpired={onExpired}
          />
        ) : personalUsage ? (
          <UsageView accountId={session.user.id} onExpired={onExpired} />
        ) : personalModel ? (
          <PersonalModelSettings
            accountId={session.user.id}
            onExpired={onExpired}
          />
        ) : admin ? (
          allowedAdmin ? (
            googleAdmin ? (
              <GoogleSetup accountId={session.user.id} onExpired={onExpired} />
            ) : adminUsage ? (
              <UsageView
                accountId={session.user.id}
                admin
                onExpired={onExpired}
              />
            ) : providerAdmin ? (
              <AdminProviderSettings
                accountId={session.user.id}
                onExpired={onExpired}
              />
            ) : (
              <Admin accountId={session.user.id} onExpired={onExpired} />
            )
          ) : (
            <div className="empty-state">
              <h2>This page is for administrators.</h2>
              <p>You can continue in your own space.</p>
              <button className="primary" onClick={() => navigate("/app")}>
                Back to conversations
              </button>
            </div>
          )
        ) : (
          <ChatPanel
            accountId={session.user.id}
            artifactTarget={artifactTarget}
            clearArtifactTarget={()=>setArtifactTarget(null)}
            openArtifact={id=>navigate(`/app/artifacts/${id}`)}
            name={session.user.name}
            threadId={selected}
            memory={chatMemory}
            onDraft={(slot, text) =>
              setChatMemory((current) => ({
                ...current,
                drafts: { ...current.drafts, [slot]: text },
              }))
            }
            onPending={(slot, value, expected) =>
              setChatMemory((current) => {
                if (expected && current.pending[slot]?.request_id !== expected)
                  return current;
                const pending = { ...current.pending };
                if (value) pending[slot] = value;
                else delete pending[slot];
                return { ...current, pending };
              })
            }
            onRestoreDraft={(slot, text) =>
              setChatMemory((current) => ({
                ...current,
                drafts: {
                  ...current.drafts,
                  [slot]:
                    text +
                    (current.drafts[slot] ? "\n\n" + current.drafts[slot] : ""),
                },
              }))
            }
            onCreated={(id, requestId) =>
              setChatMemory((current) => {
                if (current.pending.new?.request_id !== requestId)
                  return current;
                const pending = {
                  ...current.pending,
                  [id]: current.pending.new,
                };
                delete pending.new;
                const drafts = {
                  ...current.drafts,
                  [id]: current.drafts.new ?? "",
                };
                delete drafts.new;
                return { pending, drafts };
              })
            }
            onSuspend={() =>
              setChatMemory((current) => ({
                ...current,
                pending: Object.fromEntries(
                  Object.entries(current.pending).map(([id, pending]) => [
                    id,
                    pending.status === "sending"
                      ? { ...pending, status: "uncertain" as const }
                      : pending,
                  ]),
                ),
              }))
            }
            createThread={createForChat}
            onExpired={onExpired}
            onThreadMissing={threadMissing}
            onThreadUpdated={() => setRetry((value) => value + 1)}
            openSettings={() => navigate("/settings/model")}
            onOpenEvidence={(reference) => {
              setKnowledgeTarget(reference);
              navigate(reference.kind === "source" ? "/app/knowledge" : "/app/memory");
            }}
          />
        )}
      </section>
      {drawer && (
        <Modal
          title="Your conversations"
          returnFocus={historyOpener.current}
          close={() => setDrawer(false)}
        >
          <div className="mobile-history">{history}</div>
        </Modal>
      )}
      {deleting && (
        <Modal
          title="Delete conversation?"
          returnFocus={deleteOpener.current}
          close={() => {
            if (!deleteBusy) setDeleting(null);
          }}
        >
          <p>
            “{deleting.title}” will be removed from your account. This cannot be
            undone.
          </p>
          <div className="dialog-actions">
            <button
              className="secondary"
              disabled={deleteBusy}
              onClick={() => setDeleting(null)}
            >
              Keep conversation
            </button>
            <button
              className="danger"
              disabled={deleteBusy}
              onClick={removeThread}
            >
              {deleteBusy ? "Deleting…" : "Delete conversation"}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

function Admin({
  accountId,
  onExpired,
}: {
  accountId: string;
  onExpired: () => void;
}) {
  const scope = useScope(accountId);
  const invitationOpener = useRef<HTMLButtonElement>(null);
  const [users, setUsers] = useState<AccountSummary[]>([]);
  const [management, setManagement] = useState<ManagedAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [retry, setRetry] = useState(0);
  const [error, setError] = useState("");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const [issuedInvitation, setInvitation] = useState<{
    user: AccountSummary;
    temporary_password: string;
    owner_id: string;
    revision: number | null;
    request_id: string | null;
    reset?: boolean;
  } | null>(null);
  const knownTarget=issuedInvitation ? management.find(m=>m.user.id===issuedInvitation.user.id) : undefined;
  const invitation=issuedInvitation && issuedInvitation.owner_id===accountId &&
    !(issuedInvitation.revision!==null && knownTarget && knownTarget.revision>issuedInvitation.revision)
    ? issuedInvitation : null;
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState("");
  useLayoutEffect(()=>{
    if(issuedInvitation && !invitation){setInvitation(null);setCopied(false);setCopyError("");}
  },[issuedInvitation,invitation]);
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(errorText(e));
  }
  useEffect(() => {
    setLoading(users.length === 0);
    setError("");
    const controller=new AbortController();
    void scope
      .request<UsersResponse & { management?: ManagedAccount[] }>("/admin/users","GET",undefined,controller.signal)
      .then((result) => {
        setUsers(result.users);
        setManagement(result.management ?? []);
        setLoading(false);
      })
      .catch((e) => {
        if (!isAborted(e)) {
          fail(e);
          setLoading(false);
        }
      });
    return ()=>controller.abort();
  }, [scope, retry]);
  async function create(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = await scope.request<{
        user: AccountSummary;
        temporary_password: string;
        management?: ManagedAccount;
      }>("/admin/users", "POST", { name, email });
      setUsers((current) => [...current, result.user]);
      if(result.management)setManagement(current=>[...current,result.management!]);
      setInvitation({...result,owner_id:accountId,revision:result.management?.revision??null,request_id:null});
      setName("");
      setEmail("");
      setCopied(false);
      setCopyError("");
    } catch (e) {
      if (!isAborted(e) && !(e instanceof ApiFailure)) {
        setError(
          "Could not confirm account creation. Refresh the people list before trying again. No email delivery was attempted.",
        );
      } else fail(e);
    } finally {
      setBusy(false);
    }
  }
  // Clipboard is browser-owned. We never persist a credential or silently clear
  // an unrelated clipboard value later; the recipient chooses where to paste it.
  const invitationLifetime = useRef(0);
  useLayoutEffect(() => {
    invitationLifetime.current += 1;
    return () => {
      invitationLifetime.current += 1;
    };
  }, [invitation]);
  async function copyPassword() {
    if (!invitation) return;
    const generation = invitationLifetime.current;
    try {
      await navigator.clipboard.writeText(invitation.temporary_password);
      if (generation === invitationLifetime.current) setCopied(true);
    } catch {
      if (generation === invitationLifetime.current)
        setCopyError(
          "Clipboard unavailable. Select the password and copy it manually.",
        );
    }
  }
  return (
    <main className="admin-scroll">
      <div className="admin-intro">
        <p className="eyebrow">A SHARED INSTALLATION. SEPARATE SPACES.</p>
        <h2>A place for your people.</h2>
        <p>
          Manage local accounts. Each person has their own conversations and
          private workspace.
        </p>
      </div>
      {error && (
        <>
          <Notice>{error}</Notice>
          <button
            className="text-button"
            disabled={busy || loading}
            onClick={() => setRetry((n) => n + 1)}
          >
            Retry user list
          </button>
        </>
      )}
      <section className="admin-card">
        <div className="section-heading">
          <div>
            <h3>Invite someone</h3>
            <p>
              Create a member account and share its temporary password
              privately.
            </p>
          </div>
          <span className="pill">LOCAL INVITATION</span>
        </div>
        <form className="invite-form" onSubmit={create}>
          <label>
            Name
            <input
              name="name"
              autoComplete="off"
              required
              maxLength={100}
              value={name}
              disabled={busy}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label>
            Email
            <input
              name="email"
              type="email"
              autoComplete="off"
              required
              value={email}
              disabled={busy}
              onChange={(e) => setEmail(e.target.value)}
            />
          </label>
          <button
            ref={invitationOpener}
            className="primary"
            disabled={busy || loading}
          >
            {busy ? "Creating…" : "Create invitation"}
            <span aria-hidden="true">＋</span>
          </button>
        </form>
        <p className="fine">
          No email is sent. The new member must change their password on first
          sign-in.
        </p>
      </section>
      <section className="admin-card user-card">
        <div className="section-heading">
          <div>
            <h3>
              People <span className="count">{users.length}</span>
            </h3>
            <p>
              Account details only. Private conversations are not available
              here.
            </p>
          </div>
        </div>
        {loading ? (
          <p className="muted" role="status">
            Loading people…
          </p>
        ) : (
          <ul className="user-list">
            {users.map((user) => (
              <li key={user.id}>
                <span className="avatar">
                  {user.name.slice(0, 1).toUpperCase()}
                </span>
                <span className="user-detail">
                  <strong>{user.name}</strong>
                  <span>{user.email}</span>
                </span>
                <span
                  className={`pill ${user.role === "admin" ? "admin-pill" : ""}`}
                >
                  {user.role === "admin" ? "ADMIN" : "MEMBER"}
                </span>
                <span className="user-state">
                  {management.find((m)=>m.user.id===user.id)?.recovery_required ? "Recovery needed" : management.find((m)=>m.user.id===user.id)?.status==="suspended" ? "Suspended" : user.must_change_password
                    ? "Password change needed"
                    : "Active"}
                </span>
                {management.filter((m)=>m.user.id===user.id).map((target)=><AccountControls key={target.user.id} accountId={accountId} target={target} onExpired={onExpired} onChanged={()=>setRetry(n=>n+1)} onPassword={(password,receipt)=>{setInvitation({user,temporary_password:password,owner_id:accountId,revision:receipt.revision,request_id:receipt.request_id,reset:true});setCopied(false);setCopyError("");}}/>)}
              </li>
            ))}
          </ul>
        )}
      </section>
      {invitation && (
        <Modal
          title={invitation.reset ? "A new password, just once." : "An invitation, just once."}
          returnFocus={invitationOpener.current}
          close={() => {
            setInvitation(null);
            setCopied(false);
            setCopyError("");
          }}
        >
          <p>
            {invitation.reset ? <>A new temporary password was issued for <strong>{invitation.user.name}</strong>. Account suspension stays unchanged.</> : <>The account for <strong>{invitation.user.name}</strong> is created.</>}
            {" "}No email was sent. Share this password through a private channel.
          </p>
          <label className="temporary-label">
            Temporary password
            <input
              className="temporary-password"
              value={invitation.temporary_password}
              readOnly
              autoComplete="off"
              aria-label="Temporary password"
              onFocus={(e) => e.target.select()}
            />
          </label>
          <p className="fine">
            Shown only now. Closing this window removes it from this page. The
            member must choose a new password after signing in.
          </p>
          {copyError && <Notice>{copyError}</Notice>}
          <div className="dialog-actions">
            <button
              className="secondary"
              onClick={() => {
                setInvitation(null);
                setCopied(false);
                setCopyError("");
              }}
            >
              Done
            </button>
            <button className="primary" onClick={copyPassword}>
              {copied ? "Copied" : "Copy password"}
            </button>
          </div>
          <span role="status" className="sr-only">
            {copied ? "Password copied to clipboard." : ""}
          </span>
        </Modal>
      )}
    </main>
  );
}

function namesForArtifact(kind:import("../shared/api-types").ArtifactKind){return kind==="document"?"문서":kind==="goal"?"목표":"요청 기록"}
