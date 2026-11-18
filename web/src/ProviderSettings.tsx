import { QuotaPanel } from "./QuotaPanel";
import { PersonalCompatibleSettings } from "./PersonalCompatibleSettings";
import {ClaudeExecutionSettings} from "./ClaudeExecutionSettings";
import {AudioSettingsPanel} from "./AudioSettings";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import type {
  ModelCatalog,
  ModelChoice,
  ModelSelection,
  ModelSettings,
  ProviderConfig,
  ProviderKind,
  ProviderSummary,
  ProvidersResponse,
} from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";
import "./provider-settings.css";

type Props = { accountId: string; onExpired: () => void };
type Credential =
  | { action: "keep" }
  | { action: "clear" }
  | { action: "replace"; value: string };
type ProviderDraft = {
  name: string;
  config: ProviderConfig;
  enabled: boolean;
  credential: Credential;
};
const names: Record<ProviderKind, string> = {
  open_ai: "OpenAI",
  anthropic: "Anthropic (Claude API)",
  open_router: "OpenRouter",
  compatible: "로컬 / 호환 API",
};

function selectionKey(selection: ModelSelection | null) {
  return selection
    ? JSON.stringify([
        ...(selection.source && selection.source !== "shared" ? [selection.source] : []),
        selection.provider_id,
        selection.provider_revision,
        selection.model,
        selection.effort,
      ])
    : "";
}
function choiceSelection(choice: ModelChoice): ModelSelection {
  return {
    ...(choice.source && choice.source !== "shared" ? {source:choice.source} : {}),
    provider_id: choice.provider_id,
    provider_revision: choice.provider_revision,
    model: choice.model,
    effort: choice.effort,
  };
}
function currentChoices(provider: ProviderSummary) {
  if (
    !provider.enabled ||
    (provider.config.kind !== "compatible" && !provider.has_credential)
  )
    return [];
  return provider.choices.filter(
    (choice) =>
      choice.provider_id === provider.id &&
      choice.provider_revision === provider.revision,
  );
}
function RecoveryBudgetNotice({accountId}: {accountId:string}) {
  const scope=useScope(accountId);
  const [value,setValue]=useState<{accounting_state:string;inference_calls:number;committed_calls:number;stt_duration_ms:number;committed_stt_duration_ms:number}|null>(null);
  useEffect(()=>{let active=true;void scope.request<typeof value>("/recovery-budget").then(v=>{if(active)setValue(v);}).catch(()=>{});return()=>{active=false;};},[scope]);
  if(value?.accounting_state!=="accounting_unknown")return null;
  const remaining=Math.max(0,value.inference_calls-value.committed_calls);
  return <p role="status">복원 전 사용량은 알 수 없습니다. 새 추론 요청 허용량: {remaining}회, 음성 입력: {Math.max(0,value.stt_duration_ms-value.committed_stt_duration_ms)/1000}초. {remaining===0?"운영자가 중지된 서버에서 새 허용량을 배정해야 합니다. ":""}요청 수와 음성 입력 길이의 한도이며 토큰·금액 한도가 아닙니다. 결과를 확인하지 못한 요청도 차감됩니다.</p>;
}
function settingsError(error: unknown) {
  if (!(error instanceof ApiFailure))
    return "서버 응답을 확인하지 못했습니다. 연결 상태를 확인하고 새로고침해 주세요.";
  if (error.code === "recovery_forward_allocation_required") return "과거 사용량이 불명확합니다. 운영자가 중지된 서버에서 새 요청 허용량을 배정해야 합니다.";
  if (error.code === "recovery_forward_allowance_exhausted") return "복원 후 새 요청 허용량을 모두 사용했습니다. 운영자가 새 허용량을 배정해야 합니다.";
  if (error.status === 403) return "이 설정을 변경할 권한이 없습니다.";
  if (error.status === 409)
    return "연결 정보가 변경되었거나 모델을 사용할 수 없습니다. 새로고침 후 다시 확인해 주세요.";
  if (error.status === 429)
    return "요청이 많습니다. 잠시 뒤 다시 시도해 주세요.";
  if (error.code === "model_identity_unverified")
    return "응답 모델을 확인하지 못했습니다. 모델 목록의 정확한 버전 ID로 다시 확인해 주세요.";
  if (error.code === "vision_probe_failed")
    return "이 모델 조합에서 이미지 픽셀을 정확히 읽는지 확인하지 못했습니다. 다른 이미지 지원 모델을 선택한 뒤 다시 확인하세요.";
  if (error.code === "model_tool_probe_incomplete")
    return "도구 호출과 결과 연결을 확인하지 못했습니다. 일반 텍스트 대화는 계속 사용할 수 있습니다.";
  if (error.code === "model_probe_incomplete")
    return "완료된 답변을 확인하지 못했습니다. 모델 ID와 추론 강도를 확인한 뒤 다시 시도해 주세요.";
  if (error.code === "model_credentials_rejected")
    return "API 키가 거부되었습니다. 연결을 수정해 올바른 키를 다시 저장해 주세요.";
  if (error.code === "model_timeout")
    return "연결 확인 시간이 초과되었습니다. 서버 상태와 주소를 확인해 주세요.";
  if (error.code === "model_protocol_failed")
    return "응답 형식을 확인하지 못했습니다. 연결 방식과 모델 ID를 확인해 주세요.";
  if (error.code.includes("credential"))
    return "API 키를 확인해 주세요. 저장된 키는 다시 표시하지 않습니다.";
  if (error.code.includes("unsupported") || error.code.includes("capability"))
    return "이 모델에서 요청한 기능이나 추론 강도를 확인하지 못했습니다.";
  if (error.code.includes("revision"))
    return "연결이 변경되었습니다. 새로고침한 뒤 다시 확인해 주세요.";
  return "요청을 완료하지 못했습니다. 입력과 연결 상태를 확인해 주세요.";
}
function Alert({ children }: { children: string }) {
  return (
    <p className="notice settings-alert" role="alert">
      {children}
    </p>
  );
}
function toolCapability(choice: ModelChoice) {
  if (choice.function_tools) return "도구 사용 가능";
  return "도구 확인 필요";
}

function toolProbePath(choice: ModelChoice, canManageShared: boolean) {
  const id = encodeURIComponent(choice.provider_id);
  switch (choice.source) {
    case "personal_claude_code": return `/settings/claude/${id}/tool-probe`;
    case "personal_chat_gpt": return `/settings/chatgpt/${id}/tool-probe`;
    case "personal_compatible": return `/settings/compatible-providers/${id}/tool-probe`;
    default: return canManageShared ? `/admin/providers/${id}/tool-probe` : null;
  }
}

function ModelPicker({choices, draft, unavailable, busy, edit, installation = false}: {
  choices: ModelChoice[]; draft: string; unavailable: boolean; busy: boolean; edit: (value: string) => void; installation?: boolean;
}) {
  const groups = new Map<string, ModelChoice[]>();
  for (const choice of choices) {
    const key = selectionKey({ ...choice, effort: null });
    const group = groups.get(key);
    if (group) group.push(choice);
    else groups.set(key, [choice]);
  }
  const selected = choices.find(choice => selectionKey(choice) === draft);
  const modelKey = selected ? selectionKey({ ...selected, effort: null }) : draft;
  const levels = groups.get(modelKey) ?? [];
  return <>
              <label>
                {installation ? "기본 모델" : "모델"}
                <select value={modelKey} disabled={busy} onChange={event => {
                  const group = groups.get(event.target.value);
                  const first = group?.find(choice => choice.function_tools && choice.effort === selected?.effort)
                    ?? group?.find(choice => choice.function_tools && choice.effort === null)
                    ?? group?.find(choice => choice.function_tools)
                    ?? group?.find(choice => choice.effort === null) ?? group?.[0];
                  edit(first ? selectionKey(first) : "");
                }}>
                  <option value="">{installation ? "기본 모델 지정 안 함" : "설치 기본값 사용"}</option>
                  {unavailable && <option value={draft} disabled>{installation ? "현재 기본 모델 · 사용할 수 없음" : "현재 선택 · 사용할 수 없음"}</option>}
                  {[...groups].map(([key, [choice]]) => (
                    <option key={key} value={key}>{choice.provider_name} · {choice.model}</option>
                  ))}
                </select>
              </label>
              <label>
                {installation ? "기본 추론 강도" : "추론 강도"}
                <select disabled={busy || levels.length <= 1} value={selected ? draft : ""} onChange={event => edit(event.target.value)}>
                  {!selected && <option value="">{unavailable ? "사용할 수 없음" : "설치 기본값 사용"}</option>}
                  {levels.map(choice => <option key={selectionKey(choice)} value={selectionKey(choice)}>{choice.effort ?? "모델 기본값"} · {toolCapability(choice)}</option>)}
                </select>
              </label>
  </>;
}

export function MemberModelSettings({ accountId, memberId, onExpired }: Props & { memberId: string }) {
  const scope = useScope(accountId, `member-model:${memberId}`);
  const path = `/admin/accounts/${encodeURIComponent(memberId)}/model`;
  const [settings, setSettings] = useState<ModelSettings | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [refresh, setRefresh] = useState(0);
  const pending = useRef(false);
  function accept(value: ModelSettings) {
    setSettings(value);
    setDraft(selectionKey(value.selection));
  }
  function fail(error: unknown) {
    if (error instanceof ApiFailure && error.status === 401) onExpired();
    else if (!isAborted(error)) setError(settingsError(error));
  }
  useEffect(() => {
    const controller = new AbortController();
    setBusy(true);
    setSettings(null);
    void scope.request<ModelSettings>(path, "GET", undefined, controller.signal)
      .then(accept).catch(fail).finally(() => {
        if (!controller.signal.aborted) setBusy(false);
      });
    return () => controller.abort();
  }, [scope, path, refresh]);
  const selected = settings?.choices.find(choice => selectionKey(choice) === draft);
  async function save(event: FormEvent) {
    event.preventDefault();
    if (pending.current || busy || !settings || (draft && !selected)) return;
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      accept(await scope.request<ModelSettings>(path, "PUT", {
        selection: selected ? choiceSelection(selected) : null,
      }));
      setNotice("회원의 모델 설정을 저장했습니다.");
    } catch (error) {
      fail(error);
      if (!isAborted(error)) setRefresh(value => value + 1);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  return (
    <div className="model-settings">
      {error && <Alert>{error}</Alert>}
      {notice && <p role="status">{notice}</p>}
      {settings ? (
        <>
          <form aria-label="회원 모델 설정" className="model-selection-form" onSubmit={save}>
            <ModelPicker choices={settings.choices} draft={draft} unavailable={!!draft && !selected} busy={busy} edit={setDraft} />
            <button className="primary" disabled={busy || (!!draft && !selected) || draft === selectionKey(settings.selection)}>
              회원 모델 저장
            </button>
          </form>
          <p>
            {settings.effective
              ? `현재 적용 · ${settings.effective.provider_name} · ${settings.effective.model} · ${settings.effective.effort ?? "모델 기본값"}`
              : "현재 사용할 수 있는 모델이 없습니다."}
          </p>
        </>
      ) : busy ? <p role="status">회원 모델 확인 중…</p> : null}
      <button className="text-button" disabled={busy} onClick={() => {
        setError("");
        setNotice("");
        setRefresh(value => value + 1);
      }}>
        회원 모델 새로고침
      </button>
    </div>
  );
}

export function PersonalModelSettings({ accountId, onExpired, onConnectClaudeApi }: Props & { onConnectClaudeApi?: () => void }) {
  const scope = useScope(accountId);
  const [settings, setSettings] = useState<ModelSettings | null>(null);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [retry, setRetry] = useState(0);
  const dirty = useRef(false);
  const [claudeRefresh, setClaudeRefresh] = useState(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  function fail(error: unknown) {
    if (isAborted(error)) return;
    if (error instanceof ApiFailure && error.status === 401) onExpired();
    else setError(settingsError(error));
  }
  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    setNotice("");
    void scope
      .request<ModelSettings>("/settings/model")
      .then((value) => {
        if (!active) return;
        setSettings(value);
        if (!dirty.current) setDraft(selectionKey(value.selection));
        setLoading(false);
      })
      .catch((error) => {
        if (active && !isAborted(error)) {
          fail(error);
          setLoading(false);
        }
      });
    return () => { active = false; };
  }, [scope, retry]);
  async function save(event: FormEvent) {
    event.preventDefault();
    if (pending.current || !settings) return;
    const choice = settings.choices.find(
      (choice) => selectionKey(choiceSelection(choice)) === draft,
    );
    if (draft && !choice) {
      setError("현재 확인된 모델과 추론 강도만 선택할 수 있습니다.");
      return;
    }
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const value = await scope.request<ModelSettings>(
        "/settings/model",
        "PUT",
        { selection: choice ? choiceSelection(choice) : null },
      );
      setSettings(value);
      setDraft(selectionKey(value.selection));
      dirty.current = false;
      setNotice("내 모델을 저장했습니다.");
    } catch (error) {
      fail(error);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  const effective = settings?.effective;
  const probePath = effective && !effective.function_tools
    ? toolProbePath(effective, !!onConnectClaudeApi) : null;
  async function verifyEffectiveTools() {
    if (!effective || !probePath || pending.current) return;
    const choice = choiceSelection(effective);
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const checked = await scope.request<ModelChoice>(probePath, "POST", {
        revision: choice.provider_revision,
        model: choice.model,
        effort: choice.effort,
      });
      if (!checked.function_tools || selectionKey(choiceSelection(checked)) !== selectionKey(choice)) {
        throw new Error("Tool proof did not match current model");
      }
      const current = await scope.request<ModelSettings>("/settings/model");
      setSettings(current);
      if (!dirty.current) setDraft(selectionKey(current.selection));
      if (current.effective?.function_tools && selectionKey(choiceSelection(current.effective)) === selectionKey(choice)) {
        setNotice("이 모델의 도구 호출을 확인했습니다. 새 요청부터 도구를 사용할 수 있습니다.");
      } else {
        setError("모델 설정이 변경되었거나 도구 확인을 적용하지 못했습니다. 현재 적용 모델을 확인해 주세요.");
      }
    } catch (error) {
      fail(error);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  const visionPath = effective && (effective.source === "personal_claude_code" || effective.source === "personal_chat_gpt")
    ? `/settings/${effective.source === "personal_claude_code" ? "claude" : "chatgpt"}/${encodeURIComponent(effective.provider_id)}/vision-probe` : null;
  async function verifyEffectiveVision() {
    if (!effective || !visionPath || pending.current) return;
    const choice = choiceSelection(effective);
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await scope.request<{vision:boolean}>(visionPath, "POST", {
        revision: choice.provider_revision, model: choice.model, effort: choice.effort,
      });
      if (result.vision !== true) throw new Error("Image proof incomplete");
      const current = await scope.request<ModelSettings>("/settings/model");
      setSettings(current);
      if (!dirty.current) setDraft(selectionKey(current.selection));
      if (current.effective && selectionKey(choiceSelection(current.effective)) === selectionKey(choice)) {
        setNotice("이 모델과 추론 강도의 이미지 연결을 확인했습니다. 새 대화 요청에 이미지를 첨부할 수 있습니다.");
      } else {
        setError("현재 적용 모델이 변경되었습니다. 새 모델의 이미지 연결을 확인해 주세요.");
      }
    } catch (error) { fail(error); }
    finally { pending.current = false; setBusy(false); }
  }
  const unavailableDraft =
    !!draft &&
    !settings?.choices.some(
      (choice) => selectionKey(choiceSelection(choice)) === draft,
    );
  function edit(value: string) {
    dirty.current = value !== selectionKey(settings?.selection ?? null);
    setDraft(value);
    setNotice("");
  }
  return (
    <main className="admin-scroll model-settings">
      <RecoveryBudgetNotice accountId={accountId}/>
      <h2 className="sr-only">내 모델</h2>
      {error && <Alert>{error}</Alert>}
      {notice && (
        <p className="settings-success" role="status">
          {notice}
        </p>
      )}
      <section className="admin-card">
        <div className="section-heading">
          <h3>사용할 모델</h3>
        </div>
        {loading ? (
          <p className="muted" role="status">
            모델 설정을 불러오는 중…
          </p>
        ) : settings ? (
          <>
            <form onSubmit={save} className="model-selection-form">
              <ModelPicker choices={settings.choices} draft={draft} unavailable={unavailableDraft} busy={busy} edit={edit} />
              <button
                className="primary"
                disabled={
                  busy ||
                  unavailableDraft ||
                  draft === selectionKey(settings.selection)
                }
              >
                {busy ? "저장 중…" : "선택 저장"}
              </button>
            </form>
            <div className="effective-model">
              <span className="eyebrow">현재 적용</span>
              {settings.effective ? (
                <>
                  <strong>{settings.effective.model}</strong>
                  <p>
                    {settings.effective.provider_name} ·{" "}
                    {settings.effective.effort ?? "기본 강도"}
                  </p>
                  <p>{toolCapability(settings.effective)}</p>
                  {probePath && <button type="button" disabled={busy} onClick={() => void verifyEffectiveTools()}>
                    현재 모델 도구 확인
                  </button>}
                  {visionPath && <button type="button" disabled={busy} onClick={() => void verifyEffectiveVision()}>
                    현재 모델 이미지 확인
                  </button>}
                  {visionPath && <p className="settings-hint">임의 색상 이미지를 실제로 읽어 확인하며 본인 구독을 사용합니다. 확인은 이 연결·모델·추론 강도에만 적용됩니다.</p>}
                  {!settings.effective.function_tools && !probePath && <p>
                    이 공용 모델의 도구 확인은 관리자가 할 수 있습니다.
                  </p>}
                </>
              ) : (
                <>
                  <strong>사용할 수 있는 모델이 없습니다</strong>
                  <p>
                    {settings.selection
                      ? "내가 선택한 연결이 비활성화되었거나 다시 확인해야 합니다. 다른 모델로 자동 전환하지 않습니다."
                      : "설치 기본 모델이 아직 준비되지 않았습니다. 관리자에게 연결 확인을 요청하세요."}
                  </p>
                </>
              )}
            </div>

          </>
        ) : (
          <p className="muted">모델 설정을 불러오지 못했습니다.</p>
        )}
        <button
          className="text-button"
          disabled={busy || loading}
          onClick={() => { setRetry((value) => value + 1); setClaudeRefresh(value => value + 1); }}
        >
          새로고침
        </button>
      </section>
      <p className="settings-footnote">
        모델 선택은 다음 메시지부터 적용됩니다. 다시 시도는 기존 실행의 모델
        설정을 사용합니다.
      </p>
      <ClaudeExecutionSettings accountId={accountId} onExpired={onExpired} onVerified={()=>setRetry(v=>v+1)} refreshKey={claudeRefresh} onConnectApi={onConnectClaudeApi} />
      <PersonalChatGpt key={"chatgpt:"+accountId} accountId={accountId} onExpired={onExpired} choices={settings?.choices ?? []} onVerified={()=>setRetry(v=>v+1)} />
      <PersonalCompatibleSettings key={"compatible:"+accountId} accountId={accountId} onExpired={onExpired} onVerified={()=>setRetry(v=>v+1)} />
    </main>
  );
}

export function AdminProviderSettings({ accountId, onExpired }: Props) {
  const scope = useScope(accountId);
  const [data, setData] = useState<ProvidersResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [retry, setRetry] = useState(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [initialKind] = useState<ProviderKind>(() => new URLSearchParams(window.location.search).get("connect") === "anthropic" ? "anthropic" : "open_ai");
  const [editor, setEditor] = useState<ProviderSummary | "new" | null>(() => initialKind === "anthropic" ? "new" : null);
  const [defaultKey, setDefaultKey] = useState("");
  function fail(error: unknown) {
    if (isAborted(error)) return;
    if (error instanceof ApiFailure && error.status === 401) onExpired();
    else setError(settingsError(error));
  }
  useEffect(() => {
    setLoading(true);
    setError("");
    setNotice("");
    void scope
      .request<ProvidersResponse>("/admin/providers")
      .then((value) => {
        setData(value);
        setDefaultKey(selectionKey(value.default_selection));
        setLoading(false);
      })
      .catch((error) => {
        if (!isAborted(error)) {
          fail(error);
          setLoading(false);
        }
      });
  }, [scope, retry]);
  async function mutation<T>(
    run: () => Promise<T>,
    accept: (value: T) => void,
  ): Promise<boolean> {
    if (pending.current || loading) return false;
    pending.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const value = await run();
      accept(value);
      return true;
    } catch (error) {
      fail(error);
      return false;
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }
  function saveProvider(
    draft: ProviderDraft,
    existing: ProviderSummary | null,
  ) {
    return mutation(
      () =>
        scope.request<ProviderSummary>(
          existing
            ? `/admin/providers/${encodeURIComponent(existing.id)}`
            : "/admin/providers",
          existing ? "PUT" : "POST",
          {
            ...draft,
            ...(existing ? { expected_revision: existing.revision } : {}),
          },
        ),
      (provider) => {
        setData((current) =>
          current
            ? {
                ...current,
                providers: existing
                  ? current.providers.map((item) =>
                      item.id === provider.id ? provider : item,
                    )
                  : [...current.providers, provider],
              }
            : current,
        );
        setEditor(null);
        setNotice("연결을 저장했습니다. 사용할 모델을 확인해 주세요.");
      },
    );
  }
  function probe(
    provider: ProviderSummary,
    model: string,
    effort: string | null,
    tools = false,
  ) {
    return mutation(
      () =>
        scope.request<ModelChoice>(
          `/admin/providers/${encodeURIComponent(provider.id)}/${tools ? "tool-probe" : "probe"}`,
          "POST",
          { revision: provider.revision, model, effort },
        ),
      (choice) => {
        if (
          choice.provider_id !== provider.id ||
          choice.provider_revision !== provider.revision
        ) {
          setError(
            "연결이 변경되어 확인 결과를 적용하지 않았습니다. 새로고침해 주세요.",
          );
          return;
        }
        if (choice.model !== model || choice.effort !== effort) {
          setError(
            "응답 모델을 확인하지 못했습니다. 모델 목록의 정확한 버전 ID로 다시 확인해 주세요.",
          );
          return;
        }
        if (tools && choice.function_tools !== true) {
          setError("도구 지원을 확인하지 못했습니다. 기존 모델 선택은 유지됩니다.");
          return;
        }
        setData((current) =>
          current
            ? {
                ...current,
                providers: current.providers.map((item) =>
                  item.id === provider.id &&
                  item.revision === choice.provider_revision
                    ? {
                        ...item,
                        choices: [
                          ...item.choices.filter(
                            (old) =>
                              selectionKey(choiceSelection(old)) !==
                              selectionKey(choiceSelection(choice)),
                          ),
                          choice,
                        ],
                      }
                    : item,
                ),
              }
            : current,
        );
        setNotice(
          tools ? "도구 호출과 결과 연결을 확인했습니다. 이 모델에서 개인 자료 검색을 사용할 수 있습니다."
            : "모델과 추론 강도를 확인했습니다. 이제 기본 모델로 선택할 수 있습니다.",
        );
      },
    );
  }
  async function catalog(
    provider: ProviderSummary,
  ): Promise<ModelCatalog | null> {
    let found: ModelCatalog | null = null;
    await mutation(
      () =>
        scope.request<ModelCatalog>(
          `/admin/providers/${encodeURIComponent(provider.id)}/models`,
          "POST",
          { revision: provider.revision },
        ),
      (value) => {
        found = value;
      },
    );
    return found;
  }
  async function saveDefault(event: FormEvent) {
    event.preventDefault();
    if (!data) return;
    const choices = data.providers.flatMap(currentChoices);
    const choice = choices.find(
      (choice) => selectionKey(choiceSelection(choice)) === defaultKey,
    );
    if (defaultKey && !choice) {
      setError("현재 연결에서 확인한 모델만 기본값으로 선택할 수 있습니다.");
      return;
    }
    await mutation(
      () =>
        scope.request<ProvidersResponse>("/admin/providers/default", "POST", {
          selection: choice ? choiceSelection(choice) : null,
        }),
      (value) => {
        setData(value);
        setDefaultKey(selectionKey(value.default_selection));
        setNotice("설치 기본 모델을 저장했습니다.");
      },
    );
  }
  const choices = data?.providers.flatMap(currentChoices) ?? [];
  const unavailableDefault =
    !!defaultKey &&
    !choices.some(
      (choice) => selectionKey(choiceSelection(choice)) === defaultKey,
    );
  return (
    <main className="admin-scroll model-settings">
      <RecoveryBudgetNotice accountId={accountId}/>
      <h2 className="sr-only">모델 연결</h2>
      {error && <Alert>{error}</Alert>}
      {notice && (
        <p className="settings-success" role="status">
          {notice}
        </p>
      )}
      {busy && (
        <p className="settings-progress" role="status">
          요청 처리 중…
        </p>
      )}
      <AudioSettingsPanel accountId={accountId} providers={data?.providers??[]} onExpired={onExpired}/>
      <div className="settings-toolbar">
        <button
          className="primary"
          disabled={busy || loading}
          onClick={() => {
            setEditor("new");
            setNotice("");
          }}
        >
          연결 추가 <span aria-hidden="true">＋</span>
        </button>
        <button
          className="text-button"
          disabled={busy || loading}
          onClick={() => {
            setEditor(null);
            setRetry((value) => value + 1);
          }}
        >
          새로고침
        </button>
      </div>
      {editor && !loading && data && (
        <ProviderEditor
          key={editor === "new" ? "new" : `${editor.id}:${editor.revision}`}
          provider={editor === "new" ? null : editor}
          initialKind={initialKind}
          busy={busy}
          save={saveProvider}
          close={() => setEditor(null)}
        />
      )}
      {loading ? (
        <p className="muted" role="status">
          모델 연결을 불러오는 중…
        </p>
      ) : data?.providers.length === 0 ? (
        <section className="admin-card settings-empty">
          <h3>첫 연결을 추가해 주세요</h3>
          <p>
            OpenAI, Anthropic, OpenRouter 또는 로컬 호환 API를 사용할 수
            있습니다.
          </p>
        </section>
      ) : (
        data?.providers.map((provider) => (
          <section
            className="admin-card provider-card"
            aria-label={`${provider.name} 모델 연결`}
            key={`${provider.id}:${provider.revision}`}
          >
            <div className="section-heading">
              <div>
                <h3>{provider.name}</h3>
                <p>
                  {names[provider.config.kind]} ·{" "}
                  {provider.enabled ? "활성" : "비활성"} ·{" "}
                  {provider.has_credential ? "키 저장됨" : "키 없음"}
                </p>
              </div>
              <button
                className="secondary"
                disabled={busy || !!editor}
                onClick={() => {
                  setEditor(provider);
                  setNotice("");
                }}
              >
                수정
              </button>
            </div>
            {provider.config.kind === "compatible" && (
              <p className="provider-address">{provider.config.base_url}</p>
            )}
            <ul
              className="verified-choices"
              aria-label={`${provider.name} 확인된 모델`}
            >
              {currentChoices(provider).map((choice) => (
                <li key={selectionKey(choiceSelection(choice))}>
                  <span className="verified-dot" aria-hidden="true" />
                  <span>
                    <strong>{choice.model}</strong>
                    <small>{choice.effort ?? "기본 강도"} · {choice.function_tools ? "도구 사용 가능" : "대화 확인됨"}</small>
                  </span>
                  <button className="text-button" disabled={busy || !!editor} onClick={()=>void mutation(()=>scope.request(`/admin/providers/${encodeURIComponent(provider.id)}/vision-probe`,"POST",{revision:provider.revision,model:choice.model,effort:choice.effort}),()=>{setError("이미지 연결 확인 완료: 이 조합에서 이미지 분석을 사용할 수 있습니다.");})}>이미지 연결 확인</button>
                  {!choice.function_tools && (
                    <button className="text-button" disabled={busy || !!editor}
                      onClick={() => probe(provider, choice.model, choice.effort, true)}>
                      도구 연결 확인
                    </button>
                  )}
                </li>
              ))}
            </ul>
            {currentChoices(provider).length === 0 && (
              <p className="settings-hint">
                {provider.enabled
                  ? "현재 연결에서 확인한 모델이 없습니다. 아래에서 사용할 모델을 확인하세요."
                  : "비활성 연결은 선택하거나 확인할 수 없습니다. 다른 연결로 자동 전환하지 않습니다."}
              </p>
            )}
            <QuotaPanel accountId={accountId} id={provider.id} revision={provider.revision} kind="shared" onExpired={onExpired} />
            <ProbeForm
              provider={provider}
              busy={busy || !!editor}
              probe={probe}
              catalog={catalog}
            />
          </section>
        ))
      )}
      {data && !loading && (
        <section className="admin-card">
          <div className="section-heading">
            <div>
              <h3>설치 기본 모델</h3>
              <p>
                기본값을 따르는 사용자에게 적용됩니다. 확인한 모델과 추론 강도만
                선택할 수 있습니다.
              </p>
            </div>
          </div>
          <form onSubmit={saveDefault} className="model-selection-form">
            <ModelPicker choices={choices} draft={defaultKey} unavailable={unavailableDefault} busy={busy} edit={setDefaultKey} installation />
            <button
              className="primary"
              disabled={
                busy ||
                unavailableDefault ||
                defaultKey === selectionKey(data.default_selection)
              }
            >
              기본 모델 저장
            </button>
          </form>
          {unavailableDefault && (
            <p className="settings-hint">
              현재 기본 모델이 준비되지 않았습니다. 다른 모델로 자동 변경하지
              않습니다.
            </p>
          )}
        </section>
      )}
      <p className="settings-footnote">
        확인은 짧은 실제 추론 요청을 보냅니다. 확인한 모델은 대화에서 선택해
        사용할 수 있습니다.
      </p>
    </main>
  );
}

function ProbeForm({
  provider,
  busy,
  probe,
  catalog,
}: {
  provider: ProviderSummary;
  busy: boolean;
  probe: (
    provider: ProviderSummary,
    model: string,
    effort: string | null,
  ) => Promise<boolean>;
  catalog: (provider: ProviderSummary) => Promise<ModelCatalog | null>;
}) {
  const [model, setModel] = useState("");
  const [effort, setEffort] = useState("");
  const [models, setModels] = useState<ModelCatalog | null>(null);
  const modelList = useId();
  const effortList = useId();
  const unavailable =
    !provider.enabled ||
    (provider.config.kind !== "compatible" && !provider.has_credential);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || unavailable || !model.trim()) return;
    await probe(provider, model.trim(), effort.trim() || null);
  }
  return (
    <details className="probe-details">
      <summary>모델 확인하기</summary>
      <p className="fine">
        모델 ID를 정확히 입력하세요. 이 조합으로 짧은 실제 요청을 보내며,
        사용량이 발생할 수 있습니다.
      </p>
      <button
        className="text-button"
        disabled={busy || unavailable}
        onClick={async () => {
          const value = await catalog(provider);
          if (value) setModels(value);
        }}
      >
        모델 목록 불러오기
      </button>
      {models && (
        <p className="fine" role="status">
          {models.truncated
            ? "일부 모델만 표시됩니다. 목록에 없으면 정확한 ID를 직접 입력할 수 있습니다."
            : models.models.length === 0
              ? "모델 목록이 비어 있습니다. 정확한 ID를 직접 입력할 수도 있습니다."
              : "목록은 연결에서 가져온 이름입니다. 사용할 조합을 아래에서 확인해야 선택할 수 있습니다."}
        </p>
      )}
      <form className="probe-form" onSubmit={submit}>
        <label>
          모델 ID
          <input
            list={modelList}
            required
            maxLength={200}
            autoComplete="off"
            value={model}
            disabled={busy || unavailable}
            onChange={(event) => setModel(event.target.value)}
          />
        </label>
        <datalist id={modelList}>
          {models?.models.map((model) => (
            <option key={model.id} value={model.id}>
              {model.name}
            </option>
          ))}
        </datalist>
        {provider.config.kind !== "compatible" && (
          <label>
            추론 강도 (선택)
            <input
              list={effortList}
              pattern="[A-Za-z0-9_-]{1,32}"
              maxLength={32}
              autoComplete="off"
              placeholder="예: 지원이 확인된 high"
              value={effort}
              disabled={busy || unavailable}
              onChange={(event) => setEffort(event.target.value)}
            />
            <datalist id={effortList}>
              {["none", "minimal", "low", "medium", "high", "xhigh", "max"].map(
                (effort) => (
                  <option key={effort} value={effort} />
                ),
              )}
            </datalist>
          </label>
        )}
        <button
          className="secondary"
          disabled={busy || unavailable || !model.trim()}
        >
          연결·모델 확인
        </button>
      </form>
      {provider.config.kind === "compatible" && (
        <p className="fine">
          호환 연결의 추론 강도 지정은 아직 지원하지 않습니다.
        </p>
      )}
    </details>
  );
}

function ProviderEditor({
  provider,
  initialKind,
  busy,
  save,
  close,
}: {
  provider: ProviderSummary | null;
  initialKind: ProviderKind;
  busy: boolean;
  save: (
    draft: ProviderDraft,
    existing: ProviderSummary | null,
  ) => Promise<boolean>;
  close: () => void;
}) {
  const [name, setName] = useState(provider?.name ?? (initialKind === "anthropic" ? "Claude API" : ""));
  const [kind, setKind] = useState<ProviderKind>(
    provider?.config.kind ?? initialKind,
  );
  const [url, setUrl] = useState(provider?.config.base_url ?? "");
  const [ips, setIps] = useState(provider?.config.allowed_ips.join(", ") ?? "");
  const [routers, setRouters] = useState(
    provider?.config.router_providers.join(", ") ?? "",
  );
  const [zdr, setZdr] = useState(provider?.config.zero_data_retention ?? false);
  const [enabled, setEnabled] = useState(provider?.enabled ?? true);
  const [key, setKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [error, setError] = useState("");
  const submitting = useRef(false);
  const editorRef = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!provider && initialKind === "anthropic")
      editorRef.current?.scrollIntoView?.({ block: "start" });
  }, [provider, initialKind]);
  const split = (value: string) =>
    value
      .split(/[\s,]+/)
      .map((part) => part.trim())
      .filter(Boolean);
  const config: ProviderConfig = {
    kind,
    base_url: kind === "compatible" ? url.trim() : null,
    allowed_ips: kind === "compatible" ? split(ips) : [],
    router_providers: kind === "open_router" ? split(routers) : [],
    zero_data_retention: kind === "open_router" && zdr,
  };
  const changedConfig =
    !!provider && JSON.stringify(config) !== JSON.stringify(provider.config);
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || submitting.current) return;
    setError("");
    if (kind === "open_router" && config.router_providers.length === 0) {
      setError("사용할 OpenRouter 제공업체를 하나 이상 입력해 주세요.");
      return;
    }
    if (!provider && kind !== "compatible" && !key) {
      setError("이 연결에는 API 키를 입력해야 합니다.");
      return;
    }
    if (changedConfig && provider?.has_credential && !key && !clearKey) {
      setError(
        "연결 설정을 바꾸면 새 API 키를 입력하거나 저장된 키를 명시적으로 제거해야 합니다.",
      );
      return;
    }
    const credential: Credential = key
      ? { action: "replace", value: key }
      : clearKey || !provider
        ? { action: "clear" }
        : { action: "keep" };
    setKey("");
    submitting.current = true;
    try {
      await save({ name: name.trim(), config, enabled, credential }, provider);
    } finally {
      submitting.current = false;
    }
  }
  return (
    <section
      className="admin-card provider-editor"
      aria-labelledby="provider-editor-title"
      ref={editorRef}
    >
      <div className="section-heading">
        <div>
          <h3 id="provider-editor-title">
            {provider ? "연결 수정" : "새 모델 연결"}
          </h3>
          <p>키는 저장 요청 후 입력창에서 지워지며, 다시 표시하지 않습니다.</p>
          {kind === "anthropic" && <p>Claude 구독과 별도로 과금되는 Anthropic API 키가 필요합니다. 이 연결은 설치 구성원이 함께 사용합니다.</p>}
        </div>
        <button
          className="text-button"
          type="button"
          disabled={busy}
          onClick={close}
        >
          취소
        </button>
      </div>
      {error && <Alert>{error}</Alert>}
      <form onSubmit={submit} className="provider-form">
        <div className="settings-field-pair">
          <label>
            연결 이름
            <input
              required
              maxLength={100}
              autoComplete="off"
              value={name}
              disabled={busy}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label>
            연결 방식
            <select
              value={kind}
              disabled={busy}
              onChange={(event) => setKind(event.target.value as ProviderKind)}
            >
              {Object.entries(names).map(([kind, label]) => (
                <option key={kind} value={kind}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        </div>
        {kind === "compatible" && (
          <>
            <label>
              서버 API 주소
              <input
                type="url"
                required
                placeholder="http://127.0.0.1:1234/v1"
                value={url}
                disabled={busy}
                onChange={(event) => setUrl(event.target.value)}
              />
            </label>
            <p className="field-help">
              moosuri 서버에서 접근할 주소입니다. 브라우저 기기의 localhost와
              다를 수 있습니다.
            </p>
            <label>
              허용 IP
              <input
                placeholder="예: 127.0.0.1"
                value={ips}
                disabled={busy}
                onChange={(event) => setIps(event.target.value)}
              />
            </label>
            <p className="field-help">
              접근을 허용할 서버 IP를 쉼표로 구분해 입력하세요.
            </p>
          </>
        )}
        {kind === "open_router" && (
          <>
            <label>
              사용할 제공업체
              <input
                required
                autoComplete="off"
                placeholder="승인한 OpenRouter 제공업체 슬러그"
                value={routers}
                disabled={busy}
                onChange={(event) => setRouters(event.target.value)}
              />
            </label>
            <p className="field-help">
              쉼표로 구분하세요. 지정한 제공업체 밖으로 자동 전환하지 않습니다.
            </p>
            <label className="check-label">
              <input
                type="checkbox"
                checked={zdr}
                disabled={busy}
                onChange={(event) => setZdr(event.target.checked)}
              />
              데이터를 보존하지 않는 경로만 사용
            </label>
          </>
        )}
        <label>
          API 키
          {provider?.has_credential
            ? clearKey
              ? " (제거 예정)"
              : changedConfig
                ? " (새 키 또는 키 제거 필요)"
                : " (비워 두면 기존 키 유지)"
            : kind === "compatible"
              ? " (선택)"
              : ""}
          <input
            type="password"
            autoComplete="new-password"
            autoFocus={!provider && initialKind === "anthropic"}
            spellCheck={false}
            value={key}
            disabled={busy || clearKey}
            required={!provider && kind !== "compatible"}
            onChange={(event) => setKey(event.target.value)}
          />
        </label>
        {provider?.has_credential && (
          <label className="check-label">
            <input
              type="checkbox"
              checked={clearKey}
              disabled={busy}
              onChange={(event) => {
                setClearKey(event.target.checked);
                setKey("");
              }}
            />
            저장된 키 제거
          </label>
        )}
        {changedConfig && provider?.has_credential && (
          <p className="settings-hint">
            연결 설정을 바꾸면 새 키 또는 키 제거가 필요합니다. 이전 모델 확인
            결과도 초기화됩니다.
          </p>
        )}
        <label className="check-label">
          <input
            type="checkbox"
            checked={enabled}
            disabled={busy}
            onChange={(event) => setEnabled(event.target.checked)}
          />
          이 연결 사용
        </label>
        <p className="fine">
          연결을 수정하면 모델을 다시 확인해야 합니다. 비활성화해도 다른 연결로
          자동 전환하지 않습니다.
        </p>
        <div className="settings-form-actions">
          <button className="primary" disabled={busy}>
            연결 저장
          </button>
        </div>
      </form>
    </section>
  );
}


type PersonalBinding = {
  id: string;
  revision: number;
  label: string;
  status: string;
  remote_revoke: string;
  handoff_phase?: string | null;
};
function PersonalChatGpt({accountId, onExpired, onVerified, choices}: Props & {onVerified: () => void; choices: ModelChoice[]}) {
  const scope = useScope(accountId);
  const [items, setItems] = useState<PersonalBinding[]>([]);
  const [guide, setGuide] = useState(false);
  const [guideBinding, setGuideBinding] = useState<PersonalBinding | null>(null);
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [model, setModel] = useState<Record<string, string>>({});
  const [effort, setEffort] = useState<Record<string, string>>({});
  const [catalogs, setCatalogs] = useState<Record<string, ModelCatalog>>({});
  const [copied, setCopied] = useState(false);
  const [waiting, setWaiting] = useState(false);
  const [baseline, setBaseline] = useState<PersonalBinding[]>([]);
  const [pollVersion, setPollVersion] = useState(0);
  const verified = useRef(onVerified);
  verified.current = onVerified;
  const remote = window.location.protocol === "https:" && !["localhost", "127.0.0.1", "[::1]"].includes(window.location.hostname);
  const origin = remote ? window.location.origin : `${window.location.protocol}//127.0.0.1${window.location.port ? ":" + window.location.port : ""}`;
  const returning = guideBinding ? ` --binding ${guideBinding.id} --revision ${guideBinding.revision}` : "";
  const command = remote
    ? `curl -fsSL https://raw.githubusercontent.com/codemoo/moosuri/main/install.sh | bash -s -- --chatgpt-login --server ${origin}${returning}`
    : `moosuri subscriptions chatgpt-login --server ${origin} --email 내주소${returning}`;
  function beginGuide(binding: PersonalBinding | null) {
    setGuideBinding(binding); setBaseline(items); setGuide(true); setCopied(false); setWaiting(true); setNotice("");
  }
  useEffect(() => {
    if (!guide || !waiting) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const deadline = Date.now() + 10 * 60_000;
    async function poll() {
      try {
        const next = await scope.request<PersonalBinding[]>("/settings/chatgpt");
        if (!active) return;
        setItems(next);
        const connected = next.find(binding => binding.status === "active" && (guideBinding
          ? binding.id === guideBinding.id && binding.revision > guideBinding.revision
          : !baseline.some(old => old.id === binding.id && old.status === "active")));
        if (connected) {
          setGuide(false); setWaiting(false);
          setNotice("ChatGPT에 연결했습니다. 아래에서 대화 모델의 텍스트 연결을 확인하세요.");
          verified.current();
          return;
        }
      } catch (e) {
        if (!active || isAborted(e)) return;
        if (e instanceof ApiFailure && e.status === 401) { onExpired(); return; }
      }
      if (active && Date.now() < deadline) timer = setTimeout(poll, 3500);
      else if (active) setWaiting(false);
    }
    timer = setTimeout(poll, 1000);
    return () => { active = false; clearTimeout(timer); };
  }, [scope, guide, waiting, baseline, guideBinding, pollVersion]);
  function fail(error: unknown) {
    if (isAborted(error)) return;
    if (error instanceof ApiFailure && error.status === 401) onExpired();
    else setNotice("개인 ChatGPT 요청을 완료하지 못했습니다. 연결 상태와 사용 한도를 확인하세요.");
  }
  useEffect(() => {
    let active = true;
    scope.request<PersonalBinding[]>("/settings/chatgpt").then((value) => {
      if (active) setItems(value);
    }).catch((error: unknown) => { if (active) fail(error); });
    return () => { active = false; };
  }, [scope]);
  const activeCatalogs = JSON.stringify(items.filter(b => b.status === "active").map(b => [b.id, b.revision]));
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    const bindings = JSON.parse(activeCatalogs) as [string, number][];
    void (async () => {
      for (const [id, revision] of bindings) {
        try {
          const catalog = await scope.request<ModelCatalog>(`/settings/chatgpt/${encodeURIComponent(id)}/models`, "POST", {revision}, controller.signal);
          if (active) setCatalogs(old => ({...old, [id]: catalog}));
        } catch (e) {
          if (!active || isAborted(e)) return;
          if (e instanceof ApiFailure && e.status === 401) { onExpired(); return; }
        }
      }
    })();
    return () => { active = false; controller.abort(); };
  }, [scope, activeCatalogs]);
  async function toolProbe(binding: PersonalBinding, choice: ModelChoice) {
    if (busy) return;
    setBusy(true); setNotice("");
    try {
      const result = await scope.request<ModelChoice>(`/settings/chatgpt/${encodeURIComponent(binding.id)}/tool-probe`,"POST",{revision: binding.revision, model: choice.model, effort: choice.effort});
      if (!result.function_tools || selectionKey(choiceSelection(result)) !== selectionKey(choiceSelection(choice))) throw new Error("incomplete proof");
      onVerified();
      setNotice("도구 호출과 결과 연결을 확인했습니다. 새 대화 턴에서 확인한 도구를 사용할 수 있습니다.");
    } catch (error) { fail(error); }
    finally { setBusy(false); }
  }
  async function act(binding: PersonalBinding, operation: "models" | "probe" | "disconnect") {
    if (busy) return;
    setBusy(true);
    setNotice("");
    try {
      const path = `/settings/chatgpt/${encodeURIComponent(binding.id)}`;
      if (operation === "disconnect") {
        const result = await scope.request<{remote_revocation_confirmed: boolean}>(path, "DELETE", {revision: binding.revision});
        setItems(await scope.request<PersonalBinding[]>("/settings/chatgpt"));
        onVerified();
        setNotice(result.remote_revocation_confirmed
          ? "ChatGPT 연결을 해제했습니다."
          : "로컬 연결을 해제했습니다. 원격 해제는 확인되지 않았습니다. ChatGPT 설정에서 앱 연결을 해제할 수 있습니다.");
      } else if (operation === "models") {
        const result = await scope.request<ModelCatalog>(`${path}/models`, "POST", {revision: binding.revision});
        setCatalogs((old) => ({...old, [binding.id]: result}));
      } else {
        await scope.request(`${path}/probe`, "POST", {
          revision: binding.revision,
          model: (model[binding.id] ?? "").trim(),
          effort: (effort[binding.id] ?? "").trim() || null,
        });
        onVerified();
        setNotice("텍스트 연결을 확인했습니다. 사용할 모델을 선택하세요.");
      }
    } catch (error) { fail(error); }
    finally { setBusy(false); }
  }
  return (
    <section className="admin-card">
      <h3>내 ChatGPT</h3>
      <button className="primary" onClick={() => beginGuide(null)}>ChatGPT 구독 연결</button>
      {guide && <div role="region" aria-label="ChatGPT 로그인 안내">
        <h4>{guideBinding ? `${guideBinding.label} 다시 연결` : "새 ChatGPT 연결"}</h4>
        <p>브라우저를 쓰는 컴퓨터의 터미널에서 아래 명령을 실행하세요. {remote ? "서버의 SSH 연결 이름과 무수리 계정으로 본인 확인 후 공식 ChatGPT 로그인 창이 열립니다." : "도우미가 공식 ChatGPT 로그인 창을 엽니다."}</p>
        <pre className="login-command"><code>{command}</code></pre>
        <button type="button" onClick={async () => {
          try { await navigator.clipboard.writeText(command); setCopied(true); }
          catch { setNotice("위 명령을 선택해 복사하세요."); }
        }}>{copied ? "복사됨" : "명령 복사"}</button>
        <p role="status">{waiting ? "로그인을 마치면 연결 상태가 여기에 표시됩니다." : "연결 대기가 끝났습니다."}</p>
        {!waiting && <button onClick={() => { setWaiting(true); setPollVersion(v => v + 1); }}>연결 상태 다시 확인</button>}
        <button onClick={() => setGuide(false)}>닫기</button>
      </div>}
      <button disabled={busy} onClick={() => {void scope.request<PersonalBinding[]>("/settings/chatgpt").then(setItems).catch(fail);}}>연결 새로고침</button>
      <details className="connection-capabilities">
        <summary>지원 범위</summary>
        <p>자료·메일·일정 조회, 계산, 기억과 기록 관리, 로컬 메일 초안을 지원합니다. 맡긴 업무는 로그아웃한 뒤에도 실행됩니다. 메일 발송은 최종 내용을 보고 매번 허락해야 합니다.</p>
        <p>현재 요청으로 기본 캘린더의 단일 시간 일정을 만들거나 같은 날짜 안에서 수정·삭제할 수 있습니다. 예약 업무의 메일·일정 변경 및 브라우저·볼트 사용은 아직 지원하지 않습니다.</p>
        <a href="https://chatgpt.com/settings/usage" target="_blank" rel="noreferrer">ChatGPT 사용량</a>
      </details>
      {notice && <p role="status">{notice}</p>}
      {items.map((binding) => (
        <div key={binding.id} role="group" aria-label={binding.label}>
          <strong>{binding.label}</strong>
          <p>{binding.status} · 원격 해제: {binding.remote_revoke}</p>
          <p>{binding.handoff_phase === "validating" ? "보호된 연결을 서버에서 확인하는 중" : binding.handoff_phase === "prepared" || binding.handoff_phase === "committed" ? "이 컴퓨터에서 로그인·전송 대기 중" : binding.status === "active" ? "연결됨 · 텍스트 모델 확인 가능" : "다시 연결 필요"}</p>
          <button disabled={busy} onClick={() => beginGuide(binding)}>이 연결로 다시 로그인</button>
          {binding.status === "active" && <>
            <QuotaPanel accountId={accountId} id={binding.id} revision={binding.revision} kind="chatgpt" onExpired={onExpired} />
            <button disabled={busy} onClick={() => void act(binding, "models")}>내 모델 목록</button>
            <label>정확한 모델<input placeholder="모델 ID 선택 또는 입력" value={model[binding.id] ?? ""} onChange={event => {
              setModel(old => ({...old, [binding.id]: event.target.value}));
              setEffort(old => ({...old, [binding.id]: ""}));
            }} list={`personal-models-${binding.id}`} disabled={busy} /></label>
            <label>확인할 추론 강도
              {catalogs[binding.id]?.models.find(m => m.id === model[binding.id])?.efforts?.length
                ? <select value={effort[binding.id] ?? ""} onChange={event => setEffort(old => ({...old, [binding.id]: event.target.value}))} disabled={busy}>
                    <option value="">모델 기본값</option>
                    {catalogs[binding.id].models.find(m => m.id === model[binding.id])!.efforts.map(level => <option key={level}>{level}</option>)}
                  </select>
                : <input placeholder="모델 기본값" value={effort[binding.id] ?? ""} onChange={event => setEffort(old => ({...old, [binding.id]: event.target.value}))} disabled={busy} />}
            </label>
            <button disabled={busy || !(model[binding.id] ?? "").trim()} onClick={() => void act(binding, "probe")}>텍스트 연결 확인</button>
            <datalist id={`personal-models-${binding.id}`}>{catalogs[binding.id]?.models.map((entry) => <option key={entry.id} value={entry.id}>{entry.name}</option>)}</datalist>
            {catalogs[binding.id]?.truncated && <p>일부 모델만 표시합니다. 정확한 모델 ID를 입력할 수 있습니다.</p>}
            {choices.filter(choice => choice.source === "personal_chat_gpt" && choice.provider_id === binding.id && choice.provider_revision === binding.revision).map(choice => <div key={selectionKey(choiceSelection(choice))}>
              <span>{choice.model} · {choice.effort ?? "기본 강도"} · {choice.function_tools ? "도구 사용 가능" : "텍스트 확인됨"}</span>
              {!choice.function_tools && <button disabled={busy} onClick={() => void toolProbe(binding,choice)}>도구 연결 확인 · {choice.model} · {choice.effort ?? "기본 강도"}</button>}
            </div>)}
          </>}
          <button disabled={busy || binding.status === "disconnected"} onClick={() => void act(binding, "disconnect")}>ChatGPT 연결 해제</button>
        </div>
      ))}
    </section>
  );
}
