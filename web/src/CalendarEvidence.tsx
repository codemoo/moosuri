import type { CalendarChatReceipt } from "../shared/api-types";

export function CalendarEvidence({ receipt }: { receipt: CalendarChatReceipt }) {
  return <aside aria-label="Calendar 작업 영수증">
    <strong>Calendar · {receipt.state === "applied" ? "적용됨" : receipt.state === "observed_absent" ? "현재 없음 · 인과관계 불명" : receipt.state === "outcome_unknown" || receipt.state === "dispatch_committed" ? "결과 확인 필요" : receipt.state === "conflict" ? "변경 충돌" : receipt.state === "connection_needed" ? "연결 필요" : receipt.state === "clarify" ? "추가 정보 필요" : receipt.state === "unsupported" ? "지원 범위 확인" : "처리 상태"}</strong>
    <p>{receipt.message}</p>
    {receipt.action_id && <p>작업 ID: <code>{receipt.action_id}</code></p>}
    <a href="/settings/calendar">Calendar 기록과 결과 조회 ↗</a>
  </aside>;
}
