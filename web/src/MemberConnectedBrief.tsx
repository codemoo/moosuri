import { useEffect, useRef, useState } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { IMPORTANT_MEANING, browserTimezone, describeConnectedBrief, toConnectedBriefRequest, type BriefSource, type ConnectedBriefIntent, type ConnectedBriefPins } from "./MemberWorkMonitorsChat";

// Contract of /api/v1/personal/tasks/briefs (see crates/moosuri-store .../mail/member_briefs.rs).
export type BriefMessage = { subject: string; from: string; received_at_ms: number };
export type BriefEvent = { title: string; start: string; end: string; all_day: boolean };
export type BriefDetail = { captured_at: number; messages: BriefMessage[] | null; events: BriefEvent[] | null };
export type BriefRun = {
  due_at: number; state: string; freshness: string | null; guidance: string | null; skipped: number;
  gmail_count: number | null; calendar_count: number | null; finished_at: number | null; detail: BriefDetail | null;
};
export type BriefView = {
  id: string; thread_id: string; sources: BriefSource[]; at: string; timezone: string; interval_days: number; state: string;
  end_reason: string | null; revision: number; created_at: number; expires_at: number; next_due: number | null; read_only: boolean; important_only: boolean; runs: BriefRun[];
};
export type BriefPage = { items: BriefView[]; active_limit: number };

const BASE = "/personal/tasks/briefs";
const record = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);
const int = (v: unknown) => Number.isSafeInteger(v);
export function isBrief(v: unknown): v is BriefView {
  return record(v) && [v.id, v.thread_id, v.at, v.timezone, v.state].every(x => typeof x === "string") && v.read_only === true && typeof v.important_only === "boolean"
    && Array.isArray(v.sources) && v.sources.every(s => s === "gmail" || s === "calendar") && int(v.revision) && int(v.expires_at)
    && Array.isArray(v.runs) && v.runs.length <= 30 && v.runs.every(r => record(r) && int(r.due_at) && typeof r.state === "string");
}
export function isBriefPage(v: unknown): v is BriefPage {
  return record(v) && Array.isArray(v.items) && v.items.length <= 30 && v.items.every(isBrief) && int(v.active_limit);
}

const SOURCE: Record<BriefSource, string> = { gmail: "Gmail", calendar: "캘린더" };
const STATE: Record<string, string> = { active: "진행 중", stopped: "중지됨", expired: "기간 종료", revoked: "연결이 바뀌어 중지됨" };
const RUN: Record<string, string> = {
  claimed: "읽는 중", completed: "브리핑 완료", partial: "일부만 확인했어요", unavailable: "지금 읽을 수 없어 브리핑을 만들지 않았어요",
  missed: "시간이 지나 건너뜀", unknown: "앱이 다시 시작되어 결과를 알 수 없어요. 다시 읽지 않았어요", withdrawn: "연결이 바뀌어 읽은 내용을 지웠어요",
};
const time = (v: number) => new Date(v * 1000).toLocaleString();
const names = (list: string[]) => list.map(s => SOURCE[s as BriefSource] ?? s).join("·");
/** Same-thread connection guidance: what to reconnect, and that no brief was made without it. */
export const connectionGuidance = (missing: string[]) =>
  `${names(missing)} 연결이 필요해요. Google을 연결한 뒤 이 대화로 돌아와 시작해 주세요. 연결 전에는 내용을 읽지 않고, 브리핑도 만들어지지 않아요.`;

function failure(e: unknown, onExpired: () => void, conflict = "연결 상태가 바뀌었어요. 연결을 다시 확인해 주세요."): string | null {
  if (isAborted(e)) return null;
  if (e instanceof ApiFailure && e.status === 401) { onExpired(); return null; }
  if (e instanceof ApiFailure && e.status === 409) return conflict;
  return "요청을 완료하지 못했어요. 같은 요청이라 다시 눌러도 중복되지 않아요.";
}

type Connection = { connection_id: string; epoch: number };
const connectionOf = (v: unknown): Connection | null =>
  record(v) && record(v.connection) && typeof v.connection.connection_id === "string" && int(v.connection.epoch)
    ? { connection_id: v.connection.connection_id, epoch: v.connection.epoch as number } : null;

type CardProps = { accountId: string; threadId: string; intent: ConnectedBriefIntent; request?: { request_id: string; zone: string; text: string }; onConnect?: () => void; onDone: (message: string) => void; onCancel: () => void; onExpired: () => void };

/** Confirmation for a parsed "매일 … 메일/일정 알려줘". Nothing is granted until the member presses the button. */
export function ConnectedBriefCard(props: CardProps) {
  return <Card key={`${props.accountId}/${props.threadId}/${JSON.stringify(props.intent)}`} {...props} />;
}
function Card({ accountId, threadId, intent, request, onConnect, onDone, onCancel, onExpired }: CardProps) {
  const scope = useScope(accountId, "member-connected-brief-card");
  const [pins, setPins] = useState<ConnectedBriefPins | null>(null);
  const [error, setError] = useState(""), [busy, setBusy] = useState(false);
  const pending = useRef<string | null>(null), alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    // The grant is pinned to the connection shown here; the server re-checks id + epoch.
    (async () => {
      const next: ConnectedBriefPins = {};
      for (const s of intent.sources) {
        try {
          const c = connectionOf(await scope.request<unknown>(s === "gmail" ? "/personal/google" : "/personal/google/calendar/status"));
          if (c) next[s] = c;
        } catch (e) {
          if (!(e instanceof ApiFailure && e.status === 404)) { const m = failure(e, onExpired); if (m && alive.current) setError(m); }
        }
      }
      if (alive.current) setPins(next);
    })();
  }, [scope]);
  const missing = pins ? intent.sources.filter(s => !pins[s]) : [];
  async function grant() {
    if (busy || !pins) return;
    setBusy(true);
    // One id per body until the server answers: a lost response never creates a second brief.
    pending.current ??= request?.request_id ?? crypto.randomUUID();
    const body = toConnectedBriefRequest(intent, pending.current, threadId, request?.zone ?? browserTimezone(), pins);
    if (!body) { pending.current = null; setError("연결 상태가 바뀌었어요. 연결을 다시 확인해 주세요."); setBusy(false); return; }
    try {
      const result = await scope.request<unknown>(BASE, "POST", request ? { ...body, message: request.text } : body);
      if (!isBrief(result) || result.id !== body.request_id || result.thread_id !== threadId) throw new Error("Invalid brief receipt");
      pending.current = null; onDone("시작했어요. 매번 읽은 결과는 이 대화의 브리핑 기록에 남아요.");
    } catch (e) {
      if (e instanceof ApiFailure && e.status >= 400 && e.status < 500) pending.current = null;
      const m = failure(e, onExpired); if (m && alive.current) setError(m);
    } finally { if (alive.current) setBusy(false); }
  }
  return <section className="member-brief-card" aria-label="메일·일정 브리핑 요청 확인">
    <p>{describeConnectedBrief(intent)}</p>
    {pins && missing.length > 0 && <p role="status">{connectionGuidance(missing)}</p>}
    {pins && missing.length > 0 && onConnect && <button type="button" onClick={onConnect}>Google 연결 열기</button>}
    {error && <p role="alert">{error}</p>}
    <button disabled={busy || !pins || missing.length > 0} onClick={() => void grant()}>읽기 전용으로 허용하고 시작</button>
    <button disabled={busy} onClick={onCancel}>닫기</button>
  </section>;
}

type LogProps = { accountId: string; threadId: string; onExpired: () => void; refreshKey?: string };

/** The scheduled worklog for this thread: one entry per slot, counts always, content only while the grant is live. */
export function ConnectedBriefLog({ accountId, threadId, onExpired, refreshKey }: LogProps) {
  return <Log key={`${accountId}/${threadId}`} accountId={accountId} threadId={threadId} onExpired={onExpired} refreshKey={refreshKey} />;
}
function Log({ accountId, threadId, onExpired, refreshKey }: LogProps) {
  const scope = useScope(accountId, "member-connected-brief-log");
  const [items, setItems] = useState<BriefView[] | null>(null);
  const [error, setError] = useState(""), [busy, setBusy] = useState(false);
  const generation = useRef(0);
  const alive = useRef(true), stopIds = useRef(new Map<string, string>());
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  async function load() {
    const version = ++generation.current;
    try {
      const v = await scope.request<unknown>(BASE + "?thread_id=" + encodeURIComponent(threadId));
      if (!isBriefPage(v)) throw new Error("Invalid brief page");
      if (alive.current && version === generation.current) { setItems(v.items.filter(b => b.thread_id === threadId)); setError(""); }
    } catch (e) { const m = failure(e, onExpired); if (alive.current && version === generation.current) {
      setItems(null);
      if (m && !(e instanceof ApiFailure && e.status === 404)) setError("브리핑 기록을 불러오지 못했어요.");
    } }
  }
  useEffect(() => { void load(); const timer = setInterval(() => void load(), 30000); return () => clearInterval(timer); }, [scope, threadId, refreshKey]);
  async function stop(b: BriefView) {
    if (busy) return;
    setBusy(true);
    const request_id = stopIds.current.get(b.id) ?? crypto.randomUUID();
    stopIds.current.set(b.id, request_id);
    try { await scope.request(`${BASE}/${encodeURIComponent(b.id)}/stop`, "POST", { request_id, revision: b.revision }); stopIds.current.delete(b.id); await load(); }
    catch (e) { if (e instanceof ApiFailure && e.status >= 400 && e.status < 500) stopIds.current.delete(b.id); const m = failure(e, onExpired); if (m && alive.current) setError(m); }
    finally { if (alive.current) setBusy(false); }
  }
  if (!items) return error ? <p role="alert">{error}</p> : null;
  if (!items.length) return null;
  return <section className="member-briefs" aria-label="이 대화의 메일·일정 브리핑">
    {error && <p role="alert">{error}</p>}
    {items.map(b => <article className="member-brief" key={b.id}>
      <p className="member-brief-state">{STATE[b.state] ?? "상태 확인 필요"} · 읽기 전용 · {names(b.sources)}</p>
      <h3>매일 {b.at} ({b.timezone}) {b.important_only ? "중요 메일·일정 브리핑" : "메일·일정 브리핑"}</h3>
      {b.important_only && <p>{IMPORTANT_MEANING}</p>}
      {b.state === "active" && <p>{b.next_due && b.next_due < b.expires_at && <>다음 {time(b.next_due)} · </>}{time(b.expires_at)}까지</p>}
      {b.state === "revoked" && <p role="status">{connectionGuidance(b.sources)} 브리핑은 멈췄고, 이전에 읽은 내용은 지웠어요.</p>}
      <ul>{b.runs.map(r => <li key={r.due_at}>
        <p>{time(r.due_at)} · {r.state === "needs_connection" ? `${names((r.guidance ?? "").split(",").filter(Boolean))} 연결이 필요해 읽지 않았어요` : RUN[r.state] ?? r.state}
          {r.gmail_count !== null && ` · 메일 ${r.gmail_count}건`}{r.calendar_count !== null && ` · 일정 ${r.calendar_count}건`}
          {r.skipped > 0 && ` (앱이 꺼져 있던 ${r.skipped}번은 건너뜀)`}</p>
        {r.detail && <details open={r === b.runs[0]}>
          <summary>{r.freshness === "live" ? `${time(r.detail.captured_at)}에 읽은 내용` : "최신이 아닐 수 있는 저장된 내용"}</summary>
          {r.detail.events && <><h4>오늘 일정</h4>{r.detail.events.length ? <ul>{r.detail.events.map((e, i) => <li key={i}>{e.title || "(제목 없음)"} · {e.all_day ? "종일" : `${e.start} ~ ${e.end}`}</li>)}</ul> : <p>오늘 일정이 없어요.</p>}</>}
          {r.detail.messages && <><h4>{b.important_only ? "Gmail이 중요로 표시한 받은 메일" : "최근 받은 메일"}</h4>{r.detail.messages.length ? <ul>{r.detail.messages.map((m, i) => <li key={i}>{m.subject || "(제목 없음)"} — {m.from}</li>)}</ul> : <p>{b.important_only ? "새 중요 메일이 없어요." : "새 메일이 없어요."}</p>}</>}
        </details>}
      </li>)}</ul>
      {b.state === "active" && <button disabled={busy} onClick={() => void stop(b)}>멈추기</button>}
    </article>)}
  </section>;
}
