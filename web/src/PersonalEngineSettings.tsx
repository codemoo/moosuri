import { useEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import "./personal-management.css";

type Choice = { model: string; effort: string | null };
type Engine = {
  configured: boolean;
  enabled: boolean;
  revision: number;
  connection_revision: string | null;
  kind: string | null;
  host_default: Choice | null;
  choices: Choice[];
  default_choice: Choice | null;
  default_stale: boolean;
};
const path = "/personal/admin/engine";

function description(choice: Choice) {
  return `${choice.model} · ${choice.effort ?? "모델 기본값"}`;
}
function sameChoice(a: Choice | null, b: Choice | null) {
  return a?.model === b?.model && a?.effort === b?.effort;
}

export function PersonalEngineSettings({ account, expired, unavailable }: {
  account: string;
  expired: () => void;
  unavailable: () => void;
}) {
  const scope = useScope(account, "installation-engine");
  const [engine, setEngine] = useState<Engine | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const pending = useRef(false);
  function fail(error: unknown, message: string) {
    if (isAborted(error)) return;
    if (error instanceof ApiFailure && error.status === 401) expired();
    else if (error instanceof ApiFailure && error.status === 403) unavailable();
    else setError(message);
  }
  useEffect(() => {
    const controller = new AbortController();
    setBusy(true);
    setEngine(null);
    void scope.request<Engine>(path, "GET", undefined, controller.signal).then(next => {
      if (controller.signal.aborted) return;
      setEngine(next);
    }).catch(error => fail(error, "엔진 설정을 불러오지 못했습니다.")).finally(() => {
      if (!controller.signal.aborted) setBusy(false);
    });
    return () => controller.abort();
  }, [scope, refresh]);

  async function save(defaultChoice: Choice | null, enabled: boolean) {
    if (busy || pending.current || !engine?.connection_revision) return;
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setEngine(await scope.request<Engine>(path, "POST", {
        revision: engine.revision,
        connection_revision: engine.connection_revision,
        enabled,
        default_choice: defaultChoice,
      }));
      setNotice("엔진 설정을 저장했습니다.");
    } catch (error) {
      if (error instanceof ApiFailure && [401, 403].includes(error.status)) {
        fail(error, "");
      } else if (!isAborted(error)) {
        setError(error instanceof ApiFailure && error.status === 409
          ? "엔진 설정이 변경되었습니다. 최신 설정을 확인한 뒤 다시 저장해 주세요."
          : "저장 결과를 확인하지 못했습니다. 현재 설정을 확인해 주세요.");
        setRefresh(value => value + 1);
      }
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  const revision = `${engine?.connection_revision}:${engine?.revision}`;
  return (
    <section className="member-page member-management" aria-labelledby="preview-engine-title">
      <header className="member-page-heading">
        <h1 id="preview-engine-title">어시스턴트 엔진</h1>
        <button className="member-page-action" type="button" disabled={busy} onClick={() => {
          setError("");
          setNotice("");
          setRefresh(value => value + 1);
        }}>엔진 설정 새로고침</button>
      </header>
      {error && <p role="alert" className="preview-error">{error}</p>}
      {notice && <p className="management-note" role="status">{notice}</p>}
      {!engine && busy && <p className="management-note" role="status">엔진 설정 확인 중…</p>}
      {engine && (
        <section className="management-group" aria-labelledby="engine-connection-title">
          <h2 id="engine-connection-title">엔진 상태</h2>
          {engine.configured ? (
            <div className="member-page-list management-row management-connection">
              <span>연결 방식 {engine.kind ?? "확인 필요"}</span>
              <span className="management-note">{engine.enabled ? "호출 허용" : "호출 일시 중지"}</span>
            </div>
          ) : (
            <p className="member-page-list management-row management-note">
              서버에 엔진 연결이 준비되지 않았습니다. 설치 서버에서 엔진 연결을 설정한 뒤 새로고침해 주세요.
            </p>
          )}
        </section>
      )}
      {engine?.configured && (
        <section className="management-group" aria-labelledby="engine-model-title">
          <h2 id="engine-model-title">설치 모델 설정</h2>
          <div className="member-page-list">
            <ChoiceForm key={revision} engine={engine} busy={busy} save={save} />
          </div>
        </section>
      )}
    </section>
  );
}

function ChoiceForm({engine, busy, save}: {
  engine: Engine;
  busy: boolean;
  save: (choice: Choice | null, enabled: boolean) => void;
}) {
  const { choices, default_choice: current, host_default: hostDefault, default_stale: stale, enabled } = engine;
  const [choice, setChoice] = useState(current);
  const [active, setActive] = useState(enabled);
  const effective = stale ? null : current ?? hostDefault;
  const models = [...new Set(choices.map(value => value.model))];
  const levels = choices.filter(value => value.model === choice?.model);
  const valid = !choice || levels.some(value => sameChoice(value, choice));
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!busy && valid) save(choice, active);
  }
  return (
    <form className="management-list" aria-labelledby="engine-model-title" onSubmit={submit}>
      <p className="management-row management-note">선택한 모델과 추론 강도는 모든 회원의 새 모델 호출에 적용됩니다.</p>
      {effective && <p className="management-row management-note">현재 설정 {description(effective)}</p>}
      {stale && <p className="management-row management-note" role="status">엔진 연결이 바뀌었습니다. 사용할 모델과 추론 강도를 확인한 뒤 저장해 주세요.</p>}
      <label className="management-row management-toggle">
        <span>어시스턴트 사용</span>
        <input type="checkbox" checked={active} disabled={busy}
          onChange={event => setActive(event.target.checked)} />
      </label>
      <label className="management-row management-field">
        <span>모델</span>
        <select value={choice?.model ?? ""} disabled={busy} onChange={event => {
          const selected = choices.filter(value => value.model === event.target.value);
          setChoice(selected.find(value => value.effort === null) ?? selected[0] ?? null);
        }}>
          <option value="">서버 기본값 사용{hostDefault ? ` · ${description(hostDefault)}` : ""}</option>
          {choice && !models.includes(choice.model) && (
            <option value={choice.model} disabled>{choice.model} · 사용 불가</option>
          )}
          {models.map(model => <option key={model}>{model}</option>)}
        </select>
      </label>
      <label className="management-row management-field">
        <span>추론 강도</span>
        <select value={choice?.effort ?? ""} disabled={busy || levels.length === 0}
          onChange={event => setChoice(levels.find(value => (value.effort ?? "") === event.target.value) ?? null)}>
          {!choice && <option value="">{hostDefault?.effort ?? "기본값"}</option>}
          {choice && !valid && <option value={choice.effort ?? ""} disabled>사용 불가</option>}
          {levels.map(value => (
            <option key={value.effort ?? ""} value={value.effort ?? ""}>
              {value.effort ?? "모델 기본값"}
            </option>
          ))}
        </select>
      </label>
      <div className="management-row management-footer">
        <button
          className="member-page-action management-primary"
          disabled={busy || !valid || (!stale && sameChoice(choice, current) && active === enabled)}
        >
          설치 설정 저장
        </button>
      </div>
    </form>
  );
}
