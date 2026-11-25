import { useEffect, useRef, useState } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { parseWatchTiming } from "./member-watch-timing";

// Member chat control for "watch one public status page and tell me only when it changes".
// Pure parser (no network/model) + a start handler + the in-thread worklog.
// Contract of /api/v1/personal/tasks/public-watches:
//   GET  ?thread_id=UUID           -> { items: PublicWatchView[] }
//   POST {thread_id, request_id, message, url, every_seconds, days} -> PublicWatchView
//   POST /{id}/stop {request_id, revision}                          -> PublicWatchView
// Times (`due_at`) are unix seconds, like the other task routes.

export type PublicWatchCheck = { due_at: number; state: string; excerpt?: string | null };
export type PublicWatchView = {
  id: string; thread_id: string; url: string; every_seconds: number; days: number; state: string; revision: number; checks: PublicWatchCheck[];
};
export type PublicWatchEvery = 1800 | 3600 | 86400;
export type PublicWatchIntent = { url: string; every_seconds: PublicWatchEvery; days: number; assumed: string[]; message: string };
export type PublicWatchRefusal = "unsafe_url" | "many_urls" | "unsupported_interval" | "unsupported_duration";
export type PublicWatchParse =
  | { ok: true; intent: PublicWatchIntent }
  | { ok: false; reason: PublicWatchRefusal; message: string };

const BASE = "/personal/tasks/public-watches";
const QUOTED = /[‘'“"「『](.+?)[’'”"」』]/g;
// ASCII only, so Korean particles glued to the address ("…/service를") are never part of it.
const URL_RE = /https?:\/\/[A-Za-z0-9\-._~:/?#[\]@!$&()*+,;=%]+/gi;
const CHANGE = /(바뀌|바뀔|변하|변경|달라지|변화)/;
const NOTIFY = /(알려|알림|통지|말해)/;
const DECLINED = /(?:하|주|보내|만들|시작하|켜|알리|읽|보|해주|멈추|취소하)지\s*(?:마|말|않)|(?:안\s*해도|필요\s*없|말고)/;
const QUESTION = /(\?|？|(?:나요|까요|ㄹ까|을까)\s*[.!]*$)/;
const ONE_OFF = /(한\s*번만|1\s*회만?|딱\s*한\s*번|지금\s*(?:바로\s*)?(?:한\s*번\s*)?(?:확인|봐))/;
const LOCAL_HOST = /(^|\.)(localhost|local|localdomain|internal|intranet|lan|home|corp|private|invalid|home\.arpa)$/;

const refuse = (reason: PublicWatchRefusal, message: string): PublicWatchParse => ({ ok: false, reason, message });

/** Why this address may not be watched, or null when it is a plain public http(s) address. */
export function unsafePublicUrl(raw: string): string | null {
  if (raw.length > 2048) return "주소가 너무 길어요.";
  let u: URL;
  try { u = new URL(raw); } catch { return "주소 형식이 올바르지 않아요."; }
  if (u.protocol !== "https:") return "HTTPS 주소만 확인할 수 있어요.";
  if (u.username || u.password) return "주소에 로그인 정보가 들어 있어요.";
  if (u.port) return "기본 포트가 아닌 주소는 확인하지 않아요.";
  const host = u.hostname.toLowerCase().replace(/\.$/, "");
  const labels = host.split(".");
  const tld = labels[labels.length - 1];
  if (host.startsWith("[") || labels.length < 2 || !/^[a-z][a-z0-9-]*$/.test(tld) || LOCAL_HOST.test(host)) return "내부·로컬 주소나 IP 주소는 확인하지 않아요.";
  if (u.search || u.hash) return "검색 매개변수와 조각 식별자를 뺀 공개 주소를 사용해 주세요.";
  if (u.pathname.split("/").some(s => s.length >= 32 && /^[A-Za-z0-9_-]+$/.test(s) && /\d/.test(s))) return "비공개 링크처럼 보이는 주소는 확인하지 않아요.";
  return null;
}

/**
 * "https://status.example.test/service 를 매시간 확인하고 바뀔 때만 알려줘". Needs, outside quotes: one exact
 * http(s) address and a recurring change request. Explicit future change conditions default to hourly. Anything else (one-off, negated, question,
 * quoted instruction) returns null so the composer sends it on as ordinary chat. A request that is explicit but
 * unsafe or unsupported returns a refusal: nothing is created.
 */
export function parsePublicWatchChat(raw: string): PublicWatchParse | null {
  const text = raw.normalize("NFC").trim();
  if (!text || text.length > 400) return null;
  const outside = text.replace(QUOTED, (_quote, contents: string) => /^https?:\/\/[^\s]+$/i.test(contents) ? contents : " ");
  if (DECLINED.test(outside) || QUESTION.test(outside.replace(URL_RE, " ")) || ONE_OFF.test(outside)) return null;
  if (!CHANGE.test(outside) || !NOTIFY.test(outside)) return null;
  const when = parseWatchTiming(outside.replace(URL_RE, " "));
  const ongoing = /(?:바뀌|변하|달라지)면|(?:바뀔|변할|달라질)\s*때|변경(?:되면|될\s*때)/.test(outside);
  if (!when.cadence && !ongoing) return null;
  const urls = [...new Set([...outside.matchAll(URL_RE)].map(m => m[0].replace(/[.,;:!?)\]]+$/, "")))];
  if (!urls.length) return null;
  if (urls.length > 1) return refuse("many_urls", "한 번에 공개 주소 하나만 감시할 수 있어요. 감시를 만들지 않았어요.");
  const unsafe = unsafePublicUrl(urls[0]);
  if (unsafe) return refuse("unsafe_url", `${unsafe} 로그인 없이 열리는 공개 주소만 확인해요. 감시를 만들지 않았어요.`);
  const stripped = outside.replace(URL_RE, " ");
  const conflict = !when.ok && when.conflict ? " 서로 다른 값이 함께 들어 있어요." : "";
  if (!when.ok && when.reason === "unsupported_interval" || when.ok && when.every_seconds === null && /(?:마다|간격)/.test(stripped)) return refuse("unsupported_interval", `확인 간격은 30분, 1시간, 하루만 지원해요.${conflict} 감시를 만들지 않았어요.`);
  if (!when.ok) return refuse("unsupported_duration", `감시 기간은 1일에서 90일까지만 정할 수 있어요.${conflict} 감시를 만들지 않았어요.`);
  const assumed = [];
  if (when.every_seconds === null) assumed.push("every_seconds");
  if (when.days === null) assumed.push("days");
  return { ok: true, intent: { url: urls[0], every_seconds: when.every_seconds ?? 3600, days: when.days ?? 7, assumed, message: text } };
}

const EVERY: Record<number, string> = { 1800: "30분", 3600: "1시간", 86400: "하루" };
export function describePublicWatch(intent: PublicWatchIntent): string {
  const defaults = [intent.assumed.includes("every_seconds") && "간격은 1시간", intent.assumed.includes("days") && "기간은 7일"].filter(Boolean);
  const assumed = defaults.length ? ` (${defaults.join(", ")}로 정했어요)` : "";
  return `${intent.url} 주소를 ${EVERY[intent.every_seconds]}마다 확인하고, 바뀐 경우에만 알려 드려요. ${intent.days}일 동안 이어져요.${assumed} `
    + "로그인 없이 열리는 공개 페이지만 읽고, 아무것도 보내거나 바꾸지 않아요.";
}
/** Reply for a refused explicit request (null when the text is not a public-watch request or was accepted). */
export const publicWatchChatReply = (parse: PublicWatchParse | null): string | null => parse && !parse.ok ? parse.message : null;

export function toPublicWatchRequest(intent: PublicWatchIntent, requestId: string, threadId: string) {
  return { thread_id: threadId, request_id: requestId, message: intent.message, url: intent.url, every_seconds: intent.every_seconds, days: intent.days };
}

const record = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);
const int = (v: unknown) => Number.isSafeInteger(v);
export function isPublicWatch(v: unknown): v is PublicWatchView {
  return record(v) && [v.id, v.thread_id, v.url, v.state].every(x => typeof x === "string") && int(v.every_seconds) && int(v.days) && int(v.revision)
    && Array.isArray(v.checks) && v.checks.length <= 100
    && v.checks.every(c => record(c) && int(c.due_at) && typeof c.state === "string" && (c.excerpt === undefined || c.excerpt === null || typeof c.excerpt === "string"));
}
export function isPublicWatchPage(v: unknown): v is { items: PublicWatchView[] } {
  return record(v) && Array.isArray(v.items) && v.items.length <= 50 && v.items.every(isPublicWatch);
}

function failure(e: unknown, onExpired: () => void): string | null {
  if (isAborted(e)) return null;
  if (e instanceof ApiFailure && e.status === 401) { onExpired(); return null; }
  if (e instanceof ApiFailure && e.status === 409) return "이미 같은 감시가 있거나 상태가 바뀌었어요. 목록을 확인해 주세요.";
  if (e instanceof ApiFailure && (e.status === 400 || e.status === 422)) return "이 주소는 감시할 수 없어요. 감시를 만들지 않았어요.";
  if (e instanceof ApiFailure && e.status === 429) return "잠시 후 다시 시도해 주세요.";
  return "요청을 완료하지 못했어요. 같은 요청이라 다시 시도해도 중복되지 않아요.";
}

type StartProps = { accountId: string; threadId: string; intent: PublicWatchIntent; requestId?: string; onDone?: (message: string) => void; onCancel?: () => void; onExpired: () => void };

/**
 * Handler for a parsed `ok` intent, mounted by the chat with the authenticated thread. An explicit command
 * needs no confirmation: it posts once on mount, keeps ONE request id for every retry of this command, and
 * reports the outcome in one line.
 */
export function PublicWatchStart(props: StartProps) {
  return <Start key={`${props.accountId}/${props.threadId}/${props.intent.url}/${props.requestId ?? ""}`} {...props} />;
}
function Start({ accountId, threadId, intent, requestId, onDone, onExpired, onCancel }: StartProps) {
  const scope = useScope(accountId, "member-public-watch-start");
  const [phase, setPhase] = useState<"busy" | "done" | "failed">("busy"), [error, setError] = useState("");
  const id = useRef<string | null>(null), alive = useRef(true), finished = useRef(false);
  async function send() {
    id.current ??= requestId ?? crypto.randomUUID();
    const body = toPublicWatchRequest(intent, id.current, threadId);
    setPhase("busy"); setError("");
    try {
      const result = await scope.request<unknown>(BASE, "POST", body);
      if (!isPublicWatch(result) || result.thread_id !== threadId || result.id !== id.current || new URL(result.url).href !== new URL(intent.url).href || result.every_seconds !== intent.every_seconds || result.days !== intent.days) throw new Error("Invalid public watch receipt");
      finished.current = true; id.current = null;
      if (alive.current) { setPhase("done"); onDone?.("시작했어요. 확인한 결과는 이 대화의 감시 기록에 남아요."); }
    } catch (e) {
      if (e instanceof ApiFailure && e.status >= 400 && e.status < 500 && e.status !== 408 && e.status !== 429) id.current = null;
      const m = failure(e, onExpired);
      if (alive.current && m) { setError(m); setPhase("failed"); }
    }
  }
  useEffect(() => { alive.current = true; if (!finished.current) void send(); return () => { alive.current = false; }; }, [scope]);
  return <section className="member-public-watch-start" aria-label="공개 페이지 감시 시작">
    {phase === "busy" && <p role="status">감시를 시작하는 중이에요.</p>}
    {phase === "done" && <p role="status">시작했어요. 확인한 결과는 이 대화의 감시 기록에 남아요.</p>}
    {phase === "failed" && <><p role="alert">{error}</p><button type="button" onClick={() => void send()}>다시 시도</button></>}
    {phase === "failed" && onCancel && <button type="button" onClick={onCancel}>닫기</button>}
  </section>;
}

const STATE: Record<string, string> = { active: "진행 중", stopped: "중지됨", expired: "기간 종료", revoked: "권한 철회" };
const CHECK: Record<string, string> = {
  baseline: "처음 상태를 저장했어요", unchanged: "바뀐 게 없어요", changed: "바뀌었어요", failed: "확인하지 못했어요",
  unavailable: "지금 읽을 수 없어 확인하지 못했어요. 바뀐 것으로 보지 않아요", missed: "시간이 지나 건너뜀", pending: "확인 중", claimed: "확인 중", unknown: "결과 확인 대기", withdrawn: "권한 철회",
};
const time = (v: number) => new Date(v * 1000).toLocaleString();
const clip = (s: string) => (s.length > 280 ? s.slice(0, 280) + "…" : s);

type LogProps = { accountId: string; threadId: string; onExpired: () => void; refreshKey?: string };

/** The worklog of this thread's public watches. Renders nothing when there are none. */
export function PublicWatchLog({ accountId, threadId, onExpired, refreshKey }: LogProps) {
  return <Log key={`${accountId}/${threadId}`} accountId={accountId} threadId={threadId} onExpired={onExpired} refreshKey={refreshKey} />;
}
function Log({ accountId, threadId, onExpired, refreshKey }: LogProps) {
  const scope = useScope(accountId, "member-public-watch-log");
  const [items, setItems] = useState<PublicWatchView[] | null>(null);
  const [error, setError] = useState(""), [busy, setBusy] = useState(false);
  const generation = useRef(0), alive = useRef(true), stopIds = useRef(new Map<string, string>());
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  async function load() {
    const version = ++generation.current;
    try {
      const v = await scope.request<unknown>(BASE + "?thread_id=" + encodeURIComponent(threadId));
      if (!isPublicWatchPage(v)) throw new Error("Invalid public watch page");
      if (alive.current && version === generation.current) { setItems(v.items.filter(w => w.thread_id === threadId)); setError(""); }
    } catch (e) {
      const m = failure(e, onExpired);
      if (alive.current && version === generation.current) {
        setItems(null);
        if (m && !(e instanceof ApiFailure && e.status === 404)) setError("감시 기록을 불러오지 못했어요.");
      }
    }
  }
  useEffect(() => { void load(); const timer = setInterval(() => void load(), 30000); return () => clearInterval(timer); }, [scope, threadId, refreshKey]);
  async function stop(w: PublicWatchView) {
    if (busy) return;
    setBusy(true);
    // One id per (watch, revision) until the server answers: a lost response never double-stops.
    const key = `${w.id}/${w.revision}`;
    const request_id = stopIds.current.get(key) ?? crypto.randomUUID();
    stopIds.current.set(key, request_id);
    try {
      const r = await scope.request<unknown>(`${BASE}/${encodeURIComponent(w.id)}/stop`, "POST", { request_id, revision: w.revision });
      stopIds.current.delete(key);
      if (isPublicWatch(r) && r.thread_id === threadId && alive.current) setItems(list => list && list.map(x => (x.id === r.id ? r : x)));
      await load();
    } catch (e) {
      if (e instanceof ApiFailure && e.status >= 400 && e.status < 500 && e.status !== 408 && e.status !== 429) stopIds.current.delete(key);
      const m = failure(e, onExpired); if (m && alive.current) setError(m);
    } finally { if (alive.current) setBusy(false); }
  }
  if (!items) return error ? <p role="alert">{error}</p> : null;
  if (!items.length) return null;
  return <section className="member-public-watches" aria-label="이 대화의 공개 페이지 감시">
    {error && <p role="alert">{error}</p>}
    {items.map(w => <article className="member-public-watch" key={w.id}>
      <p className="member-public-watch-state">{STATE[w.state] ?? "상태 확인 필요"} · 공개 페이지 · 바뀐 경우에만 알림</p>
      <h3>{w.url}</h3>
      <p>{EVERY[w.every_seconds] ?? `${w.every_seconds}초`}마다 확인 · {w.days}일 동안</p>
      {w.checks.length > 0 && <ul>{[...w.checks].sort((a, b) => b.due_at - a.due_at).map(c => <li key={c.due_at}>
        <p>{time(c.due_at)} · {CHECK[c.state] ?? "상태 확인 필요"}</p>
        {c.excerpt && (c.state === "changed" || c.state === "unchanged" || c.state === "baseline") && <p>읽은 내용: {clip(c.excerpt)}</p>}
      </li>)}</ul>}
      {w.state === "active" && <button disabled={busy} onClick={() => void stop(w)}>멈추기</button>}
    </article>)}
  </section>;
}
