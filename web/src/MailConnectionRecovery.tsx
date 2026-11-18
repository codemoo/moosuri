import type { MailConnectionRecovery as Recovery } from "../shared/api-types";
import { GoogleLoginHelp } from "./GoogleLoginHelp";
import { rememberMailReturn } from "./mail-return";
import { GwsInboxHelp } from "./GwsInboxHelp";

// Only the server receipt chooses from this fixed local catalog. Assistant prose,
// provider URLs and user text never become navigation targets.
export function MailConnectionRecovery({ action, accountId, threadId }: { action?: Recovery; accountId?: string; threadId?: string | null }) {
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
    <h4>Gmail 연결 도우미</h4>
    {accountId && <GwsInboxHelp key={`gws-${accountId}`} accountId={accountId} />}
    <p><a href="/settings/mail" onClick={() => { if(accountId)rememberMailReturn(accountId, threadId ?? null); }}>기기 Gmail 연결 안내 열기 ↗</a></p>
    {accountId && <GoogleLoginHelp key={accountId} accountId={accountId} />}
    <details><summary>Google 웹 OAuth로 연결</summary>
    <p>{action === "admin_setup" ? "먼저 이 설치의 Google 앱을 준비한 뒤 본인 계정으로 로그인하세요." : action === "member_setup" ? "설치자의 Google 앱 준비가 필요합니다. 준비가 끝나면 본인 계정으로 연결하세요." : action === "choose_account" ? "설정에서 연결된 Google 계정과 상태를 확인하세요." : "Google 공식 화면에서 사용할 계정을 선택하고 Gmail 권한을 확인하세요."}</p>
    <p><a href={target.href} onClick={() => { if(accountId)rememberMailReturn(accountId, threadId ?? null); }}>{target.label}</a></p>
    </details>
    <p className="fine">연결 후 ‘대화로 돌아가기’로 돌아와 원래 요청을 다시 보내세요. 연결·볼트 저장으로 메일이 발송되지는 않으며, 발송할 때마다 최종 내용을 확인하고 허락합니다.</p>
  </section> : null;
}
