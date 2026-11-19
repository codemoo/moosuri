import { useEffect, useLayoutEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import "./personal-management.css";
import "./personal-google-setup.css";

const path = "/personal/admin/google-setup";
const consoleOrigin = "https://console.cloud.google.com";
const maximumFileBytes = 16 * 1024;
const projectIdPattern = /^[a-z][a-z0-9-]{4,28}[a-z0-9]$/;
const services = [
  { name: "Gmail", api: "gmail.googleapis.com", scope: "gmail.readonly" },
  { name: "Drive", api: "drive.googleapis.com", scope: "drive.metadata.readonly" },
  { name: "Docs", api: "docs.googleapis.com", scope: "documents.readonly" },
  { name: "Slides", api: "slides.googleapis.com", scope: "presentations.readonly" },
  { name: "Sheets", api: "sheets.googleapis.com", scope: "spreadsheets.readonly" },
];
const steps = ["Cloud 프로젝트", "API 사용 설정", "OAuth 동의 화면", "웹 클라이언트 등록"] as const;

type Step = 0 | 1 | 2 | 3;
type Draft = { project: string; step: Step };
type Status = { configured: boolean; redirect_uri: string };
type Props = { account: string; expired: () => void; unavailable: () => void; onConnect: () => void };

function loadDraft(key: string): Draft {
  try {
    const draft: Draft | null = JSON.parse(localStorage.getItem(key) ?? "null");
    if (draft && typeof draft.project === "string" && projectIdPattern.test(draft.project) &&
        [0, 1, 2, 3].includes(draft.step)) return draft;
    if (draft?.step === 3 && draft.project === "") return draft;
  } catch { /* Resume is optional when browser storage is unavailable. */ }
  return { project: "", step: 0 };
}

function consoleLink(pathname: string, project: string, search = "") {
  const query = [search, project && "project=" + encodeURIComponent(project)].filter(Boolean).join("&");
  return consoleOrigin + pathname + (query ? "?" + query : "");
}

function GoogleConsoleLink({ href, children, action = false }: { href: string; children: ReactNode; action?: boolean }) {
  return <a className={action ? "member-page-action" : undefined} href={href} target="_blank" rel="noopener noreferrer">{children}</a>;
}

export function PersonalGoogleSetup(props: Props) {
  return <Setup key={props.account} {...props} />;
}

function Setup({ account, expired, unavailable, onConnect }: Props) {
  const scope = useScope(account, "installation-google-setup");
  const storageKey = "moosuri.personal.google-setup.v1." + account;
  const [draft, setDraft] = useState(() => loadDraft(storageKey));
  const [projectInput, setProjectInput] = useState(draft.project);
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
  const stepHeading = useRef<HTMLHeadingElement>(null);
  const focusStep = useRef(false);
  const upload = useRef<AbortController | null>(null);
  const { project, step } = draft;

  useLayoutEffect(() => scope.onClose(() => {
    upload.current?.abort();
    if (input.current) input.current.value = "";
  }), [scope]);

  useLayoutEffect(() => {
    if (focusStep.current) {
      stepHeading.current?.focus();
      focusStep.current = false;
    }
  }, [step]);

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
    void scope.request<Status>(path, "GET", undefined, controller.signal, { timeoutMs: 15000 })
      .then(setStatus)
      .catch(failure => showError(failure, "Google 설치 설정을 확인하지 못했습니다. 다시 확인해 주세요."))
      .finally(() => {
        if (!controller.signal.aborted) setChecking(false);
      });
    return () => controller.abort();
  }, [scope, refresh]);

  useEffect(() => {
    if (!status?.configured) return;
    try { localStorage.removeItem(storageKey); } catch { /* Browser storage unavailable. */ }
  }, [status?.configured, storageKey]);

  function go(next: Draft) {
    focusStep.current = true;
    setDraft(next);
    setError("");
    setCopyMessage("");
    setFileSelected(false);
    try { localStorage.setItem(storageKey, JSON.stringify(next)); } catch { /* Browser storage unavailable. */ }
  }

  function chooseProject(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const next = projectInput.trim();
    if (!projectIdPattern.test(next)) {
      setError("프로젝트 ID는 영문 소문자로 시작하는 6~30자의 영문 소문자, 숫자, 하이픈입니다. 마지막에는 하이픈을 쓸 수 없습니다.");
      return;
    }
    go({ project: next, step: 1 });
  }

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
      const result = await scope.request<Status>(path, "POST", { web_client_json }, controller.signal, { timeoutMs: 20000 });
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

  const callbackDomain = status && new URL(status.redirect_uri).hostname;
  const callbackField = status && (
    <label className="google-setup-field">이 설치의 콜백 주소
      <input ref={callbackInput} readOnly value={status.redirect_uri} onFocus={event => event.currentTarget.select()} />
    </label>
  );
  const copyButton = <button className="member-page-action" onClick={() => void copyCallback()}>콜백 주소 복사</button>;
  const copyNotice = copyMessage && <p className="management-note" role="status">{copyMessage}</p>;
  return (
    <section className="member-page member-management personal-google-setup" aria-labelledby="google-setup-title" aria-busy={checking || busy}>
      <header className="member-page-heading">
        <h1 id="google-setup-title">Google 커넥터</h1>
      </header>
      {error && !status?.configured && <p role="alert" className="preview-error">{error}</p>}
      {checking && <p role="status" className="management-note">Google 설치 설정 확인 중…</p>}
      {!checking && !status && <button className="member-page-action" onClick={() => {
        setError("");
        setRefresh(value => value + 1);
      }}>설정 상태 다시 확인</button>}
      {!checking && status?.configured && (
        <div className="member-page-list google-setup-panel">
          <p role="status"><strong>{saved ? "Google 앱 설정을 저장했습니다." : "Google 앱 설정이 등록되어 있습니다."}</strong></p>
          <p>커넥터에서 Gmail, Drive, Docs, Slides, Sheets를 선택하고 본인 Google 계정으로 연결하세요. 각 회원은 자신의 계정으로 동의합니다.</p>
          {callbackField}
          <div className="google-setup-buttons">
            <button className="member-page-action management-primary" onClick={onConnect}>커넥터 열기</button>
            {copyButton}
          </div>
          {copyNotice}
        </div>
      )}
      {!checking && status && !status.configured && (
        <>
          <p className="management-note">설치에 사용할 Google 앱을 준비하세요. 새 브라우저 탭의 Google 공식 화면에서 로그인과 설정을 마치고 이 화면으로 돌아오세요.</p>
          <ol className="google-setup-progress" aria-label="Google 설정 단계">
            {steps.map((name, index) => <li key={name} aria-current={step === index ? "step" : undefined}>{name}</li>)}
          </ol>
          <section className="member-page-list google-setup-panel" aria-labelledby="google-setup-step">
            <h2 tabIndex={-1} ref={stepHeading} id="google-setup-step">{steps[step]}</h2>
            {step > 0 && project && <p className="management-note">프로젝트 <strong>{project}</strong></p>}
            {step === 0 && (
              <>
                <p>Google Cloud에서 기존 프로젝트를 선택하거나 새 프로젝트를 만드세요. 선택한 프로젝트의 ID를 아래에 입력하세요.</p>
                <div className="google-setup-buttons">
                  <GoogleConsoleLink href={consoleLink("/cloud-resource-manager", "")} action>기존 프로젝트 선택</GoogleConsoleLink>
                  <GoogleConsoleLink href={consoleLink("/projectcreate", "")} action>새 프로젝트 만들기</GoogleConsoleLink>
                </div>
                <p className="management-note">프로젝트를 만들 수 없으면 Google Cloud에서 권한과 프로젝트 할당량을 확인하거나 기존 프로젝트를 사용하세요.</p>
                <form className="google-setup-form" onSubmit={chooseProject}>
                  <label className="google-setup-field">프로젝트 ID
                    <input value={projectInput} onChange={event => setProjectInput(event.target.value)} autoComplete="off" spellCheck={false}
                      autoCapitalize="none" maxLength={30} placeholder="moosuri-example" required />
                  </label>
                  <div className="google-setup-buttons">
                    <button className="member-page-action management-primary" disabled={!projectInput.trim()}>프로젝트 사용</button>
                    <button type="button" className="member-page-action" onClick={() => go({ project: "", step: 3 })}>이미 받은 JSON 가져오기</button>
                  </div>
                </form>
              </>
            )}
            {step === 1 && (
              <>
                <p>Google Cloud에서 위 프로젝트가 선택되었는지 확인하고 Gmail, Drive, Docs, Slides, Sheets API를 사용 설정하세요.</p>
                <div className="google-setup-buttons">
                  <GoogleConsoleLink href={consoleLink("/apis/enableflow", project, "apiid=" + services.map(service => service.api).join(","))} action>
                    Google API 5개 사용 설정
                  </GoogleConsoleLink>
                </div>
                <details>
                  <summary>API별 사용 설정 링크</summary>
                  <ul>{services.map(service => <li key={service.api}>
                    <GoogleConsoleLink href={consoleLink("/apis/library/" + service.api, project)}>{service.name} API</GoogleConsoleLink>
                  </li>)}</ul>
                </details>
                <div className="google-setup-buttons">
                  <button className="member-page-action" onClick={() => go({ project, step: 0 })}>이전</button>
                  <button className="member-page-action management-primary" onClick={() => go({ project, step: 2 })}>사용 설정을 마쳤습니다</button>
                </div>
              </>
            )}
            {step === 2 && (
              <>
                <p>Google Auth Platform에서 앱 이름(예시 Moosuri), 지원 이메일, 개발자 연락처를 입력하세요. 승인된 도메인을 입력하는 경우 이 설치 주소(<strong>{callbackDomain}</strong>)의 등록 가능한 도메인을 사용하세요.</p>
                <p>같은 Google Workspace 조직 전용이면 Internal을 선택할 수 있습니다. 가족이나 친구와 쓰려면 External을 선택하세요. Testing으로 운영하면 연결할 계정을 테스트 사용자로 추가하세요. 테스트 상태의 갱신 토큰은 일반적으로 7일 후 만료됩니다.</p>
                <div className="google-setup-buttons">
                  <GoogleConsoleLink href={consoleLink("/auth/branding", project)} action>앱 정보 설정</GoogleConsoleLink>
                  <GoogleConsoleLink href={consoleLink("/auth/audience", project)} action>사용 대상 및 테스트 사용자</GoogleConsoleLink>
                  <GoogleConsoleLink href={consoleLink("/auth/scopes", project)} action>데이터 액세스 설정</GoogleConsoleLink>
                </div>
                <details>
                  <summary>서비스별 읽기 권한</summary>
                  <p>데이터 액세스에 계정 확인용 <code>openid</code>와 <code>https://www.googleapis.com/auth/userinfo.email</code>, 아래 읽기 권한을 등록하세요.</p>
                  <ul>{services.map(service => <li key={service.api}><code>https://www.googleapis.com/auth/{service.scope}</code></li>)}</ul>
                </details>
                <div className="google-setup-buttons">
                  <button className="member-page-action" onClick={() => go({ project, step: 1 })}>이전</button>
                  <button className="member-page-action management-primary" onClick={() => go({ project, step: 3 })}>동의 화면 설정을 마쳤습니다</button>
                </div>
              </>
            )}
            {step === 3 && (
              <>
                <p>Google에서 <strong>웹 애플리케이션</strong> 유형의 OAuth 클라이언트를 만들고 아래 콜백 주소를 승인된 리디렉션 URI에 그대로 등록하세요. 저장한 뒤 클라이언트 JSON을 다운로드하세요.</p>
                {callbackField}
                <div className="google-setup-buttons">
                  {copyButton}
                  <GoogleConsoleLink href={consoleLink("/auth/clients", project)} action>Google OAuth 클라이언트 열기</GoogleConsoleLink>
                </div>
                {copyNotice}
                <form className="google-setup-form" onSubmit={event => void save(event)}>
                  <label className="google-setup-field">Google 웹 클라이언트 JSON
                    <input ref={input} type="file" accept=".json,application/json" disabled={busy}
                      onChange={event => {
                        setFileSelected(Boolean(event.currentTarget.files?.length));
                        setError("");
                      }} />
                  </label>
                  <div className="google-setup-buttons">
                    <button type="button" className="member-page-action" disabled={busy} onClick={() => go({ project, step: project ? 2 : 0 })}>이전</button>
                    <button className="member-page-action management-primary" disabled={busy || !fileSelected}>{busy ? "저장 중…" : "이 설치에 저장"}</button>
                    {error && <button type="button" className="member-page-action" disabled={busy} onClick={() => {
                      setError("");
                      setFileSelected(false);
                      setRefresh(value => value + 1);
                    }}>설정 상태 다시 확인</button>}
                  </div>
                </form>
              </>
            )}
          </section>
        </>
      )}
    </section>
  );
}
