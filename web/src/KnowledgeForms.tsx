import {
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
  type FormEvent,
} from "react";
import type {
  IngestBatch,
  IngestReceipt,
  KnowledgeCollection,
  MemoryDocument,
  MemoryDraft,
} from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";
import {
  bytes,
  dateInput,
  localDate,
  memoryProblem,
  type KnowledgeDrafts,
  type MemoryEditorState,
  type PasteState,
} from "./KnowledgeState";

export type DraftSetter = Dispatch<SetStateAction<KnowledgeDrafts>>;
export function knowledgeError(error: unknown) {
  if (error instanceof ApiFailure) {
    if (error.status === 409)
      return "최신 상태와 충돌했습니다. 작성한 내용은 유지했습니다. 최신 내용을 확인해 주세요.";
    if (error.status === 404)
      return "이 항목은 더 이상 사용할 수 없습니다. 작성한 내용은 유지했습니다.";
    if (error.status === 403) return "이 자료에 접근할 수 없습니다.";
    if (error.status === 400 || error.status === 413 || error.status === 422)
      return "입력 크기, 유효 기간 또는 근거 구간을 확인해 주세요. 작성한 내용은 유지했습니다.";
  }
  return "서버 응답을 확인하지 못했습니다. 작성한 내용과 요청을 보관했습니다.";
}
export function TimeFields({
  from,
  until,
  disabled,
  change,
}: {
  from: number | null;
  until: number | null;
  disabled: boolean;
  change: (from: number | null, until: number | null) => void;
}) {
  return (
    <div className="knowledge-fields-pair">
      <label>
        유효 시작
        <input
          type="datetime-local"
          step="1"
          value={localDate(from)}
          disabled={disabled}
          onChange={(event) => change(dateInput(event.target.value), until)}
        />
      </label>
      <label>
        유효 종료
        <input
          type="datetime-local"
          step="1"
          value={localDate(until)}
          disabled={disabled}
          onChange={(event) => change(from, dateInput(event.target.value))}
        />
      </label>
    </div>
  );
}

export function PasteForm({
  accountId,
  state,
  update,
  refreshCollection,
  saved,
  onExpired,
}: {
  accountId: string;
  state: PasteState;
  update: DraftSetter;
  refreshCollection: (id: string) => Promise<KnowledgeCollection | null>;
  saved: (receipt: IngestReceipt, collection: KnowledgeCollection) => void;
  onExpired: () => void;
}) {
  const scope = useScope(accountId, `paste:${state.collection.id}`);
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const [error, setError] = useState("");
  function patch(value: Partial<PasteState>) {
    update((current) =>
      current.paste?.collection.id === state.collection.id
        ? { ...current, paste: { ...current.paste, ...value } }
        : current,
    );
  }
  const frozen = state.issue === "uncertain";
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (lock.current || state.issue === "conflict") return;
    setError("");
    if (
      !state.title.trim() ||
      bytes(state.title) > 200 ||
      !state.text.trim() ||
      bytes(state.text) > 64 * 1024 ||
      bytes(state.speaker) > 200
    ) {
      setError(
        "제목은 200바이트, 본문은 64KiB, 화자는 200바이트 이내로 입력해 주세요.",
      );
      return;
    }
    if (
      state.valid_from !== null &&
      state.valid_until !== null &&
      state.valid_from >= state.valid_until
    ) {
      setError("유효 종료 시각을 시작 시각 뒤로 지정해 주세요.");
      return;
    }
    const batch: IngestBatch = state.request ?? {
      request_id: crypto.randomUUID(),
      expected_cursor: state.collection.cursor,
      next_cursor: state.collection.cursor + 1,
      complete_snapshot: false,
      entries: [
        {
          external_id: crypto.randomUUID(),
          version: "1",
          expected_revision: null,
          title: state.title,
          text: state.text,
          speaker: state.speaker.trim() || null,
          observed_at: state.observed_at,
          valid_from: state.valid_from,
          valid_until: state.valid_until,
        },
      ],
    };
    patch({ request: batch, issue: "uncertain" });
    lock.current = true;
    setBusy(true);
    try {
      const receipt = await scope.request<IngestReceipt>(
        `/knowledge/collections/${encodeURIComponent(state.collection.id)}/batches`,
        "POST",
        batch,
      );
      update((current) =>
        current.paste?.request?.request_id === batch.request_id
          ? { ...current, paste: null }
          : current,
      );
      saved(receipt, state.collection);
    } catch (error) {
      if (error instanceof ApiFailure && error.status === 401) onExpired();
      else {
        const uncertain =
          isAborted(error) ||
          !(error instanceof ApiFailure) ||
          error.status >= 500;
        update((current) =>
          current.paste?.request?.request_id === batch.request_id
            ? {
                ...current,
                paste: {
                  ...current.paste,
                  issue: uncertain
                    ? "uncertain"
                    : error instanceof ApiFailure && error.status === 409
                      ? "conflict"
                      : null,
                  request: uncertain ? batch : null,
                },
              }
            : current,
        );
        if (!isAborted(error)) setError(knowledgeError(error));
      }
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  async function refreshBase() {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    try {
      const collection = await refreshCollection(state.collection.id);
      if (collection?.connected && !collection.deleted) {
        patch({ collection, request: null, issue: null });
        setError(
          "최신 모음 상태를 확인했습니다. 작성한 내용을 다시 저장할 수 있습니다.",
        );
      } else
        setError(
          "이 모음에는 더 이상 자료를 추가할 수 없습니다. 작성한 내용은 유지했습니다.",
        );
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="knowledge-editor admin-card" aria-label="자료 붙여넣기">
      <div className="section-heading">
        <div>
          <h3>자료 붙여넣기</h3>
          <p>{state.collection.name}에 새 텍스트 자료로 저장합니다.</p>
        </div>
        <button
          className="text-button"
          disabled={busy}
          onClick={() => patch({ open: false })}
        >
          접기
        </button>
      </div>
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      {frozen && !busy && (
        <p className="knowledge-pending" role="status">
          저장 여부를 확인하지 못했습니다. 입력을 유지하고 같은 요청으로 다시
          확인할 수 있습니다.
        </p>
      )}
      <form onSubmit={submit}>
        <label>
          자료 제목
          <input
            required
            value={state.title}
            disabled={busy || frozen}
            onChange={(event) => patch({ title: event.target.value })}
          />
        </label>
        <label>
          자료 본문
          <textarea
            rows={8}
            required
            value={state.text}
            disabled={busy || frozen}
            onChange={(event) => patch({ text: event.target.value })}
          />
        </label>
        <details>
          <summary>화자·유효 기간</summary>
          <label>
            자료 화자 (선택)
            <input
              value={state.speaker}
              disabled={busy || frozen}
              onChange={(event) => patch({ speaker: event.target.value })}
            />
          </label>
          <TimeFields
            from={state.valid_from}
            until={state.valid_until}
            disabled={busy || frozen}
            change={(valid_from, valid_until) =>
              patch({ valid_from, valid_until })
            }
          />
        </details>
        <p className="fine">
          본문은 UTF-8 기준 64KiB까지 저장합니다. 파일 업로드나 URL 가져오기는
          이 화면에서 지원하지 않습니다.
        </p>
        <div className="knowledge-actions">
          {state.issue === "conflict" ? (
            <button
              className="secondary"
              type="button"
              disabled={busy}
              onClick={refreshBase}
            >
              최신 모음 상태 확인
            </button>
          ) : (
            <button
              className="primary"
              disabled={busy || !state.collection.connected}
            >
              {busy
                ? "저장 확인 중…"
                : frozen
                  ? "같은 자료 요청 다시 확인"
                  : "자료 저장"}
            </button>
          )}
          {!frozen && (
            <button
              className="text-button"
              type="button"
              disabled={busy}
              onClick={() => update((current) => ({ ...current, paste: null }))}
            >
              작성 내용 지우기
            </button>
          )}
        </div>
      </form>
    </section>
  );
}

export function MemoryForm({
  accountId,
  state,
  update,
  saved,
  onExpired,
  openEvidence,
}: {
  accountId: string;
  state: MemoryEditorState;
  update: DraftSetter;
  saved: (document: MemoryDocument) => void;
  onExpired: () => void;
  openEvidence: (sourceId: string, revision: number) => void;
}) {
  const scope = useScope(accountId, `memory-editor:${state.id ?? "new"}`);
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const [error, setError] = useState("");
  const [latest, setLatest] = useState<MemoryDocument | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  function patch(value: Partial<MemoryEditorState>) {
    update((current) =>
      current.memory?.id === state.id
        ? { ...current, memory: { ...current.memory, ...value } }
        : current,
    );
  }
  function field(value: Partial<MemoryDraft>) {
    patch({ draft: { ...state.draft, ...value } });
  }
  const frozen = state.issue === "uncertain";
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (lock.current || state.issue === "conflict") return;
    setError("");
    const problem = memoryProblem(state.draft);
    if (problem) {
      setError(problem);
      return;
    }
    const request = state.request ?? {
      request_id: crypto.randomUUID(),
      draft: {
        ...state.draft,
        speaker: state.draft.speaker?.trim() || null,
        evidence: state.draft.evidence.map((item) => ({ ...item })),
      },
      id: state.id,
      expected_revision: state.expected_revision,
    };
    patch({ request, issue: "uncertain" });
    lock.current = true;
    setBusy(true);
    try {
      const body = {
        request_id: request.request_id,
        draft: request.draft,
        ...(request.id ? { expected_revision: request.expected_revision } : {}),
      };
      const value = await scope.request<MemoryDocument>(
        request.id
          ? `/knowledge/memories/${encodeURIComponent(request.id)}`
          : "/knowledge/memories",
        request.id ? "PUT" : "POST",
        body,
      );
      update((current) =>
        current.memory?.request?.request_id === request.request_id
          ? { ...current, memory: null }
          : current,
      );
      saved(value);
    } catch (error) {
      if (error instanceof ApiFailure && error.status === 401) onExpired();
      else {
        const uncertain =
          isAborted(error) ||
          !(error instanceof ApiFailure) ||
          error.status >= 500;
        update((current) =>
          current.memory?.request?.request_id === request.request_id
            ? {
                ...current,
                memory: {
                  ...current.memory,
                  issue: uncertain
                    ? "uncertain"
                    : error instanceof ApiFailure && error.status === 409
                      ? "conflict"
                      : null,
                  request: uncertain ? request : null,
                },
              }
            : current,
        );
        if (!isAborted(error)) setError(knowledgeError(error));
      }
    } finally {
      lock.current = false;
      if (alive.current) setBusy(false);
    }
  }
  async function inspectLatest() {
    if (!state.id || lock.current) return;
    lock.current = true;
    setBusy(true);
    try {
      setLatest(
        await scope.request<MemoryDocument>(
          `/knowledge/memories/${encodeURIComponent(state.id)}`,
        ),
      );
    } catch (error) {
      if (!isAborted(error)) {
        if (error instanceof ApiFailure && error.status === 401) onExpired();
        else setError(knowledgeError(error));
      }
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return (
    <section className="knowledge-editor admin-card" aria-label="기억 작성">
      <div className="section-heading">
        <div>
          <h3>{state.id ? "기억 수정" : "기억 작성"}</h3>
          <p>
            {state.draft.evidence.length
              ? "선택한 원문의 실제 구간을 근거로 저장합니다."
              : "원문 근거 없이 내가 직접 기록하는 기억입니다."}
          </p>
        </div>
        <button
          className="text-button"
          disabled={busy}
          onClick={() => patch({ open: false })}
        >
          접기
        </button>
      </div>
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      {frozen && !busy && (
        <p className="knowledge-pending" role="status">
          저장 여부가 불명확합니다. 작성한 내용과 같은 요청 ID를 유지하며
          자동으로 재전송하지 않습니다.
        </p>
      )}
      <form onSubmit={submit}>
        <label>
          기억 주제
          <input
            required
            value={state.draft.subject}
            disabled={busy || frozen}
            onChange={(event) => field({ subject: event.target.value })}
          />
        </label>
        <label>
          기억 내용
          <textarea
            rows={5}
            required
            value={state.draft.text}
            disabled={busy || frozen}
            onChange={(event) => field({ text: event.target.value })}
          />
        </label>
        <details>
          <summary>화자·유효 기간</summary>
          <label>
            기억 화자 (선택)
            <input
              value={state.draft.speaker ?? ""}
              disabled={busy || frozen}
              onChange={(event) =>
                field({ speaker: event.target.value || null })
              }
            />
          </label>
          <TimeFields
            from={state.draft.valid_from}
            until={state.draft.valid_until}
            disabled={busy || frozen}
            change={(valid_from, valid_until) =>
              field({ valid_from, valid_until })
            }
          />
        </details>
        {state.draft.evidence.length > 0 && (
          <div className="memory-evidence">
            <h4>연결한 근거</h4>
            {state.draft.evidence.map((item, index) => (
              <div
                className="evidence-card"
                key={`${item.source_id}:${item.source_revision}:${item.start_byte}:${item.end_byte}`}
              >
                <blockquote>{item.quote}</blockquote>
                <div className="knowledge-actions">
                  <button
                    type="button"
                    className="text-button"
                    onClick={() =>
                      openEvidence(item.source_id, item.source_revision)
                    }
                  >
                    원문 구간 확인
                  </button>
                  <button
                    type="button"
                    className="text-button"
                    disabled={busy || frozen}
                    onClick={() =>
                      field({
                        evidence: state.draft.evidence.filter(
                          (_, position) => position !== index,
                        ),
                      })
                    }
                  >
                    이 근거 제거
                  </button>
                </div>
              </div>
            ))}
            <p className="fine">
              근거를 모두 제거하면 직접 기록한 기억으로 저장됩니다.
            </p>
          </div>
        )}
        <p className="fine">
          기억 내용은 4KiB, 근거를 포함한 전체 작성 내용은 16KiB 이내로
          저장합니다.
        </p>
        <div className="knowledge-actions">
          {state.issue === "conflict" ? (
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={inspectLatest}
            >
              최신 기억과 비교
            </button>
          ) : (
            <button className="primary" disabled={busy}>
              {busy
                ? "저장 확인 중…"
                : frozen
                  ? "같은 기억 요청 다시 확인"
                  : "기억 저장"}
            </button>
          )}
          {!frozen && (
            <button
              type="button"
              className="text-button"
              disabled={busy}
              onClick={() =>
                update((current) => ({ ...current, memory: null }))
              }
            >
              작성 내용 지우기
            </button>
          )}
        </div>
      </form>
      {latest && (
        <div className="knowledge-conflict">
          <h4>현재 저장된 기억</h4>
          <strong>{latest.draft.subject}</strong>
          <p>{latest.draft.text}</p>
          <p className="fine">
            내 수정안은 위 입력창에 그대로 남아 있습니다. 최신 버전에 내
            수정안을 적용할 때만 아래 버튼을 누르세요.
          </p>
          <button
            className="secondary"
            disabled={busy || frozen}
            onClick={() => {
              patch({
                expected_revision: latest.revision,
                request: null,
                issue: null,
              });
              setLatest(null);
              setError("");
            }}
          >
            내 수정안으로 최신 버전 고치기
          </button>
        </div>
      )}
    </section>
  );
}
