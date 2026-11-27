import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { WorkspaceIcon } from "./WorkspaceNavigation";
import "./personal-management.css";
import "./personal-google-setup.css";

export const installationGoogleSetupPath = "/personal/admin/google-setup";
const maximumFileBytes = 16 * 1024;
type Status = { configured: boolean; redirect_uri: string };
type Props = { account: string; expired: () => void; unavailable: () => void; onConnect: () => void; onStart: () => void; returnLabel?: string; startLabel?: string; onBack?: () => void };

export function PersonalGoogleSetup(props: Props) {
  return <Setup key={props.account} {...props} />;
}

function Setup({ account, expired, unavailable, onConnect, onStart, returnLabel, startLabel, onBack }: Props) {
  const scope = useScope(account, "installation-google-setup");
  const storageKey = "moosuri.personal.google-setup.v1." + account;
  const [status, setStatus] = useState<Status | null>(null);
  const [checking, setChecking] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const [busy, setBusy] = useState(false);
  const [fileSelected, setFileSelected] = useState(false);
  const [error, setError] = useState("");
  const [copyMessage, setCopyMessage] = useState("");
  const [saved, setSaved] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const callbackInput = useRef<HTMLInputElement>(null);
  const titleHeading = useRef<HTMLHeadingElement>(null);
  const resultHeading = useRef<HTMLHeadingElement>(null);
  const upload = useRef<AbortController | null>(null);

  useLayoutEffect(() => scope.onClose(() => {
    upload.current?.abort();
    if (input.current) input.current.value = "";
  }), [scope]);

  useLayoutEffect(() => {
    titleHeading.current?.focus();
  }, []);

  useLayoutEffect(() => {
    if (saved) resultHeading.current?.focus();
  }, [saved]);

  function showError(failure: unknown, fallback: string) {
    if (isAborted(failure)) return;
    if (failure instanceof ApiFailure) {
      if (failure.status === 401) return expired();
      if (failure.status === 403) return unavailable();
    }
    setError(fallback);
  }

  useEffect(() => {
    const controller = new AbortController();
    setChecking(true);
    void scope.request<Status>(installationGoogleSetupPath, "GET", undefined, controller.signal, { timeoutMs: 15000 })
      .then(result => { if (!controller.signal.aborted) setStatus(result); })
      .catch(failure => {
        if (controller.signal.aborted || isAborted(failure)) return;
        setStatus(null);
        showError(failure, "Google 설치 설정을 확인하지 못했습니다. 다시 확인해 주세요.");
      })
      .finally(() => {
        if (!controller.signal.aborted) setChecking(false);
      });
    return () => controller.abort();
  }, [scope, refresh]);

  useEffect(() => {
    try { localStorage.removeItem(storageKey); } catch { /* Storage unavailable. */ }
  }, [storageKey]);

  async function copyCallback() {
    if (!status) return;
    try {
      await navigator.clipboard.writeText(status.redirect_uri);
      setCopyMessage("콜백 주소를 복사했습니다.");
    } catch {
      callbackInput.current?.focus();
      callbackInput.current?.select();
      setCopyMessage("선택된 콜백 주소를 직접 복사해 주세요.");
    }
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const field = input.current;
    const file = field?.files?.[0];
    if (!field || !file || !status || checking || upload.current) return;
    field.value = "";
    setFileSelected(false);
    setError("");
    if (file.size > maximumFileBytes) {
      setError("Google JSON 파일은 16KB 이하여야 합니다.");
      return;
    }
    const controller = new AbortController();
    upload.current = controller;
    setBusy(true);
    try {
      const web_client_json = await file.text();
      if (controller.signal.aborted) return;
      let client;
      try { client = JSON.parse(web_client_json); } catch { /* handled below */ }
      if (client?.installed) {
        setError("데스크톱용 OAuth 파일입니다. 같은 프로젝트에서 웹 애플리케이션을 선택하고 아래 콜백 주소를 등록해 주세요.");
        return;
      }
      if (!client?.web || typeof client.web.client_id !== "string" || !client.web.client_id ||
          typeof client.web.client_secret !== "string" || !client.web.client_secret) {
        setError("Google에서 다운로드한 웹 애플리케이션 클라이언트의 JSON 파일을 선택해 주세요.");
        return;
      }
      if (!Array.isArray(client.web.redirect_uris) || !client.web.redirect_uris.includes(status.redirect_uri)) {
        setError("콜백 주소가 다릅니다. 아래 주소를 웹 클라이언트의 승인된 리디렉션 URI에 등록하고 JSON을 다시 다운로드해 주세요.");
        return;
      }
      const result = await scope.request<Status>(installationGoogleSetupPath, "POST", { web_client_json }, controller.signal, { timeoutMs: 20000 });
      setStatus(result);
      setSaved(result.configured);
    } catch (failure) {
      if (controller.signal.aborted || isAborted(failure)) return;
      let message = "저장 결과를 확인하지 못했습니다. 설정 상태를 확인한 뒤 파일을 다시 선택해 주세요.";
      if (failure instanceof ApiFailure) {
        if (failure.code === "google_redirect_uri_mismatch") {
          message = "콜백 주소가 다릅니다. 아래 주소를 웹 클라이언트의 승인된 리디렉션 URI에 등록하고 JSON을 다시 다운로드해 주세요.";
        } else if (failure.code === "invalid_google_client_file" || failure.code === "bad_request") {
          message = "Google에서 다운로드한 웹 애플리케이션 클라이언트의 JSON 파일을 선택해 주세요.";
        } else if (failure.code === "google_setup_already_configured") {
          message = "이 설치에 이미 Google 앱이 등록되어 있습니다.";
          setRefresh(value => value + 1);
        } else if (failure.code === "google_setup_storage_unavailable") {
          message = "서버에서 Google 설정을 저장하지 못했습니다. 운영자에게 문의해 주세요.";
        }
      }
      showError(failure, message);
    } finally {
      if (!controller.signal.aborted) {
        upload.current = null;
        setBusy(false);
      }
    }
  }

  const callbackField = status && (
    <label className="google-setup-field">이 설치의 콜백 주소
      <input ref={callbackInput} readOnly value={status.redirect_uri} onFocus={event => event.currentTarget.select()} />
    </label>
  );
  const copyButton = <button className="member-page-action" onClick={() => void copyCallback()}>콜백 주소 복사</button>;
  const copyNotice = copyMessage && <p className="management-note" role="status">{copyMessage}</p>;
  return (
    <section className="member-page member-library member-management member-connections personal-google-setup" aria-labelledby="google-setup-title">
      <header className="member-page-heading google-setup-heading">
        {onBack && <button className="member-library-icon-button" aria-label="뒤로" onClick={onBack}><WorkspaceIcon name="back" /></button>}
        <h1 id="google-setup-title" tabIndex={-1} ref={titleHeading}>Google Workspace 설정</h1>
      </header>
      {error && !status && <p role="alert" className="google-setup-error">{error}</p>}
      {checking && <p role="status" className="management-note">Google 설치 설정 확인 중…</p>}
      {!checking && !status && <button className="member-page-action" onClick={() => {
        setError("");
        setRefresh(value => value + 1);
      }}>설정 상태 다시 확인</button>}
      {!checking && status?.configured && (
        <div className="member-page-list google-setup-panel">
          <h2 className="google-setup-result" tabIndex={-1} ref={resultHeading}>
            {saved ? "Google 앱 설정을 저장했습니다." : "Google 앱 설정이 등록되어 있습니다."}
          </h2>
          <p>이제 각 회원은 커넥터에서 원하는 서비스를 선택하고 자신의 Google 계정으로 동의할 수 있습니다.</p>
          {callbackField}
          <div className="google-setup-buttons">
            <button className="member-page-action management-primary" onClick={onConnect}>{returnLabel ?? "커넥터 열기"}</button>
            {copyButton}
          </div>
          {copyNotice}
        </div>
      )}
      {!checking && status && !status.configured && (
        <section className="member-page-list google-setup-panel">
          <h2>설치용 Google 앱 준비</h2>
          <p>먼저 사용할 서비스를 고르고 Google 계정으로 연결하세요. 볼트 계정 확인과 로그인 뒤 설치용 Google 앱을 준비합니다. 이후 각 회원이 자신의 계정으로 따로 동의합니다. Google이 추가 확인을 요청하면 화면에서 안내합니다.</p>
          <div className="google-setup-buttons">
            <button type="button" className="member-page-action management-primary" onClick={onStart}>{startLabel ?? "서비스 선택하고 계속"}</button>
          </div>
          <details className="google-setup-advanced">
          <summary>고급: 기존 Google 웹 클라이언트 JSON 직접 등록</summary>
          <p>이미 만든 Google 웹 클라이언트가 있다면 JSON 파일을 등록할 수 있습니다. 승인된 리디렉션 URI에 아래 주소가 있어야 합니다.</p>
          {callbackField}
          {copyButton}
          {copyNotice}
                <form className="google-setup-form" onSubmit={event => void save(event)}>
                  <label className="google-setup-field">Google 웹 클라이언트 JSON
                    <input ref={input} type="file" accept=".json,application/json" disabled={busy}
                      onChange={event => {
                        setFileSelected(Boolean(event.currentTarget.files?.length));
                        setError("");
                      }} />
                  </label>
                  {error && <p role="alert" className="google-setup-error">{error}</p>}
                  <div className="google-setup-buttons">
                    <button type="submit" className="member-page-action management-primary" disabled={busy || !fileSelected}>이 설치에 저장</button>
                    {error && <button type="button" className="member-page-action" disabled={busy} onClick={() => {
                      setError("");
                      setFileSelected(false);
                      setRefresh(value => value + 1);
                    }}>설정 상태 다시 확인</button>}
                  </div>
                  {busy && <p className="management-note" role="status">Google 앱 설정을 저장하고 있습니다…</p>}
          </form>
          </details>
        </section>
      )}
    </section>
  );
}
