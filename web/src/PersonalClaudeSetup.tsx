import { useEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { PersonalClaudeBrowser } from "./PersonalClaudeBrowser";

export type ClaudeStage = "preparing" | "awaiting_code" | "verifying" | "activating" | "ready" | "cancelled" | "failed" | "interrupted" | "cleanup_pending" | "disconnecting" | "disconnected";
type Operation = { request_id: string; state: ClaudeStage; kind?: string; browser_available?: boolean; connection_revision?: string };
type Reply = { operation?: Operation | null; replacement_revision?: string | null; connection_revision?: string | null };
const path = "/personal/admin/engine/subscription";
const stages: ClaudeStage[] = ["preparing", "awaiting_code", "verifying", "activating", "ready", "cancelled", "failed", "interrupted", "cleanup_pending", "disconnecting", "disconnected"];
const active: ClaudeStage[] = ["preparing", "awaiting_code", "verifying", "activating", "cleanup_pending", "disconnecting"];
const label: Record<ClaudeStage, string> = {
  preparing: "로그인을 준비하고 있습니다…",
  awaiting_code: "아래 화면에서 Claude에 로그인해 주세요. 로그인이 끝나면 자동으로 이어집니다.",
  verifying: "로그인 결과를 확인하고 있습니다…",
  activating: "구독 연결을 활성화하고 있습니다…",
  ready: "내 Claude 구독이 연결되었습니다. 다른 회원은 각자의 구독 연결이 준비되기 전까지 이 모델을 쓸 수 없습니다.",
  cancelled: "설정이 취소되었습니다. 기존 연결은 그대로입니다.",
  failed: "연결하지 못했습니다. 기존 연결은 그대로입니다. 입력을 확인한 뒤 다시 시도해 주세요.",
  interrupted: "서버가 다시 시작되어 설정이 중단되었습니다. 기존 연결은 그대로입니다. 새로 시작하기 전에 중단된 설정을 먼저 정리해야 합니다.",
  cleanup_pending: "이전 로그인을 정리하고 있습니다. 정리가 끝날 때까지 새로 시작할 수 없습니다. 오래 걸리면 ‘정리 다시 시도’를 눌러 주세요.",
  disconnecting: "이 서버의 Claude 로그인을 삭제하고 있습니다. 끝날 때까지 새로 연결할 수 없습니다…",
  disconnected: "구독 연결이 해제되었고 이 서버의 Claude 로그인이 삭제되었습니다. 이제 API 키 또는 새 구독으로 연결할 수 있습니다.",
};
const hostingTerms = "https://code.claude.com/docs/en/legal-and-compliance";

function known(value: unknown): value is Operation {
  const op = value as Operation | null;
  return !!op && typeof op.request_id === "string" && stages.includes(op.state);
}

/** Own-subscription Claude setup. Sign-in happens in the private browser screen
 * of the initiating session; no sign-in address or code ever reaches this page. */
export function PersonalClaudeSetup({ account, replaceRevision, disconnectRevision, onSaved, onDisconnected, onRefresh, expired, unavailable, onActive, pollMs = 1000 }: {
  account: string; replaceRevision?: string | null; onSaved: () => void; expired: () => void; unavailable: () => void;
  /** Set while this installation's own subscription is connected: offers the explicit disconnect. */
  disconnectRevision?: string;
  /** Called once when the server reports this flow's disconnect as finished. */
  onDisconnected?: () => void;
  onRefresh?: () => void;
  /** True while an operation is unfinished, so the parent keeps this flow mounted. */
  onActive?: (active: boolean) => void; pollMs?: number;
}) {
  const scope = useScope(account, "installation-engine-claude-setup");
  const [op, setOp] = useState<Operation | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [accepted, setAccepted] = useState(false);
  const [replacement, setReplacement] = useState<string | null>(null);
  const [checkedRevision, setCheckedRevision] = useState<string | null>(null);
  const [connectionChanged, setConnectionChanged] = useState(false);
  const connectionChecked = !!disconnectRevision && checkedRevision === disconnectRevision;
  const emailInput = useRef<HTMLInputElement>(null);
  const modelInput = useRef<HTMLInputElement>(null);
  const effortInput = useRef<HTMLSelectElement>(null);
  const pending = useRef(false);
  const authorityLost = useRef(false);
  const request = useRef<string>(crypto.randomUUID());
  const saved = useRef<string | null>(null);
  const terminal = useRef<string | null>(null);
  const external = connectionChecked && replacement === disconnectRevision;
  const effectiveReplace = external ? disconnectRevision : replaceRevision;
  const replacing = effectiveReplace !== undefined;
  const missingRevision = replacing && !effectiveReplace;

  function clear() {
    setOp(null); setBusy(false); pending.current = false;
  }
  /** True when the failure ended this flow (authority lost) or was an abort. */
  function fail(e: unknown): boolean {
    if (isAborted(e)) return true;
    if (e instanceof ApiFailure && e.status === 401) { clear(); if (!authorityLost.current) { authorityLost.current = true; expired(); } return true; }
    if (e instanceof ApiFailure && e.status === 403) { clear(); if (!authorityLost.current) { authorityLost.current = true; unavailable(); } return true; }
    return false;
  }
  function adopt(next: unknown) {
    // Only this flow's own operation counts; another request is never "ready" here.
    if (!known(next) || next.request_id !== request.current) { setError("설정 상태를 확인하지 못했습니다. 다시 확인해 주세요."); return; }
    // A poll sent before cancel may arrive after its terminal reply. Never
    // reopen a completed operation or announce a different late outcome.
    if (terminal.current === next.request_id) return;
    if (["ready", "cancelled", "failed", "disconnected"].includes(next.state)) terminal.current = next.request_id;
    setOp(next);
    if (next.state === "ready" && saved.current !== next.request_id) { saved.current = next.request_id; onSaved(); }
    if (next.state === "disconnected" && saved.current !== next.request_id) { saved.current = next.request_id; onDisconnected?.(); }
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
      if (signal?.aborted) return false;
      const hint = typeof reply.replacement_revision === "string" ? reply.replacement_revision : null;
      const observed = reply.connection_revision ?? hint;
      const changed = !!disconnectRevision && !!observed && observed !== disconnectRevision;
      setReplacement(changed ? null : hint);
      setCheckedRevision(changed ? null : disconnectRevision ?? null);
      setConnectionChanged(changed);
      if (changed) setError("기존 연결이 먼저 변경되었습니다. 위의 새로고침으로 현재 연결을 확인해 주세요.");
      const found = reply.operation;
      if (known(found) && terminal.current === found.request_id) return false;
      if (known(found) && (active.includes(found.state) || found.state === "interrupted")) { request.current = found.request_id; setOp(found); return true; }
    } catch (e) {
      if (signal?.aborted || fail(e)) return false;
      if (disconnectRevision) setError("기존 연결 상태를 확인하지 못했습니다. 다시 확인해 주세요.");
    }
    return false;
  }
  useEffect(() => {
    const cancel = new AbortController();
    request.current = crypto.randomUUID(); saved.current = null; terminal.current = null; authorityLost.current = false;
    setReplacement(null); setCheckedRevision(null); setConnectionChanged(false);
    if (!disconnectRevision) void resume(cancel.signal);
    return () => { cancel.abort(); setOp(null); };
  }, [scope]);
  useEffect(() => {
    const cancel = new AbortController();
    setCheckedRevision(null); setConnectionChanged(false);
    if (disconnectRevision) void resume(cancel.signal);
    return () => cancel.abort();
  }, [scope, disconnectRevision]);

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
        ...(effectiveReplace ? { replace_revision: effectiveReplace } : {}),
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
  /** One deliberate action: a fresh operation id plus the exact connection
   * revision being removed. A lost reply is inspected by the same id, never repeated. */
  async function disconnect() {
    if (pending.current || !disconnectRevision || !connectionChecked || external || op) return;
    pending.current = true; setBusy(true); setError("");
    const id = request.current;
    try {
      const reply = await scope.request<Reply>(path, "POST", { op: "disconnect", request_id: id, replace_revision: disconnectRevision }, undefined, { timeoutMs: 30_000 });
      adopt(reply.operation);
    } catch (e) {
      if (fail(e)) return;
      if (e instanceof ApiFailure && e.status === 409) {
        request.current = crypto.randomUUID();
        if (!(await resume())) setError("다른 설정이 진행 중이거나 연결이 먼저 변경되어 해제하지 못했습니다. 현재 상태를 확인한 뒤 다시 시도해 주세요.");
      } else if (e instanceof ApiFailure && e.status === 412) {
        setError("이 연결의 관리 기록을 확인하지 못해 해제하지 않았습니다. 기존 연결은 유지됩니다. 관리자에게 연결 복구를 요청해 주세요.");
      } else if (e instanceof ApiFailure) {
        setError("연결을 해제하지 못했습니다. 잠시 뒤 다시 시도해 주세요.");
      } else await inspect(id);
    } finally { pending.current = false; setBusy(false); }
  }
  function again() {
    request.current = crypto.randomUUID(); saved.current = null; terminal.current = null; setError(""); setOp(null);
  }

  const screen = op?.state === "awaiting_code" && op.browser_available === true;
  return <div className="management-list" data-testid="claude-subscription-setup">
    {error && <p role="alert" className="preview-error">{error}</p>}
    {!op && disconnectRevision && !connectionChecked && <div>
      {!error && <p role="status">기존 연결을 확인하고 있습니다…</p>}
      {error && <button className="member-page-action" type="button" onClick={() => { setError(""); if (connectionChanged && onRefresh) onRefresh(); else void resume(); }}>연결 상태 다시 확인</button>}
    </div>}
    {!op && disconnectRevision && connectionChecked && !external && <div className="management-list">
      <p role="status">Claude 구독이 연결되어 있습니다.</p>
      <p className="management-row management-note">연결을 해제하면 이 서버에 저장된 내 Claude 로그인이 삭제됩니다. 정리가 끝나면 API 키 또는 새 구독으로 다시 연결할 수 있습니다.</p>
      <div className="management-row compatible-actions">
        <button className="member-page-action" type="button" disabled={busy} onClick={() => void disconnect()}>{busy ? "해제하는 중…" : "구독 연결 해제"}</button>
      </div>
    </div>}
    {!op && (!disconnectRevision || external) && <form className="management-list" onSubmit={start}>
      {external && <p className="management-note">이전에 등록한 연결은 새 로그인이 확인될 때 교체됩니다. 이전 로그인 자체는 삭제하지 않습니다.</p>}
      <p className="management-row management-note">{replacing
        ? "내 Claude 구독으로 현재 연결을 교체합니다. 로그인이 끝나기 전에는 기존 연결이 유지됩니다."
        : "이 설치의 Claude 연결로 내 Claude 구독을 사용합니다. 같은 화면 안의 안전한 로그인 창에서 Claude에 로그인하면 자동으로 연결됩니다."}</p>
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
      <p role="status">{op.kind === "disconnect" && op.state === "failed" ? "연결 상태가 변경되어 해제하지 못했습니다. 연결과 로그인은 삭제하지 않았습니다. 현재 상태를 확인한 뒤 다시 시도해 주세요." : label[op.state]}</p>
      {op.state === "awaiting_code" && !screen && <p className="management-note">로그인을 시작한 세션에서 계속해 주세요. 이 세션에서는 로그인 화면을 볼 수 없습니다.</p>}
      {screen && <PersonalClaudeBrowser key={op.request_id} account={account} requestId={op.request_id} frameMs={pollMs}
        onOperation={adopt} onFailure={fail} onRefresh={() => void inspect(op.request_id)}/>}
      <div className="management-row compatible-actions">
        {active.includes(op.state) && op.state !== "cleanup_pending" && op.state !== "disconnecting" && (op.state !== "awaiting_code" || screen) && <button className="member-page-action" type="button" disabled={busy} onClick={() => void cancel()}>취소</button>}
        {(op.state === "cleanup_pending" || op.state === "interrupted") && <button className="member-page-action" type="button" disabled={busy} onClick={() => void cancel()}>{op.state === "interrupted" ? "중단된 설정 정리" : "정리 다시 시도"}</button>}
        {["cancelled", "failed", "disconnected"].includes(op.state) && <button className="member-page-action" type="button" onClick={again}>다시 시도</button>}
      </div>
    </>}
  </div>;
}
