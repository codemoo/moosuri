import type { RunInstructionReceipt } from "../shared/api-types";
export type InstructionTarget = { run_id: string; thread_id: string; attempt: number };
export type PendingInstruction = InstructionTarget & { request_id: string; text: string; status: "sending" | "uncertain" };
export type InstructionMemory = { mode?: "instructions" | "next"; draft: string; target?: InstructionTarget; pending?: PendingInstruction };
export const emptyInstructions = (): InstructionMemory => ({ draft: "" });
export const instructionBytes = (text: string) => new TextEncoder().encode(text).byteLength;
const object = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
const id = (v: unknown): v is string => typeof v === "string" && !!v && v.length <= 256;
const integer = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v > 0;
export function validInstructionReceipt(value: unknown): value is RunInstructionReceipt {
  return object(value) && id(value.request_id) && id(value.run_id) && id(value.thread_id) && integer(value.attempt) && integer(value.sequence) &&
    typeof value.text === "string" && instructionBytes(value.text) <= 2000 && ["received", "applied", "closed"].includes(String(value.state)) &&
    (value.reason === null || (typeof value.reason === "string" && value.reason.length <= 128));
}
export function sameInstruction(receipt: RunInstructionReceipt, pending: PendingInstruction) {
  return receipt.request_id === pending.request_id && receipt.run_id === pending.run_id && receipt.thread_id === pending.thread_id && receipt.attempt === pending.attempt && receipt.text === pending.text;
}
export function restoreInstruction(state: InstructionMemory, pending: PendingInstruction): InstructionMemory {
  if (state.pending?.request_id !== pending.request_id) return state;
  return { ...state, pending: undefined, mode: "instructions", target: { run_id: pending.run_id, thread_id: pending.thread_id, attempt: pending.attempt }, draft: pending.text + (state.draft ? `\n\n${state.draft}` : "") };
}
