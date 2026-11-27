import { useEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { PersonalClaudeSetup } from "./PersonalClaudeSetup";

type Setup = { supported: boolean; saved: boolean; connection_revision?: string; subscription_operation?: boolean };
const path = "/personal/admin/engine/setup";

/** Claude connection form. The key exists in the password input and one request,
 * never in component state, storage, model context or response projections.
 * Replacing a saved connection names its current revision (compare-and-set). */
export function PersonalEngineSetup({ account, onSaved, expired, unavailable, subscriptionRevision }: {
  account: string; onSaved: () => void; expired: () => void; unavailable: () => void;
  subscriptionRevision?: string | null;
}) {
  const scope = useScope(account, "installation-engine-setup");
  const [setup, setSetup] = useState<Setup | null>(null);
  const [replacing, setReplacing] = useState(false);
  const [mode, setMode] = useState<"api" | "subscription">("api");
  const [claudeActive, setClaudeActive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // The server finished an explicit disconnect; the parent's engine status may lag.
  const [disconnected, setDisconnected] = useState(false);
  const [notice, setNotice] = useState("");
  const keyInput = useRef<HTMLInputElement>(null);
  const modelInput = useRef<HTMLInputElement>(null);
  const effortInput = useRef<HTMLSelectElement>(null);
  const pending = useRef(false);
  const request = useRef(crypto.randomUUID());
  const subscription = subscriptionRevision !== undefined && !disconnected;
  useEffect(() => { if (subscriptionRevision === undefined) setDisconnected(false); }, [subscriptionRevision]);
  const subscriptionMismatch = subscription && (!setup?.saved || !subscriptionRevision || setup.connection_revision !== subscriptionRevision);
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) expired();
    else if (e instanceof ApiFailure && e.status === 403) unavailable();
    else if (e instanceof ApiFailure && e.status === 409) {
      request.current = crypto.randomUUID();
      setReplacing(false);
      setError("연결이 이미 다른 값으로 저장되어 있거나 먼저 변경되었습니다. 현재 상태를 확인한 뒤 다시 시도해 주세요.");
    } else setError(e instanceof ApiFailure && e.code === "model_not_in_provider_catalog"
      ? "이 계정에서 선택한 모델을 찾지 못했습니다. Anthropic에서 제공하는 모델 ID를 확인해 주세요."
      : "연결을 확인하지 못했습니다. 저장 상태를 확인한 뒤 다시 시도해 주세요.");
  }
  useEffect(() => {
    const cancel = new AbortController();
    void scope.request<Setup>(path, "GET", undefined, cancel.signal).then(next => {
      setSetup(next);
      // An unfinished own-subscription setup resumes after a reload.
      if (next.subscription_operation) setMode("subscription");
    }).catch(e => {
      if (e instanceof ApiFailure && [404, 405, 501].includes(e.status)) return;
      fail(e);
    });
    return () => { cancel.abort(); if (keyInput.current) keyInput.current.value = ""; };
  }, [scope]);
  async function refresh() {
    try { setSetup(await scope.request<Setup>(path, "GET")); } catch (e) { fail(e); }
  }
  function afterDisconnect() {
    setDisconnected(true); setReplacing(false); setMode("api");
    setNotice("구독 연결이 해제되었습니다. 이제 API 키 또는 내 Claude 구독으로 다시 연결할 수 있습니다.");
    setSetup(current => current && { ...current, saved: false, connection_revision: undefined });
    onSaved(); void refresh();
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending.current || !setup?.supported || subscriptionMismatch || (setup.saved && !replacing)) return;
    const replace = replacing ? setup.connection_revision : undefined;
    if (replacing && !replace) return;
    const key = keyInput.current?.value ?? "";
    const model = modelInput.current?.value.trim() ?? "";
    const effort = effortInput.current?.value || null;
    if (!key || !model) return;
    pending.current = true; setBusy(true); setError("");
    if (keyInput.current) keyInput.current.value = "";
    const sent = request.current;
    const done = (s: Setup) => s.saved && s.connection_revision === sent;
    try {
      const next = await scope.request<Setup>(path, "POST", {
        request_id: sent, api_key: key, model, effort, ...(replace ? { replace_revision: replace } : {}),
      }, undefined, { timeoutMs: 30_000 });
      setSetup(next);
      if (done(next)) {
        if (replace) { request.current = crypto.randomUUID(); setReplacing(false); }
        onSaved();
      } else setError("이번 요청의 저장을 확인하지 못했습니다. 현재 등록된 연결 상태를 확인해 주세요.");
    } catch (e) {
      if (isAborted(e)) return;
      if (e instanceof ApiFailure && [401, 403, 409].includes(e.status)) { fail(e); if (e.status === 409) await refresh(); return; }
      // A lost reply may follow a durable save. Inspect before asking for a
      // fresh credential; do not create a new connection or submit again.
      try {
        const saved = await scope.request<Setup>(path, "GET");
        setSetup(saved);
        if (done(saved)) {
          if (replace) { request.current = crypto.randomUUID(); setReplacing(false); }
          onSaved(); return;
        }
      } catch (checkError) { fail(checkError); return; }
      fail(e);
    } finally { pending.current = false; setBusy(false); }
  }
  if (!setup?.supported) return error ? <p role="alert">{error}</p> : null;
  if (subscriptionMismatch && !setup.subscription_operation) return <p role="alert" className="preview-error">현재 연결 상태를 확인하지 못했습니다. 위의 새로고침을 누른 뒤 다시 시도해 주세요.</p>;
  if (subscription && !setup.subscription_operation) return <section className="member-page-list management-engine-overview">
    <h2>Claude 연결</h2>
    <PersonalClaudeSetup account={account} disconnectRevision={subscriptionRevision ?? undefined}
      expired={expired} unavailable={unavailable} onSaved={() => { onSaved(); void refresh(); }}
      onDisconnected={afterDisconnect} onRefresh={() => { onSaved(); void refresh(); }}/>
  </section>;
  const form = !setup.saved || replacing;
  const claude = mode === "subscription";
  return <section className="member-page-list management-engine-overview" aria-labelledby="engine-first-connection">
    <h2 id="engine-first-connection">Claude 연결</h2>
    {error && <p role="alert" className="preview-error">{error}</p>}
    {notice && form && <p role="status">{notice}</p>}
    {!form && <>
      <p role="status">설치에 연결 설정이 등록되어 있습니다. 위 엔진 상태를 새로고침하고 대화에서 응답을 확인해 주세요.</p>
      {setup.connection_revision && <button className="member-page-action" type="button" onClick={() => { setError(""); setReplacing(true); }}>키·모델 교체</button>}
    </>}
    {(form || claude) && <fieldset className="management-row" disabled={busy || claudeActive}>
      <legend>연결 방식</legend>
      <label className="management-row management-toggle management-choice"><input type="radio" name="engine-connection-mode" checked={!claude} onChange={() => setMode("api")}/> API 키 사용</label>{" "}
      <label className="management-row management-toggle management-choice"><input type="radio" name="engine-connection-mode" checked={claude} onChange={() => setMode("subscription")}/> 내 Claude 구독 사용</label>
    </fieldset>}
    {claude && <PersonalClaudeSetup account={account} replaceRevision={setup.saved ? setup.connection_revision ?? null : undefined}
      expired={expired} unavailable={unavailable} onActive={setClaudeActive} onDisconnected={afterDisconnect}
      onSaved={() => { setReplacing(false); setNotice(""); onSaved(); void refresh(); }}/>}
    {form && !claude && <form className="management-list" onSubmit={submit}>
      <p className="management-row management-note">{replacing
        ? "새 키와 모델을 확인한 뒤 현재 연결을 교체합니다. 확인이 끝나기 전에는 기존 연결이 유지됩니다."
        : "관리자의 Anthropic API 연결을 이 설치의 회원들이 사용합니다. 키와 모델 목록을 확인한 뒤 저장합니다."}</p>
      <label className="management-row management-field"><span>Anthropic API 키</span><input ref={keyInput} type="password" autoComplete="off" required minLength={8} maxLength={8192} disabled={busy}/></label>
      <label className="management-row management-field"><span>모델 ID</span><input ref={modelInput} defaultValue="claude-sonnet-5-5" required maxLength={200} disabled={busy}/></label>
      <label className="management-row management-field"><span>추론 강도</span><select aria-label="추론 강도" ref={effortInput} defaultValue="medium" disabled={busy}>
        <option value="">모델 기본값</option><option value="low">낮음</option><option value="medium">보통</option><option value="high">높음</option>
      </select></label>
      <div className="management-row compatible-actions">
        <button className="member-page-action primary" type="submit" disabled={busy}>{busy ? "연결 확인 중…" : "연결 확인 후 저장"}</button>
        {replacing && <button className="member-page-action" type="button" disabled={busy} onClick={() => setReplacing(false)}>취소</button>}
      </div>
    </form>}
  </section>;
}
