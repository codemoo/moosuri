/** Tab-session plaintext recovery, never an authentication capability or send queue.
 * Storage is touched only after server authentication. No localStorage/cache/media bodies. */
import type { SessionView } from "../shared/api-types";
import type { ChatMemory, PendingSend } from "./ChatPanel";
export const CHAT_DRAFT_KEY = "moosuri.chat-drafts.v1";
export const CHAT_DRAFT_TTL = 8 * 60 * 60 * 1000;
export const CHAT_DRAFT_MAX_BYTES = 128 * 1024;
export const CHAT_DRAFT_MAX_SLOTS = 16;
export type DraftStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;
type Snapshot = {
    memory: ChatMemory;
    selected: string | null;
    restored: boolean;
};
const empty = (): Snapshot => ({ memory: { drafts: {}, pending: {} }, selected: null, restored: false });
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const bytes = (v: string) => new TextEncoder().encode(v).byteLength;
const exact = (v: Record<string, unknown>, keys: string[]) => Object.keys(v).every(k => keys.includes(k));
const slot = (v: unknown): v is string => typeof v === "string" && /^[a-zA-Z0-9_-]{1,256}$/.test(v) && !["__proto__", "prototype", "constructor"].includes(v);
const uuid = (v: unknown): v is string => typeof v === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v);
const revision = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v > 0;
const target = (v: unknown): v is {
    id: string;
    revision: number;
} => object(v) && exact(v, ["id", "revision"]) && uuid(v.id) && revision(v.revision);
const text = (v: unknown): v is string => typeof v === "string" && bytes(v) <= 8192;
export function draftScope(session: SessionView): string | null {
    return !session.password_change_required && !session.user.must_change_password &&
        typeof session.chat_draft_scope === "string" && /^[0-9a-f]{64}$/.test(session.chat_draft_scope)
        ? session.chat_draft_scope : null;
}
function zone(v: unknown): v is string {
    if (typeof v !== "string" || v.length > 64 || !v)
        return false;
    try {
        new Intl.DateTimeFormat("en", { timeZone: v });
        return true;
    }
    catch {
        return false;
    }
}
/** Rebuild a closed carrier. Persisted bytes never authorize an automatic POST. */
function pending(v: unknown): PendingSend | null {
    if (!object(v) || !exact(v, ["request_id", "text", "attachments", "artifact_target", "schedule_context", "mission_context", "status"]) ||
        !uuid(v.request_id) || !text(v.text) || !v.text.trim() || !["sending", "accepted", "uncertain"].includes(String(v.status)))
        return null;
    const out: PendingSend = { request_id: v.request_id, text: v.text, status: "uncertain" };
    if (v.attachments !== undefined) {
        if (!Array.isArray(v.attachments) || v.attachments.length > 4 || !v.attachments.every(target) || new Set(v.attachments.map(x => x.id)).size !== v.attachments.length)
            return null;
        out.attachments = v.attachments.map(x => ({ id: x.id, revision: x.revision }));
    }
    if (v.artifact_target !== undefined) {
        if (!target(v.artifact_target))
            return null;
        out.artifact_target = { id: v.artifact_target.id, revision: v.artifact_target.revision };
    }
    if (v.schedule_context !== undefined) {
        const c = v.schedule_context;
        if (!object(c) || !exact(c, ["timezone", "target"]) || !zone(c.timezone) || !(c.target === null || target(c.target)))
            return null;
        out.schedule_context = { timezone: c.timezone, target: c.target === null ? null : { id: c.target.id, revision: c.target.revision } };
    }
    if (v.mission_context !== undefined) {
        const c = v.mission_context;
        if (!object(c) || !exact(c, ["target"]))
            return null;
        if (c.target === null)
            out.mission_context = { target: null };
        else {
            const t = c.target;
            if (!object(t) || !exact(t, ["kind", "id", "revision"]) || !["one_off", "recurring"].includes(String(t.kind)) || !uuid(t.id) || !revision(t.revision))
                return null;
            out.mission_context = { target: { kind: String(t.kind), id: t.id, revision: t.revision } };
        }
    }
    if ([!!out.artifact_target, !!out.schedule_context?.target, !!out.mission_context?.target].filter(Boolean).length > 1)
        return null;
    return out;
}
function storage(): DraftStorage | null { try {
    return window.sessionStorage;
}
catch {
    return null;
} }
function remove(s: DraftStorage | null) { try {
    s?.removeItem(CHAT_DRAFT_KEY);
}
catch { /* Restricted storage is optional. */ } }
/** An irreversible in-memory lease prevents late responses/unmounts repopulating
 * storage after logout, even when the network logout request fails. */
export class ChatDraftSession {
    private active = true;
    private failed = false;
    constructor(readonly account: string, readonly scope: string, private readonly store: DraftStorage | null, private readonly clock: () => number) { }
    close() { this.active = false; }
    load(threads: string[]): Snapshot {
        if (!this.active || this.failed)
            return empty();
        try {
            const raw = this.store?.getItem(CHAT_DRAFT_KEY);
            if (!raw)
                return empty();
            if (bytes(raw) > CHAT_DRAFT_MAX_BYTES)
                throw new Error();
            const v: unknown = JSON.parse(raw), now = this.clock();
            if (!object(v) || !exact(v, ["version", "account", "scope", "saved_at", "selected", "slots"]) || v.version !== 1 || v.account !== this.account || v.scope !== this.scope ||
                typeof v.saved_at !== "number" || !Number.isSafeInteger(v.saved_at) || v.saved_at < 0 || v.saved_at > now || now - v.saved_at >= CHAT_DRAFT_TTL ||
                !(v.selected === null || slot(v.selected)) || !Array.isArray(v.slots) || v.slots.length > CHAT_DRAFT_MAX_SLOTS)
                throw new Error();
            const out = empty(), seen = new Set<string>(), allowed = new Set(["new", ...threads]);
            for (const row of v.slots) {
                if (!object(row) || !exact(row, ["id", "draft", "pending"]) || !slot(row.id) || seen.has(row.id) || (row.draft !== undefined && !text(row.draft)))
                    throw new Error();
                seen.add(row.id);
                if (!allowed.has(row.id))
                    continue;
                if (row.draft)
                    out.memory.drafts[row.id] = row.draft;
                if (row.pending !== undefined) {
                    const p = pending(row.pending);
                    if (p)
                        out.memory.pending[row.id] = p;
                }
            }
            out.selected = v.selected !== null && threads.includes(v.selected) ? v.selected : null;
            out.restored = true;
            return out;
        }
        catch {
            remove(this.store);
            return empty();
        }
    }
    save(memory: ChatMemory, selected: string | null, threads: string[]) {
        if (!this.active || this.failed || !this.store)
            return;
        try {
            const allowed = new Set(["new", ...threads]);
            const ids = [...new Set([...(selected ? [selected] : []), ...Object.keys(memory.pending), ...Object.keys(memory.drafts)])].filter(id => slot(id) && allowed.has(id));
            const rows: Array<{
                id: string;
                draft?: string;
                pending?: PendingSend;
            }> = [];
            const envelope = () => JSON.stringify({ version: 1, account: this.account, scope: this.scope, saved_at: this.clock(), selected: selected && threads.includes(selected) ? selected : null, slots: rows });
            for (const id of ids) {
                if (rows.length >= CHAT_DRAFT_MAX_SLOTS)
                    break;
                const row: {
                    id: string;
                    draft?: string;
                    pending?: PendingSend;
                } = { id };
                const draft = memory.drafts[id];
                if (text(draft) && draft)
                    row.draft = draft;
                const p = memory.pending[id];
                // Local attachment selection handles are UI lifetime state, not immutable
                // server attachment identities; don't persist them (or media bytes).
                if (p) {
                    const { attachment_selections: _, ...carrier } = p;
                    const restored = pending(carrier);
                    if (restored)
                        row.pending = restored;
                }
                if (!row.draft && !row.pending)
                    continue;
                rows.push(row);
                if (bytes(envelope()) > CHAT_DRAFT_MAX_BYTES)
                    rows.pop();
            }
            if (!rows.length && !selected)
                remove(this.store);
            else
                this.store.setItem(CHAT_DRAFT_KEY, envelope());
        }
        catch {
            this.failed = true;
            remove(this.store);
        }
    }
}
export class ChatDraftRecovery {
    private current: ChatDraftSession | null = null;
    constructor(private readonly getStorage: () => DraftStorage | null = storage, private readonly clock: () => number = Date.now) { }
    clear() { this.current?.close(); this.current = null; try {
        remove(this.getStorage());
    }
    catch { /* Optional storage. */ } }
    confirm(session: SessionView): ChatDraftSession | null {
        const scope = draftScope(session);
        this.current?.close();
        this.current = null;
        if (!scope || !slot(session.user.id)) {
            this.clear();
            return null;
        }
        let s: DraftStorage | null = null;
        try {
            s = this.getStorage();
        }
        catch { /* Optional storage. */ }
        // An authenticated replacement retires old plaintext immediately, even if
        // its thread inventory cannot currently be loaded. Never restore by account
        // name alone, and never postpone this fence until a workspace GET finishes.
        try {
            const raw = s?.getItem(CHAT_DRAFT_KEY);
            if (raw) {
                if (bytes(raw) > CHAT_DRAFT_MAX_BYTES)
                    throw new Error();
                const v: unknown = JSON.parse(raw);
                if (!object(v) || v.account !== session.user.id || v.scope !== scope)
                    remove(s);
            }
        }
        catch {
            remove(s);
        }
        this.current = new ChatDraftSession(session.user.id, scope, s, this.clock);
        return this.current;
    }
}
