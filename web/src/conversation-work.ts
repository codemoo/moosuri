// Member conversation work: contract types, response validation, and a closed,
// deterministic Korean parser. No model is involved; a sentence that does not
// match a rule below is never turned into work.
export type WorkScope = "memory" | "conversation";
export type WorkItemInput = { title: string; scope: WorkScope; query: string; filters?: string[] };
export type ConversationTaskState = "queued" | "needs_input" | "completed" | "cancelled" | "failed";
export type ConversationEvent = { state: string; text: string; at: number };
export type ConversationTask = {
  id: string; title: string; scope: WorkScope; query: string; filter: string | null;
  state: ConversationTaskState; revision: number; question_id: string | null; run_id: string | null;
  runs: string[]; events: ConversationEvent[]; can_reuse?: boolean; source_count: number; summary: string | null; report_id: string | null;
};
export type ConversationWorkGroup = {
  id: string; thread_id: string; turn_id: string; message: string; tasks: ConversationTask[];
  /** Combined report over every child; absent on old servers and until one is stored. */
  group_report_id?: string | null;
  previous_reports?: string[];
  /** A combined report was asked for and is not stored yet. */
  report_requested?: boolean;
};
/** A group still changing on the server: a live child or a combined report in progress. */
export const groupInProgress = (g: ConversationWorkGroup) =>
  g.tasks.some(t => t.state === "queued") || (!!g.report_requested && !g.group_report_id);
export type ConversationWorkPage = { thread_id: string; groups: ConversationWorkGroup[] };
export type ConversationWorkRequest = { request_id: string; message: string; items: WorkItemInput[] };
export type ConversationActionName = "refine" | "answer" | "cancel" | "reuse" | "save_report";
export type ConversationWorkAction = {
  request_id: string; task_id: string; revision: number; action: ConversationActionName;
  text?: string; question_id?: string;
};
export type ConversationReport = { id: string; title: string; body: string; run_id: string; children?: { title: string; body: string }[] };

export const WORK_RETRY_KEY = "moosuri-work-retry";
export const WORK_PENDING_KEY = "moosuri-pending-research-v1";
export function clearWorkRecovery() {
  try {
    sessionStorage.removeItem(WORK_RETRY_KEY);
    sessionStorage.removeItem(WORK_PENDING_KEY);
  } catch { /* Restricted tab storage carries no usable recovery record. */ }
}

export const WORK_PROVIDER_ID = "member-conversation-work";
export const MAX_WORK_ITEMS = 6;
export const MAX_QUERY_BYTES = 128;
export const scopeName: Record<WorkScope, string> = { memory: "기억", conversation: "대화" };
export const taskStateLabel: Record<ConversationTaskState, string> = {
  queued: "확인 대기·진행 중", needs_input: "답변 필요", completed: "완료", cancelled: "중단됨", failed: "실패",
};
const TASK_STATES = Object.keys(taskStateLabel);
const bytes = (value: string) => new TextEncoder().encode(value).length;
export const queryTooLong = (value: string) => bytes(value) > MAX_QUERY_BYTES;

// ---- response validation (unknown JSON -> closed shapes) -------------------
const obj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (v: unknown): v is string => typeof v === "string";
function task(v: unknown): ConversationTask | null {
  if (!obj(v) || !str(v.id) || !v.id || !str(v.title) || !str(v.query) || !str(v.state) || !TASK_STATES.includes(v.state) ||
      (v.scope !== "memory" && v.scope !== "conversation") || typeof v.revision !== "number") return null;
  const opt = (x: unknown) => (str(x) ? x : null);
  return {
    id: v.id, title: v.title, scope: v.scope, query: v.query, filter: opt(v.filter), state: v.state as ConversationTaskState,
    revision: v.revision, question_id: opt(v.question_id), run_id: opt(v.run_id),
    runs: Array.isArray(v.runs) ? v.runs.filter(str) : [],
    events: Array.isArray(v.events)
      ? v.events.flatMap(e => obj(e) && str(e.state) && str(e.text) && typeof e.at === "number" ? [{ state: e.state, text: e.text, at: e.at }] : [])
      : [],
    can_reuse: typeof v.can_reuse === "boolean" ? v.can_reuse : undefined,
    source_count: typeof v.source_count === "number" ? v.source_count : 0,
    summary: opt(v.summary), report_id: opt(v.report_id),
  };
}
export function parseGroup(v: unknown): ConversationWorkGroup | null {
  if (!obj(v) || !str(v.id) || !v.id || !str(v.thread_id) || !Array.isArray(v.tasks)) return null;
  const tasks = v.tasks.map(task);
  if (tasks.some(t => t === null)) return null;
  return {
    id: v.id, thread_id: v.thread_id, turn_id: str(v.turn_id) ? v.turn_id : "", message: str(v.message) ? v.message : "", tasks: tasks as ConversationTask[],
    ...(str(v.group_report_id) && v.group_report_id ? { group_report_id: v.group_report_id } : {}),
    ...(Array.isArray(v.previous_reports) && v.previous_reports.every(str) ? { previous_reports: v.previous_reports } : {}),
    ...(v.report_requested === true ? { report_requested: true } : {}),
  };
}
export function parsePage(v: unknown): ConversationWorkPage | null {
  if (!obj(v) || !str(v.thread_id) || !Array.isArray(v.groups)) return null;
  const groups = v.groups.map(parseGroup);
  return groups.some(g => g === null) ? null : { thread_id: v.thread_id, groups: groups as ConversationWorkGroup[] };
}
export function parseReport(v: unknown): ConversationReport | null {
  if (!obj(v) || !str(v.id) || !str(v.title) || !str(v.body)) return null;
  // A single-child report carries its run; a combined report may instead list each child's evidence.
  const children = Array.isArray(v.children)
    ? v.children.flatMap(c => obj(c) && str(c.title) && str(c.body) ? [{ title: c.title, body: c.body }] : [])
    : [];
  if (!str(v.run_id) && !children.length) return null;
  return { id: v.id, title: v.title, body: v.body, run_id: str(v.run_id) ? v.run_id : "", ...(children.length ? { children } : {}) };
}
/** A response never lowers a child's revision: late/stale pages cannot undo newer state.
 * Group-level report fields always follow the server (a deleted report must not linger). */
export function mergeGroup(old: ConversationWorkGroup | undefined, next: ConversationWorkGroup): ConversationWorkGroup {
  if (!old) return next;
  const before = new Map(old.tasks.map(t => [t.id, t]));
  return { ...next, tasks: next.tasks.map(t => { const p = before.get(t.id); return p && p.revision > t.revision ? p : t; }) };
}

// ---- eligibility (deterministic; the server remains the authority) ---------
export type TaskAction = "answer" | "refine" | "cancel" | "reuse" | "save_report" | "open_report";
export function eligibleActions(t: ConversationTask): TaskAction[] {
  switch (t.state) {
    case "needs_input": return t.question_id ? ["answer", "cancel"] : ["cancel"];
    case "queued": return ["refine", "cancel"];
    case "completed": {
      const out: TaskAction[] = ["refine"];
      if (t.run_id && t.can_reuse !== false && t.source_count > 0 && t.query.trim()) out.push("reuse");
      if (t.report_id) out.push("open_report"); else if (t.run_id) out.push("save_report");
      return out;
    }
    default: return [];
  }
}
export const isLive = (t: ConversationTask) => t.state === "queued";

// ---- composing new work ----------------------------------------------------
export type WorkParse = { kind: "items"; items: WorkItemInput[] } | { kind: "invalid"; message: string } | null;
const clip = (s: string, n: number) => Array.from(s).slice(0, n).join("");
export function workItem(scope: WorkScope, query: string): WorkItemInput {
  const q = query.trim();
  return { scope, query: q, title: clip(q ? `${q} 조사` : `${scopeName[scope]} 검색`, 80) };
}
/** The message a multi-item form sends; it re-parses to the same items. */
export function compactMessage(items: WorkItemInput[]) {
  return items.map(i => `${scopeName[i.scope]} 검색: ${i.query}`.trimEnd()).join("; ");
}
const EXPLICIT = /^(?:내\s*)?(기억|대화)\s*검색\s*[:：]\s*(.*)$/u;
const verb = "(?:찾아\\s*(?:줘|주고|주세요)|검색해\\s*(?:줘|주고|주세요)|조사해\\s*(?:줘|주고|주세요))";
const CLAUSE = new RegExp(`^(?:(.{1,32}?)\\s+)?(기억|대화)(?:에서|\\s*기록에서)\\s+(.+?)\\s*${verb}\\s*[.!?]?$`, "u");
const toScope = (w: string): WorkScope => (w === "기억" ? "memory" : "conversation");
function objectNoun(phrase: string) {
  let word = phrase.trim().replace(/^(?:정해진|저장된)\s+/u, "").replace(/(?:을|를)$/u, "");
  if (Array.from(word).length >= 3) word = word.replace(/도$/u, "");
  return word.replace(/^["'‘“]|["'’”]$/gu, "").trim();
}
function checkItems(items: WorkItemInput[]): WorkParse {
  if (items.length > MAX_WORK_ITEMS) return { kind: "invalid", message: `한 번에 ${MAX_WORK_ITEMS}개까지만 맡길 수 있습니다.` };
  if (items.some(i => queryTooLong(i.query))) return { kind: "invalid", message: `검색어는 ${MAX_QUERY_BYTES}바이트 이내로 줄여 주세요.` };
  return { kind: "items", items };
}
/** Two or more independent searches. A single search stays on the legacy route. */
export function parseWorkItems(text: string): WorkParse {
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (EXPLICIT.test(trimmed.split(/[;；\n]/u)[0].trim())) {
    const parts = trimmed.split(/[;；\n]/u).map(p => p.trim()).filter(Boolean);
    if (parts.length < 2) return null;
    const items: WorkItemInput[] = [];
    for (const part of parts) {
      const m = part.match(EXPLICIT);
      if (!m) return { kind: "invalid", message: "각 항목을 '대화 검색: 검색어; 기억 검색: 검색어' 형식으로 적어 주세요." };
      items.push(workItem(toScope(m[1]), m[2]));
    }
    return checkItems(items);
  }
  const clauses = trimmed.split(/[,，]\s*|(?<=주고)\s+/u).map(c => c.trim().replace(/^(?:그리고|또)\s+/u, "")).filter(Boolean);
  if (clauses.length < 2) return null;
  const items: WorkItemInput[] = [];
  for (const clause of clauses) {
    const m = clause.match(CLAUSE);
    const noun = m ? objectNoun(m[3]) : "";
    if (!m || !noun) return null; // never forward a clause we could not read
    const item = workItem(toScope(m[2]), noun);
    const context = (m[1] ?? "").trim().replace(/^(?:내|저장된|지난|이전)(?:\s+|$)/u, "").trim();
    if (context) {
      if (!/^[\p{L}\p{N} -]+$/u.test(context) || queryTooLong(context)) return null;
      item.filters = [context];
      item.title = clip(`${context} ${noun} 조사`, 80);
    }
    items.push(item);
  }
  return checkItems(items);
}

// ---- controlling existing work --------------------------------------------
export type WorkControl =
  | { kind: "status" }
  | { kind: "cancel"; all: boolean }
  | { kind: "refine"; subject: string; term: string }
  | { kind: "answer"; term: string };
const ALL = "둘\\s*다|셋\\s*다|넷\\s*다|모두|전부";
const STATUS_A = new RegExp(`^(?:(?:${ALL}|다|그거|검색|작업|지금)\\s*)?어디까지\\s*(?:됐어|됐니|됐나요|되었어|했어|진행\\s*됐어)\\s*[?？!.]?$`, "u");
const STATUS_B = /^(?:진행\s*상황|작업\s*상태|상태)\s*(?:알려\s*줘|어때|보여\s*줘)?\s*[?？!.]?$/u;
const CANCEL = new RegExp(`^(?:(${ALL})|그거|그\\s*작업|그\\s*검색|검색|작업)\\s*(?:다\\s*)?(?:멈춰(?:\\s*줘)?|중지(?:해\\s*줘)?|취소(?:해\\s*줘)?|그만(?:해(?:\\s*줘)?)?)\\s*[.!]?$`, "u");
const REFINE = /^(.{1,40}?)(?:은|는)\s+(.{1,60}?)\s*(?:으로만|만)\s*(?:해\s*줘|보여\s*줘|남겨\s*줘|좁혀\s*줘)?\s*[.!]?$/u;
export function parseWorkControl(text: string): WorkControl | null {
  const t = text.trim();
  if (!t || t.includes("\n")) return null;
  if (STATUS_A.test(t) || STATUS_B.test(t)) return { kind: "status" };
  const cancel = t.match(CANCEL);
  if (cancel) return { kind: "cancel", all: !!cancel[1] };
  const answer = t.match(/^(?:질문\s*답(?:변)?은|찾을\s*검색어는)\s+(.+?)\s*[.!]?$/u);
  if (answer && !/[?？]/u.test(t)) return { kind: "answer", term: answer[1].trim() };
  const refine = t.match(REFINE);
  if (refine && !/[?？]/u.test(t)) return { kind: "refine", subject: refine[1].trim(), term: refine[2].trim() };
  return null;
}

const ORDINAL_WORDS: Record<string, number> = { 첫: 1, 한: 1, 두: 2, 세: 3, 네: 4, 다섯: 5, 여섯: 6 };
/** 1-based position named by "두 번째 건" / "3번째"; null for anything else. */
export function ordinalOf(subject: string): number | null {
  const m = subject.trim().match(/^(?:(첫|한|두|세|네|다섯|여섯)|(\d{1,2}))\s*번째(?:\s*(?:건|검색|조사|것|작업))?$/u);
  if (!m) return null;
  const n = m[1] ? ORDINAL_WORDS[m[1]] : Number(m[2]);
  return n >= 1 && n <= MAX_WORK_ITEMS ? n : null;
}

export type TaskRef ={ group: ConversationWorkGroup; task: ConversationTask };
export type Resolution =
  | { kind: "pass" }                                       // no work context: ordinary chat
  | { kind: "server" }                                     // positional reference: the server maps it
  | { kind: "status"; group: ConversationWorkGroup }
  | { kind: "act"; action: "cancel" | "refine" | "answer"; targets: TaskRef[]; text?: string }
  | { kind: "choose"; action: "cancel" | "refine" | "answer"; candidates: TaskRef[]; text?: string }
  | { kind: "clarify"; message: string };
/** `groups` must be newest first. Nothing is guessed: no match means a question. */
export function resolveControl(control: WorkControl, groups: ConversationWorkGroup[]): Resolution {
  if (!groups.some(g => g.tasks.length)) return { kind: "pass" };
  if (control.kind === "status") return { kind: "status", group: groups.find(g => g.tasks.length)! };
  if (control.kind === "answer") {
    if (queryTooLong(control.term)) return { kind: "clarify", message: "찾을 검색어가 너무 깁니다." };
    const candidates = groups.flatMap(group => group.tasks.filter(t => t.state === "needs_input" && t.question_id).map(task => ({ group, task })));
    if (!candidates.length) return { kind: "clarify", message: "이 대화에 답을 기다리는 검색이 없습니다." };
    return candidates.length === 1 ? { kind: "act", action: "answer", targets: candidates, text: control.term }
      : { kind: "choose", action: "answer", candidates, text: control.term };
  }
  if (control.kind === "cancel") {
    const owner = groups.find(g => g.tasks.some(t => eligibleActions(t).includes("cancel")));
    if (!owner) return { kind: "clarify", message: "멈출 수 있는 작업이 없습니다. 이미 끝났거나 멈춘 작업입니다." };
    const all = owner.tasks.filter(t => eligibleActions(t).includes("cancel")).map(task => ({ group: owner, task }));
    return all.length === 1 || control.all ? { kind: "act", action: "cancel", targets: all }
      : { kind: "choose", action: "cancel", candidates: all };
  }
  if (queryTooLong(control.term)) return { kind: "clarify", message: `추가 조건은 ${MAX_QUERY_BYTES}바이트 이내로 줄여 주세요.` };
  const nth = ordinalOf(control.subject);
  if (nth !== null) {
    // "두 번째 건" names a position, not a title. Resolve it only when exactly one visible group
    // could be meant; otherwise the server maps it to its own candidate list.
    const withTasks = groups.filter(g => g.tasks.length);
    if (withTasks.length !== 1) return { kind: "server" };
    const task = withTasks[0].tasks[nth - 1];
    if (!task || !eligibleActions(task).includes("refine")) return { kind: "server" };
    return { kind: "act", action: "refine", targets: [{ group: withTasks[0], task }], text: control.term };
  }
  const subject = control.subject.toLowerCase();
  const found: TaskRef[] = [];
  for (const group of groups) for (const t of group.tasks)
    if (eligibleActions(t).includes("refine") && (t.query.toLowerCase().includes(subject) || t.title.toLowerCase().includes(subject)))
      found.push({ group, task: t });
  if (!found.length) return { kind: "clarify", message: `'${control.subject}'에 해당하는 진행 중이거나 끝난 검색을 찾지 못했습니다.` };
  const exact = found.filter(({ task }) => [task.query, task.title, task.title.replace(/\s+조사$/u, "")].some(name => name.toLowerCase() === subject));
  return found.length === 1 && exact.length === 1 ? { kind: "act", action: "refine", targets: found, text: control.term }
    : { kind: "choose", action: "refine", candidates: found, text: control.term };
}
