import { StrictMode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import {
  bytes,
  emptyMemory,
  memoryProblem,
  selectedEvidence,
} from "./KnowledgeState";
import type {
  EvidenceRef,
  IngestBatch,
  KnowledgeCollection,
  KnowledgeHit,
  MemoryDocument,
  MemoryDraft,
  SessionView,
  SourceDocument,
} from "../shared/api-types";

const collection: KnowledgeCollection = {
  source_connection_id: null,
  id: "11111111-1111-4111-8111-111111111111",
  name: "합성 자료 모음",
  scope: "연구 노트",
  revision: 1,
  cursor: 0,
  connected: true,
  deleted: false,
};
const source: SourceDocument = {
  provenance: null,
  source: {
    id: "22222222-2222-4222-8222-222222222222",
    collection_id: collection.id,
    external_id: "synthetic-external",
    version: "1",
    revision: 1,
    title: "합성 원문",
    speaker: null,
    observed_at: 100,
    valid_from: null,
    valid_until: null,
  },
  text: "앞🌱\r\n반복 인용\n반복 인용 끝",
};
const direct: MemoryDocument = {
  id: "33333333-3333-4333-8333-333333333333",
  revision: 1,
  status: "active",
  draft: {
    ...emptyMemory(),
    subject: "Original subject",
    text: "Original stored assertion",
  },
  created_at: 100,
  updated_at: 100,
};
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
const other: SessionView = {
  user: {
    id: "account-b",
    name: "Bob Example",
    email: "bob@example.test",
    role: "member",
    must_change_password: false,
  },
  password_change_required: false,
};
const response = (value: unknown, status = 200) =>
  new Response(status === 204 ? null : JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (value: unknown) => void;
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
function fixture(
  handler?: (call: Call) => Promise<Response> | Response | undefined,
  initialMemories: MemoryDocument[] = [],
) {
  const state = {
    session: account as SessionView | null,
    collections: [collection],
    sources: [source],
    memories: initialMemories,
    history: [direct],
  };
  const calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string, init: RequestInit = {}) => {
      const call: Call = {
        path: url.replace("/api/v1", ""),
        method: init.method ?? "GET",
        body: init.body ? JSON.parse(String(init.body)) : null,
        init,
      };
      calls.push(call);
      const handled = handler?.(call);
      if (handled) return Promise.resolve(handled);
      const { path, method, body } = call;
      if (path === "/system")
        return Promise.resolve(
          response({ initialized: true, stage: "knowledge", version: "0.1" }),
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
        state.session = other;
        state.collections = [];
        state.sources = [];
        state.memories = [];
        return Promise.resolve(response(other));
      }
      if (path === "/threads")
        return Promise.resolve(response({ threads: [] }));
      if (path === "/settings/model")
        return Promise.resolve(
          response({ selection: null, effective: null, choices: [] }),
        );
      if (path === "/knowledge/collections" && method === "GET")
        return Promise.resolve(response({ collections: state.collections }));
      if (path === "/knowledge/collections" && method === "POST") {
        const created = {
          ...collection,
          id: "44444444-4444-4444-8444-444444444444",
          name: String(body?.name),
          scope: String(body?.scope),
        };
        state.collections.push(created);
        return Promise.resolve(response(created, 201));
      }
      if (/^\/knowledge\/collections\/[^/]+\/sources/.test(path)) {
        const id = path.split("/")[3];
        return Promise.resolve(
          response({
            items: state.sources
              .filter((item) => item.source.collection_id === id)
              .map((item) => item.source),
            next_after: null,
            generation: 1,
          }),
        );
      }
      if (path.endsWith("/batches")) {
        const batch = body as unknown as IngestBatch;
        const id = path.split("/")[3];
        state.collections = state.collections.map((item) =>
          item.id === id
            ? {
                ...item,
                cursor: batch.next_cursor,
                revision: item.revision + 1,
              }
            : item,
        );
        const input = batch.entries[0];
        state.sources.push({
          provenance: null,
          source: {
            ...source.source,
            ...input,
            id: "55555555-5555-4555-8555-555555555555",
            collection_id: id,
            revision: 1,
          },
          text: input.text,
        });
        return Promise.resolve(
          response(
            {
              request_id: batch.request_id,
              cursor: batch.next_cursor,
              source_ids: ["55555555-5555-4555-8555-555555555555"],
            },
            201,
          ),
        );
      }
      if (path.endsWith("/disconnect")) {
        const id = path.split("/")[3];
        const updated = state.collections.find((item) => item.id === id)!;
        const value = {
          ...updated,
          revision: updated.revision + 1,
          connected: false,
          deleted: body?.mode === "delete",
        };
        state.collections = state.collections.map((item) =>
          item.id === id ? value : item,
        );
        if (value.deleted) {
          const ids = new Set(
            state.sources
              .filter((item) => item.source.collection_id === id)
              .map((item) => item.source.id),
          );
          state.sources = state.sources.filter(
            (item) => !ids.has(item.source.id),
          );
          state.memories = state.memories.filter(
            (item) =>
              !item.draft.evidence.some((evidence) =>
                ids.has(evidence.source_id),
              ),
          );
        }
        return Promise.resolve(response(value));
      }
      if (path.startsWith("/knowledge/sources/")) {
        const id = path.split("/")[3].split("?")[0];
        const document = state.sources.find((item) => item.source.id === id);
        if (method === "DELETE") {
          state.sources = state.sources.filter((item) => item.source.id !== id);
          state.memories = state.memories.filter(
            (item) =>
              !item.draft.evidence.some(
                (evidence) => evidence.source_id === id,
              ),
          );
          return Promise.resolve(response(null, 204));
        }
        return Promise.resolve(
          document ? response(document) : response({ code: "not_found" }, 404),
        );
      }
      if (path === "/knowledge/memories" && method === "GET")
        return Promise.resolve(
          response({ items: state.memories, next_after: null, generation: 1 }),
        );
      if (path === "/knowledge/memories" && method === "POST") {
        const value = { ...direct, draft: body?.draft as MemoryDraft };
        state.memories.push(value);
        state.history = [value];
        return Promise.resolve(response(value, 201));
      }
      if (path.startsWith("/knowledge/memories/")) {
        const id = path.split("/")[3].split("?")[0];
        if (path.includes("/history"))
          return Promise.resolve(
            response({ items: state.history, next_before: null }),
          );
        if (method === "DELETE") {
          state.memories = state.memories.filter((item) => item.id !== id);
          state.history = [];
          return Promise.resolve(response(null, 204));
        }
        const value = state.memories.find((item) => item.id === id);
        if (method === "PUT" && value) {
          const updated = {
            ...value,
            revision: value.revision + 1,
            draft: body?.draft as MemoryDraft,
          };
          state.memories = state.memories.map((item) =>
            item.id === id ? updated : item,
          );
          state.history.unshift(updated);
          return Promise.resolve(response(updated));
        }
        return Promise.resolve(
          value ? response(value) : response({ code: "not_found" }, 404),
        );
      }
      if (path === "/knowledge/search")
        return Promise.resolve(
          response({
            hits: [],
            next_cursor: null,
            truncated: false,
            generation: 1,
          }),
        );
      return Promise.resolve(response({ code: "unexpected_test_route" }, 404));
    }),
  );
  return { state, calls };
}
function open(path = "/app/knowledge") {
  window.history.replaceState(null, "", path);
  render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
const writes = (calls: Call[], suffix: string) =>
  calls.filter((call) => call.method === "POST" && call.path.endsWith(suffix));
beforeEach(() => {
  vi.stubGlobal("BroadcastChannel", undefined);
});

describe("private source material", () => {
  it("keeps managed collections separate from manual input and opens source receipt provenance", async () => {
    const test = fixture((call) => {
      if (
        call.path ===
        "/source-connections/managed-connection/receipts/request-a"
      )
        return response({
          request_id: "request-a",
          run_id: "run-a",
          sequence: 4,
          committed_at: 130,
          accepted: 1,
          unchanged: 0,
          source_ids: [source.source.id],
        });
    });
    test.state.collections = [
      { ...collection, source_connection_id: "managed-connection" },
    ];
    test.state.sources = [
      {
        ...source,
        provenance: {
          connection_id: "managed-connection",
          connection_name: "연구 Slack",
          kind: "slack",
          scope: "C_SYNTHETIC",
          external_id: "object-a",
          external_version: "v2",
          thread_id: "thread-a",
          parent_id: "parent-a",
          request_id: "request-a",
          run_id: "run-a",
          source_revision: 1,
          received_at: 130,
        },
      },
    ];
    open();
    expect(
      await screen.findByRole("button", { name: "자료 붙여넣기" }),
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "모음 연결 해제" }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole("link", { name: "수집 설정과 연결 해제" }),
    ).toHaveAttribute(
      "href",
      "/settings/sources?connection=managed-connection",
    );
    await userEvent.click(
      await screen.findByRole("button", { name: /합성 원문/ }),
    );
    await screen.findByRole("region", { name: "원문 수집 출처" });
    expect(screen.getByText("연구 Slack · Slack")).toBeInTheDocument();
    await userEvent.click(screen.getByText("원본과 저장 기록"));
    expect(screen.getByText("thread-a")).toBeInTheDocument();
    expect(screen.getByText("parent-a")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "수집 저장 기록 확인" }),
    );
    await screen.findByText(/^자료 저장 확인:/);
    expect(
      test.calls.some(
        (call) =>
          call.path ===
          "/source-connections/managed-connection/receipts/request-a",
      ),
    ).toBe(true);
    expect(screen.getByRole("button", { name: "원문 삭제" })).toBeEnabled();
  });

  it("navigates to source settings through the account navigation", async () => {
    fixture((call) => {
      if (call.path === "/source-connections")
        return response({ connections: [], gmail_configured: false });
    });
    open();
    await userEvent.click(
      await screen.findByRole("button", { name: /자료 수집 연결/ }),
    );
    await screen.findByRole("heading", { level: 2, name: "자료 수집 연결" });
    expect(window.location.pathname).toBe("/settings/sources");
    expect(
      screen.getByRole("button", { name: "커스텀 수집 연결 추가" }),
    ).toBeInTheDocument();
  });

  it("creates a manual collection from its name and pastes one source", async () => {
    const user = userEvent.setup();
    const test = fixture();
    open();
    await user.click(
      await screen.findByRole("button", { name: "모음 만들기" }),
    );
    await user.type(screen.getByLabelText("모음 이름"), "New manual notes");
    expect(screen.queryByLabelText("포함 범위")).not.toBeInTheDocument();
    expect(screen.queryByText(collection.scope)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "모음 저장" }));
    await screen.findByText("자료 모음을 만들었습니다.");
    const created = writes(test.calls, "/knowledge/collections")[0];
    expect(created.body).toEqual({
      name: "New manual notes",
      scope: expect.stringMatching(/^[0-9a-f-]{36}$/),
    });
    expect(
      screen.queryByText(String(created.body?.scope)),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "자료 붙여넣기" }));
    await user.type(screen.getByLabelText("자료 제목"), "Pasted title");
    await user.type(screen.getByLabelText("자료 본문"), "Pasted source text");
    await user.click(screen.getByRole("button", { name: "자료 저장" }));
    await screen.findByText("자료를 저장했습니다.");
    const batch = writes(test.calls, "/batches")[0];
    expect(batch.path).toContain("44444444-4444-4444-8444-444444444444");
    expect(batch.body).toMatchObject({
      expected_cursor: 0,
      next_cursor: 1,
      complete_snapshot: false,
      entries: [
        {
          version: "1",
          expected_revision: null,
          title: "Pasted title",
          text: "Pasted source text",
        },
      ],
    });
    expect(
      (batch.body?.entries as Array<{ external_id: string }>)[0].external_id,
    ).toMatch(/^[0-9a-f-]{36}$/);
    expect(
      test.calls
        .filter((call) => call.path.startsWith("/knowledge/"))
        .every(
          (call) =>
            new Headers(call.init.headers).get("X-Moosuri-Account") ===
            account.user.id,
        ),
    ).toBe(true);
  });

  it("reconciles a lost collection ACK by its retained opaque scope across route changes", async () => {
    const user = userEvent.setup();
    const test = fixture((call) =>
      call.path === "/knowledge/collections" && call.method === "POST"
        ? Promise.reject(new TypeError("Synthetic lost response"))
        : undefined,
    );
    open();
    await user.click(
      await screen.findByRole("button", { name: "모음 만들기" }),
    );
    await user.type(screen.getByLabelText("모음 이름"), collection.name);
    await user.click(screen.getByRole("button", { name: "모음 저장" }));
    await screen.findByRole("button", { name: "모음 생성 결과 확인" });
    const request = writes(test.calls, "/knowledge/collections")[0];
    expect(request.body?.scope).toMatch(/^[0-9a-f-]{36}$/);
    await user.click(screen.getByRole("button", { name: "기억" }));
    await user.click(screen.getByRole("button", { name: "자료" }));
    expect(await screen.findByLabelText("모음 이름")).toBeDisabled();
    await user.click(
      screen.getByRole("button", { name: "모음 생성 결과 확인" }),
    );
    await waitFor(() =>
      expect(writes(test.calls, "/knowledge/collections")).toHaveLength(2),
    );
    expect(writes(test.calls, "/knowledge/collections")[1].body).toEqual(
      request.body,
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "모음 생성 결과 확인" }),
      ).toBeEnabled(),
    );
    const created = {
      ...collection,
      id: "44444444-4444-4444-8444-444444444444",
      scope: String(request.body?.scope),
    };
    test.state.collections.push(created);
    await user.click(
      screen.getByRole("button", { name: "모음 생성 결과 확인" }),
    );
    await screen.findByText("자료 모음을 만들었습니다.");
    expect(screen.getByLabelText("자료 모음")).toHaveValue(created.id);
    expect(screen.queryByText(created.scope)).not.toBeInTheDocument();
    expect(writes(test.calls, "/knowledge/collections")).toHaveLength(2);
  });

  it.each([false, true])(
    "abandons a reconciled create conflict and starts a fresh collection (deleted=%s)",
    async (deleted) => {
      const user = userEvent.setup();
      let lost = true;
      const test = fixture((call) =>
        call.path === "/knowledge/collections" && call.method === "POST" && lost
          ? Promise.reject(new TypeError("Synthetic lost response"))
          : undefined,
      );
      open();
      await user.click(
        await screen.findByRole("button", { name: "모음 만들기" }),
      );
      await user.type(screen.getByLabelText("모음 이름"), "First creation");
      await user.click(screen.getByRole("button", { name: "모음 저장" }));
      await screen.findByRole("button", { name: "모음 생성 결과 확인" });
      const original = writes(test.calls, "/knowledge/collections")[0].body!;
      test.state.collections.push({
        ...collection,
        id: "44444444-4444-4444-8444-444444444444",
        name: String(original.name),
        scope: String(original.scope),
        connected: false,
        deleted,
      });
      await user.click(
        screen.getByRole("button", { name: "모음 생성 결과 확인" }),
      );
      await screen.findByRole("button", { name: "생성 초안 버리기" });
      expect(screen.getByLabelText("모음 이름")).toBeDisabled();
      expect(writes(test.calls, "/knowledge/collections")).toHaveLength(1);
      await user.click(screen.getByRole("button", { name: "기억" }));
      await user.click(screen.getByRole("button", { name: "자료" }));
      await user.click(
        await screen.findByRole("button", { name: "모음 만들기" }),
      );
      expect(screen.getByLabelText("모음 이름")).toHaveValue("First creation");
      expect(writes(test.calls, "/knowledge/collections")).toHaveLength(1);
      await user.click(
        screen.getByRole("button", { name: "생성 초안 버리기" }),
      );
      expect(screen.queryByLabelText("모음 이름")).not.toBeInTheDocument();
      expect(
        test.state.collections.find((item) => item.scope === original.scope),
      ).toMatchObject({ connected: false, deleted });
      await user.click(screen.getByRole("button", { name: "모음 만들기" }));
      expect(screen.getByLabelText("모음 이름")).toBeEnabled();
      expect(screen.getByLabelText("모음 이름")).toHaveValue("");
      await user.type(screen.getByLabelText("모음 이름"), "Fresh creation");
      lost = false;
      await user.click(screen.getByRole("button", { name: "모음 저장" }));
      await screen.findByText("자료 모음을 만들었습니다.");
      const fresh = writes(test.calls, "/knowledge/collections")[1].body!;
      expect(fresh.name).toBe("Fresh creation");
      expect(fresh.scope).not.toBe(original.scope);
      expect(fresh.scope).toMatch(/^[0-9a-f-]{36}$/);
    },
  );

  it("keeps a lost-ACK ingest UUID, source identity, cursors and body across route changes", async () => {
    const user = userEvent.setup();
    const first = deferred<Response>();
    let attempts = 0;
    const test = fixture((call) =>
      call.path.endsWith("/batches")
        ? ++attempts === 1
          ? first.promise
          : response(
              {
                request_id: call.body?.request_id,
                cursor: 1,
                source_ids: [source.source.id],
              },
              201,
            )
        : undefined,
    );
    open();
    await screen.findByRole("button", { name: /합성 원문/ });
    await user.click(screen.getByRole("button", { name: "자료 붙여넣기" }));
    await user.type(screen.getByLabelText("자료 제목"), "Preserved title");
    await user.type(
      screen.getByLabelText("자료 본문"),
      "Preserved source draft",
    );
    await user.click(screen.getByRole("button", { name: "자료 저장" }));
    await user.click(screen.getByRole("button", { name: "기억" }));
    expect(writes(test.calls, "/batches")[0].init.signal?.aborted).toBe(true);
    await user.click(screen.getByRole("button", { name: "자료" }));
    const retry = await screen.findByRole("button", {
      name: "같은 자료 요청 다시 확인",
    });
    expect(screen.getByLabelText("자료 본문")).toHaveValue(
      "Preserved source draft",
    );
    expect(screen.getByLabelText("자료 본문")).toBeDisabled();
    await user.click(retry);
    await screen.findByText("자료를 저장했습니다.");
    expect(writes(test.calls, "/batches")).toHaveLength(2);
    expect(writes(test.calls, "/batches")[1].body).toEqual(
      writes(test.calls, "/batches")[0].body,
    );
    await act(async () => {
      first.resolve(
        response(
          {
            request_id: writes(test.calls, "/batches")[0].body?.request_id,
            cursor: 1,
            source_ids: [source.source.id],
          },
          201,
        ),
      );
    });
    expect(screen.queryByLabelText("자료 본문")).not.toBeInTheDocument();
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
  });

  it("uses the selected occurrence and exact UTF-8 span as memory evidence", async () => {
    const user = userEvent.setup();
    const test = fixture();
    open();
    await user.click(await screen.findByRole("button", { name: /합성 원문/ }));
    const text = (await screen.findByLabelText(
      "원문 텍스트",
    )) as HTMLTextAreaElement;
    const selected = "반복 인용";
    const start = text.value.lastIndexOf(selected);
    await user.click(text);
    text.setSelectionRange(start, start + selected.length);
    fireEvent.select(text);
    await user.click(
      await screen.findByRole("button", { name: "이 구간을 기억 근거로" }),
    );
    await user.type(
      screen.getByLabelText("기억 내용"),
      "An explicitly written fact backed by the selected source",
    );
    await user.click(screen.getByRole("button", { name: "기억 저장" }));
    await screen.findByText("기억을 저장했습니다.");
    const request = writes(test.calls, "/knowledge/memories")[0];
    const draft = request.body?.draft as MemoryDraft;
    const rawStart = source.text.lastIndexOf(selected);
    expect(draft.evidence).toEqual([
      {
        source_id: source.source.id,
        source_revision: 1,
        start_byte: bytes(source.text.slice(0, rawStart)),
        end_byte: bytes(source.text.slice(0, rawStart + selected.length)),
        quote: selected,
      },
    ]);
    expect(
      test.calls.some((call) =>
        call.path.includes(`/sources/${source.source.id}?revision=1`),
      ),
    ).toBe(true);
    expect(draft.text).toBe(
      "An explicitly written fact backed by the selected source",
    );
  });

  it.each(["keep", "delete"] as const)(
    "confirms the exact %s collection scope",
    async (mode) => {
      const user = userEvent.setup();
      const span = selectedEvidence(source, 0, 1)!;
      const dependent = {
        ...direct,
        draft: { ...direct.draft, evidence: [span] },
      };
      const independent = {
        ...direct,
        id: "66666666-6666-4666-8666-666666666666",
        draft: { ...direct.draft, subject: "Independent assertion" },
      };
      const test = fixture(undefined, [dependent, independent]);
      open();
      await user.click(
        await screen.findByRole("button", { name: /합성 원문/ }),
      );
      const text = (await screen.findByLabelText(
        "원문 텍스트",
      )) as HTMLTextAreaElement;
      await user.click(text);
      text.setSelectionRange(0, 1);
      fireEvent.select(text);
      await user.click(
        await screen.findByRole("button", { name: "이 구간을 기억 근거로" }),
      );
      await user.type(
        screen.getByLabelText("기억 내용"),
        "Staged dependent memory",
      );
      await user.click(screen.getByRole("button", { name: "모음 연결 해제" }));
      const dialog = screen.getByRole("dialog");
      await user.click(
        within(dialog).getByRole("radio", {
          name: mode === "keep" ? /자료와 기억 보관/ : /원문과 연관 기억 삭제/,
        }),
      );
      await user.click(
        within(dialog).getByRole("button", {
          name: mode === "keep" ? "자료 보관 후 해제" : "자료 삭제 후 해제",
        }),
      );
      await waitFor(() =>
        expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
      );
      expect(writes(test.calls, "/disconnect")[0].body).toEqual({
        expected_revision: 1,
        mode,
      });
      expect(screen.queryByLabelText("기억 내용") !== null).toBe(
        mode === "keep",
      );
      if (mode === "keep") {
        expect(
          await screen.findByRole("button", { name: /합성 원문/ }),
        ).toBeInTheDocument();
        expect(
          screen.getByRole("button", { name: "자료 붙여넣기" }),
        ).toBeDisabled();
      } else
        expect(
          screen.queryByRole("button", { name: /합성 원문/ }),
        ).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "기억" }));
      await screen.findByRole("button", { name: /Independent assertion/ });
      expect(
        screen.queryByRole("button", { name: /Original subject/ }) !== null,
      ).toBe(mode === "keep");
    },
  );

  it("deletes a retained collection after keeping its sources and memories", async () => {
    const user = userEvent.setup();
    const dependent = {
      ...direct,
      draft: { ...direct.draft, evidence: [selectedEvidence(source, 0, 1)!] },
    };
    const independent = {
      ...direct,
      id: "66666666-6666-4666-8666-666666666666",
      draft: { ...direct.draft, subject: "Independent assertion" },
    };
    const test = fixture(undefined, [dependent, independent]);
    open();
    await screen.findByRole("button", { name: /합성 원문/ });
    await user.click(screen.getByRole("button", { name: "모음 연결 해제" }));
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "자료 보관 후 해제",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    await user.click(
      await screen.findByRole("button", { name: "보관된 모음 삭제" }),
    );
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("보관된 모음의 원문");
    expect(dialog).toHaveTextContent("수정 이력을 삭제");
    expect(dialog).toHaveTextContent("독립적으로 기록한 기억은 유지");
    expect(within(dialog).queryByRole("radio")).not.toBeInTheDocument();
    await user.click(
      within(dialog).getByRole("button", { name: "모음과 연관 기억 삭제" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(writes(test.calls, "/disconnect").map((call) => call.body)).toEqual([
      { expected_revision: 1, mode: "keep" },
      { expected_revision: 2, mode: "delete" },
    ]);
    expect(
      screen.queryByRole("button", { name: /합성 원문/ }),
    ).not.toBeInTheDocument();
    expect(test.state.collections[0]).toMatchObject({
      connected: false,
      deleted: true,
    });
    await user.click(screen.getByRole("button", { name: "기억" }));
    await screen.findByRole("button", { name: /Independent assertion/ });
    expect(
      screen.queryByRole("button", { name: /Original subject/ }),
    ).not.toBeInTheDocument();
  });

  it("labels raw-source deletion and clears source-dependent saved and staged memories only", async () => {
    const user = userEvent.setup();
    const dependent = {
      ...direct,
      draft: { ...direct.draft, evidence: [selectedEvidence(source, 0, 1)!] },
    };
    const independent = {
      ...direct,
      id: "66666666-6666-4666-8666-666666666666",
      draft: { ...direct.draft, subject: "Independent assertion" },
    };
    const test = fixture(undefined, [dependent, independent]);
    open();
    await user.click(await screen.findByRole("button", { name: /합성 원문/ }));
    const input = (await screen.findByLabelText(
      "원문 텍스트",
    )) as HTMLTextAreaElement;
    await user.click(input);
    input.setSelectionRange(0, 1);
    fireEvent.select(input);
    await user.click(
      await screen.findByRole("button", { name: "이 구간을 기억 근거로" }),
    );
    await user.type(
      screen.getByLabelText("기억 내용"),
      "Unstored source-dependent draft",
    );
    await user.click(screen.getByRole("button", { name: "원문 삭제" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("작성 중 기억도 지워집니다");
    await user.click(
      within(dialog).getByRole("button", { name: "원문과 연관 기억 삭제" }),
    );
    await screen.findByText("원문과 그 원문에 의존하는 기억을 삭제했습니다.");
    expect(screen.queryByLabelText("원문 텍스트")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("기억 내용")).not.toBeInTheDocument();
    const action = test.calls.find((call) => call.method === "DELETE")!;
    expect(action.path).toBe(`/knowledge/sources/${source.source.id}`);
    expect(action.body).toEqual({ expected_revision: 1 });
    await user.click(screen.getByRole("button", { name: "기억" }));
    await screen.findByRole("button", { name: /Independent assertion/ });
    expect(
      screen.queryByRole("button", { name: /Original subject/ }),
    ).not.toBeInTheDocument();
  });
});

describe("assertion editing and forgetting", () => {
  it("preserves a correction on conflict and rebases only after an explicit latest-version comparison", async () => {
    const user = userEvent.setup();
    let writes = 0;
    const newer = {
      ...direct,
      revision: 2,
      draft: { ...direct.draft, text: "Server changed assertion" },
    };
    const test = fixture(
      (call) => {
        if (
          call.path === `/knowledge/memories/${direct.id}` &&
          call.method === "PUT" &&
          ++writes === 1
        ) {
          test.state.memories = [newer];
          return response({ code: "conflict" }, 409);
        }
        return undefined;
      },
      [direct],
    );
    open("/app/memory");
    await user.click(
      await screen.findByRole("button", { name: /Original subject/ }),
    );
    await user.click(
      within(await screen.findByLabelText("기억 읽기")).getByRole("button", {
        name: "기억 수정",
      }),
    );
    await user.clear(screen.getByLabelText("기억 내용"));
    await user.type(
      screen.getByLabelText("기억 내용"),
      "My preserved correction",
    );
    await user.click(screen.getByRole("button", { name: "기억 저장" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "작성한 내용은 유지했습니다",
    );
    expect(screen.getByLabelText("기억 내용")).toHaveValue(
      "My preserved correction",
    );
    await user.click(screen.getByRole("button", { name: "최신 기억과 비교" }));
    await screen.findByText("Server changed assertion");
    expect(test.calls.filter((call) => call.method === "PUT")).toHaveLength(1);
    await user.click(
      screen.getByRole("button", { name: "내 수정안으로 최신 버전 고치기" }),
    );
    expect(screen.getByLabelText("기억 내용")).toHaveValue(
      "My preserved correction",
    );
    expect(test.calls.filter((call) => call.method === "PUT")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "기억 저장" }));
    await screen.findByText("기억을 저장했습니다.");
    const calls = test.calls.filter((call) => call.method === "PUT");
    expect(calls[0].body?.expected_revision).toBe(1);
    expect(calls[1].body?.expected_revision).toBe(2);
    expect(calls[1].body?.request_id).not.toBe(calls[0].body?.request_id);
    expect((calls[1].body?.draft as MemoryDraft).text).toBe(
      "My preserved correction",
    );
  });

  it("retains a memory creation request after a lost ACK instead of creating a new assertion", async () => {
    const user = userEvent.setup();
    const failed = deferred<Response>();
    let attempts = 0;
    const test = fixture((call) =>
      call.path === "/knowledge/memories" && call.method === "POST"
        ? ++attempts === 1
          ? failed.promise
          : response({ ...direct, draft: call.body?.draft }, 201)
        : undefined,
    );
    open("/app/memory");
    await user.click(await screen.findByRole("button", { name: "기억 추가" }));
    await user.type(screen.getByLabelText("기억 주제"), "My assertion");
    await user.type(
      screen.getByLabelText("기억 내용"),
      "Preserved assertion text",
    );
    await user.click(screen.getByRole("button", { name: "기억 저장" }));
    await act(async () => {
      failed.reject(new TypeError("synthetic lost acknowledgement"));
    });
    expect(
      await screen.findByRole("button", { name: "같은 기억 요청 다시 확인" }),
    ).toBeEnabled();
    expect(screen.getByLabelText("기억 내용")).toHaveValue(
      "Preserved assertion text",
    );
    expect(screen.getByLabelText("기억 내용")).toBeDisabled();
    await user.click(
      screen.getByRole("button", { name: "같은 기억 요청 다시 확인" }),
    );
    await screen.findByText("기억을 저장했습니다.");
    const calls = test.calls.filter(
      (call) => call.path === "/knowledge/memories" && call.method === "POST",
    );
    expect(calls).toHaveLength(2);
    expect(calls[1].body).toEqual(calls[0].body);
    expect((calls[0].body?.draft as MemoryDraft).evidence).toEqual([]);
  });

  it("reads revision history and forgets only the memory while retaining its raw source", async () => {
    const user = userEvent.setup();
    const test = fixture(undefined, [direct]);
    open("/app/memory");
    await user.click(
      await screen.findByRole("button", { name: /Original subject/ }),
    );
    const detail = await screen.findByLabelText("기억 읽기");
    await user.click(within(detail).getByRole("button", { name: "수정 이력" }));
    await screen.findByText("저장 버전 1");
    await user.click(within(detail).getByRole("button", { name: "기억 잊기" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("원문 자료는 삭제하지 않습니다");
    await user.click(
      within(dialog).getByRole("button", { name: "기억과 이력 지우기" }),
    );
    await screen.findByText(
      "기억과 수정 이력을 지웠습니다. 원문은 유지됩니다.",
    );
    expect(screen.queryByText("저장 버전 1")).not.toBeInTheDocument();
    expect(
      test.calls
        .filter((call) => call.method === "DELETE")
        .map((call) => call.path),
    ).toEqual([`/knowledge/memories/${direct.id}`]);
    await user.click(screen.getByRole("button", { name: "자료" }));
    await user.click(await screen.findByRole("button", { name: /합성 원문/ }));
    expect(await screen.findByLabelText("원문 텍스트")).toHaveValue(
      source.text.replace(/\r\n?/g, "\n"),
    );
  });
});

describe("knowledge lifetime boundaries", () => {
  it("does not replace a newer search with a late response to an abandoned query", async () => {
    const user = userEvent.setup();
    const late = deferred<Response>();
    const hit = (title: string): KnowledgeHit => ({
      kind: "source",
      id: source.source.id,
      revision: 1,
      title,
      snippet: "Synthetic matching snippet",
      subject: null,
      speaker: null,
      valid_from: null,
      valid_until: null,
    });
    const test = fixture((call) =>
      call.path === "/knowledge/search"
        ? call.body?.text === "first"
          ? late.promise
          : response({
              hits: [hit("New query result")],
              next_cursor: null,
              truncated: false,
              generation: 1,
            })
        : undefined,
    );
    open();
    await screen.findByRole("button", { name: /합성 원문/ });
    await user.type(screen.getByLabelText("자료 검색"), "first");
    await user.click(screen.getByRole("button", { name: "검색" }));
    const old = test.calls.find((call) => call.path === "/knowledge/search")!;
    await user.clear(screen.getByLabelText("자료 검색"));
    await user.type(screen.getByLabelText("자료 검색"), "second");
    await user.click(screen.getByRole("button", { name: "검색" }));
    await screen.findByText("New query result");
    expect(old.init.signal?.aborted).toBe(true);
    await act(async () => {
      late.resolve(
        response({
          hits: [hit("Old query result")],
          next_cursor: null,
          truncated: false,
          generation: 1,
        }),
      );
    });
    expect(screen.queryByText("Old query result")).not.toBeInTheDocument();
    expect(screen.getByText("New query result")).toBeInTheDocument();
  });

  it("does not apply an old source reader to a newly selected source", async () => {
    const user = userEvent.setup();
    const old = deferred<Response>();
    const second = {
      provenance: null,
      source: {
        ...source.source,
        id: "77777777-7777-4777-8777-777777777777",
        title: "Second source",
      },
      text: "Second source actual text",
    };
    const test = fixture((call) =>
      call.path.startsWith(`/knowledge/sources/${source.source.id}`)
        ? old.promise
        : undefined,
    );
    test.state.sources = [source, second];
    open();
    await user.click(await screen.findByRole("button", { name: /합성 원문/ }));
    await user.click(screen.getByRole("button", { name: /Second source/ }));
    expect(await screen.findByLabelText("원문 텍스트")).toHaveValue(
      second.text,
    );
    await act(async () => {
      old.resolve(response(source));
    });
    expect(screen.getByLabelText("원문 텍스트")).toHaveValue(second.text);
    expect(
      test.calls.find((call) =>
        call.path.startsWith(`/knowledge/sources/${source.source.id}`),
      )?.init.signal?.aborted,
    ).toBe(true);
  });

  it("discards an account's late editor result and all draft content on sign-out", async () => {
    const user = userEvent.setup();
    const late = deferred<Response>();
    const test = fixture((call) =>
      call.path === "/knowledge/memories" && call.method === "POST"
        ? late.promise
        : undefined,
    );
    open("/app/memory");
    await user.click(await screen.findByRole("button", { name: "기억 추가" }));
    await user.type(screen.getByLabelText("기억 주제"), "Private A subject");
    await user.type(
      screen.getByLabelText("기억 내용"),
      "Private A memory draft",
    );
    await user.click(screen.getByRole("button", { name: "기억 저장" }));
    const request = test.calls.find(
      (call) => call.path === "/knowledge/memories" && call.method === "POST",
    )!;
    await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
    await user.type(await screen.findByLabelText("Email"), other.user.email);
    await user.type(screen.getByLabelText("Password"), "synthetic-password");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByRole("heading", { name: "Hello, Bob." });
    await user.click(screen.getByRole("button", { name: "기억" }));
    expect(request.init.signal?.aborted).toBe(true);
    await act(async () => {
      late.resolve(response({ ...direct, draft: request.body?.draft }, 201));
    });
    expect(
      screen.queryByDisplayValue("Private A subject"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByDisplayValue("Private A memory draft"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("기억을 저장했습니다.")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "기억 추가" }),
    ).toBeInTheDocument();
  });
});

it("enforces evidence and memory byte boundaries without guessing a repeated quote", () => {
  const visible = source.text.replace(/\r\n?/g, "\n");
  const start = visible.lastIndexOf("반복 인용");
  const evidence = selectedEvidence(source, start, start + "반복 인용".length)!;
  expect(evidence.quote).toBe("반복 인용");
  expect(evidence.start_byte).toBe(
    bytes(source.text.slice(0, source.text.lastIndexOf("반복 인용"))),
  );
  expect(selectedEvidence(source, 2, 3)).toBeNull(); // Half of the emoji's UTF-16 surrogate pair.
  expect(
    memoryProblem({
      ...emptyMemory(),
      subject: "Synthetic subject",
      text: "가".repeat(1366),
    }),
  ).toContain("4KiB");
  const large: EvidenceRef[] = Array.from({ length: 4 }, (_, index) => ({
    source_id: String(index),
    source_revision: 1,
    start_byte: 0,
    end_byte: 4096,
    quote: "x".repeat(4096),
  }));
  expect(
    memoryProblem({
      ...emptyMemory(),
      subject: "Synthetic subject",
      text: "Synthetic memory",
      evidence: large,
    }),
  ).toContain("16KiB");
});
