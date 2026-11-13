import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import type {
  ChatEvidence,
  EvidenceRef,
  KnowledgeCollection,
  KnowledgeHit,
  KnowledgeQuery,
  KnowledgeSearch,
  MemoryDocument,
  MemoryHistory,
  MemoryPage,
  SourceDocument,
  SourcePage,
} from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";
import {
  MemoryForm,
  PasteForm,
  knowledgeError,
  type DraftSetter,
} from "./KnowledgeForms";
import {
  bytes,
  dateInput,
  localDate,
  selectedEvidence,
  startMemory,
  startPaste,
  type KnowledgeDrafts,
} from "./KnowledgeState";
import "./knowledge.css";
import { MemoryExtraction } from "./MemoryExtraction";
import { SourceProvenance } from "./SourceProvenance";

type Mode = "source" | "memory";
type Resource = { id: string; revision: number };
type Danger =
  | { kind: "source" | "memory"; id: string; revision: number; label: string }
  | { kind: "collection"; collection: KnowledgeCollection };

export function KnowledgeView({
  accountId,
  mode,
  drafts,
  updateDrafts,
  onExpired,
  target,
  onTargetConsumed,
}: {
  accountId: string;
  mode: Mode;
  drafts: KnowledgeDrafts;
  updateDrafts: DraftSetter;
  onExpired: () => void;
  target?: ChatEvidence | null;
  onTargetConsumed?: () => void;
}) {
  const scope = useScope(accountId, `knowledge:${mode}`);
  const [collections, setCollections] = useState<KnowledgeCollection[]>([]);
  const [collectionId, setCollectionId] = useState<string | null>(null);
  const [sources, setSources] = useState<SourcePage | null>(null);
  const [memories, setMemories] = useState<MemoryPage | null>(null);
  const [pageStack, setPageStack] = useState<Array<string | null>>([null]);
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [source, setSource] = useState<Resource | null>(null);
  const [memory, setMemory] = useState<Resource | null>(null);
  const [danger, setDanger] = useState<Danger | null>(null);
  const [busy, setBusy] = useState(false);
  const mutationLock = useRef(false);
  const [query, setQuery] = useState("");
  const [speaker, setSpeaker] = useState("");
  const [subject, setSubject] = useState("");
  const [validAt, setValidAt] = useState<number | null>(null);
  const [search, setSearch] = useState<KnowledgeSearch | null>(null);
  const [searched, setSearched] = useState<KnowledgeQuery | null>(null);
  const [searching, setSearching] = useState(false);
  const searchController = useRef<AbortController | null>(null);
  const searchGeneration = useRef(0);
  const currentCollection =
    collections.find(
      (collection) => collection.id === collectionId && !collection.deleted,
    ) ?? null;
  function fail(error: unknown) {
    if (isAborted(error)) return;
    if (error instanceof ApiFailure && error.status === 401) onExpired();
    else setError(knowledgeError(error));
  }
  function clearSearch() {
    searchGeneration.current++;
    searchController.current?.abort();
    setSearch(null);
    setSearched(null);
    setSearching(false);
  }
  function refreshLists() {
    clearSearch();
    setPageStack([null]);
    setRefresh((value) => value + 1);
  }
  useEffect(() => {
    clearSearch();
    setQuery("");
    setSpeaker("");
    setSubject("");
    setValidAt(null);
    setPageStack([null]);
    setSource(null);
    setMemory(null);
    setError("");
    setNotice("");
    return () => {
      searchGeneration.current++;
      searchController.current?.abort();
    };
  }, [mode]);
  useEffect(() => {
    if (!target || target.kind !== mode) return;
    if (target.kind === "source")
      setSource({ id: target.id, revision: target.revision });
    else setMemory({ id: target.id, revision: target.revision });
    onTargetConsumed?.();
  }, [target, mode]);
  useEffect(() => {
    if (mode !== "source") return;
    const controller = new AbortController();
    setLoading(true);
    void scope
      .request<{ collections: KnowledgeCollection[] }>(
        "/knowledge/collections",
        "GET",
        undefined,
        controller.signal,
      )
      .then((value) => {
        setCollections(value.collections);
        setCollectionId((current) =>
          value.collections.some(
            (collection) => collection.id === current && !collection.deleted,
          )
            ? current
            : (value.collections.find((collection) => !collection.deleted)
                ?.id ?? null),
        );
        if (!value.collections.some((collection) => !collection.deleted)) {
          setSources(null);
          setLoading(false);
        }
      })
      .catch((error) => {
        if (!isAborted(error)) {
          fail(error);
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [mode, scope, refresh]);
  useEffect(() => {
    const controller = new AbortController();
    const after = pageStack.at(-1);
    if (mode === "source" && !collectionId) {
      setSources(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError("");
    const base =
      mode === "source"
        ? `/knowledge/collections/${encodeURIComponent(collectionId!)}/sources`
        : "/knowledge/memories";
    void scope
      .request<SourcePage | MemoryPage>(
        `${base}${after ? `?after=${encodeURIComponent(after)}` : ""}`,
        "GET",
        undefined,
        controller.signal,
      )
      .then((value) => {
        if (mode === "source") setSources(value as SourcePage);
        else setMemories(value as MemoryPage);
        setLoading(false);
      })
      .catch((error) => {
        if (!isAborted(error)) {
          fail(error);
          setLoading(false);
        }
      });
    return () => controller.abort();
  }, [mode, collectionId, pageStack, scope, refresh]);
  async function refreshCollection(id: string) {
    try {
      const value = await scope.request<{ collections: KnowledgeCollection[] }>(
        "/knowledge/collections",
      );
      setCollections(value.collections);
      return (
        value.collections.find((collection) => collection.id === id) ?? null
      );
    } catch (error) {
      fail(error);
      return null;
    }
  }
  async function createCollection(event: FormEvent) {
    event.preventDefault();
    const draft = drafts.collection;
    if (!draft || mutationLock.current) return;
    if (!draft.name.trim() || bytes(draft.name) > 200) {
      setError("모음 이름을 200바이트 이내로 입력해 주세요.");
      return;
    }
    mutationLock.current = true;
    setBusy(true);
    setError("");
    updateDrafts((current) =>
      current.collection
        ? { ...current, collection: { ...current.collection, uncertain: true } }
        : current,
    );
    try {
      let result: KnowledgeCollection;
      if (draft.uncertain) {
        const value = await scope.request<{
          collections: KnowledgeCollection[];
        }>("/knowledge/collections");
        setCollections(value.collections);
        const found = value.collections.find(
          (item) => item.scope === draft.scope,
        );
        if (
          found &&
          (!found.connected ||
            found.deleted ||
            found.name !== draft.name.trim())
        )
          throw new ApiFailure(409, "conflict");
        result =
          found ??
          (await scope.request<KnowledgeCollection>(
            "/knowledge/collections",
            "POST",
            { name: draft.name.trim(), scope: draft.scope },
          ));
      } else {
        result = await scope.request<KnowledgeCollection>(
          "/knowledge/collections",
          "POST",
          { name: draft.name.trim(), scope: draft.scope },
        );
        setCollections((current) => [...current, result]);
      }
      setCollectionId(result.id);
      updateDrafts((current) => ({ ...current, collection: null }));
      setNotice("자료 모음을 만들었습니다.");
      refreshLists();
    } catch (error) {
      updateDrafts((current) =>
        current.collection
          ? {
              ...current,
              collection: {
                ...current.collection,
                conflict:
                  draft.uncertain &&
                  error instanceof ApiFailure &&
                  error.status === 409,
                uncertain:
                  draft.uncertain ||
                  isAborted(error) ||
                  !(error instanceof ApiFailure) ||
                  error.status >= 500,
              },
            }
          : current,
      );
      fail(error);
    } finally {
      mutationLock.current = false;
      setBusy(false);
    }
  }
  async function runSearch(cursor: string | null = null) {
    if (!cursor && (!query.trim() || bytes(query) > 128)) {
      setError("검색어를 128바이트 이내로 입력해 주세요.");
      return;
    }
    if (
      bytes(speaker) > 200 ||
      bytes(subject) > 200 ||
      (validAt !== null && !Number.isSafeInteger(validAt))
    ) {
      setError("검색 조건을 확인해 주세요.");
      return;
    }
    searchController.current?.abort();
    const controller = new AbortController();
    searchController.current = controller;
    const generation = ++searchGeneration.current;
    const request: KnowledgeQuery =
      cursor && searched
        ? { ...searched, cursor }
        : {
            kind: mode,
            text: query.trim(),
            collection_id: mode === "source" ? collectionId : null,
            subject: mode === "memory" ? subject.trim() || null : null,
            speaker: speaker.trim() || null,
            valid_at: validAt,
            cursor: null,
          };
    setSearching(true);
    setError("");
    if (!cursor) setSearch(null);
    try {
      const result = await scope.request<KnowledgeSearch>(
        "/knowledge/search",
        "POST",
        request,
        controller.signal,
      );
      if (generation !== searchGeneration.current) return;
      setSearch(result);
      setSearched(request);
      setSource(null);
      setMemory(null);
    } catch (error) {
      if (generation === searchGeneration.current) fail(error);
    } finally {
      if (generation === searchGeneration.current) setSearching(false);
    }
  }
  function startAssertion(document?: MemoryDocument) {
    if (drafts.memory) {
      updateDrafts((current) => ({
        ...current,
        memory: current.memory ? { ...current.memory, open: true } : null,
      }));
      if (drafts.memory.id !== (document?.id ?? null))
        setError(
          "작성 중인 기억을 먼저 저장하거나 작성 내용을 지워 주세요. 기존 수정안은 유지했습니다.",
        );
      return;
    }
    updateDrafts((current) => ({ ...current, memory: startMemory(document) }));
  }
  function addEvidence(evidence: EvidenceRef, title: string) {
    if (drafts.memory?.issue === "uncertain") {
      setError(
        "기억 저장 결과를 먼저 확인해 주세요. 같은 요청의 근거를 바꾸지 않았습니다.",
      );
      return;
    }
    const existing = drafts.memory?.draft.evidence ?? [];
    if (
      existing.some(
        (item) =>
          item.source_id === evidence.source_id &&
          item.source_revision === evidence.source_revision &&
          item.start_byte === evidence.start_byte &&
          item.end_byte === evidence.end_byte,
      )
    ) {
      setError("이미 연결한 원문 구간입니다.");
      return;
    }
    if (existing.length >= 8) {
      setError("하나의 기억에는 근거를 최대 8개까지 연결할 수 있습니다.");
      return;
    }
    updateDrafts((current) => {
      const editor = current.memory ?? startMemory();
      return {
        ...current,
        memory: {
          ...editor,
          open: true,
          draft: {
            ...editor.draft,
            subject: editor.draft.subject || title,
            evidence: [...editor.draft.evidence, evidence],
          },
        },
      };
    });
    setError("");
    setNotice(
      "선택한 원문 구간을 기억 근거에 연결했습니다. 기억 내용을 작성해 저장하세요.",
    );
  }
  async function destroy(modeChoice: "keep" | "delete") {
    const target = danger;
    if (!target || mutationLock.current) return;
    mutationLock.current = true;
    setBusy(true);
    setError("");
    try {
      if (target.kind === "collection") {
        // Evidence may belong to sources outside the current page. Resolve
        // ownership before deletion, while the confirmed dialog holds the UI.
        const removed = new Set<string>();
        if (modeChoice === "delete") {
          const evidence = drafts.memory?.draft.evidence ?? [];
          const checked = await Promise.allSettled(
            evidence.map((item) =>
              scope.request<SourceDocument>(
                `/knowledge/sources/${encodeURIComponent(item.source_id)}?revision=${item.source_revision}`,
              ),
            ),
          );
          for (const [index, result] of checked.entries()) {
            if (result.status === "fulfilled") {
              if (result.value.source.collection_id === target.collection.id)
                removed.add(evidence[index].source_id);
            } else if (
              result.reason instanceof ApiFailure &&
              result.reason.status === 404
            ) {
              removed.add(evidence[index].source_id);
            } else throw result.reason;
          }
        }
        await scope.request<KnowledgeCollection>(
          `/knowledge/collections/${encodeURIComponent(target.collection.id)}/disconnect`,
          "POST",
          { expected_revision: target.collection.revision, mode: modeChoice },
        );
        if (modeChoice === "delete") {
          updateDrafts((current) => ({
            ...current,
            paste:
              current.paste?.collection.id === target.collection.id
                ? null
                : current.paste,
            memory: current.memory?.draft.evidence.some((item) =>
              removed.has(item.source_id),
            )
              ? null
              : current.memory,
          }));
          setCollectionId(null);
        }
        setNotice(
          modeChoice === "keep"
            ? "연결을 해제하고 기존 자료와 기억을 보관했습니다."
            : "모음의 원문과 그 원문에 의존하는 기억을 삭제했습니다.",
        );
      } else {
        await scope.request(
          `/knowledge/${target.kind === "source" ? "sources" : "memories"}/${encodeURIComponent(target.id)}`,
          "DELETE",
          { expected_revision: target.revision },
        );
        if (target.kind === "source") {
          updateDrafts((current) => ({
            ...current,
            memory: current.memory?.draft.evidence.some(
              (item) => item.source_id === target.id,
            )
              ? null
              : current.memory,
          }));
          setNotice("원문과 그 원문에 의존하는 기억을 삭제했습니다.");
        } else {
          updateDrafts((current) => ({
            ...current,
            memory: current.memory?.id === target.id ? null : current.memory,
          }));
          setNotice("기억과 수정 이력을 지웠습니다. 원문은 유지됩니다.");
        }
      }
      setSource(null);
      setMemory(null);
      setDanger(null);
      refreshLists();
    } catch (error) {
      fail(error);
    } finally {
      mutationLock.current = false;
      setBusy(false);
    }
  }
  const next = mode === "source" ? sources?.next_after : memories?.next_after;
  function openHit(hit: KnowledgeHit) {
    if (hit.kind === "source")
      setSource({ id: hit.id, revision: hit.revision });
    else setMemory({ id: hit.id, revision: hit.revision });
  }
  return (
    <main className="admin-scroll knowledge-view">
      <div className="admin-intro">
        <p className="eyebrow">내 계정의 자료와 기억</p>
        <h2>{mode === "source" ? "자료" : "기억"}</h2>
        <p>
          {mode === "source"
            ? "텍스트 자료를 모으고, 필요한 원문 구간을 기억의 근거로 연결하세요."
            : "직접 기록하거나 자료를 근거로 만든 기억을 읽고 고칠 수 있습니다."}
        </p>
      </div>
      {!drafts.noticeHidden && (
        <div className="knowledge-stage">
          <p>
            도구 사용이 확인된 모델에서는 채팅 중 필요한 자료와 기억을 찾아볼 수
            있습니다. “기억해 줘”, “수정해 줘”, “잊어 줘”라고 요청해도 됩니다.
            요청하지 않은 내용을 자동으로 기억하지는 않습니다.
          </p>
          <button
            className="text-button"
            onClick={() =>
              updateDrafts((current) => ({ ...current, noticeHidden: true }))
            }
          >
            알겠습니다
          </button>
        </div>
      )}
      {error && !danger && (
        <p className="notice knowledge-alert" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="knowledge-success" role="status">
          {notice}
        </p>
      )}
      <div className="knowledge-toolbar">
        {mode === "source" ? (
          <>
            <label>
              자료 모음
              <select
                value={collectionId ?? ""}
                disabled={busy || !!drafts.paste?.open}
                onChange={(event) => {
                  setCollectionId(event.target.value || null);
                  setPageStack([null]);
                  setSource(null);
                  clearSearch();
                }}
              >
                <option value="">모음 선택</option>
                {collections
                  .filter((collection) => !collection.deleted)
                  .map((collection) => (
                    <option value={collection.id} key={collection.id}>
                      {collection.name}
                      {collection.source_connection_id
                        ? " · 수집 연결"
                        : collection.connected
                          ? ""
                          : " · 보관됨"}
                    </option>
                  ))}
              </select>
            </label>
            <button
              className="secondary"
              disabled={busy}
              onClick={() => {
                const token = drafts.collection?.scope ?? crypto.randomUUID();
                updateDrafts((current) => ({
                  ...current,
                  collection: current.collection
                    ? { ...current.collection, open: true }
                    : {
                        open: true,
                        name: "",
                        scope: token,
                        uncertain: false,
                        conflict: false,
                      },
                }));
              }}
            >
              모음 만들기
            </button>
            <button
              className="primary"
              disabled={
                busy ||
                !!currentCollection?.source_connection_id ||
                (!drafts.paste && !currentCollection?.connected)
              }
              onClick={() => {
                if (drafts.paste) {
                  setCollectionId(drafts.paste.collection.id);
                  updateDrafts((current) => ({
                    ...current,
                    paste: current.paste
                      ? { ...current.paste, open: true }
                      : null,
                  }));
                } else if (
                  currentCollection &&
                  !currentCollection.source_connection_id
                )
                  updateDrafts((current) => ({
                    ...current,
                    paste: startPaste(currentCollection),
                  }));
              }}
            >
              {drafts.paste ? "자료 작성 이어가기" : "자료 붙여넣기"}
            </button>
          </>
        ) : (
          <button className="primary" onClick={() => startAssertion()}>
            {drafts.memory ? "기억 작성 이어가기" : "기억 추가"}
          </button>
        )}
        <button
          className="text-button"
          disabled={busy}
          onClick={() => {
            refreshLists();
            setSource(null);
            setMemory(null);
          }}
        >
          목록 새로고침
        </button>
      </div>
      {mode === "source" && currentCollection && (
        <div className="collection-context">
          <MemoryExtraction accountId={accountId} collectionId={currentCollection.id} connected={currentCollection.connected} onExpired={onExpired}/>

          {!currentCollection.connected && (
            <p>자료 추가가 중단된 모음입니다.</p>
          )}
          {currentCollection.source_connection_id ? (
            <p>
              수집 연결에서 관리하는 자료 모음입니다.{" "}
              <a
                href={`/settings/sources?connection=${encodeURIComponent(currentCollection.source_connection_id)}`}
              >
                수집 설정과 연결 해제
              </a>
            </p>
          ) : (
            <button
              className={currentCollection.connected ? "text-button" : "danger"}
              disabled={busy}
              onClick={() => {
                setError("");
                setDanger({
                  kind: "collection",
                  collection: currentCollection,
                });
              }}
            >
              {currentCollection.connected
                ? "모음 연결 해제"
                : "보관된 모음 삭제"}
            </button>
          )}
        </div>
      )}
      {mode === "source" && drafts.collection?.open && (
        <section className="admin-card knowledge-editor">
          <div className="section-heading">
            <h3>새 자료 모음</h3>
            <button
              className="text-button"
              disabled={busy}
              onClick={() =>
                updateDrafts((current) => ({
                  ...current,
                  collection: current.collection
                    ? { ...current.collection, open: false }
                    : null,
                }))
              }
            >
              접기
            </button>
          </div>
          {drafts.collection.uncertain && (
            <p className="notice">
              {drafts.collection.conflict
                ? "작성하던 모음이 다른 곳에서 변경되거나 삭제되어 이어갈 수 없습니다. 이 생성 초안을 버린 뒤 새 모음을 만들 수 있습니다. 초안을 버려도 서버의 모음과 자료는 변경되지 않습니다."
                : "응답이 유실되었을 수 있습니다. 생성 결과를 확인해 작성하던 모음을 이어갈 수 있습니다."}
            </p>
          )}
          <form onSubmit={createCollection}>
            <label>
              모음 이름
              <input
                required
                value={drafts.collection.name}
                disabled={busy || drafts.collection.uncertain}
                onChange={(event) =>
                  updateDrafts((current) => ({
                    ...current,
                    collection: current.collection
                      ? { ...current.collection, name: event.target.value }
                      : null,
                  }))
                }
              />
            </label>
            <button className="primary" disabled={busy}>
              {busy
                ? "만드는 중…"
                : drafts.collection.uncertain
                  ? "모음 생성 결과 확인"
                  : "모음 저장"}
            </button>
          </form>
          {drafts.collection.conflict && (
            <button
              className="secondary"
              disabled={busy}
              onClick={() => {
                updateDrafts((current) => ({ ...current, collection: null }));
                setError("");
                setNotice(
                  "생성 초안을 버렸습니다. 모음 만들기로 새로 시작할 수 있습니다.",
                );
              }}
            >
              생성 초안 버리기
            </button>
          )}
        </section>
      )}
      {mode === "source" && drafts.paste?.open && (
        <PasteForm
          key={drafts.paste.collection.id}
          accountId={accountId}
          state={drafts.paste}
          update={updateDrafts}
          refreshCollection={refreshCollection}
          onExpired={onExpired}
          saved={() => {
            setNotice("자료를 저장했습니다.");
            refreshLists();
          }}
        />
      )}
      {drafts.memory?.open && (
        <MemoryForm
          key={drafts.memory.id ?? "new"}
          accountId={accountId}
          state={drafts.memory}
          update={updateDrafts}
          onExpired={onExpired}
          saved={(value) => {
            setMemory({ id: value.id, revision: value.revision });
            setNotice("기억을 저장했습니다.");
            refreshLists();
          }}
          openEvidence={(id, revision) => setSource({ id, revision })}
        />
      )}
      <form
        className="knowledge-search admin-card"
        onSubmit={(event: FormEvent) => {
          event.preventDefault();
          void runSearch();
        }}
      >
        <div className="knowledge-search-main">
          <label>
            {mode === "source" ? "자료 검색" : "기억 검색"}
            <input
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                clearSearch();
              }}
              placeholder="찾을 내용을 입력하세요"
            />
          </label>
          <button className="secondary" disabled={searching || !query.trim()}>
            {searching ? "검색 중…" : "검색"}
          </button>
          {search && (
            <button
              type="button"
              className="text-button"
              onClick={() => {
                clearSearch();
                setQuery("");
              }}
            >
              검색 닫기
            </button>
          )}
        </div>
        <details>
          <summary>검색 조건</summary>
          {mode === "memory" && (
            <label>
              주제 일치
              <input
                value={subject}
                onChange={(event) => {
                  setSubject(event.target.value);
                  clearSearch();
                }}
              />
            </label>
          )}
          <label>
            화자 일치
            <input
              value={speaker}
              onChange={(event) => {
                setSpeaker(event.target.value);
                clearSearch();
              }}
            />
          </label>
          <label>
            기준 시각 (선택)
            <input
              type="datetime-local"
              step="1"
              value={localDate(validAt)}
              onChange={(event) => {
                setValidAt(dateInput(event.target.value));
                clearSearch();
              }}
            />
          </label>
        </details>
      </form>
      {search ? (
        <section className="admin-card knowledge-list" aria-label="검색 결과">
          <div className="section-heading">
            <h3>검색 결과</h3>
          </div>
          {search.hits.length ? (
            search.hits.map((hit) => (
              <button
                className="knowledge-row"
                key={`${hit.kind}:${hit.id}:${hit.revision}`}
                onClick={() => openHit(hit)}
              >
                <strong>{hit.title}</strong>
                <span>{hit.snippet}</span>
              </button>
            ))
          ) : (
            <p className="knowledge-empty">일치하는 항목이 없습니다.</p>
          )}
          {search.truncated && (
            <p className="fine">
              일부 자료만 확인한 결과입니다. 더 구체적인 검색어로 다시 확인해
              주세요.
            </p>
          )}
          {search.next_cursor && (
            <button
              className="text-button"
              disabled={searching}
              onClick={() => runSearch(search.next_cursor)}
            >
              다음 검색 결과
            </button>
          )}
        </section>
      ) : (
        <section
          className="admin-card knowledge-list"
          aria-label={mode === "source" ? "자료 목록" : "기억 목록"}
        >
          {loading ? (
            <p role="status">목록을 불러오는 중…</p>
          ) : mode === "source" ? (
            sources?.items.length ? (
              sources.items.map((item) => (
                <button
                  className="knowledge-row"
                  key={item.id}
                  onClick={() =>
                    setSource({ id: item.id, revision: item.revision })
                  }
                >
                  <strong>{item.title}</strong>
                  <span>
                    {item.speaker ??
                      (currentCollection?.source_connection_id
                        ? "연결에서 수집한 자료"
                        : "직접 붙여넣은 자료")}
                  </span>
                </button>
              ))
            ) : (
              <p className="knowledge-empty">
                {currentCollection?.source_connection_id
                  ? "아직 수집된 자료가 없습니다. 수집 연결의 실행 이력을 확인하세요."
                  : "저장된 자료가 없습니다. 모음을 선택하고 텍스트를 붙여넣어 주세요."}
              </p>
            )
          ) : memories?.items.length ? (
            memories.items.map((item) => (
              <button
                className="knowledge-row"
                key={item.id}
                onClick={() =>
                  setMemory({ id: item.id, revision: item.revision })
                }
              >
                <strong>
                  {item.draft.subject}
                  {item.status === "invalidated" && (
                    <small>근거 확인 필요</small>
                  )}
                </strong>
                <span>{item.draft.text}</span>
              </button>
            ))
          ) : (
            <p className="knowledge-empty">아직 기록한 기억이 없습니다.</p>
          )}
          <div className="knowledge-pagination">
            {pageStack.length > 1 && (
              <button
                className="text-button"
                disabled={loading}
                onClick={() => setPageStack((current) => current.slice(0, -1))}
              >
                이전 목록
              </button>
            )}
            {next && (
              <button
                className="text-button"
                disabled={loading}
                onClick={() => setPageStack((current) => [...current, next])}
              >
                다음 목록
              </button>
            )}
          </div>
        </section>
      )}
      {source && (
        <SourceDetail
          key={`${source.id}:${source.revision}`}
          accountId={accountId}
          resource={source}
          onExpired={onExpired}
          close={() => setSource(null)}
          evidence={addEvidence}
          destroy={(document) => {
            setError("");
            setDanger({
              kind: "source",
              id: document.source.id,
              revision: document.source.revision,
              label: document.source.title,
            });
          }}
        />
      )}
      {memory && mode === "memory" && (
        <MemoryDetail
          key={`${memory.id}:${memory.revision}`}
          accountId={accountId}
          resource={memory}
          onExpired={onExpired}
          close={() => setMemory(null)}
          edit={startAssertion}
          evidence={(id, revision) => setSource({ id, revision })}
          forget={(document) => {
            setError("");
            setDanger({
              kind: "memory",
              id: document.id,
              revision: document.revision,
              label: document.draft.subject,
            });
          }}
        />
      )}
      {danger && (
        <KnowledgeConfirm
          danger={danger}
          busy={busy}
          error={error}
          close={() => {
            if (!busy) setDanger(null);
          }}
          confirm={destroy}
        />
      )}
    </main>
  );
}

function SourceDetail({
  accountId,
  resource,
  onExpired,
  close,
  evidence,
  destroy,
}: {
  accountId: string;
  resource: Resource;
  onExpired: () => void;
  close: () => void;
  evidence: (evidence: EvidenceRef, title: string) => void;
  destroy: (source: SourceDocument) => void;
}) {
  const scope = useScope(
    accountId,
    `source:${resource.id}:${resource.revision}`,
  );
  const [document, setDocument] = useState<SourceDocument | null>(null);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<EvidenceRef | null>(null);
  const [copied, setCopied] = useState(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    void scope
      .request<SourceDocument>(
        `/knowledge/sources/${encodeURIComponent(resource.id)}?revision=${resource.revision}`,
      )
      .then(setDocument)
      .catch((error) => {
        if (!isAborted(error)) {
          if (error instanceof ApiFailure && error.status === 401) onExpired();
          else setError(knowledgeError(error));
        }
      });
    return () => {
      alive.current = false;
    };
  }, [scope, resource.id, resource.revision]);
  return (
    <section className="admin-card knowledge-detail" aria-label="원문 읽기">
      <div className="section-heading">
        <div>
          <h3>{document?.source.title ?? "원문"}</h3>
          {document && (
            <p>
              {document.source.speaker ?? "화자 미지정"} ·{" "}
              {new Date(document.source.observed_at * 1000).toLocaleString()}
            </p>
          )}
        </div>
        <button className="text-button" onClick={close}>
          원문 닫기
        </button>
      </div>
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      {document ? (
        <>
          {document.provenance && (
            <SourceProvenance
              accountId={accountId}
              provenance={document.provenance}
              onExpired={onExpired}
            />
          )}
          <MemoryExtraction accountId={accountId} collectionId={document.source.collection_id} connected={true} onExpired={onExpired} source={{id:document.source.id,revision:document.source.revision}}/>
          <label>
            원문 텍스트
            <textarea
              className="source-text"
              rows={10}
              value={document.text}
              readOnly
              onSelect={(event) => {
                setSelected(
                  selectedEvidence(
                    document,
                    event.currentTarget.selectionStart,
                    event.currentTarget.selectionEnd,
                  ),
                );
                setCopied(false);
              }}
            />
          </label>
          <p className="fine">
            기억의 근거로 쓸 구간을 원문에서 4KiB 이내로 선택하세요. 선택한
            위치와 텍스트를 그대로 연결합니다.
          </p>
          {selected && (
            <div className="source-selection">
              <blockquote>{selected.quote}</blockquote>
              <div className="knowledge-actions">
                <button
                  className="secondary"
                  onClick={() => evidence(selected, document.source.title)}
                >
                  이 구간을 기억 근거로
                </button>
                <button
                  className="text-button"
                  onClick={async () => {
                    const selection = selected;
                    try {
                      await navigator.clipboard.writeText(selection.quote);
                      if (alive.current) setCopied(true);
                    } catch {
                      if (alive.current)
                        setError(
                          "클립보드에 복사하지 못했습니다. 원문 선택 영역을 직접 복사해 주세요.",
                        );
                    }
                  }}
                >
                  {copied ? "선택 구간 복사됨" : "선택 구간 복사"}
                </button>
              </div>
            </div>
          )}
          <button
            className="text-button destructive-text"
            onClick={() => destroy(document)}
          >
            원문 삭제
          </button>
        </>
      ) : (
        !error && <p role="status">원문을 불러오는 중…</p>
      )}
    </section>
  );
}

function MemoryDetail({
  accountId,
  resource,
  onExpired,
  close,
  edit,
  forget,
  evidence,
}: {
  accountId: string;
  resource: Resource;
  onExpired: () => void;
  close: () => void;
  edit: (document: MemoryDocument) => void;
  forget: (document: MemoryDocument) => void;
  evidence: (id: string, revision: number) => void;
}) {
  const scope = useScope(
    accountId,
    `memory:${resource.id}:${resource.revision}`,
  );
  const [document, setDocument] = useState<MemoryDocument | null>(null);
  const [error, setError] = useState("");
  const [history, setHistory] = useState<MemoryHistory | null>(null);
  const [loading, setLoading] = useState(false);
  const lock = useRef(false);
  useEffect(() => {
    void scope
      .request<MemoryDocument>(
        `/knowledge/memories/${encodeURIComponent(resource.id)}?revision=${resource.revision}`,
      )
      .then(setDocument)
      .catch((error) => {
        if (!isAborted(error)) {
          if (error instanceof ApiFailure && error.status === 401) onExpired();
          else setError(knowledgeError(error));
        }
      });
  }, [scope, resource.id, resource.revision]);
  async function loadHistory(before?: number) {
    if (lock.current) return;
    lock.current = true;
    setLoading(true);
    try {
      const value = await scope.request<MemoryHistory>(
        `/knowledge/memories/${encodeURIComponent(resource.id)}/history${before ? `?before=${before}` : ""}`,
      );
      setHistory((current) =>
        before && current
          ? { ...value, items: [...current.items, ...value.items] }
          : value,
      );
    } catch (error) {
      if (!isAborted(error)) {
        if (error instanceof ApiFailure && error.status === 401) onExpired();
        else setError(knowledgeError(error));
      }
    } finally {
      lock.current = false;
      setLoading(false);
    }
  }
  return (
    <section className="admin-card knowledge-detail" aria-label="기억 읽기">
      <div className="section-heading">
        <h3>{document?.draft.subject ?? "기억"}</h3>
        <button className="text-button" onClick={close}>
          기억 닫기
        </button>
      </div>
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      {document ? (
        <>
          {document.status === "invalidated" && (
            <p className="knowledge-pending">
              원문이 변경되어 근거를 다시 확인해야 하는 기억입니다.
            </p>
          )}
          {document.extraction_origin&&<p className="fine">자료에서 추출한 사실 · 표시된 작성자: {document.extraction_origin.reported_speaker}. 확인된 신원이라는 뜻은 아닙니다.</p>}
          <p className="knowledge-body">{document.draft.text}</p>
          <p className="fine">
            {document.draft.evidence.length
              ? "원문 근거가 연결된 기억"
              : "내가 직접 기록한 기억"}
            {document.draft.speaker ? ` · ${document.draft.speaker}` : ""}
          </p>
          {document.draft.evidence.map((item) => (
            <div
              className="evidence-card"
              key={`${item.source_id}:${item.start_byte}:${item.end_byte}`}
            >
              <blockquote>{item.quote}</blockquote>
              <button
                className="text-button"
                onClick={() => evidence(item.source_id, item.source_revision)}
              >
                근거 원문 보기
              </button>
            </div>
          ))}
          <div className="knowledge-actions">
            <button className="secondary" onClick={() => edit(document)}>
              기억 수정
            </button>
            <button
              className="text-button"
              disabled={loading}
              onClick={() => loadHistory()}
            >
              수정 이력
            </button>
            <button
              className="text-button destructive-text"
              onClick={() => forget(document)}
            >
              기억 잊기
            </button>
          </div>
        </>
      ) : (
        !error && <p role="status">기억을 불러오는 중…</p>
      )}
      {history && (
        <div className="memory-history" aria-label="기억 수정 이력">
          <h4>수정 이력</h4>
          {history.items.map((item) => (
            <article key={item.revision}>
              <p className="fine">
                저장 버전 {item.revision}
                {item.status === "invalidated" ? " · 근거 확인 필요" : ""}
              </p>
              <strong>{item.draft.subject}</strong>
              <p className="knowledge-body">{item.draft.text}</p>
            </article>
          ))}
          {history.next_before && (
            <button
              className="text-button"
              disabled={loading}
              onClick={() => loadHistory(history.next_before!)}
            >
              이전 이력 보기
            </button>
          )}
        </div>
      )}
    </section>
  );
}

function KnowledgeConfirm({
  danger,
  busy,
  error,
  close,
  confirm,
}: {
  danger: Danger;
  busy: boolean;
  error: string;
  close: () => void;
  confirm: (mode: "keep" | "delete") => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const title = useId();
  const retained = danger.kind === "collection" && !danger.collection.connected;
  const [mode, setMode] = useState<"keep" | "delete">(
    retained ? "delete" : "keep",
  );
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    ref.current!.showModal();
    return () => {
      ref.current?.close();
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  return (
    <dialog
      ref={ref}
      aria-labelledby={title}
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) close();
      }}
    >
      <div className="dialog-top">
        <h2 id={title}>
          {danger.kind === "collection"
            ? retained
              ? "보관된 모음 삭제"
              : "모음 연결 해제"
            : danger.kind === "source"
              ? "원문 삭제"
              : "기억 잊기"}
        </h2>
      </div>
      <p>
        <strong>
          {danger.kind === "collection" ? danger.collection.name : danger.label}
        </strong>
      </p>
      {danger.kind === "collection" && retained ? (
        <p>
          보관된 모음의 원문과 그 원문에 의존하는 기억·수정 이력을 삭제합니다.
          삭제한 원문을 근거로 한 작성 중 기억도 지워집니다. 독립적으로 기록한
          기억은 유지됩니다.
        </p>
      ) : danger.kind === "collection" ? (
        <fieldset disabled={busy} className="disconnect-options">
          <legend>해제할 때 보관할 내용</legend>
          <label>
            <input
              type="radio"
              name="disconnect-mode"
              value="keep"
              checked={mode === "keep"}
              onChange={() => setMode("keep")}
            />
            <span>
              <strong>자료와 기억 보관</strong>
              <small>추가만 중단하고 기존 원문과 기억을 유지합니다.</small>
            </span>
          </label>
          <label>
            <input
              type="radio"
              name="disconnect-mode"
              value="delete"
              checked={mode === "delete"}
              onChange={() => setMode("delete")}
            />
            <span>
              <strong>원문과 연관 기억 삭제</strong>
              <small>
                모음의 원문과 그 원문에 의존하는 기억·이력을 함께 삭제합니다.
                삭제한 원문을 근거로 한 작성 중 기억도 지워집니다. 독립적으로
                기록한 기억은 유지됩니다.
              </small>
            </span>
          </label>
        </fieldset>
      ) : (
        <p>
          {danger.kind === "source"
            ? "이 원문과 그 원문에 의존하는 기억·수정 이력을 삭제합니다. 이 원문을 근거로 한 작성 중 기억도 지워집니다. 독립적으로 기록한 기억은 유지됩니다."
            : "이 기억과 수정 이력을 지웁니다. 원문 자료는 삭제하지 않습니다."}
        </p>
      )}
      {(danger.kind !== "collection" || retained || mode === "delete") && (
        <p>
          이 자료나 기억을 참고한 채팅 답변도 지워집니다. 내가 보낸 질문은
          유지됩니다.
        </p>
      )}
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      <div className="dialog-actions">
        <button className="secondary" disabled={busy} onClick={close}>
          취소
        </button>
        <button
          className={
            danger.kind === "collection" && mode === "keep"
              ? "primary"
              : "danger"
          }
          disabled={busy}
          onClick={() => confirm(mode)}
        >
          {busy
            ? "처리 중…"
            : danger.kind === "collection"
              ? retained
                ? "모음과 연관 기억 삭제"
                : mode === "keep"
                  ? "자료 보관 후 해제"
                  : "자료 삭제 후 해제"
              : danger.kind === "source"
                ? "원문과 연관 기억 삭제"
                : "기억과 이력 지우기"}
        </button>
      </div>
    </dialog>
  );
}
