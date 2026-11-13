import { useEffect, useRef, useState, type FormEvent } from "react";
import type {
  MailConnections,
  SourceConnection,
  SourceConnectionKind,
  SourceSyncRun,
} from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import "./source-connections.css";

const base = "/source-connections";
export const sourceKindName = (kind: SourceConnectionKind) =>
  ({ gmail: "Gmail", slack: "Slack", custom_http: "커스텀 HTTP" })[kind];
export const sourceTime = (value: number | null) =>
  value === null ? "아직 없음" : new Date(value * 1000).toLocaleString("ko-KR");
const localInputTime = (value: number) => {
  const date = new Date(value);
  return new Date(value - date.getTimezoneOffset() * 60000)
    .toISOString()
    .slice(0, 16);
};
const disconnected = (c: SourceConnection) =>
  c.state === "disconnected" || c.state === "deleted" || c.state === "revoked";
const reconnect = (c: SourceConnection) =>
  c.state === "needs_reconnect" ||
  c.error === "needs_reconnect" ||
  !c.credential_present;
export function sourceState(c: SourceConnection) {
  if (disconnected(c))
    return c.state === "deleted" ? "자료 삭제됨" : "연결 해제됨";
  if (c.state === "paused") return "일시정지";
  if (reconnect(c))
    return c.kind === "custom_http" ? "수집 토큰 필요" : "재연결 필요";
  if (c.state === "partial") return "일부만 확인됨";
  if (c.state === "unknown" || c.error === "outcome_unknown")
    return "결과 확인 필요";
  if (c.error === "checkpoint_invalid") return "동기화 다시 시작 필요";
  if (c.error === "rate_limited") return "요청 제한 · 재시도 대기";
  if (c.error === "network_error") return "일시 오류 · 재시도 대기";
  if (c.state === "failed" || c.error) return "동기화 확인 필요";
  if (c.kind === "custom_http") return "자료 수신 대기";
  if (!c.sync_enabled) return "자동 동기화 꺼짐";
  return c.state === "running" ? "동기화 중" : "동기화 대기";
}
export function sourceRunState(run: SourceSyncRun) {
  const label: Record<string, string> = {
    running: "진행 중",
    completed: "범위 동기화 완료",
    polled: "설정한 기간 조회 완료",
    no_change: "변경 없음",
    partial: "일부만 확인됨",
    unknown: "결과 확인 필요",
    failed: "실패",
    needs_reconnect: "재연결 필요",
    paused: "일시정지",
    cancelled: "중단됨",
  };
  return `${run.producer_report ? "수집기 보고: " : ""}${label[run.state] ?? "상태 확인 필요"}`;
}
export function sourceError(error: unknown) {
  if (error instanceof ApiFailure) {
    if (error.status === 409)
      return "연결 상태가 바뀌었습니다. 새로고침한 뒤 다시 시도하세요.";
    if (error.status === 429)
      return "요청이 많습니다. 잠시 후 다시 시도하세요.";
    if (error.status === 403) return "이 계정에서 사용할 수 없는 작업입니다.";
    if (error.status === 400 || error.status === 422)
      return "입력한 범위와 연결 설정을 확인해 주세요.";
    if (error.status === 404) return "연결 또는 수집 기록을 찾을 수 없습니다.";
  }
  return "결과를 확인하지 못했습니다. 상태를 새로고침해 확인하세요.";
}

type Props = { accountId: string; onExpired: () => void; back: () => void };
// Account changes unmount all forms, one-time credentials and pending UI state.
export function SourceConnections(props: Props) {
  return <Connections key={props.accountId} {...props} />;
}
function Connections({ accountId, onExpired, back }: Props) {
  const scope = useScope(accountId);
  const [connections, setConnections] = useState<SourceConnection[] | null>(
    null,
  );
  const [mail, setMail] = useState<MailConnections | null>(null);
  const [gmailConfigured, setGmailConfigured] = useState(false);
  const [selected, setSelected] = useState<string | null>(() =>
    new URLSearchParams(window.location.search).get("connection"),
  );
  const [adding, setAdding] = useState<SourceConnectionKind | null>(null);
  const [error, setError] = useState("");
  const [refresh, setRefresh] = useState(0);
  const [loading, setLoading] = useState(true);
  const listRequest = useRef<AbortController | null>(null);
  const connection = connections?.find((c) => c.id === selected);
  const gmailReconnect = connection?.kind === "gmail" && reconnect(connection);
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(sourceError(e));
  }
  useEffect(() => {
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible")
        setRefresh((value) => value + 1);
    }, 10000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    listRequest.current = controller;
    setLoading(true);
    void scope
      .request<{ connections: SourceConnection[]; gmail_configured: boolean }>(
        base,
        "GET",
        undefined,
        controller.signal,
      )
      .then((value) => {
        setConnections(value.connections);
        setGmailConfigured(value.gmail_configured);
        setLoading(false);
      })
      .catch((e) => {
        if (!isAborted(e)) {
          fail(e);
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [scope, refresh]);
  useEffect(() => {
    if (adding !== "gmail" && !gmailReconnect) return;
    const controller = new AbortController();
    void scope
      .request<MailConnections>(
        "/mail/connections",
        "GET",
        undefined,
        controller.signal,
      )
      .then(setMail)
      .catch(fail);
    return () => controller.abort();
  }, [scope, adding, gmailReconnect]);
  function changed(value: SourceConnection) {
    listRequest.current?.abort();
    setLoading(false);
    setConnections(
      (current) =>
        current?.map((c) =>
          c.id === value.id && c.config_revision <= value.config_revision
            ? value
            : c,
        ) ?? null,
    );
  }
  return (
    <main className="admin-scroll source-connections">
      <div className="admin-intro">
        <h2>자료 수집 연결</h2>
        <p>
          허용한 범위의 자료를 내 계정으로 모읍니다. 직접 붙여넣는 자료 모음은
          자료 화면에서 관리하세요.
        </p>
      </div>
      <button className="text-button" onClick={back}>
        ← 대화로 돌아가기
      </button>
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      <div className="source-actions">
        <button
          disabled={loading}
          onClick={() => {
            setError("");
            setRefresh((v) => v + 1);
          }}
        >
          상태 새로고침
        </button>
      </div>
      <section className="admin-card">
        <h3>연결 추가</h3>
        <p className="fine">
          Gmail과 Slack은 무수리가 제공하는 어댑터입니다. 서비스 제공자의 제휴나
          보증을 뜻하지 않습니다.
        </p>
        <div className="source-actions">
          <button onClick={() => setAdding("gmail")}>Gmail 추가</button>
          <button onClick={() => setAdding("slack")}>Slack 추가</button>
          <button onClick={() => setAdding("custom_http")}>
            커스텀 수집 연결 추가
          </button>
        </div>
      </section>
      {adding && (
        <AddConnection
          key={adding}
          kind={adding}
          scope={scope}
          mail={mail}
          gmailConfigured={gmailConfigured}
          close={() => setAdding(null)}
          fail={fail}
          created={(value) => {
            setConnections((c) => [...(c ?? []), value]);
            setSelected(value.id);
            setAdding(null);
          }}
        />
      )}
      {loading && !connections ? (
        <p role="status">연결 상태 확인 중…</p>
      ) : (
        <section className="admin-card" aria-label="내 수집 연결">
          <h3>내 연결</h3>
          {connections?.length ? (
            <ul className="source-list">
              {connections.map((c) => (
                <li key={c.id}>
                  <button
                    aria-pressed={selected === c.id}
                    onClick={() => setSelected(c.id)}
                  >
                    <strong>{c.name}</strong>
                    <span>
                      {sourceKindName(c.kind)} · {sourceState(c)}
                    </span>
                    <small>
                      마지막 자료 저장: {sourceTime(c.last_commit_at)}
                    </small>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p>
              {connections === null
                ? "연결 목록을 확인할 수 없습니다."
                : "아직 수집 연결이 없습니다."}
            </p>
          )}
        </section>
      )}
      {connection && (
        <ConnectionDetail
          key={connection.id}
          connection={connection}
          mail={mail}
          scope={scope}
          fail={fail}
          changed={changed}
          refresh={refresh}
          reload={() => setRefresh((v) => v + 1)}
        />
      )}
    </main>
  );
}
type Scope = ReturnType<typeof useScope>;
function AddConnection({
  kind,
  scope,
  mail,
  gmailConfigured,
  close,
  fail,
  created,
}: {
  kind: SourceConnectionKind;
  scope: Scope;
  mail: MailConnections | null;
  gmailConfigured: boolean;
  close: () => void;
  fail: (e: unknown) => void;
  created: (c: SourceConnection) => void;
}) {
  const [name, setName] = useState("");
  const [range, setRange] = useState("");
  const [mailId, setMailId] = useState("");
  const [token, setToken] = useState("");
  const [since, setSince] = useState("");
  const [interval, setInterval] = useState(300);
  const [lookback, setLookback] = useState(86400);
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const activeMail =
    mail?.connections.filter((c) => c.status === "active") ?? [];
  const selectedMail = mailId || activeMail[0]?.id || "";
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    const after = Math.floor(new Date(since).getTime() / 1000);
    const payload =
      kind === "custom_http"
        ? { kind, name: name.trim(), scope: range.trim() }
        : kind === "gmail"
          ? {
              kind,
              name: name.trim(),
              mail_connection_id: selectedMail,
              label_ids: range
                .split(",")
                .map((v) => v.trim())
                .filter(Boolean),
              after_unix: after,
              poll_interval_seconds: interval,
            }
          : {
              kind,
              name: name.trim(),
              token,
              channel_id: range.trim(),
              oldest_unix: after,
              reconcile_lookback_seconds: lookback,
              poll_interval_seconds: interval,
            };
    setToken("");
    try {
      created(await scope.request<SourceConnection>(base, "POST", payload));
    } catch (e) {
      fail(e);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="admin-card" aria-label="수집 연결 설정">
      <h3>{sourceKindName(kind)} 연결 설정</h3>
      <form onSubmit={submit} className="source-form">
        <fieldset disabled={busy}>
          <label>
            연결 이름
            <input
              required
              maxLength={100}
              value={name}
              onChange={(e) => setName(e.target.value)}
              autoComplete="off"
            />
          </label>
          {kind === "gmail" && (
            <>
              {!gmailConfigured && (
                <p>설치 관리자가 Gmail 설정을 준비해야 합니다.</p>
              )}
              {mail && !activeMail.length && (
                <p>
                  먼저 <a href="/settings/mail">내 Gmail</a>에서 Google 계정을
                  연결하세요.
                </p>
              )}
              <label>
                Google 계정
                <select
                  required
                  value={selectedMail}
                  onChange={(e) => setMailId(e.target.value)}
                >
                  <option value="">계정 선택</option>
                  {activeMail.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.email}
                    </option>
                  ))}
                </select>
              </label>
            </>
          )}
          <label>
            {kind === "gmail"
              ? "Gmail 라벨 ID (쉼표로 구분)"
              : kind === "slack"
                ? "Slack 채널 ID"
                : "고정 수집 범위"}
            <input
              required
              maxLength={1024}
              value={range}
              onChange={(e) => setRange(e.target.value)}
              autoComplete="off"
            />
          </label>
          {kind === "custom_http" ? (
            <p className="fine">
              외부 수집기가 이 범위의 자료를 HTTPS로 보냅니다. 연결을 만든 뒤
              만료되는 수집 전용 토큰을 발급하세요. 연결 범위는 나중에 바꿀 수
              없습니다.
            </p>
          ) : (
            <>
              <p className="fine">
                선택한 계정과 범위만 수집합니다. 범위를 바꾸려면 새 연결을
                만드세요.
              </p>
              <label>
                수집 시작 시각
                <input
                  type="datetime-local"
                  required
                  value={since}
                  max={localInputTime(Date.now())}
                  min={localInputTime(
                    Date.now() - (kind === "gmail" ? 366 : 90) * 86400000,
                  )}
                  onChange={(e) => setSince(e.target.value)}
                />
              </label>
              <p className="fine">
                {kind === "gmail" ? "최근 366일" : "최근 90일"} 안에서 수집을
                시작할 수 있습니다.
              </p>
              <label>
                동기화 간격 (초)
                <input
                  type="number"
                  required
                  min={60}
                  max={86400}
                  value={interval}
                  onChange={(e) => setInterval(Number(e.target.value))}
                />
              </label>
            </>
          )}
          {kind === "slack" && (
            <>
              <label>
                Slack 수집 토큰
                <input
                  type="password"
                  required
                  autoComplete="new-password"
                  value={token}
                  onChange={(e) => setToken(e.target.value)}
                />
              </label>
              <label>
                답글 재확인 범위 (초)
                <input
                  type="number"
                  min={60}
                  max={2592000}
                  required
                  value={lookback}
                  onChange={(e) => setLookback(Number(e.target.value))}
                />
              </label>
              <p className="fine">
                접근 가능한 채널의 메시지와 답글을 읽을 권한이 필요합니다.
                설정한 재확인 기간 밖의 답글과 원본 삭제는 감지하지 못합니다.
                토큰은 전송 직후 이 화면에서 지워집니다.
              </p>
            </>
          )}
          {kind !== "custom_http" && (
            <label className="source-check">
              <input
                type="checkbox"
                required
                checked={consent}
                onChange={(e) => setConsent(e.target.checked)}
              />
              이 범위의 자료를 동기화하는 데 동의합니다.
            </label>
          )}
          <div className="source-actions">
            <button
              className="primary"
              disabled={
                busy ||
                (kind === "gmail" && (!gmailConfigured || !selectedMail))
              }
            >
              {busy ? "연결 중…" : "연결 만들기"}
            </button>
            <button
              type="button"
              onClick={() => {
                setToken("");
                close();
              }}
            >
              취소
            </button>
          </div>
        </fieldset>
      </form>
    </section>
  );
}

function ConnectionDetail({
  connection: c,
  mail,
  scope,
  fail,
  changed,
  refresh,
  reload,
}: {
  connection: SourceConnection;
  mail: MailConnections | null;
  scope: Scope;
  fail: (e: unknown) => void;
  changed: (c: SourceConnection) => void;
  refresh: number;
  reload: () => void;
}) {
  const [runs, setRuns] = useState<SourceSyncRun[] | null>(null);
  const [runsError, setRunsError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState(c.name);
  const [interval, setInterval] = useState(c.poll_interval_seconds);
  const [syncEnabled, setSyncEnabled] = useState(c.sync_enabled);
  const [disconnecting, setDisconnecting] = useState(false);
  const [mode, setMode] = useState<"keep" | "delete">("keep");
  const [secret, setSecret] = useState<{
    token: string;
    expires_at: number;
  } | null>(null);
  const [ttl, setTtl] = useState(86400);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState("");
  const [slackToken, setSlackToken] = useState("");
  const [gmailConnectionId, setGmailConnectionId] = useState("");
  const activeMail =
    mail?.connections.filter((connection) => connection.status === "active") ??
    [];
  const selectedGmail = activeMail.some(
    (connection) => connection.id === gmailConnectionId,
  )
    ? gmailConnectionId
    : (activeMail.find((connection) => connection.id === c.mail_connection_id)
        ?.id ??
      activeMail[0]?.id ??
      "");
  const [historyRefresh, setHistoryRefresh] = useState(0);
  const lock = useRef(false);
  const secretGeneration = useRef(0);
  const clearSecret = () => {
    secretGeneration.current++;
    setSecret(null);
    setCopied(false);
    setCopyError("");
  };
  const path = `${base}/${encodeURIComponent(c.id)}`;
  useEffect(() => {
    setName(c.name);
    setInterval(c.poll_interval_seconds);
    setSyncEnabled(c.sync_enabled);
  }, [c.config_revision, c.name, c.poll_interval_seconds, c.sync_enabled]);
  useEffect(() => {
    const controller = new AbortController();
    setRunsError(false);
    void scope
      .request<{ runs: SourceSyncRun[] }>(
        `${path}/runs`,
        "GET",
        undefined,
        controller.signal,
      )
      .then((v) => setRuns(v.runs))
      .catch((e) => {
        if (!isAborted(e)) {
          setRunsError(true);
          fail(e);
        }
      });
    return () => controller.abort();
  }, [scope, path, refresh, historyRefresh]);
  useEffect(() => {
    if (!secret) return;
    const timer = window.setTimeout(
      clearSecret,
      Math.max(0, Math.min(2147483647, secret.expires_at * 1000 - Date.now())),
    );
    return () => window.clearTimeout(timer);
  }, [secret]);
  useEffect(() => {
    if (disconnected(c) || !c.credential_present) clearSecret();
  }, [c.state, c.credential_present]);
  useEffect(
    () => () => {
      secretGeneration.current++;
    },
    [],
  );
  async function mutate(action: string, body: unknown, method = "POST") {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    try {
      changed(
        await scope.request<SourceConnection>(`${path}${action}`, method, body),
      );
      setHistoryRefresh((v) => v + 1);
      setDisconnecting(false);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      lock.current = false;
    }
  }
  async function issueToken() {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    clearSecret();
    try {
      setSecret(
        await scope.request<{ token: string; expires_at: number }>(
          `${path}/token`,
          "POST",
          { ttl_seconds: ttl },
        ),
      );
      reload();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      lock.current = false;
    }
  }
  async function copyToken() {
    if (!secret || secret.expires_at * 1000 <= Date.now()) {
      clearSecret();
      return;
    }
    const generation = secretGeneration.current;
    try {
      await navigator.clipboard.writeText(secret.token);
      if (generation === secretGeneration.current) setCopied(true);
    } catch {
      if (generation === secretGeneration.current)
        setCopyError("복사하지 못했습니다. 토큰을 선택해 직접 복사하세요.");
    }
  }
  const stopped = disconnected(c);
  return (
    <section className="admin-card" aria-label="수집 연결 상세">
      <div className="section-heading">
        <h3>{c.name}</h3>
        <span className="pill">{sourceState(c)}</span>
      </div>
      <p>
        {sourceKindName(c.kind)} ·{" "}
        {c.kind === "custom_http"
          ? "외부 수집기 · 사용자 관리"
          : "공식 어댑터 · 무수리 관리"}
      </p>
      <dl className="source-times">
        <div>
          <dt>마지막 접속</dt>
          <dd>{sourceTime(c.last_heartbeat_at)}</dd>
        </div>
        <div>
          <dt>마지막 자료 저장</dt>
          <dd>{sourceTime(c.last_commit_at)}</dd>
        </div>
        <div>
          <dt>
            {c.kind === "custom_http"
              ? "마지막 완료 보고 (수집기)"
              : "마지막 범위 동기화 완료"}
          </dt>
          <dd>{sourceTime(c.last_completed_at)}</dd>
        </div>
      </dl>
      {c.kind === "custom_http" && (
        <p className="fine">
          접속은 자료 저장과 다릅니다. 완료 보고는 수집기의 주장입니다. 무수리가
          확인하는 것은 수신한 자료의 저장입니다.
        </p>
      )}
      {c.kind === "slack" && (
        <p className="fine">
          설정한 채널과 기간을 조회합니다. 재확인 기간 밖의 답글과 원본 삭제는
          감지하지 못하며, 조회 완료가 전체 이력의 동기화 완료를 뜻하지
          않습니다.
        </p>
      )}
      {reconnect(c) && !stopped && (
        <p role="status">
          {c.kind === "gmail"
            ? "Google 계정의 권한을 내 Gmail에서 다시 연결해 주세요."
            : c.kind === "slack"
              ? "Slack 수집 토큰의 권한과 유효성을 확인한 뒤 다시 연결하세요."
              : "만료되는 수집 토큰을 발급하고 외부 수집기에 설정하세요."}
          {c.kind === "gmail" && (
            <>
              {" "}
              <a href="/settings/mail">내 Gmail 열기</a>
            </>
          )}
        </p>
      )}
      {c.error && (
        <p className="notice">
          {c.error === "checkpoint_invalid"
            ? "원본 서비스의 동기화 위치가 만료되었습니다. 기존 자료를 유지하고 허용한 범위를 다시 확인할 수 있습니다."
            : c.error === "invalid_batch"
              ? "한 번에 처리할 수 있는 수집 범위를 넘었거나 지원하지 않는 자료가 있습니다. 더 좁은 범위로 새 연결을 만드세요. 자료를 자동으로 건너뛰지 않습니다."
              : c.state === "partial"
                ? "일부 범위만 확인했습니다. 전체 동기화가 완료된 상태는 아닙니다."
                : c.state === "unknown" || c.error === "outcome_unknown"
                  ? "이전 실행 결과를 확인할 수 없습니다. 이력을 확인하세요."
                  : c.error === "rate_limited"
                    ? "원본 서비스의 요청 제한으로 기다리는 중입니다. 허용된 시각에 다시 확인합니다."
                    : c.error === "network_error"
                      ? "원본 서비스에 잠시 연결하지 못했습니다. 자동으로 다시 확인합니다."
                      : "연결 설정 또는 실행 이력을 확인해 주세요."}
        </p>
      )}
      {c.kind !== "custom_http" &&
        !stopped &&
        (c.error === "rate_limited" || c.error === "network_error") && (
          <p className="fine">다음 재시도: {sourceTime(c.next_due_at)}</p>
        )}
      <details>
        <summary>연결 설정</summary>
        <p>수집 범위: {c.scope}</p>
        {c.kind !== "custom_http" && <p>원본 계정: {c.remote_account}</p>}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void mutate(
              "",
              {
                expected_revision: c.config_revision,
                name: name.trim(),
                sync_enabled: syncEnabled,
                poll_interval_seconds: interval,
              },
              "PATCH",
            );
          }}
          className="source-form"
        >
          <fieldset disabled={busy || stopped}>
            <label>
              표시 이름
              <input
                required
                maxLength={100}
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            {c.kind !== "custom_http" && (
              <>
                <label className="source-check">
                  <input
                    type="checkbox"
                    checked={syncEnabled}
                    onChange={(e) => setSyncEnabled(e.target.checked)}
                  />
                  자동 동기화
                </label>
                <label>
                  동기화 간격 (초)
                  <input
                    type="number"
                    min={60}
                    max={86400}
                    value={interval}
                    onChange={(e) => setInterval(Number(e.target.value))}
                  />
                </label>
              </>
            )}
            <button>설정 저장</button>
          </fieldset>
        </form>
      </details>
      {c.kind !== "custom_http" &&
        c.error === "checkpoint_invalid" &&
        !stopped && (
          <button
            disabled={busy}
            onClick={() =>
              void mutate("/reset-checkpoint", {
                expected_revision: c.config_revision,
              })
            }
          >
            동기화 다시 시작
          </button>
        )}
      {!stopped && (
        <div className="source-actions">
          <button
            disabled={busy}
            onClick={() =>
              void mutate("/pause", {
                expected_revision: c.config_revision,
                paused: c.state !== "paused",
              })
            }
          >
            {c.state === "paused" ? "수집 재개" : "수집 일시정지"}
          </button>
          {c.kind !== "custom_http" && (
            <button
              disabled={
                busy ||
                c.state === "paused" ||
                reconnect(c) ||
                !c.sync_enabled ||
                c.error === "checkpoint_invalid" ||
                c.error === "invalid_batch"
              }
              onClick={() =>
                void mutate("/sync", { expected_revision: c.config_revision })
              }
            >
              지금 동기화
            </button>
          )}
          <button
            onClick={() => {
              clearSecret();
              setDisconnecting(true);
            }}
            disabled={busy}
          >
            연결 해제
          </button>
        </div>
      )}
      {!stopped && c.kind !== "custom_http" && !c.sync_enabled && (
        <p className="fine">
          연결 설정에서 자동 동기화를 켜면 지금 동기화할 수 있습니다.
        </p>
      )}
      {stopped && c.state !== "deleted" && (
        <button
          className="danger"
          disabled={busy}
          onClick={() => {
            setMode("delete");
            setDisconnecting(true);
          }}
        >
          보관된 자료 삭제
        </button>
      )}
      {c.kind === "gmail" && reconnect(c) && !stopped && (
        <form
          className="source-form"
          onSubmit={(event) => {
            event.preventDefault();
            void mutate("/gmail-connection", {
              expected_revision: c.config_revision,
              mail_connection_id: selectedGmail,
            });
          }}
        >
          <label>
            다시 연결할 Google 계정
            <select
              required
              disabled={busy || !activeMail.length}
              value={selectedGmail}
              onChange={(event) => setGmailConnectionId(event.target.value)}
            >
              <option value="">계정 선택</option>
              {activeMail.map((connection) => (
                <option key={connection.id} value={connection.id}>
                  {connection.email}
                </option>
              ))}
            </select>
          </label>
          <p className="fine">
            같은 Google 계정의 새 연결만 사용할 수 있습니다. 수집 범위와 기존
            자료는 유지합니다.
          </p>
          <button disabled={busy || !selectedGmail}>Gmail 재연결</button>
        </form>
      )}
      {c.kind === "slack" && !stopped && (
        <form
          className="source-form"
          onSubmit={(e) => {
            e.preventDefault();
            const token = slackToken;
            setSlackToken("");
            void mutate("/slack-token", { token });
          }}
        >
          <label>
            새 Slack 수집 토큰
            <input
              type="password"
              autoComplete="new-password"
              required
              value={slackToken}
              onChange={(e) => setSlackToken(e.target.value)}
              disabled={busy}
            />
          </label>
          <button disabled={busy}>Slack 재연결</button>
        </form>
      )}
      {c.kind === "custom_http" && !stopped && (
        <section aria-label="수집 전용 토큰">
          <h4>수집 전용 토큰</h4>
          <p className="fine">
            이 연결로 자료를 보내는 권한만 줍니다. 토큰을 새로 발급하면 이전
            토큰은 즉시 사용할 수 없습니다. 앱 로그아웃만으로 수집 권한이
            해제되지는 않습니다.
          </p>
          <label>
            토큰 유효기간
            <select
              value={ttl}
              disabled={busy}
              onChange={(e) => setTtl(Number(e.target.value))}
            >
              <option value={3600}>1시간</option>
              <option value={86400}>1일</option>
              <option value={604800}>7일</option>
            </select>
          </label>
          <div className="source-actions">
            <button disabled={busy} onClick={() => void issueToken()}>
              {c.credential_present || secret ? "토큰 교체" : "토큰 발급"}
            </button>
            <button
              disabled={busy}
              onClick={async () => {
                if (lock.current) return;
                lock.current = true;
                setBusy(true);
                clearSecret();
                try {
                  await scope.request(`${path}/revoke-token`, "POST", {});
                  reload();
                } catch (e) {
                  fail(e);
                } finally {
                  lock.current = false;
                  setBusy(false);
                }
              }}
            >
              토큰 철회
            </button>
          </div>
          {secret && (
            <div className="source-secret" aria-label="일회 수집 토큰">
              <p>
                지금 한 번만 표시합니다. 외부 수집기의 비밀 설정에 보관하세요.
              </p>
              <label>
                수집 토큰
                <input
                  value={secret.token}
                  readOnly
                  autoComplete="off"
                  onFocus={(e) => e.target.select()}
                />
              </label>
              <p>만료: {sourceTime(secret.expires_at)}</p>
              <div className="source-actions">
                <button onClick={() => void copyToken()}>
                  {copied ? "복사됨" : "토큰 복사"}
                </button>
                <button onClick={clearSecret}>토큰 표시 닫기</button>
              </div>
              {copyError && <p role="alert">{copyError}</p>}
            </div>
          )}
        </section>
      )}
      {disconnecting && (
        <section className="source-confirm" aria-label="연결 해제 확인">
          <h4>
            {stopped ? "보관된 자료를 삭제할까요?" : "연결을 해제할까요?"}
          </h4>
          {!stopped && <p>새 수집과 수집 토큰을 중단합니다.</p>}
          <fieldset disabled={busy || stopped}>
            <legend>기존 자료 처리</legend>
            <label className="source-check">
              <input
                type="radio"
                name="source-disconnect"
                checked={mode === "keep"}
                onChange={() => setMode("keep")}
              />
              자료와 기억 보관
            </label>
            <label className="source-check">
              <input
                type="radio"
                name="source-disconnect"
                checked={mode === "delete"}
                onChange={() => setMode("delete")}
              />
              원문과 연관 기억 삭제
            </label>
          </fieldset>
          {mode === "delete" && (
            <p>
              이 연결의 원문과 그 원문에 의존하는 기억·이력·채팅 답변을
              삭제합니다. 내가 보낸 질문과 독립적으로 기록한 기억은 유지됩니다.
              삭제는 되돌릴 수 없습니다.
            </p>
          )}
          <div className="source-actions">
            <button
              disabled={busy}
              className={mode === "delete" ? "danger" : "primary"}
              onClick={() =>
                void mutate("/disconnect", {
                  expected_revision: c.config_revision,
                  mode,
                })
              }
            >
              {mode === "keep" ? "자료 보관 후 해제" : "자료 삭제 후 해제"}
            </button>
            <button disabled={busy} onClick={() => setDisconnecting(false)}>
              취소
            </button>
          </div>
        </section>
      )}
      <section aria-label="수집 실행 이력">
        <div className="section-heading">
          <h4>실행 이력</h4>
          <button
            disabled={busy}
            onClick={() => setHistoryRefresh((v) => v + 1)}
          >
            이력 새로고침
          </button>
        </div>
        {runsError ? (
          <p>이력을 불러오지 못했습니다. 다시 확인해 주세요.</p>
        ) : runs === null ? (
          <p role="status">이력 확인 중…</p>
        ) : runs.length ? (
          <ol className="source-runs">
            {runs.map((run) => (
              <li key={run.id}>
                <strong>{sourceRunState(run)}</strong>
                <p>
                  시작: {sourceTime(run.started_at)} · 마지막 접속:{" "}
                  {sourceTime(run.heartbeat_at)}
                </p>
                {run.finished_at !== null && (
                  <p>종료: {sourceTime(run.finished_at)}</p>
                )}
                {run.producer_report && (
                  <small>전체 범위를 무수리가 검증한 결과가 아닙니다.</small>
                )}
                {run.error && (
                  <p className="fine">
                    실행을 마치지 못했습니다. 연결 권한과 설정을 확인하세요.
                  </p>
                )}
              </li>
            ))}
          </ol>
        ) : (
          <p>아직 실행 이력이 없습니다.</p>
        )}
      </section>
    </section>
  );
}
