import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { ApiFailure, isAborted, RequestScope } from "./api";

/* Durable shopping / grant-application outcomes of one thread. Everything shown is re-derived by the
 * server from the member's own readable sources on every read; this helper only renders it. It never
 * opens a browser, buys, submits or sends: it hands the existing browser a URL or a resume request. */

export type OutcomeKind = "shopping" | "application";
export type Criteria = { deadline?: string; budget_minor?: number; currency?: string; terms?: string[] };
export type Money = { currency: string; minor: number };
export type Citation = { n: number; kind: "browser" | "mail"; label: string; host: string | null; observed_at: number | null; status: "current" | "changed" | "withdrawn"; run_id: string | null; request_id: string | null };
export type Fact = { key: string; label: string; value: string; cites: number[] };
export type Offer = { seller: string; price: Money | null; shipping: Money | null; total: Money | null; delivery_by: string | null; available: boolean; verdict: string; reasons: string[]; cites: number[] };
export type Handoff = { workspace_id: string; revision: number; browser_state: string; reason: string | null; action: "resume" | "takeover" | "reopen" | "wait" };
export type Next = { kind: "none" | "ask_url" | "open_browser" | "resume_browser" | "takeover_browser" | "read_mail" | "confirm_manually"; text: string; url: string | null };
export type Outcome = {
  id: string; thread_id: string; kind: OutcomeKind; title: string; message: string; state: string; revision: number; criteria: Criteria;
  citations: Citation[]; facts: Fact[]; offers: Offer[]; missing: { code: string; text: string }[];
  conflicts: { key: string; label: string; values: { value: string; cites: number[] }[] }[];
  timing: { need_by: string | null; delivery_by: string | null; margin_days: number | null; deadline_in_days: number | null } | null;
  draft: { state: "draft"; submitted: false; title: string; body: string; cites: number[] } | null;
  receipts: { kind: string; reference: string; host: string | null; observed_at: number; cite: number; verified: "observed_on_page" }[];
  handoff: Handoff | null; next: Next; question: { id: string; text: string } | null; created_at: number;
};
export type ActionName = "refine" | "refresh" | "draft" | "answer" | "withdraw";
export type OutcomeRequest = { request_id: string; kind: OutcomeKind; message: string; criteria: Criteria; browser_runs: string[]; mail_reads: string[] };

const base = (thread: string) => `/personal/tasks/research-outcomes/${encodeURIComponent(thread)}`;
const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const arr = (v: unknown) => Array.isArray(v);

/** Anything that is not the exact expected shape grants nothing (no fact, no action). */
export function parseOutcome(v: unknown): Outcome | null {
  if (!isObj(v)) return null;
  const ok = typeof v.id === "string" && typeof v.thread_id === "string" && (v.kind === "shopping" || v.kind === "application")
    && typeof v.title === "string" && typeof v.state === "string" && Number.isInteger(v.revision)
    && isObj(v.criteria) && arr(v.citations) && arr(v.facts) && arr(v.offers) && arr(v.missing) && arr(v.conflicts)
    && arr(v.receipts) && isObj(v.next) && typeof (v.next as Record<string, unknown>).kind === "string";
  if (!ok) return null;
  if (v.draft && (!isObj(v.draft) || v.draft.submitted !== false)) return null; // a draft can never claim submission
  if ((v.receipts as unknown[]).some(r => !isObj(r) || r.verified !== "observed_on_page" || typeof r.reference !== "string")) return null;
  return v as unknown as Outcome;
}
export function parsePage(v: unknown): { thread_id: string; outcomes: Outcome[] } | null {
  if (!isObj(v) || typeof v.thread_id !== "string" || !arr(v.outcomes)) return null;
  const outcomes = (v.outcomes as unknown[]).map(parseOutcome);
  return outcomes.every(Boolean) ? { thread_id: v.thread_id, outcomes: outcomes as Outcome[] } : null;
}

/** The fallback when the member opens search without a specific topic. Nothing from the member's
 * message, criteria, mail, browser pages or history is placed in it, so the public engine learns nothing
 * about the member. It is offered only while the outcome has no URL to read. */
export const PUBLIC_SEARCH_TOPIC: Record<OutcomeKind, string> = {
  shopping: "online shopping price comparison",
  application: "grant funding announcement application",
};
export function publicSearchUrl(kind: OutcomeKind): string {
  return `https://duckduckgo.com/?q=${encodeURIComponent(PUBLIC_SEARCH_TOPIC[kind])}`;
}

export function formatMoney(m: Money | null): string {
  if (!m) return "확인 안 됨";
  const zero = m.currency === "KRW" || m.currency === "JPY";
  const whole = zero ? m.minor : Math.floor(m.minor / 100);
  const fraction = zero ? "" : `.${String(m.minor % 100).padStart(2, "0")}`;
  return `${whole.toLocaleString("en-US")}${fraction} ${m.currency}`;
}

/** Recognize the comparison and retain the deadline and budget in the original request. */
export function suggestOutcome(message: string, today = new Date()): { kind: OutcomeKind; criteria: Criteria } | null {
  const text = message.trim();
  const shopping = /(최저가|배송|도착|구매|주문|가격|비교).*(비교|합계|가격|배송|도착)|compare.*(offer|price|delivery)|cheapest/i.test(text) && /(배송|도착|가격|합계|구매|offer|price|delivery)/i.test(text);
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
  const won = text.match(/(\d[\d,]*)\s*(만)?\s*원\s*(이하|이내|까지|미만)?/);
  if (shopping && won) {
    const amount = Number(won[1].replace(/,/g, "")) * (won[2] ? 10000 : 1);
    if (Number.isSafeInteger(amount) && amount > 0) { criteria.budget_minor = amount - (won[3] === "미만" ? 1 : 0); criteria.currency = "KRW"; }
  }
  return { kind: shopping && !application ? "shopping" : application && !shopping ? "application" : (/(공고|grant|지원)/i.test(text) ? "application" : "shopping"), criteria };
}

export type OutcomeApi = {
  outcomes: Record<string, Outcome>; order: string[]; busy: Set<string>; notices: Record<string, string>; loading: boolean;
  refresh: () => void;
  /** `requestId` keeps one id across a reload (the caller persists it); `thread` targets a thread that is
   * not (yet) the observed one, e.g. a conversation created a moment ago; `onError` receives the failure. */
  create: (kind: OutcomeKind, message: string, criteria: Criteria,
    opts?: { requestId?: string; thread?: string; onError?: (e: unknown) => void }) => Promise<Outcome | null>;
  act: (outcome: Outcome, action: ActionName, extra?: { criteria?: Criteria; text?: string; question_id?: string }) => Promise<boolean>;
};

export function useResearchOutcomes(opts: {
  accountId: string; thread: string | null; enabled: boolean; expired: () => void;
  /** Bump to re-read after the existing browser or inbox read changed something. */
  refreshKey?: number;
}): OutcomeApi {
  const { accountId, thread, enabled, refreshKey } = opts;
  const [outcomes, setOutcomes] = useState<Record<string, Outcome>>({});
  const [order, setOrder] = useState<string[]>([]);
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const [notices, setNotices] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const scopeRef = useRef<RequestScope | null>(null);
  const threadRef = useRef(thread);
  threadRef.current = thread;
  const expiredRef = useRef(opts.expired);
  expiredRef.current = opts.expired;
  const busyRef = useRef(new Set<string>());
  // One request id per distinct attempt, kept across a lost response so the retry is the same request.
  const attempts = useRef(new Map<string, string>());

  useEffect(() => {
    const scope = new RequestScope(accountId);
    scopeRef.current = enabled && thread ? scope : null;
    busyRef.current = new Set(); attempts.current = new Map();
    setOutcomes({}); setOrder([]); setBusy(new Set()); setNotices({});
    if (!enabled || !thread) return () => scope.close();
    let live = true;
    setLoading(true);
    void (async () => {
      try {
        const page = parsePage(await scope.request<unknown>(base(thread)));
        if (!live || scopeRef.current !== scope || !page || page.thread_id !== thread) return;
        setOutcomes(Object.fromEntries(page.outcomes.filter(o => o.thread_id === thread).map(o => [o.id, o])));
        setOrder(page.outcomes.map(o => o.id));
      } catch (e) {
        if (!isAborted(e) && e instanceof ApiFailure && e.status === 401) expiredRef.current();
      } finally { if (live) setLoading(false); }
    })();
    return () => { live = false; if (scopeRef.current === scope) scopeRef.current = null; scope.close(); };
  }, [accountId, thread, enabled, refreshKey]);

  const merge = useCallback((outcome: Outcome) => {
    if (outcome.thread_id !== threadRef.current) return;
    setOutcomes(previous => ({ ...previous, [outcome.id]: outcome }));
    setOrder(previous => (previous.includes(outcome.id) ? previous : [...previous, outcome.id]));
  }, []);
  const setNotice = (id: string, text: string | null) =>
    setNotices(previous => { const next = { ...previous }; if (text) next[id] = text; else delete next[id]; return next; });
  /** Re-read the thread's outcomes in place (keeps notices); stale responses are fenced by the scope. */
  const reload = useCallback(async () => {
    const scope = scopeRef.current, current = threadRef.current;
    if (!scope || !current) return;
    try {
      const page = parsePage(await scope.request<unknown>(base(current)));
      if (scopeRef.current !== scope || !page || page.thread_id !== current) return;
      for (const outcome of page.outcomes) merge(outcome);
    } catch (e) {
      if (!isAborted(e) && e instanceof ApiFailure && e.status === 401) expiredRef.current();
    }
  }, [merge]);
  const failure = useCallback((id: string, e: unknown) => {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) { expiredRef.current(); return; }
    if (e instanceof ApiFailure && e.status === 409) { setNotice(id, "상태가 바뀌었습니다. 최신 내용으로 갱신합니다."); void reload(); }
    else if (e instanceof ApiFailure && [400, 403, 404, 422].includes(e.status)) setNotice(id, "이 요청을 처리할 수 없습니다. 입력과 상태를 확인해 주세요.");
    else setNotice(id, "결과를 확인하지 못했습니다. 같은 동작을 다시 누르면 중복 없이 확인합니다.");
  }, [reload]);

  const create = useCallback(async (kind: OutcomeKind, message: string, criteria: Criteria,
    opts: { requestId?: string; thread?: string; onError?: (e: unknown) => void } = {}) => {
    const current = opts.thread ?? threadRef.current;
    // The observed thread keeps its fenced scope; another thread gets a one-off scope of the same account.
    const observed = !!scopeRef.current && current === threadRef.current;
    const scope = observed ? scopeRef.current : current ? new RequestScope(accountId) : null;
    if (!scope || !current) return null;
    const key = JSON.stringify([current, kind, message, criteria]);
    if (!attempts.current.has(key)) attempts.current.set(key, opts.requestId ?? crypto.randomUUID());
    const id = attempts.current.get(key)!;
    if (busyRef.current.has(key)) { if (!observed) scope.close(); return null; }
    busyRef.current.add(key); setBusy(new Set(busyRef.current));
    try {
      const body: OutcomeRequest = { request_id: id, kind, message, criteria, browser_runs: [], mail_reads: [] };
      const outcome = parseOutcome(await scope.request<unknown>(base(current), "POST", body, undefined, { timeoutMs: 30000 }));
      if (!outcome || outcome.id !== id || outcome.thread_id !== current) throw new ApiFailure(502, "research_outcome_invalid");
      attempts.current.delete(key);
      merge(outcome);
      return outcome;
    } catch (e) { failure(id, e); opts.onError?.(e); return null; }
    finally { busyRef.current.delete(key); setBusy(new Set(busyRef.current)); if (!observed) scope.close(); }
  }, [merge, failure, accountId]);

  const act = useCallback(async (outcome: Outcome, action: ActionName, extra: { criteria?: Criteria; text?: string; question_id?: string } = {}) => {
    const scope = scopeRef.current;
    if (!scope || busyRef.current.has(outcome.id)) return false;
    busyRef.current.add(outcome.id); setBusy(new Set(busyRef.current)); setNotice(outcome.id, null);
    try {
      const body = { request_id: outcome.id, revision: outcome.revision, action, ...extra };
      const result = parseOutcome(await scope.request<unknown>(`${base(outcome.thread_id)}/control`, "POST", body, undefined, { timeoutMs: 30000 }));
      if (!result || result.id !== outcome.id) throw new ApiFailure(502, "research_outcome_invalid");
      merge(result);
      return true;
    } catch (e) { failure(outcome.id, e); return false; }
    finally { busyRef.current.delete(outcome.id); setBusy(new Set(busyRef.current)); }
  }, [merge, failure]);

  return { outcomes, order, busy, notices, loading, refresh: () => void reload(), create, act };
}

// ---- rendering ----------------------------------------------------------------
export const stateLabel: Record<string, string> = {
  needs_input: "URL이 필요합니다", needs_source: "읽을 페이지가 필요합니다", ranked: "비교 완료", no_match: "조건에 맞는 항목 없음",
  no_offers: "가격 정보를 찾지 못함", handoff: "브라우저 직접 조작 중", uncertain: "결과를 확인할 수 없음",
  confirmation_observed: "페이지에서 확인 번호 관찰됨", submission_observed: "페이지에서 접수 번호 관찰됨",
  source_changed: "출처가 바뀜", conflict: "출처 간 값이 다름", deadline_passed: "마감이 지남", draft_ready: "초안 준비됨",
  ready: "공고 확인됨", withdrawn: "근거를 읽을 수 없음", closed: "닫힘",
};
const verdictLabel: Record<string, string> = {
  best: "가장 유리", eligible: "조건 충족", late: "기한 초과", over_budget: "예산 초과", unavailable: "재고 없음",
  unverified: "확인 부족", currency_mismatch: "통화 다름",
};
const cites = (list: number[]) => list.map(n => `[${n}]`).join("");
const clock = new Intl.DateTimeFormat("ko-KR", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

function RefineForm({ outcome, pending, onApply }: { outcome: Outcome; pending: boolean; onApply: (criteria: Criteria) => void }) {
  const [deadline, setDeadline] = useState(outcome.criteria.deadline ?? "");
  const places = (currency: string) => /^(KRW|JPY)$/i.test(currency) ? 0 : 2;
  const [budget, setBudget] = useState(outcome.criteria.budget_minor ?
    (outcome.criteria.budget_minor / 10 ** places(outcome.criteria.currency ?? "KRW")).toFixed(places(outcome.criteria.currency ?? "KRW")) : "");
  const [currency, setCurrency] = useState(outcome.criteria.currency ?? "KRW");
  const [terms, setTerms] = useState((outcome.criteria.terms ?? []).join(", "));
  const precision = places(currency);
  const [units, fraction = ""] = budget.trim().split(".");
  const amount = Number(units + fraction.padEnd(precision, "0"));
  const budgetBad = budget.trim() !== "" && (!/^\d+(?:\.\d+)?$/.test(budget.trim()) || fraction.length > precision ||
    !Number.isSafeInteger(amount) || amount <= 0 || !/^[A-Za-z]{3}$/.test(currency));
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (budgetBad) return;
    const next: Criteria = {};
    if (outcome.kind === "shopping") {
      if (deadline) next.deadline = deadline;
      if (budget.trim()) { next.budget_minor = amount; next.currency = currency.toUpperCase(); }
    } else {
      const list = terms.split(",").map(t => t.trim()).filter(Boolean).slice(0, 6);
      if (list.length) next.terms = list;
    }
    onApply(next);
  };
  return <form className="outcome-form" onSubmit={submit} aria-label="조건 변경">
    {outcome.kind === "shopping" ? <>
      <label>도착 기한<input type="date" value={deadline} onChange={e => setDeadline(e.target.value)} /></label>
      <label>예산 ({currency.toUpperCase()})<input inputMode="decimal" value={budget} onChange={e => setBudget(e.target.value)} /></label>
      <label>통화<input value={currency} maxLength={3} onChange={e => setCurrency(e.target.value)} /></label>
    </> : <label>공고를 찾을 메일 제목 단어(쉼표로 구분)<input value={terms} onChange={e => setTerms(e.target.value)} /></label>}
    <button type="submit" disabled={pending || budgetBad}>조건 적용</button>
  </form>;
}

export function OutcomeCard({ outcome, api, onOpenBrowser, onBrowser, onReadMail }: {
  outcome: Outcome; api: OutcomeApi;
  /** Hand a URL the member gave to the existing browser (it opens and reads it). */
  onOpenBrowser: (url: string | null, outcomeId: string) => void;
  /** Hand a resume/takeover/reopen request to the existing browser for this workspace. */
  onBrowser: (handoff: Handoff) => void;
  onReadMail: () => void;
}) {
  const [panel, setPanel] = useState<"" | "refine" | "answer">("");
  const [url, setUrl] = useState("");
  const pending = api.busy.has(outcome.id);
  const closed = outcome.state === "closed";
  const next = outcome.next;
  const answer = async (event: FormEvent) => {
    event.preventDefault();
    if (!outcome.question || !url.trim() || pending) return;
    if (await api.act(outcome, "answer", { text: url.trim(), question_id: outcome.question.id })) { setUrl(""); setPanel(""); }
  };
  return <li className="outcome" data-state={outcome.state} aria-label={outcome.title}>
    <div className="outcome-head"><strong>{outcome.title}</strong><span className="outcome-state">{stateLabel[outcome.state] ?? outcome.state}</span></div>
    {api.notices[outcome.id] && <p role="alert" className="outcome-notice">{api.notices[outcome.id]}</p>}
    {outcome.timing && (outcome.timing.need_by || outcome.timing.deadline_in_days !== null) && <p className="outcome-meta">
      {outcome.timing.need_by ? `도착 기한 ${outcome.timing.need_by}` : ""}
      {outcome.timing.margin_days !== null ? ` · 여유 ${outcome.timing.margin_days}일` : ""}
      {outcome.timing.deadline_in_days !== null && outcome.kind === "application" ? `마감까지 ${outcome.timing.deadline_in_days}일` : ""}
    </p>}
    {outcome.offers.length > 0 && <div className="outcome-table"><table className="outcome-offers"><caption>판매 조건 비교</caption>
      <thead><tr><th>판매처</th><th>상품</th><th>배송비</th><th>합계</th><th>도착</th><th>판정</th></tr></thead>
      <tbody>{outcome.offers.map((o, i) => <tr key={i} data-verdict={o.verdict}>
        <td data-label="판매처">{o.seller} <sup>{cites(o.cites)}</sup></td><td data-label="상품">{formatMoney(o.price)}</td><td data-label="배송비">{formatMoney(o.shipping)}</td>
        <td data-label="합계">{formatMoney(o.total)}</td><td data-label="도착">{o.delivery_by ?? "확인 안 됨"}</td>
        <td data-label="판정">{verdictLabel[o.verdict] ?? o.verdict}{o.reasons.length > 0 ? ` — ${o.reasons.join(", ")}` : ""}</td></tr>)}</tbody></table></div>}
    {outcome.facts.length > 0 && <dl className="outcome-facts">{outcome.facts.map((f, i) => <div key={i}><dt>{f.label}</dt><dd>{f.value} <sup>{cites(f.cites)}</sup></dd></div>)}</dl>}
    {outcome.conflicts.map((c, i) => <div key={i} role="status" className="outcome-conflict"><strong>{c.label}: 값이 서로 다릅니다</strong>
      <ul>{c.values.map((v, j) => <li key={j}>{v.value} <sup>{cites(v.cites)}</sup></li>)}</ul></div>)}
    {outcome.missing.length > 0 && <ul className="outcome-missing" aria-label="확인되지 않은 조건">{outcome.missing.map(m => <li key={m.code}>{m.text}</li>)}</ul>}
    {outcome.draft && <section className="outcome-draft" aria-label="지원서 초안">
      <h4>{outcome.draft.title} <small>(제출되지 않음)</small></h4><pre>{outcome.draft.body}</pre></section>}
    {outcome.receipts.length > 0 && <section className="outcome-receipts" aria-label="관찰된 확인 번호">
      <h4>관찰된 확인 번호</h4><p className="outcome-meta">화면에서 본 번호이며 판매처·기관·카드사에 확인된 것은 아닙니다.</p>
      <ul>{outcome.receipts.map(r => <li key={r.reference}>{r.reference} <sup>[{r.cite}]</sup> {r.host ?? ""}</li>)}</ul></section>}
    {outcome.citations.length > 0 && <ol className="outcome-citations" aria-label="근거">{outcome.citations.map(c => <li key={c.n} value={c.n} data-status={c.status}>
      {c.label}{c.host ? ` · ${c.host}` : ""}{c.observed_at ? ` · ${clock.format(c.observed_at * 1000)}` : ""}
      {c.status === "changed" ? " · 읽은 뒤 주소가 바뀜" : c.status === "withdrawn" ? " · 더 이상 읽을 수 없음" : ""}</li>)}</ol>}
    {next.kind !== "none" && <p className="outcome-next" role="status">{next.text}</p>}
    {outcome.question && panel === "answer" && <form className="outcome-form" onSubmit={e => void answer(e)}>
      <label>공개 페이지 URL<input value={url} onChange={e => setUrl(e.target.value)} placeholder="https://" autoFocus /></label>
      <button type="submit" disabled={pending || !url.trim()}>알려주기</button></form>}
    {panel === "refine" && <RefineForm outcome={outcome} pending={pending} onApply={criteria => void api.act(outcome, "refine", { criteria }).then(ok => ok && setPanel(""))} />}
    {!closed && <div className="outcome-actions">
      {next.kind === "ask_url" && <button type="button" aria-expanded={panel === "answer"} onClick={() => setPanel(panel === "answer" ? "" : "answer")}>URL 알려주기</button>}
      {next.kind === "ask_url" && <button type="button" onClick={() => onOpenBrowser(publicSearchUrl(outcome.kind), outcome.id)}
        title={`검색어 “${PUBLIC_SEARCH_TOPIC[outcome.kind]}”`}>공개 검색 열기</button>}
      {next.kind === "open_browser" && <button type="button" onClick={() => onOpenBrowser(next.url, outcome.id)}>{next.url ? "브라우저로 열기" : "브라우저 열기"}</button>}
      {outcome.handoff && ["resume_browser", "takeover_browser"].includes(next.kind) && <button type="button" onClick={() => onBrowser(outcome.handoff!)}>{next.kind === "resume_browser" ? "브라우저에서 이어서 진행" : "브라우저에서 직접 진행"}</button>}
      {next.kind === "read_mail" && <button type="button" onClick={onReadMail}>받은편지함 읽기</button>}
      <button type="button" disabled={pending} onClick={() => void api.act(outcome, "refresh")}>다시 확인</button>
      {outcome.state !== "needs_input" && <button type="button" aria-expanded={panel === "refine"} disabled={pending} onClick={() => setPanel(panel === "refine" ? "" : "refine")}>조건 변경</button>}
      {outcome.kind === "application" && !outcome.draft && outcome.state !== "needs_input" && outcome.state !== "needs_source" && <button type="button" disabled={pending} onClick={() => void api.act(outcome, "draft")}>초안 만들기</button>}
      <button type="button" disabled={pending} onClick={() => void api.act(outcome, "withdraw")}>이 비교 철회</button>
    </div>}
  </li>;
}

export function MemberResearchOutcomes(props: {
  api: OutcomeApi; onOpenBrowser: (url: string | null, outcomeId: string) => void; onBrowser: (handoff: Handoff) => void; onReadMail: () => void;
}) {
  const { api, ...handlers } = props; // `api` is passed to each card exactly once
  if (api.loading && api.order.length === 0) return <p role="status" className="outcome-meta">비교 기록을 불러오는 중…</p>;
  if (api.order.length === 0) return null;
  return <section className="outcomes" aria-label="비교와 지원 준비">
    <ul>{api.order.map(id => api.outcomes[id] && <OutcomeCard key={id} outcome={api.outcomes[id]} api={api} {...handlers} />)}</ul>
    <button type="button" onClick={api.refresh}>전체 다시 읽기</button>
  </section>;
}
