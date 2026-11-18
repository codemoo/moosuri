// Only an account-owned conversation ID is retained across Google's redirect.
// Never store login fields, OAuth handles or authorization URLs here.
const prefix = "moosuri.gmail-return.v1.";
const idPattern = /^[a-f0-9-]{36}$/i;
export function rememberMailReturn(accountId: string, threadId: string | null) {
  try {
    if (threadId && idPattern.test(threadId))
      sessionStorage.setItem(prefix + accountId, JSON.stringify({ threadId, at: Date.now() }));
  } catch { /* Connection still works when tab storage is unavailable. */ }
}
export function mailReturnThread(accountId: string): string | null {
  try {
    const value = JSON.parse(sessionStorage.getItem(prefix + accountId) ?? "null");
    if (value && typeof value.threadId === "string" && idPattern.test(value.threadId)
      && Number.isFinite(value.at) && value.at <= Date.now() && Date.now() - value.at < 3600000)
      return value.threadId;
    sessionStorage.removeItem(prefix + accountId);
  } catch { /* Invalid or disabled tab storage has no return target. */ }
  return null;
}
export function clearMailReturn(accountId: string) {
  try { sessionStorage.removeItem(prefix + accountId); } catch { /* Optional continuity. */ }
}
