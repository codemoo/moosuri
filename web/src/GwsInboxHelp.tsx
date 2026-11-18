import { useCallback, useEffect, useRef, useState } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { announceGwsConnected, beginGws, claimGws, clearPendingGws, pendingGws, releaseGws, samePending, type GmailResumeTarget } from "./gws-connection";
import { GoogleLoginHelp } from "./GoogleLoginHelp";
import "./google-login-help.css";

type Readiness = { state: "unconfigured" | "needs_confirmation" | "ready" | "login_required" | "wrong_account" | "offline"; mailbox?: string; confirmation?: string };
type Overview = { configured: boolean; mailbox?: string; confirmed?: boolean };
export function GwsInboxHelp({ accountId, resumeTarget, context = "settings", configuredOnly = false }: { accountId: string; resumeTarget?: GmailResumeTarget; context?: "chat" | "settings"; configuredOnly?: boolean }) {
  const scope = useScope(accountId);
  const [overview, setOverview] = useState<Overview | null>(null);
  const [status, setStatus] = useState<Readiness | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [loginStarted, setLoginStarted] = useState(false);
  const busyRef = useRef(false);
  const targetThread = resumeTarget?.threadId;
  const targetRun = resumeTarget?.runId;
  const relevantPending = useCallback(() => {
    const value = pendingGws(accountId);
    if (context === "chat" && !targetRun && value?.target) return null;
    return !targetRun || (value?.target?.runId === targetRun && value.target.threadId === targetThread) ? value : null;
  }, [accountId, targetRun, targetThread, context]);
  const fail = useCallback((e: unknown) => {
    if (isAborted(e)) return;
    setError(e instanceof ApiFailure && e.status === 401
      ? "로그인이 만료됐습니다. 무수리에 다시 로그인한 뒤 연결을 확인하세요."
      : "기기 연결을 확인하지 못했습니다. 연결한 기기과 네트워크를 확인한 뒤 다시 확인하세요.");
  }, []);
  const verify = useCallback(async (confirmation?: string) => {
    if (busyRef.current || !claimGws(accountId)) return;
    busyRef.current = true; setBusy(true); setError(""); setNotice("");
    const pending = relevantPending();
    try {
      const next = await scope.request<Readiness>(confirmation ? "/mail/gws-inbox/confirm" : "/mail/gws-inbox/check", "POST", confirmation ? { confirmation } : {}, undefined, { timeoutMs: 30000 });
      setStatus(next);
      if (next.state !== "ready") return;
      setLoginStarted(false);
      // Settings status alone never authorizes a read. Only the owner's pending
      // click and this fresh check may invoke the original server-owned intent.
      if (!pending || !samePending(pending, relevantPending())) {
        setNotice("기기 Gmail 연결을 확인했습니다.");
        return;
      }
      if (pending.target) {
        try {
          await scope.request(`/threads/${encodeURIComponent(pending.target.threadId)}/runs/${encodeURIComponent(pending.target.runId)}/resume-gmail`, "POST", {}, undefined, { timeoutMs: 30000 });
        } catch (e) {
          if (e instanceof ApiFailure && [403, 404, 409].includes(e.status)) {
            clearPendingGws(accountId);
            setError("Gmail은 연결됐지만 원래 요청은 이어갈 수 없습니다. 대화에서 최근 받은메일을 새로 요청하세요.");
            return;
          }
          throw e; // The same server receipt makes an unknown outcome recoverable.
        }
      }
      if (!samePending(pending, relevantPending())) return;
      clearPendingGws(accountId);
      setNotice(pending.target ? "연결했습니다. 원래 받은메일 요청을 이어갑니다." : "기기 Gmail 연결을 마쳤습니다.");
      if (pending.target) announceGwsConnected(accountId, pending.target);
    } catch (e) { fail(e); }
    finally { busyRef.current = false; releaseGws(accountId); setBusy(false); }
  }, [accountId, scope, relevantPending, fail]);
  useEffect(() => {
    void scope.request<Overview>("/mail/gws-inbox").then(setOverview).catch(fail);
  }, [scope, fail]);
  useEffect(() => {
    // Returning from the trusted device's Google screen may only continue an
    // already explicit, unexpired workflow belonging to this account/card.
    const returned = () => { if ((!configuredOnly || overview?.configured) && document.visibilityState === "visible" && relevantPending()) void verify(); };
    returned();
    window.addEventListener("focus", returned);
    window.addEventListener("online", returned);
    document.addEventListener("visibilitychange", returned);
    return () => {
      window.removeEventListener("focus", returned);
      window.removeEventListener("online", returned);
      document.removeEventListener("visibilitychange", returned);
    };
  }, [configuredOnly, overview?.configured, relevantPending, verify]);
  async function login() {
    if (busyRef.current || !claimGws(accountId)) return;
    busyRef.current = true; setBusy(true); setError("");
    try {
      const result = await scope.request<Readiness | { state: "login_started"; mailbox?: string }>("/mail/gws-inbox/login", "POST", {}, undefined, { timeoutMs: 30000 });
      if (result.state === "login_started") { setLoginStarted(true); setNotice("연결한 기기에서 Google 로그인 화면을 열었습니다. 로그인·2단계 인증을 마친 뒤 아래에서 연결을 확인하세요."); }
      else setStatus(result);
    } catch (e) {
      if (!isAborted(e)) setError("로그인 화면을 열었는지 확인하지 못했습니다. 연결한 기기를 확인하거나 아래 직접 연결 안내를 사용하세요.");
    } finally { busyRef.current = false; releaseGws(accountId); setBusy(false); }
  }
  function connect() {
    if (busyRef.current) return;
    if (!relevantPending()) beginGws(accountId, targetThread && targetRun ? {threadId: targetThread, runId: targetRun} : undefined);
    void verify();
  }
  const mailbox = status?.mailbox ?? overview?.mailbox;
  const descriptions: Record<Readiness["state"], string> = {
    unconfigured: "아직 기기 Gmail 연결이 준비되지 않았습니다. 설치자에게 이 계정의 기기 연결을 요청하세요.",
    needs_confirmation: "이 Gmail 계정의 최근 받은메일을 무수리에서 읽을까요? 표시된 계정을 확인해 주세요.",
    ready: "기기의 Google 로그인과 Gmail 읽기 연결을 확인했습니다.",
    login_required: "연결한 기기에서 Google 로그인이 필요합니다.",
    wrong_account: "연결한 기기에 로그인된 Google 계정이 연결할 Gmail과 다릅니다. 올바른 계정으로 다시 로그인하세요.",
    offline: "연결한 기기에 닿지 않습니다. 연결한 기기를 켜고 네트워크 연결을 확인한 뒤 다시 확인하세요.",
  };
  if (configuredOnly && !overview?.configured) return null;
  return <section className="gws-inbox-help" aria-label="기기 Gmail 연결" aria-busy={busy}>
    <div className="gws-inbox-heading"><span className="gws-inbox-symbol" aria-hidden="true">G</span><div><h4>기존 기기 Gmail 연결 · 선택 사항</h4><p className="fine">기기의 Google 로그인으로 최근 받은메일 읽기</p></div></div>
    {mailbox && <p className="gws-mailbox"><span className="fine">연결할 계정</span><strong>{mailbox}</strong></p>}
    <p role="status">{busy ? "기기 Gmail 연결 확인 중…" : status ? descriptions[status.state] : overview ? overview.configured ? overview.confirmed ? "이 계정으로 연결한 기록이 있습니다. 연결을 확인하면 사용할 수 있어요." : "기기 연결이 준비됐습니다. 사용할 Gmail 계정을 확인해 주세요." : descriptions.unconfigured : error ? "기기 연결 상태를 불러오지 못했습니다." : "기기 연결 설정 확인 중…"}</p>
    {error && <p role="alert" className="notice">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    <div className="gws-inbox-actions">
      {status?.state === "needs_confirmation" && status.mailbox && status.confirmation
        ? <button type="button" disabled={busy} onClick={() => void verify(status.confirmation)}>이 Gmail 계정으로 연결</button>
        : <button type="button" disabled={busy || (!overview && !error)} onClick={connect}>{busy ? "확인 중…" : loginStarted ? "로그인 완료 · 연결 확인" : status ? "다시 연결 확인" : resumeTarget ? "Gmail 연결하고 요청 이어가기" : "기기 Gmail 연결하기"}</button>}
      {status?.state === "needs_confirmation" && <button type="button" disabled={busy} onClick={connect}>계정 다시 확인</button>}
      {(status?.state === "login_required" || status?.state === "wrong_account") && <button type="button" disabled={busy} onClick={() => void login()}>{loginStarted ? "연결한 기기에서 Google 로그인 다시 열기" : "연결한 기기에서 Google 로그인 열기"}</button>}
    </div>
    {relevantPending() && <button type="button" className="text-button" disabled={busy} onClick={() => { clearPendingGws(accountId); setStatus(null); setLoginStarted(false); setError(""); setNotice("연결을 취소했습니다. 원래 요청을 자동으로 이어가지 않습니다."); }}>연결 취소</button>}
    {resumeTarget && <p className="fine">연결을 확인하면 원래 최근 받은메일 요청만 이어갑니다.</p>}
    {(status?.state === "unconfigured" || overview?.configured === false) && <p><a href="https://github.com/codemoo/moosuri/blob/main/docs/gws-inbox.md" target="_blank" rel="noreferrer">기기 Gmail 설치 안내 ↗</a></p>}
    <details><summary>직접 연결 · 도움말</summary>
      <p>연결한 기기의 Google 공식 화면에서 계정 선택·로그인·2단계 인증을 완료하세요. 이 화면에서는 ID·비밀번호·인증 코드를 입력하지 않습니다.</p>
      <ol><li>처음이라면 설치자가 이 무수리 계정과 사용할 기기의 GWS 브리지를 연결해야 합니다.</li><li>연결한 기기에서 <code>gws auth login --readonly --services gmail</code>을 실행해 직접 로그인할 수도 있어요.</li><li>완료 후 이 화면의 연결 확인 버튼을 누르세요. Google 로그인만으로 연결이 완료되지는 않습니다.</li></ol>
      <p><a href="https://github.com/codemoo/moosuri/blob/main/docs/gws-inbox.md" target="_blank" rel="noreferrer">기기 Gmail 설치 안내 ↗</a> · Mac·Linux 데스크톱에서 Google 로그인을 완료할 수 있어요.</p>
      <p>최근 받은메일 읽기 전용입니다. 이 연결로 메일을 발송하지 않습니다. 연결한 기기가 켜져 있어야 합니다.</p>
      <GoogleLoginHelp accountId={accountId} />
    </details>
  </section>;
}
