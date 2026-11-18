import { openWorkspaceMenu } from "./test-workspace";
import { StrictMode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { RequestScope, isAborted } from "./api";
import { SseDecoder, StreamFailure } from "./event-stream";
import { applyEvents } from "./chat-state";
import type {
  ChatEvent,
  ChatPage,
  ChatTurn,
  ModelChoice,
  SessionView,
  ThreadSummary,
  UsageSummary,
} from "../shared/api-types";

const account: SessionView = {
  user: {
    id: "account-a",
    name: "Alice Example",
    email: "alice@example.test",
    role: "admin",
    must_change_password: false,
  },
  password_change_required: false,
};
const bob: SessionView = {
  user: {
    id: "account-b",
    name: "Bob Example",
    email: "bob@example.test",
    role: "member",
    must_change_password: false,
  },
  password_change_required: false,
};
const thread: ThreadSummary = {
  id: "thread-a",
  title: "Synthetic conversation",
  created_at: 1,
  updated_at: 1,
};
const choice: ModelChoice = {
  provider_id: "profile-a",
  provider_revision: 1,
  provider_name: "Synthetic local",
  kind: "compatible",
  model: "synthetic-text",
  effort: null,
  function_tools: false,
  verified_at: 1,
};
const usage = {
  input_tokens: null,
  output_tokens: null,
  cache_read_tokens: null,
  cache_write_tokens: null,
};
function turn(
  overrides: Partial<ChatTurn["run"]> = {},
  text = "Synthetic question",
): ChatTurn {
  return {
    sequence: 1,
    user_text: text,
    assistant: [],
    run: {
      id: "run-a",
      request_id: "11111111-1111-4111-8111-111111111111",
      thread_id: thread.id,
      attempt: 1,
      state: "queued",
      version: 1,
      model: choice,
      error: null,
      usage,
      evidence: [],
      can_cancel: true,
      can_retry: false,
      created_at: 1,
      ...overrides,
    },
  };
}
const response = (value: unknown, status = 200) =>
  new Response(status === 204 ? null : JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (value?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
type Call = {
  path: string;
  method: string;
  body: Record<string, unknown> | null;
  init: RequestInit;
};
type Stream = {
  path: string;
  controller: ReadableStreamDefaultController<Uint8Array>;
  init: RequestInit;
  closed: boolean;
};
function fixture(
  options: {
    turns?: ChatTurn[];
    cursor?: number;
    threads?: ThreadSummary[];
    post?: (call: Call) => Promise<Response> | Response;
    action?: (call: Call) => Promise<Response> | Response;
    before?: ChatPage;
    history?: (call: Call) => Promise<Response> | Response | undefined;
    extra?: (call: Call) => Promise<Response> | Response | undefined;
  } = {},
) {
  const state = {
    session: account as SessionView | null,
    threads: options.threads ?? [thread],
    page: {
      thread_id: thread.id,
      cursor: options.cursor ?? 0,
      turns: options.turns ?? [],
      next_before: options.before ? 2 : null,
    } as ChatPage,
  };
  const calls: Call[] = [];
  const streams: Stream[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string, init: RequestInit = {}) => {
      const path = url.replace("/api/v1", "");
      const method = init.method ?? "GET";
      const call: Call = {
        path,
        method,
        body: init.body ? JSON.parse(String(init.body)) : null,
        init,
      };
      calls.push(call);
      const extra = options.extra?.(call);
      if (extra) return Promise.resolve(extra);
      if (path === "/system")
        return Promise.resolve(
          response({ initialized: true, version: "0.1", stage: "text-chat" }),
        );
      if (path === "/auth/session")
        return Promise.resolve(
          state.session
            ? response(state.session)
            : response({ code: "unauthorized" }, 401),
        );
      if (path === "/auth/logout") {
        state.session = null;
        return Promise.resolve(response(null, 204));
      }
      if (path === "/auth/login") {
        state.session = bob;
        state.threads = [];
        return Promise.resolve(response(bob));
      }
      if (path === "/settings/model")
        return Promise.resolve(
          response({ selection: null, effective: choice, choices: [choice] }),
        );
      if (path === "/threads" && method === "GET")
        return Promise.resolve(response({ threads: state.threads }));
      if (path === "/threads" && method === "POST") {
        state.threads = [thread];
        return Promise.resolve(response(thread, 201));
      }
      if (path === `/threads/${thread.id}` && method === "DELETE") {
        state.threads = [];
        return Promise.resolve(response(null, 204));
      }
      if (path.startsWith(`/threads/${thread.id}/messages`) && method === "GET")
        return Promise.resolve(
          options.history?.(call) ??
            response(path.includes("?before=") ? options.before : state.page),
        );
      if (path === `/threads/${thread.id}/messages` && method === "POST") {
        if (options.post) return Promise.resolve(options.post(call));
        const value = turn(
          { request_id: String(call.body?.request_id) },
          String(call.body?.text),
        );
        state.page = { ...state.page, cursor: 1, turns: [value] };
        return Promise.resolve(response(value, 201));
      }
      if (path.startsWith("/runs/"))
        return Promise.resolve(
          options.action?.(call) ??
            response({ code: "unexpected_action" }, 409),
        );
      if (path.includes("/events?")) {
        let entry!: Stream;
        const body = new ReadableStream<Uint8Array>({
          start(controller) {
            entry = { path, controller, init, closed: false };
            streams.push(entry);
          },
          cancel() {
            entry.closed = true;
          },
        });
        return Promise.resolve(
          new Response(body, {
            headers: { "Content-Type": "text/event-stream" },
          }),
        );
      }
      const summary: UsageSummary = {
        dispatched_calls: 3,
        unreported_calls: 2,
        reported_input_tokens: 0,
        reported_output_tokens: 0,
        reported_cache_read_tokens: 0,
        reported_cache_write_tokens: 0,
      };
      if (path === "/usage") return Promise.resolve(response(summary));
      if (path === "/admin/usage")
        return Promise.resolve(
          response({
            accounts: [
              {
                account_id: account.user.id,
                name: account.user.name,
                email: account.user.email,
                usage: summary,
              },
            ],
          }),
        );
      return Promise.resolve(response({ code: "unexpected_test_route" }, 404));
    }),
  );
  function emit(event: ChatEvent, stream = streams.at(-1)!) {
    if (!stream.closed)
      stream.controller.enqueue(
        new TextEncoder().encode(
          `event: change\nid: ${event.sequence}\ndata: ${JSON.stringify(event)}\n\n`,
        ),
      );
  }
  return { state, calls, streams, emit };
}
function open(path = "/app") {
  window.history.replaceState(null, "", path);
  return render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
async function ready(streams: Stream[]) {
  await waitFor(() =>
    expect(streams.some((stream) => !stream.closed)).toBe(true),
  );
  await waitFor(() =>
    expect(
      screen.queryByText("대화 기록을 불러오는 중…"),
    ).not.toBeInTheDocument(),
  );
}
const postCalls = (calls: Call[]) =>
  calls.filter(
    (call) => call.method === "POST" && call.path.endsWith("/messages"),
  );
beforeEach(() => {
  sessionStorage.clear();
  vi.stubGlobal("BroadcastChannel", undefined);
});

describe("durable text chat", () => {
  it("streams text and late phase without letting a late queued acknowledgement reopen the completed run", async () => {
    const user = userEvent.setup();
    const ack = deferred<Response>();
    const test = fixture({ post: () => ack.promise });
    open();
    await ready(test.streams);
    await user.type(screen.getByLabelText("메시지"), "Synthetic question");
    await user.click(screen.getByRole("button", { name: "메시지 전송" }));
    const request = postCalls(test.calls)[0];
    const queued = turn({ request_id: String(request.body?.request_id) });
    const running = { ...queued.run, state: "running" as const, version: 2 };
    await act(async () => {
      test.emit({ sequence: 1, change: { kind: "turn", turn: queued } });
      test.emit({ sequence: 2, change: { kind: "state", run: running } });
      test.emit({
        sequence: 3,
        change: {
          kind: "delta",
          run_id: running.id,
          attempt: 1,
          message_id: "message-a",
          text: "Safe streamed answer",
          phase: null,
          refusal: false,
        },
      });
      test.emit({
        sequence: 4,
        change: {
          kind: "phase",
          run_id: running.id,
          attempt: 1,
          message_id: "message-a",
          phase: "final_answer",
        },
      });
      test.emit({
        sequence: 5,
        change: {
          kind: "state",
          run: {
            ...running,
            state: "completed",
            version: 3,
            can_cancel: false,
          },
        },
      });
    });
    expect(await screen.findByText("Safe streamed answer")).toBeInTheDocument();
    expect(screen.getByText("답변")).toBeInTheDocument();
    test.state.page = {
      thread_id: thread.id,
      cursor: 5,
      next_before: null,
      turns: [
        {
          ...queued,
          run: {
            ...running,
            state: "completed",
            version: 3,
            can_cancel: false,
          },
          assistant: [
            {
              id: "message-a",
              text: "Safe streamed answer",
              phase: "final_answer",
              refusal: false,
            },
          ],
        },
      ],
    };
    await act(async () => {
      ack.resolve(response(queued, 201));
    });
    expect(screen.getByText("완료")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "응답 중지" }),
    ).not.toBeInTheDocument();
    expect(screen.getAllByText("Synthetic question")).toHaveLength(1);
    expect(postCalls(test.calls)).toHaveLength(1);
    expect(screen.queryByText("실행 정보")).not.toBeInTheDocument();
    expect(screen.queryByText("사용량 미보고")).not.toBeInTheDocument();
    expect(
      test.calls
        .filter(
          (call) =>
            call.path.includes("/messages") || call.path.includes("/events"),
        )
        .every(
          (call) =>
            new Headers(call.init.headers).get("X-Moosuri-Account") ===
            account.user.id,
        ),
    ).toBe(true);
  });

  it("first-send creates a thread and retains a newly edited next draft", async () => {
    const user = userEvent.setup();
    const ack = deferred<Response>();
    const test = fixture({ threads: [], post: () => ack.promise });
    open();
    const input = await screen.findByLabelText("메시지");
    await user.type(input, "First message");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "메시지 전송" })).toBeEnabled(),
    );
    await user.click(screen.getByRole("button", { name: "메시지 전송" }));
    await waitFor(() => expect(postCalls(test.calls)).toHaveLength(1));
    await user.type(screen.getByLabelText("메시지"), "Next draft");
    const post = postCalls(test.calls)[0];
    expect(
      test.calls.filter(
        (call) => call.path === "/threads" && call.method === "POST",
      ),
    ).toHaveLength(1);
    expect(post.body?.text).toBe("First message");
    expect(screen.getAllByText("First message")).toHaveLength(1);
    test.state.page = {
      thread_id: thread.id,
      cursor: 1,
      next_before: null,
      turns: [
        turn({ request_id: String(post.body?.request_id) }, "First message"),
      ],
    };
    await act(async () => {
      ack.resolve(
        response(
          turn({ request_id: String(post.body?.request_id) }, "First message"),
          201,
        ),
      );
    });
    expect(screen.getByLabelText("메시지")).toHaveValue("Next draft");
  });

  it("keeps the same request ID after a lost acknowledgement and resubmits only on the user's action", async () => {
    const user = userEvent.setup();
    const first = deferred<Response>();
    let attempts = 0;
    const test = fixture({
      post: (call) => {
        attempts++;
        if (attempts === 1) return first.promise;
        const accepted = turn(
          { request_id: String(call.body?.request_id) },
          String(call.body?.text),
        );
        test.state.page = {
          thread_id: thread.id,
          cursor: 1,
          turns: [accepted],
          next_before: null,
        };
        return response(accepted, 201);
      },
    });
    open();
    await ready(test.streams);
    await user.type(
      screen.getByLabelText("메시지"),
      "Request with uncertain acknowledgement",
    );
    await user.dblClick(screen.getByRole("button", { name: "메시지 전송" }));
    expect(postCalls(test.calls)).toHaveLength(1);
    await act(async () => {
      first.reject(new TypeError("synthetic connection loss"));
    });
    await screen.findByRole("button", { name: "같은 요청 다시 제출" });
    expect(postCalls(test.calls)).toHaveLength(1);
    await user.type(screen.getByLabelText("메시지"), "Unsent future draft");
    await user.click(
      screen.getByRole("button", { name: "같은 요청 다시 제출" }),
    );
    await waitFor(() => expect(postCalls(test.calls)).toHaveLength(2));
    expect(postCalls(test.calls)[1].body).toEqual(
      postCalls(test.calls)[0].body,
    );
    expect(screen.getByLabelText("메시지")).toHaveValue("Unsent future draft");
    expect(
      screen.getAllByText("Request with uncertain acknowledgement"),
    ).toHaveLength(1);
  });

  it("reanchors a same-ID running POST snapshot before releasing delayed SSE deltas", async () => {
    const user = userEvent.setup();
    const first = deferred<Response>();
    const snapshot = deferred<Response>();
    let posts = 0;
    let holdSnapshot = false;
    let snapshotRequested = false;
    let accepted!: ChatTurn;
    const test = fixture({
      post: (call) => {
        posts++;
        if (posts === 1) return first.promise;
        accepted = {
          ...turn(
            {
              request_id: String(call.body?.request_id),
              state: "running",
              version: 2,
            },
            String(call.body?.text),
          ),
          assistant: [
            {
              id: "message-a",
              text: "Already streamed",
              phase: "commentary",
              refusal: false,
            },
          ],
        };
        holdSnapshot = true;
        return response(accepted, 201);
      },
      history: () => {
        if (holdSnapshot) {
          snapshotRequested = true;
          return snapshot.promise;
        }
        return undefined;
      },
    });
    open();
    await ready(test.streams);
    await user.type(
      screen.getByLabelText("메시지"),
      "Lost acknowledgement question",
    );
    await user.click(screen.getByRole("button", { name: "메시지 전송" }));
    await act(async () => {
      first.reject(new TypeError("synthetic lost acknowledgement"));
    });
    await screen.findByRole("button", { name: "같은 요청 다시 제출" });
    const heldStream = test.streams.at(-1)!;
    await user.type(
      screen.getByLabelText("메시지"),
      "Future draft stays intact",
    );
    await user.click(
      screen.getByRole("button", { name: "같은 요청 다시 제출" }),
    );
    await waitFor(() => expect(snapshotRequested).toBe(true));
    expect(heldStream.init.signal?.aborted).toBe(true);
    expect(screen.queryByText("Already streamed")).not.toBeInTheDocument();
    await act(async () => {
      snapshot.resolve(
        response({
          thread_id: thread.id,
          cursor: 4,
          next_before: null,
          turns: [accepted],
        }),
      );
    });
    expect(await screen.findByText("Already streamed")).toBeInTheDocument();
    await waitFor(() => expect(test.streams.at(-1)?.path).toContain("after=4"));
    await act(async () => {
      // Previously queued bytes may be replayed by a reconnecting transport.
      // The committed page cursor makes these no-ops, rather than duplicate text.
      test.emit({
        sequence: 3,
        change: {
          kind: "delta",
          run_id: accepted.run.id,
          attempt: 1,
          message_id: "message-a",
          text: "Already ",
          phase: null,
          refusal: false,
        },
      });
      test.emit({
        sequence: 4,
        change: {
          kind: "delta",
          run_id: accepted.run.id,
          attempt: 1,
          message_id: "message-a",
          text: "streamed",
          phase: null,
          refusal: false,
        },
      });
      test.emit({
        sequence: 5,
        change: {
          kind: "delta",
          run_id: accepted.run.id,
          attempt: 1,
          message_id: "message-a",
          text: " once",
          phase: null,
          refusal: false,
        },
      });
    });
    expect(
      await screen.findByText("Already streamed once"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/Already streamedAlready/),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("메시지")).toHaveValue(
      "Future draft stays intact",
    );
    expect(postCalls(test.calls)).toHaveLength(2);
    expect(postCalls(test.calls)[1].body).toEqual(
      postCalls(test.calls)[0].body,
    );
  });

  it("distinguishes an explicit null phase from a delta with no new phase", async () => {
    const initial = {
      ...turn({ state: "running", version: 2 }),
      assistant: [
        {
          id: "message-a",
          text: "Existing explanation",
          phase: "commentary" as const,
          refusal: false,
        },
      ],
    };
    const test = fixture({ turns: [initial], cursor: 3 });
    open();
    await ready(test.streams);
    expect(screen.getByText("진행 설명")).toBeInTheDocument();
    await act(async () => {
      test.emit({
        sequence: 4,
        change: {
          kind: "delta",
          run_id: initial.run.id,
          attempt: 1,
          message_id: "message-a",
          text: " continues",
          phase: null,
          refusal: false,
        },
      });
    });
    await screen.findByText("Existing explanation continues");
    expect(screen.getByText("진행 설명")).toBeInTheDocument();
    await act(async () => {
      test.emit({
        sequence: 5,
        change: {
          kind: "phase",
          run_id: initial.run.id,
          attempt: 1,
          message_id: "message-a",
          phase: null,
        },
      });
    });
    await waitFor(() =>
      expect(screen.queryByText("진행 설명")).not.toBeInTheDocument(),
    );
    expect(
      screen.getByText("Existing explanation continues"),
    ).toBeInTheDocument();
  });

  it("synchronizes accepted history after a lost acknowledgement without replaying inference", async () => {
    const user = userEvent.setup();
    const ack = deferred<Response>();
    const test = fixture({ post: () => ack.promise });
    open();
    await ready(test.streams);
    await user.type(screen.getByLabelText("메시지"), "Accepted once");
    await user.click(screen.getByRole("button", { name: "메시지 전송" }));
    const request = postCalls(test.calls)[0];
    test.state.page = {
      thread_id: thread.id,
      cursor: 1,
      turns: [
        turn({ request_id: String(request.body?.request_id) }, "Accepted once"),
      ],
      next_before: null,
    };
    await act(async () => {
      ack.reject(new TypeError("synthetic lost acknowledgement"));
    });
    expect(
      await screen.findByRole("button", { name: "응답 중지" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "같은 요청 다시 제출" }),
    ).not.toBeInTheDocument();
    expect(postCalls(test.calls)).toHaveLength(1);
    expect(screen.getAllByText("Accepted once")).toHaveLength(1);
  });

  it("keeps a cancelled run terminal when earlier queued stream bytes arrive later", async () => {
    const user = userEvent.setup();
    const initial = turn({ state: "running", version: 2 });
    const cancelled = {
      ...initial,
      run: {
        ...initial.run,
        state: "cancelled" as const,
        version: 4,
        can_cancel: false,
      },
    };
    const test = fixture({
      turns: [initial],
      cursor: 2,
      action: () => response(cancelled),
    });
    open();
    await ready(test.streams);
    await user.type(screen.getByLabelText("메시지"), "Editable next draft");
    expect(screen.getByRole("button", { name: "메시지 전송" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "응답 중지" }));
    await screen.findByText("중지됨");
    await act(async () => {
      test.emit({
        sequence: 3,
        change: { kind: "state", run: { ...initial.run, version: 3 } },
      });
      test.emit({
        sequence: 4,
        change: {
          kind: "delta",
          run_id: initial.run.id,
          attempt: 1,
          message_id: "message-a",
          text: "Never render late cancelled text",
          phase: null,
          refusal: false,
        },
      });
      await new Promise((resolve) => window.setTimeout(resolve, 35));
    });
    await waitFor(() =>
      expect(
        screen.queryByText("Never render late cancelled text"),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByText("중지됨")).toBeInTheDocument();
    expect(screen.getByLabelText("메시지")).toHaveValue("Editable next draft");
    expect(
      test.calls.find((call) => call.path.endsWith("/cancel"))?.body,
    ).toEqual({ expected_attempt: 1 });
  });

  it("retries the original run without replacing the draft or resurrecting old-attempt text", async () => {
    const user = userEvent.setup();
    const failed = {
      ...turn({
        state: "failed",
        version: 3,
        can_cancel: false,
        can_retry: true,
        error: "model_timeout",
      }),
      assistant: [
        {
          id: "message-a",
          text: "Old partial answer",
          phase: null,
          refusal: false,
        },
      ],
    };
    const retried = {
      ...failed,
      assistant: [],
      run: {
        ...failed.run,
        attempt: 2,
        version: 4,
        state: "queued" as const,
        can_retry: false,
        can_cancel: true,
        error: null,
      },
    };
    const test = fixture({
      turns: [failed],
      cursor: 3,
      action: () => response(retried),
    });
    open();
    await ready(test.streams);
    await user.type(
      screen.getByLabelText("메시지"),
      "Future question remains a draft",
    );
    await user.click(
      screen.getByRole("button", { name: "같은 설정으로 다시 시도" }),
    );
    await waitFor(() =>
      expect(screen.queryByText("Old partial answer")).not.toBeInTheDocument(),
    );
    await act(async () => {
      test.emit({
        sequence: 4,
        change: {
          kind: "delta",
          run_id: failed.run.id,
          attempt: 1,
          message_id: "message-a",
          text: "Old attempt must stay hidden",
          phase: "final_answer",
          refusal: false,
        },
      });
      await new Promise((resolve) => window.setTimeout(resolve, 35));
    });
    expect(screen.getByLabelText("메시지")).toHaveValue(
      "Future question remains a draft",
    );
    expect(
      screen.queryByText("Old attempt must stay hidden"),
    ).not.toBeInTheDocument();
    expect(
      test.calls.find((call) => call.path.endsWith("/retry"))?.body,
    ).toEqual({ expected_attempt: 1 });
    expect(postCalls(test.calls)).toHaveLength(0);
  });

  it("restores a snapshot, ignores duplicate events, and refetches after a sequence gap", async () => {
    const initial = {
      ...turn({ state: "running", version: 2 }),
      assistant: [
        { id: "message-a", text: "Restored text", phase: null, refusal: false },
      ],
    };
    const test = fixture({ turns: [initial], cursor: 5 });
    open();
    await ready(test.streams);
    expect(test.streams.at(-1)?.path).toContain("after=5");
    expect(screen.getByText("Restored text")).toBeInTheDocument();
    const latest = {
      ...initial,
      assistant: [{ ...initial.assistant[0], text: "Canonical replacement" }],
    };
    test.state.page = { ...test.state.page, cursor: 7, turns: [latest] };
    await act(async () => {
      test.emit({
        sequence: 5,
        change: {
          kind: "delta",
          run_id: initial.run.id,
          attempt: 1,
          message_id: "message-a",
          text: "Duplicate must not append",
          phase: null,
          refusal: false,
        },
      });
      test.emit({
        sequence: 7,
        change: {
          kind: "delta",
          run_id: initial.run.id,
          attempt: 1,
          message_id: "message-a",
          text: "Gap must not append",
          phase: null,
          refusal: false,
        },
      });
    });
    expect(
      await screen.findByText("Canonical replacement"),
    ).toBeInTheDocument();
    await waitFor(
      () =>
        expect(
          test.streams.some((stream) => stream.path.endsWith("after=7")),
        ).toBe(true),
      { timeout: 1600 },
    );
    expect(screen.queryByText(/must not append/)).not.toBeInTheDocument();
  });

  it("does not send during IME composition or with Shift+Enter and enforces UTF-8 bytes", async () => {
    const test = fixture();
    open();
    await ready(test.streams);
    const input = screen.getByLabelText("메시지");
    fireEvent.change(input, { target: { value: "안녕" } });
    fireEvent.compositionStart(input);
    fireEvent.keyDown(input, { key: "Enter", isComposing: true, keyCode: 229 });
    fireEvent.compositionEnd(input);
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(postCalls(test.calls)).toHaveLength(0);
    fireEvent.change(input, { target: { value: "가".repeat(2731) } });
    expect(screen.getByRole("button", { name: "메시지 전송" })).toBeDisabled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(postCalls(test.calls)).toHaveLength(0);
    fireEvent.change(input, { target: { value: "안녕" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(postCalls(test.calls)).toHaveLength(1));
    expect(postCalls(test.calls)[0].body?.text).toBe("안녕");
  });

  it("loads older turns without replacing the current page or changing the stream cursor", async () => {
    const user = userEvent.setup();
    const latest = {
      ...turn(
        { id: "run-latest", state: "completed", version: 3, can_cancel: false },
        "Latest question",
      ),
      sequence: 2,
    };
    const previous = turn(
      {
        id: "run-previous",
        request_id: "22222222-2222-4222-8222-222222222222",
        state: "completed",
        version: 3,
        can_cancel: false,
      },
      "Previous question",
    );
    const test = fixture({
      turns: [latest],
      cursor: 6,
      before: {
        thread_id: thread.id,
        cursor: 6,
        turns: [previous],
        next_before: null,
      },
    });
    open();
    await ready(test.streams);
    await user.click(screen.getByRole("button", { name: "이전 대화 보기" }));
    expect(await screen.findByText("Previous question")).toBeInTheDocument();
    expect(screen.getByText("Latest question")).toBeInTheDocument();
    expect(
      test.calls.some((call) => call.path.endsWith("messages?before=2")),
    ).toBe(true);
    expect(test.streams.at(-1)?.path).toContain("after=6");
    expect(
      screen.queryByRole("button", { name: "이전 대화 보기" }),
    ).not.toBeInTheDocument();
  });

  it("does not pull a reader down for new text and offers an explicit jump to the response", async () => {
    const initial = turn({ state: "running", version: 2 });
    const test = fixture({ turns: [initial], cursor: 2 });
    open();
    await ready(test.streams);
    const element = document.querySelector(".chat-scroll") as HTMLDivElement;
    Object.defineProperty(element, "scrollHeight", {
      configurable: true,
      value: 1200,
    });
    Object.defineProperty(element, "clientHeight", {
      configurable: true,
      value: 400,
    });
    // Let the layout observer see the synthetic resize before simulating a
    // reader scroll. A resize-induced scroll is deliberately not reader intent.
    fireEvent(window, new Event("resize"));
    element.scrollTop = 100;
    fireEvent.scroll(element);
    await act(async () => {
      test.emit({
        sequence: 3,
        change: {
          kind: "delta",
          run_id: initial.run.id,
          attempt: 1,
          message_id: "message-a",
          text: "New visible answer",
          phase: null,
          refusal: false,
        },
      });
    });
    await screen.findByText("New visible answer");
    expect(element.scrollTop).toBe(100);
    fireEvent.click(screen.getByRole("button", { name: "새 응답 보기 ↓" }));
    expect(element.scrollTop).toBe(1200);
  });

  it.each(["delete", "account"] as const)(
    "fences private stream results after %s changes the visible lifetime",
    async (kind) => {
      const user = userEvent.setup();
      const initial = turn({ state: "running", version: 2 });
      const test = fixture({ turns: [initial], cursor: 2 });
      open();
      await ready(test.streams);
      const old = test.streams.at(-1)!;
      await user.type(screen.getByLabelText("메시지"), "Private unsent draft");
      if (kind === "delete") {
        await user.click(
          openWorkspaceMenu().getByRole("button", { name: `Delete ${thread.title}` }),
        );
        await user.click(
          within(screen.getByRole("dialog")).getByRole("button", {
            name: "Delete conversation",
          }),
        );
        await waitFor(() =>
          expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
        );
      } else {
        await user.click(
          openWorkspaceMenu().getByRole("button", { name: "Sign out" }),
        );
        await user.type(await screen.findByLabelText("Email"), bob.user.email);
        await user.type(
          screen.getByLabelText("Password"),
          "synthetic-password",
        );
        await user.click(screen.getByRole("button", { name: "Sign in" }));
        await screen.findByRole("heading", { name: "Bob님, 무엇을 도와드릴까요?" });
      }
      expect(old.init.signal?.aborted).toBe(true);
      test.emit(
        {
          sequence: 3,
          change: {
            kind: "delta",
            run_id: initial.run.id,
            attempt: 1,
            message_id: "message-a",
            text: "Private late response",
            phase: "final_answer",
            refusal: false,
          },
        },
        old,
      );
      expect(
        screen.queryByText("Private late response"),
      ).not.toBeInTheDocument();
      expect(screen.getByLabelText("메시지")).toHaveValue("");
    },
  );
});

it("shows reported token totals separately from unreported charged calls without private admin content", async () => {
  fixture();
  open("/settings/usage");
  expect(await screen.findByText("사용량 미보고 2회")).toBeInTheDocument();
  expect(screen.getByText("보고된 입력 토큰")).toBeInTheDocument();
  expect(screen.getByText(/무료 요청을 뜻하지 않습니다/)).toBeInTheDocument();
});

describe("stream and reducer boundaries", () => {
  it("parses split Unicode, CRLF and multiline SSE with bounded frames", () => {
    const wire =
      ': keepalive\r\n\r\nevent: change\r\nid: 1\r\ndata: {"sequence":1,\r\ndata: "text":"안녕 🌱"}\r\n\r\n';
    const bytes = new TextEncoder().encode(wire);
    for (let split = 0; split <= bytes.length; split++) {
      const decoder = new SseDecoder();
      const frames = [
        ...decoder.push(bytes.slice(0, split)),
        ...decoder.push(bytes.slice(split)),
      ];
      decoder.finish();
      expect(frames).toHaveLength(1);
      expect(frames[0].id).toBe("1");
      expect(JSON.parse(frames[0].data).text).toBe("안녕 🌱");
    }
    const decoder = new SseDecoder();
    expect(() => decoder.push(new Uint8Array(1024 * 1024 + 1))).toThrow(
      StreamFailure,
    );
  });
  it("aborts an ignored fetch stream before any late callback can cross an account boundary", async () => {
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        Promise.resolve(
          new Response(
            new ReadableStream<Uint8Array>({
              start(value) {
                controller = value;
              },
            }),
            { headers: { "Content-Type": "text/event-stream" } },
          ),
        ),
      ),
    );
    const scope = new RequestScope("account-a");
    const receive = vi.fn();
    const running = scope
      .stream("/threads/thread-a/events?after=0", receive)
      .catch((error) => error);
    await Promise.resolve();
    await Promise.resolve();
    scope.close();
    try {
      controller.enqueue(
        new TextEncoder().encode("event: change\ndata: {}\n\n"),
      );
    } catch {
      /* Reader was cancelled as required. */
    }
    expect(isAborted(await running)).toBe(true);
    expect(receive).not.toHaveBeenCalled();
  });
  it("reconciles many fragments once per message and ignores old attempts and versions", () => {
    const initial = turn({ state: "running", version: 5, attempt: 2 });
    const events: ChatEvent[] = Array.from({ length: 2000 }, (_, index) => ({
      sequence: index + 1,
      change: {
        kind: "delta",
        run_id: initial.run.id,
        attempt: 2,
        message_id: "message-a",
        text: "x",
        phase: null,
        refusal: false,
      },
    }));
    events.push({
      sequence: 2001,
      change: {
        kind: "phase",
        run_id: initial.run.id,
        attempt: 2,
        message_id: "message-a",
        phase: "final_answer",
      },
    });
    events.push({
      sequence: 2002,
      change: {
        kind: "delta",
        run_id: initial.run.id,
        attempt: 1,
        message_id: "message-a",
        text: "old",
        phase: null,
        refusal: false,
      },
    });
    events.push({
      sequence: 2003,
      change: {
        kind: "state",
        run: { ...initial.run, version: 4, state: "queued" },
      },
    });
    const result = applyEvents([initial], events, thread.id);
    expect(result.needsSnapshot).toBe(false);
    expect(result.turns[0].assistant[0].text).toBe("x".repeat(2000));
    expect(result.turns[0].assistant[0].phase).toBe("final_answer");
    expect(initial.assistant).toHaveLength(0);
    expect(result.turns[0].run.state).toBe("running");
  });
});

it("marks a quiet authenticated SSE connection ready before the first message event", async () => {
  const test=fixture();
  open();
  await ready(test.streams);
  await waitFor(()=>expect(screen.queryByText("연결 중…")).not.toBeInTheDocument());
  expect(screen.getByLabelText("메시지")).toBeEnabled();
});


it("drops previously loaded older evidence immediately on reset even while the fresh snapshot is held", async () => {
  const user = userEvent.setup();
  const latest = {...turn({state:"completed",can_cancel:false}),sequence:2};
  const old = turn({id:"old-run",request_id:"old-request",state:"completed",can_cancel:false,
    evidence:[{kind:"source",id:"source-a",revision:1,title:"철회할 원문"}]},"이전 질문");
  old.assistant=[{id:"old-answer",text:"철회할 과거 답변",phase:null,refusal:false}];
  const held=deferred<Response>();
  let resetting=false;
  const test=fixture({turns:[latest],cursor:2,
    before:{thread_id:thread.id,cursor:2,turns:[old],next_before:null},
    history:(call)=>resetting && !call.path.includes("?before=") ? held.promise : undefined});
  open();await ready(test.streams);
  await user.click(screen.getByRole("button",{name:"이전 대화 보기"}));
  expect(await screen.findByText("철회할 과거 답변")).toBeInTheDocument();
  await user.type(screen.getByLabelText("메시지"),"보존할 초안");
  resetting=true;
  await act(async()=>{test.streams.at(-1)!.controller.enqueue(new TextEncoder().encode("event: reset\ndata: {}\n\n"));});
  await waitFor(()=>expect(screen.queryByText("철회할 과거 답변")).not.toBeInTheDocument());
  expect(screen.queryByRole("button",{name:"철회할 원문 ↗"})).not.toBeInTheDocument();
  expect(screen.getByLabelText("메시지")).toHaveValue("보존할 초안");
  await act(async()=>{held.resolve(response({...test.state.page,cursor:9,next_before:2}));});
  await waitFor(()=>expect(screen.getByText("Synthetic question")).toBeInTheDocument());
  expect(screen.queryByText("철회할 과거 답변")).not.toBeInTheDocument();
});

it("does not restore old output from a retry response that arrives after a knowledge reset",async()=>{
  const user=userEvent.setup();const held=deferred<Response>();
  const initial=turn({state:"failed",can_cancel:false,can_retry:true});
  initial.assistant=[{id:"partial",text:"철회할 부분 답변",phase:null,refusal:false}];
  const test=fixture({turns:[initial],cursor:1,action:()=>held.promise});
  open();await ready(test.streams);
  await user.click(screen.getByRole("button",{name:"같은 설정으로 다시 시도"}));
  const invalidated={...initial,assistant:[],run:{...initial.run,state:"incomplete" as const,error:"knowledge_invalidated",version:8,can_retry:false}};
  test.state.page={...test.state.page,cursor:8,turns:[invalidated]};
  await act(async()=>{test.streams.at(-1)!.controller.enqueue(new TextEncoder().encode("event: reset\ndata: {}\n\n"));});
  await waitFor(()=>expect(screen.queryByText("철회할 부분 답변")).not.toBeInTheDocument());
  await act(async()=>{held.resolve(response({...initial,run:{...initial.run,state:"running",attempt:2,version:3}}));});
  expect(await screen.findByText(/참고한 자료가 변경되거나 삭제되어/)).toBeInTheDocument();
  expect(screen.queryByText("철회할 부분 답변")).not.toBeInTheDocument();
});

it("replaces cached older evidence after a missed reset and fences an old retry acknowledgement", async () => {
  const user = userEvent.setup();
  const latest = { ...turn({ state: "failed", can_cancel: false, can_retry: true }), sequence: 2 };
  const old = turn({ id: "old-run", request_id: "old-request", state: "failed",
    can_cancel: false, can_retry: false,
    evidence: [{ kind: "source", id: "source-a", revision: 1, title: "오프라인 중 삭제한 원문" }] }, "이전 질문");
  old.assistant = [{ id: "old-answer", text: "오프라인 중 철회된 답변", phase: null, refusal: false }];
  const ack = deferred<Response>();
  const fresh = deferred<Response>();
  let refreshing = false;
  const test = fixture({ turns: [latest], cursor: 2,
    before: { thread_id: thread.id, cursor: 2, turns: [old], next_before: null },
    history: (call) => refreshing && !call.path.includes("?before=") ? fresh.promise.then((value) => value.clone()) : undefined,
    action: () => ack.promise });
  open();
  await ready(test.streams);
  await user.click(screen.getByRole("button", { name: "이전 대화 보기" }));
  expect(await screen.findByText("오프라인 중 철회된 답변")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "같은 설정으로 다시 시도" }));
  await user.type(screen.getByLabelText("메시지"), "유지할 초안");
  refreshing = true;
  // No reset event arrives: the browser learns only the newer page and cursor.
  await act(async () => { window.dispatchEvent(new Event("online")); });
  await waitFor(() => expect(screen.queryByText("오프라인 중 철회된 답변")).not.toBeInTheDocument());
  await act(async () => { fresh.resolve(response({ ...test.state.page, cursor: 9, next_before: 2 })); });
  await waitFor(() => expect(test.streams.at(-1)?.path).toContain("after=9"));
  await act(async () => { ack.resolve(response({ ...latest, assistant: old.assistant,
    run: { ...latest.run, state: "running", attempt: 2, version: 3 } })); });
  expect(screen.queryByText("오프라인 중 철회된 답변")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "오프라인 중 삭제한 원문 ↗" })).not.toBeInTheDocument();
  expect(screen.getByLabelText("메시지")).toHaveValue("유지할 초안");
  expect(screen.getByRole("button", { name: "이전 대화 보기" })).toBeInTheDocument();
});

it("opens only the exact backend evidence revision from a chat reference",async()=>{
  const user=userEvent.setup();const id="11111111-1111-4111-8111-111111111111";
  const value=turn({state:"completed",can_cancel:false,evidence:[{kind:"source",id,revision:7,title:"회의 메모"}]});
  value.assistant=[{id:"answer",text:"회의 자료를 확인했습니다.",phase:"final_answer",refusal:false}];
  const test=fixture({turns:[value],cursor:1,extra:(call)=>{
    if(call.path==="/knowledge/collections")return response({collections:[]});
    if(call.path===`/knowledge/sources/${id}?revision=7`)return response({source:{id,collection_id:"collection-a",external_id:"source-a",version:"7",revision:7,title:"회의 메모",speaker:null,observed_at:1,valid_from:null,valid_until:null},text:"확인한 원문의 정확한 버전입니다."});
  }});
  open();await ready(test.streams);
  await user.click(screen.getByRole("button",{name:"회의 메모 ↗"}));
  expect(await screen.findByLabelText("원문 텍스트")).toHaveValue("확인한 원문의 정확한 버전입니다.");
  expect(test.calls.some(call=>call.path===`/knowledge/sources/${id}?revision=7`)).toBe(true);
  expect(window.location.pathname).toBe("/app/knowledge");
});

it("attaches an exact private file from the library and keeps its original refs through lost ACK retry",async()=>{
 const user=userEvent.setup();const id="11111111-1111-4111-8111-111111111111";const original={id,revision:1,filename:"synthetic-receipt.json",media_type:"application/json",digest:"a".repeat(64),state:"ready",width:0,height:0,bytes:15,created_at:1};let attempts=0;
 const test=fixture({extra:call=>{if(call.path==="/media")return response({items:[original],used_bytes:30,quota_bytes:67108864});if(call.path===`/files/${id}/preview`)return response('{"quantity":37}');},post:call=>{if(++attempts===1)return Promise.reject(new TypeError("Synthetic lost ACK"));const accepted={...turn({request_id:String(call.body?.request_id)},String(call.body?.text)),attachments:[original]};test.state.page={thread_id:thread.id,cursor:1,turns:[accepted],next_before:null};return response(accepted,201);}});
 open();await ready(test.streams);await user.click(screen.getByRole("button",{name:"첨부 추가"}));await user.click(screen.getByLabelText("내 이미지·파일 보관함"));await user.click(await screen.findByRole("button",{name:"대화에 첨부"}));await user.type(screen.getByLabelText("메시지"),"What is quantity in my exact selected file?");await user.click(screen.getByRole("button",{name:"메시지 전송"}));await waitFor(()=>expect(postCalls(test.calls)).toHaveLength(1));await user.type(screen.getByLabelText("메시지"),"Next draft remains");
 const retry=await screen.findByRole("button",{name:/같은 요청 다시 제출/});await user.click(retry);await waitFor(()=>expect(postCalls(test.calls)).toHaveLength(2));expect(postCalls(test.calls)[0].body?.attachments).toEqual([{id,revision:1}]);expect(postCalls(test.calls)[1].body).toEqual(postCalls(test.calls)[0].body);expect(screen.getByLabelText("메시지")).toHaveValue("Next draft remains");expect(screen.getAllByText("synthetic-receipt.json").length).toBeGreaterThan(0);
});

it("fresh sends keep the existing stream and loaded history while their SSE receipt races the POST", async () => {
  const user = userEvent.setup();
  const ack = deferred<Response>();
  const old = { ...turn({ id: "old-run", request_id: "old-request", state: "completed", can_cancel: false }, "Older preserved question"), sequence: 1 };
  const latest = { ...turn({ id: "latest-run", request_id: "latest-request", state: "completed", can_cancel: false }, "Latest preserved question"), sequence: 2 };
  const test = fixture({ turns: [latest], cursor: 2, before: { thread_id: thread.id, cursor: 2, turns: [old], next_before: null }, post: () => ack.promise });
  open();
  await ready(test.streams);
  await user.click(screen.getByRole("button", { name: /이전 대화/ }));
  expect(await screen.findByText("Older preserved question")).toBeInTheDocument();
  const live = test.streams.at(-1)!;
  const historyCount = test.calls.filter(call => call.path.includes("/messages") && call.method === "GET").length;
  await user.type(screen.getByLabelText("메시지"), "Fresh receipt question");
  await user.dblClick(screen.getByRole("button", { name: "메시지 전송" }));
  const fresh = { ...turn({ id: "fresh-run", request_id: String(postCalls(test.calls)[0].body?.request_id) }, "Fresh receipt question"), sequence: 3 };
  await act(async () => { ack.resolve(response(fresh, 201)); });
  await act(async () => {
    test.emit({ sequence: 3, change: { kind: "turn", turn: fresh } }, live);
    test.emit({ sequence: 4, change: { kind: "delta", run_id: fresh.run.id, attempt: 1, message_id: "fresh-answer", text: "Immediate visible answer", phase: null, refusal: false } }, live);
  });
  expect(await screen.findByText("Immediate visible answer")).toBeInTheDocument();
  expect(screen.getByText("Older preserved question")).toBeInTheDocument();
  expect(live.init.signal?.aborted).toBe(false);
  expect(test.calls.filter(call => call.path.includes("/messages") && call.method === "GET")).toHaveLength(historyCount);
  expect(postCalls(test.calls)).toHaveLength(1);
});

it("resets reconnect failures after new validated events, while open-only failures remain bounded", async () => {
  const saved = turn();
  const test = fixture({ turns: [saved], cursor: 1 });
  open();
  await ready(test.streams);
  vi.useFakeTimers();
  try {
    for (let recovered = 0; recovered < 7; recovered++) {
      await act(async () => {
        test.emit({ sequence: 2 + recovered, change: { kind: "state", run: { ...saved.run, state: "running", version: 2 + recovered } } });
        await vi.advanceTimersByTimeAsync(40);
      });
      const live = test.streams.at(-1)!;
      await act(async () => {
        live.closed = true;
        live.controller.close();
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(test.streams.at(-1)).not.toBe(live);
    }
    expect(screen.queryByText(/자동 재연결을 멈췄습니다/)).not.toBeInTheDocument();
    const count = test.streams.length;
    for (let failure = 1; failure <= 5; failure++) {
      const live = test.streams.at(-1)!;
      await act(async () => {
        live.controller.enqueue(new TextEncoder().encode(": immediate comment\n\n"));
        await vi.advanceTimersByTimeAsync(0);
        live.closed = true;
        live.controller.close();
        await vi.advanceTimersByTimeAsync(8000);
      });
    }
    expect(screen.getByText(/자동 재연결을 멈췄습니다/)).toBeInTheDocument();
    expect(test.streams).toHaveLength(count + 4);
    await act(async () => {
      fireEvent(window, new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(test.streams).toHaveLength(count + 5);
    expect(screen.queryByText(/자동 재연결을 멈췄습니다/)).not.toBeInTheDocument();
    expect(postCalls(test.calls)).toHaveLength(0);
  } finally { vi.useRealTimers(); }
});

it("keeps quiet heartbeat connections healthy and recovers a half-open stream by GET only", async () => {
  const test = fixture();
  open();
  await ready(test.streams);
  vi.useFakeTimers();
  try {
    for (let recovered = 0; recovered < 7; recovered++) {
      const live = test.streams.at(-1)!;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(15000);
        live.controller.enqueue(new TextEncoder().encode(": keepalive\n\n"));
        await vi.advanceTimersByTimeAsync(0);
        live.closed = true;
        live.controller.close();
        await vi.advanceTimersByTimeAsync(500);
      });
      expect(test.streams.at(-1)).not.toBe(live);
    }
    const stalled = test.streams.at(-1)!;
    const histories = test.calls.filter(call => call.path.endsWith("/messages")).length;
    await act(async () => { await vi.advanceTimersByTimeAsync(46000); });
    expect(stalled.init.signal?.aborted).toBe(true);
    expect(test.streams.at(-1)).not.toBe(stalled);
    expect(test.calls.filter(call => call.path.endsWith("/messages"))).toHaveLength(histories + 1);
    expect(postCalls(test.calls)).toHaveLength(0);
  } finally { vi.useRealTimers(); }
});

it("falls back to a canonical page when a fresh accepted request has no SSE receipt", async () => {
  const user = userEvent.setup();
  const ack = deferred<Response>();
  const test = fixture({ post: () => ack.promise });
  open();
  await ready(test.streams);
  await user.type(screen.getByLabelText("메시지"), "Accepted without an event");
  await user.click(screen.getByRole("button", { name: "메시지 전송" }));
  const accepted = turn({ request_id: String(postCalls(test.calls)[0].body?.request_id) }, "Accepted without an event");
  test.state.page = { thread_id: thread.id, cursor: 1, turns: [accepted], next_before: null };
  const live = test.streams.at(-1)!;
  vi.useFakeTimers();
  try {
    await act(async () => { ack.resolve(response(accepted, 201)); await vi.advanceTimersByTimeAsync(1000); });
    expect(live.init.signal?.aborted).toBe(true);
    expect(test.streams.at(-1)?.path).toContain("after=1");
    expect(screen.getAllByText("Accepted without an event")).toHaveLength(1);
    expect(postCalls(test.calls)).toHaveLength(1);
  } finally { vi.useRealTimers(); }
});

it("shows model settings for an operation blocked by the selected model capability", async () => {
  const user = userEvent.setup();
  const initial = turn({state:"failed", error:"tools_required", can_retry:false});
  fixture({turns:[initial]});
  open();
  const settings = await screen.findByRole("button", {name:"내 모델에서 도구 확인"});
  expect(screen.getByText(/이 요청에는 도구가 필요합니다/)).toBeInTheDocument();
  expect(screen.queryByRole("button", {name:"같은 설정으로 다시 시도"})).not.toBeInTheDocument();
  await user.click(settings);
  expect(window.location.pathname).toBe("/settings/model");
});

it.each(["personal_chat_gpt", "personal_claude_code", "personal_compatible"] as const)("routes failed %s authentication to the owner's model connection", async source => {
  const user = userEvent.setup();
  fixture({turns:[turn({state:"failed", error:"credentials_rejected", model:{...choice, source}, can_retry:false})]});
  open();
  const settings = await screen.findByRole("button", {name:"내 모델 연결 확인"});
  expect(screen.getByText(/개인 모델 연결 인증에 실패했습니다/)).toBeInTheDocument();
  expect(screen.queryByText(/관리자에게 연결 확인을 요청하세요/)).not.toBeInTheDocument();
  await user.click(settings);
  expect(window.location.pathname).toBe("/settings/model");
});

it("logout fences a pending streamed receipt before its delayed GET fallback", async () => {
  const user = userEvent.setup();
  const ack = deferred<Response>();
  const test = fixture({ post: () => ack.promise });
  open();
  await ready(test.streams);
  await user.type(screen.getByLabelText("메시지"), "Do not restore this account after logout");
  await user.click(screen.getByRole("button", { name: "메시지 전송" }));
  const accepted = turn({ request_id: String(postCalls(test.calls)[0].body?.request_id) }, "Do not restore this account after logout");
  await act(async () => { ack.resolve(response(accepted, 201)); });
  const histories = test.calls.filter(call => call.path.endsWith("/messages") && call.method === "GET").length;
  await user.click(openWorkspaceMenu().getByRole("button", { name: "Sign out" }));
  await screen.findByRole("button", { name: "Sign in" });
  vi.useFakeTimers();
  try {
    await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
    expect(test.calls.filter(call => call.path.endsWith("/messages") && call.method === "GET")).toHaveLength(histories);
    expect(screen.queryByText("Do not restore this account after logout")).not.toBeInTheDocument();
  } finally { vi.useRealTimers(); }
});


it("a timed-out canonical snapshot leaves loading and permits explicit GET-only recovery", async () => {
  const held = deferred<Response>();
  let hold = true;
  const test = fixture({ history: () => hold ? held.promise : undefined });
  vi.useFakeTimers();
  try {
    open();
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByText("대화 기록을 불러오는 중…")).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(30000); });
    expect(screen.getByText("대화 기록을 불러오지 못했습니다. 새로고침해 주세요.")).toBeInTheDocument();
    expect(screen.queryByText("대화 기록을 불러오는 중…")).not.toBeInTheDocument();
    hold = false;
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "기록 새로고침" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(test.streams.some(stream => !stream.closed)).toBe(true);
    expect(screen.queryByText("대화 기록을 불러오지 못했습니다. 새로고침해 주세요.")).not.toBeInTheDocument();
    await act(async () => {
      held.resolve(response({ thread_id: thread.id, cursor: 1, turns: [turn({}, "Abandoned snapshot must stay hidden")], next_before: null }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.queryByText("Abandoned snapshot must stay hidden")).not.toBeInTheDocument();
    expect(postCalls(test.calls)).toHaveLength(0);
  } finally { vi.useRealTimers(); }
});


it("a hung send recovers as uncertain and explicit retry preserves its immutable request", async () => {
  const user = userEvent.setup();
  const held = deferred<Response>();
  let sends = 0;
  const test = fixture({ post: call => {
    if (++sends === 1) return held.promise;
    const accepted = turn({ request_id: String(call.body?.request_id) }, String(call.body?.text));
    test.state.page = { thread_id: thread.id, cursor: 1, turns: [accepted], next_before: null };
    return response(accepted, 201);
  }});
  open();
  await ready(test.streams);
  await user.type(screen.getByLabelText("메시지"), "Immutable timed-out question");
  vi.useFakeTimers();
  try {
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "메시지 전송" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(postCalls(test.calls)).toHaveLength(1);
    await act(async () => {
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "Unsent next draft" } });
      await vi.advanceTimersByTimeAsync(30000);
    });
    expect(screen.getByRole("button", { name: "같은 요청 다시 제출" })).toBeEnabled();
    expect(postCalls(test.calls)).toHaveLength(1);
    expect(postCalls(test.calls)[0].init.signal?.aborted).toBe(true);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 제출" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(postCalls(test.calls)).toHaveLength(2);
    expect(postCalls(test.calls)[1].body).toEqual(postCalls(test.calls)[0].body);
    expect(screen.getByLabelText("메시지")).toHaveValue("Unsent next draft");
    const cancel = vi.fn();
    await act(async () => {
      held.resolve(new Response(new ReadableStream({ cancel })));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(cancel).toHaveBeenCalledOnce();
    expect(screen.queryByText("전송 접수 확인 필요")).not.toBeInTheDocument();
    expect(postCalls(test.calls)).toHaveLength(2);
  } finally { vi.useRealTimers(); }
});


it("a canonical SSE receipt releases a hung send so the next completed-turn draft can send", async () => {
  const user = userEvent.setup();
  const test = fixture({ post: () => new Promise<Response>(() => {}) });
  open();
  await ready(test.streams);
  await user.type(screen.getByLabelText("메시지"), "Admitted while HTTP stalled");
  await user.click(screen.getByRole("button", { name: "메시지 전송" }));
  const admitted = turn({ request_id: String(postCalls(test.calls)[0].body?.request_id), state: "completed", can_cancel: false }, "Admitted while HTTP stalled");
  vi.useFakeTimers();
  try {
    await act(async () => {
      test.emit({ sequence: 1, change: { kind: "turn", turn: admitted } });
      await vi.advanceTimersByTimeAsync(40);
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "Next question" } });
    });
    expect(postCalls(test.calls)[0].init.signal?.aborted).toBe(true);
    expect(screen.getAllByText("Admitted while HTTP stalled")).toHaveLength(1);
    expect(screen.queryByText("전송 접수 확인 필요")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "메시지 전송" })).toBeEnabled();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "메시지 전송" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(postCalls(test.calls)).toHaveLength(2);
    expect(postCalls(test.calls)[1].body?.request_id).not.toBe(postCalls(test.calls)[0].body?.request_id);
    expect(postCalls(test.calls)[1].body?.text).toBe("Next question");
  } finally { vi.useRealTimers(); }
});

it.each(["cancel", "retry"] as const)("a hung %s reads canonical state and unlocks without replaying the action", async kind => {
  const initial = turn({ state: kind === "cancel" ? "running" : "failed", can_cancel: kind === "cancel", can_retry: kind === "retry" });
  const test = fixture({ turns: [initial], action: () => {
    test.state.page = { ...test.state.page, cursor: 2, turns: [turn({ state: kind === "cancel" ? "cancelled" : "running", attempt: kind === "cancel" ? 1 : 2, version: 2, can_cancel: kind === "retry", can_retry: false })] };
    return new Promise<Response>(() => {});
  }});
  open();
  await ready(test.streams);
  vi.useFakeTimers();
  try {
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: kind === "cancel" ? "응답 중지" : "같은 설정으로 다시 시도" }));
      await vi.advanceTimersByTimeAsync(30000);
    });
    const actions = test.calls.filter(call => call.path === `/runs/run-a/${kind}`);
    expect(actions).toHaveLength(1);
    expect(actions[0].init.signal?.aborted).toBe(true);
    expect(actions[0].body).toEqual({ expected_attempt: 1 });
    expect(screen.queryByText("처리 중…")).not.toBeInTheDocument();
    expect(screen.getByText(kind === "cancel" ? "중지됨" : "답변 중")).toBeInTheDocument();
    expect(postCalls(test.calls)).toHaveLength(0);
  } finally { vi.useRealTimers(); }
});

it.each(["composer", "button"])("a timed-out first creation and immutable message survive reload and explicitly retry both original identities via %s", async route => {
  const user = userEvent.setup();
  const held = deferred<Response>();
  let creates = 0;
  const test = fixture({ threads: [], extra: call => {
    if (call.path === "/threads" && call.method === "POST") {
      test.state.threads = [thread];
      if (++creates === 1) return held.promise;
      return response(thread, 201);
    }
  }});
  test.state.session = { ...account, chat_draft_scope: "a".repeat(64) };
  open();
  await screen.findByText("Alice님, 무엇을 도와드릴까요?");
  await user.type(screen.getByLabelText("메시지"), "Still unsent during thread creation");
  vi.useFakeTimers();
  try {
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "메시지 전송" }));
      await vi.advanceTimersByTimeAsync(0);
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "Separate next draft" } });
      await vi.advanceTimersByTimeAsync(30000);
    });
    expect(screen.getByLabelText("메시지")).toHaveValue("Separate next draft");
    expect(screen.getByText(/새 대화 생성 결과를 확인하지 못했습니다/)).toBeInTheDocument();
    expect(openWorkspaceMenu().getByRole("button", { name: "Synthetic conversation" })).toBeInTheDocument();
    expect(creates).toBe(1);
    expect(postCalls(test.calls)).toHaveLength(0);
    const encoded = JSON.parse(sessionStorage.getItem("moosuri.chat-drafts.v1")!);
    const originalMessage = encoded.slots.find((row: { id: string }) => row.id === "new").pending;
    expect(originalMessage.text).toBe("Still unsent during thread creation");
    expect(encoded.thread_creation.request_id).toMatch(/^[0-9a-f-]{36}$/);
    await act(async () => { cleanup(); open(); await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByLabelText("메시지")).toHaveValue("Separate next draft");
    expect(creates).toBe(1);
    expect(postCalls(test.calls)).toHaveLength(0);
    if (route === "button") {
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "새 대화 다시 확인" }));
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(postCalls(test.calls)).toHaveLength(0);
      expect(screen.getByLabelText("메시지")).toHaveValue("Separate next draft");
    }
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 제출" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    const creationCalls = test.calls.filter(call => call.path === "/threads" && call.method === "POST");
    expect(creationCalls).toHaveLength(2);
    expect(creationCalls[1].body).toEqual(creationCalls[0].body);
    expect(postCalls(test.calls)).toHaveLength(1);
    expect(postCalls(test.calls)[0].body).toMatchObject({ request_id: originalMessage.request_id, text: originalMessage.text, schedule_context: originalMessage.schedule_context });
    expect(screen.getByLabelText("메시지")).toHaveValue("Separate next draft");
    expect(screen.queryByRole("button", { name: "같은 요청 다시 제출" })).not.toBeInTheDocument();
    const saved = JSON.parse(sessionStorage.getItem("moosuri.chat-drafts.v1")!);
    expect(saved.thread_creation).toBeUndefined();
    expect(saved.slots.some((row: { id: string }) => row.id === "new")).toBe(false);
    const cancel = vi.fn();
    await act(async () => { held.resolve(new Response(new ReadableStream({ cancel }))); await vi.advanceTimersByTimeAsync(0); });
    expect(cancel).toHaveBeenCalledOnce();
    expect(postCalls(test.calls)).toHaveLength(1);
  } finally { vi.useRealTimers(); }
});


it("explicit empty-conversation recovery reuses the original creation UUID", async () => {
  let creates = 0;
  const test = fixture({ extra: call => {
    if (call.path === "/threads" && call.method === "POST") {
      if (++creates === 1) return new Promise<Response>(() => {});
      return response(thread, 201);
    }
  }});
  open();
  await ready(test.streams);
  vi.useFakeTimers();
  try {
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "새 대화" }));
      await vi.advanceTimersByTimeAsync(30000);
    });
    const original = test.calls.find(call => call.path === "/threads" && call.method === "POST")!;
    expect(original.body?.request_id).toMatch(/^[0-9a-f-]{36}$/);
    expect(creates).toBe(1);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "새 대화 다시 확인" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    const requests = test.calls.filter(call => call.path === "/threads" && call.method === "POST");
    expect(requests).toHaveLength(2);
    expect(requests[1].body).toEqual(original.body);
    expect(postCalls(test.calls)).toHaveLength(0);
  } finally { vi.useRealTimers(); }
});

it.each([404, 409])("a definite creation %s restores unsent text and clears only its unusable creation carrier", async status => {
  let creates = 0;
  const test = fixture({ threads: [], extra: call => {
    if (call.path === "/threads" && call.method === "POST") {
      return ++creates === 1 ? new Promise<Response>(() => {}) : response({ code: status === 404 ? "not_found" : "conflict" }, status);
    }
  }});
  test.state.session = { ...account, chat_draft_scope: "a".repeat(64) };
  open();
  await screen.findByText("Alice님, 무엇을 도와드릴까요?");
  vi.useFakeTimers();
  try {
    await act(async () => {
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "Original unsent message" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "메시지 전송" }));
      await vi.advanceTimersByTimeAsync(0);
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "Next draft" } });
      await vi.advanceTimersByTimeAsync(30000);
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 제출" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByLabelText("메시지")).toHaveValue("Original unsent message\n\nNext draft");
    expect(screen.getByRole("button", { name: "새 대화" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "같은 요청 다시 제출" })).not.toBeInTheDocument();
    expect(creates).toBe(2);
    const calls = test.calls.filter(call => call.path === "/threads" && call.method === "POST");
    expect(calls[0].body).toEqual(calls[1].body);
    expect(postCalls(test.calls)).toHaveLength(0);
    expect(JSON.parse(sessionStorage.getItem("moosuri.chat-drafts.v1")!).thread_creation).toBeUndefined();
  } finally { vi.useRealTimers(); }
});

it("a late explicit new-conversation receipt preserves the thread and draft selected while it was pending", async () => {
  const user = userEvent.setup();
  const held = deferred<Response>();
  const other = { ...thread, id: "thread-b", title: "Other conversation" };
  const created = { ...thread, id: "thread-new", title: "New conversation" };
  const test = fixture({ threads: [thread, other], extra: call => {
    if (call.path === "/threads" && call.method === "POST") {
      test.state.threads = [created, thread, other];
      return held.promise;
    }
    if (call.path === `/threads/${other.id}/messages`) return response({ thread_id: other.id, cursor: 0, turns: [], next_before: null });
  }});
  open();
  await ready(test.streams);
  await user.click(screen.getByRole("button", { name: "새 대화" }));
  await user.click(openWorkspaceMenu().getByRole("button", { name: "Other conversation" }));
  await screen.findByRole("heading", { name: "Other conversation" });
  await user.type(screen.getByLabelText("메시지"), "Other thread draft stays here");
  await act(async () => { held.resolve(response(created, 201)); });
  expect(screen.getByRole("heading", { name: "Other conversation" })).toBeInTheDocument();
  expect(screen.getByLabelText("메시지")).toHaveValue("Other thread draft stays here");
  expect(postCalls(test.calls)).toHaveLength(0);
});

it("creation recovery preserves both drafts when its admitted thread was selected and edited before recovery", async () => {
  const user = userEvent.setup();
  let creates = 0;
  const test = fixture({ threads: [], extra: call => {
    if (call.path === "/threads" && call.method === "POST") {
      test.state.threads = [thread];
      return ++creates === 1 ? new Promise<Response>(() => {}) : response(thread, 201);
    }
  }});
  test.state.session = { ...account, chat_draft_scope: "a".repeat(64) };
  open();
  await screen.findByText("Alice님, 무엇을 도와드릴까요?");
  await user.type(screen.getByLabelText("메시지"), "Immutable first request");
  vi.useFakeTimers();
  try {
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "메시지 전송" }));
      await vi.advanceTimersByTimeAsync(0);
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "Original separate next draft" } });
      await vi.advanceTimersByTimeAsync(30000);
    });
    const admitted = openWorkspaceMenu().getByRole("button", { name: thread.title });
    await act(async () => {
      fireEvent.click(admitted);
      await vi.advanceTimersByTimeAsync(0);
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "Draft written in admitted thread" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "새 대화 다시 확인" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByLabelText("메시지")).toHaveValue("Draft written in admitted thread");
    expect(screen.getByText(/새 대화에 다른 초안이나 접수 확인 중인 메시지가 있습니다/)).toBeInTheDocument();
    expect(postCalls(test.calls)).toHaveLength(0);
    let saved = JSON.parse(sessionStorage.getItem("moosuri.chat-drafts.v1")!);
    expect(saved.slots.find((row: { id: string }) => row.id === "new").draft).toBe("Original separate next draft");
    expect(saved.slots.find((row: { id: string }) => row.id === "new").pending.text).toBe("Immutable first request");
    expect(saved.thread_creation.request_id).toBe(test.calls.find(call => call.path === "/threads" && call.method === "POST")!.body?.request_id);
    await act(async () => {
      fireEvent.change(screen.getByLabelText("메시지"), { target: { value: "" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "새 대화 다시 확인" }));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByLabelText("메시지")).toHaveValue("Original separate next draft");
    expect(screen.getByRole("button", { name: "같은 요청 다시 제출" })).toBeEnabled();
    expect(postCalls(test.calls)).toHaveLength(0);
    saved = JSON.parse(sessionStorage.getItem("moosuri.chat-drafts.v1")!);
    expect(saved.thread_creation).toBeUndefined();
    expect(saved.slots.find((row: { id: string }) => row.id === thread.id).pending.text).toBe("Immutable first request");
    const creationCalls = test.calls.filter(call => call.path === "/threads" && call.method === "POST");
    expect(creationCalls).toHaveLength(3);
    expect(creationCalls.every(call => JSON.stringify(call.body) === JSON.stringify(creationCalls[0].body))).toBe(true);
  } finally { vi.useRealTimers(); }
});

it.each(["retry", "send"] as const)("holds the per-thread mutation fence when %s starts before the other operation", async first => {
  const user = userEvent.setup();
  const held = deferred<Response>();
  const initial = turn({ state: "failed", can_retry: true, can_cancel: false });
  const test = fixture({ turns: [initial], action: () => held.promise, post: () => held.promise });
  open();
  await ready(test.streams);
  await user.type(screen.getByLabelText("메시지"), "Keep this next question");
  await user.click(screen.getByRole("button", { name: first === "retry" ? "같은 설정으로 다시 시도" : "메시지 전송" }));
  if (first === "retry") {
    expect(screen.getByRole("button", { name: "메시지 전송" })).toBeDisabled();
    fireEvent.submit(screen.getByLabelText("메시지").closest("form")!);
    expect(postCalls(test.calls)).toHaveLength(0);
    expect(screen.getByLabelText("메시지")).toHaveValue("Keep this next question");
  } else {
    expect(screen.getByRole("button", { name: "같은 설정으로 다시 시도" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "같은 설정으로 다시 시도" }));
    expect(test.calls.filter(call => call.path.endsWith("/retry"))).toHaveLength(0);
    expect(postCalls(test.calls)).toHaveLength(1);
  }
});

it("retires a previous failed-turn retry control when SSE admits a later turn", async () => {
  const initial = turn({ state: "failed", can_retry: true, can_cancel: false });
  const test = fixture({ turns: [initial], cursor: 1 });
  open();
  await ready(test.streams);
  expect(screen.getByRole("button", { name: "같은 설정으로 다시 시도" })).toBeEnabled();
  const latest = { ...turn({ id: "later-run", request_id: "later-request", state: "completed", can_cancel: false, can_retry: false }, "Later question"), sequence: 2 };
  await act(async () => {
    test.emit({ sequence: 2, change: { kind: "turn", turn: latest } });
    await new Promise(resolve => setTimeout(resolve, 40));
  });
  expect(screen.queryByRole("button", { name: "같은 설정으로 다시 시도" })).not.toBeInTheDocument();
});

it("a canonical retry attempt releases its lost HTTP receipt so that attempt can be cancelled immediately", async () => {
  const user = userEvent.setup();
  const held = deferred<Response>();
  const initial = turn({ state: "failed", can_retry: true, can_cancel: false });
  const retried = { ...initial, run: { ...initial.run, state: "running" as const, attempt: 2, version: 2, can_retry: false, can_cancel: true } };
  const cancelled = { ...retried, run: { ...retried.run, state: "cancelled" as const, version: 3, can_cancel: false } };
  const test = fixture({ turns: [initial], cursor: 1, action: call => call.path.endsWith("/retry") ? held.promise : response(cancelled) });
  open();
  await ready(test.streams);
  await user.click(screen.getByRole("button", { name: "같은 설정으로 다시 시도" }));
  await act(async () => {
    test.emit({ sequence: 2, change: { kind: "turn", turn: retried } });
    await new Promise(resolve => setTimeout(resolve, 40));
  });
  expect(screen.getByRole("button", { name: "응답 중지" })).toBeEnabled();
  expect(test.calls.find(call => call.path.endsWith("/retry"))!.init.signal?.aborted).toBe(true);
  await user.click(screen.getByRole("button", { name: "응답 중지" }));
  await screen.findByText("중지됨");
  expect(test.calls.find(call => call.path.endsWith("/cancel"))!.body).toEqual({ expected_attempt: 2 });
  await act(async () => { held.resolve(response(retried)); });
  expect(screen.getByText("중지됨")).toBeInTheDocument();
  expect(test.calls.filter(call => call.path.endsWith("/retry"))).toHaveLength(1);
  expect(test.calls.filter(call => call.path.endsWith("/cancel"))).toHaveLength(1);
});

it("deletion preserves refreshed inventory and fences an older inventory response", async () => {
  const user = userEvent.setup();
  const deletion = deferred<Response>();
  const inventory = deferred<Response>();
  const other = { ...thread, id: "thread-b", title: "Other conversation" };
  const newest = { ...thread, id: "thread-c", title: "Newly refreshed conversation" };
  const initial = turn({ state: "running" });
  let holdInventory = false;
  const test = fixture({ turns: [initial], threads: [thread, other], cursor: 1, extra: call => {
    if (call.path === `/threads/${thread.id}` && call.method === "DELETE") return deletion.promise;
    if (call.path === "/threads" && call.method === "GET" && holdInventory) return inventory.promise;
    if (call.path === `/threads/${other.id}/messages`) return response({ thread_id: other.id, cursor: 0, turns: [], next_before: null });
  }});
  open();
  await ready(test.streams);
  await user.click(openWorkspaceMenu().getByRole("button", { name: `Delete ${thread.title}` }));
  await user.click(screen.getByRole("button", { name: "Delete conversation" }));
  test.state.threads = [thread, other, newest];
  await act(async () => {
    test.emit({ sequence: 2, change: { kind: "state", run: { ...initial.run, state: "completed", version: 2, can_cancel: false } } });
    await new Promise(resolve => setTimeout(resolve, 40));
  });
  await waitFor(() => expect(test.calls.filter(call => call.path === "/threads" && call.method === "GET").length).toBeGreaterThan(1));
  holdInventory = true;
  await act(async () => {
    test.emit({ sequence: 3, change: { kind: "state", run: { ...initial.run, state: "completed", version: 3, can_cancel: false } } });
    await new Promise(resolve => setTimeout(resolve, 40));
  });
  const stale = test.calls.filter(call => call.path === "/threads" && call.method === "GET").at(-1)!;
  await act(async () => { deletion.resolve(response(null, 204)); });
  expect(openWorkspaceMenu().getByRole("button", { name: newest.title })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: other.title })).toBeInTheDocument();
  expect(stale.init.signal?.aborted).toBe(true);
  const cancel = vi.fn();
  await act(async () => { inventory.resolve(new Response(new ReadableStream({ cancel }))); });
  expect(cancel).toHaveBeenCalledOnce();
  expect(openWorkspaceMenu().queryByRole("button", { name: thread.title })).not.toBeInTheDocument();
  expect(openWorkspaceMenu().getByRole("button", { name: newest.title })).toBeInTheDocument();
});

it("a rejected background admission cannot restore a deleted conversation's draft", async () => {
  const user = userEvent.setup();
  const creation = deferred<Response>();
  const admission = deferred<Response>();
  const other = { ...thread, id: "thread-b", title: "Other conversation" };
  const test = fixture({ threads: [other], post: () => admission.promise, extra: call => {
    if (call.path === "/threads" && call.method === "POST") return creation.promise;
    if (call.path === `/threads/${other.id}/messages`) return response({ thread_id: other.id, cursor: 0, turns: [], next_before: null });
  }});
  test.state.session = { ...account, chat_draft_scope: "a".repeat(64) };
  sessionStorage.setItem("moosuri.chat-drafts.v1", JSON.stringify({ version: 1, account: account.user.id, scope: "a".repeat(64), saved_at: Date.now(), selected: null, slots: [{ id: "new", draft: "Private text from removed background conversation" }] }));
  open();
  await screen.findByText("Alice님, 무엇을 도와드릴까요?");
  await waitFor(() => expect(screen.getByRole("button", { name: "메시지 전송" })).toBeEnabled());
  await user.click(screen.getByRole("button", { name: "메시지 전송" }));
  await user.click(openWorkspaceMenu().getByRole("button", { name: other.title }));
  test.state.threads = [thread, other];
  await act(async () => { creation.resolve(response(thread, 201)); });
  await waitFor(() => expect(postCalls(test.calls)).toHaveLength(1));
  await user.type(screen.getByLabelText("메시지"), "Other conversation draft remains");
  await user.click(openWorkspaceMenu().getByRole("button", { name: `Delete ${thread.title}` }));
  await user.click(screen.getByRole("button", { name: "Delete conversation" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  await act(async () => { admission.resolve(response({ code: "conflict" }, 409)); });
  expect(screen.getByLabelText("메시지")).toHaveValue("Other conversation draft remains");
  expect(sessionStorage.getItem("moosuri.chat-drafts.v1")).not.toContain("Private text from removed background conversation");
  expect(openWorkspaceMenu().queryByRole("button", { name: thread.title })).not.toBeInTheDocument();
});

it("canonical cancellation releases a lost HTTP receipt before the next explicit message", async () => {
  const user = userEvent.setup();
  const held = deferred<Response>();
  const initial = turn({ state: "running", version: 1 });
  const test = fixture({ turns: [initial], cursor: 1, action: () => held.promise });
  open();
  await ready(test.streams);
  await user.type(screen.getByLabelText("메시지"), "Next after confirmed stop");
  await user.click(screen.getByRole("button", { name: "응답 중지" }));
  expect(screen.getByRole("button", { name: "메시지 전송" })).toBeDisabled();
  await act(async () => {
    test.emit({ sequence: 2, change: { kind: "state", run: { ...initial.run, state: "cancelled", version: 2, can_cancel: false } } });
    await new Promise(resolve => setTimeout(resolve, 40));
  });
  expect(screen.getByRole("button", { name: "메시지 전송" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "메시지 전송" }));
  expect(postCalls(test.calls)).toHaveLength(1);
  expect(postCalls(test.calls)[0].body?.text).toBe("Next after confirmed stop");
  const cancel = vi.fn();
  await act(async () => { held.resolve(new Response(new ReadableStream({ cancel }))); });
  expect(cancel).toHaveBeenCalledOnce();
  expect(postCalls(test.calls)).toHaveLength(1);
  expect(test.calls.filter(call => call.path.endsWith("/cancel"))).toHaveLength(1);
});

it("authoritative thread absence releases its held DELETE without unlocking a newer deletion", async () => {
  const user = userEvent.setup();
  const first = deferred<Response>();
  const second = deferred<Response>();
  const other = { ...thread, id: "thread-b", title: "Other conversation" };
  const test = fixture({ threads: [thread, other], extra: call => {
    if (call.method === "DELETE") return call.path === `/threads/${thread.id}` ? first.promise : second.promise;
  }});
  open();
  await ready(test.streams);
  await user.click(openWorkspaceMenu().getByRole("button", { name: `Delete ${thread.title}` }));
  await user.click(screen.getByRole("button", { name: "Delete conversation" }));
  const original = test.calls.find(call => call.method === "DELETE")!;
  await act(async () => {
    test.streams.at(-1)!.controller.enqueue(new TextEncoder().encode('event: closed\ndata: {"code":"not_found"}\n\n'));
  });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(openWorkspaceMenu().getByRole("button", { name: `Delete ${other.title}` })).toBeEnabled();
  expect(original.init.signal?.aborted).toBe(true);
  await user.click(openWorkspaceMenu().getByRole("button", { name: `Delete ${other.title}` }));
  await user.click(screen.getByRole("button", { name: "Delete conversation" }));
  expect(test.calls.filter(call => call.method === "DELETE")).toHaveLength(2);
  const cancel = vi.fn();
  await act(async () => { first.resolve(new Response(new ReadableStream({ cancel }))); });
  expect(cancel).toHaveBeenCalledOnce();
  expect(screen.getByRole("dialog")).toHaveTextContent(other.title);
  expect(screen.getByRole("button", { name: "Deleting…" })).toBeDisabled();
  await act(async () => { second.resolve(response(null, 204)); });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(test.calls.filter(call => call.method === "DELETE")).toHaveLength(2);
});

it.each([
 ["attachment_reselection_required",false], ["attachment_reselection_required",true],
 ["vision_required",false], ["vision_required",true],
] as const)("recovers %s with retained input and %s existing media without an automatic resend",async(code,existing)=>{
 const user=userEvent.setup();
 const media={id:"11111111-1111-4111-8111-111111111111",revision:2,digest:"synthetic",state:"ready",width:32,height:32,bytes:30,created_at:1};
 const prior={...turn({state:"completed",can_cancel:false},"Earlier image question"),attachments:[media]};
 const model={...choice,source:code==="attachment_reselection_required"?"personal_chat_gpt":"personal_compatible"};
 const test=fixture({turns:existing?[prior]:[],extra:call=>{
  if(call.path==="/settings/model")return response({selection:null,effective:model,choices:[model]});
  if(call.path==="/media")return response({items:[media],used_bytes:30,quota_bytes:67108864});
  if(call.path.startsWith("/mail/reply-watches"))return response({targets:[],watches:[]});
 },post:()=>response({code},400)});
 open();await ready(test.streams);
 if(!existing){
  await user.click(screen.getByRole("button",{name:"첨부 추가"}));
  await user.click(screen.getByLabelText("내 이미지·파일 보관함"));
  await user.click(await screen.findByRole("button",{name:"대화에 첨부"}));
 }
 await user.type(screen.getByLabelText("메시지"),"Keep this image question");
 await user.click(screen.getByRole("button",{name:"메시지 전송"}));
 const alert=await screen.findByRole("alert");
 expect(alert).toHaveTextContent(code==="vision_required"?"현재 모델의 이미지 읽기가 확인되지 않았습니다":"이 대화에는 다른 연결로 보낸 첨부가 있습니다");
 expect(alert).toHaveTextContent(code==="vision_required"?"이전 메시지에도":"다시 선택");
 expect(screen.getByLabelText("메시지")).toHaveValue("Keep this image question");
 if(!existing)expect(screen.getByText("첨부 준비됨")).toBeInTheDocument();
 expect(screen.queryByRole("button",{name:"같은 요청 다시 제출"})).not.toBeInTheDocument();
 const settings=screen.getByRole("link",{name:"내 모델 확인 ↗"});
 expect(settings).toHaveAttribute("href","/settings/model");
 expect(settings).toHaveAttribute("target","_blank");
 await user.click(settings);
 const reads=test.calls.filter(call=>call.path==="/settings/model").length;
 await act(async()=>{window.dispatchEvent(new Event("focus"))});
 await waitFor(()=>expect(test.calls.filter(call=>call.path==="/settings/model").length).toBeGreaterThan(reads));
 expect(postCalls(test.calls)).toHaveLength(1);
 expect(screen.getByLabelText("메시지")).toHaveValue("Keep this image question");
 await waitFor(()=>expect(screen.getByRole("button",{name:"메시지 전송"})).toBeEnabled());
 await user.click(screen.getByRole("button",{name:"메시지 전송"}));
 await waitFor(()=>expect(postCalls(test.calls)).toHaveLength(2));
 expect(postCalls(test.calls)[1].body?.attachments).toEqual(existing?undefined:[{id:media.id,revision:2}]);
 expect(postCalls(test.calls)[1].body?.request_id).not.toBe(postCalls(test.calls)[0].body?.request_id);
});

it("keeps model-required guidance and the rejected draft until the owner saves a model and explicitly resends",async()=>{
 let available=true;
 const user=userEvent.setup();
 const test=fixture({extra:call=>{
  if(call.path==="/settings/model")return response({selection:null,effective:available?choice:null,choices:available?[choice]:[]});
  if(call.path.startsWith("/mail/reply-watches"))return response({targets:[],watches:[]});
 },post:()=>{available=false;return response({code:"model_required"},400)}});
 open();await ready(test.streams);
 await user.type(screen.getByLabelText("메시지"),"Preserve this request while configuring my model");
 await user.click(screen.getByRole("button",{name:"메시지 전송"}));
 expect(await screen.findByRole("alert")).toHaveTextContent("사용할 모델이 없습니다");
 expect(screen.getByRole("link",{name:"내 모델 확인 ↗"})).toHaveAttribute("target","_blank");
 await waitFor(()=>expect(screen.getByRole("button",{name:"메시지 전송"})).toBeDisabled());
 available=true;
 await act(async()=>{window.dispatchEvent(new Event("focus"))});
 await waitFor(()=>expect(screen.getByRole("button",{name:"메시지 전송"})).toBeEnabled());
 expect(screen.getByRole("alert")).toHaveTextContent("직접 다시 보내세요");
 expect(screen.getByLabelText("메시지")).toHaveValue("Preserve this request while configuring my model");
 expect(postCalls(test.calls)).toHaveLength(1);
 await user.click(screen.getByRole("button",{name:"메시지 전송"}));
 await waitFor(()=>expect(postCalls(test.calls)).toHaveLength(2));
});
