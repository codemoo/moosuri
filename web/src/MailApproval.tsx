import { useState } from "react";
import type { MailAction } from "../shared/api-types";
export function MailApproval({
  action,
  decide,
  edit,
  disabled = false,
}: {
  disabled?: boolean;
  action: MailAction;
  decide: (approve: boolean) => Promise<void>;
  edit: () => void;
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
