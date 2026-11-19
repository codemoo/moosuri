import { useEffect, useRef, useState, type ReactNode } from "react";
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
import calendarMark from "./connector-marks/calendar.png?no-inline";
import slackMark from "./connector-marks/slack.png?no-inline";

const candidates = [
  { id: "calendar", name: "Google Calendar", mark: calendarMark, description: "예정된 일정을 조회하고 새 일정을 추가합니다." },
  { id: "slack", name: "Slack", mark: slackMark, description: "워크스페이스의 채널과 메시지를 검색합니다." },
] as const;

const workspaceConnectors = [
  { id: "drive", name: "Google Drive", mark: driveMark, description: "Drive 파일의 읽기 권한을 연결합니다.", aliases: ["구글 드라이브"] },
  { id: "docs", name: "Google Docs", mark: docsMark, description: "Docs 문서의 읽기 권한을 연결합니다.", aliases: ["구글 문서", "구글 독스", "Google Documents"] },
  { id: "slides", name: "Google Slides", mark: slidesMark, description: "Slides 프레젠테이션의 읽기 권한을 연결합니다.", aliases: ["구글 슬라이드", "구글 프레젠테이션", "Google Presentations"] },
  { id: "sheets", name: "Google Sheets", mark: sheetsMark, description: "Sheets 스프레드시트의 읽기 권한을 연결합니다.", aliases: ["구글 시트", "구글 스프레드시트", "Google Spreadsheets"] },
] as const;

type Connector = {
  id: string;
  name: string;
  mark?: string;
  description?: string;
  aliases?: readonly string[];
};
const gmail: Connector = { id: "gmail", name: "Gmail", mark: gmailMark };
const vault: Connector = { id: "vault", name: "볼트" };
const connectors: readonly Connector[] = [gmail, ...workspaceConnectors, vault, ...candidates];

function searchText(value: string) {
  return value.normalize("NFKC").toLocaleLowerCase().replace(/\s+/g, "");
}

export function PersonalConnections({
  onBack, installationAdmin = false, onAdminUnavailable, ...props
}: Parameters<typeof usePersonalGoogle>[0] & { onBack: () => void; installationAdmin?: boolean; onAdminUnavailable?: () => void }) {
  const [workspaceReturn] = useState(() => workspaceGoogleReturn(props.session));
  const [selected, setSelected] = useState<Connector | null>(
    () => workspaceReturn ? workspaceConnectors.find(({ id }) => id === workspaceReturn.service) ?? null
      : props.readIntent || googleReturn() ? gmail : null,
  );
  const [workspaceEnabled, setWorkspaceEnabled] = useState(() => selected !== gmail);
  const google = usePersonalGoogle(props, !workspaceReturn);
  const workspace = usePersonalGoogleWorkspace({ session: props.session, expired: props.expired, enabled: workspaceEnabled, returned: workspaceReturn });
  const vaultStatus = useVaultConnection(props.session.user.id, props.expired);
  const [view, setView] = useState<"available" | "connected">("available");
  const [query, setQuery] = useState("");
  const search = searchText(query);
  const matches = (connector: Connector) => [connector.name, connector.description ?? "", ...(connector.aliases ?? [])].some(
    (value) => searchText(value).includes(search),
  );
  const connected = [...(google.connection ? [gmail] : []), ...workspaceConnectors.filter(({ id }) => workspace.connections[id])];
  const found = (view === "available" ? connectors : connected).filter(matches);
  const [setupOpen, setSetupOpen] = useState(false);
  const [installed, setInstalled] = useState<boolean | null>(null);
  const scope = useScope(props.session.user.id, "connector-installation-status");
  useEffect(() => {
    if (!installationAdmin || setupOpen) return;
    const controller = new AbortController();
    void scope.request<{ configured: boolean }>(installationGoogleSetupPath, "GET", undefined, controller.signal)
      .then(result => { if (!controller.signal.aborted) setInstalled(result.configured); })
      .catch(error => {
        if (controller.signal.aborted || isAborted(error)) return;
        setInstalled(null);
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
    if (!installationAdmin || installed !== true || setupOpen) return;
    // The member relay imports the shared configuration asynchronously.
    const timers = [0, 4000, 10000, 18000].map(delay => window.setTimeout(() => refreshMemberServices.current(), delay));
    return () => timers.forEach(clearTimeout);
  }, [installationAdmin, installed, setupOpen, workspaceEnabled]);
  const unavailable = installationAdmin && installed === false ? "Google Workspace 설치 설정이 필요합니다."
    : installationAdmin && installed === true ? "이 회원의 Google 연결을 준비 중입니다."
    : "이 서버에서 Google 로그인을 아직 사용할 수 없습니다.";
  const gmailStatus = google.status === "checking" ? "연결 상태 확인 중…"
    : google.status === "disabled" ? unavailable
    : google.status === "error" ? "연결 상태를 확인할 수 없습니다."
    : google.connection ? "연결됨" : "연결된 계정이 없습니다.";
  const workspaceService = selected && isWorkspaceGoogleService(selected.id) ? selected.id : null;
  const workspaceGrant = workspaceService ? workspace.connections[workspaceService] : null;
  const workspaceStatus = workspace.status === "checking" ? "연결 상태 확인 중…"
    : workspace.status === "disabled" ? unavailable
    : workspace.status === "error" ? "연결 상태를 확인할 수 없습니다."
    : "연결된 계정이 없습니다.";
  const vaultLabel = vaultStatus.status === "checking" ? "연결 상태 확인 중…"
    : vaultStatus.status === "setup_required" ? "회원 인증 연결 필요"
    : "연결 상태를 확인할 수 없습니다.";

  const setupGuide = installationAdmin && (
    <button className="member-page-action" onClick={() => setSetupOpen(true)}>Google Workspace 설정</button>
  );

  if (installationAdmin && setupOpen) {
    return <PersonalGoogleSetup account={props.session.user.id} expired={props.expired}
      unavailable={() => { setSetupOpen(false); onAdminUnavailable?.(); }}
      onBack={() => setSetupOpen(false)}
      onConnect={() => { setInstalled(true); setSetupOpen(false); }}
      returnLabel={selected ? selected.name + " 연결로 돌아가기" : "커넥터 열기"} />;
  }

  function row(connector: Connector, description?: string, trailing?: string, candidate = false) {
    return (
      <li key={connector.id}>
        <button className="member-library-row member-connector-row" data-candidate={candidate || undefined} onClick={() => setSelected(connector)}>
          <ConnectorMark mark={connector.mark} />
          <span className="member-library-row-copy">
            <strong>{connector.name}</strong>
            {description && <span>{description}</span>}
          </span>
          {trailing ? <span className="member-connector-row-action">{trailing}</span>
            : <span aria-hidden="true" className="member-library-chevron" />}
        </button>
      </li>
    );
  }

  function group(title: string, rows: ReactNode[]) {
    if (rows.length === 0) return null;
    return <section className="member-connector-group" aria-label={title}>
      <h2>{title}</h2>
      <ul className="member-page-list member-library-list">{rows}</ul>
    </section>;
  }

  return (
    <div className="member-page member-library member-connections">
      <header className="member-page-heading member-connection-header">
        <button className="member-library-icon-button" aria-label="뒤로" onClick={() => {
          if (selected && !props.readIntent) {
            setSelected(null);
            setWorkspaceEnabled(true);
          } else onBack();
        }}><WorkspaceIcon name="back" /></button>
        <h1>{selected?.name ?? "커넥터"}</h1>
      </header>
      {selected ? (
        selected.id === gmail.id ? (
          <>
            <ConnectorDetail connector={gmail} status={gmailStatus} busy={google.busy}>
              {google.connection && <span>{google.connection.mailbox}</span>}
            </ConnectorDetail>
            <div className="member-connector-actions">
              {google.status === "ready" && (
                google.connection ? (
                  <button className="member-page-action" disabled={google.busy} onClick={() => void google.action("disconnect")}>연결 해제</button>
                ) : (
                  <button className="member-page-action" disabled={google.busy} onClick={() => void google.action("connect")}>
                    {props.readIntent ? "Google 연결만" : "Google 연결"}
                  </button>
                )
              )}
              {setupGuide}
              {(google.status === "error" || google.status === "disabled") && !google.busy && (
                <button className="member-page-action" onClick={google.retry}>다시 확인</button>
              )}
              {props.readIntent && google.status === "ready" && (
                <button className="member-page-action" disabled={google.busy} onClick={() => void google.action("read")}>
                  {google.busy ? "메일 확인 중…" : google.connection ? "메일 확인" : "Google로 연결하고 이 대화에서 메일 확인"}
                </button>
              )}
            </div>
            {google.notice && <p className="member-library-status" role="status">{google.notice}</p>}
          </>
        ) : workspaceService ? (
          <>
            <ConnectorDetail connector={selected} status={workspaceGrant ? "연결됨" : workspaceStatus} busy={workspace.busy}>
              {workspaceGrant && <span>{workspaceGrant.mailbox}</span>}
              <span>{selected.description}</span>
            </ConnectorDetail>
            <div className="member-connector-actions">
              {workspace.status === "ready" && (workspaceGrant ? (
                <button className="member-page-action" disabled={workspace.busy} onClick={() => void workspace.action(workspaceService, "disconnect")}>연결 해제</button>
              ) : (
                <button className="member-page-action" disabled={workspace.busy} onClick={() => void workspace.action(workspaceService, "connect")}>Google 연결</button>
              ))}
              {setupGuide}
              {(workspace.status === "error" || workspace.status === "disabled") && !workspace.busy && (
                <button className="member-page-action" onClick={() => void workspace.retry()}>다시 확인</button>
              )}
            </div>
            {workspace.notice?.service === workspaceService && (
              <p className="member-library-status" role="status">{workspace.notice.message}</p>
            )}
          </>
        ) : selected.id === vault.id ? (
          <>
            <ConnectorDetail connector={vault} status={vaultLabel} busy={vaultStatus.status === "checking"}>
              {vaultStatus.status === "setup_required" && <span>현재 설치의 회원 세션과 볼트가 연결되지 않았습니다.</span>}
            </ConnectorDetail>
            {vaultStatus.siteOrigin && (
              <div className="member-connector-actions">
                <a className="member-page-action" href={vaultStatus.siteOrigin} target="_blank" rel="noopener noreferrer">
                  볼트 사이트 보기
                </a>
              </div>
            )}
            {vaultStatus.status === "error" && (
              <div className="member-connector-actions">
                <button className="member-page-action" onClick={vaultStatus.retry}>다시 확인</button>
              </div>
            )}
          </>
        ) : (
          <ConnectorDetail connector={selected} status="도입 후보">
            <span>{selected.description}</span>
            <span>현재 이 설치에서 연결할 수 없습니다.</span>
          </ConnectorDetail>
        )
      ) : (
        <>
          {installationAdmin && <button type="button" className="member-connector-setup" aria-label="Google Workspace 설정"
            onClick={() => setSetupOpen(true)}>
            <span><strong>Google Workspace 설정</strong><small>{installed === false ? "설치 설정 필요" : installed === true ? "설치 설정 완료" : "설치 관리"}</small></span>
            <span aria-hidden="true" className="member-library-chevron" />
          </button>}
          <label className="member-library-filter">
            <WorkspaceIcon name="search" />
            <input type="search" value={query} onChange={(event) => setQuery(event.target.value)}
              placeholder="커넥터 검색" aria-label="커넥터 검색" />
          </label>
          <div className="member-connection-switch" role="group" aria-label="서비스 목록">
            <button aria-pressed={view === "available"} onClick={() => setView("available")}>사용 가능</button>
            <button aria-pressed={view === "connected"} onClick={() => setView("connected")}>연결됨</button>
          </div>
          {workspace.notice?.service === null && <p className="member-library-status" role="status">{workspace.notice.message}</p>}
          {view === "available" ? (
            <>
              {group("생산성", [
                ...(found.includes(gmail) ? [row(gmail, google.connection?.mailbox ?? gmailStatus, google.status === "ready" && !google.connection ? "연결" : "보기")] : []),
                ...workspaceConnectors.filter(matches).map((connector) => {
                  const grant = workspace.connections[connector.id];
                  return row(connector, grant?.mailbox ?? workspaceStatus, workspace.status === "ready" && !grant ? "연결" : "보기");
                }),
              ])}
              {group("보안", found.includes(vault) ? [row(vault, vaultLabel, "보기")] : [])}
              {group("추천 · 도입 후보", candidates.filter(matches).map((connector) => row(connector, connector.description, undefined, true)))}
            </>
          ) : found.length > 0 && (
            <ul className="member-page-list member-library-list">{found.map((connector) => row(connector,
              isWorkspaceGoogleService(connector.id) ? workspace.connections[connector.id]?.mailbox : google.connection?.mailbox,
            ))}</ul>
          )}
          {found.length === 0 && (
            <p className="member-library-status member-connector-empty" role="status">
              {view === "connected" && (google.status === "checking" || workspace.status === "checking") ? "연결된 서비스를 확인 중…"
                : view === "connected" && (google.status === "error" || workspace.status === "error") ? "연결 상태를 확인할 수 없습니다."
                : search ? "검색어와 일치하는 서비스가 없습니다." : "연결된 서비스가 없습니다."}
            </p>
          )}
        </>
      )}
    </div>
  );
}

function ConnectorMark({ mark }: { mark?: string }) {
  return (
    <span className="member-library-well" aria-hidden="true">
      {mark ? <img src={mark} alt="" /> : <WorkspaceIcon name="vault" />}
    </span>
  );
}

function ConnectorDetail({
  connector, status, busy, children,
}: { connector: Connector; status: string; busy?: boolean; children?: ReactNode }) {
  return <section className="member-page-list member-library-list" aria-label={connector.name} aria-busy={busy}>
    <div className="member-library-row">
      <ConnectorMark mark={connector.mark} />
      <div className="member-library-row-copy">
        <strong role="status">{status}</strong>
        {children}
      </div>
    </div>
  </section>;
}

function separateHttpsOrigin(value: unknown) {
  if (typeof value !== "string") return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.origin === value && url.origin !== location.origin
      ? url.origin
      : null;
  } catch {
    return null;
  }
}

type VaultConnectionState =
  | { status: "checking" | "error"; siteOrigin?: never }
  | { status: "setup_required"; siteOrigin: string | null };

function useVaultConnection(account: string, expired: () => void) {
  const scope = useScope(account, "personal-vault");
  const [connection, setConnection] = useState<VaultConnectionState>({ status: "checking" });
  async function refresh() {
    setConnection({ status: "checking" });
    try {
      const { state, site_origin } = await scope.request<{ state: string; site_origin?: unknown }>("/personal/vault");
      setConnection(state === "setup_required"
        ? { status: state, siteOrigin: separateHttpsOrigin(site_origin) }
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
  return { ...connection, retry: () => void refresh() };
}
