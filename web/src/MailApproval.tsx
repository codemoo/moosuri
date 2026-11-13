import { useState } from "react";
import type { MailAction } from "../shared/api-types";
export function MailApproval({
  action,
  decide,
  edit,
  checkSent,
  disabled = false,
}: {
  disabled?: boolean;
  action: MailAction;
  decide: (approve: boolean) => Promise<void>;
  edit: () => void;
  checkSent?: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const c = action.content;
  const label: Record<string, string> = {
    pending_approval: "발송 허락 대기 · 무수리 로컬 초안",
    approved: "허락됨 · 발송 대기",
    dispatch_committed: "Gmail에 발송 요청 중",
    sent: "Gmail 발송 접수",
    outcome_unknown:
      "발송 결과 불명 · 보낸편지함을 확인하세요. 자동 재발송하지 않습니다.",
    failed_definite: "발송 요청 거절됨",
    cancelled: "취소됨",
    rejected: "거절됨",
    expired: "허락 유효기간 만료",
  };
  return (
    <article className="mail-preview" aria-label="메일 발송 확인">
      <h3>{label[action.state] ?? action.state}</h3>
      {c && (
        <>
          <dl>
            <dt>보내는 사람 (From)</dt>
            <dd>{c.from}</dd>
            <dt>받는 사람 (To)</dt>
            <dd>{c.to.join(", ") || "없음"}</dd>
            <dt>참조 (Cc)</dt>
            <dd>{c.cc.join(", ") || "없음"}</dd>
            <dt>숨은참조 (Bcc)</dt>
            <dd>{c.bcc.join(", ") || "없음"}</dd>
            <dt>제목</dt>
            <dd>{c.subject || "(제목 없음)"}</dd>
            <dt>첨부</dt>
            <dd>없음 · 현재 첨부파일 발송은 지원하지 않습니다.</dd>
          </dl>
          <h4>전체 본문</h4>
          <pre className="mail-body">{c.text}</pre>
        </>
      )}
      {action.provider_message_id && (
        <p>
          Gmail 접수 ID: {action.provider_message_id}. 상대 수신·열람 여부는
          확인되지 않았습니다.
        </p>
      )}
      {action.reconciliation?.status === "receipt_conflict" && <p role="status">실제 발송 응답 영수증과 조회 후보 ID가 다릅니다. 실제 응답을 보존했으며 직접 보낸편지함을 확인하세요.</p>}
      {action.state === "outcome_unknown" && (
        <section aria-label="보낸편지함 조회 결과">
          <p>{({
            lookup_conflict: "서로 다른 일치 후보가 관찰되어 충돌이 있습니다. 직접 보낸편지함을 확인하세요.",
            matching_sent_observed: "보낸편지함에서 원래 발송 내용과 일치하는 메일 하나를 관찰했습니다. 발송 응답 영수증이나 상대 수신·열람 확인은 아닙니다.",
            no_candidates: "일치 후보 없음 · 검색 지연이나 누락일 수 있어 발송 실패를 뜻하지 않습니다.",
            multiple_candidates: "후보가 여러 개여서 확정할 수 없습니다.",
            incomplete_search: "검색 범위가 완전하지 않아 확정할 수 없습니다.",
            legacy_evidence_unavailable: "이전 작업의 원래 내용 증거가 없어 후보만 확인할 수 있습니다.",
            candidate_mismatch: "후보가 원래 내용과 일치하지 않아 결과가 불명입니다.",
            unsupported_candidate: "Google이 추가한 Received·DKIM·X-Google 헤더, Bcc 누락이나 인코딩 변경 때문에 전체 내용을 엄격히 비교할 수 없습니다. 실제 보낸 메일도 결과 불명으로 남을 수 있습니다.",
            lookup_unavailable: "조회하지 못했습니다. 발송 결과는 계속 불명입니다.",
            restore_quarantined: "복원된 작업의 조회 권한이 격리되어 있습니다.",
          } as Record<string,string>)[action.reconciliation?.status ?? ""] ?? "보낸편지함 확인 대기"}</p>
          {action.reconciliation?.last_checked_at && <p>최근 조회: {({matching_sent_observed:"동결 내용 일치",no_candidates:"후보 없음",multiple_candidates:"복수 후보",incomplete_search:"불완전한 검색",candidate_mismatch:"후보 불일치",unsupported_candidate:"비교할 수 없는 후보",legacy_evidence_unavailable:"원래 증거 없음",lookup_unavailable:"조회 불가"} as Record<string,string>)[action.reconciliation.last_result] ?? "확인 필요"}</p>}
          {action.reconciliation?.last_checked_at && <p>마지막 확인: {new Date(action.reconciliation.last_checked_at * 1000).toLocaleString()} · 후보 {action.reconciliation.candidate_count}개</p>}
          {checkSent && <button disabled={busy || disabled || action.reconciliation?.status === "restore_quarantined" || (action.reconciliation?.manual_after ?? 0) > Date.now()/1000} onClick={async () => { setBusy(true); try { await checkSent(); } finally { setBusy(false); } }}>보낸편지함 다시 확인</button>}
          <p>조회는 메일을 다시 보내지 않습니다. 자동 조회는 최대 네 번이며 직접 확인은 1분 간격입니다.</p>
        </section>
      )}
      {action.state === "pending_approval" && c && (
        <div className="dialog-actions">
          <button
            disabled={busy || disabled}
            className="secondary"
            onClick={edit}
          >
            내용 수정
          </button>
          <button
            disabled={busy || disabled}
            className="secondary"
            onClick={async () => {
              setBusy(true);
              try {
                await decide(false);
              } finally {
                setBusy(false);
              }
            }}
          >
            거절
          </button>
          <button
            disabled={busy || disabled}
            className="primary"
            onClick={async () => {
              setBusy(true);
              try {
                await decide(true);
              } finally {
                setBusy(false);
              }
            }}
          >
            허락하고 보내기
          </button>
        </div>
      )}
    </article>
  );
}
