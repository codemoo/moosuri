import { useState } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { ChatTurn, ModelChoice, ScheduleChatReceipt } from "../shared/api-types";
import { ChatPanel, type ChatMemory } from "./ChatPanel";

const response = (value: unknown, status = 200) => new Response(JSON.stringify(value), {
  status, headers: { "content-type": "application/json" },
});
const choice: ModelChoice = { provider_id: "p", provider_revision: 1, provider_name: "fixture",
  kind: "compatible", model: "fixture", effort: null, function_tools: false, verified_at: 1 };
function turn(sequence: number, request: string, receipt: ScheduleChatReceipt): ChatTurn {
  return { sequence, user_text: "Synthetic reminder request", assistant: [], run: {
    id: `run-${sequence}`, request_id: request, thread_id: "thread", attempt: 1,
    state: "completed", version: 2, model: choice, error: null,
    usage: { input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null },
    evidence: [], can_cancel: false, can_retry: false, created_at: sequence, schedule_receipts: [receipt],
  } };
}
function fixture(options: { lostAck?: boolean; hold?: Promise<void>; second?: boolean }) {
  const posts: Array<{ request_id: string; text: string; schedule_context: { target: { id: string; revision: number } } }> = [];
  const receipt = (id: string, title: string): ScheduleChatReceipt => ({ request_id: `initial-${id}`,
    id, revision: 1, action: "create", state: "active", title, next_local_slot: "utc:1800000000",
    timezone: "UTC", url: "/app/schedules" });
  const states = new Map([["water", receipt("water", "Water")], ["stretch", receipt("stretch", "Stretch")]]);
  const turns = [turn(1, "initial-water", states.get("water")!)];
  if (options.second) turns.push(turn(2, "initial-stretch", states.get("stretch")!));
  vi.stubGlobal("fetch", vi.fn(async (path: string, init?: RequestInit) => {
    if (path.endsWith("/messages") && init?.method === "POST") {
      const input = JSON.parse(String(init.body));
      posts.push(input);
      if (posts.length === 1 && options.hold) await options.hold;
      const prior = states.get(input.schedule_context.target.id)!;
      if (prior.revision !== input.schedule_context.target.revision) return response({ code: "conflict" }, 409);
      const next: ScheduleChatReceipt = { ...prior, request_id: input.request_id,
        revision: prior.revision + 1, action: input.text.startsWith("resume") ? "resume" : "pause",
        state: input.text.startsWith("resume") ? "active" : "paused" };
      states.set(next.id, next);
      const result = turn(turns.length + 1, input.request_id, next);
      turns.push(result);
      if (posts.length === 1 && options.lostAck) throw new TypeError("synthetic lost acknowledgement");
      return response(result);
    }
    if (path.includes("/settings/model")) return response({ selection: null, effective: choice, choices: [choice] });
    if (path.includes("/events")) return new Response(new ReadableStream({ start(controller) {
      init?.signal?.addEventListener("abort", () => { try { controller.close(); } catch { /* consumer cancelled */ } }, { once: true });
    } }), { headers: { "content-type": "text/event-stream" } });
    if (path.endsWith("/messages")) return response({ thread_id: "thread", cursor: 0, turns, next_before: null });
    return response({}, 404);
  }));
  return posts;
}
function Harness() {
  const [memory, setMemory] = useState<ChatMemory>({ drafts: { thread: "pause reminder" }, pending: {} });
  return <ChatPanel accountId="account" name="owner" threadId="thread" memory={memory}
    onDraft={(slot, text) => setMemory(current => ({ ...current, drafts: { ...current.drafts, [slot]: text } }))}
    onPending={(slot, value) => setMemory(current => { const pending = { ...current.pending };
      if (value) pending[slot] = value; else delete pending[slot]; return { ...current, pending }; })}
    onRestoreDraft={vi.fn()} onCreated={vi.fn()} onSuspend={vi.fn()} createThread={async () => "thread"}
    onExpired={vi.fn()} onThreadMissing={vi.fn()} onThreadUpdated={vi.fn()} openSettings={vi.fn()} />;
}
async function send(text: string) {
  fireEvent.change(screen.getByRole("textbox", { name: "메시지" }), { target: { value: text } });
  const button = screen.getByRole("button", { name: "메시지 전송" });
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
}
it.each([false, true])("continues with the committed reminder revision after a mutation (lost ACK: %s)", async lostAck => {
  const posts = fixture({ lostAck });
  render(<Harness />);
  fireEvent.click(await screen.findByRole("button", { name: "이 알림 선택" }));
  await send("pause reminder");
  await screen.findAllByRole("link", { name: "알림 · Water ↗" });
  await waitFor(() => expect(screen.getAllByRole("button", { name: "이 알림 선택" })).toHaveLength(2));
  await send("resume reminder");
  await waitFor(() => expect(posts).toHaveLength(2));
  expect(posts[0].schedule_context.target).toEqual({ id: "water", revision: 1 });
  expect(posts[1].schedule_context.target).toEqual({ id: "water", revision: 2 });
  await waitFor(() => expect(screen.getAllByRole("button", { name: "이 알림 선택" })).toHaveLength(3));
});
it("a delayed mutation acknowledgement preserves a newer reminder selection", async () => {
  let release!: () => void;
  const hold = new Promise<void>(resolve => { release = resolve; });
  const posts = fixture({ hold, second: true });
  render(<Harness />);
  const choices = await screen.findAllByRole("button", { name: "이 알림 선택" });
  fireEvent.click(choices[0]);
  await send("pause reminder");
  await waitFor(() => expect(posts).toHaveLength(1));
  fireEvent.click(choices[1]);
  await act(async () => release());
  await waitFor(() => expect(screen.getAllByRole("button", { name: "이 알림 선택" })).toHaveLength(3));
  expect(screen.getByText("선택한 알림: Stretch")).toBeInTheDocument();
  await send("pause reminder");
  await waitFor(() => expect(posts).toHaveLength(2));
  expect(posts[1].schedule_context.target).toEqual({ id: "stretch", revision: 1 });
});
