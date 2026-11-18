import type { MailConnectionRecovery as Recovery } from "../shared/api-types";

// Only the server receipt chooses from this fixed local catalog. Assistant prose,
// provider URLs and user text never become navigation targets.
export function MailConnectionRecovery({ action }: { action?: Recovery }) {
  if (!action) return null;
  const actions: Record<Recovery, { href: string; label: string }> = {
    admin_setup: { href: "/admin/google", label: "Google 설치 설정 열기 ↗" },
    member_setup: { href: "/settings/mail", label: "Gmail 연결 안내 열기 ↗" },
    connect: { href: "/settings/mail", label: "내 Gmail 읽기 연결하기 ↗" },
    reconnect: { href: "/settings/mail", label: "Gmail 다시 연결하기 ↗" },
    choose_account: { href: "/settings/mail", label: "연결된 Gmail 계정 확인 ↗" },
  };
  const target = actions[action];
  return target ? <p><a href={target.href} target="_blank" rel="noreferrer">{target.label}</a></p> : null;
}
