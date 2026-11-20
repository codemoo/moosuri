export type Change = "promote" | "demote" | "suspend" | "resume";
export type ChangeRequest = {
  request_id: string; account_id: string; generation: number; revision: number; action: Change;
};
type Invitation = { request_id: string; name: string; email: string };
export type InvitationReceipt = Pick<Invitation, "request_id" | "email">;
export type PendingChange = { request: ChangeRequest; email: string };
export type AdminRecord =
  | { kind: "invite"; invitation: Invitation }
  | { kind: "receipt"; receipt: InvitationReceipt }
  | { kind: "change" | "changed"; change: PendingChange };

const key = "moosuri.personal.admin-operation.v1";
const uuid = (s: unknown): s is string => typeof s === "string" && /^[a-f0-9]{8}(-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(s);
const object = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;
const field = (s: unknown, max: number): s is string => typeof s === "string" && !!s.trim() && s.length <= max;
function valid(value: unknown): value is AdminRecord {
  if (!object(value)) return false;
  if (value.kind === "invite" || value.kind === "receipt") {
    const data = value.kind === "invite" ? value.invitation : value.receipt;
    return object(data) && uuid(data.request_id) && field(data.email, 254)
      && (value.kind !== "invite" || field(data.name, 200));
  }
  if (value.kind !== "change" && value.kind !== "changed") return false;
  const data = value.change;
  if (!object(data) || !field(data.email, 254) || !object(data.request)) return false;
  const req = data.request;
  return uuid(req.request_id) && uuid(req.account_id) && typeof req.action === "string"
    && ["promote", "demote", "suspend", "resume"].includes(req.action)
    && Number.isSafeInteger(req.generation) && Number(req.generation) > 0
    && Number.isSafeInteger(req.revision) && Number(req.revision) >= 0;
}

export function clearAdminRecovery() {
  try { window.sessionStorage.removeItem(key); } catch { /* Storage may be disabled. */ }
}
export function readAdminRecovery(account: string, scope?: string | null): AdminRecord | null {
  try {
    const raw = window.sessionStorage.getItem(key);
    if (!raw) return null;
    if (raw.length <= 8192 && scope) {
      const data = JSON.parse(raw);
      if (data?.account === account && data.scope === scope && valid(data.record)) return data.record;
    }
  } catch { /* Start with the fresh roster. */ }
  clearAdminRecovery();
  return null;
}
export function saveAdminRecovery(account: string, scope: string | null | undefined, record: AdminRecord) {
  if (!scope) return;
  try { window.sessionStorage.setItem(key, JSON.stringify({ account, scope, record })); }
  catch { /* Manual checks remain available in this view. */ }
}
