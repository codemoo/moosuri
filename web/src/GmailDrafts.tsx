import { useState } from "react";
import type { GmailDraft, GmailDraftPage, GmailDraftRef, MailAction, MailDraftContent } from "../shared/api-types";
import type { RequestScope } from "./api";
export function GmailDrafts({ connection, action, request, importDraft, fail, disabled }: {
  connection: string;
  action: MailAction | null;
  request: RequestScope["request"];
  importDraft: (content: MailDraftContent) => void;
  fail: (e: unknown) => void;
  disabled: boolean;
}) {
  const [page, setPage] = useState<GmailDraftPage | null>(null);
  const [draft, setDraft] = useState<GmailDraft | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const suffix = `connection_id=${encodeURIComponent(connection)}`;
  async function list(next = false) {
    const token = next && page?.next_page_token ? `&page_token=${encodeURIComponent(page.next_page_token)}` : "";
    setPage(await request<GmailDraftPage>(`/mail/drafts?${suffix}${token}`));
  }
  async function run(op: () => Promise<void>, writing = false) {
    setBusy(true); setStatus("");
    try { await op(); } catch (e) {
      if (writing) setStatus("Gmail 변경 결과를 확인하지 못했습니다. 초안 목록을 새로 읽어 확인한 뒤 작업하세요. 자동으로 다시 저장하거나 보내지 않습니다.");
      fail(e);
    } finally { setBusy(false); }
  }
  async function save(update: boolean) {
    if (!action || !action.content) return;
    await run(async () => {
      const saved = await request<GmailDraftRef>(`/mail/actions/${encodeURIComponent(action.id)}/provider-draft`, "POST", {
        expected: { revision: action.revision, digest: action.digest },
        draft_id: update ? draft?.id : null,
        expected_provider_digest: update ? draft?.digest : null,
      });
      setStatus("Gmail 초안함에 저장했습니다. 발송 허락은 부여하지 않았습니다.");
      setDraft(await request<GmailDraft>(`/mail/drafts/${encodeURIComponent(saved.id)}?${suffix}`));
      await list();
    }, true);
  }
  const canSave = action?.connection_id === connection && !!action.content && ["pending_approval", "approved"].includes(action.state);
  return <section className="admin-card" aria-label="Gmail 초안함">
    <h3>Gmail 초안함</h3>
    <p>Gmail에 실제 초안을 저장·조회·수정·삭제합니다. 저장은 발송 허락이 아닙니다. Gmail에서 바꾼 내용은 무수리의 승인된 내용을 바꾸지 않습니다.</p>
    <button disabled={disabled || busy} onClick={() => void run(() => list())}>Gmail 초안 목록 읽기</button>
    {page?.next_page_token && <button disabled={disabled || busy} onClick={() => void run(() => list(true))}>다음 초안 목록</button>}
    <ul>{page?.drafts.map(d => <li key={d.id}><button disabled={disabled || busy} onClick={() => void run(async () => { setDraft(null); setDraft(await request<GmailDraft>(`/mail/drafts/${encodeURIComponent(d.id)}?${suffix}`)); })}>Gmail 초안 {d.id} 읽기</button></li>)}</ul>
    {status && <p role="status">{status}</p>}
    {draft && <article>
      <h4>{draft.subject || "(제목 없음)"}</h4>
      {draft.content && <p>From: {draft.content.from}<br/>To: {draft.content.to.join(", ")}<br/>Cc: {draft.content.cc.join(", ")}<br/>Bcc: {draft.content.bcc.join(", ")}</p>}
      <pre className="mail-body">{draft.text}</pre>
      {draft.text_truncated && <p>일부 본문만 표시되었습니다.</p>}
      {draft.attachment_names.length > 0 && <p>첨부: {draft.attachment_names.join(", ")}</p>}
      {draft.content ? <button disabled={disabled || busy} onClick={() => importDraft(draft.content!)}>로컬 편집기로 복사 · 새 발송 확인 필요</button> : <p>이 초안의 MIME·HTML·첨부·별칭·답장 메타데이터는 현재 편집 범위를 벗어납니다. 덮어쓰거나 발송용으로 가져오지 않습니다.</p>}
      <button disabled={disabled || busy} onClick={() => void run(async () => {
        await request(`/mail/drafts/${encodeURIComponent(draft.id)}`, "DELETE", { connection_id: connection, expected_digest: draft.digest });
        setDraft(null); setStatus("Gmail 초안을 삭제했습니다. 로컬 발송 작업은 별개로 유지됩니다."); await list();
      }, true)}>이 Gmail 초안 삭제</button>
    </article>}
    {canSave && <>
      <p>아래 발송 확인에 표시된 로컬 내용으로 Gmail 초안을 저장합니다.</p>
      <button disabled={disabled || busy} onClick={() => void save(false)}>확인본을 Gmail에 새 초안으로 저장</button>
      {draft?.content && <><p>Gmail은 원자적 수정 충돌 검사를 제공하지 않습니다. 다른 곳에서 동시에 편집하지 마세요. 목록에서 읽은 초안을 아래 로컬 확인본으로 덮어씁니다.</p><button disabled={disabled || busy} onClick={() => void save(true)}>선택한 Gmail 초안을 확인본으로 덮어쓰기</button></>}
    </>}
  </section>;
}
