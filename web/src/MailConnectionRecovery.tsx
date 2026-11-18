import type { MailConnectionRecovery as Recovery } from "../shared/api-types";
import { rememberMailReturn } from "./mail-return";
import { GwsInboxHelp } from "./GwsInboxHelp";

// Only the server receipt chooses from this fixed local catalog. Assistant prose,
// provider URLs and user text never become navigation targets.
export function MailConnectionRecovery({ action, accountId, threadId, runId, canResume }: { action?: Recovery; accountId?: string; threadId?: string | null; runId?: string; canResume?: boolean }) {
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
  return target ? <section className="mail-connection-recovery" aria-label="Gmail 연결 도우미">
    {accountId && <GwsInboxHelp key={`gws-${accountId}`} accountId={accountId} context="chat" resumeTarget={canResume && threadId && runId ? {threadId, runId} : undefined} />}
    <p><a href="/settings/mail" onClick={() => { if(accountId)rememberMailReturn(accountId, threadId ?? null); }}>기기 Gmail 연결 안내 열기 ↗</a></p>
    <details><summary>다른 방법 · Google 웹 OAuth로 연결</summary>
    <p>{action === "admin_setup" ? "먼저 이 설치의 Google 앱을 준비한 뒤 본인 계정으로 로그인하세요." : action === "member_setup" ? "설치자의 Google 앱 준비가 필요합니다. 준비가 끝나면 본인 계정으로 연결하세요." : action === "choose_account" ? "설정에서 연결된 Google 계정과 상태를 확인하세요." : "Google 공식 화면에서 사용할 계정을 선택하고 Gmail 권한을 확인하세요."}</p>
    <p><a href={target.href} onClick={() => { if(accountId)rememberMailReturn(accountId, threadId ?? null); }}>{target.label}</a></p>
    </details>
    <p className="fine">기기 연결로 메일이 발송되지는 않습니다. 발송·예약 등 다른 작업은 자동으로 다시 실행하지 않습니다.</p>
  </section> : null;
}
