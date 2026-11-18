import { AppShell } from "@astryxdesign/core/AppShell";
import { SideNav, SideNavItem } from "@astryxdesign/core/SideNav";
import { TopNav } from "@astryxdesign/core/TopNav";
import { Button } from "@astryxdesign/core/Button";
import { Dialog } from "@astryxdesign/core/Dialog";
import { ConversationSearch, WorkspaceIcon } from "./WorkspaceNavigation";
import { MediaLibraryView } from "./Media";
import { emptyInstructions } from "./run-instructions";
import { accountPasswordValid, ACCOUNT_PASSWORD_HELP } from "./account-password";
import {InvitationEmail} from "./InvitationEmail";
import { useWorkspaceViewport } from "./workspace-viewport";
import { fenceIncomingShares } from "./share-state";
import { refreshShareSession } from "./share-intake";
import { IncomingShareNotice } from "./IncomingShare";
import {ChatDraftRecovery,draftScope,type ChatDraftSession} from "./chat-drafts";
import {activatePushAccount,clearBrowserPushBinding} from "./push-binding";

import {VaultLaunch,lockVaultWindows} from "./VaultLaunch";
import { clearMailReturn, mailReturnThread, rememberMailReturn } from "./mail-return";
import { GWS_CONNECTED, gwsConnectedTarget } from "./gws-connection";
import {
  lazy,
  useEffect,
  useLayoutEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type SetStateAction,
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
const TelegramSettings = lazy(() => import("./TelegramSettings").then(module => ({default: module.TelegramSettings})));
const NativeCredentials = lazy(() => import("./NativeCredentials").then(module => ({default: module.NativeCredentials})));
const MailConnections = lazy(() => import("./MailConnections").then(module => ({default: module.MailConnections})));
const GoogleSetup = lazy(() => import("./GoogleSetup").then(module => ({default: module.GoogleSetup})));
import type { ArtifactDrafts } from "./Artifacts";
const Artifacts = lazy(() => import("./Artifacts").then(module => ({default: module.Artifacts})));
const CommunitySkills = lazy(() => import("./CommunitySkills").then(module => ({default: module.CommunitySkills})));
const Missions = lazy(() => import("./Missions").then(module => ({default: module.Missions})));
const Calendar = lazy(() => import("./Calendar").then(module => ({default: module.Calendar})));
const Schedules = lazy(() => import("./Schedules").then(module => ({default: module.Schedules})));
const SourceConnections = lazy(() => import("./SourceConnections").then(module => ({default: module.SourceConnections})));
const UsageView = lazy(() => import("./UsageView").then(module => ({default: module.UsageView})));
const KnowledgeView = lazy(() => import("./KnowledgeView").then(module => ({default: module.KnowledgeView})));
import { initialKnowledgeDrafts } from "./KnowledgeState";
import { DeferredPage } from "./DeferredPage";
const AdminProviderSettings = lazy(() => import("./ProviderSettings").then(module => ({default: module.AdminProviderSettings})));
const PersonalModelSettings = lazy(() => import("./ProviderSettings").then(module => ({default: module.PersonalModelSettings})));
const MemberModelSettings = lazy(() => import("./ProviderSettings").then(module => ({default: module.MemberModelSettings})));

function Brand() {
  return <span className="brand"><img className="brand-icon" src="/moosuri-icon-192.png" width="32" height="32" alt="" />Moosuri</span>;
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
      <header><Brand /></header>
      <main className="gate-main">{children}</main>
    </div>
  );
}
function Modal({
  title,
  children,
  close,
  returnFocus,
  fallbackFocus,
  side = false,
}: {
  title: ReactNode;
  children: ReactNode;
  close: () => void;
  returnFocus?: HTMLElement | null;
  fallbackFocus?: HTMLElement | null;
  side?: boolean;
}) {
  const titleId = useId();
  useEffect(() => {
    return () => {
      // The action which opened deletion may have disappeared with its row.
      if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
      else fallbackFocus?.focus({ preventScroll: true });
    };
  }, [returnFocus, fallbackFocus]);
  return (
    <Dialog isOpen purpose="form" className={side ? "workspace-dialog workspace-drawer" : "workspace-dialog"} width={side ? 360 : 520}
      position={side ? {start: 0, top: 0, bottom: 0} : undefined} aria-labelledby={titleId}
      onOpenChange={open => { if (!open) close(); }}>
      <div className="dialog-top">
        <h2 id={titleId}>{title}</h2>
        <Button variant="ghost" label="Close dialog" icon={<span aria-hidden="true">×</span>} isIconOnly onClick={close}/>
      </div>
      {children}
    </Dialog>
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
  const draftRecovery=useMemo(()=>new ChatDraftRecovery(),[]);
  const [draftSession,setDraftSession]=useState<ChatDraftSession|null>(null);
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
  const revalidateRef = useRef<(destructive?: boolean) => void>(() => {});
  const retryBootRef = useRef<() => void>(() => {});
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
    lockVaultWindows(session?.user.id ?? logoutIdentity.current);
    for (const scope of scopes) scope.close();
    bootScope.current?.close();
    focusScope.current?.close();
    focusScope.current = null;
    logoutScope.current?.close();
    logoutScope.current = null;
  }
  useEffect(()=>{if(session)void refreshShareSession().catch(()=>{});},[session?.user.id, session?.chat_draft_scope]);
  useEffect(()=>{if(session)void activatePushAccount(session.user.id).catch(()=>{});},[session?.user.id]);
  useEffect(()=>{const listener=(event:MessageEvent)=>{if(event.data?.type==="moosuri-open-schedules"&&session)navigate("/app/schedules");if(event.data?.type==="moosuri-open-mission"&&session&&event.data.account_id===session.user.id&&typeof event.data.mission_id==="string"&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(event.data.mission_id))navigate(`/app/missions#mission-${event.data.mission_id}`);};navigator.serviceWorker?.addEventListener("message",listener);return()=>navigator.serviceWorker?.removeEventListener("message",listener);},[session?.user.id]);
  function retryBoot() {
    // Connectivity is not an authentication transition. Keep saved bytes opaque
    // until the fresh session and owned inventory confirm them, but synchronously
    // retire the abandoned probe before React starts the replacement effect.
    bootScope.current?.close();
    bootScope.current = null;
    setLoading(true);
    setBoot((value) => value + 1);
  }
  function revalidate(destructive = false) {
    void fenceIncomingShares(destructive, session ? draftScope(session) : null, session?.user.id).catch(()=>{});
    draftRecovery.clear();
    setDraftSession(null);
    if(session)void clearBrowserPushBinding(session.user.id);
    // Synchronously fence every old response before React unmounts its UI.
    clearLifetimes();
    authMutation.current = null;
    setSession(null);
    setEpoch((value) => value + 1);
    setLoggingOut(false);
    retryBoot();
  }
  revalidateRef.current = revalidate;
  retryBootRef.current = retryBoot;
  function beginMutation() {
    void fenceIncomingShares(!!session, session ? draftScope(session) : null, session?.user.id).catch(()=>{});
    if (session) announceTransition(true);
    draftRecovery.clear();
    setDraftSession(null);
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
          draftScope(current) !== draftScope(session) ||
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
          if (event.data?.type === "auth-changed") revalidateRef.current(event.data?.retired === true);
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
  function announceTransition(retired = false) {
    try {
      authChannel.current?.postMessage(retired ? { type: "auth-changed", retired: true } : { type: "auth-changed" });
    } catch {
      /* Optional cross-tab hint. */
    }
  }
  function navigate(next: string) {
    window.history.pushState(null, "", next);
    setPath(window.location.pathname);
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
        if (bootScope.current !== scope) return;
        setSystem(status);
        if (status.initialized) {
          try {
            const current = await scope.request<SessionView>("/auth/session");
            if (bootScope.current !== scope) return;
            logoutIdentity.current = null;
            setDraftSession(draftRecovery.confirm(current));
            setSession(current);
          } catch (e) {
            if (bootScope.current !== scope) return;
            if (!(e instanceof ApiFailure && e.status === 401)) throw e;
            draftRecovery.clear();
            setDraftSession(null);
            void clearBrowserPushBinding();
            setSession(null);
          }
        }
        if(!status.initialized)draftRecovery.clear();
        setLoading(false);
      } catch (e) {
        if (bootScope.current !== scope) return;
        if (!isAborted(e)) {
          setError(errorText(e));
          setBootFailed(true);
          setLoading(false);
        }
      }
    })();
    return () => {
      scope.close();
      if (bootScope.current === scope) bootScope.current = null;
    };
  }, [boot]);
  function adopt(next: SessionView) {
    draftRecovery.clear();
    setDraftSession(draftRecovery.confirm(next));
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
      if (bootFailed || loading) retryBootRef.current();
      else checkSessionRef.current();
    };
    window.addEventListener("online", reconnect);
    return () => window.removeEventListener("online", reconnect);
  }, [bootFailed, loading]);
  async function logout() {
    const expectedAccount = session?.user.id ?? logoutIdentity.current;
    if (!expectedAccount || logoutScope.current) return;
    draftRecovery.clear();
    setDraftSession(null);
    void fenceIncomingShares(true, session ? draftScope(session) : null, expectedAccount).catch(()=>{});
    void clearBrowserPushBinding(expectedAccount);
    announceTransition(true);
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
          <p role="status">무수리 연결 중…</p>
        </div>
      </Gate>
    );
  if (!system || bootFailed)
    return (
      <Gate>
        <div className="gate-card">
          <h1>연결을 확인해 주세요</h1>
          <Notice>{error}</Notice>
          <button
            className="primary"
            onClick={retryBoot}
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
          <h1>설치 설정</h1>
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
            onClick={retryBoot}
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
      draftSession={draftSession}
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
        <h1>로그인</h1>
        <IncomingShareNotice/>
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
        <p className="fine">계정이 필요하면 관리자에게 초대를 요청하세요.</p>
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
    if (!accountPasswordValid(password)) {
      setError(ACCOUNT_PASSWORD_HELP);
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
        <h1>비밀번호 변경</h1>
        <p className="muted">
          계속하려면 임시 비밀번호를 바꿔 주세요.
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
              minLength={8}
              required
              value={password}
              disabled={busy}
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
          <p id="password-help" className="field-help">
            {ACCOUNT_PASSWORD_HELP}
          </p>
          <label>
            Confirm new password
            <input
              type="password"
              autoComplete="new-password"
              minLength={8}
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
  draftSession,
  path,
  navigate: navigateParent,
  onLogout,
  onExpired,
}: {
  session: SessionView;
  draftSession: ChatDraftSession|null;
  path: string;
  navigate: (path: string) => void;
  onLogout: () => void;
  onExpired: () => void;
}) {
  const scope = useScope(session.user.id);
  const workspace = useWorkspaceViewport();
  const [threads, setThreads] = useState<ThreadSummary[]>([]);
  const [selected, updateSelected] = useState<string | null>(null);
  const [knowledgeDrafts, setKnowledgeDrafts] = useState(
    initialKnowledgeDrafts,
  );
  const [artifactTarget,setArtifactTarget]=useState<{id:string;revision:number;title:string}|null>(null);
  const [artifactKind,setArtifactKind]=useState<import("../shared/api-types").ArtifactKind>("document");
  const [artifactDrafts,setArtifactDrafts]=useState<ArtifactDrafts>({});
  const [knowledgeTarget, setKnowledgeTarget] = useState<ChatEvidence | null>(null);
  const [chatMemory, updateChatMemory] = useState<ChatMemory>({drafts:{},pending:{}});
  const chatMemoryRef=useRef(chatMemory);
  const recoveryReady=useRef(false);
  const [chatReady,setChatReady]=useState(!draftSession);
  const alive=useRef(true);
  useLayoutEffect(()=>{alive.current=true;return()=>{alive.current=false;};},[]);
  const threadIds=useRef<string[]>([]);
  function persist() {
    if(alive.current&&recoveryReady.current)draftSession?.save(chatMemoryRef.current,selectedRef.current,threadIds.current);
  }
  function setChatMemory(next:SetStateAction<ChatMemory>) {
    if(!alive.current)return;
    const memory=typeof next==="function"?next(chatMemoryRef.current):next;
    chatMemoryRef.current=memory;
    updateChatMemory(memory);
    persist();
  }
  function setSelected(next:SetStateAction<string|null>) {
    if(!alive.current)return;
    const id=typeof next==="function"?next(selectedRef.current):next;
    selectedRef.current=id;
    updateSelected(id);
    persist();
  }
  const selectedRef = useRef(selected);
  const pathRef = useRef(path);
  pathRef.current = path;
  const listController = useRef<AbortController | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [creating, setCreating] = useState(false);
  const creationInFlight = useRef<Promise<ThreadSummary> | null>(null);
  const [drawer, setDrawer] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const searchOpener = useRef<HTMLElement | null>(null);
  const [deleting, setDeleting] = useState<ThreadSummary | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const deletePending = useRef<{ id: string; controller: AbortController } | null>(null);
  const historyOpener = useRef<HTMLButtonElement>(null);
  const pageHeading = useRef<HTMLHeadingElement>(null);
  const [navigationFocus, setNavigationFocus] = useState(0);
  function navigate(destination: string) {
    // Only deliberate navigation moves focus. Saved titles, streamed answers and
    // the implicit creation of a sent conversation must not interrupt typing.
    setNavigationFocus(value => value + 1);
    navigateParent(destination);
  }
  function returnFromMail() {
    const mission = new URLSearchParams(window.location.search).get("mission_return");
    if (mission && /^[a-f0-9-]{36}$/i.test(mission)) {
      navigate(`/app/missions#mission-${mission}`);
      return;
    }
    const thread = mailReturnThread(session.user.id);
    clearMailReturn(session.user.id);
    if (thread && threads.some(value => value.id === thread)) setSelected(thread);
    navigate("/app");
  }
  useEffect(() => {
    if (navigationFocus > 0) pageHeading.current?.focus({ preventScroll: true });
  }, [navigationFocus]);
  useEffect(() => {
    let live = true;
    const resumed = (event: Event) => {
      const target = gwsConnectedTarget(event, session.user.id);
      if (!target) return;
      // A delayed connection must not take the owner away from another task.
      const canReturn = () => pathRef.current === "/settings/mail"
        || pathRef.current === "/admin/google"
        || ((pathRef.current === "/app" || pathRef.current === "/") && selectedRef.current === target.threadId);
      if (!canReturn()) return;
      void (async () => {
        const owned = threadIds.current.includes(target.threadId)
          || (await scope.request<ThreadsResponse>("/threads")).threads.some(thread => thread.id === target.threadId);
        if (!live || !owned || !canReturn()) return;
        clearMailReturn(session.user.id);
        setSelected(target.threadId);
        setRetry(value => value + 1);
        navigateParent("/app");
      })().catch(fail);
    };
    window.addEventListener(GWS_CONNECTED, resumed);
    return () => { live = false; window.removeEventListener(GWS_CONNECTED, resumed); };
  }, [scope, session.user.id, navigateParent]);
  const deleteOpener = useRef<HTMLButtonElement | null>(null);
  const allowedAdmin = session.user.role === "admin";
  const admin = path === "/admin" || path.startsWith("/admin/");
  const telegramAdmin = path === "/admin/telegram";
  const personalTelegram = path === "/settings/telegram";
  const googleAdmin = path === "/admin/google";
  const adminUsage = path === "/admin/usage";
  const personalUsage = path === "/settings/usage";
  const knowledgePage = path === "/app/knowledge";
  const memoryPage = path === "/app/memory";
  const knowledgeOpen = knowledgePage || memoryPage;
  const providerAdmin = path === "/admin/providers";
  const personalModel = /^\/settings\/(model|claude|chatgpt|compatible-providers)(?:\/|$)/.test(path);
  const denied = !allowedAdmin && (admin || personalModel);
  const calendarPage = path === "/settings/calendar";
  const nativeCredentials = path === "/settings/credentials";
  const personalMail = path === "/settings/mail";
  const personalSources = path === "/settings/sources";
  const communitySkills = path === "/app/skills";
  const goalPage = path === "/app/goals" || path.startsWith("/app/goals/");
  const libraryPage = path === "/app/library";
  const feedPage = path === "/app/feed";
  const artifactPage = goalPage || path === "/app/artifacts" || path.startsWith("/app/artifacts/");
  const missionsPage = path === "/app/missions";
  const schedulesPage = path === "/app/schedules" || feedPage;
  const chatPage = ![admin, personalUsage, knowledgeOpen, personalModel, calendarPage,
    nativeCredentials, personalMail, personalSources, personalTelegram, communitySkills, artifactPage,
    missionsPage, schedulesPage, libraryPage].some(Boolean);
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
        threadIds.current=result.threads.map(t=>t.id);
        setThreads(result.threads);
        if(!recoveryReady.current) {
          const recovered=draftSession?.load(threadIds.current);
          recoveryReady.current=true;
          const linkedThread=new URLSearchParams(window.location.search).get("thread")
            ?? ((window.location.pathname === "/settings/mail" || window.location.pathname === "/admin/google") ? mailReturnThread(session.user.id) : null);
          if(linkedThread && threadIds.current.includes(linkedThread)) {
            if(recovered?.restored)setChatMemory(recovered.memory);
            setSelected(linkedThread);
          }else if(recovered?.restored) {
            setChatMemory(recovered.memory);
            setSelected(recovered.selected??(recovered.memory.drafts.new||recovered.memory.pending.new||recovered.memory.threadCreation?null:result.threads[0]?.id??null));
          }else setSelected(result.threads[0]?.id??null);
        }else {
          const allowed=new Set(["new",...threadIds.current]);
          setChatMemory(current=>({...current,drafts:Object.fromEntries(Object.entries(current.drafts).filter(([id])=>allowed.has(id))),pending:Object.fromEntries(Object.entries(current.pending).filter(([id])=>allowed.has(id))),...(current.instructions?{instructions:Object.fromEntries(Object.entries(current.instructions).filter(([id])=>allowed.has(id)))}:{})}));
          setSelected(current=>result.threads.some(t=>t.id===current)?current:current===null&&(chatMemoryRef.current.drafts.new||chatMemoryRef.current.pending.new||chatMemoryRef.current.threadCreation)?null:result.threads[0]?.id??null);
        }
        persist();
        setChatReady(true);
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
  function moveNewMemory(current: ChatMemory, id: string): ChatMemory {
    const pending = { ...current.pending };
    const drafts = { ...current.drafts };
    if (pending.new) pending[id] = pending.new;
    if (drafts.new !== undefined && !drafts[id]) drafts[id] = drafts.new;
    delete pending.new;
    delete drafts.new;
    return { ...current, pending, drafts };
  }
  function obtainThread(): Promise<ThreadSummary> {
    if (creationInFlight.current) return creationInFlight.current;
    const value = { ...(chatMemoryRef.current.threadCreation ?? {
      request_id: crypto.randomUUID(), title: "New conversation",
    }), status: "sending" as const };
    setChatMemory(current => ({ ...current, threadCreation: value }));
    setCreating(true);
    setError("");
    listController.current?.abort();
    const operation = (async () => {
      try {
        const thread = await scope.request<ThreadSummary>("/threads", "POST", {
          title: value.title, request_id: value.request_id,
        }, undefined, { timeoutMs: 30000 });
        const memory = chatMemoryRef.current;
        if ((memory.drafts.new && memory.drafts[thread.id]) ||
            (memory.pending.new && memory.pending[thread.id] && memory.pending.new.request_id !== memory.pending[thread.id].request_id))
          throw new ApiFailure(409, "thread_creation_busy");
        threadIds.current = [thread.id, ...threadIds.current.filter(id => id !== thread.id)];
        setThreads(current => [thread, ...current.filter(item => item.id !== thread.id)]);
        // Persist receipt completion and migration together. A reload must never
        // strand the original message under an unknown new-thread slot.
        setChatMemory(current => current.threadCreation?.request_id === value.request_id
          ? { ...moveNewMemory(current, thread.id), threadCreation: undefined }
          : current);
        return thread;
      } catch (error) {
        const definite = error instanceof ApiFailure && error.status < 500 && error.code !== "thread_creation_busy";
        setChatMemory(current => current.threadCreation?.request_id === value.request_id
          ? { ...current, threadCreation: definite ? undefined : { ...value, status: "uncertain" } }
          : current);
        if (!isAborted(error) && !(error instanceof ApiFailure && error.code === "thread_creation_busy"))
          setRetry(current => current + 1);
        throw error;
      } finally {
        creationInFlight.current = null;
        setCreating(false);
      }
    })();
    creationInFlight.current = operation;
    return operation;
  }
  async function createThread() {
    const origin = selectedRef.current;
    const originPath = pathRef.current;
    try {
      const thread = await obtainThread();
      if (selectedRef.current === origin && pathRef.current === originPath) {
        setSelected(thread.id);
        setDrawer(false);
        navigate("/app");
      }
    } catch (error) {
      if (error instanceof ApiFailure && error.code === "thread_creation_busy")
        setError("새 대화에 다른 초안이나 접수 확인 중인 메시지가 있습니다. 그 대화를 먼저 마무리한 뒤 같은 생성 요청을 다시 확인해 주세요.");
      else fail(error);
    }
  }
  async function createForChat(): Promise<string> {
    const thread = await obtainThread();
    if (selectedRef.current === null && (pathRef.current === "/app" || pathRef.current === "/"))
      setSelected(thread.id);
    return thread.id;
  }
  function removeMemory(id: string) {
    setChatMemory((current) => {
      const drafts = { ...current.drafts };
      const pending = { ...current.pending };
      delete drafts[id];
      delete pending[id];
      const instructions = { ...current.instructions }; delete instructions[id];
      return { ...current, drafts, pending, ...(current.instructions ? {instructions} : {}) };
    });
  }
  function threadMissing(id: string, selectNext = false) {
    const pending = deletePending.current;
    if (pending?.id === id) {
      deletePending.current = null;
      setDeleteBusy(false);
      pending.controller.abort();
    }
    // Fence an inventory snapshot started before this confirmed removal.
    listController.current?.abort();
    setLoading(false);
    threadIds.current = threadIds.current.filter(current => current !== id);
    setThreads(current => current.filter(thread => thread.id !== id));
    setSelected(current => current === id ? selectNext ? threadIds.current[0] ?? null : null : current);
    removeMemory(id);
    setDeleting(current => current?.id === id ? null : current);
  }
  async function removeThread() {
    if (!deleting || deletePending.current) return;
    const target = deleting;
    const owner = { id: target.id, controller: new AbortController() };
    deletePending.current = owner;
    setDeleteBusy(true);
    setError("");
    function confirmed() {
      threadMissing(target.id, true);
      setNavigationFocus(value => value + 1);
    }
    try {
      await scope.request(`/threads/${encodeURIComponent(target.id)}`, "DELETE", {}, owner.controller.signal, { timeoutMs: 30000 });
      if (deletePending.current !== owner) return;
      confirmed();
    } catch (error) {
      if (deletePending.current !== owner || isAborted(error)) return;
      if (error instanceof ApiFailure && error.status === 401) { fail(error); return; }
      if (error instanceof ApiFailure && error.status === 404) { confirmed(); return; }
      // A missing DELETE receipt may already be committed. Check only inventory;
      // never infer definite failure or replay the write automatically.
      try {
        const inventory = await scope.request<ThreadsResponse>("/threads", "GET", undefined, owner.controller.signal);
        if (deletePending.current !== owner) return;
        if (!inventory.threads.some(thread => thread.id === target.id)) confirmed();
        else setError("삭제 접수를 확인하지 못했습니다. 대화는 목록에 남아 있습니다. 자동으로 다시 삭제하지 않습니다.");
      } catch (reconcileError) {
        if (!isAborted(reconcileError)) {
          fail(reconcileError);
          if (!(reconcileError instanceof ApiFailure && reconcileError.status === 401))
            setError("삭제 결과와 대화 목록을 확인하지 못했습니다. 자동으로 다시 삭제하지 않습니다. 잠시 뒤 기록을 확인해 주세요.");
        }
      }
    } finally {
      if (deletePending.current === owner) {
        deletePending.current = null;
        setDeleteBusy(false);
      }
    }
  }
  const current = threads.find((t) => t.id === selected);
  function openDestination(destination: string) {
    setDrawer(false);
    setSearchOpen(false);
    navigate(destination);
  }
  function openThread(id: string) {
    setSelected(id);
    openDestination("/app");
  }
  function openSearch(event: React.MouseEvent) {
    searchOpener.current = drawer ? historyOpener.current : event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    setDrawer(false);
    setSearchOpen(true);
  }
  useEffect(() => {
    function shortcut(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && !event.altKey && event.key.toLowerCase() === "k" && !deleting) {
        event.preventDefault();
        searchOpener.current = drawer ? historyOpener.current : document.activeElement instanceof HTMLElement ? document.activeElement : null;
        setDrawer(false);
        setSearchOpen(value => !value);
      }
    }
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, [deleting, drawer]);
  const primary = [
    {label:"채팅", path:"/app", active:chatPage, icon:"chat"},
    {label:"소식함", path:"/app/feed", active:feedPage, icon:"feed"},
    {label:"아이디어", path:"/app/skills", active:communitySkills, icon:"idea"},
    {label:"목표", path:"/app/goals", active:goalPage, icon:"goal"},
    {label:"보관함", path:"/app/library", active:libraryPage, icon:"library"},
  ] as const;
  const workLinks = [
    {label:"맡긴 일", path:"/app/missions", active:missionsPage},
    {label:"예약 · 소식", path:"/app/schedules", active:schedulesPage && !feedPage},
    {label:"문서 · 목표 · 요청", path:"/app/artifacts", active:artifactPage && !goalPage},
    {label:"자료", path:"/app/knowledge", active:knowledgePage},
    {label:"기억", path:"/app/memory", active:memoryPage},
  ];
  const connections = [
    ...(allowedAdmin ? [{label:"내 모델", path:"/settings/model", active:personalModel}] : []),
    {label:"내 Calendar", path:"/settings/calendar", active:calendarPage},
    {label:"내 Gmail · 발송 확인", path:"/settings/mail", active:personalMail},
    {label:"내 Telegram", path:"/settings/telegram", active:personalTelegram},
    {label:"자료 수집 연결", path:"/settings/sources", active:personalSources},
    {label:"신뢰 기기 credential · 실험", path:"/settings/credentials", active:nativeCredentials},
    {label:"사용량", path:"/settings/usage", active:personalUsage},
    ...(allowedAdmin ? [{label:admin ? "← Back to your space" : "Manage installation", path:admin ? "/app" : "/admin", active:admin}] : []),
  ];
  function links(items: typeof workLinks) {
    return items.map(item => <Button key={item.label} label={item.label} variant="ghost" width="100%"
      className={`nav-link ${item.active ? "chosen" : ""}`} aria-current={item.active ? "page" : undefined}
      onClick={() => openDestination(item.path)}/>);
  }
  const history = (
    <>
        <div className="workspace-shortcuts">
        <button className="new-chat" onClick={createThread} disabled={creating || loading}>
          <WorkspaceIcon name="new"/>
          {creating ? "만드는 중…" : chatMemory.threadCreation ? "새 대화 다시 확인" : "새 대화"}
        </button>
        <Button variant="ghost" className="workspace-search-button" onClick={openSearch} label="대화 검색" icon={<WorkspaceIcon name="search"/>} isIconOnly/>
      </div>
      {chatMemory.threadCreation?.status === "uncertain" && <p className="sidebar-hint" role="status">새 대화 생성 접수를 확인하지 못했습니다. 다시 확인할 때 같은 요청을 사용합니다.</p>}
      <nav className="workspace-mobile-destinations" aria-label="주 메뉴">
        {primary.map(item => <SideNavItem key={item.path} label={item.label} isSelected={item.active}
          icon={<WorkspaceIcon name={item.icon}/>} onClick={() => openDestination(item.path)}/>)}
      </nav>
      <div className="workspace-sidebar-scroll">
        <div className="history-title">최근 대화</div>
        <nav className="thread-list" aria-label="Conversations">
          {loading ? <p className="sidebar-hint" role="status">Loading conversations…</p>
            : threads.length === 0 ? <p className="sidebar-hint">아직 대화가 없습니다.</p>
            : threads.slice(0, 12).map(thread => (
              <div className={`thread-row ${thread.id === selected && chatPage ? "active" : ""}`} key={thread.id}>
                <button className="thread-select" onClick={() => openThread(thread.id)}
                  aria-current={thread.id === selected && chatPage ? "page" : undefined}><span>{thread.title}</span></button>
                <button className="thread-delete" disabled={deleteBusy} aria-label={`Delete ${thread.title}`}
                  onClick={event => { deleteOpener.current = event.currentTarget; setDeleting(thread); setDrawer(false); }}>×</button>
              </div>
            ))}
        </nav>
        {threads.length > 12 && <button className="nav-link" onClick={openSearch}>대화 모두 찾기</button>}
        <div className="history-title">작업 공간</div>
        <nav className="workspace-links" aria-label="작업 공간">{links(workLinks)}</nav>
        <div className="history-title">연결 및 설정</div>
        <nav className="workspace-links" aria-label="연결 및 설정">{links(connections)}</nav>
      </div>
      <div className="sidebar-bottom">
        <PwaInstallButton />
        <div className="account">
          <span className="avatar">{session.user.name.slice(0, 1).toUpperCase()}</span>
          <span className="account-name">{session.user.name}<small>{session.user.email}</small></span>
          <VaultLaunch accountId={session.user.id}/>
          <button className="icon-button" title="Sign out" aria-label="Sign out" onClick={onLogout}>↪</button>
        </div>
      </div>
    </>
  );
  return (
    <div className="workspace" ref={workspace}>
      <AppShell className="workspace-shell" variant="wash" height="fill" contentPadding={0}
        mobileNav={{hasToggle: false, isOpen: false}}
        sideNav={<SideNav className="workspace-rail" aria-label="주 메뉴">
          <div className="workspace-rail-items">
            {primary.map((item, index) => <div className="workspace-rail-entry" key={item.path}>
              {index === 1 && <SideNavItem className="workspace-rail-item" label="대화 검색"
                icon={<WorkspaceIcon name="search"/>} onClick={openSearch}/>}
              <SideNavItem className="workspace-rail-item" label={item.label} isSelected={item.active}
                icon={<WorkspaceIcon name={item.icon}/>} onClick={() => openDestination(item.path)}/>
            </div>)}
          </div>
        </SideNav>}
        topNav={<TopNav className="work-header" label="대화와 계정"
          heading={<div className="work-title">
            <button className="icon-button workspace-account-toggle" ref={historyOpener} aria-label="Open conversations"
              aria-expanded={drawer} title="대화 및 설정" onClick={() => setDrawer(true)}>
              <img src="/moosuri-icon-192.png" width="32" height="32" alt="" />
            </button>
            <h1 id="workspace-heading" ref={pageHeading} tabIndex={-1}>
              {denied ? "관리자 전용" : personalTelegram ? "내 Telegram" : personalMail ? "내 Gmail" : nativeCredentials ? "신뢰 기기 연결" : calendarPage ? "내 Calendar" : missionsPage ? "맡긴 일" : feedPage ? "소식함" : schedulesPage ? "예약 · 소식" : libraryPage ? "보관함" : goalPage ? "목표" : artifactPage ? "문서 · 목표 · 요청" : communitySkills ? "아이디어" : personalSources ? "자료 수집 연결" : knowledgeOpen
                ? memoryPage
                  ? "기억"
                  : "자료"
                : admin
                  ? telegramAdmin
                    ? "Telegram 봇 설정"
                    : googleAdmin
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
                      : (current?.title ?? "새 대화")}
            </h1>
          </div>}
          endContent={<div className="work-header-actions">
            <Button variant="ghost" className="workspace-mobile-search" label="대화 검색" isIconOnly
              icon={<WorkspaceIcon name="search"/>} onClick={openSearch}/>
            <Button variant="ghost" label={chatMemory.threadCreation ? "새 대화 다시 확인" : "새 대화"} isIconOnly
              icon={<WorkspaceIcon name="new"/>} isDisabled={creating || loading} onClick={createThread}/>
          </div>}/>}
      >
      <section className="work-area">
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
              aria-current={!providerAdmin && !adminUsage && !googleAdmin && !telegramAdmin ? "page" : undefined}
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
            <button aria-current={telegramAdmin ? "page" : undefined} onClick={() => navigate("/admin/telegram")}>Telegram 봇</button>
          </nav>
        )}
        <DeferredPage key={path} back={() => navigate("/app")}>
        {denied ? (
          <div className="empty-state">
            <h2>This page is for administrators.</h2>
            <p>You can continue in your own space.</p>
            <button className="primary" onClick={() => navigate("/app")}>
              Back to conversations
            </button>
          </div>
        ) : missionsPage ? (
          <Missions accountId={session.user.id} onExpired={onExpired} canManageModel={allowedAdmin} back={() => navigate("/app")}
            instructionDrafts={chatMemory.missionInstructions}
            onInstruction={(id, change) => setChatMemory(current => ({
              ...current, missionInstructions: {
                ...current.missionInstructions,
                [id]: change(current.missionInstructions?.[id] ?? emptyInstructions()),
              },
            }))}/>
        ) : schedulesPage ? (
          <Schedules accountId={session.user.id} onExpired={onExpired} feedOnly={feedPage}
            manageSchedules={() => navigate("/app/schedules")} back={() => navigate("/app")}
            openMission={id => navigate(`/app/missions#mission-${id}`)}
            openArtifact={id => navigate(`/app/artifacts/${id}`)}/>
        ) : libraryPage ? (
          <section className="workspace-library admin-scroll" aria-label="내 보관함">
            <div className="workspace-library-tabs">
              <Button label="문서" onClick={() => { setArtifactKind("document"); navigate("/app/artifacts"); }}/>
              <Button label="자료" onClick={() => navigate("/app/knowledge")}/>
              <Button label="기억" onClick={() => navigate("/app/memory")}/>
            </div>
            <h2>이미지와 파일</h2>
            <MediaLibraryView accountId={session.user.id} standalone/>
          </section>
        ) : artifactPage ? (
          <Artifacts accountId={session.user.id} initialKind={goalPage ? "goal" : artifactKind}
            collectionPath={goalPage ? "/app/goals" : "/app/artifacts"} onKind={setArtifactKind}
            id={path.split("/")[3]} drafts={artifactDrafts} updateDrafts={setArtifactDrafts}
            navigate={navigate} onExpired={onExpired}
            openReference={reference => {
              if (reference.kind === "artifact") {
                navigate(`/app/artifacts/${reference.id}`);
              } else {
                setKnowledgeTarget({kind:reference.kind, id:reference.id, revision:reference.revision, title:"기록의 근거"});
                navigate(reference.kind === "source" ? "/app/knowledge" : "/app/memory");
              }
            }}
            revise={(id, revision, title) => {
              setArtifactTarget({id, revision, title});
              setSelected(null);
              setChatMemory(current => ({
                ...current, drafts: {
                  ...current.drafts,
                  new: `선택한 ${namesForArtifact(goalPage ? "goal" : artifactKind)}를 수정해 줘. ${current.drafts.new ?? ""}`,
                },
              }));
              navigate("/app");
            }}/>
        ) : personalSources ? (
          <SourceConnections accountId={session.user.id} onExpired={onExpired} back={() => navigate("/app")} />
        ) : personalTelegram ? <TelegramSettings accountId={session.user.id} onExpired={onExpired}/> : calendarPage ? <Calendar accountId={session.user.id} onExpired={onExpired} back={()=>navigate("/app")} setup={allowedAdmin?()=>navigate("/admin/google"):undefined}/> : communitySkills ? <CommunitySkills accountId={session.user.id} onExpired={onExpired} back={()=>navigate("/app")} connect={()=>navigate("/settings/mail")} start={(name)=>{
          setSelected(null);
          setChatMemory(current => ({
            ...current,
            drafts: {
              ...current.drafts,
              new: `${name} 절차로 도와줘: ${current.drafts.new ?? ""}`,
            },
          }));
          navigate("/app");
        }}/> : nativeCredentials ? <NativeCredentials key={session.user.id} accountId={session.user.id} onExpired={onExpired} back={()=>{const id=new URLSearchParams(window.location.search).get("mission_return");navigate(id&&/^[a-f0-9-]{36}$/i.test(id)?`/app/missions#mission-${id}`:"/app")}}/> : personalMail ? <MailConnections accountId={session.user.id} onExpired={onExpired} back={returnFromMail} originThreadId={selected} openGoogleSetup={allowedAdmin ? () => { rememberMailReturn(session.user.id, selected); navigate("/admin/google"); } : undefined}/> : knowledgeOpen ? (
          <KnowledgeView
            accountId={session.user.id}
            mode={memoryPage ? "memory" : "source"}
            target={knowledgeTarget}
            onTargetConsumed={() => setKnowledgeTarget(null)}
            drafts={knowledgeDrafts}
            updateDrafts={setKnowledgeDrafts}
            onExpired={onExpired}
            canManageModel={allowedAdmin}
          />
        ) : personalUsage ? (
          <UsageView accountId={session.user.id} onExpired={onExpired} />
        ) : personalModel ? (
          <PersonalModelSettings
            accountId={session.user.id}
            onExpired={onExpired}
            onConnectClaudeApi={() => navigate("/admin/providers?connect=anthropic")}
          />
        ) : admin ? (
          telegramAdmin ? (
            <TelegramSettings accountId={session.user.id} admin onExpired={onExpired} />
          ) : googleAdmin ? (
            <GoogleSetup accountId={session.user.id} onExpired={onExpired} back={returnFromMail} />
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
        ) : !chatReady ? <p role="status">Checking your conversations…</p> : (
          <ChatPanel
            openCredentials={()=>navigate("/settings/credentials")}
            accountId={session.user.id}
            shareScope={draftScope(session)}
            artifactTarget={artifactTarget}
            clearArtifactTarget={()=>setArtifactTarget(null)}
            openArtifact={id=>navigate(`/app/artifacts/${id}`)}
            name={session.user.name}
            threadId={selected}
            memory={chatMemory}
            onInstructions={(slot, change) => setChatMemory(current => slot !== "new" && !threadIds.current.includes(slot) ? current : ({
              ...current, instructions: { ...current.instructions, [slot]: change(current.instructions?.[slot] ?? emptyInstructions()) },
            }))}
            onDraft={(slot, text) =>
              setChatMemory((current) => slot !== "new" && !threadIds.current.includes(slot) ? current : ({
                ...current,
                drafts: { ...current.drafts, [slot]: text },
              }))
            }
            onPending={(slot, value, expected) =>
              setChatMemory((current) => {
                if (value && slot !== "new" && !threadIds.current.includes(slot)) return current;
                if (expected && current.pending[slot]?.request_id !== expected)
                  return current;
                const pending = { ...current.pending };
                if (value) pending[slot] = value;
                else delete pending[slot];
                return { ...current, pending };
              })
            }
            onRestoreDraft={(slot, text) =>
              setChatMemory((current) => slot !== "new" && !threadIds.current.includes(slot) ? current : ({
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
                return moveNewMemory(current, id);
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
            openSettings={allowedAdmin ? () => navigate("/settings/model") : undefined}
            onOpenEvidence={(reference) => {
              setKnowledgeTarget(reference);
              navigate(reference.kind === "source" ? "/app/knowledge" : "/app/memory");
            }}
          />
        )}
        </DeferredPage>
      </section>
      </AppShell>
      {drawer && (
        <Modal
          title={<Brand />}
          side
          returnFocus={historyOpener.current}
          close={() => setDrawer(false)}
        >
          <div className="workspace-history">{history}</div>
        </Modal>
      )}
      {searchOpen && <Modal title="대화 검색" returnFocus={searchOpener.current} fallbackFocus={pageHeading.current} close={() => setSearchOpen(false)}>
        <ConversationSearch threads={threads} openThread={openThread} openSources={() => openDestination("/app/knowledge")}/>
      </Modal>}
      {deleting && (
        <Modal
          title="Delete conversation?"
          returnFocus={deleteOpener.current}
          fallbackFocus={pageHeading.current}
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
  const [modelMember, setModelMember] = useState<string | null>(null);
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
    <section className="admin-scroll">
      <div className="admin-intro">
        <h2>사용자 관리</h2>

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
          Creating an account sends no email. Optional email delivery needs a separate final preview approval. The member must change their password on first
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
                  {management.find((m)=>m.user.id===user.id)?.deletion ? "Deleted" : management.find((m)=>m.user.id===user.id)?.recovery_required ? "Recovery needed" : management.find((m)=>m.user.id===user.id)?.status==="suspended" ? "Suspended" : user.must_change_password
                    ? "Password change needed"
                    : "Active"}
                </span>
                {user.role === "member" && !management.find((m) => m.user.id === user.id)?.deletion && (
                  <details className="account-controls" open={modelMember === user.id}
                    onToggle={(event) => {
                      const open = event.currentTarget.open;
                      setModelMember(current => open ? user.id : current === user.id ? null : current);
                    }}>
                    <summary>사용할 모델 · 추론 강도</summary>
                    {modelMember === user.id && (
                      <DeferredPage back={() => setModelMember(null)}>
                        <MemberModelSettings accountId={accountId} memberId={user.id} onExpired={onExpired} />
                      </DeferredPage>
                    )}
                  </details>
                )}
                {management.filter((m)=>m.user.id===user.id).map((target)=><AccountControls key={target.user.id} accountId={accountId} target={target} onExpired={onExpired} onChanged={()=>setRetry(n=>n+1)} onPassword={(password,receipt)=>{setInvitation({user,temporary_password:password,owner_id:accountId,revision:receipt.revision,request_id:receipt.request_id,reset:true});setCopied(false);setCopyError("");}}/>)}
              </li>
            ))}
          </ul>
        )}
      </section>
      <InvitationEmail accountId={accountId} onExpired={onExpired}/>
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
          {invitation.revision!==null && <InvitationEmail key={`${invitation.user.id}:${invitation.revision}`} accountId={accountId} onExpired={onExpired} target={{account_id:invitation.user.id,revision:invitation.revision,temporary_password:invitation.temporary_password}}/>}
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
    </section>
  );
}

function namesForArtifact(kind:import("../shared/api-types").ArtifactKind){return kind==="document"?"문서":kind==="goal"?"목표":"요청 기록"}
