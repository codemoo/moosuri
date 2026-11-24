import { useEffect, useRef, useState } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { browserTimezone, describeIntent, resolveStopTarget, toMonitorRequest, type MonitorIntent } from "./MemberWorkMonitorsChat";

// Contract of /api/v1/personal/tasks/monitors (see crates/moosuri-store .../workflows/monitors/types.rs).
export type MonitorKind = "watch" | "brief";
export type MonitorSourceRef = { id: string; revision: number; available: boolean; title: string | null };
export type MonitorEvidence = { run_id: string; read_at: number; limited: boolean; sources: MonitorSourceRef[] };
export type MonitorCheck = { due_at: number; checked_at: number; outcome: string; source_count: number; skipped: number; run_id: string | null; notified: boolean };
export type MonitorView = {
  id: string; kind: MonitorKind; title: string; scope: "memory" | "conversation"; query: string; state: string;
  every_seconds: number | null; at: string | null; timezone: string | null; days: number; revision: number;
  created_at: number; expires_at: number; next_due: number | null; last_checked_at: number | null; last_changed_at: number | null;
  alerts: number; basis: string; latest: MonitorEvidence | null; recent: MonitorCheck[];
};
export type MonitorPage = { items: MonitorView[]; active_limit: number };
export type MonitorRequest = {
  request_id: string; kind: MonitorKind; title: string; scope: "memory" | "conversation"; query: string;
  every_seconds?: number; at?: string; timezone?: string; days: number;
};

const BASE = "/personal/tasks/monitors";
const record = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);
const int = (v: unknown) => Number.isSafeInteger(v);
const maybeInt = (v: unknown) => v === null || Number.isSafeInteger(v);
export function isMonitor(v: unknown): v is MonitorView {
  return record(v) && [v.id, v.kind, v.title, v.query, v.state, v.basis].every(x => typeof x === "string")
    && (v.kind === "watch" || v.kind === "brief") && int(v.revision) && int(v.expires_at) && maybeInt(v.next_due)
    && Array.isArray(v.recent) && (v.latest === null || record(v.latest) && Array.isArray(v.latest.sources));
}
export function isMonitorPage(v: unknown): v is MonitorPage {
  return record(v) && Array.isArray(v.items) && v.items.length <= 100 && v.items.every(isMonitor) && int(v.active_limit);
}

const STATE: Record<string, string> = { active: "진행 중", stopped: "중지됨", expired: "기간 종료", revoked: "권한이 바뀌어 중지됨" };
const OUTCOME: Record<string, string> = { baseline: "기준 상태 저장", unchanged: "변화 없음", changed: "변화를 알렸어요", briefed: "브리핑을 보냈어요", missed: "시간이 지나 건너뜀", failed: "확인하지 못함" };
const EVERY: Record<number, string> = { 1800: "30분", 3600: "1시간", 86400: "하루" };
const when = (m: MonitorView) => m.kind === "watch" ? `${EVERY[m.every_seconds ?? 0] ?? ""}마다 확인 · 바뀔 때만 알림` : `매일 ${m.at} (${m.timezone}) 브리핑`;
const time = (v: number) => new Date(v * 1000).toLocaleString();

function failure(e: unknown, onExpired: () => void, conflict = "상태가 바뀌었어요. 최신 상태를 확인해 주세요."): string | null {
  if (isAborted(e)) return null;
  if (e instanceof ApiFailure && e.status === 401) { onExpired(); return null; }
  if (e instanceof ApiFailure && e.status === 409) return conflict;
  return "요청을 완료하지 못했어요. 같은 요청이라 다시 눌러도 중복되지 않아요.";
}

type ListProps = { accountId: string; onExpired: () => void; refreshKey?: string; onOpenRun?: (runId: string) => void };

/** Status list. Reads only: checks run on the member timer, never from this view. */
export function MemberWorkMonitors({ accountId, onExpired, refreshKey, onOpenRun }: ListProps) {
  return <Monitors key={accountId} accountId={accountId} onExpired={onExpired} refreshKey={refreshKey} onOpenRun={onOpenRun} />;
}
function Monitors({ accountId, onExpired, refreshKey, onOpenRun }: ListProps) {
  const scope = useScope(accountId, "member-monitors");
  const [page, setPage] = useState<MonitorPage | null>(null);
  const [error, setError] = useState(""), [busy, setBusy] = useState(false);
  const alive = useRef(true), stopIds = useRef(new Map<string, string>());
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  async function load() {
    try {
      const v = await scope.request<unknown>(BASE);
      if (!isMonitorPage(v)) throw new Error("Invalid monitor page");
      if (alive.current) { setPage(v); setError(""); }
    } catch (e) { const m = failure(e, onExpired); if (m && alive.current) setError("감시 상태를 확인하지 못했어요. 표시된 정보가 최신이 아닐 수 있어요."); }
  }
  useEffect(() => { void load(); const timer = setInterval(() => void load(), 15000); return () => clearInterval(timer); }, [scope, refreshKey]);
  async function stop(m: MonitorView) {
    if (busy) return;
    setBusy(true);
    // The same request id is kept until the server answers, so a lost response never double-applies.
    const request_id = stopIds.current.get(m.id) ?? crypto.randomUUID();
    stopIds.current.set(m.id, request_id);
    try {
      await scope.request(`${BASE}/${encodeURIComponent(m.id)}/stop`, "POST", { request_id, revision: m.revision });
      stopIds.current.delete(m.id); await load();
    } catch (e) {
      if (e instanceof ApiFailure && e.status >= 400 && e.status < 500) stopIds.current.delete(m.id);
      const msg = failure(e, onExpired); if (msg && alive.current) setError(msg);
    } finally { if (alive.current) setBusy(false); }
  }
  if (!page) return error ? <p role="alert">{error}</p> : null;
  if (!page.items.length) return <p className="member-monitors-empty">진행 중인 감시나 브리핑이 없어요. “‘서비스 상태’ 기억이 바뀌면 알려줘”처럼 말해 보세요.</p>;
  return <section className="member-monitors" aria-label="내 감시와 브리핑">
    {error && <p role="alert">{error}</p>}
    {page.items.map(m => <article className="member-monitor" key={m.id}>
      <p className="member-monitor-state">{STATE[m.state] ?? "상태 확인 필요"} · {m.kind === "watch" ? "변화 감시" : "일일 브리핑"}</p>
      <h3>{m.title}</h3>
      <p>{when(m)} · {m.days}일간</p>
      <p className="member-monitor-basis">{m.basis}</p>
      {m.state === "active" && <p>{m.last_checked_at ? `${time(m.last_checked_at)} 확인` : "첫 확인 대기 중"}{m.next_due ? ` · 다음 ${time(m.next_due)}` : ""} · 알림 {m.alerts}회</p>}
      {m.recent[0] && <p>최근: {OUTCOME[m.recent[0].outcome] ?? m.recent[0].outcome}{m.recent[0].skipped > 0 ? ` (앱이 꺼져 있던 ${m.recent[0].skipped}번은 묶어서 확인)` : ""}</p>}
      {m.latest && <details><summary>확인한 근거 {m.latest.sources.length}건{m.latest.limited ? " · 조회 한도 내" : ""}</summary>
        <ul>{m.latest.sources.map(s => <li key={`${s.id}:${s.revision}`}>{s.available && s.title ? s.title : "원본이 변경되었거나 더 이상 볼 수 없어요"}</li>)}</ul>
        {onOpenRun && <button onClick={() => onOpenRun(m.latest!.run_id)}>업무 기록에서 보기</button>}
      </details>}
      {m.state === "active" && <button disabled={busy} onClick={() => void stop(m)}>멈추기</button>}
    </article>)}
  </section>;
}

type CardProps = { accountId: string; intent: Exclude<MonitorIntent, { kind: "list" }>; onDone: (message: string) => void; onCancel: () => void; onExpired: () => void };

/** Confirmation for a parsed chat request. Nothing is created or stopped until the member presses the button. */
export function MonitorIntentCard(props: CardProps) {
  return <Card key={`${props.accountId}/${JSON.stringify(props.intent)}`} {...props} />;
}
function Card({ accountId, intent, onDone, onCancel, onExpired }: CardProps) {
  const scope = useScope(accountId, "member-monitor-card");
  const [query, setQuery] = useState(intent.kind === "stop" ? "" : intent.query);
  const [error, setError] = useState(""), [busy, setBusy] = useState(false);
  const [matches, setMatches] = useState<MonitorView[] | null>(null);
  const pending = useRef<{ id: string; query: string } | null>(null);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    if (intent.kind !== "stop") return;
    scope.request<unknown>(BASE).then(v => { if (isMonitorPage(v) && alive.current) setMatches(v.items); }).catch(e => { const m = failure(e, onExpired); if (m && alive.current) setError(m); });
  }, [scope]);
  async function start() {
    if (busy || intent.kind === "stop") return;
    setBusy(true);
    // Same id for the same body until acknowledged; an edited query is a different request.
    if (pending.current?.query !== query) pending.current = { id: crypto.randomUUID(), query };
    const body = toMonitorRequest({ ...intent, query: query.trim() }, pending.current.id, browserTimezone());
    try {
      await scope.request(BASE, "POST", body);
      pending.current = null; onDone("시작했어요. 진행 상황은 내 감시 목록에서 볼 수 있어요.");
    } catch (e) {
      if (e instanceof ApiFailure && e.status >= 400 && e.status < 500) pending.current = null;
      const m = failure(e, onExpired, "이미 같은 이름의 요청이 있어요. 목록을 확인해 주세요."); if (m && alive.current) setError(m);
    } finally { if (alive.current) setBusy(false); }
  }
  async function stop(m: { id: string; revision: number }) {
    if (busy) return;
    setBusy(true);
    try {
      await scope.request(`${BASE}/${encodeURIComponent(m.id)}/stop`, "POST", { request_id: crypto.randomUUID(), revision: m.revision });
      onDone("멈췄어요. 이미 보낸 알림은 목록에 남아 있어요.");
    } catch (e) { const msg = failure(e, onExpired); if (msg && alive.current) setError(msg); }
    finally { if (alive.current) setBusy(false); }
  }
  const resolved = intent.kind === "stop" && matches ? resolveStopTarget(intent.target, matches) : null;
  return <section className="member-monitor-card" aria-label="감시 요청 확인">
    <p>{describeIntent(intent.kind === "stop" ? intent : { ...intent, query: query.trim() || intent.query })}</p>
    {intent.kind !== "stop" && <label>확인할 내용<input value={query} maxLength={128} disabled={busy} onChange={e => setQuery(e.target.value)} /></label>}
    {error && <p role="alert">{error}</p>}
    {intent.kind !== "stop" && <button disabled={busy || !query.trim()} onClick={() => void start()}>시작</button>}
    {resolved?.found === "one" && <button disabled={busy} onClick={() => void stop(resolved)}>멈추기</button>}
    {resolved?.found === "none" && <p>멈출 감시를 찾지 못했어요.</p>}
    {resolved?.found === "many" && <p>여러 감시가 있어요. 이름을 더 구체적으로 말해 주세요: {resolved.titles.join(", ")}</p>}
    <button disabled={busy} onClick={onCancel}>닫기</button>
  </section>;
}
