import { useEffect, useRef, useState } from "react";
import type { MediaSummary } from "../shared/api-types";
import { useScope } from "./account-scope";
import { isAborted } from "./api";

type Pending = { url?: string; timer?: ReturnType<typeof setTimeout> };

/** React renders UTF-8 as text, including Markdown and HTML-looking input. */
export function PrivateFile({ accountId, media }: {
  accountId: string;
  media: MediaSummary;
}) {
  const scope = useScope(accountId, media.id);
  const [text, setText] = useState<string>();
  const [error, setError] = useState("");
  const [deleted, setDeleted] = useState(media.state === "deleted");
  const generation = useRef(0);
  const pending = useRef(new Map<AbortController, Pending>());

  useEffect(() => {
    setText(undefined);
    setError("");
    setDeleted(media.state === "deleted");
    const clear = () => {
      generation.current++;
      for (const [controller, value] of pending.current) {
        controller.abort();
        if (value.timer !== undefined) clearTimeout(value.timer);
        if (value.url) scope.releaseUrl(value.url);
      }
      pending.current.clear();
    };
    const onDelete = (event: Event) => {
      if ((event as CustomEvent<string>).detail === media.id) {
        clear();
        setText(undefined);
        setError("");
        setDeleted(true);
      }
    };
    window.addEventListener("moosuri-media-deleted", onDelete);
    return () => {
      clear();
      window.removeEventListener("moosuri-media-deleted", onDelete);
    };
  }, [scope, media.id, media.revision, media.state]);

  async function preview() {
    const before = generation.current;
    const controller = new AbortController();
    pending.current.set(controller, {});
    try {
      const value = await scope.request<string>(
        `/files/${encodeURIComponent(media.id)}/preview`,
        "GET", undefined, controller.signal,
      );
      if (before === generation.current && !controller.signal.aborted) {
        setText(value);
        setError("");
      }
    } catch (e) {
      if (!isAborted(e) && before === generation.current)
        setError("파일을 불러오지 못했습니다.");
    } finally {
      pending.current.delete(controller);
    }
  }

  async function download() {
    const before = generation.current;
    const controller = new AbortController();
    const value: Pending = {};
    pending.current.set(controller, value);
    try {
      const url = await scope.imageUrl(
        `/files/${encodeURIComponent(media.id)}/content`, controller.signal,
      );
      if (before !== generation.current || controller.signal.aborted) {
        scope.releaseUrl(url);
        return;
      }
      value.url = url;
      const a = document.createElement("a");
      a.href = url;
      a.download = media.filename ?? "document.txt";
      a.click();
      value.timer = setTimeout(() => {
        scope.releaseUrl(url);
        pending.current.delete(controller);
      }, 1000);
    } catch (e) {
      if (!isAborted(e) && before === generation.current)
        setError("파일을 다운로드하지 못했습니다.");
    } finally {
      if (!value.url) pending.current.delete(controller);
    }
  }

  return (
    <div className="private-file">
      <strong>{media.filename}</strong>
      {deleted ? (
        <p>삭제된 파일 · 원문과 파생 답변 철회됨</p>
      ) : media.state !== "ready" ? (
        <p>완료되지 않은 업로드 · 원본을 다시 선택하세요</p>
      ) : (
        <>
          <button type="button" onClick={() => void preview()}>{media.media_type === "application/pdf" ? "텍스트 미리보기" : "원문 미리보기"}</button>
          <button type="button" onClick={() => void download()}>원본 다운로드</button>
          {text !== undefined && (
            <details open>
              <summary>{media.media_type === "application/pdf" ? "PDF에서 추출한 텍스트 · 스캔 이미지 제외" : "원문 · 읽기 전용 자료"}</summary>
              <pre className="private-file-text">{text}</pre>
            </details>
          )}
        </>
      )}
      {error && <p role="status">{error}</p>}
    </div>
  );
}
