import { parseKoreanCalendarRequest, type CalendarParse } from "./MemberCalendarWork";
import type { Criteria, OutcomeKind } from "./MemberResearchOutcomes";
import { parseWorkItems } from "./conversation-work";
import { parsePublicWatchChat } from "./MemberPublicWatch";
import { parseConnectedBriefChat, parseMonitorChat } from "./MemberWorkMonitorsChat";
import { personalResearchIntent } from "./PersonalWorkflows";

/* ---- everyday member requests typed into the ordinary composer ----------------------------------
 * Deterministic gates, not a router: each recognises only an explicit, unnegated, unquoted request and
 * otherwise leaves the text to ordinary chat / the earlier work paths. Pure: no network, no storage. */
export type MemberRoute =
  | { kind: "calendar"; parsed: CalendarParse }
  | { kind: "mail"; to: string; body: string }
  | { kind: "research"; outcome: OutcomeKind; criteria: Criteria };

const NEGATED = /(지\s*(?:마(?![가-힣])|말|않)|지는\s*마|말아\s*(?:줘|주세요)|말고|마세요|마라(?![가-힣])|안\s*해도|않아도|필요\s*없|\bdo\s*not\b|\bdon'?t\b|\bnever\b)/i;
const QUOTED = /"[^"\n]*"|“[^”\n]*”|‘[^’\n]*’|「[^」\n]*」|'[^'\n]*'/g;
// quoted text | sentence end (a dot only before whitespace, so addresses and URLs stay whole) | anything else
const TOKEN = /("[^"\n]*"|“[^”]*”|‘[^’]*’|「[^」]*」|'[^'\n]*')|((?:[!?。！？\n]|\.(?=\s|$))+)|([^!?。！？\n"“‘「'.]+|["“‘「'.])/gu;
/** Sentences, never split inside a quotation: a quoted body such as “…드릴게요.” stays in its clause. */
function clauses(text: string): string[] {
  const out: string[] = [];
  let current = "";
  for (const token of text.matchAll(TOKEN)) {
    if (token[2]) { if (current.trim()) out.push(current.trim()); current = ""; }
    else current += token[1] ?? token[3];
  }
  if (current.trim()) out.push(current.trim());
  return out;
}
const unquoted = (text: string): string => text.replace(QUOTED, " ");

// Asking how something is done, or what a phrase means, is a question about the work, not the work.
const HOW_TO = /(어떻게|어떡|방법|하는\s*법|쓰는\s*법|하는지|뜻이|무슨\s*뜻|의미|\bhow\b|\bwhat\s+does\b)/i;

const STRONG_VERB = /(잡아|옮겨|옮기|미뤄|미루|당겨|당기)/;
const WEAK_VERB = /(추가|만들|등록|넣어|생성|변경|바꿔|바꾸|수정|이동|예약해)/;
const CAL_NOUN = /(일정|캘린더|달력|스케줄|회의(?!록|실)|미팅|약속|예약)/;
const TIME_HINT = /(\d\s*시|오전|오후|저녁|아침|새벽|\d{1,2}:\d{2}|오늘|내일|모레|글피|요일|\d{1,2}월\s*\d{1,2}일|\d{4}-\d{2}-\d{2})/;
/** An actual request to create or move a calendar event: a calendar verb outside quotes, not negated. */
export function explicitCalendarRequest(text: string): boolean {
  let explicit = false;
  for (const clause of clauses(text)) {
    const open = unquoted(clause);
    const strong = STRONG_VERB.test(open), weak = WEAK_VERB.test(open);
    if (!strong && !weak) continue;
    if (NEGATED.test(open)) return false; // "달력 등록은 하지 마" cancels the whole message
    if (HOW_TO.test(open)) continue;
    if (CAL_NOUN.test(clause) || (strong && TIME_HINT.test(clause))) explicit = true;
  }
  return explicit;
}
const MAIL_NOUN = /(메일|이메일|e-?mail|지메일|gmail)/i;
const REPLY = /(답장|회신|답신|\breply\b)/i;
const DRAFT = /(초안|\bdraft\b)/i;
const WRITE = /(써|쓰|작성|만들|준비|보내|해\s*줘|해줘|해\s*주세요)/;
/** An explicit request for a reply/draft of a mail. Sending is never part of its effect. */
export function explicitMailRequest(text: string): boolean {
  let explicit = false;
  for (const clause of clauses(text)) {
    const open = unquoted(clause);
    if (!MAIL_NOUN.test(clause) || !((REPLY.test(open) || DRAFT.test(open)) && WRITE.test(open))) continue;
    if (NEGATED.test(open)) return false;
    if (HOW_TO.test(open)) continue;
    explicit = true;
  }
  return explicit;
}
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+/g;
/** The recipient and body the member gave: a quoted text (or "내용은 …") is the exact body; addresses outside it are recipients. */
export function parseMailDetails(text: string): { to: string; body: string } {
  const quotes = [...text.matchAll(/"([^"\n]{2,})"|“([^”]{2,})”|‘([^’]{2,})’|「([^」]{2,})」/g)].map(m => (m[1] ?? m[2] ?? m[3] ?? m[4]).trim());
  let body = quotes.filter(q => !new RegExp(`^${EMAIL.source}$`).test(q)).sort((a, b) => b.length - a.length)[0] ?? "";
  if (!body) {
    const m = text.match(/(?:내용|본문)\s*(?:은|는|:|：)\s*([\s\S]+)$/u);
    if (m) body = m[1].replace(/\s*(?:라고|이라고)?\s*(?:써|적어|보내|작성|답장)\S*(?:\s*(?:줘|주세요|줄래))?[.!?]*\s*$/u, "").trim();
  }
  const outside = body ? text.replace(body, " ") : text;
  return { to: [...new Set(outside.match(EMAIL) ?? [])].join(", "), body };
}
export function localZone(): string {
  try { return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC"; } catch { return "UTC"; }
}
/** The member's own words decide; null means ordinary chat. `now`/`zone` only feed the calendar parser. */
export function memberRoute(text: string, now: Date = new Date(), zone: string = localZone()): MemberRoute | null {
  const t = text.trim();
  if (!t) return null;
  if (explicitCalendarRequest(t)) {
    const parsed = parseKoreanCalendarRequest(t, now, zone);
    if (parsed) return { kind: "calendar", parsed };
  }
  const parts = clauses(t).map(unquoted);
  const excluded = parts.filter(c => NEGATED.test(c));
  const outcome = suggestOutcome(parts.filter(c => !NEGATED.test(c)).join(" "));
  // Shopping never consumes mail. A separate instruction to leave the inbox alone is
  // compatible with a price comparison; a prohibition on the requested work still wins.
  const leaveMailAlone = /^(?:내\s*)?(?:이?메일|지메일|받은편지함)(?:은|는|을|를)?\s*(?:읽|보|쓰|사용하|열|참고하)지\s*(?:마|말아\s*줘|마세요|말아\s*주세요)$/;
  if (outcome && (!excluded.length || outcome.kind === "shopping" && excluded.every(c => leaveMailAlone.test(c.trim())))) {
    return { kind: "research", outcome: outcome.kind, criteria: outcome.criteria };
  }
  if (explicitMailRequest(t)) return { kind: "mail", ...parseMailDetails(t) };
  return null;
}

/** A stated budget or ceiling constrains a comparison; an observed seller price does not. */
function budgetCeiling(text: string): number | undefined {
  const amounts = [...text.matchAll(/(\d[\d,]*)\s*(만)?\s*원(?:\s*(이하|이내|까지|미만|이상|초과|부터))?/g)];
  const eligible = amounts.filter(m => !["이상", "초과", "부터"].includes(m[3]) &&
    !/^\s*넘/.test(text.slice(m.index + m[0].length)));
  const explicit = eligible.find(m => /(?:예산|상한|최대|budget)\s*(?:은|는|이|가|을|를|:)?\s*$/i.test(text.slice(0, m.index)));
  const ceiling = explicit ?? eligible.find(m => m[3]);
  if (!ceiling) return undefined;
  const won = Number(ceiling[1].replace(/,/g, "")) * (ceiling[2] ? 10000 : 1) - (ceiling[3] === "미만" ? 1 : 0);
  return Number.isSafeInteger(won) && won > 0 ? won : undefined;
}

/** Recognize the comparison and retain the deadline and budget in the original request. */
export function suggestOutcome(message: string, today = new Date()): { kind: OutcomeKind; criteria: Criteria } | null {
  const text = message.trim();
  const commerce = /판매처|판매자|상품|제품|가격|최저가|구매|주문|배송|쇼핑|\b(?:offers?|sellers?|products?|shops?|prices?|delivery|cheapest|purchase|buy)\b/i.test(text);
  const comparing = /비교|합계|최저가|가장\s*(?:싼|저렴)|\b(?:compare|cheapest)\b/i.test(text);
  const shopping = commerce && comparing;
  const application = /(공고|지원금|공모|지원서|신청서|grant|funding).*(메일|초안|지원|apply|application|draft)|(메일|mail).*(공고|grant)/i.test(text);
  if (!shopping && !application) return null;
  const criteria: Criteria = {};
  const iso = text.match(/\b(20\d{2})-(\d{2})-(\d{2})\b/);
  const korean = text.match(/(?:(20\d{2})년\s*)?(\d{1,2})월\s*(\d{1,2})일/);
  if (shopping && iso) criteria.deadline = iso[0];
  else if (shopping && korean) {
    const passed = +korean[2] < today.getMonth() + 1 || (+korean[2] === today.getMonth() + 1 && +korean[3] < today.getDate());
    const year = korean[1] ?? String(today.getFullYear() + (passed ? 1 : 0));
    criteria.deadline = `${year}-${korean[2].padStart(2, "0")}-${korean[3].padStart(2, "0")}`;
  }
  if (shopping && !criteria.deadline) {
    const relative = text.match(/(오늘|내일|모레|글피)(?:까지|\s*(?:도착|배송))/);
    const weekday = text.match(/(?:(이번|다음)\s*주\s*)?([월화수목금토일])요일(?:까지|\s*(?:도착|배송))/);
    let days: number | undefined;
    if (relative) days = ["오늘", "내일", "모레", "글피"].indexOf(relative[1]);
    else if (weekday) {
      const target = "월화수목금토일".indexOf(weekday[2]), current = (today.getDay() + 6) % 7;
      days = weekday[1] === "다음" ? target - current + 7 : weekday[1] === "이번" ? target - current : (target - current + 7) % 7;
    }
    if (days !== undefined) {
      const date = new Date(today.getFullYear(), today.getMonth(), today.getDate() + days, 12);
      criteria.deadline = [date.getFullYear(), String(date.getMonth() + 1).padStart(2, "0"), String(date.getDate()).padStart(2, "0")].join("-");
    }
  }
  if (shopping) {
    const budget = budgetCeiling(text);
    if (budget !== undefined) { criteria.budget_minor = budget; criteria.currency = "KRW"; }
  }
  return { kind: shopping && !application ? "shopping" : application && !shopping ? "application" : (/(공고|grant|지원)/i.test(text) ? "application" : "shopping"), criteria };
}

/** Ordered work decisions shared by the composer button and its submit handler. */
export function composerWork(text: string) {
  if (!text.trim()) return null;
  const watch = parsePublicWatchChat(text);
  if (watch) return watch.ok ? { kind: "publicWatch" as const }
    : { kind: "invalid" as const, message: watch.message };
  const brief = !NEGATED.test(unquoted(text)) && parseConnectedBriefChat(text);
  if (brief) return brief.ok ? { kind: "connectedBrief" as const }
    : { kind: "invalid" as const, message: brief.message };
  const monitor = parseMonitorChat(text);
  if (monitor.ok) return { kind: "monitor" as const, intent: monitor.intent };
  if (monitor.reason !== "not_monitor") return { kind: "invalid" as const, message: monitor.message };
  const items = parseWorkItems(text);
  if (items) return items;
  const search = personalResearchIntent(text);
  if (search) return { kind: "search" as const, search };
  return memberRoute(text);
}
