import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { ApiFailure, isAborted, RequestScope } from "./api";
import {
  WORK_RETRY_KEY, compactMessage, eligibleActions, isLive, MAX_QUERY_BYTES, MAX_WORK_ITEMS, mergeGroup, parseGroup, parsePage, parseReport,
  queryTooLong, scopeName, taskStateLabel, workItem,
  type ConversationActionName, type ConversationReport, type ConversationTask, type ConversationWorkAction,
  type ConversationWorkGroup, type ConversationWorkRequest, type WorkItemInput, type WorkScope,
} from "./conversation-work";

const POLL_MS = 3000;
const MAX_POLLS = 100; // about five minutes; then the member refreshes by hand
const OLDER_PER_POLL = 3;
const base = (thread: string) => `/personal/tasks/conversations/${encodeURIComponent(thread)}`;

export type WorkApi = {
  groups: Record<string, ConversationWorkGroup>;
  stale: boolean;
  missing: Set<string>;
  busy: Set<string>;
  notices: Record<string, string>;
  merge: (group: ConversationWorkGroup) => void;
  refresh: () => void;
  retry: (groupId: string) => void;
  post: (thread: string, request: ConversationWorkRequest) => Promise<ConversationWorkGroup>;
  act: (group: ConversationWorkGroup, task: ConversationTask, action: ConversationActionName, extra?: { text?: string; question_id?: string }) => Promise<boolean>;
  report: (id: string) => Promise<ConversationReport>;
};

/** Owns every conversation-work request for one account+thread lifetime. Changing
 * either (or leaving) closes the RequestScope, so stale responses cannot land. */
export function useConversationWork(opts: {
  accountId: string; sessionScope?: string; thread: string | null; enabled: boolean; receiptIds: string[];
  expired: () => void; observe?: (group: ConversationWorkGroup) => void; onNewGroup?: (group: ConversationWorkGroup) => void;
}): WorkApi {
  const { accountId, sessionScope, thread, receiptIds } = opts;
  const [visible, setVisible] = useState(() => document.visibilityState !== "hidden");
  useEffect(() => {
    const visibility = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", visibility);
    return () => document.removeEventListener("visibilitychange", visibility);
  }, []);
  const enabled = opts.enabled && visible;
  const [groups, setGroups] = useState<Record<string, ConversationWorkGroup>>({});
  const [stale, setStale] = useState(false);
  const [missing, setMissing] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const [notices, setNotices] = useState<Record<string, string>>({});
  const [kick, setKick] = useState(0);
  const scopeRef = useRef<RequestScope | null>(null);
  // Writes and report reads belong to the account, not the visible thread: leaving a
  // thread must not abort a send and leave it uncertain. Logout/account change closes it.
  const writeRef = useRef<RequestScope | null>(null);
  const groupsRef = useRef(groups);
  groupsRef.current = groups;
  const recentDone = useRef(false);
  const asked = useRef(new Set<string>());
  const polls = useRef(0);
  const inflight = useRef(false);
  const busyRef = useRef(new Set<string>());
  const reuseIds = useRef(new Map<string, string>());
  const expiredRef = useRef(opts.expired);
  expiredRef.current = opts.expired;
  const newGroupRef = useRef(opts.onNewGroup);
  newGroupRef.current = opts.onNewGroup;
  const threadRef = useRef(thread);
  threadRef.current = thread;

  const persistReuse = useCallback(() => {
    try {
      if (sessionScope && reuseIds.current.size) {
        sessionStorage.setItem(WORK_RETRY_KEY, JSON.stringify({ accountId, sessionScope, entries: [...reuseIds.current].slice(-24) }));
      } else sessionStorage.removeItem(WORK_RETRY_KEY);
    } catch { /* In-memory retries are still available. */ }
  }, [accountId, sessionScope]);
  useEffect(() => {
    const writes = new RequestScope(accountId);
    writeRef.current = writes;
    busyRef.current = new Set();
    reuseIds.current = new Map();
    try {
      const saved = JSON.parse(sessionStorage.getItem(WORK_RETRY_KEY) ?? "null");
      if (sessionScope && saved?.accountId === accountId && saved?.sessionScope === sessionScope && Array.isArray(saved.entries)) {
        for (const entry of saved.entries.slice(-24)) {
          if (Array.isArray(entry) && entry.length === 2 && entry.every(x => typeof x === "string"))
            reuseIds.current.set(entry[0], entry[1]);
        }
      }
    } catch { /* An invalid record grants no action. */ }
    persistReuse();
    return () => { if (writeRef.current === writes) writeRef.current = null; writes.close(); };
  }, [accountId, sessionScope, persistReuse]);
  useEffect(() => {
    const scope = new RequestScope(accountId);
    scopeRef.current = enabled ? scope : null;
    recentDone.current = false;
    asked.current = new Set();
    polls.current = 0;
    inflight.current = false;
    setGroups({}); setStale(false); setMissing(new Set()); setBusy(new Set()); setNotices({});
    return () => { if (scopeRef.current === scope) scopeRef.current = null; scope.close(); };
  }, [accountId, thread, enabled]);

  const observe = useRef(opts.observe);
  observe.current = opts.observe;
  const merge = useCallback((group: ConversationWorkGroup) => {
    observe.current?.(group);
    if (group.thread_id !== threadRef.current) return;
    setGroups(previous => ({ ...previous, [group.id]: mergeGroup(previous[group.id], group) }));
  }, []);
  const fail = useCallback((e: unknown) => {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) expiredRef.current();
  }, []);

  /** One bounded read. Returns the groups it merged, or null if it was fenced/failed. */
  const read = useCallback(async (query = ""): Promise<ConversationWorkGroup[] | null> => {
    const scope = scopeRef.current, current = threadRef.current;
    if (!scope || !current) return null;
    try {
      const page = parsePage(await scope.request<unknown>(base(current) + query));
      if (scopeRef.current !== scope || !page || page.thread_id !== current) return null;
      for (const group of page.groups) if (group.thread_id === current) merge(group);
      return page.groups;
    } catch (e) { fail(e); return null; }
  }, [merge, fail]);

  const idsKey = receiptIds.join("|");
  // One recent read per thread; older groups are fetched once each by request_id.
  useEffect(() => {
    if (!enabled || !thread || !receiptIds.length) return;
    let live = true;
    void (async () => {
      const have = new Set(Object.keys(groupsRef.current));
      if (!recentDone.current) {
        recentDone.current = true;
        const recent = await read();
        for (const group of recent ?? []) have.add(group.id);
      }
      if (!live) return;
      const absent = receiptIds.filter(id => !have.has(id) && !asked.current.has(id));
      for (const id of absent) {
        asked.current.add(id);
        const got = await read(`?request_id=${encodeURIComponent(id)}`);
        if (!live) return;
        if (!got?.some(g => g.id === id)) setMissing(previous => new Set(previous).add(id));
      }
    })();
    return () => { live = false; };
  }, [enabled, thread, idsKey, kick, read]);

  const anyLive = Object.values(groups).some(g => g.tasks.some(isLive));
  useEffect(() => {
    if (!enabled || !thread || !anyLive || stale) return;
    const timer = window.setInterval(() => {
      if (inflight.current) return;
      if (polls.current >= MAX_POLLS) { setStale(true); return; }
      polls.current += 1;
      inflight.current = true;
      void (async () => {
        try {
          const recent = await read();
          const seen = new Set((recent ?? []).map(g => g.id));
          const older = Object.values(groupsRef.current).filter(g => !seen.has(g.id) && g.tasks.some(isLive)).slice(0, OLDER_PER_POLL);
          for (const group of older) await read(`?request_id=${encodeURIComponent(group.id)}`);
        } finally { inflight.current = false; }
      })();
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, [enabled, thread, anyLive, stale, read]);

  const refresh = useCallback(() => {
    polls.current = 0; setStale(false);
    void read();
  }, [read]);
  const retry = useCallback((id: string) => {
    asked.current.delete(id);
    setMissing(previous => { const next = new Set(previous); next.delete(id); return next; });
    setKick(v => v + 1);
  }, []);

  const post = useCallback(async (thread: string, request: ConversationWorkRequest) => {
    const scope = writeRef.current;
    if (!scope) throw new DOMException("Closed account lifetime", "AbortError");
    const group = parseGroup(await scope.request<unknown>(base(thread), "POST", request, undefined, { timeoutMs: 30000 }));
    if (!group || writeRef.current !== scope) throw new ApiFailure(502, "conversation_work_invalid");
    merge(group);
    polls.current = 0; setStale(false);
    return group;
  }, [merge]);

  const setNotice = (id: string, text: string | null) =>
    setNotices(previous => { const next = { ...previous }; if (text) next[id] = text; else delete next[id]; return next; });
  const act = useCallback(async (group: ConversationWorkGroup, task: ConversationTask, action: ConversationActionName,
    extra: { text?: string; question_id?: string } = {}) => {
    const scope = writeRef.current, current = group.thread_id;
    if (!scope || busyRef.current.has(task.id)) return false;
    // Reuse creates a new group, so its request_id is a UUID kept stable across lost-ACK
    // retries of the same attempt. Every other action is keyed by the group it targets.
    let request_id = group.id;
    if (action === "reuse") {
      const key = [group.id, task.id, task.revision, extra.text ?? ""].join("\u0000");
      if (!reuseIds.current.has(key)) reuseIds.current.set(key, crypto.randomUUID());
      request_id = reuseIds.current.get(key)!;
      persistReuse();
    }
    const body: ConversationWorkAction = {
      request_id, task_id: task.id, revision: task.revision, action,
      ...(extra.text !== undefined ? { text: extra.text } : {}),
      ...(extra.question_id !== undefined ? { question_id: extra.question_id } : {}),
    };
    busyRef.current.add(task.id);
    setBusy(new Set(busyRef.current)); setNotice(task.id, null);
    try {
      const result = parseGroup(await scope.request<unknown>(`${base(current)}/control`, "POST", body, undefined, { timeoutMs: 30000 }));
      if (writeRef.current !== scope) return false;
      if (!result) throw new ApiFailure(502, "conversation_work_invalid");
      merge(result);
      polls.current = 0; setStale(false);
      if (action === "reuse") {
        reuseIds.current.delete([group.id, task.id, task.revision, extra.text ?? ""].join("\u0000"));
        persistReuse();
        if (threadRef.current === current) newGroupRef.current?.(result);
      }
      return true;
    } catch (e) {
      if (isAborted(e)) return false;
      fail(e);
      if (e instanceof ApiFailure && e.status === 401) return false;
      if (e instanceof ApiFailure && e.status === 409) {
        setNotice(task.id, "작업 상태가 바뀌었습니다. 최신 상태로 갱신했습니다. 다시 확인한 뒤 시도해 주세요.");
        reuseIds.current.delete([group.id, task.id, task.revision, extra.text ?? ""].join("\u0000"));
        persistReuse();
        void read(`?request_id=${encodeURIComponent(group.id)}`);
      } else if (e instanceof ApiFailure && [400, 403, 404, 422].includes(e.status)) {
        setNotice(task.id, "이 동작을 처리할 수 없습니다. 상태를 확인해 주세요.");
      } else setNotice(task.id, "처리 결과를 확인하지 못했습니다. 같은 동작으로 다시 시도하면 중복 없이 확인합니다.");
      return false;
    } finally {
      busyRef.current.delete(task.id);
      if (writeRef.current === scope) setBusy(new Set(busyRef.current));
    }
  }, [merge, fail, read, persistReuse]);

  const report = useCallback(async (id: string) => {
    const scope = scopeRef.current, current = threadRef.current;
    if (!scope || !current) throw new DOMException("Closed account lifetime", "AbortError");
    const value = parseReport(await scope.request<unknown>(`${base(current)}/reports/${encodeURIComponent(id)}`));
    if (!value || value.id !== id) throw new ApiFailure(502, "conversation_report_invalid");
    return value;
  }, []);

  return { groups, stale, missing, busy, notices, merge, refresh, retry, post, act, report };
}

// ---- cards -----------------------------------------------------------------
const timeFormat = new Intl.DateTimeFormat("ko-KR", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
const at = (seconds: number) => timeFormat.format(seconds * 1000);

function TaskCard({ group, task, work, openRun }: {
  group: ConversationWorkGroup; task: ConversationTask; work: WorkApi; openRun: (runId: string) => void;
}) {
  const [panel, setPanel] = useState<"" | "history" | "refine" | "reuse" | "answer">("");
  const [text, setText] = useState("");
  const [report, setReport] = useState<ConversationReport | null>(null);
  const [reportIssue, setReportIssue] = useState("");
  const [reading, setReading] = useState(false);
  const actions = eligibleActions(task);
  const pending = work.busy.has(task.id);
  const toggle = (next: typeof panel) => { setPanel(panel === next ? "" : next); setText(""); };
  async function submit(event: FormEvent, action: "refine" | "reuse" | "answer") {
    event.preventDefault();
    const value = text.trim();
    if (!value || queryTooLong(value) || pending) return;
    const ok = await work.act(group, task, action, action === "answer"
      ? { text: value, question_id: task.question_id ?? undefined } : { text: value });
    if (ok) { setPanel(""); setText(""); }
  }
  async function openReport() {
    if (!task.report_id || reading) return;
    if (report) { setReport(null); return; }
    setReading(true); setReportIssue("");
    try { setReport(await work.report(task.report_id)); }
    catch (e) { if (!isAborted(e)) setReportIssue("보고서를 불러오지 못했습니다."); }
    finally { setReading(false); }
  }
  const field = (label: string, action: "refine" | "reuse" | "answer", button: string) =>
    <form className="work-form" onSubmit={e => void submit(e, action)}>
      <label>{label}<input value={text} onChange={e => setText(e.target.value)} placeholder="검색어" autoFocus /></label>
      <button type="submit" disabled={pending || !text.trim() || queryTooLong(text.trim())}>{button}</button>
      {queryTooLong(text.trim()) && <small role="alert">{MAX_QUERY_BYTES}바이트 이내로 줄여 주세요.</small>}
    </form>;
  return <li className="work-task" data-state={task.state} aria-label={task.title}>
    <div className="work-task-head">
      <strong>{task.title}</strong>
      <span className="work-state">{taskStateLabel[task.state]}</span>
    </div>
    <p className="work-meta">
      {scopeName[task.scope]} 검색 · “{task.query || "검색어 없음"}”{task.filter ? ` 그리고 “${task.filter}”` : ""}
      {task.state === "completed" ? ` · 근거 ${task.source_count}건 (기록 당시)` : ""}
    </p>
    {task.state === "needs_input" && <p className="work-question" role="status">어떤 검색어로 찾을까요?</p>}
    {task.state === "completed" && task.summary && <p className="work-summary">{task.summary}</p>}
    {task.state === "completed" && !task.summary && <p className="work-meta">지금은 요약을 읽을 수 없습니다. 상세 기록에서 확인하세요.</p>}
    {work.notices[task.id] && <p role="alert" className="work-notice">{work.notices[task.id]}</p>}
    <div className="work-actions">
      <button type="button" aria-expanded={panel === "history"} onClick={() => toggle("history")}>진행 기록</button>
      {task.run_id && <button type="button" onClick={() => openRun(task.run_id!)}>상세·근거 보기</button>}
      {actions.includes("answer") && <button type="button" aria-expanded={panel === "answer"} disabled={pending} onClick={() => toggle("answer")}>검색어 입력</button>}
      {actions.includes("refine") && <button type="button" aria-expanded={panel === "refine"} disabled={pending} onClick={() => toggle("refine")}>조건 좁히기</button>}
      {actions.includes("reuse") && <button type="button" aria-expanded={panel === "reuse"} disabled={pending} onClick={() => toggle("reuse")}>새 검색어로 다시</button>}
      {actions.includes("save_report") && <button type="button" disabled={pending} onClick={() => void work.act(group, task, "save_report")}>보고서 만들기</button>}
      {actions.includes("open_report") && <button type="button" aria-expanded={!!report} disabled={reading} onClick={() => void openReport()}>{report ? "보고서 닫기" : "보고서 보기"}</button>}
      {actions.includes("cancel") && <button type="button" disabled={pending} onClick={() => void work.act(group, task, "cancel")}>중단</button>}
    </div>
    {panel === "answer" && field("찾을 검색어", "answer", "답하기")}
    {panel === "refine" && field("추가 조건(모두 포함)", "refine", "좁혀서 다시 찾기")}
    {panel === "reuse" && field("새 검색어", "reuse", "새 검색 시작")}
    {panel === "history" && <div className="work-history">
      {task.events.length === 0 && <p className="work-meta">기록이 아직 없습니다.</p>}
      <ol>{task.events.map((e, i) => <li key={i}><span>{e.text}</span> <time dateTime={new Date(e.at * 1000).toISOString()}>{at(e.at)}</time></li>)}</ol>
      {task.runs.length > 0 && <ul className="work-runs">{task.runs.map((run, i) =>
        <li key={run}><button type="button" onClick={() => openRun(run)}>{i + 1}번째 실행 상세</button></li>)}</ul>}
    </div>}
    {reportIssue && <p role="alert">{reportIssue}</p>}
    {report && <section className="work-report" aria-label={`보고서: ${report.title}`}>
      <h4>{report.title}</h4>
      <div className="work-report-body">{report.body}</div>
    </section>}
  </li>;
}

function GroupBody({ group, work, openRun }: { group: ConversationWorkGroup | undefined; work: WorkApi; openRun: (runId: string) => void }) {
  if (!group) return <p className="work-meta" role="status">작업 기록을 불러오는 중…</p>;
  return <section className="work-group" aria-label="맡긴 검색">
    <ul>{group.tasks.map(task => <TaskCard key={task.id} group={group} task={task} work={work} openRun={openRun} />)}</ul>
    {work.stale && group.tasks.some(isLive) && <p className="work-meta" role="status">자동 갱신을 멈췄습니다. <button type="button" onClick={work.refresh}>지금 확인</button></p>}
  </section>;
}
export function MissingGroup({ id, work }: { id: string; work: WorkApi }) {
  return <p className="work-meta" role="status">이 검색 기록을 불러오지 못했습니다. <button type="button" onClick={() => work.retry(id)}>다시 시도</button></p>;
}
export function GroupForTurn({ requestId, work, openRun }: { requestId: string; work: WorkApi; openRun: (runId: string) => void }) {
  const group = work.groups[requestId];
  if (!group && work.missing.has(requestId)) return <MissingGroup id={requestId} work={work} />;
  return <GroupBody group={group} work={work} openRun={openRun} />;
}

/** Live read-only status of one group, built from the same polled state. Never dispatches. */
export function WorkStatusNotice({ group, onClose }: { group: ConversationWorkGroup | undefined; onClose: () => void }) {
  if (!group) return null;
  return <div className="member-chat-notice work-status-notice" role="status">
    <strong>지금까지의 진행</strong>
    <ul>{group.tasks.map(t => <li key={t.id}>{t.title} — {taskStateLabel[t.state]}{t.state === "completed" ? ` (근거 ${t.source_count}건)` : ""}</li>)}</ul>
    <button type="button" onClick={onClose}>닫기</button>
  </div>;
}

/** Explicit way to compose up to six independent searches, whatever the wording. */
export function WorkComposer({ disabled, onSubmit }: { disabled: boolean; onSubmit: (items: WorkItemInput[]) => void }) {
  const [open, setOpen] = useState(false);
  const [rows, setRows] = useState<{ scope: WorkScope; query: string }[]>([{ scope: "conversation", query: "" }, { scope: "memory", query: "" }]);
  const filled = rows.filter(r => r.query.trim());
  const invalid = filled.length === 0 || rows.some(r => queryTooLong(r.query.trim()));
  const update = (i: number, patch: Partial<{ scope: WorkScope; query: string }>) =>
    setRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  if (!open) return <div className="work-composer-toggle"><button type="button" disabled={disabled} onClick={() => setOpen(true)}>여러 검색 한꺼번에 맡기기</button></div>;
  return <form className="work-composer" aria-label="여러 검색 맡기기" onSubmit={(e) => {
    e.preventDefault();
    if (invalid || disabled) return;
    onSubmit(filled.map(r => workItem(r.scope, r.query)));
    setOpen(false);
    setRows(rows.map(r => ({ ...r, query: "" })));
  }}>
    {rows.map((row, i) => <div className="work-composer-row" key={i}>
      <select aria-label={`${i + 1}번 찾을 곳`} value={row.scope} onChange={e => update(i, { scope: e.target.value as WorkScope })}>
        <option value="conversation">대화</option><option value="memory">기억</option>
      </select>
      <input aria-label={`${i + 1}번 검색어`} value={row.query} onChange={e => update(i, { query: e.target.value })} />
      {rows.length > 2 && <button type="button" onClick={() => setRows(rows.filter((_, j) => j !== i))}>빼기</button>}
    </div>)}
    <div className="work-actions">
      {rows.length < MAX_WORK_ITEMS && <button type="button" onClick={() => setRows([...rows, { scope: "conversation", query: "" }])}>항목 추가</button>}
      <button type="submit" disabled={disabled || invalid}>{filled.length}개 맡기기</button>
      <button type="button" onClick={() => setOpen(false)}>닫기</button>
    </div>
    {filled.length > 0 && <small>보내는 메시지: {compactMessage(filled.map(r => workItem(r.scope, r.query)))}</small>}
  </form>;
}

export function WorkCompletionMenu({ items, onOpen, onDismiss }: {
  items: { run: string; title: string; occurrence: number }[];
  onOpen: (run: string) => void; onDismiss: (run: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLElement | null>(null);
  const trigger = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);
  if (!items.length) return null;
  return <aside ref={root} className="work-complete" aria-label="완료된 조사" onKeyDown={e => {
    if (e.key === "Escape") { setOpen(false); trigger.current?.focus(); }
  }}>
    <button type="button" ref={trigger} className="work-complete-trigger" aria-expanded={open}
      onClick={() => setOpen(!open)}><span aria-hidden="true">✓</span> <span aria-live="polite">완료 {items.length}건</span></button>
    {open && <div className="work-complete-list">
      {[...items].reverse().map(item => <div key={item.run}>
        <button type="button" onClick={() => { setOpen(false); onOpen(item.run); }}>
          <strong>{item.title} 완료</strong><small>{item.occurrence}번째 실행 · 결과 보기</small>
        </button>
        <button type="button" aria-label={item.title + " 완료 알림 닫기"} onClick={() => onDismiss(item.run)}>×</button>
      </div>)}
    </div>}
  </aside>;
}
