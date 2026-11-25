// Pure helpers for selective import. They mirror (never replace) the checks the
// Rust engine enforces, so the UI can explain a problem before any file is read.
export const MAX_ITEMS = 1000; // keep equal to moosuri_vault_core::MAX_ITEMS
export const MAX_TERMS = 8;
export const MIN_TERM_CHARS = 2;
export const MAX_TERM_BYTES = 128;
export type FilterMode = "all" | "exact" | "any";
export type TermsError = "" | "empty" | "short" | "many" | "long";

export function parseTerms(text: string): { terms: string[]; error: TermsError } {
  const seen = new Set<string>();
  const terms: string[] = [];
  for (const part of text.split(/[\n,;]/)) {
    const term = part.trim();
    if (term === "") continue;
    if ([...term].length < MIN_TERM_CHARS) return { terms: [], error: "short" };
    if (new TextEncoder().encode(term).length > MAX_TERM_BYTES) return { terms: [], error: "long" };
    const key = term.toLowerCase();
    if (!seen.has(key)) {
      seen.add(key);
      terms.push(term);
    }
  }
  if (terms.length === 0) return { terms, error: "empty" };
  if (terms.length > MAX_TERMS) return { terms: [], error: "many" };
  return { terms, error: "" };
}

export type Selector = { username: string | null; usernames: string[] | null };
export function selector(mode: FilterMode, username: string, termsText: string): { ok: true; value: Selector } | { ok: false; message: string } {
  if (mode === "all") return { ok: true, value: { username: null, usernames: null } };
  if (mode === "exact") {
    return username.trim() === ""
      ? { ok: false, message: "사용자 이름을 입력해 주세요." }
      : { ok: true, value: { username, usernames: null } };
  }
  const { terms, error } = parseTerms(termsText);
  if (error === "empty") return { ok: false, message: "포함할 글자를 한 개 이상 입력해 주세요." };
  if (error === "short") return { ok: false, message: `각 항목은 ${MIN_TERM_CHARS}글자 이상이어야 합니다.` };
  if (error === "many") return { ok: false, message: `최대 ${MAX_TERMS}개까지 입력할 수 있습니다.` };
  if (error === "long") return { ok: false, message: "입력한 글자가 너무 깁니다." };
  return { ok: true, value: { username: null, usernames: terms } };
}

// Worker-side validation of untrusted command fields before they reach Rust.
export function workerSelector(username: unknown, usernames: unknown): Selector {
  const bad = () => new Error("vault_invalid_input");
  const one = username === undefined || username === null ? null : username;
  const many = usernames === undefined || usernames === null ? null : usernames;
  if (one !== null && typeof one !== "string") throw bad();
  if (many !== null && (!Array.isArray(many) || many.length === 0 || many.length > MAX_TERMS || many.some(v => typeof v !== "string"))) throw bad();
  if (one !== null && many !== null) throw bad();
  return { username: one, usernames: many as string[] | null };
}

export type PreviewRow = { row: number; status: string };
export const selectable = (rows: PreviewRow[]) => rows.filter(r => r.status === "new" || r.status === "changed").map(r => r.row);

// Preselect every importable row only when ALL of them fit. Otherwise select
// nothing: silently keeping "the first N" would hide which rows were dropped.
export function initialSelection(rows: PreviewRow[], capacity: number): { rows: number[]; overCapacity: boolean; available: number } {
  const all = selectable(rows);
  const fits = all.length <= Math.max(0, capacity);
  return { rows: fits ? all : [], overCapacity: !fits, available: all.length };
}

// Human-readable description of what a preview covers, fixed at preview time.
export function scopeLabel(mode: FilterMode, username: string, termsText: string): string {
  if (mode === "all") return "파일의 모든 항목";
  if (mode === "exact") return `사용자 이름이 "${username}"와 정확히 같은 항목만`;
  return `사용자 이름에 ${parseTerms(termsText).terms.map(t => `"${t}"`).join(", ")} 중 하나가 포함된 항목만 (대소문자 무시)`;
}

export const formatMiB = (bytes: number) => `${(bytes / 1048576).toFixed(1)}MiB`;

export function selectionState(selected: number, existing: number, capacity: number, limit = MAX_ITEMS) {
  return { total: existing + selected, limit, over: selected > capacity, empty: selected === 0 };
}
