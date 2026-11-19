import { Icon } from "@astryxdesign/core/Icon";
import { useLayoutEffect, useRef, type ReactNode } from "react";

export function PersonalChatComposer({
  draft, disabled, busy, waiting, mode, onDraft, onSend, browserMode,
}: {
  draft: string;
  disabled: boolean;
  busy: boolean;
  waiting: string;
  mode: "chat" | "start" | "guide";
  onDraft: (value: string) => void;
  onSend: () => void;
  browserMode?: ReactNode;
}) {
  const editor = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  const tooLong = new TextEncoder().encode(draft).length > (mode === "guide" ? 2000 : 8192);
  const canSend = !disabled && !busy && !tooLong && !!draft.trim();
  const placeholder = disabled ? "답변을 받을 수 없습니다."
    : busy ? waiting
    : mode === "start" ? "웹 주소와 할 일을 입력하세요"
    : mode === "guide" ? "브라우저 작업에 추가 지시하기"
    : "메시지 보내기";

  useLayoutEffect(() => {
    const element = editor.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${Math.min(144, Math.max(44, element.scrollHeight))}px`;
  }, [draft]);

  return (
    <footer className="member-composer">
      {browserMode}
      <form className="member-composer-dock" onSubmit={(event) => {
        event.preventDefault();
        if (canSend && !composing.current) onSend();
      }}>
        <label className="sr-only" htmlFor="preview-message">메시지</label>
        <textarea id="preview-message" ref={editor}
          value={draft} onChange={(event) => onDraft(event.target.value)}
          placeholder={placeholder} disabled={disabled}
          maxLength={8192} rows={1}
          aria-describedby={tooLong ? "member-composer-limit" : undefined}
          aria-keyshortcuts="Enter Shift+Enter"
          onCompositionStart={() => { composing.current = true; }}
          onCompositionEnd={() => { composing.current = false; }}
          onKeyDown={(event) => {
            if (event.key !== "Enter" || event.shiftKey ||
                event.nativeEvent.isComposing || composing.current ||
                event.nativeEvent.keyCode === 229) return;
            event.preventDefault();
            if (canSend) onSend();
          }}
        />
        <button className="member-composer-send" type="submit" aria-label="보내기" disabled={!canSend}>
          <Icon icon="arrowUp" size="md" color="inherit" />
        </button>
      </form>
      {tooLong && (
        <p id="member-composer-limit" role="alert">{mode === "guide"
          ? "추가 지시가 너무 깁니다. 내용을 줄여 주세요."
          : "메시지가 너무 깁니다. 내용을 줄여 주세요."}</p>
      )}
    </footer>
  );
}
