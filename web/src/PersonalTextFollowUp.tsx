import { useRef } from "react";
import type { RunInstructionReceipt, RunView } from "../shared/api-types";
import { ApiFailure, isAborted, type RequestScope } from "./api";
import type { ChatMemory } from "./ChatPanel";
import { instructionBytes, sameInstruction, validInstructionReceipt, type PendingInstruction } from "./run-instructions";

export const isTextFollowUp = (r: RunInstructionReceipt) =>
  r.reason === "text_follow_up_saved" || r.reason === "text_follow_up_sent";

export function clearSavedFollowUp(m: ChatMemory, request: PendingInstruction): ChatMemory {
  if (m.instructions?.[request.thread_id]?.pending?.request_id !== request.request_id) return m;
  const instructions = { ...m.instructions };
  delete instructions[request.thread_id];
  return { ...m, instructions };
}

export function PersonalTextFollowUp({ thread, run, draft, pending, rejected, scope, change, refresh, expired }: {
  thread: string; run?: RunView; draft: string; pending?: PendingInstruction; rejected?: string;
  scope: Pick<RequestScope, "request">; change: (edit: (m: ChatMemory) => ChatMemory) => void;
  refresh: () => void; expired: () => void;
}) {
  const inFlight = useRef(false);
  if (!run && !pending && !rejected) return null;
  async function save() {
    if (inFlight.current || (!pending && (!run || !draft.trim() || instructionBytes(draft) > 2000))) return;
    const request: PendingInstruction = pending ?? {
      thread_id: thread, run_id: run!.id, attempt: run!.attempt,
      request_id: crypto.randomUUID(), text: draft, status: "sending",
    };
    inFlight.current = true;
    change(m => ({
      ...m, drafts: !pending && m.drafts[thread] === draft ? { ...m.drafts, [thread]: "" } : m.drafts,
      instructions: { ...m.instructions, [thread]: { draft: "", pending: { ...request, status: "sending" } } },
    }));
    try {
      const receipt = await scope.request<unknown>(`/threads/${encodeURIComponent(thread)}/text-follow-ups`, "POST", {
        run_id: request.run_id,
        instruction: { request_id: request.request_id, expected_attempt: request.attempt, text: request.text },
      }, undefined, { timeoutMs: 15000 });
      if (!validInstructionReceipt(receipt) || !isTextFollowUp(receipt) || !sameInstruction(receipt, request)) {
        throw new Error("unconfirmed receipt");
      }
      change(m => clearSavedFollowUp(m, request));
      refresh();
    } catch (e) {
      if (isAborted(e)) return;
      if (e instanceof ApiFailure && e.status === 401) { expired(); return; }
      if (e instanceof ApiFailure && [400, 403, 404, 409, 413, 422, 429].includes(e.status)) {
        change(m => m.instructions?.[thread]?.pending?.request_id !== request.request_id ? m : ({
          ...m, instructions: { ...m.instructions, [thread]: { draft: request.text } },
        }));
        refresh();
        return;
      }
      change(m => m.instructions?.[thread]?.pending?.request_id !== request.request_id ? m : ({
        ...m, instructions: { ...m.instructions, [thread]: { draft: "", pending: { ...request, status: "uncertain" } } },
      }));
      refresh();
    } finally { inFlight.current = false; }
  }
  if (rejected) return <div className="member-chat-notice">
    <p>{rejected}</p><p role="status">이 답변에 후속 지시를 저장하지 못했습니다.</p>
    <button type="button" disabled={!!draft} onClick={() => change(m => {
      if (m.drafts[thread] || m.instructions?.[thread]?.draft !== rejected) return m;
      const instructions = { ...m.instructions };
      delete instructions[thread];
      return { ...m, instructions, drafts: { ...m.drafts, [thread]: rejected } };
    })}>작성창으로 가져오기</button>
  </div>;
  if (pending) return <div className="member-chat-notice">
    <p>{pending.text}</p>
    <p role="status">{pending.status === "sending" ? "후속 지시 저장 중…" : "후속 지시가 저장됐는지 확인하지 못했습니다."}</p>
    {pending.status === "uncertain" && <>
      <button type="button" onClick={() => void save()}>같은 지시 저장 다시 확인</button>
      <button type="button" disabled={!!draft} onClick={() => change(m =>
        m.drafts[thread] || m.instructions?.[thread]?.pending?.request_id !== pending.request_id ? m :
          { ...clearSavedFollowUp(m, pending), drafts: { ...m.drafts, [thread]: pending.text } })}>작성창으로 가져오기</button>
    </>}
  </div>;
  return <div className="member-chat-notice">
    <p>작성 중인 답변에 덧붙일 내용은 후속 지시로 저장해 두었다가 답변이 끝난 뒤 보낼 수 있습니다.</p>
    <button type="button" disabled={!draft.trim() || instructionBytes(draft) > 2000} onClick={() => void save()}>후속 지시 저장</button>
    {instructionBytes(draft) > 2000 && <span role="status">후속 지시는 2,000바이트까지 저장할 수 있습니다.</span>}
  </div>;
}
