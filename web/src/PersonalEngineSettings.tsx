import { useEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import type { Member } from "./PersonalAdmin";

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
  members: { account_id: string; generation: number; choice: Choice | null; stale: boolean }[];
};
type Update =
  | { enabled: boolean; default_choice: Choice | null }
  | { account_id: string; generation: number; choice: Choice | null };
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
  const [members, setMembers] = useState<Member[]>([]);
  const [accountId, setAccountId] = useState("");
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
    void Promise.all([
      scope.request<Engine>(path, "GET", undefined, controller.signal),
      scope.request<{members: Member[]}>("/personal/admin/members", "GET", undefined, controller.signal),
    ]).then(([next, people]) => {
      if (controller.signal.aborted) return;
      setEngine(next);
      setMembers(people.members);
    }).catch(error => fail(error, "엔진 설정을 불러오지 못했습니다.")).finally(() => {
      if (!controller.signal.aborted) setBusy(false);
    });
    return () => controller.abort();
  }, [scope, refresh]);

  async function save(update: Update) {
    if (busy || pending.current || !engine?.connection_revision) return;
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setEngine(await scope.request<Engine>("account_id" in update ? path + "/member" : path, "POST", {
        revision: engine.revision,
        connection_revision: engine.connection_revision,
        ...update,
      }));
      setNotice("엔진 설정을 저장했습니다.");
    } catch (error) {
      if (error instanceof ApiFailure && [401, 403].includes(error.status)) {
        fail(error, "");
      } else if (!isAborted(error)) {
        setError(error instanceof ApiFailure && error.status === 409
          ? "엔진 또는 회원 정보가 변경되었습니다. 최신 설정을 확인한 뒤 다시 저장해 주세요."
          : "저장 결과를 확인하지 못했습니다. 현재 설정을 확인해 주세요.");
        setRefresh(value => value + 1);
      }
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  const available = members.filter(member => engine?.members.some(
    value => value.account_id === member.account_id && value.generation === member.generation,
  ));
  const member = available.find(member => member.account_id === accountId);
  const assignment = engine?.members.find(
    value => value.account_id === member?.account_id && value.generation === member?.generation,
  );
  const installationDefault = engine?.default_stale ? null : engine?.default_choice ?? engine?.host_default;
  const revision = `${engine?.connection_revision}:${engine?.revision}`;
  return (
    <section className="preview-gate preview-engine" aria-labelledby="preview-engine-title">
      <h1 id="preview-engine-title">어시스턴트 엔진</h1>
      {error && <p role="alert" className="preview-error">{error}</p>}
      {notice && <p role="status">{notice}</p>}
      {!engine && busy && <p role="status">엔진 설정 확인 중…</p>}
      {engine && !engine.configured && (
        <p>서버에 엔진 연결이 준비되지 않았습니다. 설치 서버에서 엔진 연결을 설정한 뒤 새로고침해 주세요.</p>
      )}
      {engine?.configured && (
        <>
          <p>연결 방식 {engine.kind ?? "확인 필요"} · {engine.enabled ? "사용 중" : "일시 중지"}</p>
          <ChoiceForm key={revision} title="설치 기본 설정"
            choices={engine.choices} current={engine.default_choice}
            inherited={engine.host_default} inheritLabel="서버 기본값 사용" stale={engine.default_stale}
            enabled={engine.enabled} busy={busy}
            save={(choice, enabled) => void save({ enabled, default_choice: choice })}
          />
          <label className="preview-engine-members">
            회원
            <select value={member?.account_id ?? ""} disabled={busy || available.length === 0}
              onChange={event => setAccountId(event.target.value)}>
              <option value="">회원 선택</option>
              {available.map(value => (
                <option key={value.account_id} value={value.account_id}>
                  {value.invitation_name ? `${value.invitation_name} · ` : ""}
                  {value.email}{value.suspended ? " · 정지" : value.ready ? "" : " · 준비 중"}
                </option>
              ))}
            </select>
          </label>
          {member && (
            <ChoiceForm key={`${revision}:${member.account_id}:${member.generation}`} title="회원 설정"
              choices={engine.choices} current={assignment?.choice ?? null} stale={assignment?.stale}
              inherited={installationDefault} inheritLabel="설치 기본값 사용" busy={busy}
              save={choice => void save({ account_id: member.account_id, generation: member.generation, choice })}
            />
          )}
        </>
      )}
      <button type="button" disabled={busy} onClick={() => {
        setError("");
        setNotice("");
        setRefresh(value => value + 1);
      }}>엔진 설정 새로고침</button>
    </section>
  );
}

function ChoiceForm({title, choices, current, inherited, inheritLabel, stale, enabled, busy, save}: {
  title: string;
  choices: Choice[];
  current: Choice | null;
  inherited: Choice | null | undefined;
  inheritLabel: string;
  stale?: boolean;
  enabled?: boolean;
  busy: boolean;
  save: (choice: Choice | null, enabled: boolean) => void;
}) {
  const [choice, setChoice] = useState(current);
  const [active, setActive] = useState(enabled ?? true);
  const models = [...new Set(choices.map(value => value.model))];
  const levels = choices.filter(value => value.model === choice?.model);
  const valid = !choice || levels.some(value => sameChoice(value, choice));
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!busy && valid) save(choice, active);
  }
  return (
    <form aria-label={title} onSubmit={submit}>
      <h2>{title}</h2>
      {stale && <p role="status">엔진 연결이 바뀌었습니다. 사용할 모델과 추론 강도를 확인한 뒤 저장해 주세요.</p>}
      {enabled !== undefined && (
        <label className="preview-engine-toggle">
          <input type="checkbox" checked={active} disabled={busy}
            onChange={event => setActive(event.target.checked)} />
          어시스턴트 사용
        </label>
      )}
      <label>
        모델
        <select value={choice?.model ?? ""} disabled={busy} onChange={event => {
          const selected = choices.filter(value => value.model === event.target.value);
          setChoice(selected.find(value => value.effort === null) ?? selected[0] ?? null);
        }}>
          <option value="">{inheritLabel}</option>
          {choice && !models.includes(choice.model) && (
            <option value={choice.model} disabled>{choice.model} · 사용 불가</option>
          )}
          {models.map(model => <option key={model}>{model}</option>)}
        </select>
      </label>
      <label>
        추론 강도
        <select value={choice?.effort ?? ""} disabled={busy || levels.length === 0}
          onChange={event => setChoice(levels.find(value => (value.effort ?? "") === event.target.value) ?? null)}>
          {!choice && <option value="">{inherited?.effort ?? "기본값"}</option>}
          {choice && !valid && <option value={choice.effort ?? ""} disabled>사용 불가</option>}
          {levels.map(value => (
            <option key={value.effort ?? ""} value={value.effort ?? ""}>
              {value.effort ?? "모델 기본값"}
            </option>
          ))}
        </select>
      </label>
      {!choice && inherited && <p>상속값 {description(inherited)}</p>}
      <button disabled={busy || !valid || (!stale && sameChoice(choice, current) && active === (enabled ?? true))}>
        {enabled === undefined ? "회원 설정 저장" : "설치 설정 저장"}
      </button>
    </form>
  );
}
