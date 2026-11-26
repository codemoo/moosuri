import { useEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

type Setup = { supported: boolean; saved: boolean; connection_revision?: string };
const path = "/personal/admin/engine/setup";

/** Claude connection form. The key exists in the password input and one request,
 * never in component state, storage, model context or response projections.
 * Replacing a saved connection names its current revision (compare-and-set). */
export function PersonalEngineSetup({ account, onSaved, expired, unavailable }: {
  account: string; onSaved: () => void; expired: () => void; unavailable: () => void;
}) {
  const scope = useScope(account, "installation-engine-setup");
  const [setup, setSetup] = useState<Setup | null>(null);
  const [replacing, setReplacing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const keyInput = useRef<HTMLInputElement>(null);
  const modelInput = useRef<HTMLInputElement>(null);
  const effortInput = useRef<HTMLSelectElement>(null);
  const pending = useRef(false);
  const request = useRef(crypto.randomUUID());
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
    void scope.request<Setup>(path, "GET", undefined, cancel.signal).then(setSetup).catch(e => {
      if (e instanceof ApiFailure && [404, 405, 501].includes(e.status)) return;
      fail(e);
    });
    return () => { cancel.abort(); if (keyInput.current) keyInput.current.value = ""; };
  }, [scope]);
  async function refresh() {
    try { setSetup(await scope.request<Setup>(path, "GET")); } catch (e) { fail(e); }
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending.current || !setup?.supported || (setup.saved && !replacing)) return;
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
  const form = !setup.saved || replacing;
  return <section className="member-page-list management-engine-overview" aria-labelledby="engine-first-connection">
    <h2 id="engine-first-connection">Claude 연결</h2>
    {error && <p role="alert" className="preview-error">{error}</p>}
    {!form && <>
      <p role="status">설치에 연결 설정이 등록되어 있습니다. 위 엔진 상태를 새로고침하고 대화에서 응답을 확인해 주세요.</p>
      {setup.connection_revision && <button type="button" onClick={() => { setError(""); setReplacing(true); }}>키·모델 교체</button>}
    </>}
    {form && <form className="management-list" onSubmit={submit}>
      <p className="management-note">{replacing
        ? "새 키와 모델을 확인한 뒤 현재 연결을 교체합니다. 확인이 끝나기 전에는 기존 연결이 유지됩니다."
        : "관리자의 Anthropic API 연결을 이 설치의 회원들이 사용합니다. 키와 모델 목록을 확인한 뒤 저장합니다."}</p>
      <label>Anthropic API 키<input ref={keyInput} type="password" autoComplete="off" required minLength={8} maxLength={8192} disabled={busy}/></label>
      <label>모델 ID<input ref={modelInput} defaultValue="claude-sonnet-5-5" required maxLength={200} disabled={busy}/></label>
      <label>추론 강도<select ref={effortInput} defaultValue="medium" disabled={busy}>
        <option value="">모델 기본값</option><option value="low">낮음</option><option value="medium">보통</option><option value="high">높음</option>
      </select></label>
      <button type="submit" disabled={busy}>{busy ? "연결 확인 중…" : "연결 확인 후 저장"}</button>
      {replacing && <button type="button" disabled={busy} onClick={() => setReplacing(false)}>취소</button>}
    </form>}
  </section>;
}
