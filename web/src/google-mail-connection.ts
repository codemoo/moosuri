import type { MailConnection } from "../shared/api-types";
import { announceGwsConnected, type GmailResumeTarget } from "./gws-connection";

// Explicit eligible chat clicks retain only opaque IDs, never request text or
// OAuth credentials. Ordinary settings connections do not create this marker.
const prefix = "moosuri.google-mail-resume.v1.";
const idPattern = /^[a-f0-9-]{36}$/i;
export function clearGoogleMailResume(accountId: string) {
  try { sessionStorage.removeItem(prefix + accountId); } catch { /* Optional continuity. */ }
}
export function beginGoogleMailResume(accountId: string, target?: GmailResumeTarget) {
  clearGoogleMailResume(accountId);
  if (!target || !idPattern.test(target.threadId) || !idPattern.test(target.runId)) return;
  try { sessionStorage.setItem(prefix + accountId, JSON.stringify({ ...target, at: Date.now() })); } catch { /* Connect without continuation. */ }
}
export function pendingGoogleMailResume(accountId: string): GmailResumeTarget | null {
  try {
    const value = JSON.parse(sessionStorage.getItem(prefix + accountId) ?? "null");
    if (value && idPattern.test(value.threadId) && idPattern.test(value.runId)
      && Number.isFinite(value.at) && value.at <= Date.now() && Date.now() - value.at < 30 * 60 * 1000)
      return { threadId: value.threadId, runId: value.runId };
  } catch { /* Missing or malformed state cannot authorize a read. */ }
  clearGoogleMailResume(accountId);
  return null;
}
export async function resumeGoogleMail(accountId: string, target: GmailResumeTarget, verified: MailConnection,
  request: <T>(path: string, method?: string, body?: unknown) => Promise<T>) {
  const current = pendingGoogleMailResume(accountId);
  if (!current || current.threadId !== target.threadId || current.runId !== target.runId) return;
  // Consume before transport. The server independently enforces newest turn,
  // account, exact read intent and once-only child execution across tabs.
  clearGoogleMailResume(accountId);
  if (!verified.id || !Number.isSafeInteger(verified.epoch) || verified.epoch < 1 || verified.status !== "active")
    throw new Error("Google 연결 상태를 확인하지 못했습니다. 대화에서 메일 조회를 새로 요청하세요.");
  await request(`/threads/${encodeURIComponent(target.threadId)}/runs/${encodeURIComponent(target.runId)}/resume-gmail`, "POST", {
    connection_id: verified.id, connection_epoch: verified.epoch,
  });
  announceGwsConnected(accountId, target);
}
