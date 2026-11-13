import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { GoogleConnectionCheck, GoogleSetupStatus, GoogleSetupValidation, MailConnection } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, errorText, isAborted } from "./api";
import "./google-setup.css";

const maximumBytes = 16384;
function setupError(error: unknown) {
  if (error instanceof ApiFailure) {
    const messages: Record<string, string> = {
      invalid_google_client_file: "Google에서 내려받은 웹 애플리케이션용 JSON 파일을 선택하세요.",
      google_redirect_uri_mismatch: "콜백 주소가 다릅니다. 아래 주소를 Google Cloud에 정확히 등록한 뒤 JSON을 다시 내려받으세요.",
      google_managed_by_operator: "이 설치는 운영자 파일로 관리합니다. 운영자에게 파일 변경을 요청하세요.",
      google_connections_in_use: "현재 앱을 사용하는 개인 연결이 있어 다른 앱으로 바꿀 수 없습니다. 기존 앱 설정을 유지하거나 연결을 정리한 뒤 다시 시도하세요.",
      google_authorization_in_progress: "Google 로그인이 진행 중입니다. 로그인 완료 또는 만료 후 상태를 새로고침하고 다시 시도하세요.",
      google_setup_required: "먼저 설치의 Google 앱 설정을 저장하세요.",
      gmail_setup_required: "먼저 설치의 Google 앱 설정을 저장하세요.",
      google_origin_changed: "서버 접속 주소가 바뀌었습니다. 현재 콜백으로 Google 앱 설정을 다시 저장하세요.",
      google_connection_check_failed: "Google 연결을 확인하지 못했습니다. 잠시 후 다시 확인하거나 계정을 다시 연결하세요.",
      google_profile_mismatch: "연결한 계정과 Google 프로필이 다릅니다. 본인 계정을 다시 연결하세요.",
      google_oauth_exchange_failed: "Google 연결을 완료하지 못했습니다. Google 로그인을 처음부터 다시 진행하세요.",
      google_consent_incomplete: "Gmail 읽기·발송 권한과 지속 연결에 필요한 동의가 누락됐습니다. Google 연결을 처음부터 다시 진행하세요.",
      google_profile_unavailable: "Google 프로필을 확인하지 못했습니다. 잠시 후 계정을 다시 연결하세요.",
    };
    if (messages[error.code]) return messages[error.code];
    if (error.status === 409) return "설정이 변경되었습니다. 최신 상태를 확인하고 JSON 파일을 다시 선택하세요.";
  }
  return errorText(error);
}

// An account switch remounts all state, including browser-owned file inputs.
export function GoogleSetup(props: { accountId: string; onExpired: () => void }) {
  return <GoogleSetupAccount key={props.accountId} {...props} />;
}
function GoogleSetupAccount({ accountId, onExpired }: { accountId: string; onExpired: () => void }) {
  const scope = useScope(accountId);
  const [status, setStatus] = useState<GoogleSetupStatus | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [hasFile, setHasFile] = useState(false);
  const [validation, setValidation] = useState<GoogleSetupValidation | null>(null);
  const [selected, setSelected] = useState("");
  const [checked, setChecked] = useState("");
  const [copied, setCopied] = useState(false);
  const imported = useRef("");
  const fileInput = useRef<HTMLInputElement>(null);
  const lifetime = useRef(0);
  const fileVersion = useRef(0);
  const callback = useRef<{ handle: string | null; denied: boolean } | null>(null);
  const callbackStarted = useRef(false);
  const reloadVersion = useRef(0);
  useLayoutEffect(() => {
    lifetime.current += 1;
    return () => {
      lifetime.current += 1;
      fileVersion.current += 1;
      imported.current = "";
      if (fileInput.current) fileInput.current.value = "";
    };
  }, []);
  function clearImport(resetInput = true) {
    fileVersion.current += 1;
    imported.current = "";
    setHasFile(false);
    setValidation(null);
    if (resetInput && fileInput.current) fileInput.current.value = "";
  }
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(setupError(e));
  }
  async function reload() {
    const version = ++reloadVersion.current;
    const next = await scope.request<GoogleSetupStatus>("/admin/google");
    if (version !== reloadVersion.current) return;
    setStatus(next);
    setCopied(false);
    setSelected((old) => next.personal_connections.some(c => c.id === old && c.status === "active")
      ? old : next.personal_connections.find(c => c.status === "active")?.id ?? "");
  }
  useEffect(() => { void reload().catch(fail); }, [scope]);
  useEffect(() => {
    if (!callback.current) {
      const params = new URLSearchParams(window.location.search);
      const handle = params.get("oauth");
      const denied = params.has("oauth_error");
      callback.current = { handle, denied };
      if (handle || denied) {
        params.delete("oauth");
        params.delete("oauth_error");
        const query = params.toString();
        window.history.replaceState(null, "", window.location.pathname + (query ? `?${query}` : "") + window.location.hash);
      }
    }
    let active = true;
    // Defer transport until after StrictMode's effect replay. The URL is already
    // clean; a consumed handle is never automatically retried after failure.
    void Promise.resolve().then(async () => {
      if (!active || callbackStarted.current) return;
      const result = callback.current!;
      if (result.denied) setError("Google 연결을 허락하지 않았습니다. 원할 때 다시 연결하세요.");
      if (!result.handle) return;
      callbackStarted.current = true;
      const handle = result.handle;
      result.handle = null;
      setBusy(true);
      setChecked("");
      try {
        const connection = await scope.request<MailConnection>("/mail/oauth/complete", "POST", { handle });
        if (!active) return;
        await reload();
        if (active) {
          setSelected(connection.id);
          setChecked(connection.email);
          setNotice("Google 로그인과 본인 프로필 확인을 마쳤습니다.");
        }
      } catch (e) { if (active) fail(e); }
      finally { if (active) setBusy(false); }
    });
    return () => { active = false; };
  }, [scope]);
  async function chooseFile(file?: File) {
    clearImport(false);
    setError(""); setNotice("");
    if (!file) return;
    if (file.size > maximumBytes) { setError("JSON 파일은 16KB 이하여야 합니다."); return; }
    const version = fileVersion.current;
    const owner = lifetime.current;
    try {
      const raw = await file.text();
      if (owner !== lifetime.current || version !== fileVersion.current) return;
      if (new TextEncoder().encode(raw).length > maximumBytes) throw new Error();
      const data = JSON.parse(raw);
      if (!data?.web || typeof data.web.client_id !== "string" || typeof data.web.client_secret !== "string" || !Array.isArray(data.web.redirect_uris)) throw new Error();
      imported.current = raw;
      setHasFile(true);
    } catch {
      if (owner === lifetime.current && version === fileVersion.current)
        setError("Google에서 내려받은 웹 애플리케이션용 JSON 파일을 선택하세요. 파일 크기는 16KB 이하여야 합니다.");
    }
  }
  async function validate() {
    if (!imported.current || busy) return;
    const version = fileVersion.current;
    setBusy(true); setError(""); setNotice("");
    try {
      const result = await scope.request<GoogleSetupValidation>("/admin/google/validate", "POST", { client_json: imported.current });
      if (version !== fileVersion.current) return;
      if (!result.local_valid || result.redirect_uri !== status?.redirect_uri)
        throw new ApiFailure(400, "google_redirect_uri_mismatch");
      setValidation(result);
    } catch (e) { if (version === fileVersion.current) { setValidation(null); fail(e); } }
    finally { setBusy(false); }
  }
  async function save() {
    if (!status || !validation || !imported.current || busy) return;
    const client_json = imported.current;
    const expected_revision = status.revision;
    clearImport(); setBusy(true); setError(""); setNotice(""); setChecked("");
    try {
      const next = await scope.request<GoogleSetupStatus>("/admin/google", "PUT", { expected_revision, client_json });
      setStatus(next);
      setNotice("설치의 Google 앱 설정을 저장했습니다. 다음으로 본인 계정을 연결해 확인하세요.");
    } catch (e) {
      fail(e);
      if (e instanceof ApiFailure && e.status === 409) await reload().catch(fail);
    } finally { setBusy(false); }
  }
  async function connect() {
    setBusy(true); setError(""); setNotice(""); setChecked("");
    try {
      const result = await scope.request<{ authorization_url: string }>("/admin/google/oauth/start", "POST", {});
      const url = new URL(result.authorization_url);
      if (url.origin !== "https://accounts.google.com") throw new Error();
      window.location.assign(url.href);
    } catch (e) { fail(e); setBusy(false); }
  }
  async function check() {
    if (!selected || busy) return;
    setBusy(true); setError(""); setNotice(""); setChecked("");
    try {
      const result = await scope.request<GoogleConnectionCheck>("/admin/google/check", "POST", { connection_id: selected });
      setChecked(result.email);
    } catch (e) { fail(e); }
    finally { setBusy(false); }
  }
  const operator = status?.mode === "operator_file";
  const ready = !!status?.configured && status.redirect_matches;
  return <main className="google-setup-scroll">
    <div className="google-setup-intro"><p className="eyebrow">GOOGLE · 설치 설정</p><h2>한 번 준비하고, 각자 연결하세요.</h2><p>설치자가 Google 앱 하나를 준비합니다. 가족과 친구는 각자의 Google 계정으로 로그인해 개인 Gmail을 연결합니다.</p></div>
    {error && <p className="notice" role="alert">{error}</p>}
    {notice && <p className="google-setup-success" role="status">{notice}</p>}
    <button className="text-button" disabled={busy} onClick={() => { setError(""); setChecked(""); clearImport(); void reload().catch(fail); }}>상태 새로고침</button>
    {!status ? <p role="status">Google 설정 확인 중…</p> : <>
      {status.mode !== "unconfigured" && !status.redirect_matches && <p role="alert" className="notice">서버 접속 주소와 저장된 콜백이 다릅니다. 현재 콜백을 Google Cloud에 등록하고 설정을 갱신하세요.{operator && " 운영자에게 설정 파일 갱신을 요청하세요."}</p>}
      <section className="admin-card google-setup-step" aria-labelledby="google-step-1"><span className="pill">1 · GOOGLE CLOUD</span><h3 id="google-step-1">Google 앱 준비</h3>
        <ol><li>Google Cloud 프로젝트를 만들거나 선택하고 <a href="https://console.cloud.google.com/apis/library/gmail.googleapis.com" target="_blank" rel="noreferrer">Gmail API를 사용 설정</a>하세요.</li><li>OAuth 동의 화면을 설정하세요. 가족·친구와 쓰면 External, 같은 Workspace 조직 내부 전용이면 가능한 경우 Internal을 선택하세요.</li><li>External의 Testing 상태에서는 연결할 사람을 테스트 사용자로 추가하세요. 이 Gmail 권한의 테스트용 갱신 토큰은 7일 후 만료될 수 있습니다.</li><li><a href="https://console.cloud.google.com/auth/clients" target="_blank" rel="noreferrer">OAuth 클라이언트</a>를 <strong>웹 애플리케이션</strong>으로 만들고, 승인된 리디렉션 URI에 아래 주소를 그대로 등록하세요.</li></ol>
        <label>이 설치의 콜백 주소<input aria-label="이 설치의 콜백 주소" readOnly value={status.redirect_uri} onFocus={e => e.currentTarget.select()} /></label>
        <button disabled={busy} onClick={async () => { const owner = lifetime.current; try { await navigator.clipboard.writeText(status.redirect_uri); if (owner === lifetime.current) setCopied(true); } catch { if (owner === lifetime.current) setError("복사하지 못했습니다. 콜백 주소를 선택해 직접 복사하세요."); } }}>{copied ? "콜백 주소 복사됨" : "콜백 주소 복사"}</button>
        <details><summary>Google 공식 도움말</summary><p><a href="https://developers.google.com/workspace/guides/configure-oauth-consent" target="_blank" rel="noreferrer">동의 화면과 테스트 사용자 설정</a> · <a href="https://developers.google.com/identity/protocols/oauth2/web-server" target="_blank" rel="noreferrer">웹 서버 OAuth 설정</a> · <a href="https://developers.google.com/identity/protocols/oauth2#expiration" target="_blank" rel="noreferrer">토큰 만료 안내</a></p></details>
      </section>
      <section className="admin-card google-setup-step" aria-labelledby="google-step-2"><span className="pill">2 · 설치에 저장</span><h3 id="google-step-2">웹 클라이언트 JSON 가져오기</h3>
        {operator ? <p>운영자의 <code>--google-client-file</code> 파일로 관리 중입니다. 이 화면에서는 앱 설정을 바꿀 수 없습니다. 변경은 운영자에게 요청하세요.</p> : <><p>Google Cloud에서 내려받은 JSON 파일을 선택하세요. 비밀 키는 화면에 표시하거나 브라우저 저장소에 남기지 않습니다.</p><label>Google 웹 클라이언트 JSON<input ref={fileInput} type="file" accept=".json,application/json" disabled={busy} onChange={e => { const file = e.currentTarget.files?.[0]; void chooseFile(file); }} /></label><div className="google-setup-actions"><button disabled={busy || !hasFile} onClick={() => void validate()}>파일과 콜백 확인</button><button className="primary" disabled={busy || !validation} onClick={() => void save()}>이 설치에 저장</button></div></>}
        {validation && <div className="google-local-result" role="status"><strong>파일 형식과 콜백 일치 확인</strong><dl><dt>클라이언트 ID</dt><dd>{validation.client_id}</dd><dt>콜백 주소</dt><dd>{validation.redirect_uri}</dd></dl></div>}
        <p className="fine">이 확인은 Google에 접속하지 않습니다. Gmail API 사용 설정, 동의 화면, 테스트 사용자 등록 여부는 확인하지 않습니다.</p>
        {status.client_id && <p className="google-client-id">저장된 클라이언트 ID: <span>{status.client_id}</span></p>}
        <p>설치 설정: {status.mode === "unconfigured" ? "아직 저장하지 않음" : !status.redirect_matches ? "콜백 갱신 필요" : ready ? "저장됨 · 현재 콜백과 일치" : "설정 확인 필요"}</p>
      </section>
      <section className="admin-card google-setup-step" aria-labelledby="google-step-3"><span className="pill">3 · 내 계정으로 확인</span><h3 id="google-step-3">본인 Google 계정 연결</h3><p>각 회원도 자신의 설정에서 직접 로그인합니다. 여기서는 본인 연결의 Google 프로필만 확인하며, 메일을 보내거나 자동으로 수집하지 않습니다.</p>
        <button className="primary" disabled={busy || !ready} onClick={() => void connect()}>내 Google 계정 연결</button>
        {status.personal_connections.some(c => c.status === "active") && <><label>확인할 내 계정<select value={selected} disabled={busy} onChange={e => { setSelected(e.target.value); setChecked(""); }}>{status.personal_connections.filter(c => c.status === "active").map(c => <option key={c.id} value={c.id}>{c.email}</option>)}</select></label><button disabled={busy || !ready || !selected} onClick={() => void check()}>내 연결 프로필 확인</button></>}
        {checked && <p className="google-setup-success" role="status">Google 프로필 확인 성공 · {checked}</p>}
        {!ready && <p className="fine">2단계 설정과 현재 콜백을 먼저 확인하세요.</p>}
      </section>
    </>}
  </main>;
}
