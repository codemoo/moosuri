// Contract for POST /threads/{thread}/work-interpret. The server's answer is a closed,
// untrusted shape: anything outside it is an error, never a guess and never "success".
import { parseGroup, type ConversationWorkGroup } from "./conversation-work";
import { parseMemoryReceiptView, type MemberMemoryReceiptView } from "./MemberMemoryReceipt";

export type WorkFocusKind = "tasks" | "status" | "evidence" | "report";
export const WORK_FOCUSES: WorkFocusKind[] = ["tasks", "status", "evidence", "report"];
export type WorkInterpretation =
  | { kind: "chat" }
  | { kind: "work"; group: ConversationWorkGroup; focus: WorkFocusKind; task_id: string | null }
  | { kind: "clarify"; message: string }
  | { kind: "preference"; view: MemberMemoryReceiptView; thread_id: string };

const MAX_MESSAGE = 600;
const obj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** `thread` is the thread the request was sent for; a group from anywhere else is rejected. */
export function parseInterpretation(v: unknown, thread: string): WorkInterpretation | null {
  if (!obj(v)) return null;
  if (v.kind === "chat") return { kind: "chat" };
  if (v.kind === "clarify") {
    return typeof v.message === "string" && v.message.trim() && v.message.length <= MAX_MESSAGE
      ? { kind: "clarify", message: v.message.trim() } : null;
  }
  if (v.kind === "preference") {
    const view = parseMemoryReceiptView(v.view);
    return view && v.thread_id === thread ? { kind: "preference", view, thread_id: thread } : null;
  }
  if (v.kind === "work") {
    const group = parseGroup(v.group);
    const focus = WORK_FOCUSES.find(f => f === v.focus);
    const taskId = v.task_id === null || v.task_id === undefined ? null : typeof v.task_id === "string" ? v.task_id : undefined;
    if (!group || group.thread_id !== thread || !focus || taskId === undefined) return null;
    if (taskId !== null && !group.tasks.some(t => t.id === taskId)) return null;
    return { kind: "work", group, focus, task_id: taskId };
  }
  return null;
}

/** Old servers have no such route; the member then keeps ordinary text behavior. */
export const interpretUnsupported = (status: number) => status === 404 || status === 405 || status === 501;
