// Continuity contains only opaque IDs, never the user's request or Google data.
// A marker is created only by an explicit connect click and expires in 30 minutes.
export const GWS_CONNECTED = "moosuri:gws-inbox-connected";
const prefix = "moosuri.gws-connect.v1.";
const ttl = 30 * 60 * 1000;
const idPattern = /^[a-f0-9-]{36}$/i;
export type GmailResumeTarget = { threadId: string; runId: string };
export type PendingGws = { target: GmailResumeTarget | null; at: number };
const memory = new Map<string, PendingGws>();
const volatileAccounts = new Set<string>();
export function pendingGws(accountId: string): PendingGws | null {
  try {
    const raw = volatileAccounts.has(accountId) ? null : localStorage.getItem(prefix + accountId);
    const value = volatileAccounts.has(accountId) ? memory.get(accountId) : raw ? JSON.parse(raw) : null;
    if (value && Number.isFinite(value.at) && value.at <= Date.now() && Date.now() - value.at < ttl
      && (value.target === null || (idPattern.test(value.target?.threadId) && idPattern.test(value.target?.runId))))
      return value;
  } catch { /* Disabled storage still permits the current explicit workflow. */
    const value = memory.get(accountId);
    if (value && value.at <= Date.now() && Date.now() - value.at < ttl) return value;
  }
  clearPendingGws(accountId);
  return null;
}
export function beginGws(accountId: string, target?: GmailResumeTarget): PendingGws {
  const value = { target: target ?? null, at: Date.now() };
  memory.set(accountId, value);
  try { localStorage.setItem(prefix + accountId, JSON.stringify(value)); volatileAccounts.delete(accountId); }
  catch { volatileAccounts.add(accountId); /* Keep this page usable without durable storage. */ }
  return value;
}
export function clearPendingGws(accountId: string) {
  memory.delete(accountId);
  volatileAccounts.delete(accountId);
  try { localStorage.removeItem(prefix + accountId); } catch { /* Optional continuity. */ }
}
export function samePending(a: PendingGws | null, b: PendingGws | null) {
  return !!a && !!b && a.at === b.at && a.target?.threadId === b.target?.threadId && a.target?.runId === b.target?.runId;
}
// A single page can render several historical recovery cards. Only one may own
// the pending account workflow; duplicate tabs remain safe at the server receipt.
const owners = new Set<string>();
export function claimGws(accountId: string) {
  if (owners.has(accountId)) return false;
  owners.add(accountId);
  return true;
}
export function releaseGws(accountId: string) { owners.delete(accountId); }
export function announceGwsConnected(accountId: string, target: GmailResumeTarget) {
  window.dispatchEvent(new CustomEvent(GWS_CONNECTED, { detail: { accountId, ...target } }));
}
export function gwsConnectedTarget(event: Event, accountId: string): GmailResumeTarget | null {
  const detail = (event as CustomEvent).detail;
  return detail?.accountId === accountId && typeof detail.threadId === "string" && typeof detail.runId === "string"
    && idPattern.test(detail.threadId) && idPattern.test(detail.runId)
    ? { threadId: detail.threadId, runId: detail.runId } : null;
}
