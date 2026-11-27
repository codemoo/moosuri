import { useEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

export type ClaudeStage = "preparing" | "awaiting_code" | "verifying" | "activating" | "ready" | "cancelled" | "failed" | "interrupted" | "cleanup_pending";
type Operation = { request_id: string; state: ClaudeStage; authorization_url?: string; connection_revision?: string };
type Reply = { operation?: Operation | null };
const path = "/personal/admin/engine/subscription";
const stages: ClaudeStage[] = ["preparing", "awaiting_code", "verifying", "activating", "ready", "cancelled", "failed", "interrupted", "cleanup_pending"];
const active: ClaudeStage[] = ["preparing", "awaiting_code", "verifying", "activating", "cleanup_pending"];
const label: Record<ClaudeStage, string> = {
  preparing: "로그인을 준비하고 있습니다…",
  awaiting_code: "Claude 로그인 후 표시되는 코드를 입력해 주세요.",
  verifying: "로그인 코드를 확인하고 있습니다…",
  activating: "구독 연결을 활성화하고 있습니다…",
  ready: "내 Claude 구독이 연결되었습니다. 다른 회원은 각자의 구독 연결이 준비되기 전까지 이 모델을 쓸 수 없습니다.",
  cancelled: "설정이 취소되었습니다. 기존 연결은 그대로입니다.",
  failed: "연결하지 못했습니다. 기존 연결은 그대로입니다. 입력을 확인한 뒤 다시 시도해 주세요.",
  interrupted: "서버가 다시 시작되어 설정이 중단되었습니다. 기존 연결은 그대로입니다. 새로 시작하기 전에 중단된 설정을 먼저 정리해야 합니다.",
  cleanup_pending: "이전 로그인을 정리하고 있습니다. 정리가 끝날 때까지 새로 시작할 수 없습니다. 오래 걸리면 ‘정리 다시 시도’를 눌러 주세요.",
};
const hostingTerms = "https://code.claude.com/docs/en/legal-and-compliance";

/** Only the vetted official sign-in page may ever become a link target. */
export function officialAuthorizationUrl(value: unknown): string | null {
  if (typeof value !== "string" || value.length > 4096 || !value.startsWith("https://claude.com/cai/oauth/authorize?")) return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname === "claude.com" && url.port === "" && !url.username && !url.password
      && !url.hash && url.pathname === "/cai/oauth/authorize" ? url.href : null;
  } catch { return null; }
}
function known(value: unknown): value is Operation {
  const op = value as Operation | null;
  return !!op && typeof op.request_id === "string" && stages.includes(op.state);
}

/** Own-subscription Claude setup. The code lives only in a password input and one
 * request; the sign-in URL only in memory while the exact operation awaits it. */
export function PersonalClaudeSetup({ account, replaceRevision, onSaved, expired, unavailable, onActive, pollMs = 1000 }: {
  account: string; replaceRevision?: string | null; onSaved: () => void; expired: () => void; unavailable: () => void;
  /** True while an operation is unfinished, so the parent keeps this flow mounted. */
  onActive?: (active: boolean) => void; pollMs?: number;
}) {
  const scope = useScope(account, "installation-engine-claude-setup");
  const [op, setOp] = useState<Operation | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [accepted, setAccepted] = useState(false);
  const emailInput = useRef<HTMLInputElement>(null);
  const modelInput = useRef<HTMLInputElement>(null);
  const effortInput = useRef<HTMLSelectElement>(null);
  const codeInput = useRef<HTMLInputElement>(null);
  const pending = useRef(false);
  const request = useRef<string>(crypto.randomUUID());
  const saved = useRef<string | null>(null);
  const replacing = replaceRevision !== undefined;
  const missingRevision = replacing && !replaceRevision;

  function clear() {
    if (codeInput.current) codeInput.current.value = "";
    setOp(null); setBusy(false); pending.current = false;
  }
  /** True when the failure ended this flow (authority lost) or was an abort. */
  function fail(e: unknown): boolean {
    if (isAborted(e)) return true;
    if (e instanceof ApiFailure && e.status === 401) { clear(); expired(); return true; }
    if (e instanceof ApiFailure && e.status === 403) { clear(); unavailable(); return true; }
    return false;
  }
  function adopt(next: unknown) {
    // Only this flow's own operation counts; another request is never "ready" here.
    if (!known(next) || next.request_id !== request.current) { setError("설정 상태를 확인하지 못했습니다. 다시 확인해 주세요."); return; }
    setOp(next);
    if (next.state === "ready" && saved.current !== next.request_id) { saved.current = next.request_id; onSaved(); }
  }
  async function inspect(id: string) {
    try {
      const reply = await scope.request<Reply>(path, "POST", { op: "status", request_id: id }, undefined, { timeoutMs: 10_000 });
      if (reply.operation?.request_id === id) adopt(reply.operation);
      else setError("설정 상태를 확인하지 못했습니다. 다시 확인해 주세요.");
      return reply.operation?.state;
    } catch (e) {
      if (fail(e)) return undefined;
      setError(e instanceof ApiFailure && e.status === 404 ? "요청이 전달되지 않았습니다. 다시 시도해 주세요." : "상태를 확인하지 못했습니다. 잠시 뒤 다시 시도해 주세요.");
      return undefined;
    }
  }

  /** Adopt the owner's unfinished operation (a reload, another session, or a
   * restart): its URL (if any) stays in the exact initiating session only, and
   * an interrupted one offers the authenticated cleanup below. Returns whether
   * one was adopted. */
  async function resume(signal?: AbortSignal): Promise<boolean> {
    try {
      const reply = await scope.request<Reply>(path, "GET", undefined, signal);
      const found = reply.operation;
      if (known(found) && (active.includes(found.state) || found.state === "interrupted")) { request.current = found.request_id; setOp(found); return true; }
    } catch (e) { if (!(e instanceof ApiFailure && [404, 405, 501].includes(e.status))) fail(e); }
    return false;
  }
  useEffect(() => {
    const cancel = new AbortController();
    request.current = crypto.randomUUID(); saved.current = null;
    void resume(cancel.signal);
    return () => { cancel.abort(); if (codeInput.current) codeInput.current.value = ""; setOp(null); };
  }, [scope]);

  const polling = op !== null && active.includes(op.state);
  const unfinished = polling || op?.state === "interrupted";
  const operationId = op?.request_id;
  useEffect(() => { onActive?.(unfinished); return () => onActive?.(false); }, [unfinished]);
  useEffect(() => {
    if (!polling || !operationId) return;
    const cancel = new AbortController();
    let timer = 0;
    const tick = async () => {
      try {
        const reply = await scope.request<Reply>(path, "POST", { op: "status", request_id: operationId }, cancel.signal, { timeoutMs: 10_000 });
        if (cancel.signal.aborted) return;
        if (reply.operation?.request_id === operationId) adopt(reply.operation);
      } catch (e) {
        if (cancel.signal.aborted || isAborted(e)) return;
        if (e instanceof ApiFailure && e.status === 404) { setOp(null); setError("진행 중인 설정을 찾지 못했습니다. 다시 시작해 주세요."); return; }
        if (fail(e)) return;
      }
      timer = window.setTimeout(tick, pollMs);
    };
    timer = window.setTimeout(tick, pollMs);
    return () => { cancel.abort(); window.clearTimeout(timer); };
  }, [scope, polling, operationId]);

  async function start(event: FormEvent) {
    event.preventDefault();
    if (pending.current || !accepted || missingRevision) return;
    const email = emailInput.current?.value.trim() ?? "";
    const model = modelInput.current?.value.trim() ?? "";
    const effort = effortInput.current?.value || null;
    if (!email || !model) return;
    pending.current = true; setBusy(true); setError("");
    const id = request.current;
    try {
      const reply = await scope.request<Reply>(path, "POST", {
        op: "start", request_id: id, email, model, effort, hosting_terms_accepted: true,
        ...(replaceRevision ? { replace_revision: replaceRevision } : {}),
      }, undefined, { timeoutMs: 30_000 });
      adopt(reply.operation);
    } catch (e) {
      if (fail(e)) return;
      if (e instanceof ApiFailure && e.status === 409) {
        request.current = crypto.randomUUID();
        // An unfinished or interrupted operation of this owner is shown with its
        // cleanup controls; otherwise the connection changed first.
        if (!(await resume())) setError("이미 진행 중인 설정이 있거나 연결이 먼저 변경되었습니다. 현재 상태를 확인한 뒤 다시 시도해 주세요.");
      } else if (e instanceof ApiFailure) {
        setError("설정을 시작하지 못했습니다. 이메일과 모델을 확인해 주세요.");
      } else await inspect(id); // A lost reply may follow a started login: inspect, never start twice.
    } finally { pending.current = false; setBusy(false); }
  }
  async function submitCode(event: FormEvent) {
    event.preventDefault();
    if (pending.current || op?.state !== "awaiting_code") return;
    const entered = codeInput.current?.value ?? "";
    if (entered.length < 8) return;
    if (codeInput.current) codeInput.current.value = "";
    pending.current = true; setBusy(true); setError("");
    const id = op.request_id;
    try {
      const reply = await scope.request<Reply>(path, "POST", { op: "code", request_id: id, code: entered }, undefined, { timeoutMs: 30_000 });
      adopt(reply.operation);
    } catch (e) {
      if (fail(e)) return;
      if (e instanceof ApiFailure && e.status === 400) setError("로그인 코드 형식을 확인해 주세요.");
      else if (e instanceof ApiFailure && e.status === 409) setError("지금은 코드를 받을 수 없습니다. 상태를 확인해 주세요.");
      else if (!(e instanceof ApiFailure)) {
        // The code may have been delivered. Inspect; ask again only if it was not.
        if (await inspect(id) === "awaiting_code") setError("코드 전달을 확인하지 못했습니다. 코드를 다시 입력해 주세요.");
      } else setError("코드를 전달하지 못했습니다. 다시 시도해 주세요.");
    } finally { pending.current = false; setBusy(false); }
  }
  /** Cancel a running login, or (cleanup_pending / interrupted) ask the server
   * to retire what the unfinished setup left behind. Both are the same
   * authenticated request for this exact operation. */
  async function cancel() {
    if (!op || pending.current) return;
    pending.current = true; setBusy(true); setError("");
    try {
      const reply = await scope.request<Reply>(path, "POST", { op: "cancel", request_id: op.request_id }, undefined, { timeoutMs: 30_000 });
      adopt(reply.operation);
    } catch (e) {
      if (fail(e)) return;
      setError(e instanceof ApiFailure && e.status === 409 ? "다른 설정이 진행 중이라 지금은 정리할 수 없습니다. 잠시 뒤 다시 시도해 주세요." : "취소하지 못했습니다. 상태를 확인해 주세요.");
    } finally { pending.current = false; setBusy(false); }
  }
  function again() {
    request.current = crypto.randomUUID(); saved.current = null; setError(""); setOp(null);
  }

  const link = op?.state === "awaiting_code" ? officialAuthorizationUrl(op.authorization_url) : null;
  return <div className="management-list" data-testid="claude-subscription-setup">
    {error && <p role="alert" className="preview-error">{error}</p>}
    {!op && <form className="management-list" onSubmit={start}>
      <p className="management-row management-note">{replacing
        ? "내 Claude 구독으로 현재 연결을 교체합니다. 로그인이 끝나기 전에는 기존 연결이 유지됩니다."
        : "이 설치의 Claude 연결로 내 Claude 구독을 사용합니다. Claude에 로그인한 뒤 표시되는 코드를 여기에 입력합니다."}</p>
      <label className="management-row management-field"><span>Claude 계정 이메일</span><input ref={emailInput} type="email" autoComplete="off" required maxLength={254} disabled={busy}/></label>
      <label className="management-row management-field"><span>구독 모델 ID</span><input ref={modelInput} defaultValue="claude-sonnet-5-5" required maxLength={200} disabled={busy}/></label>
      <label className="management-row management-field"><span>구독 추론 강도</span><select aria-label="구독 추론 강도" ref={effortInput} defaultValue="medium" disabled={busy}>
        <option value="">모델 기본값</option><option value="low">낮음</option><option value="medium">보통</option><option value="high">높음</option>
      </select></label>
      <p className="management-row management-note"><a href={hostingTerms} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">Claude Code 호스팅 조건</a>을 먼저 확인해 주세요.</p>
      <label className="management-row management-toggle management-choice"><input type="checkbox" checked={accepted} onChange={event => setAccepted(event.target.checked)} disabled={busy}/> 호스팅 조건을 확인했고, 제 구독을 이 설치에서 호스팅하는 데 동의합니다</label>
      {missingRevision && <p role="alert" className="preview-error">현재 연결 상태를 확인하지 못해 교체할 수 없습니다.</p>}
      <div className="management-row compatible-actions">
        <button className="member-page-action primary" type="submit" disabled={busy || !accepted || missingRevision}>{busy ? "시작하는 중…" : "구독 로그인 시작"}</button>
      </div>
    </form>}
    {op && <>
      <p role="status">{label[op.state]}</p>
      {link && <p className="management-row"><a className="member-page-action" href={link} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">Claude 로그인 열기</a></p>}
      {op.state === "awaiting_code" && !link && <p className="management-note">로그인을 시작한 세션에서 계속해 주세요. 이 세션에서는 로그인 코드를 제출할 수 없습니다.</p>}
      {op.state === "awaiting_code" && link && <form className="management-row management-field" onSubmit={submitCode}>
        <label><span>로그인 코드</span><input ref={codeInput} type="password" autoComplete="off" spellCheck={false} required minLength={8} maxLength={4096} disabled={busy}/></label>
        <button className="member-page-action primary" type="submit" disabled={busy}>코드 제출</button>
      </form>}
      <div className="management-row compatible-actions">
        {active.includes(op.state) && op.state !== "cleanup_pending" && (op.state !== "awaiting_code" || !!link) && <button className="member-page-action" type="button" disabled={busy} onClick={() => void cancel()}>취소</button>}
        {(op.state === "cleanup_pending" || op.state === "interrupted") && <button className="member-page-action" type="button" disabled={busy} onClick={() => void cancel()}>{op.state === "interrupted" ? "중단된 설정 정리" : "정리 다시 시도"}</button>}
        {["cancelled", "failed"].includes(op.state) && <button className="member-page-action" type="button" onClick={again}>다시 시도</button>}
      </div>
    </>}
  </div>;
}
