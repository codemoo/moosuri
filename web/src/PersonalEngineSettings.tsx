import { useEffect, useRef, useState, type FormEvent } from "react";
import type { ProviderKind } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import "./personal-management.css";

type Choice = { model: string; effort: string | null };
type Engine = {
  configured: boolean;
  enabled: boolean;
  revision: number;
  connection_revision: string | null;
  kind: ProviderKind | null;
  host_default: Choice | null;
  choices: Choice[];
  default_choice: Choice | null;
  default_stale: boolean;
};
const path = "/personal/admin/engine";
const engineGuide = "https://github.com/codemoo/moosuri/blob/main/deployment/kiri/subscriptions.md";
const providerNames: Record<ProviderKind, string> = {
  open_ai: "OpenAI",
  anthropic: "Anthropic",
  open_router: "OpenRouter",
  compatible: "OpenAI 호환 API",
};
const effortNames = new Map([
  ["none", "사용 안 함"], ["minimal", "최소"], ["low", "낮음"], ["medium", "보통"],
  ["high", "높음"], ["xhigh", "매우 높음"], ["max", "최대"],
]);

function effortLabel(effort: string | null | undefined) {
  return effort == null ? "모델 기본값" : effortNames.get(effort) ?? effort;
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
  const [busy, setBusy] = useState<"loading" | "saving" | null>("loading");
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
    setBusy("loading");
    void scope.request<Engine>(path, "GET", undefined, controller.signal).then(next => {
      if (!controller.signal.aborted) setEngine(next);
    }).catch(error => {
      if (controller.signal.aborted) return;
      setEngine(null);
      fail(error, "엔진 설정을 불러오지 못했습니다. 새로고침해 주세요.");
    }).finally(() => {
      if (!controller.signal.aborted) setBusy(null);
    });
    return () => controller.abort();
  }, [scope, refresh]);

  async function save(defaultChoice: Choice | null, enabled: boolean) {
    if (busy || pending.current || !engine?.connection_revision) return;
    pending.current = true;
    setBusy("saving");
    setError("");
    setNotice("");
    try {
      const next = await scope.request<Engine>(path, "POST", {
        revision: engine.revision,
        connection_revision: engine.connection_revision,
        enabled,
        default_choice: defaultChoice,
      }, undefined, { timeoutMs: 30_000 });
      setEngine(next);
      setNotice(next.enabled
        ? "엔진 설정을 저장했습니다. 새 요청부터 적용됩니다."
        : "엔진 설정을 저장했습니다. 새 모델 요청이 일시 중지됐습니다.");
      setBusy(null);
    } catch (error) {
      if (error instanceof ApiFailure && [401, 403].includes(error.status)) {
        fail(error, "");
        setBusy(null);
      } else if (!isAborted(error)) {
        setError(error instanceof ApiFailure && error.status === 409
          ? "엔진 설정이 변경되었습니다. 최신 설정을 확인한 뒤 다시 저장해 주세요."
          : "저장 결과를 받지 못했습니다. 새로고침된 엔진 상태를 확인해 주세요.");
        setBusy("loading");
        setRefresh(value => value + 1);
      }
    } finally {
      pending.current = false;
    }
  }
  const effective = engine?.default_stale ? null : engine?.default_choice ?? engine?.host_default;
  return (
    <section className="member-page member-management" aria-labelledby="preview-engine-title">
      <header className="member-page-heading management-heading">
        <h1 id="preview-engine-title">어시스턴트 엔진</h1>
        <button className="member-page-action" type="button" disabled={!!busy} onClick={() => {
          setError("");
          setNotice("");
          setBusy("loading");
          setRefresh(value => value + 1);
        }}>새로고침</button>
      </header>
      {error && <p role="alert" className="preview-error">{error}</p>}
      {notice && <p className="management-note" role="status">{notice}</p>}
      {busy === "loading" && <p className="management-note" role="status">엔진 설정 확인 중…</p>}
      {engine && (
        <section className="management-group" aria-labelledby="engine-connection-title">
          <h2 id="engine-connection-title">엔진 상태</h2>
          {engine.configured ? (
            <dl className="member-page-list management-list management-definitions">
              <div className="management-row management-connection">
                <dt>새 모델 요청</dt>
                <dd>{!engine.enabled ? "일시 중지" : engine.default_stale ? "모델 설정 필요" : "허용"}</dd>
              </div>
              <div className="management-row management-connection">
                <dt>공급자</dt>
                <dd>{engine.kind ? providerNames[engine.kind] ?? "확인 필요" : "확인 필요"}</dd>
              </div>
              {effective && <div className="management-row management-connection">
                <dt>{engine.default_choice ? "저장된 모델" : "서버 기본 모델"}</dt>
                <dd>{effective.model} · {effortLabel(effective.effort)}</dd>
              </div>}
            </dl>
          ) : (
            <p className="member-page-list management-row management-note">
              엔진 연결을 준비해야 회원이 어시스턴트에 요청할 수 있습니다. 설치 서버에서 모델 연결을 설정한 뒤 새로고침해 주세요.
            </p>
          )}
          <p className="management-note"><a href={engineGuide} target="_blank" rel="noreferrer">설치 서버에서 엔진 연결 설정하기</a></p>
        </section>
      )}
      {engine?.configured && (
        <section className="management-group" aria-labelledby="engine-model-title">
          <h2 id="engine-model-title">설치 모델 설정</h2>
          <div className="member-page-list">
            <ChoiceForm key={`${engine.connection_revision}:${engine.revision}`} engine={engine} busy={busy} save={save} />
          </div>
        </section>
      )}
    </section>
  );
}

function ChoiceForm({engine, busy, save}: {
  engine: Engine;
  busy: "loading" | "saving" | null;
  save: (choice: Choice | null, enabled: boolean) => void;
}) {
  const { choices, default_choice: current, host_default: hostDefault, default_stale: stale, enabled } = engine;
  const [choice, setChoice] = useState(current);
  const [active, setActive] = useState(enabled);
  const models = [...new Set(choices.map(value => value.model))];
  const levels = choices.filter(value => value.model === choice?.model);
  const valid = !choice || levels.some(value => sameChoice(value, choice));
  const canSave = !busy && valid && (stale || !sameChoice(choice, current) || active !== enabled);
  function submit(event: FormEvent) {
    event.preventDefault();
    if (canSave) save(choice, active);
  }
  return (
    <form className="management-list" aria-labelledby="engine-model-title" onSubmit={submit}>
      <p className="management-row management-note">선택한 모델과 추론 강도는 엔진을 이용하는 모든 회원의 새 요청에 적용됩니다.</p>
      {stale && <p className="management-row management-note" role="status">엔진 연결이 바뀌었습니다. 새 연결에서 사용할 모델과 추론 강도를 확인하고 저장해 주세요.</p>}
      <label className="management-row management-toggle">
        <span>새 모델 요청 허용</span>
        <input type="checkbox" checked={active} disabled={!!busy}
          onChange={event => setActive(event.target.checked)} />
      </label>
      {!active && <p className="management-row management-note">일시 중지하면 모든 회원의 새 모델 요청을 차단합니다. 공급자에 이미 전달된 요청은 계속 처리될 수 있습니다.</p>}
      <label className="management-row management-field">
        <span>모델</span>
        <select value={choice?.model ?? ""} disabled={!!busy} onChange={event => {
          const selected = choices.filter(value => value.model === event.target.value);
          setChoice(selected.find(value => value.effort === null) ?? selected[0] ?? null);
        }}>
          <option value="">서버 기본값 사용</option>
          {choice && !models.includes(choice.model) && (
            <option value={choice.model} disabled>{choice.model} · 사용 불가</option>
          )}
          {models.map(model => <option key={model}>{model}</option>)}
        </select>
      </label>
      {choice ? (
        <label className="management-row management-field">
          <span>추론 강도</span>
          <select value={choice.effort ?? ""} disabled={!!busy || levels.length === 0}
            onChange={event => setChoice(levels.find(value => (value.effort ?? "") === event.target.value) ?? null)}>
            {!valid && <option value={choice.effort ?? ""} disabled>{effortLabel(choice.effort)} · 사용 불가</option>}
            {levels.map(value => (
              <option key={value.effort ?? ""} value={value.effort ?? ""}>
                {effortLabel(value.effort)}
              </option>
            ))}
          </select>
        </label>
      ) : current && hostDefault && (
        <p className="management-row management-note">서버 기본 모델은 {hostDefault.model}, 추론 강도는 {effortLabel(hostDefault.effort)}입니다.</p>
      )}
      <div className="management-row management-footer">
        <button className="member-page-action management-primary" disabled={!canSave}>
          {busy === "saving" ? "저장 중…" : "설치 설정 저장"}
        </button>
      </div>
    </form>
  );
}
