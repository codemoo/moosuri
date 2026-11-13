import { useEffect, useRef, useState } from "react";
import type { MediaSummary, MediaLibrary } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { PreparationError, prepareImage } from "./media-prepare";
import "./media.css";
import {PrivateFile} from "./PrivateFiles";
export function PrivateImage(props: {accountId:string;media:MediaSummary}) {
  return props.media.filename ? <PrivateFile {...props}/> : <ImagePreview {...props}/>;
}
function ImagePreview({
  accountId,
  media,
}: {
  accountId: string;
  media: MediaSummary;
}) {
  const scope = useScope(accountId, media.id);
  const [url, setUrl] = useState<string>();
  const [full, setFull] = useState<string>();
  const fullRef = useRef<string | undefined>(undefined);
  const generation = useRef(0);
  function closeFull() {
    if (fullRef.current) scope.releaseUrl(fullRef.current);
    fullRef.current = undefined;
    setFull(undefined);
  }
  async function openFull() {
    const before = generation.current;
    try {
      const result = await scope.imageUrl(
        `/media/${encodeURIComponent(media.id)}/content`,
      );
      if (before !== generation.current) {
        scope.releaseUrl(result);
        return;
      }
      if (fullRef.current) scope.releaseUrl(fullRef.current);
      fullRef.current = result;
      setFull(result);
    } catch {}
  }

  useEffect(() => {
    if (media.state !== "ready") {
      closeFull();
      setUrl(undefined);
      return;
    }
    const controller = new AbortController();
    let owned: string | undefined;
    let alive = true;
    void scope
      .imageUrl(
        `/media/${encodeURIComponent(media.id)}/thumbnail`,
        controller.signal,
      )
      .then((value) => {
        if (alive) {
          owned = value;
          setUrl(value);
        } else scope.releaseUrl(value);
      })
      .catch(() => {});
    const deleted = (e: Event) => {
      if ((e as CustomEvent<string>).detail === media.id) {
        alive = false;
        generation.current++;
        controller.abort();
        if (owned) scope.releaseUrl(owned);
        closeFull();
        setUrl(undefined);
      }
    };
    window.addEventListener("moosuri-media-deleted", deleted);
    return () => {
      alive = false;
      generation.current++;
      controller.abort();
      if (owned) scope.releaseUrl(owned);
      if (fullRef.current) scope.releaseUrl(fullRef.current);
      window.removeEventListener("moosuri-media-deleted", deleted);
    };
  }, [scope, media.id, media.revision, media.state]);
  return (
    <>
      {url ? (
        <button
          type="button"
          className="media-preview-button"
          aria-label="첨부 이미지 확대"
          onClick={() => void openFull()}
        >
          <img className="media-thumbnail" src={url} alt="첨부 이미지" />
        </button>
      ) : (
        <span className="media-placeholder">
          {media.state === "deleted"
            ? "삭제된 이미지"
            : "이미지 미리보기 준비 중"}
        </span>
      )}
      {full && (
        <div
          className="media-lightbox"
          role="dialog"
          aria-label="첨부 이미지 확대"
          aria-modal="true"
          onKeyDown={(e) => {
            if (e.key === "Escape") closeFull();
          }}
        >
          <button type="button" autoFocus onClick={closeFull}>
            닫기
          </button>
          <img src={full} alt="확대한 첨부 이미지" />
        </div>
      )}
    </>
  );
}
type Upload = {
  id: string;
  selection: string;
  slot: string;
  status: "preparing" | "uploading" | "ready" | "failed";
  progress: number;
  blob?: Blob;
  filename?: string;
  url?: string;
  summary?: MediaSummary;
  error?: string;
  controller: AbortController;
};
export function useMediaUploads(accountId: string, slot: string) {
  const scope = useScope(accountId);
  const [items, setItems] = useState<Upload[]>([]);
  const current = useRef(items);
  current.current = items;
  const alive = useRef(true);
  const serial = useRef(Promise.resolve());
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      for (const item of current.current) item.controller.abort();
    };
  }, []);
  function update(selection: string, patch: Partial<Upload>) {
    if (alive.current)
      setItems((all) =>
        all.map((item) =>
          item.selection === selection ? { ...item, ...patch } : item,
        ),
      );
  }
  async function upload(item: Upload) {
    if (!item.blob) return;
    update(item.selection, { status: "uploading", error: undefined });
    try {
      const summary = await scope.upload<MediaSummary>(
        item.id,
        item.blob,
        (value) => update(item.selection, { progress: value }),
        item.controller.signal,
        item.filename,
      );
      if (item.controller.signal.aborted) return;
      update(item.selection, { status: "ready", summary, progress: 100 });
    } catch (e) {
      if (!isAborted(e))
        update(item.selection, {
          status: "failed",
          error:
            e instanceof ApiFailure && e.status === 503
              ? "업로드가 처리 중이거나 보관함 한도에 도달했습니다. 보관함 사용량을 확인하고 잠시 뒤 같은 파일로 다시 시도하세요."
              : e instanceof ApiFailure && e.status === 409
                ? "이 업로드는 취소·삭제되었거나 내용이 다릅니다. 파일을 새로 선택하세요."
                : e instanceof ApiFailure && e.status === 400
                  ? (item.filename ? "파일은 24KiB 이하의 유효한 UTF-8 TXT/Markdown/CSV/JSON이어야 합니다. PDF·음성·Office·바이너리는 지원하지 않습니다." : "이미지를 확인하지 못했습니다. PNG/JPEG/WebP와 준비된 2MiB·2400px 한도를 확인한 뒤 작은 파일을 다시 선택하세요.")
                  : "업로드를 확인하지 못했습니다. 같은 파일로 다시 시도하세요.",
        });
    }
  }
  function add(files: FileList | null, document = false) {
    if (!files) return;
    for (const file of Array.from(files).slice(
      0,
      4 - current.current.filter((v) => v.slot === slot).length,
    )) {
      const item: Upload = {
        id: crypto.randomUUID(),
        selection: crypto.randomUUID(),
        slot,
        status: "preparing",
        progress: 0,
        controller: new AbortController(),
        ...(document ? {filename:file.name}:{}),
      };
      setItems((all) => [...all, item]);
      const prepare = async () => {
        try {
          if (document && (!/\.(txt|md|markdown|csv|json)$/i.test(file.name) || file.size === 0 || file.size > 24576)) throw new PreparationError("파일은 24KiB 이하의 TXT/Markdown/CSV/JSON만 지원합니다. PDF·음성·Office는 아직 지원하지 않습니다.");
          const blob = document ? file : await prepareImage(file);
          if (!alive.current || item.controller.signal.aborted) return;
          item.blob = blob;
          if (!document) item.url = scope.localUrl(blob);
          update(item.selection, { blob, url: item.url });
          await upload(item);
        } catch (e) {
          if (!item.controller.signal.aborted)
            update(item.selection, {
              status: "failed",
              error:
                e instanceof PreparationError
                  ? e.message
                  : "이미지를 준비하지 못했습니다. 지원하는 작은 이미지를 다시 선택하세요.",
            });
        }
      };
      if (document) void prepare();
      else serial.current = serial.current.then(prepare);
    }
  }
  function remove(selection: string) {
    const item = current.current.find((v) => v.selection === selection);
    if (!item) return;
    item.controller.abort();
    if (item.url) scope.releaseUrl(item.url);
    setItems((all) => all.filter((v) => v.selection !== selection));
    if (item.status !== "ready")
      void scope
        .request(`/media/${item.id}`, "DELETE")
        .then(() =>
          window.dispatchEvent(
            new CustomEvent("moosuri-media-deleted", { detail: item.id }),
          ),
        )
        .catch(() => {});
  }
  function sent(selections: string[] | undefined) {
    if (!selections?.length) return;
    const captured = new Set(selections ?? []);
    for (const item of current.current)
      if (captured.has(item.selection) && item.url) scope.releaseUrl(item.url);
    setItems((all) =>
      all.some((item) => captured.has(item.selection))
        ? all.filter((item) => !captured.has(item.selection))
        : all,
    );
  }
  function retry(selection: string) {
    const item = current.current.find((v) => v.selection === selection);
    if (item?.blob) {
      item.controller = new AbortController();
      void upload(item);
    }
  }
  function select(media: MediaSummary) {
    if (
      current.current.filter((v) => v.slot === slot).length >= 4 ||
      current.current.some((v) => v.slot === slot && v.id === media.id)
    )
      return;
    setItems((all) => [
      ...all,
      {
        id: media.id,
        selection: crypto.randomUUID(),
        slot,
        status: "ready",
        progress: 100,
        summary: media,
        controller: new AbortController(),
      },
    ]);
  }
  useEffect(() => {
    const deleted = (event: Event) => {
      const id = (event as CustomEvent<string>).detail;
      for (const item of current.current.filter((v) => v.id === id)) {
        item.controller.abort();
        if (item.url) scope.releaseUrl(item.url);
      }
      setItems((all) => all.filter((v) => v.id !== id));
    };
    window.addEventListener("moosuri-media-deleted", deleted);
    return () => window.removeEventListener("moosuri-media-deleted", deleted);
  }, [scope]);
  function moveSlot(from: string, to: string) {
    setItems((all) =>
      all.map((item) => (item.slot === from ? { ...item, slot: to } : item)),
    );
  }
  const visible = items.filter((v) => v.slot === slot);
  const ready = visible
    .filter((v) => v.status === "ready")
    .map((v) => ({ id: v.id, revision: v.summary!.revision }));
  const readySelections = visible
    .filter((v) => v.status === "ready")
    .map((v) => v.selection);
  return {
    readySelections,
    ready,
    sent,
    select,
    moveSlot,
    view: (
      <div className="media-composer">
        <div className="media-attach-actions">
        <label className="media-file-button">
          <span aria-hidden="true">＋</span>
          이미지 첨부
          <input
            aria-label="이미지 첨부"
            type="file"
            accept="image/png,image/jpeg,image/webp"
            multiple
            onChange={(e) => {
              add(e.target.files);
              e.target.value = "";
            }}
            disabled={visible.length >= 4}
          />
        </label>
        <label className="media-file-button">
          <span aria-hidden="true">＋</span>
          파일 첨부
          <input aria-label="파일 첨부" type="file" accept=".txt,.md,.markdown,.csv,.json" multiple disabled={visible.length>=4} onChange={e=>{add(e.target.files,true);e.target.value="";}}/>
        </label>
        </div>
        <span className="media-limit">
          이미지 · TXT · Markdown · CSV · JSON
        </span>
        <div className="media-upload-list">
          {visible.map((item) => (
            <div className="media-upload" key={item.selection}>
              {item.url ? (
                <img
                  className="media-thumbnail"
                  src={item.url}
                  alt="선택한 이미지"
                />
              ) : item.summary ? (
                <PrivateImage accountId={accountId} media={item.summary} />
              ) : null}
              {item.filename && !item.summary && <span>{item.filename}</span>}
              <span>
                {item.status === "preparing"
                  ? (item.filename ? "파일 확인 중" : "이미지 준비 중")
                  : item.status === "uploading"
                    ? `업로드 ${item.progress}%`
                    : item.status === "ready"
                      ? "첨부 준비됨"
                      : "준비 실패"}
              </span>
              {item.status === "uploading" && (
                <progress max={100} value={item.progress} />
              )}{" "}
              {item.error && <p role="status">{item.error}</p>}
              {item.status === "failed" && item.blob && (
                <button type="button" onClick={() => retry(item.selection)}>
                  같은 파일 재시도
                </button>
              )}
              <button type="button" onClick={() => remove(item.selection)}>
                {item.status === "ready" ? "선택 해제" : "취소"}
              </button>
            </div>
          ))}
        </div>
        {visible.some((v) => v.status !== "ready") && (
          <p className="media-limit">
            지금 메시지를 보내면 준비된 첨부만 전송됩니다. 진행 중인 파일은
            여기 남습니다.
          </p>
        )}
      </div>
    ),
  };
}
export function MediaLibraryView({
  accountId,
  onSelect,
}: {
  accountId: string;
  onSelect: (media: MediaSummary) => void;
}) {
  const scope = useScope(accountId);
  const [data, setData] = useState<MediaLibrary>();
  const [error, setError] = useState("");
  const deletedIds = useRef(new Set<string>());
  const retries = useRef(new Map<string, AbortController>());
  const [retryProgress, setRetryProgress] = useState<Record<string, number>>({});
  const generation = useRef(0);
  const downloads = useRef(
    new Map<AbortController, { id: string; url?: string; timer?: ReturnType<typeof setTimeout> }>(),
  );
  useEffect(() => {
    deletedIds.current.clear();
    const stop = (controller: AbortController) => {
      const pending = downloads.current.get(controller);
      controller.abort();
      if (pending?.timer !== undefined) clearTimeout(pending.timer);
      if (pending?.url) scope.releaseUrl(pending.url);
      downloads.current.delete(controller);
    };
    const deleted = (event: Event) => {
      const id = (event as CustomEvent<string>).detail;
      deletedIds.current.add(id);
      retries.current.get(id)?.abort();
      retries.current.delete(id);
      for (const [controller, pending] of downloads.current) {
        if (pending.id === id) stop(controller);
      }
      setData((current) => current && ({ ...current, items: current.items.filter((item) => item.id !== id) }));
    };
    window.addEventListener("moosuri-media-deleted", deleted);
    return () => {
      generation.current++;
      window.removeEventListener("moosuri-media-deleted", deleted);
      for (const controller of downloads.current.keys()) stop(controller);
      for (const controller of retries.current.values()) controller.abort();
      retries.current.clear();
    };
  }, [scope]);
  async function refresh() {
    try {
      const result = await scope.request<MediaLibrary>("/media");
      setData({ ...result, items: result.items.filter((item) => !deletedIds.current.has(item.id)) });
      setError("");
    } catch (e) {
      if (!isAborted(e)) setError("보관함을 불러오지 못했습니다.");
    }
  }
  async function remove(id: string) {
    try {
      await scope.request(`/media/${id}`, "DELETE");
      window.dispatchEvent(
        new CustomEvent("moosuri-media-deleted", { detail: id }),
      );
      await refresh();
    } catch (e) {
      if (!isAborted(e))
        setError("삭제를 확인하지 못했습니다. 다시 확인하세요.");
    }
  }
  async function resume(media: MediaSummary, original: File | undefined) {
    if (!original || retries.current.has(media.id) || deletedIds.current.has(media.id)) return;
    if (!/\.(txt|md|markdown|csv|json)$/i.test(original.name) || original.size === 0 || original.size > 24576) {
      setError("24KiB 이하의 원래 UTF-8 TXT/Markdown/CSV/JSON 파일을 선택하세요.");
      return;
    }
    const before = generation.current;
    const controller = new AbortController();
    retries.current.set(media.id, controller);
    setRetryProgress(value => ({...value, [media.id]: 0}));
    try {
      await scope.upload<MediaSummary>(media.id, original, percent => {
        if (before === generation.current && !controller.signal.aborted)
          setRetryProgress(value => ({...value, [media.id]: percent}));
      }, controller.signal, original.name);
      if (before !== generation.current || controller.signal.aborted || deletedIds.current.has(media.id)) return;
      await refresh();
    } catch (e) {
      if (!isAborted(e) && before === generation.current && !deletedIds.current.has(media.id))
        setError(e instanceof ApiFailure && e.status === 409
          ? "처음 원본과 이름·내용이 다르거나 삭제된 항목입니다. 원래 파일을 확인하세요."
          : "원본 재시도를 확인하지 못했습니다. 보관함을 새로고침하고 같은 파일로 다시 시도하세요.");
    } finally {
      if (retries.current.get(media.id) === controller) retries.current.delete(media.id);
      if (before === generation.current) setRetryProgress(value => {
        const next={...value}; delete next[media.id]; return next;
      });
    }
  }
  async function download(id: string) {
    if (deletedIds.current.has(id)) return;
    const before = generation.current;
    const selected=data?.items.find(item=>item.id===id);
    const controller = new AbortController();
    const pending: { id: string; url?: string; timer?: ReturnType<typeof setTimeout> } = { id };
    downloads.current.set(controller, pending);
    try {
      const url = await scope.imageUrl(`/${selected?.filename ? "files" : "media"}/${encodeURIComponent(id)}/content`, controller.signal);
      if (before !== generation.current || controller.signal.aborted || deletedIds.current.has(id)) {
        scope.releaseUrl(url);
        return;
      }
      pending.url = url;
      const a = document.createElement("a");
      a.href = url;
      a.download = selected?.filename ?? "image.jpg";
      a.click();
      pending.timer = setTimeout(() => {
        scope.releaseUrl(url);
        downloads.current.delete(controller);
      }, 1000);
    } catch (e) {
      if (!isAborted(e) && !controller.signal.aborted && before === generation.current && !deletedIds.current.has(id)) setError("다운로드하지 못했습니다.");
    } finally {
      if (pending.url && pending.timer === undefined) {
        scope.releaseUrl(pending.url);
        downloads.current.delete(controller);
      }
      if (!pending.url) downloads.current.delete(controller);
    }
  }
  return (
    <details
      className="media-library"
      onToggle={(e) => {
        if (e.currentTarget.open) void refresh();
      }}
    >
      <summary>내 이미지·파일 보관함</summary>
      <p>삭제하면 해당 첨부에서 파생된 대화 답변도 철회됩니다.</p>
      {data && data.used_bytes >= data.quota_bytes * 0.85 && (
        <p>보관함 공간이 거의 찼습니다. 필요 없는 이미지를 지워 주세요.</p>
      )}
      {error && <p role="status">{error}</p>}
      <button type="button" onClick={() => void refresh()}>
        새로고침
      </button>
      <div className="media-library-grid">
        {data?.items.map((media) => (
          <article className="media-card" key={media.id}>
            <PrivateImage accountId={accountId} media={media} />
            {media.state === "ready" ? (
              <>
                <button type="button" onClick={() => onSelect(media)}>
                  대화에 첨부
                </button>
                {!media.filename && <button type="button" onClick={() => void download(media.id)}>
                  다운로드
                </button>}
              </>
            ) : (
              <>
                <span>완료되지 않은 업로드 · 원본을 다시 선택하세요</span>
                {media.filename && <label>원본 다시 선택
                  <input aria-label={`원본 다시 선택: ${media.filename}`} type="file" accept=".txt,.md,.markdown,.csv,.json"
                    disabled={retryProgress[media.id] !== undefined}
                    onChange={event => {void resume(media,event.target.files?.[0]);event.target.value="";}}/>
                </label>}
                {retryProgress[media.id] !== undefined && <progress aria-label="원본 재시도 진행률" value={retryProgress[media.id]} max={100}/>}
              </>
            )}
            <button type="button" onClick={() => void remove(media.id)}>
              {media.filename ? "파일 삭제" : "이미지 삭제"}
            </button>
          </article>
        ))}
      </div>
      {data?.items.length === 0 && <p>보관한 첨부가 없습니다.</p>}
    </details>
  );
}
