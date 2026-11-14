/** Temporary plaintext one-use handoff. This store is neither auth nor a queue.
 * One bounded payload per browser; transactions, not messages, arbitrate owners. */
export const SHARE_TTL = 5 * 60 * 1000;
export const SHARE_DB = "moosuri-incoming-share-v1";
export type ShareSession = { account: string; scope: string };
export type SharePayload = { title: string; text: string; url: string };
type Pending = {
  id: string; created: number; expires: number; device: string; generation: string;
  intended: string; originalScope: string; state: "pending" | "claimed" | "consumed";
  window: string | null; payload?: SharePayload;
};
export type ShareRow = {
  device: string; generation: string; account: string | null; scope: string | null; originScope?: string; retiredScope?: string;
  pending?: Pending;
  tombstones?: { id: string; expires: number }[];
};
export type ShareClaim = { id: string; window: string; generation: string; epoch: number; session: ShareSession; expires: number; originalScope: string; payload: SharePayload };
let epoch = 0;
const retirementListeners = new Set<() => void>();
let channel: BroadcastChannel | null | undefined;
function retirementHint() { epoch++; for (const listener of retirementListeners) listener(); }
function retirementChannel() {
  if (channel !== undefined || !("document" in globalThis)) return channel;
  try { channel = new BroadcastChannel("moosuri-share-lifetime"); channel.onmessage = event => { if (event.data?.type === "retired") retirementHint(); }; }
  catch { channel = null; }
  return channel;
}
export function onShareRetirement(listener: () => void): () => void {
  retirementChannel(); retirementListeners.add(listener);
  return () => { retirementListeners.delete(listener); };
}
export const shareEpoch = () => epoch;
export const shareId = (v: unknown): v is string => typeof v === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v);
const unavailable = () => new Error("Share unavailable; share again.");
const fresh = (): ShareRow => ({ device: crypto.randomUUID(), generation: crypto.randomUUID(), account: null, scope: null });
function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(SHARE_DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore("handoff");
    request.onsuccess = () => resolve(request.result);
    request.onerror = request.onblocked = () => reject(unavailable());
  });
}
function valid(row: ShareRow): boolean {
  return !!row && shareId(row.device) && shareId(row.generation) &&
    (row.account === null || typeof row.account === "string") && (row.scope === null || /^[0-9a-f]{64}$/.test(row.scope));
}
async function transaction<T>(work: (row: ShareRow) => T): Promise<T> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("handoff", "readwrite");
    const store = tx.objectStore("handoff");
    const request = store.get("browser");
    let value: T;
    let failed = false;
    request.onsuccess = () => {
      try {
        const row: ShareRow = request.result ?? fresh();
        if (!valid(row)) throw unavailable();
        // Absolute TTL also rejects clock rollback/future-created corrupt records.
        if (row.pending && (!Number.isFinite(row.pending.created) || !Number.isFinite(row.pending.expires) ||
          row.pending.created > Date.now() || row.pending.expires !== row.pending.created + SHARE_TTL ||
          row.pending.expires <= Date.now())) delete row.pending;
        row.tombstones = (row.tombstones ?? []).filter(t => t.expires > Date.now());
        if (row.tombstones.length > 32) throw unavailable();
        const clean = structuredClone(row);
        try { value = work(row); } catch { failed = true; store.put(clean, "browser"); return; }
        store.put(row, "browser");
      } catch { tx.abort(); }
    };
    tx.oncomplete = () => { db.close(); if (failed) reject(unavailable()); else resolve(value); };
    tx.onerror = tx.onabort = () => { db.close(); reject(unavailable()); };
  });
}
export const readShareRow = () => transaction(row => structuredClone(row));
function rotate(row: ShareRow) { row.generation = crypto.randomUUID(); delete row.pending; delete row.tombstones; }
/** The memory fence happens before the first await. Every delayed operation also
 * checks this token; clearing in IDB rotates the cross-context CAS generation. */
export function fenceIncomingShares(clear = false, retiredScope?: string | null, expectedAccount?: string): Promise<void> {
  retirementHint();
  if (clear) { try { retirementChannel()?.postMessage({ type: "retired" }); } catch { /* hint only */ } }
  if (!clear) return Promise.resolve();
  return transaction(row => {
    if (expectedAccount && row.account && row.account !== expectedAccount) return;
    row.retiredScope = retiredScope ?? row.scope ?? row.pending?.originalScope ?? row.retiredScope; rotate(row); row.account = null; row.scope = null; delete row.originScope; });
}
export async function bindShareSession(captured: ShareRow, session: ShareSession | null, token = epoch): Promise<ShareRow> {
  return transaction(row => {
    if (token !== epoch || row.generation !== captured.generation || row.device !== captured.device) throw unavailable();
    if (session) {
      if (session.scope === row.retiredScope) throw unavailable();
      if (row.account !== null && row.account !== session.account) rotate(row);
      // A claim can never migrate to a renewed login, even for the same account.
      if (row.scope !== session.scope && row.pending?.state === "claimed") delete row.pending;
      row.account = session.account; row.scope = session.scope; row.originScope = session.scope;
    } else row.scope = null; // ordinary expiry preserves an opaque intended owner
    return structuredClone(row);
  });
}
export function stageShare(captured: ShareRow, payload: SharePayload, token = epoch): Promise<string> {
  return transaction(row => {
    if (token !== epoch || row.generation !== captured.generation || row.device !== captured.device ||
      !row.account || row.account !== captured.account || row.scope !== captured.scope ||
      !row.originScope || !/^[0-9a-f]{64}$/.test(row.originScope)) throw unavailable();
    if (!validSharePayload(payload)) throw unavailable();
    if ((row.tombstones?.length ?? 0) >= 32) throw unavailable();
    // A deliberate fresh OS share replaces the previous handoff; old claims fail.
    if (row.pending) (row.tombstones ??= []).push({ id: row.pending.id, expires: row.pending.expires });
    const id = crypto.randomUUID(), created = Date.now();
    row.pending = { id, created, expires: created + SHARE_TTL, device: row.device, generation: row.generation,
      intended: row.account, originalScope: row.originScope, state: "pending", window: null, payload };
    return id;
  });
}
export function claimShare(captured: ShareRow, id: string, window: string, session: ShareSession, token = epoch): Promise<ShareClaim> {
  return transaction(row => {
    const p = row.pending;
    if (token !== epoch || !shareId(id) || !shareId(window) || row.generation !== captured.generation ||
      row.account !== session.account || row.scope !== session.scope || !p || p.id !== id ||
      p.device !== row.device || p.generation !== row.generation || p.intended !== session.account ||
      !/^[0-9a-f]{64}$/.test(p.originalScope) || p.state !== "pending" || !p.payload || !validSharePayload(p.payload)) throw unavailable();
    p.state = "claimed"; p.window = window;
    return { id, window, generation: row.generation, epoch: token, session, expires: p.expires, originalScope: p.originalScope, payload: structuredClone(p.payload) };
  });
}
/** Payload is erased before the caller appends. A crash after this commit may
 * lose the import; replay never duplicates it. No automatic claim transfer. */
export function consumeShare(claim: ShareClaim, token = epoch): Promise<void> {
  return transaction(row => {
    const p = row.pending;
    if (token !== epoch || claim.epoch !== epoch || row.generation !== claim.generation || row.account !== claim.session.account ||
      row.scope !== claim.session.scope || !p || p.id !== claim.id || p.state !== "claimed" || p.window !== claim.window) throw unavailable();
    p.state = "consumed"; delete p.payload;
  });
}
export function validSharePayload(p: SharePayload): boolean {
  return !!p && [p.title, p.text, p.url].every(v => typeof v === "string") &&
    new TextEncoder().encode(p.title + p.text + p.url).byteLength <= 8192 && !!(p.title + p.text + p.url).trim();
}
export function shareText(p: SharePayload): string { return [p.title, p.text, p.url].filter(Boolean).join("\n"); }
export function appendShare(draft: string, text: string): string | null {
  const value = draft + (draft ? "\n\n" : "") + text;
  return text.trim() && new TextEncoder().encode(value).byteLength <= 8192 ? value : null;
}

export function discardShare(claim: ShareClaim): Promise<void> {
  return transaction(row => { if (row.generation === claim.generation && row.pending?.id === claim.id) rotate(row); });
}

/** Execute the final synchronous UI write while the current durable row is held
 * by the transaction. Do not authorize UI writes from a delayed oncomplete value. */
export function deliverShare(claim: ShareClaim, state: "claimed" | "consumed", deliver: () => void, token = epoch): Promise<void> {
  return transaction(row => {
    const p = row.pending;
    if (token !== epoch || claim.epoch !== epoch || row.generation !== claim.generation ||
      row.account !== claim.session.account || row.scope !== claim.session.scope || !p ||
      p.id !== claim.id || p.state !== state || p.window !== claim.window || p.originalScope !== claim.originalScope) throw unavailable();
    deliver();
  });
}
