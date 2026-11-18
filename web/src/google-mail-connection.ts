import type { MailConnection } from "../shared/api-types";
import { announceGwsConnected, type GmailResumeTarget } from "./gws-connection";

// Only opaque IDs and a public domain-separated consent-flow tag. The server
// rederives that tag from the original session-bound handle at completion.
const prefix = "moosuri.google-mail-resume.v1.";
const idPattern = /^[a-f0-9-]{36}$/i;
const flowPattern = /^[a-f0-9]{64}$/;
export type GoogleMailResumeTarget = GmailResumeTarget & { flowId: string };
export type VerifiedGoogleMailConnection = MailConnection & { flow_id: string };
export function clearGoogleMailResume(accountId: string) {
  try { sessionStorage.removeItem(prefix + accountId); } catch { /* Optional continuity. */ }
}
export function clearGoogleMailResumeForFlow(accountId: string, flowId: string | null) {
  if (flowId && pendingGoogleMailResume(accountId)?.flowId === flowId) clearGoogleMailResume(accountId);
}
export function beginGoogleMailResume(accountId: string, target?: GmailResumeTarget, flowId?: string) {
  clearGoogleMailResume(accountId);
  if (!target || !idPattern.test(target.threadId) || !idPattern.test(target.runId) || !flowId || !flowPattern.test(flowId)) return;
  try { sessionStorage.setItem(prefix + accountId, JSON.stringify({ ...target, flowId, at: Date.now() })); } catch { /* Connect without continuation. */ }
}
export function pendingGoogleMailResume(accountId: string): GoogleMailResumeTarget | null {
  try {
    const value = JSON.parse(sessionStorage.getItem(prefix + accountId) ?? "null");
    if (value && idPattern.test(value.threadId) && idPattern.test(value.runId) && flowPattern.test(value.flowId)
      && Number.isFinite(value.at) && value.at <= Date.now() && Date.now() - value.at < 30 * 60 * 1000)
      return { threadId: value.threadId, runId: value.runId, flowId: value.flowId };
  } catch { /* Missing or malformed state cannot authorize a read. */ }
  clearGoogleMailResume(accountId);
  return null;
}
export async function resumeGoogleMail(accountId: string, target: GoogleMailResumeTarget, verified: VerifiedGoogleMailConnection,
  request: <T>(path: string, method?: string, body?: unknown) => Promise<T>): Promise<boolean> {
  const current = pendingGoogleMailResume(accountId);
  if (!current || current.threadId !== target.threadId || current.runId !== target.runId || current.flowId !== target.flowId
    || !flowPattern.test(verified.flow_id) || verified.flow_id !== target.flowId) return false;
  // An older callback cannot consume a newer flow's marker. Once matching,
  // consume before transport; canonical server submission is once-only too.
  clearGoogleMailResumeForFlow(accountId, verified.flow_id);
  if (!verified.id || !Number.isSafeInteger(verified.epoch) || verified.epoch < 1 || verified.status !== "active")
    throw new Error("Google 연결 상태를 확인하지 못했습니다. 대화에서 메일 조회를 새로 요청하세요.");
  await request(`/threads/${encodeURIComponent(target.threadId)}/runs/${encodeURIComponent(target.runId)}/resume-gmail`, "POST", {
    connection_id: verified.id, connection_epoch: verified.epoch,
  });
  announceGwsConnected(accountId, target);
  return true;
}
