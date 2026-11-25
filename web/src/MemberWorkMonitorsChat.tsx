// Natural Korean chat control for member monitors. Pure helpers: no network, no React state, no model.
// The composer calls parseMonitorChat(text); an `ok` intent is shown for confirmation (describeIntent)
// and only then sent through toMonitorRequest(...). Anything outside the straightforward cases returns a
// specific reason instead of a guess. Only the member's own memory and conversation records are
// supported: external sites, mail and calendar are refused here, not silently approximated.
import type { MonitorRequest, MonitorView } from "./MemberWorkMonitors";
import { parseWatchTiming, stripWatchTiming } from "./member-watch-timing";

export type MonitorScope = "memory" | "conversation";
export type MonitorIntent =
  | { kind: "watch"; scope: MonitorScope; query: string; every_seconds: 1800 | 3600 | 86400; days: number; assumed: string[] }
  | { kind: "brief"; scope: MonitorScope; query: string; at: string; days: number; assumed: string[] }
  | { kind: "stop"; target: string }
  | { kind: "list" };
export type MonitorRefusal =
  | "needs_query" | "needs_time" | "ambiguous_time" | "unsupported_source"
  | "unsupported_interval" | "unsupported_duration" | "connected_brief";
export type MonitorParse =
  | { ok: true; intent: MonitorIntent }
  | { ok: false; reason: "not_monitor" }
  | { ok: false; reason: MonitorRefusal; message: string };

const QUOTED = /[‘'“"「『](.+?)[’'”"」』]/g;
const EXTERNAL = /(https?:\/\/|www\.|웹\s?사이트|사이트|홈페이지|도메인|메일|이메일|gmail|지메일|캘린더|구글|드라이브|슬랙|slack|github|깃허브)/i;
const CLOCK = /(오전|아침|새벽|오후|저녁|밤)?\s*(\d{1,2})\s*(?:시(?!간)\s*(?:(\d{1,2})\s*분|(반))?|:(\d{2}))\s*에?/g;
const WATCH_VERB = /(바뀌|바뀔|변하|변경|달라지|변화|새로\s?(?:생기|추가|올라))/;
// A notification about a future change is recurring. Asking what already changed is ordinary chat.
const WHEN_CHANGED = /(?:바뀌|변하|달라지|변경되|추가되|생기|올라오)(?:면|는\s*대로)|(?:바뀔|변할|달라질|변경될|추가될|생길|올라올)\s*때|(?:변화|변경|변동)(?:\s*사항)?(?:가|이)?\s*(?:있으면|있을\s*때|시(?=\s|$))/;
const NOTIFY_AFTER_CONDITION = /^(?:마다|에만|만)?[\s,]*(?:(?:그때|바로|즉시|꼭|좀|나한테|저한테|나에게|저에게|내게)\s*)*(?:알려|알림|통지|말해)/;
const NOTIFY = /(알려|알림|통지|말해)/;
const DECLINED = /(?:하|주|보내|만들|시작하|켜|알리|읽|보|해주|멈추|취소하)지\s*(?:마|말|않)|(?:안\s*해도|필요\s*없)/;
const STOP = /(그만|중지|멈춰|멈추|해제|취소|중단|꺼\s?줘|끄)/;
// Plain reminders ("알림") are a different feature: only watches and briefings are controlled here.
const KIND = /(감시|모니터|브리핑)/;

const refuse = (reason: MonitorRefusal, message: string): MonitorParse => ({ ok: false, reason, message });
const unquoted = (text: string) => text.replace(QUOTED, " ");
const quotedText = (text: string) => { QUOTED.lastIndex = 0; const m = QUOTED.exec(text); QUOTED.lastIndex = 0; return m ? m[1].trim() : ""; };

/** Explicit civil clock only. A bare "9시" is ambiguous (오전/오후), exactly like the server's civil-time rule. */
function clock(text: string): { at: string } | "needs_time" | "ambiguous_time" {
  const found = [...unquoted(text).matchAll(CLOCK)];
  const normal = unquoted(text);
  if (/정오/.test(normal) && !found.length) return { at: "12:00" };
  if (/자정/.test(normal) && !found.length) return { at: "00:00" };
  if (!found.length) return "needs_time";
  if (found.length > 1) return "ambiguous_time";
  const [, marker, hourText, minuteText, half, colon] = found[0];
  let hour = Number(hourText);
  const minute = half ? 30 : minuteText ? Number(minuteText) : colon ? Number(colon) : 0;
  if (minute > 59 || hour > 23) return "ambiguous_time";
  if (marker) {
    if (hour < 1 || hour > 12) return "ambiguous_time";
    hour = hour % 12 + (/오후|저녁|밤/.test(marker) ? 12 : 0);
  } else if (colon === undefined && hour >= 1 && hour <= 12) {
    return "ambiguous_time";
  }
  return { at: `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}` };
}

function query(text: string): string {
  const quoted = quotedText(text);
  if (quoted) return quoted.slice(0, 128);
  let r = stripWatchTiming(text)
    .replace(CLOCK, " ")
    .replace(/(오전|아침|새벽|오후|저녁|밤|정오|자정)/g, " ")
    .replace(/동안/g, " ")
    .replace(/(변화가\s*있으면|바뀌었으면|바뀌면|바뀔\s*때|변하면|변경되면|달라지면|새로\s*생기면|새로\s*추가되면)/g, " ")
    .replace(/(알려\s*주세요|알려\s*줘|알려줘|알려|알림|지켜봐\s*줘|지켜봐|감시해\s*줘|감시|모니터링|브리핑해\s*줘|브리핑|요약해\s*줘|요약|정리해\s*줘|해\s*주세요|해\s*줘|해줘|주세요|줘)/g, " ")
    .replace(/(내\s*)?(기억|메모리|메모|대화\s*기록|대화|채팅)\s*(에서|의|이|가|을|를|은|는|에)?/g, " ");
  r = r.replace(/[.,!?~]+/g, " ").replace(/\s+/g, " ").trim();
  // Only the few particles that rarely end a noun, and only on a word long enough to survive the trim.
  const last = r.split(" ").pop() ?? "";
  if (last.length >= 3 && /(가|을|를|은|는)$/.test(last)) r = r.slice(0, -1);
  return r.length >= 1 && r.length <= 80 ? r : "";
}

// ---- Connected morning brief: live Gmail + Calendar, read only. Own parser; never a guess. ----
export type BriefSource = "gmail" | "calendar";
/** `important_only`: the member asked for important mail, meaning Gmail's own IMPORTANT label (never our guess). */
export type ConnectedBriefIntent = { sources: BriefSource[]; at: string; days: number; interval_days: 1; assumed: string[]; important_only: boolean };
export type ConnectedBriefParse =
  | { ok: true; intent: ConnectedBriefIntent }
  | { ok: false; reason: "ambiguous_time" | "unsupported_interval" | "unsupported_duration"; message: string };
const MAIL = /(메일|이메일|gmail|지메일)/i;
const CALENDAR = /(캘린더|일정)/;
// Anything about the member's own memory/conversation, or change watching, stays with the local monitors.
const LOCAL_ONLY = /(기억|메모|대화\s*기록|감시|모니터|바뀌|바뀔|변하|변경|달라지|새로\s?(?:생기|추가))/;

/**
 * "매일 아침 8시에 오늘 일정과 중요한 메일을 알려줘". Needs a daily cadence, an explicit clock and at least
 * one of mail/calendar outside quotes. Returns null for everything else (including a missing clock, which
 * keeps the existing monitor refusals unchanged).
 */
export function parseConnectedBriefChat(raw: string): ConnectedBriefParse | null {
  const text = raw.normalize("NFC").trim();
  if (!text || text.length > 400) return null;
  const outside = unquoted(text);
  if (!/(매일|하루에\s*한\s*번)/.test(outside) || !(NOTIFY.test(outside) || /(브리핑|요약|정리)/.test(outside))) return null;
  if (DECLINED.test(outside) || STOP.test(outside) || LOCAL_ONLY.test(outside) || EXTERNAL_NON_GOOGLE.test(outside)) return null;
  const sources: BriefSource[] = [];
  if (MAIL.test(outside)) sources.push("gmail");
  if (CALENDAR.test(outside)) sources.push("calendar");
  if (!sources.length) return null;
  const at = clock(text);
  if (at === "needs_time") return null;
  if (at === "ambiguous_time") return { ok: false, reason: "ambiguous_time", message: "오전인지 오후인지 알려 주세요. 예: 매일 아침 8시, 오후 6시 30분, 08:00" };
  const timing = parseWatchTiming(outside);
  if (!timing.ok) return { ok: false, reason: timing.reason, message: timing.reason === "unsupported_duration"
    ? "브리핑 기간을 1일에서 90일 사이로 하나만 정해 주세요." : "아침 브리핑은 하루 한 번 받을 수 있어요. 간격을 확인해 주세요." };
  const span = timing.days;
  // Only "중요한 메일 / 중요 메일 / 중요한 이메일" asks for important mail; other uses of 중요 do not switch the source.
  const important_only = sources.includes("gmail") && /중요(?:한|\s*표시된?)?\s*(?:메일|이메일|gmail|지메일)/i.test(outside);
  return { ok: true, intent: { sources, at: at.at, days: span ?? 7, interval_days: 1, assumed: span === null ? ["days"] : [], important_only } };
}
const EXTERNAL_NON_GOOGLE = /(https?:\/\/|www\.|웹\s?사이트|사이트|홈페이지|도메인|드라이브|슬랙|slack|github|깃허브)/i;

const SOURCE_LABEL: Record<BriefSource, string> = { gmail: "최근 받은 메일(제목·보낸 사람 최대 5건)", calendar: "오늘 일정(제목·시간)" };
const IMPORTANT_LABEL = "Gmail이 ‘중요’로 표시한 받은 메일(제목·보낸 사람 최대 5건)";
/** What “중요” means here, shown before the member grants anything. */
export const IMPORTANT_MEANING = "여기서 ‘중요’는 Gmail이 자체 판단으로 메일에 붙이는 ‘중요(IMPORTANT)’ 표시예요. 받은편지함에서 그 표시가 있는 메일만 읽고, 저는 중요도를 따로 판단하지 않아요.";
/** One honest sentence: what is read, how often, until when, and that nothing is ever written or sent. */
export function describeConnectedBrief(intent: ConnectedBriefIntent): string {
  const assumed = intent.assumed.length ? " (기간은 7일로 정했어요)" : "";
  const label = (s: BriefSource) => s === "gmail" && intent.important_only ? IMPORTANT_LABEL : SOURCE_LABEL[s];
  return `매일 ${intent.at}에 ${intent.sources.map(label).join("과 ")}을 읽어 알려 드려요. ${intent.days}일 동안 이어져요.${assumed} `
    + (intent.important_only ? `${IMPORTANT_MEANING} ` : "")
    + "읽기 전용이에요: 메일을 보내거나 일정을 만들고 바꾸지 않아요. 메일 중요도는 판단하지 않고 본문은 읽지 않아요. 연결이 끊기면 내용 없이 연결 안내만 드려요.";
}

export type ConnectedBriefPins = { gmail?: { connection_id: string; epoch: number }; calendar?: { connection_id: string; epoch: number } };
export type ConnectedBriefRequest = {
  request_id: string; thread_id: string; consent: true; gmail: ConnectedBriefPins["gmail"] | null; calendar: ConnectedBriefPins["calendar"] | null;
  at: string; timezone: string; interval_days: number; days: number; important_only: boolean;
};
/** Body for POST /personal/tasks/briefs. `consent` is only ever set by the member's button press. */
export function toConnectedBriefRequest(intent: ConnectedBriefIntent, requestId: string, threadId: string, timezone: string, pins: ConnectedBriefPins): ConnectedBriefRequest | null {
  const gmail = intent.sources.includes("gmail") ? pins.gmail ?? null : null;
  const calendar = intent.sources.includes("calendar") ? pins.calendar ?? null : null;
  if ((intent.sources.includes("gmail") && !gmail) || (intent.sources.includes("calendar") && !calendar)) return null;
  return { request_id: requestId, thread_id: threadId, consent: true, gmail, calendar, at: intent.at, timezone, interval_days: intent.interval_days, days: intent.days, important_only: gmail !== null && intent.important_only };
}

/** Straightforward Korean requests only. Returns `not_monitor` for ordinary chat so the composer sends it on. */
export function parseMonitorChat(raw: string): MonitorParse {
  // URLs are handled by the public-page or one-off browser route. A quoted web request is never a local memory watch.
  if (/https?:\/\//i.test(raw)) return { ok: false, reason: "not_monitor" };
  const text = raw.normalize("NFC").trim();
  if (!text || text.length > 400) return { ok: false, reason: "not_monitor" };
  const outside = unquoted(text);
  if (DECLINED.test(outside)) return { ok: false, reason: "not_monitor" };
  const stopping = STOP.test(outside) && KIND.test(outside);
  if (stopping) {
    const target = quotedText(text) || query(text.replace(STOP, " ").replace(/(그만|해제|취소|중단|중지)/g, " "));
    return { ok: true, intent: { kind: "stop", target } };
  }
  if (KIND.test(outside) && /(목록|리스트|현황|무엇|뭐\s*(?:가|있)|보여)/.test(outside) && !WATCH_VERB.test(outside) && !/매일/.test(outside)) {
    return { ok: true, intent: { kind: "list" } };
  }
  // Live mail/calendar briefs need a connection card, not a local monitor. Say so instead of refusing wrongly.
  const connected = parseConnectedBriefChat(text);
  if (connected) {
    return connected.ok
      ? refuse("connected_brief", "메일·캘린더 브리핑은 Google 연결과 읽기 허용을 확인하는 카드로 설정해요. 읽기 전용이며 메일을 보내거나 일정을 바꾸지 않아요.")
      : refuse(connected.reason, connected.message);
  }
  const brief = (/브리핑/.test(outside) || /요약/.test(outside) && NOTIFY.test(outside)) && /(매일|하루에\s*한\s*번)/.test(outside);
  const when = parseWatchTiming(outside);
  const condition = WHEN_CHANGED.exec(outside);
  const notifyOnChange = condition && NOTIFY_AFTER_CONDITION.test(outside.slice(condition.index + condition[0].length));
  const periodicChange = when.cadence && WATCH_VERB.test(outside) && NOTIFY.test(outside);
  const watch = !brief && (notifyOnChange || periodicChange || /(감시|지켜봐|모니터)/.test(outside));
  if (!brief && !watch) return { ok: false, reason: "not_monitor" };
  if (EXTERNAL.test(outside)) {
    return refuse("unsupported_source", "지금은 내 기억과 대화 기록의 변화만 확인할 수 있어요. 외부 사이트, 메일, 캘린더는 읽지 않아요.");
  }
  const scope: MonitorScope = /(대화|채팅)/.test(outside) && !/기억|메모/.test(outside) ? "conversation" : "memory";
  const q = query(text);
  if (!q) return refuse("needs_query", brief ? "어떤 내용을 브리핑할까요? 예: ‘오늘 일정’ 브리핑" : "어떤 내용의 변화를 볼까요? 예: ‘서비스 상태’ 기억이 바뀌면 알려줘");
  if (!when.ok) {
    const conflict = when.conflict ? " 서로 다른 값이 함께 들어 있어요. 하나만 말해 주세요." : "";
    return when.reason === "unsupported_interval"
      ? refuse("unsupported_interval", `변화 감시는 30분, 1시간, 하루 간격만 지원해요.${conflict}`)
      : refuse("unsupported_duration", `감시 기간은 1일에서 90일까지만 정할 수 있어요.${conflict}`);
  }
  const span = when.days;
  const every = when.every_seconds;
  const assumed: string[] = [];
  let days = 7;
  if (span === null) assumed.push("days"); else days = span;
  if (brief) {
    const at = clock(text);
    if (at === "needs_time") return refuse("needs_time", "몇 시에 받을까요? 예: 매일 아침 9시에");
    if (at === "ambiguous_time") return refuse("ambiguous_time", "오전인지 오후인지 알려 주세요. 예: 오전 9시, 오후 6시 30분, 09:00");
    return { ok: true, intent: { kind: "brief", scope, query: q, at: at.at, days, assumed } };
  }
  if (/\d\s*시(?!간)|\d:\d{2}/.test(outside)) {
    return refuse("unsupported_interval", "변화 감시는 30분, 1시간, 하루 간격만 지원해요. 정해진 시각에 받으려면 ‘매일 아침 9시에 ○○ 브리핑’이라고 말해 주세요.");
  }
  if (every === null) assumed.push("every_seconds");
  return { ok: true, intent: { kind: "watch", scope, query: q, every_seconds: every ?? 3600, days, assumed } };
}

const EVERY: Record<number, string> = { 1800: "30분", 3600: "1시간", 86400: "하루" };
const SCOPE: Record<MonitorScope, string> = { memory: "내 기억", conversation: "내 대화 기록" };

/** One confirmation sentence that states what will actually be read and what was assumed. */
export function describeIntent(intent: MonitorIntent): string {
  if (intent.kind === "list") return "현재 감시와 브리핑을 보여 드릴게요.";
  if (intent.kind === "stop") return intent.target ? `‘${intent.target}’ 감시를 멈출까요?` : "감시를 멈출까요?";
  const assumed = intent.assumed.length ? ` (${intent.assumed.map(a => a === "days" ? "기간은 7일" : "간격은 1시간").join(", ")}로 정했어요)` : "";
  const reads = `${SCOPE[intent.scope]}에서 ‘${intent.query}’`;
  return intent.kind === "watch"
    ? `${EVERY[intent.every_seconds]}마다 ${reads} 자료를 확인하고, 바뀐 경우에만 알려 드려요. ${intent.days}일 동안 이어져요.${assumed} 외부 사이트, 메일, 캘린더는 읽지 않아요.`
    : `매일 ${intent.at}에 ${reads} 관련 자료를 요약해 알려 드려요. ${intent.days}일 동안 이어져요.${assumed} 메일과 캘린더는 읽지 않아요.`;
}

export function monitorChatReply(parse: MonitorParse): string | null {
  return parse.ok || parse.reason === "not_monitor" ? null : parse.message;
}

/** Browser-supplied civil zone: IANA name, or a fixed UTC offset when the runtime cannot name one. */
export function browserTimezone(): string {
  try {
    const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    if (zone && zone.length <= 64) return zone;
  } catch { /* fall through to the offset */ }
  return browserOffset();
}
export function browserOffset(date = new Date()): string {
  const minutes = -date.getTimezoneOffset();
  const abs = Math.abs(minutes);
  return `${minutes < 0 ? "-" : "+"}${String(Math.floor(abs / 60)).padStart(2, "0")}:${String(abs % 60).padStart(2, "0")}`;
}

/** The body for POST /personal/tasks/monitors. Keep `requestId` until the request is acknowledged. */
export function toMonitorRequest(intent: Extract<MonitorIntent, { kind: "watch" | "brief" }>, requestId: string, timezone: string): MonitorRequest;
export function toMonitorRequest(intent: MonitorIntent, requestId: string, timezone: string): MonitorRequest | null;
export function toMonitorRequest(intent: MonitorIntent, requestId: string, timezone: string): MonitorRequest | null {
  if (intent.kind === "watch") {
    return { request_id: requestId, kind: "watch", title: `${intent.query} 변화 감시`.slice(0, 240), scope: intent.scope, query: intent.query, every_seconds: intent.every_seconds, days: intent.days };
  }
  if (intent.kind === "brief") {
    return { request_id: requestId, kind: "brief", title: `${intent.query} 브리핑`.slice(0, 240), scope: intent.scope, query: intent.query, at: intent.at, timezone, days: intent.days };
  }
  return null;
}

export type StopResolution =
  | { found: "one"; id: string; revision: number }
  | { found: "none" }
  | { found: "many"; titles: string[] };

/** Matches a spoken target against the member's active monitors; never guesses between several. */
export function resolveStopTarget(target: string, monitors: MonitorView[]): StopResolution {
  const live = monitors.filter(m => m.state === "active");
  const wanted = target.trim();
  const named = wanted ? live.filter(m => m.title === wanted) : [];
  const hits = named.length ? named : wanted ? live.filter(m => m.title.includes(wanted) || m.query.includes(wanted) || wanted.includes(m.query)) : live;
  if (hits.length === 1) return { found: "one", id: hits[0].id, revision: hits[0].revision };
  return hits.length ? { found: "many", titles: hits.map(m => m.title) } : { found: "none" };
}
