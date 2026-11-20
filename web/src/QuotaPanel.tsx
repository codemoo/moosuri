import {useState} from "react";
import type {ProviderQuota} from "../shared/api-types";
import {useScope} from "./account-scope";
import {ApiFailure, isAborted} from "./api";

type ClaudeLimit = {label: string; remaining_percent: number | null; resets_at: string | null; scope: string | null};
type Props = {accountId: string; id: string; revision: number; kind: "claude" | "chatgpt" | "shared" | "kiri"; onExpired: () => void; onUnavailable?: () => void};
type Row = {label: string; used?: string | null; limit?: string | null; remaining: string | null; reset: Date | null; interval?: string | null};
type Report = {rows: Row[]; message: string; observed: Date | null};
const date = (value: Date) => value.toLocaleString(undefined, {year:"numeric", month:"short", day:"numeric", hour:"2-digit", minute:"2-digit", second:"2-digit", timeZoneName:"short"});
const amount = (value: number, usd: boolean) => usd
  ? new Intl.NumberFormat(undefined, {style:"currency", currency:"USD", maximumFractionDigits:20}).format(value)
  : value.toLocaleString() + "회";
const intervals: Record<string,string> = {daily:"매일", weekly:"매주", monthly:"매월"};

export function QuotaPanel({accountId, id, revision, kind, onExpired, onUnavailable}: Props) {
  const scope = useScope(accountId);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState(false);
  async function kiriQuota(): Promise<ProviderQuota> {
    type Probe = {probe_id: string; report: ProviderQuota | null};
    const deadline = Date.now() + 25000;
    let probe: string | null = null;
    do {
      const response: Probe = await scope.request<Probe>("/personal/admin/engine/quota", "POST",
        {revision, connection_revision: id, probe_id: probe}, undefined, {timeoutMs: 5000});
      if (!response.probe_id || (probe && response.probe_id !== probe)) throw new Error("quota_read_changed");
      probe = response.probe_id;
      if (response.report) {
        if (response.report.provider_id !== id || response.report.provider_revision !== revision) throw new Error("quota_binding_changed");
        return response.report;
      }
      await new Promise(resolve => setTimeout(resolve, 700));
    } while (Date.now() < deadline);
    throw new Error("quota_read_expired");
  }
  async function read() {
    setBusy(true); setError(false);
    if (kind === "kiri") setReport(null);
    try {
      if (kind === "claude") {
        const value = await scope.request<{status:string; limits:ClaudeLimit[]}>(`/settings/claude/${encodeURIComponent(id)}/usage`, "POST", {revision});
        setReport({
          rows: value.limits.map(limit => ({label:[limit.scope || "구독 공통", limit.label].join(" · "), remaining:limit.remaining_percent === null ? null : limit.remaining_percent.toLocaleString(undefined,{maximumFractionDigits:2}) + "%", reset:limit.resets_at ? new Date(limit.resets_at) : null})),
          message: value.status === "available" ? (value.limits.length ? "" : "Claude가 표시할 한도 항목을 반환하지 않았습니다.") : "이 Claude 연결에서는 현재 구독 잔여량과 리셋 시각을 조회할 수 없습니다.",
          observed: value.status === "available" ? new Date() : null,
        });
      } else {
        const base = kind === "shared" ? "/admin/providers" : "/settings/chatgpt";
        const value = kind === "kiri" ? await kiriQuota() : await scope.request<ProviderQuota>(`${base}/${encodeURIComponent(id)}/quota?revision=${revision}`);
        setReport({
          rows:value.metrics.map(m => ({label:m.label, used: m.used == null ? null : amount(m.used, m.unit === "usd"), limit: m.limit == null ? null : amount(m.limit, m.unit === "usd"), remaining:m.remaining === null ? null : amount(m.remaining, m.unit === "usd"),reset:m.reset_at === null ? null : new Date(m.reset_at*1000), interval:m.reset_interval})),
          message:value.status === "known" ? "이 API 키를 쓰는 모델이 함께 사용하는 한도입니다." : kind === "chatgpt" ? "ChatGPT 구독 연결은 잔여량과 리셋 시각을 제공하지 않습니다." : value.status === "unavailable" ? "제공사에서 한도 정보를 받지 못했습니다." : "이 제공사는 현재 연결로 조회할 수 있는 한도 정보를 제공하지 않습니다.",
          observed:value.observed_at === null ? null : new Date(value.observed_at*1000),
        });
      }
    } catch (e) {
      if (isAborted(e)) return;
      if (e instanceof ApiFailure && e.status === 401) onExpired();
      else if (e instanceof ApiFailure && e.status === 403 && onUnavailable) onUnavailable();
      else setError(true);
    } finally { setBusy(false); }
  }
  return <div className="quota-panel">
    {kind === "kiri" && !report && <p>API 연결 · 잔여량 확인 불가 · 초기화 시각 확인 불가</p>}
    <button type="button" className="text-button" disabled={busy} onClick={() => void read()}>{busy ? "사용량 확인 중…" : report ? "사용량 다시 확인" : "남은 사용량 확인"}</button>
    {error && <p role="status">사용량을 새로 확인하지 못했습니다.</p>}
    {report && <>
      {report.message && <p>{report.message}</p>}
      {report.rows.map((row,i) => <p key={i}><strong>{row.label}</strong>{row.used && ` · 사용 ${row.used}`}{row.limit && ` · 한도 ${row.limit}`} · {row.remaining === null ? "잔여량 미제공" : `남은 양 ${row.remaining}`} · {row.reset ? `리셋 ${date(row.reset)}` : row.interval ? `리셋 주기 ${intervals[row.interval] ?? row.interval} (시각 미제공)` : "리셋 시각 미제공"}</p>)}
      {report.observed && <small>{date(report.observed)} 기준</small>}
      {(kind === "chatgpt" || kind === "claude") && <p><a href={kind === "claude" ? "https://claude.ai/settings/usage" : "https://chatgpt.com/settings/usage"} target="_blank" rel="noreferrer">공식 사용량 페이지 열기</a></p>}
    </>}
  </div>;
}
