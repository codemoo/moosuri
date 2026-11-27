import { useEffect, useRef, useState, type FormEvent } from "react";
import type { ProviderKind } from "../shared/api-types";
import { QuotaPanel } from "./QuotaPanel";
import { PersonalEngineSetup } from "./PersonalEngineSetup";
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
  connection_mode?: "api" | "claude_code" | "chatgpt_plan" | null;
  host_default: Choice | null;
  choices: Choice[];
  default_choice: Choice | null;
  default_stale: boolean;
};
const path = "/personal/admin/engine";
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
  const [current, setCurrent] = useState(false);
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
      if (!controller.signal.aborted) {
        setEngine(next);
        setCurrent(true);
      }
    }).catch(error => {
      if (controller.signal.aborted) return;
      setCurrent(false);
      fail(error, "엔진 설정을 불러오지 못했습니다. 새로고침해 주세요.");
    }).finally(() => {
      if (!controller.signal.aborted) setBusy(null);
    });
    return () => controller.abort();
  }, [scope, refresh]);

  async function save(defaultChoice: Choice | null, enabled: boolean) {
    if (busy || pending.current || !current || !engine?.connection_revision) return;
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
  const state = !engine?.configured ? "unconfigured"
    : !engine.enabled ? "paused" : engine.default_stale ? "stale" : "ready";
  const stateLabel = {
    unconfigured: "연결 설정 필요", paused: "일시 중지", stale: "모델 설정 필요", ready: "허용",
  }[state];
  return (
    <section className="member-page member-management" aria-labelledby="preview-engine-title">
      <header className="member-page-heading management-heading">
        <div>
          <h1 id="preview-engine-title">어시스턴트 엔진</h1>
          <p className="management-note">이 설치에서 회원들이 사용하는 모델과 추론 강도를 관리합니다.</p>
        </div>
        <button className="member-page-action" type="button" disabled={!!busy} onClick={() => {
          setError("");
          setNotice("");
          setBusy("loading");
          setRefresh(value => value + 1);
        }}>{busy === "loading" ? "설정 확인 중…" : "새로고침"}</button>
      </header>
      {error && <p role="alert" className="preview-error">{error}</p>}
      {notice && <p className="management-feedback" role="status">{notice}</p>}
      {busy === "loading" && !engine && <p className="management-note" role="status">엔진 설정을 불러오는 중…</p>}
      {engine && (
        <section className="member-page-list management-engine-overview" aria-labelledby="engine-connection-title">
          <div className="management-section-heading">
            <h2 id="engine-connection-title">엔진 상태</h2>
            <div className="management-engine-request-state">
              {engine.configured && <span className="management-note">새 모델 요청</span>}
              <span className="management-status" data-state={current ? state : "stale"}>{current ? stateLabel : "최신 상태 확인 필요"}</span>
            </div>
          </div>
          <div className="management-engine-current">
            {engine.configured ? (
              <>
                {effective && <p className="management-current-model">{effective.model} · {effortLabel(effective.effort)}</p>}
                <p className="management-note">{engine.kind ? providerNames[engine.kind] ?? "공급자 확인 필요" : "공급자 확인 필요"}
                  {effective && (engine.default_choice ? " · 관리자 지정" : " · 서버 기본값")}
                </p>
              </>
            ) : (
              <p className="management-note">설치 서버에서 엔진 연결을 설정하면 회원이 어시스턴트에 요청할 수 있습니다.</p>
            )}
            {!current && <p className="management-note" role="status">마지막으로 확인한 상태입니다. 새로고침하면 설정을 편집할 수 있습니다.</p>}
          </div>

        </section>
      )}
      {engine?.configured && (
        <section className="management-group" aria-labelledby="engine-model-title">
          <div className="management-section-heading"><h2 id="engine-model-title">설치 모델 설정</h2></div>
          <div className="member-page-list">
            <ChoiceForm key={`${engine.connection_revision}:${engine.revision}`} engine={engine} busy={busy} editable={current} save={save} />
          </div>

        </section>
      )}
      {engine && <>
        {(!engine.configured || (engine.kind === "anthropic" && ["api", "claude_code"].includes(engine.connection_mode ?? ""))) && <PersonalEngineSetup key={account} account={account} subscriptionRevision={engine.connection_mode === "claude_code" ? engine.connection_revision : undefined} expired={expired} unavailable={unavailable} onSaved={() => setRefresh(value => value + 1)} />}
        {engine.configured && <ProviderUsage key={account + "/" + engine.connection_revision + "/" + engine.revision} engine={engine} account={account} current={current} expired={expired} unavailable={unavailable} />}
        <p className="management-note">연결을 완료하면 여기에서 사용할 모델과 추론 강도를 확인하고, 대화에서 요청을 보내세요.</p>
      </>}
    </section>
  );
}

function ProviderUsage({ engine, account, current, expired, unavailable }: {
  engine: Engine; account: string; current: boolean; expired: () => void; unavailable: () => void;
}) {
  const mode = engine.connection_mode;
  const links: [string, string][] = [];
  if (mode === "claude_code") links.push(["Claude 구독 사용량", "https://claude.ai/settings/usage"]);
  else if (mode === "chatgpt_plan") links.push(["ChatGPT 사용량", "https://chatgpt.com/codex/settings/usage"]);
  else if (engine.kind === "open_ai") {
    links.push(["OpenAI API 사용량", "https://platform.openai.com/usage"]);
    if (!mode) links.push(["ChatGPT 사용량", "https://chatgpt.com/codex/settings/usage"]);
  } else if (engine.kind === "anthropic") {
    links.push(["Anthropic API 사용량", "https://platform.claude.com/settings/usage"]);
    if (!mode) links.push(["Claude 구독 사용량", "https://claude.ai/settings/usage"]);
  } else if (engine.kind === "open_router") links.push(["OpenRouter 사용량", "https://openrouter.ai/activity"]);
  const source = mode === "claude_code" ? "Claude Code 로그인" : mode === "chatgpt_plan" ? "ChatGPT 공식 로그인"
    : mode === "api" ? "API 연결" : "연결 방식 확인 필요";
  return <section className="member-page-list management-engine-overview" aria-labelledby="engine-quota">
    <h2 id="engine-quota">공급자 잔여 한도</h2>
    {current && engine.enabled && mode === "api" && engine.kind === "open_router" && engine.connection_revision
      ? <QuotaPanel accountId={account} id={engine.connection_revision} revision={engine.revision} kind="kiri" onExpired={expired} onUnavailable={unavailable}/>
      : <p>{source} · 잔여량 확인 불가 · 초기화 시각 확인 불가</p>}
    <p className="management-note">{engine.kind === "open_router" && mode === "api" ? "OpenRouter는 설치 API 키의 한도와 사용량을 제공합니다. " : ""}
      {links.length === 0 ? "호환 API 또는 로컬 모델은 연결을 제공하는 운영자에게 한도를 확인하세요." : "해당 연결로 로그인한 공급자 계정에서 확인하세요."}</p>
    {links.map(([name, url]) => <a key={url} href={url} target="_blank" rel="noreferrer">{name} 열기</a>)}
    <details>
      <summary>무수리 회원별 사용 집계</summary>
      <p>웹에서 회원별 합계를 조회할 수 없습니다. 설치 서버의 모델 원장에서 호출 수와 예산에 반영된 토큰 합계를 확인할 수 있습니다. 미정산 요청에는 예약 토큰이 포함됩니다.</p>
      <a href="https://github.com/codemoo/moosuri/blob/main/deployment/kiri/admin.md#회원별-사용-집계" target="_blank" rel="noreferrer">서버에서 회원별 집계 확인하기</a>
    </details>
  </section>;
}

function ChoiceForm({ engine, busy, editable, save }: {
  engine: Engine;
  busy: "loading" | "saving" | null;
  editable: boolean;
  save: (choice: Choice | null, enabled: boolean) => void;
}) {
  const { choices, default_choice: current, host_default: hostDefault, default_stale: stale, enabled } = engine;
  const [choice, setChoice] = useState(current);
  const [active, setActive] = useState(enabled);
  const models = [...new Set(choices.map(value => value.model))];
  const levels = choices.filter(value => value.model === choice?.model);
  const valid = !choice || levels.some(value => sameChoice(value, choice));
  const changed = !sameChoice(choice, current) || active !== enabled;
  const disabled = !!busy || !editable;
  const canSave = !disabled && valid && (stale || changed);
  const next = choice ?? hostDefault;
  function submit(event: FormEvent) {
    event.preventDefault();
    if (canSave) save(choice, active);
  }
  return (
    <form className="management-list" aria-labelledby="engine-model-title" onSubmit={submit}>
      <label className="management-row management-toggle">
        <span>새 모델 요청 허용</span>
        <input type="checkbox" checked={active} disabled={disabled}
          onChange={event => setActive(event.target.checked)} />
      </label>
      <label className="management-row management-field">
        <span>모델</span>
        <select value={choice?.model ?? ""} disabled={disabled} onChange={event => {
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
          <select value={choice.effort ?? ""} disabled={disabled || levels.length === 0}
            onChange={event => setChoice(levels.find(value => (value.effort ?? "") === event.target.value) ?? null)}>
            {!valid && <option value={choice.effort ?? ""} disabled>{effortLabel(choice.effort)} · 사용 불가</option>}
            {levels.map(value => (
              <option key={value.effort ?? ""} value={value.effort ?? ""}>
                {effortLabel(value.effort)}
              </option>
            ))}
          </select>
        </label>
      ) : null}
      <div className="management-row management-save">
        {(!valid || changed || stale) && <div className="management-save-copy" role="status">
          {stale && <p>엔진 연결이 바뀌었습니다. 새 연결에 쓸 모델을 확인하고 저장하세요.</p>}
          {!valid ? (
            <p>현재 연결에서 이 모델과 추론 강도를 사용할 수 없습니다. 다른 조합이나 서버 기본값을 선택하세요.</p>
          ) : (
            <>
              <p><strong>저장하면</strong> {active && next ? `${next.model} · ${effortLabel(next.effort)} 조합을 모든 회원의 새 요청에 적용합니다.` : "모든 회원의 새 모델 요청이 일시 중지됩니다."}</p>
              {!active && <p className="management-note">공급자에 이미 전달된 요청은 계속 처리될 수 있습니다.</p>}
            </>
          )}
        </div>}
        <div className="management-footer">
          {changed && <button type="button" className="member-page-action" disabled={disabled} onClick={() => {
            setChoice(current);
            setActive(enabled);
          }}>변경 취소</button>}
          <button className="member-page-action management-primary" disabled={!canSave}>
            {busy === "saving" ? "저장 중…" : "설치 설정 저장"}
          </button>
        </div>
      </div>
    </form>
  );
}
