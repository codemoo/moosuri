import { useState } from "react";
import type { MailConnectionRecovery as Recovery } from "../shared/api-types";
import { useScope } from "./account-scope";
import { errorText, isAborted } from "./api";
import { rememberMailReturn } from "./mail-return";
import { beginGoogleMailResume, clearGoogleMailResume, clearGoogleMailResumeForFlow } from "./google-mail-connection";
import { GwsInboxHelp } from "./GwsInboxHelp";

// Only a server receipt chooses from this fixed local catalog.
export function MailConnectionRecovery({ action, accountId, threadId, runId, canResume }: { action?: Recovery; accountId?: string; threadId?: string | null; runId?: string; canResume?: boolean }) {
  const scope = useScope(accountId);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!action) return null;
  const actions: Record<Recovery, { href: string; label: string }> = {
    gws_setup: { href: "/settings/mail", label: "Gmail 연결 설정 열기 ↗" },
    admin_setup: { href: "/admin/google", label: "Google 설치 설정 열기 ↗" },
    member_setup: { href: "/settings/mail", label: "Gmail 연결 안내 열기 ↗" },
    connect: { href: "/settings/mail", label: "내 Gmail 읽기 연결하기 ↗" },
    reconnect: { href: "/settings/mail", label: "Gmail 다시 연결하기 ↗" },
    choose_account: { href: "/settings/mail", label: "연결된 Gmail 계정 확인 ↗" },
  };
  const target = Object.hasOwn(actions, action) ? actions[action] : null;
  const resumeAfterLogin = canResume && ["connect", "reconnect", "gws_setup"].includes(action);
  async function connect() {
    if (!accountId || busy) return;
    setBusy(true); setError("");
    rememberMailReturn(accountId, threadId ?? null);
    clearGoogleMailResume(accountId);
    let startedFlow: string | null = null;
    try {
      const result = await scope.request<{ authorization_url: string; flow_id: string }>("/mail/oauth/start", "POST", {});
      const url = new URL(result.authorization_url);
      if (url.origin !== "https://accounts.google.com") throw new Error("Google 로그인 주소를 확인하지 못했습니다.");
      if (!/^[a-f0-9]{64}$/.test(result.flow_id)) throw new Error("Google 연결 흐름을 확인하지 못했습니다.");
      startedFlow = result.flow_id;
      beginGoogleMailResume(accountId, canResume && threadId && runId ? { threadId, runId } : undefined, startedFlow);
      window.location.assign(url.href);
    } catch (e) {
      clearGoogleMailResumeForFlow(accountId, startedFlow);
      if (!isAborted(e)) { setError(errorText(e)); setBusy(false); }
    }
  }
  return target ? <section className="mail-connection-recovery" aria-label="Gmail 연결 도우미">
    <p>{action === "admin_setup" ? "먼저 이 설치의 Google 앱을 준비한 뒤 본인 계정으로 로그인하세요." : action === "member_setup" ? "설치자의 Google 앱 준비가 필요합니다. 준비가 끝나면 본인 계정으로 연결하세요." : action === "choose_account" ? "설정에서 연결된 Google 계정과 상태를 확인하세요." : "Google 공식 화면에서 본인 계정으로 로그인하면 서버에서 Gmail을 사용할 수 있습니다."}</p>
    {accountId && ["connect", "reconnect"].includes(action) && <button type="button" disabled={busy} onClick={() => void connect()}>{busy ? "Google 로그인 여는 중…" : canResume ? "Google 로그인하고 요청 이어가기" : "Google 계정으로 로그인"}</button>}
    {error && <p role="alert">{error}</p>}
    <p><a href={target.href} onClick={() => { if(accountId) { rememberMailReturn(accountId, threadId ?? null); clearGoogleMailResume(accountId); } }}>{target.label}</a></p>
    {accountId && action === "gws_setup" && <GwsInboxHelp key={`gws-${accountId}`} accountId={accountId} context="chat" configuredOnly resumeTarget={canResume && threadId && runId ? {threadId, runId} : undefined} />}
    <p className="fine">{resumeAfterLogin ? "연결을 마치면 원래 받은메일 조회만 한 번 이어갑니다. " : ""}발송·예약 등 다른 작업은 자동으로 다시 실행하지 않습니다.</p>
  </section> : null;
}
