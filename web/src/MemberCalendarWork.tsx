import { useEffect, useRef, useState } from "react";
import { ApiFailure, errorText } from "./api";

/** Member Calendar: one primary timed event create or move, no invitations.
 * The ordinary Korean request is parsed here, deterministically, into civil values;
 * the server resolves the zone, lists candidates, and only writes after the member
 * approves the exact final content shown in the preview. */
const base = "/personal/google/calendar";

export type CalendarRequestInput = {
  kind: "create" | "move";
  title: string;
  date: string;
  time: string;
  time_zone: string;
  duration_minutes?: number;
  from_date?: string;
};
export type CalendarParse =
  | ({ ok: true } & CalendarRequestInput)
  | { ok: false; reason: string; message: string };

type Candidate = { candidate_ref: string; title: string; start: string; end: string; time_zone: string | null };
type Preview = {
  kind: "create" | "move";
  digest: string;
  title: string;
  start: string;
  end: string;
  time_zone: string;
  current: Candidate | null;
};
type Prepared =
  | { state: "needs_connection"; reason: string }
  | { state: "needs_selection"; candidates: Candidate[]; partial: boolean }
  | { state: "not_found"; partial: boolean }
  | { state: "stale" }
  | { state: "preview"; preview: Preview };
type Receipt =
  | { state: "needs_connection"; reason: string }
  | { state: "in_progress" }
  | {
      state: "confirmed";
      confirmed_by: "provider_response" | "readback";
      receipt: "created" | "moved";
      event: { id: string; title: string; start: string; end: string; time_zone: string | null };
    }
  | { state: "conflict" }
  | { state: "failed"; code: string }
  | { state: "unknown"; code: string }
  | { state: "withdrawn" };
type Scope = { request<T>(path: string, method?: string, body?: unknown): Promise<T> };

const ZONES: [RegExp, string][] = [
  [/서울\s*시간|한국\s*시간|\bKST\b/, "Asia/Seoul"],
  [/도쿄\s*시간|일본\s*시간|\bJST\b/, "Asia/Tokyo"],
  [/뉴욕\s*시간|동부\s*시간|\b(?:EST|EDT)\b/, "America/New_York"],
  [/(?:\bLA|로스앤젤레스|캘리포니아)\s*시간|\b(?:PST|PDT)\b/, "America/Los_Angeles"],
  [/런던\s*시간|영국\s*시간/, "Europe/London"],
  [/\bUTC\b|세계\s*표준시/, "UTC"],
];
const CREATE = /(잡아|추가|만들|등록|넣어|생성|예약)/;
const MOVE = /(옮겨|옮기|이동|변경|바꿔|바꾸|수정|미뤄|미루|당겨|당기)/;
const UNSUPPORTED = /(초대|참석자|게스트|삭제|취소|지워|반복|매주|매일|매월|종일|하루\s*종일)/;
const WEEKDAYS = ["일", "월", "화", "수", "목", "금", "토"];

function zoneOk(z: string) {
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: z });
    return true;
  } catch {
    return false;
  }
}
function civilToday(now: Date, zone: string) {
  return new Intl.DateTimeFormat("en-CA", { timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit" }).format(now);
}
function addDays(date: string, n: number) {
  const d = new Date(`${date}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}
function validDate(y: number, m: number, d: number) {
  const x = new Date(Date.UTC(y, m - 1, d));
  return x.getUTCFullYear() === y && x.getUTCMonth() === m - 1 && x.getUTCDate() === d;
}
const pad = (n: number) => String(n).padStart(2, "0");

type Hit = { index: number; length: number; date: string };
/** Every explicit date expression, left to right. */
function dates(text: string, today: string): Hit[] {
  const hits: Hit[] = [];
  const [ty, tm, td] = today.split("-").map(Number);
  const weekday = new Date(`${today}T00:00:00Z`).getUTCDay();
  const add = (m: RegExpMatchArray, date: string | null) => {
    if (date) hits.push({ index: m.index ?? 0, length: m[0].length, date });
  };
  for (const m of text.matchAll(/(\d{4})-(\d{2})-(\d{2})/g))
    add(m, validDate(+m[1], +m[2], +m[3]) ? m[0] : null);
  for (const m of text.matchAll(/(?:(\d{4})년\s*)?(\d{1,2})월\s*(\d{1,2})일/g)) {
    let y = m[1] ? +m[1] : ty;
    if (!m[1] && (+m[2] < tm || (+m[2] === tm && +m[3] < td))) y += 1;
    add(m, validDate(y, +m[2], +m[3]) ? `${y}-${pad(+m[2])}-${pad(+m[3])}` : null);
  }
  for (const [word, n] of [["오늘", 0], ["내일", 1], ["모레", 2], ["글피", 3]] as const)
    for (const m of text.matchAll(new RegExp(word, "g"))) add(m, addDays(today, n));
  for (const m of text.matchAll(/(이번|다음)\s*주\s*([일월화수목금토])요일/g)) {
    const target = WEEKDAYS.indexOf(m[2]);
    const monday = addDays(today, -((weekday + 6) % 7));
    add(m, addDays(monday, ((target + 6) % 7) + (m[1] === "다음" ? 7 : 0)));
  }
  for (const m of text.matchAll(/(?<!주\s*)(?<![이번다음]\s*주\s*)([일월화수목금토])요일/g)) {
    const ahead = ((WEEKDAYS.indexOf(m[1]) - weekday + 6) % 7) + 1;
    add(m, addDays(today, ahead));
  }
  return hits.sort((a, b) => a.index - b.index);
}
type Clock = { index: number; length: number; hour: number; minute: number; meridiem: boolean };
function clocks(text: string): Clock[] {
  const out: Clock[] = [];
  for (const m of text.matchAll(/(오전|오후|아침|저녁|밤|새벽)?\s*(\d{1,2})\s*시(?!간)(?:\s*(\d{1,2})\s*분|\s*(반))?/g)) {
    let hour = +m[2];
    const minute = m[4] ? 30 : m[3] ? +m[3] : 0;
    const pm = m[1] && /오후|저녁|밤/.test(m[1]);
    if (m[1]) {
      if (pm && hour < 12) hour += 12;
      if (!pm && hour === 12) hour = 0;
    }
    // "밤 12시" is midnight of an unstated day: ask instead of choosing one.
    const clear = (!!m[1] && !(m[1] === "밤" && +m[2] === 12)) || hour >= 13 || hour === 0;
    out.push({ index: m.index ?? 0, length: m[0].length, hour, minute, meridiem: clear });
  }
  for (const m of text.matchAll(/(?<!\d)([01]?\d|2[0-3]):([0-5]\d)(?!\d)/g))
    out.push({ index: m.index ?? 0, length: m[0].length, hour: +m[1], minute: +m[2], meridiem: true });
  return out.sort((a, b) => a.index - b.index);
}
function stripSpans(text: string, spans: { index: number; length: number }[]) {
  let out = text;
  for (const s of [...spans].sort((a, b) => b.index - a.index))
    out = out.slice(0, s.index) + "|" + out.slice(s.index + s.length);
  return out;
}

/** Parse one ordinary Korean calendar request. Never guesses a missing date,
 * a bare "3시" without 오전/오후, a create duration, or a title. `null` = not a
 * calendar request at all. */
export function parseKoreanCalendarRequest(text: string, now: Date, browserZone: string): CalendarParse | null {
  const t = text.trim();
  const wantsMove = MOVE.test(t);
  const wantsCreate = CREATE.test(t);
  const fail = (reason: string, message: string): CalendarParse => ({ ok: false, reason, message });
  const calendarish = wantsMove || wantsCreate || /(일정|캘린더|회의|미팅|약속|예약)/.test(t);
  if (t && calendarish && UNSUPPORTED.test(t))
    return fail("unsupported", "초대, 삭제, 반복, 종일 일정은 지원하지 않습니다. 시간이 정해진 일정 하나를 만들거나 옮길 수 있습니다.");
  if (!t || (!wantsMove && !wantsCreate)) return null;
  // "잡아둔 일정을 옮겨줘" names both verbs: the move wins; it never creates.
  const kind = wantsMove ? "move" : "create";
  let zone = browserZone;
  const iana = t.match(/\b([A-Z][A-Za-z_]+\/[A-Za-z_+-]+)\b/);
  let zoneSpan: { index: number; length: number }[] = [];
  if (iana) {
    if (!zoneOk(iana[1])) return fail("zone", "알 수 없는 시간대입니다.");
    zone = iana[1];
    zoneSpan = [{ index: iana.index ?? 0, length: iana[0].length }];
  } else {
    for (const [re, z] of ZONES) {
      const m = t.match(re);
      if (m) {
        zone = z;
        zoneSpan = [{ index: m.index ?? 0, length: m[0].length }];
        break;
      }
    }
  }
  if (!zoneOk(zone)) return fail("zone", "시간대를 확인할 수 없습니다.");
  const today = civilToday(now, zone);
  const ds = dates(t, today);
  const cs = clocks(t);
  if (!cs.length) return fail("time", "시작 시각을 알려 주세요. 예: 오후 3시");
  const start = cs[0];
  if (!start.meridiem) return fail("meridiem", "오전인지 오후인지 알려 주세요. 예: 오후 3시");
  // Target date: the last date for a move ("<from> ... <to>로 옮겨"), the only one otherwise.
  if (!ds.length) return fail("date", "날짜를 알려 주세요. 예: 내일, 12월 3일");
  const target = ds[ds.length - 1].date;
  const from = kind === "move" && ds.length > 1 ? ds[0].date : undefined;
  // Duration: explicit end clock, or an explicit length. A create never gets a default.
  let duration: number | undefined;
  const spans: { index: number; length: number }[] = [...zoneSpan, ...ds, ...cs];
  if (cs.length > 1) {
    const end = cs[1];
    let endHour = end.hour;
    if (!end.meridiem && start.hour >= 12 && endHour < 12) endHour += 12;
    const minutes = endHour * 60 + end.minute - (start.hour * 60 + start.minute);
    if (minutes <= 0) return fail("end_before_start", "끝나는 시각이 시작 시각보다 늦어야 합니다.");
    duration = minutes;
  }
  if (duration === undefined) {
    for (const len of t.matchAll(/(\d+)\s*시간(?:\s*(\d+)\s*분|\s*(반))?|(\d+)\s*분(?!\s*(?:전|후))/g)) {
      const at = len.index ?? 0;
      // "3시 30분" is a clock time, not a 30-minute length.
      if (cs.some((c) => at < c.index + c.length && c.index < at + len[0].length)) continue;
      duration = len[4] ? +len[4] : +len[1] * 60 + (len[3] ? 30 : len[2] ? +len[2] : 0);
      spans.push({ index: at, length: len[0].length });
      break;
    }
  }
  if (duration !== undefined && (duration < 1 || duration > 1440)) return fail("duration", "일정 길이는 1분에서 24시간 사이여야 합니다.");
  if (kind === "create" && duration === undefined) return fail("duration", "일정 길이나 끝나는 시각을 알려 주세요. 예: 1시간, 4시까지");
  // Title: a quoted phrase wins; otherwise the longest leftover segment before the verb.
  let title = "";
  const quoted = t.match(/["'“‘「]([^"'”’」]{1,200})["'”’」]/);
  if (quoted) title = quoted[1].trim();
  else {
    const verb = t.search(wantsMove ? MOVE : CREATE);
    const head = stripSpans(t.slice(0, verb < 0 ? t.length : verb), spans.filter((s) => s.index < (verb < 0 ? t.length : verb)));
    title = head
      .split("|")
      .map((s) => s.replace(/^\s*(에서|에|부터|까지|의|으로|로)\s+/, "").replace(/(을|를)\s*$/, "").replace(/\s*(일정|캘린더|구글\s*캘린더)\s*$/, "").trim())
      .filter((s) => s && !/^(에서|에|부터|까지|으로|로|일정|캘린더|구글\s*캘린더)$/.test(s))
      .sort((a, b) => b.length - a.length)[0] ?? "";
  }
  if (!title) return fail("title", "일정 제목을 알려 주세요. 제목은 따옴표로 감싸면 정확합니다.");
  return {
    ok: true,
    kind,
    title,
    date: target,
    time: `${pad(start.hour)}:${pad(start.minute)}`,
    time_zone: zone,
    ...(duration !== undefined ? { duration_minutes: duration } : {}),
    ...(from ? { from_date: from } : {}),
  };
}

function when(start: string, end: string, zone: string | null) {
  const f = (v: string) => v.replace("T", " ").replace(/(:\d{2})?([+-]\d{2}:\d{2}|Z)$/, "");
  return `${f(start)} – ${f(end).slice(11)}${zone ? ` (${zone})` : ""}`;
}
/** The route does not exist on this installation (feature not installed), as opposed to a failed attempt. */
const unavailable = (e: unknown) => e instanceof ApiFailure && (e.status === 404 || e.status === 501);
function failureText(e: unknown) {
  if (unavailable(e)) return "이 설치에서는 Google 캘린더 기능을 사용할 수 없습니다. 입력한 내용은 지우지 않고 남겨 두었습니다. 관리자에게 문의해 주세요.";
  if (e instanceof ApiFailure && e.code === "approval_mismatch") return "승인한 내용과 달라졌습니다. 미리보기를 다시 만들어 주세요.";
  if (e instanceof ApiFailure && e.status === 409) return "연결이나 일정 상태가 바뀌었습니다. 다시 시도해 주세요.";
  if (e instanceof ApiFailure && e.status === 502) return "Google 캘린더에 연결하지 못했습니다. 잠시 후 다시 시도해 주세요.";
  return errorText(e);
}

export function MemberCalendarWork(props: {
  scope: Scope;
  /** Original chat text. Calendar details never go to a model or into chat memory. */
  text: string;
  connection: { connection_id: string; epoch: number } | null;
  /** Opens the existing Google consent UI for the writable calendar scope. */
  onConnect: () => void;
  now?: Date;
  zone?: string;
  requestId?: string;
  /** The original conversation: sent with prepare and apply so the server links the work into that thread's history. */
  threadId: string;
  /** The write reached a final answer from Google (confirmed, conflict, refused) or the connection withdrew it. */
  onDone?: (state: "confirmed" | "conflict" | "failed" | "withdrawn") => void;
  /** The installation has no calendar routes; the caller keeps the member's text. */
  onUnavailable?: () => void;
}) {
  const { scope, connection } = props;
  const parsed = useRef(
    parseKoreanCalendarRequest(
      props.text,
      props.now ?? new Date(),
      props.zone ?? Intl.DateTimeFormat().resolvedOptions().timeZone,
    ),
  ).current;
  const [requestId] = useState(() => props.requestId ?? crypto.randomUUID());
  const [candidate, setCandidate] = useState<string | undefined>();
  const [prepared, setPrepared] = useState<Prepared | null>(null);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);

  function body(c = candidate) {
    if (!parsed?.ok || !connection) return null;
    const { ok: _ok, ...fields } = parsed;
    return { request_id: requestId, thread_id: props.threadId, connection_id: connection.connection_id, epoch: connection.epoch, ...fields, ...(c ? { candidate_ref: c } : {}) };
  }
  async function prepare(c?: string) {
    const b = body(c);
    if (!b || lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      const next = await scope.request<Prepared>(`${base}/prepare`, "POST", b);
      setPrepared(next);
      if (c) setCandidate(c);
    } catch (e) {
      setError(failureText(e));
      if (unavailable(e)) props.onUnavailable?.();
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  async function apply() {
    if (prepared?.state !== "preview") return;
    // A lone match is resolved server-side; the preview carries its handle.
    const b = body(prepared.preview.current?.candidate_ref ?? candidate);
    if (!b || lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      const done = await scope.request<Receipt>(`${base}/apply`, "POST", { request: b, approved_digest: prepared.preview.digest });
      setReceipt(done);
      if (done.state === "confirmed" || done.state === "conflict" || done.state === "failed" || done.state === "withdrawn") props.onDone?.(done.state);
    } catch (e) {
      // The same request id + digest is safe to retry: the server never re-sends.
      setError(failureText(e));
      if (unavailable(e)) props.onUnavailable?.();
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  const started = useRef(false);
  useEffect(() => {
    if (started.current || !parsed?.ok || !connection) return;
    started.current = true;
    void prepare();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!parsed) return null;
  if (!parsed.ok) return <section className="member-calendar-work" role="status"><p>{parsed.message}</p></section>;
  const needsConnect = !connection || prepared?.state === "needs_connection" || receipt?.state === "needs_connection";
  return (
    <section className="member-calendar-work" aria-label="캘린더 일정" aria-busy={busy}>
      <h3>{parsed.kind === "create" ? "일정 만들기" : "일정 옮기기"}</h3>
      {needsConnect && (
        <div role="alert">
          <p>
            Google 캘린더 연결이 필요합니다. 내 캘린더의 일정만 보고 만들고 수정할 수 있는 권한을 요청하며, 초대는 보내지 않습니다.
          </p>
          <button type="button" onClick={props.onConnect}>Google 캘린더 연결</button>
        </div>
      )}
      {!needsConnect && prepared?.state === "not_found" && (
        <p role="status">같은 제목의 일정을 찾지 못했습니다.{prepared.partial ? " 일정이 많아 일부만 확인했습니다. 날짜를 지정해 보세요." : ""}</p>
      )}
      {!needsConnect && prepared?.state === "stale" && (
        <div role="status">
          <p>선택한 일정이 그 사이 바뀌었습니다. 다시 확인해 주세요.</p>
          <button type="button" disabled={busy} onClick={() => { setCandidate(undefined); void prepare(undefined); }}>다시 찾기</button>
        </div>
      )}
      {!needsConnect && prepared?.state === "needs_selection" && !receipt && (
        <fieldset disabled={busy}>
          <legend>같은 제목의 일정이 여러 개입니다. 옮길 일정을 직접 선택하세요.{prepared.partial ? " (일부만 표시됨)" : ""}</legend>
          {prepared.candidates.map((c) => (
            <label key={c.candidate_ref}>
              <input type="radio" name="candidate" onChange={() => void prepare(c.candidate_ref)} />
              {c.title} · {when(c.start, c.end, c.time_zone)}
            </label>
          ))}
        </fieldset>
      )}
      {!needsConnect && prepared?.state === "preview" && !receipt && (
        <div>
          <p role="status"><strong>미리보기</strong> · 아직 Google 캘린더에 저장되지 않았습니다.</p>
          <p>{prepared.preview.title}</p>
          {prepared.preview.current && <p>현재: {when(prepared.preview.current.start, prepared.preview.current.end, prepared.preview.current.time_zone)}</p>}
          <p>{prepared.preview.current ? "변경 후" : "시간"}: {when(prepared.preview.start, prepared.preview.end, prepared.preview.time_zone)}</p>
          <button type="button" disabled={busy} onClick={() => void apply()}>
            {prepared.preview.kind === "create" ? "Google 캘린더에 저장" : "일정 옮기기"}
          </button>
        </div>
      )}
      {receipt?.state === "confirmed" && (
        <p role="status">
          <strong>Google 캘린더에서 확인됨</strong> · {receipt.receipt === "created" ? "만들었습니다" : "옮겼습니다"}: {receipt.event.title} · {when(receipt.event.start, receipt.event.end, receipt.event.time_zone)}
          <br />
          {receipt.confirmed_by === "provider_response" ? "Google의 응답으로 확인했습니다." : "응답이 유실되어 일정을 다시 읽어 확인했습니다. 중복 생성되지 않았습니다."}
        </p>
      )}
      {receipt?.state === "conflict" && <p role="status">다른 곳에서 일정이 바뀌어 옮기지 않았습니다. 변경된 내용은 없습니다.</p>}
      {receipt?.state === "in_progress" && <p role="status">저장 중입니다. 잠시 후 같은 요청을 다시 확인해 주세요.</p>}
      {receipt?.state === "withdrawn" && <p role="status">연결이 바뀌어 이 결과를 제거했습니다.</p>}
      {receipt?.state === "failed" && <p role="alert">Google이 요청을 처리하지 못했습니다 ({receipt.code}). 저장되지 않았습니다.</p>}
      {receipt?.state === "unknown" && (
        <div role="alert">
          <p>저장 결과를 아직 확인하지 못했습니다. 같은 요청을 다시 확인해도 일정이 중복 생성되지 않습니다.</p>
          <button type="button" disabled={busy} onClick={() => void apply()}>다시 확인</button>
        </div>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
