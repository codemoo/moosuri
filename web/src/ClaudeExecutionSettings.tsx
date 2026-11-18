import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { ClaudeSubscriptionLogin } from "./ClaudeSubscriptionLogin";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

type Binding = { id: string; revision: number; label: string; status: string; cleanup_state?: string };
type ModelSyncStatus = "checking" | "ready" | "login_required" | "unavailable";
type State = { runtime_configured: boolean; bindings: Binding[]; text_only: true };
type Props = {
  accountId: string;
  onExpired: () => void;
  onVerified: () => void;
  onConnectApi?: () => void;
  refreshKey?: number;
};
const setupGuide = "https://github.com/codemoo/moosuri/blob/main/docs/claude-execution.md#operator-profile";

// Remount all local form and operation state when the authenticated owner changes.
export function ClaudeExecutionSettings(props: Props) {
  return <ClaudeConnections key={props.accountId} {...props} />;
}

function ClaudeConnections({ accountId, onExpired, onVerified, onConnectApi, refreshKey = 0 }: Props) {
  const scope = useScope(accountId);
  const guideId = useId();
  const [state, setState] = useState<State | null>(null);
  const [loading, setLoading] = useState(true);
  const [guideOpen, setGuideOpen] = useState(false);
  const [label, setLabel] = useState("내 Claude");
  const [models, setModels] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const pending = useRef(false);
  const queuedRefresh = useRef(false);
  const queuedLogins = useRef(new Map<string, Binding>());
  const [syncStatus, setSyncStatus] = useState<Record<string, ModelSyncStatus>>({});
  const verified = useRef(onVerified);
  verified.current = onVerified;

  function fail(error: unknown, fallback: string) {
    if (isAborted(error)) return;
    if (error instanceof ApiFailure && error.status === 401) onExpired();
    else if (error instanceof ApiFailure && error.status === 409)
      setError("연결 정보가 바뀌었습니다. Claude 연결 새로고침 후 다시 확인해 주세요.");
    else if (error instanceof ApiFailure && error.status === 429)
      setError("다른 연결 확인이 진행 중입니다. 잠시 뒤 다시 시도해 주세요.");
    else setError(fallback);
  }

  // Only entry and an explicit parent refresh recover pending bindings. A
  // successful sync reloads the selector without triggering another probe.
  useEffect(() => {
    if (pending.current) queuedRefresh.current = true;
    else void refresh();
  }, [scope, refreshKey]);

  function finish() {
    pending.current = false;
    setLoading(false);
    setBusy(false);
    // A login can settle while a status refresh is in progress; retain that
    // one recovery without replaying duplicate button clicks.
    const login = queuedLogins.current.values().next().value;
    if (login) {
      queuedLogins.current.delete(login.id);
      void refresh(login);
    } else if (queuedRefresh.current) {
      queuedRefresh.current = false;
      void refresh();
    }
  }

  async function syncModels(binding: Binding, force = false) {
    setSyncStatus(old => ({ ...old, [binding.id]: "checking" }));
    try {
      const result = await scope.request<{ status: string; models: string[] }>(
        `/settings/claude/${encodeURIComponent(binding.id)}/models`, "POST", { revision: binding.revision, ...(force ? { force: true } : {}) },
      );
      const ready = result.status === "ready" && Array.isArray(result.models) && result.models.length > 0;
      setSyncStatus(old => ({ ...old, [binding.id]: ready ? "ready" : result.status === "login_required" ? "login_required" : "unavailable" }));
      if (ready) {
        setState(current => current && { ...current, bindings: current.bindings.map(item => item.id === binding.id && item.revision === binding.revision
          ? { ...item, status: "active" } : item) });
        verified.current();
      }
    } catch (error) {
      if (isAborted(error) || error instanceof ApiFailure && error.status === 401) throw error;
      setSyncStatus(old => ({ ...old, [binding.id]: "unavailable" }));
      fail(error, "Claude 모델을 불러오지 못했습니다. 잠시 뒤 모델 목록 다시 확인을 눌러 주세요.");
    }
  }

  async function refresh(loginBinding?: Binding, options: { afterLogin?: boolean; force?: boolean } = {}) {
    if (pending.current) {
      if (options.afterLogin && loginBinding) queuedLogins.current.set(loginBinding.id, loginBinding);
      return;
    }
    pending.current = true;
    setLoading(true);
    setError("");
    try {
      const value = await scope.request<State>("/settings/claude");
      setState(value);
      if (value.runtime_configured) {
        const candidates = loginBinding ? value.bindings.filter(item => item.id === loginBinding.id && item.revision === loginBinding.revision)
          : value.bindings.filter(item => item.status === "pending" && item.cleanup_state !== "pending");
        for (const binding of candidates) {
          if (binding.status === "pending" || binding.status === "active") await syncModels(binding, options.force);
        }
      }
    } catch (error) {
      fail(error, "Claude 연결 상태를 불러오지 못했습니다. 연결 새로고침으로 다시 확인해 주세요.");
    } finally { finish(); }
  }

  async function action(kind: "create" | "probe" | "disconnect", binding?: Binding) {
    if (pending.current || loading || !state) return;
    if (kind !== "disconnect" && !state.runtime_configured) return;
    if (kind === "create" && !label.trim()) return;
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      if (kind === "create") {
        // Keep the server receipt even if the subsequent list refresh fails.
        const submittedLabel = label.trim();
        const created = await scope.request<Pick<Binding, "id" | "revision">>("/settings/claude", "POST", { label: submittedLabel });
        setState(current => current && { ...current, bindings: [...current.bindings, {
          id: created.id, revision: created.revision, label: submittedLabel,
          status: "pending", cleanup_state: "not_requested",
        }] });
        setGuideOpen(false);
        setNotice("연결을 추가했습니다. 구독 로그인을 눌러 Claude 공식 로그인과 코드 입력을 완료하세요.");
      }
      if (binding && kind === "probe") {
        await scope.request(`/settings/claude/${encodeURIComponent(binding.id)}/probe`, "POST", {
          revision: binding.revision, model: (models[binding.id] ?? "").trim(),
        });
        setNotice("정확한 모델의 텍스트 응답을 확인했습니다. 아래 사용할 모델에서 선택하고 저장하세요.");
        onVerified();
      }
      if (binding && kind === "disconnect") {
        await scope.request(`/settings/claude/${encodeURIComponent(binding.id)}`, "DELETE", { revision: binding.revision });
        setState(current => current && { ...current, bindings: current.bindings.map(item => item.id === binding.id
          ? { ...item, status: "disconnected", cleanup_state: "pending" } : item) });
        setNotice("개인 연결을 해제했습니다. 로그인 파일 정리 상태는 아래에 표시됩니다. 원격 세션 해제는 Claude 계정 설정에서 확인하세요.");
        onVerified();
      }
      try {
        setState(await scope.request<State>("/settings/claude"));
      } catch (error) {
        fail(error, "요청은 처리했지만 최신 상태를 불러오지 못했습니다. Claude 연결 새로고침을 눌러 주세요.");
      }
    } catch (error) {
      fail(error, kind === "probe"
        ? "텍스트 연결을 확인하지 못했습니다. 서버의 공식 로그인 상태와 정확한 모델 ID를 확인해 주세요. 요청은 자동으로 다시 보내지 않습니다."
        : "요청 결과를 확인하지 못했습니다. Claude 연결 새로고침으로 현재 상태를 먼저 확인해 주세요.");
    } finally {
      finish();
    }
  }

  function create(event: FormEvent) {
    event.preventDefault();
    void action("create");
  }

  return <section className="admin-card claude-connections" aria-label="내 Claude 연결">
    <div className="section-heading">
      <div><h3>Claude</h3><p>내 Claude 구독 또는 Anthropic API로 연결하세요.</p></div>
      <button className="primary" aria-expanded={guideOpen} aria-controls={guideId}
        onClick={() => setGuideOpen(open => !open)}>Claude 연결</button>
    </div>
    {loading ? <p role="status">Claude 연결 상태를 확인하는 중…</p>
      : state && <p className="settings-hint">{state.runtime_configured
        ? "구독 실행 환경이 설정되어 있습니다. 공식 로그인과 모델 확인을 마치면 사용할 수 있습니다."
        : "구독 연결은 서버 설정이 필요합니다. API 연결은 별도로 사용할 수 있습니다."}</p>}
    {error && <p className="notice settings-alert" role="alert">{error}</p>}
    {notice && <p className="settings-success" role="status">{notice}</p>}

    {guideOpen && <div id={guideId} className="claude-connection-options" role="region" aria-label="Claude 연결 방식">
      <section className="claude-connection-option" aria-label="Claude 구독 연결">
        <h4>Claude 구독으로 연결</h4>
        <p>본인 계정으로 공식 Claude Code에 로그인합니다. 현재 텍스트 대화를 지원하며 도구·첨부·기억 저장·일정·백그라운드 업무는 지원하지 않습니다.</p>
        {loading ? <p>서버 설정을 확인하고 있습니다.</p>
          : !state ? <p>서버 상태를 확인한 뒤 구독 연결을 시작할 수 있습니다. 아래에서 연결을 새로고침하세요.</p>
          : state.runtime_configured ? <form className="claude-create-form" onSubmit={create}>
            <label>Claude 연결 이름<input value={label} onChange={event => setLabel(event.target.value)} maxLength={80} required disabled={busy} /></label>
            <button className="secondary" disabled={busy || !label.trim()}>내 Claude 연결 추가</button>
          </form> : <>
            <p><strong>이 서버의 구독 실행 환경이 아직 준비되지 않았습니다.</strong></p>
            <p>{onConnectApi ? "서버에 공식 Claude Code 격리 실행 환경을 구성한 뒤 이 화면에서 새로고침하세요." : "설치 관리자에게 Claude 구독 실행 환경 설정을 요청하세요. 설정 후 이 화면에서 연결할 수 있습니다."}</p>
            <a href={setupGuide} target="_blank" rel="noreferrer">서버 설정 안내 열기</a>
          </>}
      </section>
      <section className="claude-connection-option" aria-label="Anthropic API 연결">
        <h4>Anthropic API로 연결</h4>
        <p>Anthropic API 키로 연결합니다. Claude 구독과 별도로 사용량이 청구되며, 설치 구성원이 함께 사용하는 연결입니다.</p>
        {onConnectApi
          ? <button className="secondary" onClick={onConnectApi}>Claude API 연결</button>
          : <p>공유 API 연결은 관리자가 추가합니다. 연결된 모델은 아래 ‘사용할 모델’에서 선택하세요.</p>}
      </section>
    </div>}

    {state?.bindings.map(binding => <div key={binding.id} className="claude-binding" role="group" aria-label={binding.label}>
      <strong>{binding.label}</strong>
      <p>{binding.cleanup_state === "pending" ? "로컬 실행 차단됨 · 개인 로그인 파일 정리 대기"
        : binding.status === "active" ? "텍스트 확인됨"
        : binding.status === "pending" ? "공식 로그인과 모델 확인 대기"
        : binding.status === "disconnected" ? "로컬 연결 해제됨" : "다시 연결 필요"}</p>
      {binding.status === "active" && <p className="settings-hint">다른 Claude 계정을 사용하려면 위 ‘Claude 연결’에서 새 연결을 추가하세요.</p>}
      {state.runtime_configured && (binding.status === "pending" || binding.status === "active") && <>
        {binding.status === "pending" && <ClaudeSubscriptionLogin key={`${binding.id}:${binding.revision}`} accountId={accountId} bindingId={binding.id}
          revision={binding.revision} disabled={busy || loading} onExpired={onExpired} onSucceeded={() => void refresh(binding, { afterLogin: true })} />}
        <p className="settings-hint">공식 로그인 후 사용 가능한 모델을 자동으로 확인합니다. 모델 확인은 본인 구독을 사용합니다.</p>
        {syncStatus[binding.id] === "checking" && <p role="status">사용할 Claude 모델을 확인하는 중…</p>}
        {syncStatus[binding.id] === "ready" && <p role="status">Claude 모델을 준비했습니다. 아래 ‘사용할 모델’에서 선택하고 저장하세요.</p>}
        {syncStatus[binding.id] === "login_required" && <p role="status">{binding.status === "pending"
          ? "공식 로그인이 필요합니다. 구독 로그인을 완료하면 모델 목록을 자동으로 불러옵니다."
          : "Claude 로그인 상태를 확인할 수 없습니다. 위 ‘Claude 연결’에서 새 연결을 추가해 로그인하세요."}</p>}
        {syncStatus[binding.id] === "unavailable" && <p role="status">사용 가능한 모델을 확인하지 못했습니다. 구독 이용 상태와 서버 연결을 확인한 뒤 다시 시도하세요.</p>}
        <p className="settings-hint">모델 목록 다시 확인은 아직 확인하지 못한 모델만 구독으로 확인합니다. 이미 확인한 모델은 다시 호출하지 않습니다.</p>
        <button className="secondary" disabled={busy || loading} onClick={() => void refresh(binding, { force: true })}>모델 목록 다시 확인</button>
        {binding.status === "pending" && <details className="probe-details">
          <summary>터미널로 연결하기 (고급)</summary>
          <p>서버 터미널에서 아래 도우미를 실행하세요. 내주소와 프로필 경로를 자신의 무수리 이메일과 설치 관리자가 제공한 경로로 바꾸세요. 비밀번호는 도우미가 별도로 묻습니다.</p>
          <code className="claude-login-command">{`moosuri subscriptions claude-login --server ${window.location.origin} --email 내주소 --binding ${binding.id} --revision ${binding.revision} --profile-file /private/path/claude-profile.json`}</code>
          <p>터미널 연결을 선택해도 동일한 공식 로그인과 코드 입력을 사용합니다.</p>
        </details>}
        <details className="probe-details">
          <summary>정확한 모델 직접 확인 (고급)</summary>
          <label>정확한 모델 ID<input aria-label={`Claude 모델 ${binding.label}`} value={models[binding.id] ?? ""} maxLength={200}
            onChange={event => setModels(old => ({ ...old, [binding.id]: event.target.value }))}
            disabled={busy || loading} placeholder="공식 계정에서 사용할 정확한 claude- 버전 ID" /></label>
          <p className="settings-hint">목록에 없는 정확한 모델 ID를 알고 있을 때만 직접 확인하세요. 이 확인은 본인 구독을 실제로 사용합니다.</p>
          <button className="secondary" disabled={busy || loading || !(models[binding.id] ?? "").trim()}
            onClick={() => void action("probe", binding)}>Claude 텍스트 연결 확인</button>
        </details>
      </>}
      <button className="text-button" disabled={busy || loading || binding.status === "disconnected"}
        onClick={() => void action("disconnect", binding)}>Claude 연결 해제</button>
    </div>)}
    {busy && <p role="status">Claude 연결 요청을 처리하는 중…</p>}
    <button className="text-button" disabled={busy || loading} onClick={() => void refresh()}>Claude 연결 새로고침</button>
  </section>;
}
