import { connectInKiri } from "./kiri-google-connection";
import { postVaultForm, trustedVaultOrigin } from "./VaultLaunch";
import { setupOpenedFromAdmin, type MemberRoute } from "./member-route";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { googleReturn, usePersonalGoogle } from "./PersonalGoogle";
import { isWorkspaceGoogleService, usePersonalGoogleWorkspace, workspaceGoogleReturn } from "./personal-google-workspace";
import { PersonalGoogleSetup, installationGoogleSetupPath } from "./PersonalGoogleSetup";
import { WorkspaceIcon } from "./WorkspaceNavigation";
import gmailMark from "./connector-marks/gmail.png?no-inline";
import driveMark from "./connector-marks/drive.png?no-inline";
import docsMark from "./connector-marks/docs.png?no-inline";
import slidesMark from "./connector-marks/slides.png?no-inline";
import sheetsMark from "./connector-marks/sheets.png?no-inline";
import "./personal-connections.css";

type Connector = { id: string; name: string; description: string; mark?: string; aliases?: readonly string[] };
const workspaceConnectors = [
  { id: "drive", name: "Google Drive", mark: driveMark, description: "파일 목록, 이름, 유형 조회", aliases: ["구글 드라이브"] },
  { id: "docs", name: "Google Docs", mark: docsMark, description: "지정한 Docs 문서의 제목 조회", aliases: ["구글 문서", "구글 독스", "Google Documents"] },
  { id: "slides", name: "Google Slides", mark: slidesMark, description: "지정한 프레젠테이션의 제목과 슬라이드 수 조회", aliases: ["구글 슬라이드", "구글 프레젠테이션", "Google Presentations"] },
  { id: "sheets", name: "Google Sheets", mark: sheetsMark, description: "지정한 스프레드시트의 제목과 탭 목록 조회", aliases: ["구글 시트", "구글 스프레드시트", "Google Spreadsheets"] },
] as const satisfies readonly Connector[];
const gmail: Connector = { id: "gmail", name: "Gmail", mark: gmailMark, description: "최근 받은 메일의 발신자, 제목, 수신일 조회", aliases: ["지메일", "구글 메일"] };
const vault: Connector = { id: "vault", name: "볼트", description: "볼트 웹에서 로그인 항목 관리" };
const connectors = [gmail, ...workspaceConnectors, vault];

type ConnectionStatus = "checking" | "ready" | "disabled" | "error";
type InstallationStatus = "checking" | "required" | "configured" | "error";
type StatusBadge = { label: string; tone?: "connected" | "attention" };
type ConnectionInfo = { badge: StatusBadge; message?: string };

function searchText(value: string) {
  return value.normalize("NFKC").toLocaleLowerCase().replace(/\s+/g, "");
}

function googleInfo(status: ConnectionStatus, mailbox: string | undefined, installation?: InstallationStatus): ConnectionInfo {
  if (status === "checking") return { badge: { label: "확인 중" }, message: "연결 상태를 확인하고 있습니다." };
  if (status === "error") return { badge: { label: "확인 실패", tone: "attention" }, message: "연결 상태를 확인할 수 없습니다. 다시 확인해 주세요." };
  if (status === "disabled") {
    if (installation === "required") return { badge: { label: "설치 설정 필요", tone: "attention" }, message: "Google 연결을 시작하려면 먼저 이 앱 안에서 Google Workspace 설치 설정을 마쳐 주세요." };
    if (installation === "configured") return { badge: { label: "연결 준비 중", tone: "attention" }, message: "이 회원의 Google 연결을 준비 중입니다. 잠시 후 다시 확인해 주세요." };
    return { badge: { label: "연결 불가", tone: "attention" }, message: "이 서버에서 Google 로그인을 아직 사용할 수 없습니다. 설치 관리자에게 문의해 주세요." };
  }
  return mailbox ? { badge: { label: "연결됨", tone: "connected" } }
    : { badge: { label: "미연결" }, message: "연결된 계정이 없습니다." };
}

function getVaultInfo(connection: VaultConnectionState): ConnectionInfo {
  if (connection.status === "checking") return { badge: { label: "확인 중" }, message: "볼트 주소를 확인하고 있습니다." };
  if (connection.status === "error") return { badge: { label: "확인 실패", tone: "attention" }, message: "연결 상태를 확인할 수 없습니다." };
  if (connection.status === "ready") return { badge: { label: "연결됨", tone: "connected" }, message: "볼트는 새 탭에서 열리며, 지금 로그인한 계정으로 바로 연결됩니다. 볼트 비밀번호(마스터 비밀번호)는 이 앱에 전달되지 않습니다." };
  return connection.siteOrigin
    ? { badge: { label: "별도 로그인" }, message: "볼트는 새 탭에서 열립니다. 볼트 계정으로 로그인하세요." }
    : { badge: { label: "볼트 주소 미설정", tone: "attention" }, message: "이 설치에 등록된 볼트 주소가 없습니다. 설치 관리자에게 문의해 주세요." };
}

export function PersonalConnections({
  onBack, installationAdmin = false, onAdminUnavailable, navigation, ...props
}: Parameters<typeof usePersonalGoogle>[0] & { navigation?: { route: MemberRoute; open: (route: MemberRoute, back?: boolean) => void }; onBack: () => void; installationAdmin?: boolean; onAdminUnavailable?: () => void }) {
  const [workspaceReturn] = useState(() => workspaceGoogleReturn(props.session));
  const [localService, setLocalService] = useState<Connector | null>(
    () => workspaceReturn ? workspaceConnectors.find(({ id }) => id === workspaceReturn.service) ?? null
      : props.readIntent || googleReturn() ? gmail : null,
  );
  const selected = navigation ? connectors.find(item => item.id === navigation.route.service) ?? null : localService;
  const connectScope=useScope(props.session.user.id, selected?.id);
  const active=useRef<AbortController|null>(null);
  const [connecting,setConnecting]=useState(false);
  const [kiriMessage,setKiriMessage]=useState("");
  useEffect(()=>{
    setConnecting(false);setKiriMessage("");
    return ()=>{active.current?.abort();active.current=null;};
  },[connectScope]);
  const workspaceEnabled = selected !== gmail;
  const google = usePersonalGoogle(props, !workspaceReturn);
  const workspace = usePersonalGoogleWorkspace({ session: props.session, expired: props.expired, enabled: workspaceEnabled, returned: workspaceReturn });
  const vaultStatus = useVaultConnection(props.session.user.id, props.expired);
  const [view, setView] = useState<"available" | "connected">("available");
  const [query, setQuery] = useState("");
  const [localSetup, setLocalSetup] = useState(false);
  const setupOpen = navigation ? !!navigation.route.setup : localSetup;
  function show(service: Connector | null, setup: boolean, back = false) {
    if (navigation) navigation.open({ view: "connections", service: service?.id, setup }, back);
    else {
      setLocalService(service);
      setLocalSetup(setup);
    }
  }
  const [installation, setInstallation] = useState<InstallationStatus>("checking");
  const scope = useScope(props.session.user.id, "connector-installation-status");
  const heading = useRef<HTMLHeadingElement>(null);
  const rows = useRef(new Map<string, HTMLButtonElement>());
  const focusDestination = useRef<string | null>(null);

  useLayoutEffect(() => {
    if (!focusDestination.current) return;
    const row = rows.current.get(focusDestination.current);
    (row ?? heading.current)?.focus();
    focusDestination.current = null;
  }, [selected, setupOpen]);

  useEffect(() => {
    if (!installationAdmin || setupOpen) return;
    const controller = new AbortController();
    void scope.request<{ configured: boolean }>(installationGoogleSetupPath, "GET", undefined, controller.signal)
      .then(result => { if (!controller.signal.aborted) setInstallation(result.configured ? "configured" : "required"); })
      .catch(error => {
        if (controller.signal.aborted || isAborted(error)) return;
        setInstallation("error");
        if (error instanceof ApiFailure && error.status === 403) onAdminUnavailable?.();
        if (error instanceof ApiFailure && error.status === 401) props.expired();
      });
    return () => controller.abort();
  }, [scope, installationAdmin, setupOpen, onAdminUnavailable, props.expired]);

  const refreshMemberServices = useRef(() => {});
  useEffect(() => {
    refreshMemberServices.current = () => {
      if (google.status === "disabled") google.retry();
      if (workspaceEnabled && workspace.status === "disabled") void workspace.retry();
    };
  });
  useEffect(() => {
    if (!installationAdmin || installation !== "configured" || setupOpen) return;
    // The member relay imports the shared configuration asynchronously.
    const timers = [0, 4000, 10000, 18000].map(delay => window.setTimeout(() => refreshMemberServices.current(), delay));
    return () => timers.forEach(clearTimeout);
  }, [installationAdmin, installation, setupOpen, workspaceEnabled]);

  const search = searchText(query);
  const matches = (connector: Connector) => [connector.name, connector.description, ...(connector.aliases ?? [])].some(
    (value) => searchText(value).includes(search),
  );
  function mailboxFor(connector: Connector) {
    return isWorkspaceGoogleService(connector.id) ? workspace.connections[connector.id]?.mailbox
      : connector.id === gmail.id ? google.connection?.mailbox : undefined;
  }
  function infoFor(connector: Connector) {
    if (connector.id === vault.id) return getVaultInfo(vaultStatus);
    return googleInfo(connector.id === gmail.id ? google.status : workspace.status, mailboxFor(connector), installationAdmin ? installation : undefined);
  }
  const connected = connectors.filter(connector => mailboxFor(connector) && infoFor(connector).badge.tone === "connected");
  const found = (view === "available" ? connectors : connected).filter(matches);
  const accountsChecking = google.status === "checking" || workspace.status === "checking";
  const accountsError = google.status === "error" || workspace.status === "error";

  function backFromSetup(configured = false) {
    focusDestination.current = "heading";
    if (configured) setInstallation("configured");
    show(selected, false, true);
  }
  if (installationAdmin && setupOpen) {
    return <PersonalGoogleSetup account={props.session.user.id} expired={props.expired}
      unavailable={() => { backFromSetup(); onAdminUnavailable?.(); }}
      onBack={() => !selected && setupOpenedFromAdmin(props.session) ? onBack() : backFromSetup()}
      onConnect={() => backFromSetup(true)}
      returnLabel={selected ? selected.name + " 연결로 돌아가기" : "커넥터 열기"} />;
  }

  function row(connector: Connector) {
    return <li key={connector.id}>
      <button type="button" className="member-library-row member-connector-row" ref={button => {
        if (button) rows.current.set(connector.id, button);
        else rows.current.delete(connector.id);
      }} onClick={() => {
        focusDestination.current = "heading";
        show(connector, false);
      }}>
        <ConnectorMark mark={connector.mark} />
        <span className="member-library-row-copy">
          <strong>{connector.name}</strong>
          <span>{mailboxFor(connector) ?? connector.description}</span>
        </span>
        <Badge badge={infoFor(connector).badge} />
        <span aria-hidden="true" className="member-library-chevron" />
      </button>
    </li>;
  }

  function group(title: string, entries: Connector[]) {
    if (entries.length === 0) return null;
    return <section className="member-connector-group" aria-label={title}>
      <h2>{title}</h2>
      <ul className="member-page-list member-library-list">{entries.map(row)}</ul>
    </section>;
  }

  const workspaceService = selected && isWorkspaceGoogleService(selected.id) ? selected.id : null;
  const gmailRead = selected?.id === gmail.id && Boolean(props.readIntent);
  const memberFlow = selected?.id === gmail.id ? google : workspace;
  async function connect(read=false) {
    if (active.current || !selected || vaultStatus.status!=="ready") return;
    const controller=new AbortController();active.current=controller;setConnecting(true);
    try {
      const result=await connectInKiri(connectScope.request,props.session.user.id,props.session.user.email,selected.id,vaultStatus.siteOrigin,
        read ? props.readIntent??undefined : undefined,controller.signal,setKiriMessage);
      if(controller.signal.aborted)return;
      const prefix="/personal/google"+(workspaceService?"/"+workspaceService:"");
      const grant=await connectScope.request<{flow_id:string;mailbox:string;continuation?:{thread_id:string;request_id:string}}>(
        prefix+"/complete","POST",{handle:result.handle},controller.signal,{timeoutMs:75000});
      if(grant.flow_id!==result.flow || grant.mailbox.toLowerCase()!==props.session.user.email.toLowerCase()) throw Error("Google 연결 계정을 확인하지 못했습니다.");
      setKiriMessage("Google 서비스 연결을 확인했습니다.");
      if(workspaceService) await workspace.retry();
      else if(read && props.readIntent && props.readIntent.thread_id===grant.continuation?.thread_id && props.readIntent.request_id===grant.continuation.request_id)
        await google.refreshRead(props.readIntent);
      else google.retry();
    } catch(error) {
      if(!controller.signal.aborted) {
        if(error instanceof ApiFailure && error.status===401)props.expired();
        else setKiriMessage(error instanceof ApiFailure && error.status===409?"이 계정에 진행 중인 Kiri 연결이 있습니다.":
          error instanceof ApiFailure && error.status===429?"최근 Google 연결 시도가 많습니다. 잠시 후 다시 시도해 주세요.":
          error instanceof Error && !(error instanceof ApiFailure)?error.message:"Google 연결 상태를 확인하지 못했습니다.");
      }
    } finally {if(active.current===controller){active.current=null;setConnecting(false);}}
  }
  const mailbox = selected ? mailboxFor(selected) : undefined;
  const notice = selected?.id === gmail.id ? google.notice
    : workspaceService && workspace.notice?.service === workspaceService ? workspace.notice.message : undefined;

  return (
    <div className="member-page member-library member-connections connector-page">
      <header className="member-page-heading connector-heading">
        <button type="button" className="member-library-icon-button" aria-label="뒤로" onClick={() => {
          if (selected && !props.readIntent) {
            focusDestination.current = selected.id;
            show(null, false, true);
          } else onBack();
        }}><WorkspaceIcon name="back" /></button>
        <h1 ref={heading} tabIndex={-1}>{selected?.name ?? "커넥터"}</h1>
      </header>
      {selected ? (
        <ConnectorDetail connector={selected} info={infoFor(selected)} mailbox={mailbox}
          busy={selected.id !== vault.id && memberFlow.busy && memberFlow.status === "ready"}
          notice={notice}>
          {selected.id === vault.id ? (
            vaultStatus.status === "ready" ? (
              <>
                <button type="button" className="member-page-action connector-primary" disabled={vaultStatus.busy} onClick={() => void vaultStatus.open()}>볼트 열기</button>
                {vaultStatus.failure && <p className="connector-notice" role="status">{vaultStatus.failure}</p>}
              </>
            ) : vaultStatus.siteOrigin ? (
              <a className="member-page-action connector-primary" href={vaultStatus.siteOrigin} target="_blank" rel="noopener noreferrer">볼트 열기</a>
            ) : vaultStatus.status === "error" && (
              <button type="button" className="member-page-action" onClick={vaultStatus.retry}>다시 확인</button>
            )
          ) : (
            <>
              {mailbox ? <>
                {gmailRead && <button className="member-page-action connector-primary" disabled={memberFlow.busy} onClick={()=>void google.action("read")}>메일 확인</button>}
                <button className="member-page-action" disabled={memberFlow.busy} onClick={()=>{
                  if(workspaceService)void workspace.action(workspaceService,"disconnect");else void google.action("disconnect");
                }}>연결 해제</button>
              </> : memberFlow.status==="ready" ? <button className="member-page-action connector-primary" disabled={connecting || vaultStatus.status!=="ready"}
                onClick={()=>void connect(gmailRead)}>{gmailRead?"Google 연결하고 메일 확인":"Google 연결"}</button>
                : installationAdmin && memberFlow.status==="disabled" ? <button className="member-page-action connector-primary"
                  onClick={()=>show(selected,true)}>Google Workspace 설정</button>
                : memberFlow.status!=="checking" && <button className="member-page-action"
                  onClick={()=>{if(workspaceService)void workspace.retry();else google.retry();}}>다시 확인</button>}
              {!mailbox && memberFlow.status==="ready" && vaultStatus.status!=="ready" && <p role="status">{getVaultInfo(vaultStatus).message}</p>}
              {kiriMessage && <p role="status">{kiriMessage}</p>}
              {connecting && <button className="member-page-action" onClick={()=>{active.current?.abort();setKiriMessage("연결을 취소했습니다.");}}>취소</button>}
              {vaultStatus.failure && <p className="connector-notice" role="status">{vaultStatus.failure}</p>}
            </>
          )}
        </ConnectorDetail>
      ) : (
        <>
          {installationAdmin && <section className="member-connector-group" aria-label="Google Workspace 설치 설정">
            <h2>Google Workspace 설치 설정</h2>
            <p>{installation==="configured"?"설치 설정 완료":installation==="required"?"설치 설정 필요":installation==="error"?"설치 설정 확인 실패":"설치 설정 확인 중"}</p>
            <button type="button" className="member-page-action" disabled={installation==="checking"}
              onClick={()=>show(null,true)}>Google Workspace 설정</button>
          </section>}
          <div className="connector-controls">
            <label className="member-library-filter">
              <WorkspaceIcon name="search" />
              <input type="search" value={query} onChange={(event) => setQuery(event.target.value)}
                placeholder="커넥터 검색" aria-label="커넥터 검색" />
            </label>
            <div className="member-connection-switch" role="group" aria-label="서비스 목록">
              <button type="button" aria-pressed={view === "available"} onClick={() => setView("available")}>전체</button>
              <button type="button" aria-pressed={view === "connected"} onClick={() => setView("connected")}>연결됨</button>
            </div>
          </div>
          {workspace.notice?.service === null && <p className="connector-notice" role="status">{workspace.notice.message}</p>}
          {view === "connected" && (accountsChecking || accountsError) && (
            <p className="connector-notice" role="status">
              {accountsChecking ? "연결된 서비스를 확인 중…" : "일부 Google 서비스의 연결 상태를 확인할 수 없습니다. 전체 목록에서 해당 서비스를 선택해 다시 확인하세요."}
            </p>
          )}
          {found.length > 0 ? (
            <>
              {group("Google Workspace", found.filter(connector => connector.id !== vault.id))}
              {group("보안", found.filter(connector => connector.id === vault.id))}
            </>
          ) : (view === "available" || (!accountsChecking && !accountsError)) && (
            <p className="member-page-list connector-empty" role="status">
              {search ? "검색어와 일치하는 서비스가 없습니다." : "연결된 서비스가 없습니다."}
            </p>
          )}
        </>
      )}
    </div>
  );
}

function ConnectorMark({ mark }: { mark?: string }) {
  return <span className="member-library-well" aria-hidden="true">
    {mark ? <img src={mark} alt="" /> : <WorkspaceIcon name="vault" />}
  </span>;
}

function Badge({ badge }: { badge: StatusBadge }) {
  return <span className="connector-badge" data-tone={badge.tone}>{badge.label}</span>;
}

function ConnectorDetail({ connector, info, mailbox, busy, notice, children }: {
  connector: Connector; info: ConnectionInfo; mailbox?: string; busy?: boolean; notice?: string; children: ReactNode;
}) {
  return <section className="member-page-list connector-detail" aria-label={connector.name}>
    <div className="connector-detail-title">
      <ConnectorMark mark={connector.mark} />
      <div>
        <h2>내 연결</h2>
        <div role="status"><Badge badge={busy ? { label: "요청 처리 중" } : info.badge} /></div>
      </div>
    </div>
    {mailbox && <dl><dt>연결된 계정</dt><dd>{mailbox}</dd></dl>}
    {info.message && <p className="connector-detail-message">{info.message}</p>}
    <dl className="connector-permission">
      <dt>{connector.id === vault.id ? "이용 범위" : "읽기 전용"}</dt><dd>{connector.description}</dd>
    </dl>
    <div className="member-connector-actions connector-actions">{children}</div>
    {notice && <p className="connector-notice" role="status">{notice}</p>}
  </section>;
}

function separateHttpsOrigin(value: unknown) {
  if (typeof value !== "string") return null;
  try {
    return value.startsWith("https://") ? trustedVaultOrigin(value) : null;
  } catch {
    return null;
  }
}

type VaultConnectionState =
  | { status: "checking" | "error"; siteOrigin?: never }
  | { status: "setup_required"; siteOrigin: string | null }
  | { status: "ready"; siteOrigin: string };

function useVaultConnection(account: string, expired: () => void) {
  const scope = useScope(account, "personal-vault");
  const [connection, setConnection] = useState<VaultConnectionState>({ status: "checking" });
  async function refresh() {
    setConnection({ status: "checking" });
    try {
      const { state, site_origin } = await scope.request<{ state: string; site_origin?: unknown }>("/personal/vault");
      const origin = separateHttpsOrigin(site_origin);
      setConnection(state === "setup_required" ? { status: state, siteOrigin: origin }
        : state === "ready" && origin ? { status: state, siteOrigin: origin }
        : { status: "error" });
    } catch (error) {
      if (isAborted(error)) return;
      if (error instanceof ApiFailure && error.status === 401) expired();
      else setConnection({ status: "error" });
    }
  }
  useEffect(() => {
    const timer = window.setTimeout(() => void refresh(), 0);
    return () => clearTimeout(timer);
  }, [scope, expired]);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  async function open() {
    if (connection.status !== "ready" || busy) return;
    const expected = connection.siteOrigin;
    setBusy(true);
    setFailure("");
    // Open synchronously inside the click so the browser keeps the user gesture.
    const target = `moosuri-vault-${crypto.randomUUID()}`;
    const popup = window.open("about:blank", target);
    if (!popup) {
      setFailure("새 창을 허용한 뒤 다시 열어 주세요.");
      setBusy(false);
      return;
    }
    try {
      const handoff = await scope.request<{ token: unknown; vault_origin: unknown }>("/personal/vault/handoff", "POST", {});
      if (typeof handoff.token !== "string" || !/^[0-9a-f]{64}$/i.test(handoff.token) || handoff.vault_origin !== expected) {
        throw new Error("vault_unavailable");
      }
      postVaultForm(popup, target, { token: handoff.token, vault_origin: expected }, account);
    } catch (error) {
      popup.close();
      if (isAborted(error)) return;
      if (error instanceof ApiFailure && error.status === 401) expired();
      else setFailure("볼트 연결을 확인하지 못했습니다. 다시 열어 주세요.");
    } finally {
      setBusy(false);
    }
  }
  return { ...connection, busy, failure, open, retry: () => void refresh() };
}
