import { useEffect, useState, type ReactNode } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { googleReturn, usePersonalGoogle } from "./PersonalGoogle";
import { WorkspaceIcon } from "./WorkspaceNavigation";
import gmailMark from "./connector-marks/gmail.png?no-inline";
import driveMark from "./connector-marks/drive.png?no-inline";
import calendarMark from "./connector-marks/calendar.png?no-inline";
import slackMark from "./connector-marks/slack.png?no-inline";

const candidates = [
  { id: "drive", name: "Google Drive", mark: driveMark, description: "드라이브에 있는 파일과 문서를 검색합니다." },
  { id: "calendar", name: "Google Calendar", mark: calendarMark, description: "예정된 일정을 조회하고 새 일정을 추가합니다." },
  { id: "slack", name: "Slack", mark: slackMark, description: "워크스페이스의 채널과 메시지를 검색합니다." },
] as const;

type Connector = {
  id: string;
  name: string;
  mark?: string;
  description?: string;
};
const gmail: Connector = { id: "gmail", name: "Gmail", mark: gmailMark };
const vault: Connector = { id: "vault", name: "볼트" };
const connectors: readonly Connector[] = [gmail, vault, ...candidates];

export function PersonalConnections({
  onBack, ...props
}: Parameters<typeof usePersonalGoogle>[0] & { onBack: () => void }) {
  const google = usePersonalGoogle(props);
  const vaultStatus = useVaultConnection(props.session.user.id, props.expired);
  const [selected, setSelected] = useState<Connector | null>(
    () => props.readIntent || googleReturn() ? gmail : null,
  );
  const [view, setView] = useState<"available" | "connected">("available");
  const [query, setQuery] = useState("");
  const search = query.trim().normalize("NFKC").toLocaleLowerCase();
  const matches = (connector: Connector) => [connector.name, connector.description].some(
    (value) => value?.normalize("NFKC").toLocaleLowerCase().includes(search),
  );
  const found = (view === "available" ? connectors : google.connection ? [gmail] : []).filter(matches);
  const gmailStatus = google.status === "checking" ? "연결 상태 확인 중…"
    : google.status === "disabled" ? "설치 관리자가 Google 연결을 활성화하면 사용할 수 있습니다."
    : google.status === "error" ? "연결 상태를 확인할 수 없습니다."
    : google.connection ? "연결됨" : "연결된 계정이 없습니다.";
  const vaultLabel = vaultStatus.status === "checking" ? "연결 상태 확인 중…"
    : vaultStatus.status === "setup_required" ? "회원 인증 연결 필요"
    : "연결 상태를 확인할 수 없습니다.";

  function row(connector: Connector, description?: string, trailing?: string) {
    return (
      <li key={connector.id}>
        <button className="member-library-row member-connector-row" data-candidate={!!connector.description || undefined} onClick={() => setSelected(connector)}>
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
          if (selected && !props.readIntent) setSelected(null);
          else onBack();
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
        ) : selected.id === vault.id ? (
          <>
            <ConnectorDetail connector={vault} status={vaultLabel} busy={vaultStatus.status === "checking"}>
              {vaultStatus.status === "setup_required" && <span>설치 관리자가 볼트에 회원 인증을 연결해야 합니다.</span>}
              {vaultStatus.siteOrigin && <span>독립 볼트에서는 별도로 로그인합니다.</span>}
            </ConnectorDetail>
            {vaultStatus.siteOrigin && (
              <div className="member-connector-actions">
                <a className="member-page-action" href={vaultStatus.siteOrigin} target="_blank" rel="noopener noreferrer">
                  독립 볼트 열기
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
          <label className="member-library-filter">
            <WorkspaceIcon name="search" />
            <input type="search" value={query} onChange={(event) => setQuery(event.target.value)}
              placeholder="커넥터 검색" aria-label="커넥터 검색" />
          </label>
          <div className="member-connection-switch" role="group" aria-label="서비스 목록">
            <button aria-pressed={view === "available"} onClick={() => setView("available")}>사용 가능</button>
            <button aria-pressed={view === "connected"} onClick={() => setView("connected")}>연결됨</button>
          </div>
          {view === "available" ? (
            <>
              {group("추천 · 도입 후보", candidates.filter(matches).map((connector) => row(connector, connector.description)))}
              {group("생산성", found.includes(gmail) ? [row(gmail, google.connection?.mailbox, google.status === "ready" && !google.connection ? "연결" : "보기")] : [])}
              {group("보안", found.includes(vault) ? [row(vault, vaultLabel, "보기")] : [])}
            </>
          ) : found.length > 0 && (
            <ul className="member-page-list member-library-list">{found.map((connector) => row(connector, google.connection?.mailbox))}</ul>
          )}
          {found.length === 0 && (
            <p className="member-library-status member-connector-empty" role="status">
              {view === "connected" && google.status === "checking" ? "연결된 서비스를 확인 중…"
                : view === "connected" && google.status === "error" ? "연결 상태를 확인할 수 없습니다."
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
