import {
  createContext,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";
import type {
  AccountSummary,
  SessionView,
  SystemStatus,
  ThreadSummary,
  ThreadsResponse,
  UsersResponse,
} from "../shared/api-types";
import { ApiFailure, errorText, isAborted, RequestScope } from "./api";

const ScopeRegistry = createContext<Set<RequestScope> | null>(null);

function useScope(expectedAccountId?: string) {
  const registry = useContext(ScopeRegistry);
  const ref = useRef<RequestScope | null>(null);
  useLayoutEffect(() => {
    const scope = new RequestScope(expectedAccountId);
    ref.current = scope;
    registry?.add(scope);
    return () => {
      scope.close();
      registry?.delete(scope);
    };
  }, [expectedAccountId, registry]);
  return useMemo(
    () => ({
      request: <T,>(path: string, method?: string, body?: unknown) =>
        ref.current!.request<T>(path, method, body),
    }),
    [],
  );
}
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
  function revalidate() {
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
  async function logout() {
    const expectedAccount = session?.user.id ?? logoutIdentity.current;
    if (!expectedAccount || logoutScope.current) return;
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
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [creating, setCreating] = useState(false);
  const [drawer, setDrawer] = useState(false);
  const [deleting, setDeleting] = useState<ThreadSummary | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const historyOpener = useRef<HTMLButtonElement>(null);
  const deleteOpener = useRef<HTMLButtonElement | null>(null);
  const admin = path === "/admin";
  const allowedAdmin = session.user.role === "admin";
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(errorText(e));
  }
  useEffect(() => {
    setLoading(true);
    setError("");
    void scope
      .request<ThreadsResponse>("/threads")
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
              className={`thread-row ${thread.id === selected && !admin ? "active" : ""}`}
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
                  thread.id === selected && !admin ? "page" : undefined
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
        <div className="boundary-note">
          <span aria-hidden="true">⌁</span>
          <span>
            Your conversations belong
            <br />
            to your account.
          </span>
        </div>
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
              {admin ? "INSTALLATION" : "YOUR SPACE"}
            </span>
            <h1>
              {admin
                ? "People & access"
                : (current?.title ?? "A new beginning")}
            </h1>
          </div>
          <div className="connection-status">
            <span />
            Model not connected
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
        {admin ? (
          allowedAdmin ? (
            <Admin accountId={session.user.id} onExpired={onExpired} />
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
          <>
            <main className="conversation-scroll">
              <div className="empty-state">
                <div className="empty-mark">
                  <Mark large />
                </div>
                <p className="eyebrow">ROOM FOR YOUR NEXT THOUGHT</p>
                <h2>
                  {current
                    ? "Your conversation is ready."
                    : `Hello, ${session.user.name.split(" ")[0]}.`}
                </h2>
                <p>
                  {current
                    ? "This conversation has no messages yet."
                    : "Start with a fresh conversation. A place to think things through, one step at a time."}
                </p>
                <div className="availability">
                  <span className="availability-icon" aria-hidden="true">
                    ⌁
                  </span>
                  <div>
                    <strong>A connection comes next</strong>
                    <p>
                      Chat is unavailable in this build. Model connections and
                      message sending are still being implemented.
                    </p>
                  </div>
                </div>
                {!current && (
                  <button
                    className="text-button"
                    onClick={createThread}
                    disabled={creating || loading}
                  >
                    {creating ? "Creating…" : "Create a conversation"}{" "}
                    <span aria-hidden="true">↗</span>
                  </button>
                )}
              </div>
            </main>
            <footer className="composer-area">
              <div className="composer">
                <label className="sr-only" htmlFor="message">
                  Message
                </label>
                <textarea
                  id="message"
                  placeholder="Your next thought goes here…"
                  disabled
                  aria-describedby="composer-status"
                  rows={2}
                />
                <div className="composer-bottom">
                  <span>Waiting for a model connection</span>
                  <button disabled aria-label="Send message unavailable">
                    ↑
                  </button>
                </div>
              </div>
              <p id="composer-status" className="composer-caption">
                Message sending is not available yet. Nothing is sent to a
                model.
              </p>
            </footer>
          </>
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
  const [loading, setLoading] = useState(true);
  const [retry, setRetry] = useState(0);
  const [error, setError] = useState("");
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [busy, setBusy] = useState(false);
  const [invitation, setInvitation] = useState<{
    user: AccountSummary;
    temporary_password: string;
  } | null>(null);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState("");
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(errorText(e));
  }
  useEffect(() => {
    setLoading(true);
    setError("");
    void scope
      .request<UsersResponse>("/admin/users")
      .then((result) => {
        setUsers(result.users);
        setLoading(false);
      })
      .catch((e) => {
        if (!isAborted(e)) {
          fail(e);
          setLoading(false);
        }
      });
  }, [scope, retry]);
  async function create(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = await scope.request<{
        user: AccountSummary;
        temporary_password: string;
      }>("/admin/users", "POST", { name, email });
      setUsers((current) => [...current, result.user]);
      setInvitation(result);
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
  useEffect(() => {
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
                  {user.must_change_password
                    ? "Password change needed"
                    : "Active"}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
      {invitation && (
        <Modal
          title="An invitation, just once."
          returnFocus={invitationOpener.current}
          close={() => {
            setInvitation(null);
            setCopied(false);
            setCopyError("");
          }}
        >
          <p>
            The account for <strong>{invitation.user.name}</strong> is created.
            No email was sent. Share this password through a private channel.
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
