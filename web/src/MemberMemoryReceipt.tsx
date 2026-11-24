// Same-thread receipt for a long-term preference decision. It renders only the live view the
// server projects (see crates/moosuri-server/src/personal_text/preferences.rs `View`): after a
// correction it shows the current text, after a forget it shows no text at all.
export type MemberMemoryOperation = "create" | "edit" | "forget";

export type MemberMemoryCandidate = { memory_id: string; revision: number; subject: string };

export type MemberMemoryReceiptView =
  | {
      kind: "remembered";
      operation: Exclude<MemberMemoryOperation, "forget">;
      memory_id: string;
      revision: number;
      current_revision: number;
      subject: string;
      text: string;
      superseded: boolean;
    }
  | { kind: "withdrawn"; memory_id: string }
  | { kind: "no_change"; reason: string; candidates: MemberMemoryCandidate[] }
  | { kind: "stale" };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";
const isRev = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;

/** The server reply is untrusted: anything outside the closed view shape renders nothing. */
export function parseMemoryReceiptView(v: unknown): MemberMemoryReceiptView | null {
  if (!isObj(v)) return null;
  if (v.kind === "stale") return { kind: "stale" };
  if (v.kind === "withdrawn") return isStr(v.memory_id) && v.memory_id ? { kind: "withdrawn", memory_id: v.memory_id as string } : null;
  if (v.kind === "remembered") {
    if ((v.operation !== "create" && v.operation !== "edit") || !isStr(v.memory_id) || !v.memory_id || !isRev(v.revision) ||
        !isRev(v.current_revision) || !isStr(v.subject) || !isStr(v.text) || typeof v.superseded !== "boolean") return null;
    return { kind: "remembered", operation: v.operation as "create" | "edit", memory_id: v.memory_id as string, revision: v.revision as number,
      current_revision: v.current_revision as number, subject: v.subject as string, text: v.text as string, superseded: v.superseded as boolean };
  }
  if (v.kind === "no_change") {
    if (!isStr(v.reason) || !Array.isArray(v.candidates)) return null;
    const candidates: MemberMemoryCandidate[] = [];
    for (const c of v.candidates as unknown[]) {
      if (!isObj(c) || !isStr(c.memory_id) || !isRev(c.revision) || !isStr(c.subject)) return null;
      candidates.push({ memory_id: c.memory_id as string, revision: c.revision as number, subject: c.subject as string });
    }
    return { kind: "no_change", reason: v.reason as string, candidates };
  }
  return null;
}

const NO_CHANGE: Record<string, string> = {
  one_time: "이번 한 번만 적용하는 말씀으로 이해해서 저장된 기억은 바꾸지 않았어요.",
  negated: "하지 말라는 뜻으로 이해해서 아무것도 바꾸지 않았어요.",
  not_a_request: "기억을 바꾸라는 요청이 아니라서 아무것도 바꾸지 않았어요.",
  needs_literal: "기억할 이름과 내용을 직접 적어 주세요. 아무것도 바꾸지 않았어요.",
  no_match: "같은 이름의 기억을 찾지 못했어요. 아무것도 바꾸지 않았어요.",
  stale: "그 사이 기억이 바뀌었어요. 현재 기억을 확인한 뒤 다시 말씀해 주세요.",
  ambiguous: "어떤 기억인지 하나로 정할 수 없어요. 정확한 이름으로 다시 알려 주세요.",
  exists: "같은 이름의 기억이 이미 있어요. 바꾸려면 '바꿔 줘'라고 말씀해 주세요.",
  unchanged: "이미 같은 내용으로 기억하고 있어요.",
};

export function MemberMemoryReceipt({ view, onOpenMemories }: { view: MemberMemoryReceiptView; onOpenMemories?: () => void }) {
  const open = onOpenMemories && (
    <button type="button" onClick={onOpenMemories}>기억 목록 보기</button>
  );
  if (view.kind === "remembered") {
    const verb = view.operation === "create" ? "기억했어요" : "고쳐서 기억했어요";
    return (
      <section role="status" aria-label="기억 변경 결과">
        <p>{view.superseded ? "이후 더 최근에 고친 내용이에요." : verb}</p>
        <p><strong>{view.subject}</strong></p>
        <p>{view.text}</p>
        {open}
      </section>
    );
  }
  if (view.kind === "withdrawn") {
    return (
      <section role="status" aria-label="기억 변경 결과">
        <p>이 기억은 잊었어요. 저장된 내용과 연결된 근거도 더 이상 사용하지 않아요.</p>
        {open}
      </section>
    );
  }
  if (view.kind === "stale") {
    return <section role="status" aria-label="기억 변경 결과"><p>{NO_CHANGE.stale}</p>{open}</section>;
  }
  return (
    <section role="status" aria-label="기억 변경 결과">
      <p>{NO_CHANGE[view.reason] ?? "아무것도 바꾸지 않았어요."}</p>
      {view.candidates.length > 0 && (
        <ul aria-label="해당할 수 있는 기억">
          {view.candidates.map(c => <li key={c.memory_id}>{c.subject}</li>)}
        </ul>
      )}
      {open}
    </section>
  );
}
