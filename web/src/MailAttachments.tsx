import { useLayoutEffect, useRef, useState } from "react";
export type MailAttachment = {
    part_id: string;
    attachment_id: string | null;
    filename: string;
    mime_type: string;
    size: number;
    readable: boolean;
};
type Page = {
    status: string;
    text?: string;
    partial?: boolean;
    next_start_byte?: number | null;
    total_text_bytes?: number;
    page_digest?: string;
    pages?: number | null;
    attachment: MailAttachment;
};
const messages: Record<string, string> = { too_large: "첨부파일 크기 제한(2MiB)을 넘었습니다.", unsupported_type: "PDF와 UTF-8 텍스트만 읽을 수 있습니다.", invalid_document: "문서를 읽을 수 없습니다.", encrypted: "암호화된 PDF는 읽을 수 없습니다.", no_text: "추출할 텍스트가 없습니다. 스캔 이미지의 글자는 지원하지 않습니다.", unsupported_content: "지원하지 않는 문서 내용입니다.", extraction_limit: "텍스트 추출 범위(50쪽·24KiB)를 넘었습니다.", extractor_unavailable: "PDF 읽기가 구성되지 않았습니다.", extraction_timeout: "PDF 읽기 시간이 초과되었습니다.", changed_restart_read: "첨부파일 내용이 달라졌습니다. 처음부터 다시 읽어 주세요." };
function fileSize(bytes: number) {
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    if (bytes >= 1024) return `${Math.ceil(bytes / 1024)} KB`;
    return `${bytes} B`;
}
export function MailAttachments({ connection, message, attachments, request, fail }: {
    connection: string;
    message: string;
    attachments: MailAttachment[];
    request: <T>(path: string, method?: string, body?: unknown) => Promise<T>;
    fail: (error: unknown) => void;
}) {
    const [page, setPage] = useState<Page | null>(null);
    const [busy, setBusy] = useState(false);
    const generation = useRef(0);
    useLayoutEffect(() => { generation.current += 1; setPage(null); setBusy(false); return () => { generation.current += 1; }; }, [connection, message]);
    async function read(attachment: MailAttachment, next = false) { const current = generation.current; setBusy(true); try {
        const result = await request<Page>("/mail/attachments/read", "POST", { connection_id: connection, message_id: message, part_id: attachment.part_id, attachment_id: attachment.attachment_id, start_byte: next ? page?.next_start_byte ?? 0 : 0, max_bytes: 4096, expected_digest: next ? page?.page_digest ?? null : null });
        if (current === generation.current)
            setPage(result);
    }
    catch (error) {
        if (current === generation.current) {
            setPage(null);
            fail(error);
        }
    }
    finally {
        if (current === generation.current)
            setBusy(false);
    } }
    return <section aria-label="첨부파일">
    <h5>첨부파일</h5>
    {busy && <p role="status">첨부파일을 읽는 중…</p>}
    <ul>{attachments.map(a => <li key={a.part_id}>{a.filename} · {fileSize(a.size)} <button disabled={busy || !a.readable} onClick={() => void read(a)}>첨부 {a.filename} 읽기</button>{!a.readable && <span> · 지원 형식 또는 크기 제한을 벗어났습니다.</span>}</li>)}</ul>
    {page && <article><h5>{page.attachment.filename}</h5>{page.status === "ok" ? <><pre className="mail-body">{page.text}</pre>{page.pages && <p>PDF {page.pages}쪽</p>}{page.partial && <p>첨부 내용의 일부를 표시했습니다.</p>}{page.next_start_byte != null && <button disabled={busy} onClick={() => void read(page.attachment, true)}>첨부 다음 부분 읽기</button>}</> : <p>{messages[page.status] ?? "첨부파일을 읽을 수 없습니다."}</p>}</article>}
  </section>;
}
