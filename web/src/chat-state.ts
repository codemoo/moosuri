import type {
  AssistantMessage,
  ChatEvent,
  ChatPage,
  ChatTurn,
  MessagePhase,
  ModelChoice,
  RunUsage,
  RunView,
} from "../shared/api-types";
import { StreamFailure } from "./event-stream";

const object = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === "object" && !Array.isArray(value);
const integer = (value: unknown, minimum = 0): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= minimum;
const id = (value: unknown): value is string =>
  typeof value === "string" && value.length > 0 && value.length <= 256;
const phase = (value: unknown): value is MessagePhase | null =>
  value === null || value === "commentary" || value === "final_answer";
const states = new Set([
  "queued",
  "running",
  "completed",
  "failed",
  "cancelled",
  "incomplete",
  "refused",
  "outcome_unknown",
]);
export const active = (run: RunView) =>
  run.state === "queued" || run.state === "running";
export function validUsage(value: unknown): value is RunUsage {
  return (
    object(value) &&
    [
      "input_tokens",
      "output_tokens",
      "cache_read_tokens",
      "cache_write_tokens",
    ].every((key) => value[key] === null || integer(value[key]))
  );
}
export function validChoice(value: unknown): value is ModelChoice {
  return (
    object(value) &&
    id(value.provider_id) &&
    integer(value.provider_revision, 1) &&
    id(value.provider_name) &&
    id(value.model) &&
    (value.effort === null || id(value.effort)) &&
    ["open_ai", "anthropic", "open_router", "compatible"].includes(
      String(value.kind),
    ) &&
    integer(value.verified_at)
  );
}
export function validRun(value: unknown): value is RunView {
  return (
    object(value) &&
    id(value.id) &&
    id(value.request_id) &&
    id(value.thread_id) &&
    integer(value.attempt, 1) &&
    integer(value.version) &&
    states.has(String(value.state)) &&
    validChoice(value.model) &&
    (value.error === null ||
      (typeof value.error === "string" && value.error.length <= 128)) &&
    validUsage(value.usage) &&
    typeof value.can_retry === "boolean" &&
    typeof value.can_cancel === "boolean" &&
    integer(value.created_at)
  );
}
function validMessage(value: unknown): value is AssistantMessage {
  return (
    object(value) &&
    id(value.id) &&
    typeof value.text === "string" &&
    value.text.length <= 1024 * 1024 &&
    phase(value.phase) &&
    typeof value.refusal === "boolean"
  );
}
export function validTurn(value: unknown): value is ChatTurn {
  return (
    object(value) &&
    integer(value.sequence, 1) &&
    typeof value.user_text === "string" &&
    new TextEncoder().encode(value.user_text).byteLength <= 8192 &&
    Array.isArray(value.assistant) &&
    value.assistant.length <= 256 &&
    value.assistant.every(validMessage) &&
    new Set(value.assistant.map((item) => item.id)).size ===
      value.assistant.length &&
    validRun(value.run)
  );
}
export function parsePage(value: unknown, threadId: string): ChatPage {
  if (
    !object(value) ||
    value.thread_id !== threadId ||
    !integer(value.cursor) ||
    !Array.isArray(value.turns) ||
    value.turns.length > 200 ||
    !value.turns.every(validTurn) ||
    !value.turns.every((turn) => turn.run.thread_id === threadId) ||
    !(value.next_before === null || integer(value.next_before, 1)) ||
    new Set(value.turns.map((turn) => turn.run.id)).size !==
      value.turns.length ||
    new Set(value.turns.map((turn) => turn.sequence)).size !==
      value.turns.length
  )
    throw new StreamFailure();
  return value as unknown as ChatPage;
}
export function parseEvent(value: unknown, frameId: string | null): ChatEvent {
  if (
    !object(value) ||
    !integer(value.sequence, 1) ||
    frameId !== String(value.sequence) ||
    !object(value.change)
  )
    throw new StreamFailure();
  const change = value.change;
  let valid = false;
  if (change.kind === "turn") valid = validTurn(change.turn);
  else if (change.kind === "state") valid = validRun(change.run);
  else if (change.kind === "delta" || change.kind === "phase") {
    valid =
      id(change.run_id) &&
      integer(change.attempt, 1) &&
      id(change.message_id) &&
      phase(change.phase);
    if (change.kind === "delta")
      valid =
        valid &&
        typeof change.text === "string" &&
        change.text.length <= 512 * 1024 &&
        typeof change.refusal === "boolean";
  }
  if (!valid) throw new StreamFailure();
  return value as unknown as ChatEvent;
}

/** Cancel/retry action responses must not reopen terminal or older attempts.
 * Message POST responses need a fresh cursor-bound ChatPage instead: this helper
 * cannot deduplicate SSE deltas already represented in a POST text snapshot. */
export function acknowledge(turns: ChatTurn[], incoming: ChatTurn): ChatTurn[] {
  const current = turns.find((turn) => turn.run.id === incoming.run.id);
  if (!current)
    return [...turns, incoming].sort((a, b) => a.sequence - b.sequence);
  if (
    incoming.run.attempt < current.run.attempt ||
    (incoming.run.attempt === current.run.attempt &&
      incoming.run.version < current.run.version)
  )
    return turns;
  if (incoming.run.attempt > current.run.attempt)
    return turns.map((turn) => (turn === current ? incoming : turn));
  const assistant = incoming.assistant.map((message) => {
    const streamed = current.assistant.find((old) => old.id === message.id);
    if (!streamed) return message;
    if (streamed.text.startsWith(message.text))
      return {
        ...streamed,
        phase: message.phase ?? streamed.phase,
        refusal: message.refusal || streamed.refusal,
      };
    return message;
  });
  for (const message of current.assistant)
    if (!assistant.some((item) => item.id === message.id))
      assistant.push(message);
  return turns.map((turn) =>
    turn === current ? { ...incoming, assistant } : turn,
  );
}

/** Joins delta fragments once per UI batch, never once per token or full reply
 * snapshot. Higher-attempt Turn events replace the previous assistant snapshot. */
export function applyEvents(
  source: ChatTurn[],
  events: ChatEvent[],
  threadId: string,
): { turns: ChatTurn[]; needsSnapshot: boolean } {
  const turns = new Map(
    source.map((turn) => [
      turn.run.id,
      { ...turn, assistant: [...turn.assistant] },
    ]),
  );
  const buffers = new Map<
    string,
    {
      turn: ChatTurn;
      message: AssistantMessage;
      parts: string[];
      bytes: number;
    }
  >();
  let needsSnapshot = false;
  function flush(runId?: string) {
    for (const [key, value] of buffers) {
      if (runId && value.turn.run.id !== runId) continue;
      value.message.text += value.parts.join("");
      buffers.delete(key);
    }
  }
  for (const { change } of events) {
    if (change.kind === "turn") {
      if (change.turn.run.thread_id !== threadId) {
        needsSnapshot = true;
        break;
      }
      const old = turns.get(change.turn.run.id);
      if (
        old &&
        (change.turn.run.attempt < old.run.attempt ||
          (change.turn.run.attempt === old.run.attempt &&
            change.turn.run.version < old.run.version))
      )
        continue;
      flush(change.turn.run.id);
      turns.set(change.turn.run.id, {
        ...change.turn,
        assistant: change.turn.assistant.map((message) => ({ ...message })),
      });
      continue;
    }
    const runId = change.kind === "state" ? change.run.id : change.run_id;
    const turn = turns.get(runId);
    const attempt =
      change.kind === "state" ? change.run.attempt : change.attempt;
    if (!turn || attempt > turn.run.attempt) {
      needsSnapshot = true;
      break;
    }
    if (attempt < turn.run.attempt) continue;
    if (change.kind === "state") {
      if (change.run.thread_id !== threadId) {
        needsSnapshot = true;
        break;
      }
      if (change.run.version <= turn.run.version) continue;
      flush(runId);
      turn.run = change.run;
      continue;
    }
    if (!active(turn.run)) continue;
    let message = turn.assistant.find(
      (message) => message.id === change.message_id,
    );
    const key = JSON.stringify([runId, attempt, change.message_id]);
    let buffer = buffers.get(key);
    if (!buffer) {
      message = message
        ? { ...message }
        : { id: change.message_id, text: "", phase: null, refusal: false };
      const index = turn.assistant.findIndex((old) => old.id === message!.id);
      if (index < 0) turn.assistant.push(message);
      else turn.assistant[index] = message;
      buffer = { turn, message, parts: [], bytes: message.text.length };
      buffers.set(key, buffer);
    }
    if (change.kind === "phase" || change.phase !== null)
      buffer.message.phase = change.phase;
    if (change.kind === "delta") {
      buffer.bytes += change.text.length;
      if (buffer.bytes > 1024 * 1024) {
        needsSnapshot = true;
        break;
      }
      buffer.parts.push(change.text);
      buffer.message.refusal ||= change.refusal;
    }
  }
  flush();
  return {
    turns: [...turns.values()].sort((a, b) => a.sequence - b.sequence),
    needsSnapshot,
  };
}
