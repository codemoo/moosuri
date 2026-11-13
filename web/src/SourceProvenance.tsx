import { useState } from "react";
import type {
  SourceBatchReceipt,
  SourceProvenance as Provenance,
} from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { sourceError, sourceKindName, sourceTime } from "./SourceConnections";

export function SourceProvenance({
  accountId,
  provenance,
  onExpired,
}: {
  accountId: string;
  provenance: Provenance;
  onExpired: () => void;
}) {
  return (
    <ProvenanceDetail
      key={`${accountId}:${provenance.connection_id}:${provenance.request_id}`}
      accountId={accountId}
      provenance={provenance}
      onExpired={onExpired}
    />
  );
}
function ProvenanceDetail({
  accountId,
  provenance: p,
  onExpired,
}: {
  accountId: string;
  provenance: Provenance;
  onExpired: () => void;
}) {
  const scope = useScope(
    accountId,
    `provenance:${p.connection_id}:${p.request_id}`,
  );
  const [receipt, setReceipt] = useState<SourceBatchReceipt | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function loadReceipt() {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      setReceipt(
        await scope.request<SourceBatchReceipt>(
          `/source-connections/${encodeURIComponent(p.connection_id)}/receipts/${encodeURIComponent(p.request_id)}`,
        ),
      );
    } catch (e) {
      if (!isAborted(e)) {
        if (e instanceof ApiFailure && e.status === 401) onExpired();
        else setError(sourceError(e));
      }
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="source-provenance" aria-label="원문 수집 출처">
      <h4>수집 출처</h4>
      <p>
        {p.connection_name} · {sourceKindName(p.kind)}
      </p>
      <p className="fine">무수리 수신: {sourceTime(p.received_at)}</p>
      <a
        href={`/settings/sources?connection=${encodeURIComponent(p.connection_id)}`}
      >
        수집 연결과 실행 이력 보기
      </a>
      <details>
        <summary>원본과 저장 기록</summary>
        <dl>
          <dt>수집 범위</dt>
          <dd>{p.scope}</dd>
          <dt>원본 항목</dt>
          <dd>{p.external_id}</dd>
          <dt>원본 버전</dt>
          <dd>{p.external_version}</dd>
          {p.thread_id && (
            <>
              <dt>원본 대화</dt>
              <dd>{p.thread_id}</dd>
            </>
          )}
          {p.parent_id && (
            <>
              <dt>상위 원본</dt>
              <dd>{p.parent_id}</dd>
            </>
          )}
          <dt>원문 저장 버전</dt>
          <dd>{p.source_revision}</dd>
        </dl>
        <button disabled={busy} onClick={() => void loadReceipt()}>
          {busy ? "저장 기록 확인 중…" : "수집 저장 기록 확인"}
        </button>
        {error && <p role="alert">{error}</p>}
        {receipt && (
          <div aria-label="수집 저장 기록">
            <p>자료 저장 확인: {sourceTime(receipt.committed_at)}</p>
            <p className="fine">
              이 기록은 무수리의 자료 저장을 확인합니다. 원본 내용이나 전체
              범위의 완료를 증명하지 않습니다.
            </p>
            <details>
              <summary>기록 식별 정보</summary>
              <dl>
                <dt>실행</dt>
                <dd>{p.run_id}</dd>
                <dt>수신 요청</dt>
                <dd>{receipt.request_id}</dd>
                <dt>저장 순서</dt>
                <dd>{receipt.sequence}</dd>
              </dl>
            </details>
          </div>
        )}
      </details>
    </section>
  );
}
