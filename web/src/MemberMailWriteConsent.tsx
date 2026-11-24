import { useEffect, useRef, useState } from "react";
import type { RequestScope } from "./api";

/** Exact wire contract of /api/v1/personal/google/mail-write/* (see mail_write.rs). */
const base = "/personal/google/mail-write";
export type WriteReason = "google_setup_required" | "write_consent_required" | "mailbox_mismatch" | "connection_inactive" | "scope_incomplete";
export type WriteState =
  | { state: "needs_connection"; reason: WriteReason; next: "configure_google_app" | "start_write_consent" }
  | { state: "authorization"; authorization_url: string; flow_id: string }
  | { state: "connected"; connection_id: string; epoch: number; mailbox: string };

const bad = () => Error("invalid mail write response");
const reasons: WriteReason[] = ["google_setup_required", "write_consent_required", "mailbox_mismatch", "connection_inactive", "scope_incomplete"];
/** Strict: unknown states or extra fields (e.g. a leaked token) are rejected. */
export function parseWriteState(v: unknown): WriteState {
  if (!v || typeof v !== "object" || Array.isArray(v)) throw bad();
  const r = v as Record<string, unknown>;
  const only = (...keys: string[]) => { if (Object.keys(r).some(k => k !== "state" && !keys.includes(k)) || keys.some(k => !(k in r))) throw bad(); };
  switch (r.state) {
    case "needs_connection": {
      only("reason", "next");
      if (!reasons.includes(r.reason as WriteReason) || (r.next !== "configure_google_app" && r.next !== "start_write_consent")) throw bad();
      return { state: "needs_connection", reason: r.reason as WriteReason, next: r.next };
    }
    case "authorization": {
      only("authorization_url", "flow_id");
      if (typeof r.authorization_url !== "string" || typeof r.flow_id !== "string") throw bad();
      return { state: "authorization", authorization_url: r.authorization_url, flow_id: r.flow_id };
    }
    case "connected": {
      only("connection_id", "epoch", "mailbox");
      if (typeof r.connection_id !== "string" || typeof r.mailbox !== "string" || typeof r.epoch !== "number" || !Number.isFinite(r.epoch)) throw bad();
      return { state: "connected", connection_id: r.connection_id, epoch: r.epoch, mailbox: r.mailbox };
    }
    default: throw bad();
  }
}
/** Only Google's own consent page is ever navigated to. */
export function isGoogleConsentUrl(raw: string): boolean {
  try { const u = new URL(raw); return u.protocol === "https:" && u.hostname === "accounts.google.com" && u.pathname === "/o/oauth2/v2/auth"; } catch { return false; }
}

/* Tab-bound bookkeeping for the consent round trip. The gateway callback returns to
 * `/#google_handle=…&flow=…&google_service=mail-write` (or `#google_denied=…&google_service=mail-write`).
 * Only the account, session scope and original thread id are kept: never a code, token or mail text. The server
 * still verifies the one-use handle for the signed-in member; this only decides which tab/thread the return
 * belongs to. Reading is pure; `settleMailWriteReturn` consumes it once. */
const returnKey = "moosuri.member-mail-write.v1";
const slot = /^[a-zA-Z0-9_-]{1,256}$/;
const handlePattern = /^[a-f0-9]{64}$/;
type ReturnPending = { account: string; session: string; thread: string; expires: number };
function readReturn(account: string, session: string | null | undefined): ReturnPending | null {
  try {
    const p: ReturnPending | null = JSON.parse(sessionStorage.getItem(returnKey) ?? "null");
    if (session && p && p.account === account && p.session === session && p.expires > Date.now() && slot.test(p.thread)) return p;
  } catch { /* Unavailable or malformed storage cannot authorize a return. */ }
  return null;
}
export function clearMailWriteConsent() {
  try { sessionStorage.removeItem(returnKey); } catch { /* Storage unavailable. */ }
}
const service = (hash: string) => new URLSearchParams(hash.replace(/^#/, "")).getAll("google_service");
/** True for a Calendar or mail-write consent return fragment (both come back to the original conversation). */
export function isMemberConsentReturn(hash = location.hash): boolean {
  const f = new URLSearchParams(hash.replace(/^#/, "")), s = service(hash);
  return s.length === 1 && (s[0] === "calendar" || s[0] === "mail-write") && (f.has("google_handle") || f.has("google_denied"));
}
/** Remember the original conversation right before the browser leaves for Google's consent page. */
export function rememberMailWriteConsent(account: string, session: string | null | undefined, thread: string) {
  try {
    if (session && slot.test(thread))
      sessionStorage.setItem(returnKey, JSON.stringify({ account, session, thread, expires: Date.now() + 600000 } satisfies ReturnPending));
  } catch { /* Without storage the member returns to the newest conversation. */ }
}
export type MailWriteReturn = { thread: string; handle: string | null };
/** The return of the consent this tab started: its thread and the one handle (null when denied). */
export function mailWriteReturn(account: string, session: string | null | undefined, hash = location.hash): MailWriteReturn | null {
  if (!isMemberConsentReturn(hash) || service(hash)[0] !== "mail-write") return null;
  const f = new URLSearchParams(hash.replace(/^#/, "")), p = readReturn(account, session);
  if (!p) return null;
  const handles = f.getAll("google_handle"), denied = f.getAll("google_denied");
  if (handles.length === 1 && denied.length === 0 && handlePattern.test(handles[0])) return { thread: p.thread, handle: handles[0] };
  if (denied.length === 1 && handles.length === 0) return { thread: p.thread, handle: null };
  return null;
}
/** Drop the mail-write fragment and stored thread once the return was handed to the consent component. */
export function settleMailWriteReturn() {
  clearMailWriteConsent();
  if (service(location.hash)[0] === "mail-write") history.replaceState(history.state, "", location.pathname + location.search);
}

const reasonText: Record<WriteReason, string> = {
  google_setup_required: "이 계정에는 Google 앱 설정이 아직 없어 Gmail 쓰기 권한을 요청할 수 없습니다. 설정을 마친 뒤 다시 시도하세요.",
  write_consent_required: "지금은 읽기 전용입니다. Gmail에 초안을 저장하거나 메일을 보내려면 아래에서 쓰기 권한을 따로 허용해야 합니다.",
  mailbox_mismatch: "허용된 쓰기 연결이 읽은 편지함과 다른 주소입니다. 읽은 편지함과 같은 주소로 다시 허용하세요.",
  connection_inactive: "쓰기 연결이 해제되었거나 다시 연결이 필요합니다. 다시 허용하세요.",
  scope_incomplete: "Google 동의 화면에서 필요한 권한이 모두 선택되지 않아 연결하지 않았습니다. 모든 항목을 허용해야 합니다.",
};

export function MemberMailWriteConsent({ request, callbackHandle = null, fail, disabled, navigate = (url: string) => window.location.assign(url), onChange }: {
  request: RequestScope["request"];
  /** One-use handle from the OAuth callback return, if this page load is that return. */
  callbackHandle?: string | null;
  fail: (e: unknown) => void;
  disabled: boolean;
  navigate?: (url: string) => void;
  onChange?: (state: WriteState) => void;
}) {
  const [state, setState] = useState<WriteState | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const pending = useRef<{ handle: string; reply: Promise<unknown> } | null>(null);
  const call = (path: string, method: "GET" | "POST", payload?: unknown) => request<unknown>(`${base}/${path}`, method, payload);
  function show(next: WriteState) { setState(next); onChange?.(next); }
  async function run(op: () => Promise<void>) {
    setBusy(true); setNote("");
    try { await op(); } catch (e) { fail(e); } finally { setBusy(false); }
  }
  useEffect(() => {
    let live = true;
    // Returning from Google: finish the consent exactly once per handle, then show real state.
    const finish = async () => {
      if (callbackHandle) {
        if (pending.current?.handle !== callbackHandle)
          pending.current = { handle: callbackHandle, reply: call("complete", "POST", { handle: callbackHandle }) };
        const done = parseWriteState(await pending.current.reply);
        if (done.state === "authorization") throw bad();
        if (live) show(done);
        return;
      }
      const current = parseWriteState(await call("status", "GET"));
      if (current.state === "authorization") throw bad();
      if (live) show(current);
    };
    finish().catch(e => { if (live) fail(e); });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [callbackHandle]);

  const optIn = () => run(async () => {
    const out = parseWriteState(await call("start", "POST", {}));
    if (out.state === "needs_connection") { show(out); return; }
    if (out.state !== "authorization" || !isGoogleConsentUrl(out.authorization_url)) throw bad();
    setNote("Google 동의 화면으로 이동합니다.");
    navigate(out.authorization_url);
  });
  const revoke = (c: Extract<WriteState, { state: "connected" }>) => run(async () => {
    await call("disconnect", "POST", { connection_id: c.connection_id, epoch: c.epoch });
    const current = parseWriteState(await call("status", "GET"));
    if (current.state === "authorization") throw bad();
    show(current);
    setNote("쓰기 권한을 해제했습니다. 이 대기 중인 저장·발송은 더 이상 실행되지 않습니다.");
  });

  return <section className="admin-card" aria-label="Gmail 쓰기 권한">
    <h3>Gmail 쓰기 권한 (선택)</h3>
    {!state && <p role="status">권한 상태를 확인하는 중입니다.</p>}
    {state?.state === "needs_connection" && <>
      <p role="status">{reasonText[state.reason]}</p>
      {state.next === "start_write_consent" && <>
        <p>허용하면 이 앱은 연결된 같은 주소의 Gmail에 <strong>초안을 저장</strong>하고, 최종 내용을 직접 승인한 메일만 <strong>발송</strong>할 수 있습니다. 권한은 언제든 해제할 수 있습니다.</p>
        <button disabled={disabled || busy} onClick={() => void optIn()}>Gmail 초안 저장·발송 권한 허용</button>
      </>}
    </>}
    {state?.state === "connected" && <>
      <p role="status">쓰기 권한이 허용되었습니다: {state.mailbox}. 모든 저장·발송은 최종 확인본을 직접 승인한 뒤에만 실행됩니다.</p>
      <button disabled={disabled || busy} onClick={() => void revoke(state)}>쓰기 권한 해제</button>
    </>}
    {note && <p role="status">{note}</p>}
  </section>;
}
