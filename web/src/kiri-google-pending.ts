import type { GoogleReadIntent } from "./PersonalGoogle";

// Bounded recovery metadata only: no credentials, sealed packets, handles,
// Google addresses, URLs or codes. The server stays the authority for the job.
export type GoogleKiriPending = {
  job: string; account: string; scope: string; service: string;
  created: number; updated: number; read?: GoogleReadIntent;
};
export type GoogleKiriPendingKey = Pick<GoogleKiriPending, "account" | "scope" | "service" | "job">;

const prefix = "moosuri.google-kiri-pending.v1:";
const maxAge = 3 * 60 * 60 * 1000;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const key = (account: string, scope: string, service: string) => prefix + JSON.stringify([account, scope, service]);
const bounded = (value: unknown, max = 200): value is string => typeof value === "string" && value.length > 0 && value.length <= max;

function parse(raw: string | null, account: string, scope: string, service: string, now: number): GoogleKiriPending | null {
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as Partial<GoogleKiriPending> | null;
    if (!v || typeof v !== "object" || typeof v.job !== "string" || !uuid.test(v.job)) return null;
    if (v.account !== account || v.scope !== scope || v.service !== service) return null;
    if (typeof v.created !== "number" || typeof v.updated !== "number" || v.created > now + 60000 || now - v.created > maxAge) return null;
    let read: GoogleReadIntent | undefined;
    if (v.read !== undefined) {
      if (!v.read || !bounded(v.read.thread_id) || !bounded(v.read.request_id)) return null;
      read = { thread_id: v.read.thread_id, request_id: v.read.request_id };
    }
    return { job: v.job, account, scope, service, created: v.created, updated: v.updated, ...(read ? { read } : {}) };
  } catch {
    return null;
  }
}

/** Exact member + login + service record, or null. Corrupt/expired records are dropped. */
export function readGooglePending(account: string, scope: string | undefined, service: string, now = Date.now()) {
  if (!scope) return null;
  try {
    const k = key(account, scope, service), raw = sessionStorage.getItem(k);
    const record = parse(raw, account, scope, service, now);
    if (raw && !record && sessionStorage.getItem(k) === raw) sessionStorage.removeItem(k);
    return record;
  } catch {
    return null;
  }
}

/** True only when this exact record is durably stored. Never replaces another job's record. */
export function writeGooglePending(record: GoogleKiriPending, now = Date.now()) {
  try {
    const k = key(record.account, record.scope, record.service);
    const existing = parse(sessionStorage.getItem(k), record.account, record.scope, record.service, now);
    if (existing && existing.job !== record.job) return false;
    const value = {job:record.job,account:record.account,scope:record.scope,service:record.service,
      created:record.created,updated:now,...(record.read ? {read:{thread_id:record.read.thread_id,request_id:record.read.request_id}} : {})};
    sessionStorage.setItem(k, JSON.stringify(value));
    return readGooglePending(record.account, record.scope, record.service, now)?.job === record.job;
  } catch {
    return false;
  }
}

/** Removes the record only if it still names this job, so late older promises cannot erase newer work. */
export function clearGooglePending(record: GoogleKiriPendingKey) {
  try {
    const k = key(record.account, record.scope, record.service);
    const existing = parse(sessionStorage.getItem(k), record.account, record.scope, record.service, Date.now());
    if (existing?.job === record.job) sessionStorage.removeItem(k);
  } catch { /* storage unavailable: nothing durable to clear */ }
}
