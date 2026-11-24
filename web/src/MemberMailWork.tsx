import { useEffect, useRef, useState } from "react";
import type { RequestScope } from "./api";

/** Exact wire contract of /api/v1/personal/google/mail-work/* (see mail_work.rs). */
const base = "/personal/google/mail-work";
export type MailWorkPreview = {
  action_id: string; action_revision: number; action_digest: string;
  connection_id: string; connection_epoch: number; from: string; to: string[];
  subject: string; body: string; expires_at: number; action_state: string;
  threaded: boolean; can_save_draft: boolean; can_approve_send: boolean;
};
export type MailWorkChoice = { connection_id: string; epoch: number; mailbox: string; can_save_draft: boolean; can_send: boolean };
export type MailWorkDelivery =
  | { state: "needs_connection"; reason: string; requirements: string[] }
  | { state: "ready"; connections: MailWorkChoice[] }
  | { state: "final_preview"; preview: MailWorkPreview }
  | { state: "approved"; preview: MailWorkPreview; gmail_draft_id: string | null }
  | { state: "receipt"; kind: "gmail_draft" | "sent"; provider_draft_id: string | null; provider_message_id: string | null; provider_thread_id: string | null; preview: MailWorkPreview | null }
  | { state: "unknown"; operation: "gmail_draft" | "send"; preview: MailWorkPreview | null }
  | { state: "failed"; operation: string; reason: string }
  | { state: "stale"; reason: string };
export type MemberMailWorkItem = {
  id: string; thread_id: string; revision: number; digest: string;
  state: "local_draft" | "withdrawn";
  source: { read_request_id: string; connection_id: string; epoch: number; mailbox: string; message_id: string; subject: string; from: string; date: string } | null;
  draft: { to: string[]; subject: string; body: string } | null;
  delivery: MailWorkDelivery; created_at: number; updated_at: number;
};
export type MailWorkPrepared =
  | { outcome: "draft"; work: MemberMailWorkItem }
  | { outcome: "needs_details"; missing: string[] }
  | { outcome: "source_unavailable"; read_state: string };
/** The completed member readonly snapshot the composer already holds (/inbox). */
export type CitedRead = {
  request_id: string; state: string;
  source: { mailbox: string } | null;
  result: { mailbox: string; messages: { id: string; subject: string; from: string; date: string }[] } | null;
};

function rec(v: unknown): Record<string, unknown> {
  if (!v || typeof v !== "object" || Array.isArray(v)) throw Error("invalid mail work response");
  return v as Record<string, unknown>;
}
const str = (v: unknown) => { if (typeof v !== "string") throw Error("invalid mail work response"); return v; };
const num = (v: unknown) => { if (typeof v !== "number" || !Number.isFinite(v)) throw Error("invalid mail work response"); return v; };
const bool = (v: unknown) => { if (typeof v !== "boolean") throw Error("invalid mail work response"); return v; };
const opt = <T,>(v: unknown, f: (v: unknown) => T): T | null => (v === null || v === undefined ? null : f(v));
const strs = (v: unknown) => { if (!Array.isArray(v)) throw Error("invalid mail work response"); return v.map(str); };
function preview(v: unknown): MailWorkPreview {
  const r = rec(v);
  return {
    action_id: str(r.action_id), action_revision: num(r.action_revision), action_digest: str(r.action_digest),
    connection_id: str(r.connection_id), connection_epoch: num(r.connection_epoch), from: str(r.from), to: strs(r.to),
    subject: str(r.subject), body: str(r.body), expires_at: num(r.expires_at), action_state: str(r.action_state),
    threaded: bool(r.threaded), can_save_draft: bool(r.can_save_draft), can_approve_send: bool(r.can_approve_send),
  };
}
function delivery(v: unknown): MailWorkDelivery {
  const r = rec(v);
  switch (r.state) {
    case "needs_connection": return { state: "needs_connection", reason: str(r.reason), requirements: strs(r.requirements) };
    case "ready": {
      if (!Array.isArray(r.connections)) throw Error("invalid mail work response");
      return { state: "ready", connections: r.connections.map(c => { const x = rec(c); return { connection_id: str(x.connection_id), epoch: num(x.epoch), mailbox: str(x.mailbox), can_save_draft: bool(x.can_save_draft), can_send: bool(x.can_send) }; }) };
    }
    case "final_preview": return { state: "final_preview", preview: preview(r.preview) };
    case "approved": return { state: "approved", preview: preview(r.preview), gmail_draft_id: opt(r.gmail_draft_id, str) };
    case "receipt": {
      const kind = str(r.kind);
      if (kind !== "gmail_draft" && kind !== "sent") throw Error("invalid mail work response");
      return { state: "receipt", kind, provider_draft_id: opt(r.provider_draft_id, str), provider_message_id: opt(r.provider_message_id, str), provider_thread_id: opt(r.provider_thread_id, str), preview: opt(r.preview, preview) };
    }
    case "unknown": {
      const operation = str(r.operation);
      if (operation !== "gmail_draft" && operation !== "send") throw Error("invalid mail work response");
      return { state: "unknown", operation, preview: opt(r.preview, preview) };
    }
    case "failed": return { state: "failed", operation: str(r.operation), reason: str(r.reason) };
    case "stale": return { state: "stale", reason: str(r.reason) };
    default: throw Error("invalid mail work response");
  }
}
export function parseWork(v: unknown): MemberMailWorkItem {
  const r = rec(v);
  const state = str(r.state);
  if (state !== "local_draft" && state !== "withdrawn") throw Error("invalid mail work response");
  const source = opt(r.source, s => { const x = rec(s); return { read_request_id: str(x.read_request_id), connection_id: str(x.connection_id), epoch: num(x.epoch), mailbox: str(x.mailbox), message_id: str(x.message_id), subject: str(x.subject), from: str(x.from), date: str(x.date) }; });
  const draft = opt(r.draft, d => { const x = rec(d); return { to: strs(x.to), subject: str(x.subject), body: str(x.body) }; });
  if ((state === "local_draft") !== (source !== null && draft !== null)) throw Error("invalid mail work response");
  return { id: str(r.id), thread_id: str(r.thread_id), revision: num(r.revision), digest: str(r.digest), state, source, draft, delivery: delivery(r.delivery), created_at: num(r.created_at), updated_at: num(r.updated_at) };
}
export function parsePrepared(v: unknown): MailWorkPrepared {
  const r = rec(v);
  if (r.outcome === "draft") return { outcome: "draft", work: parseWork(r.work) };
  if (r.outcome === "needs_details") return { outcome: "needs_details", missing: strs(r.missing) };
  if (r.outcome === "source_unavailable") return { outcome: "source_unavailable", read_state: str(r.read_state) };
  throw Error("invalid mail work response");
}
function parseList(v: unknown): MemberMailWorkItem[] {
  const items = rec(v).items;
  if (!Array.isArray(items)) throw Error("invalid mail work response");
  return items.map(parseWork);
}

const reasons: Record<string, string> = {
  readonly_only: "연결된 Gmail은 읽기 전용입니다. 이 초안은 이 앱 안에만 저장되어 있으며 Gmail에 저장되거나 발송되지 않았습니다.",
  mailbox_mismatch: "다른 주소의 쓰기 연결만 있습니다. 읽은 편지함과 같은 주소의 연결이 필요합니다.",
  connection_inactive: "같은 주소의 쓰기 연결이 해제되었거나 다시 연결이 필요합니다.",
  wrong_scope: "같은 주소의 연결에 필요한 권한이 없습니다.",
};
const requirementText: Record<string, string> = {
  standard_gmail_connection_same_mailbox: "읽은 편지함과 같은 주소의 표준 Gmail 연결",
  scope_gmail_send_to_approve_and_send: "발송: gmail.send 권한과 최종 내용 승인",
  scope_gmail_compose_to_save_gmail_draft: "Gmail 초안 저장: gmail.compose 권한",
};
const stale: Record<string, string> = {
  source_withdrawn: "원본 Gmail 읽기 연결이 해제되어 초안과 출처가 삭제되었습니다. 발송 승인도 취소되었습니다.",
  expired: "최종 확인본이 만료되었습니다. 새 확인본이 필요합니다.",
  connection_changed: "쓰기 연결이 바뀌었거나 해제되었습니다. 이전 승인은 사용할 수 없습니다.",
  action_changed: "확인본이 바뀌었습니다. 새 확인본이 필요합니다.",
};
const splitList = (v: string) => v.split(/[,;\s]+/).filter(Boolean);
const act = (w: MemberMailWorkItem, p: MailWorkPreview) => ({ id: w.id, revision: w.revision, action_revision: p.action_revision, action_digest: p.action_digest });

/** What the member already said in chat: the original text, and the recipient/body parsed from it. */
export type MailSeed = { text: string; to: string; body: string; message_id?: string };
/** The one cited message the request names (subject, sender address or sender name), or the only message read. */
export function matchMailMessage(text: string, messages: { id: string; subject: string; from: string }[]): string | null {
  if (messages.length === 1) return messages[0].id;
  const t = text.toLowerCase();
  const hits = messages.filter(m => {
    const subject = m.subject.trim().toLowerCase();
    if (subject.length >= 2 && t.includes(subject)) return true;
    const email = m.from.match(/[^\s<>"]+@[^\s<>"]+/)?.[0]?.toLowerCase();
    if (email && t.includes(email)) return true;
    const name = m.from.replace(/<[^>]*>/g, "").replace(/["']/g, "").trim().toLowerCase();
    return name.length >= 2 && !name.includes("@") && t.includes(name);
  });
  return hits.length === 1 ? hits[0].id : null;
}

export function MemberMailWork({ request, threadId, read, initialBody = "", fail, disabled, newId = () => crypto.randomUUID(), seed, onConnect, onPrepared, onForm }: {
  request: RequestScope["request"];
  threadId: string;
  read: CitedRead | null;
  initialBody?: string;
  fail: (e: unknown) => void;
  disabled: boolean;
  newId?: () => string;
  /** A request made in chat: preselects the cited message and recipient/body; with all of them known it
   * prepares the local draft once (never anything in Gmail). */
  seed?: MailSeed;
  /** Opens the (separate) Gmail write-consent UI; the button only shows when this is given. */
  onConnect?: () => void;
  /** A local draft now exists on the server, so the chat-side seed is no longer needed. */
  onPrepared?: () => void;
  /** The unsent form changed (only while a seed is active), so a reload restores exactly what was typed. */
  onForm?: (form: { to: string; body: string; message_id: string }) => void;
}) {
  const initialMessage = () => {
    const list = read?.result?.messages ?? [];
    if (!seed) return list[0]?.id ?? "";
    return (seed.message_id && list.some(m => m.id === seed.message_id) ? seed.message_id : matchMailMessage(seed.text, list)) ?? "";
  };
  const [works, setWorks] = useState<MemberMailWorkItem[]>([]);
  const [body, setBody] = useState(seed ? seed.body : initialBody);
  const [to, setTo] = useState(seed?.to ?? "");
  const [message, setMessage] = useState(initialMessage);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState(() => {
    if (!seed || !read?.result) return "";
    const missing = [!initialMessage() && "회신할 메일 선택", !seed.body.trim() && "회신 본문 입력"].filter(Boolean);
    return missing.length ? `회신 초안을 만들려면 ${missing.join(", ")}이(가) 필요합니다.` : "";
  });
  const [editing, setEditing] = useState<{ id: string; to: string; body: string } | null>(null);
  const auto = useRef(!!seed && seed.body.trim() !== "");
  const pending = useRef<string | null>(null);
  const post = (path: string, payload: unknown) => request<unknown>(`${base}/${path}`, "POST", payload);
  function put(w: MemberMailWorkItem) { setWorks(all => all.some(x => x.id === w.id) ? all.map(x => x.id === w.id ? w : x) : [w, ...all]); }
  async function run(op: () => Promise<void>, uncertain = "") {
    setBusy(true); setStatus("");
    try { await op(); } catch (e) { if (uncertain) setStatus(uncertain); fail(e); } finally { setBusy(false); }
  }
  useEffect(() => {
    let live = true;
    post("list", { thread_id: threadId }).then(v => { if (live) setWorks(parseList(v)); }).catch(fail);
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [threadId]);
  const completed = read?.state === "completed" && read.result;
  // A read that arrives after the first render (or a re-read) re-resolves the cited message.
  useEffect(() => {
    const list = read?.result?.messages ?? [];
    if (list.length && !list.some(m => m.id === message)) setMessage(initialMessage());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [read?.request_id]);
  useEffect(() => {
    if (seed) onForm?.({ to, body, message_id: message });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [to, body, message]);
  useEffect(() => {
    if (!auto.current || !completed || !message || !body.trim()) return;
    auto.current = false;
    void prepare();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [!!completed, message]);
  async function prepare() {
    if (!read || !completed) return;
    const cited = read.request_id;
    // One id per intent: a retry after an unknown outcome replays, never duplicates.
    const id = pending.current ?? (pending.current = newId());
    await run(async () => {
      const out = parsePrepared(await post("prepare", { id, thread_id: threadId, read_request_id: cited, message_id: message, to: splitList(to), body }));
      if (out.outcome === "draft") { pending.current = null; put(out.work); onPrepared?.(); setStatus("로컬 답장 초안을 만들었습니다. Gmail에는 아무것도 저장하거나 보내지 않았습니다."); }
      else if (out.outcome === "needs_details") { pending.current = null; setStatus(`초안을 만들려면 ${out.missing.map(m => m === "to" ? "받는 사람" : "본문").join(", ")}이(가) 필요합니다.`); }
      else { pending.current = null; setStatus("읽은 편지함 출처가 더 이상 유효하지 않습니다. 다시 읽은 뒤 시도하세요."); }
    }, "결과를 확인하지 못했습니다. 같은 요청을 다시 누르면 중복 없이 이어집니다.");
  }
  const step = (path: string, w: MemberMailWorkItem, extra: object, uncertain = "") =>
    run(async () => { put(parseWork(await post(path, { id: w.id, revision: w.revision, ...extra }))); }, uncertain);
  /** Exact recipient/body revision: a new local draft from the same cited message, then the old one is discarded. */
  async function revise(w: MemberMailWorkItem, nextTo: string, nextBody: string) {
    const src = w.source;
    if (!src) return;
    const id = newId();
    await run(async () => {
      const out = parsePrepared(await post("prepare", { id, thread_id: threadId, read_request_id: src.read_request_id, message_id: src.message_id, to: splitList(nextTo), body: nextBody }));
      if (out.outcome === "needs_details") { setStatus(`수정본을 만들려면 ${out.missing.map(m => m === "to" ? "받는 사람" : "본문").join(", ")}이(가) 필요합니다.`); return; }
      if (out.outcome !== "draft") { setStatus("읽은 편지함 출처가 더 이상 유효하지 않아 수정본을 만들지 못했습니다. 기존 초안은 그대로입니다."); return; }
      put(out.work);
      setEditing(null);
      try {
        parseWork(await post("discard", { id: w.id, revision: w.revision }));
        setWorks(all => all.filter(x => x.id !== w.id));
        setStatus("수정한 받는 사람·본문으로 새 로컬 초안을 만들었습니다. 이전 초안은 삭제했고, 이전 최종 확인본은 더 이상 쓸 수 없습니다.");
      } catch (e) {
        setStatus("수정본을 만들었지만 이전 초안을 삭제하지 못했습니다. 이전 초안을 직접 삭제해 주세요.");
        fail(e);
      }
    }, "수정본 결과를 확인하지 못했습니다. 이전 초안은 그대로입니다.");
  }
  const editable = (w: MemberMailWorkItem) => w.state === "local_draft" && !!w.draft && !!w.source &&
    ["needs_connection", "ready", "final_preview", "stale", "failed"].includes(w.delivery.state);
  function card(w: MemberMailWorkItem) {
    const d = w.delivery;
    const p = "preview" in d ? d.preview : null;
    return <article key={w.id} aria-label="회신 초안">
      {w.state === "withdrawn" && <p role="status">{stale.source_withdrawn}</p>}
      {w.source && <p>출처: 읽기 전용 Gmail {w.source.mailbox} · 메일 “{w.source.subject}” ({w.source.from}, {w.source.date})</p>}
      {w.draft && <><p>받는 사람: {w.draft.to.join(", ")}<br />제목: {w.draft.subject}</p><pre className="mail-body">{w.draft.body}</pre></>}
      {editing?.id === w.id ? <form aria-label="초안 수정" onSubmit={e => { e.preventDefault(); void revise(w, editing.to, editing.body); }}>
        <input aria-label="받는 사람 수정" value={editing.to} onChange={e => setEditing({ ...editing, to: e.target.value })} disabled={disabled || busy} />
        <textarea aria-label="회신 본문 수정" value={editing.body} onChange={e => setEditing({ ...editing, body: e.target.value })} disabled={disabled || busy} />
        <button type="submit" disabled={disabled || busy || !editing.body.trim()}>수정본 저장</button>
        <button type="button" disabled={busy} onClick={() => setEditing(null)}>수정 취소</button>
      </form> : editable(w) && <button disabled={disabled || busy} onClick={() => setEditing({ id: w.id, to: w.draft!.to.join(", "), body: w.draft!.body })}>받는 사람·본문 수정</button>}
      {d.state === "needs_connection" && <><p role="status">{reasons[d.reason] ?? "쓰기 연결이 필요합니다."}</p><p>Gmail에 저장하거나 보내려면:</p><ul>{d.requirements.map(r => <li key={r}>{requirementText[r] ?? r}</li>)}</ul>
        {onConnect && <button type="button" disabled={busy} onClick={onConnect}>Gmail 쓰기 연결</button>}</>}
      {d.state === "ready" && <><p>같은 주소의 쓰기 연결이 있습니다. 최종 내용을 확인한 뒤에만 저장·발송할 수 있습니다.</p>
        {d.connections.map(c => <button key={c.connection_id} disabled={disabled || busy} onClick={() => void step("preview", w, { connection_id: c.connection_id, connection_epoch: c.epoch })}>{c.mailbox} 연결로 최종 확인본 만들기</button>)}</>}
      {p && <section aria-label="최종 확인본">
        <p>보내는 사람: {p.from}<br />받는 사람: {p.to.join(", ")}<br />제목: {p.subject}</p><pre className="mail-body">{p.body}</pre>
        {!p.threaded && <p>원본 메일의 스레드에 연결되지 않은 새 메일로 저장·발송됩니다.</p>}
      </section>}
      {d.state === "final_preview" && <p role="status">최종 확인본입니다. 아직 저장되거나 발송되지 않았습니다.</p>}
      {p?.can_save_draft && <button disabled={disabled || busy} onClick={() => void step("save-draft", w, act(w, p), "Gmail 저장 결과를 확인하지 못했습니다. 자동으로 다시 저장하지 않습니다.")}>이 확인본을 Gmail 초안으로 저장</button>}
      {p?.can_approve_send && <button disabled={disabled || busy} onClick={() => void step("approve", w, act(w, p))}>이 확인본 발송 승인</button>}
      {d.state === "approved" && <p role="status">발송이 승인되었습니다. 발송 확인이 기록되기 전에는 보냈다고 표시하지 않습니다.{d.gmail_draft_id && ` Gmail 초안 ${d.gmail_draft_id}`}</p>}
      {d.state === "receipt" && <p role="status">{d.kind === "sent" ? `발송 확인됨 (메시지 ${d.provider_message_id ?? "-"})` : `Gmail 초안에 저장됨 (초안 ${d.provider_draft_id ?? "-"}). 발송은 아직 승인되지 않았습니다.`}</p>}
      {d.state === "unknown" && <><p role="alert">{d.operation === "send" ? "발송 결과를 확인하지 못했습니다. 다시 보내지 않습니다. 보낸편지함을 확인하세요." : "Gmail 저장 결과를 확인하지 못했습니다. 아래 확인은 읽기만 하며 중복 초안을 만들지 않습니다."}</p>
        {d.operation === "gmail_draft" && d.preview && <button disabled={disabled || busy} onClick={() => void step("save-draft", w, act(w, d.preview!))}>Gmail 저장 결과 확인</button>}</>}
      {d.state === "failed" && <p role="alert">Gmail이 {d.operation === "send" ? "발송" : "저장"}을 거절했습니다. 새 확인본이 필요합니다.</p>}
      {d.state === "stale" && w.state === "local_draft" && <p role="status">{stale[d.reason] ?? `확인본을 사용할 수 없습니다 (${d.reason}).`}</p>}
      {d.state === "stale" && d.reason === "connection_changed" && onConnect && <button type="button" disabled={busy} onClick={onConnect}>Gmail 쓰기 연결</button>}
      {w.state === "local_draft" && <button disabled={disabled || busy} onClick={() => void step("discard", w, {})}>로컬 초안 삭제</button>}
    </article>;
  }
  return <section className="admin-card" aria-label="메일 회신 초안">
    <h3>메일 회신 초안</h3>
    {!completed && <p>읽기를 마친 메일이 있어야 회신 초안을 만들 수 있습니다.</p>}
    {completed && <>
      <p>{read!.result!.mailbox}에서 읽은 메일 {read!.result!.messages.length}건 중 하나를 고르세요. 초안은 이 대화에만 저장되며 모델 기록에 들어가지 않습니다.</p>
      <select aria-label="회신할 메일" value={message} onChange={e => setMessage(e.target.value)} disabled={disabled || busy}>
        {!message && <option value="">메일을 선택하세요</option>}
        {read!.result!.messages.map(m => <option key={m.id} value={m.id}>{m.subject} — {m.from}</option>)}
      </select>
      <input aria-label="받는 사람(비우면 보낸 사람)" value={to} onChange={e => setTo(e.target.value)} disabled={disabled || busy} />
      <textarea aria-label="회신 본문" value={body} onChange={e => setBody(e.target.value)} disabled={disabled || busy} />
      <button disabled={disabled || busy || !message} onClick={() => void prepare()}>로컬 회신 초안 만들기</button>
    </>}
    {status && <p role="status">{status}</p>}
    {works.map(card)}
  </section>;
}
