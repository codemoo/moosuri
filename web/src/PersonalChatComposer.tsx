import { Button } from "@astryxdesign/core/Button";
import { ChatComposer } from "@astryxdesign/core/Chat";
import { Icon } from "@astryxdesign/core/Icon";
import { useLayoutEffect, useRef } from "react";

export function PersonalChatComposer({
  draft, disabled, busy, onDraft, onSend,
}: {
  draft: string;
  disabled: boolean;
  busy: boolean;
  onDraft: (value: string) => void;
  onSend: () => void;
}) {
  const editor = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  const tooLong = new TextEncoder().encode(draft).length > 8192;
  const canSend = !disabled && !busy && !tooLong && !!draft.trim();

  useLayoutEffect(() => {
    const element = editor.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${Math.min(144, Math.max(44, element.scrollHeight))}px`;
  }, [draft]);

  return (
    <footer className="member-composer">
      <ChatComposer className="member-composer-dock" elevation="none" onSubmit={() => {
        if (canSend && !composing.current) onSend();
      }} input={
        <form className="member-composer-input" onSubmit={(event) => {
          event.preventDefault();
          if (canSend && !composing.current) onSend();
        }}>
          <label className="sr-only" htmlFor="preview-message">메시지</label>
          <textarea id="preview-message" ref={editor}
            value={draft} onChange={(event) => onDraft(event.target.value)}
            placeholder="메시지를 입력하세요…" disabled={disabled}
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
          <Button className="member-composer-send" variant="primary" type="submit"
            label="보내기" isIconOnly isDisabled={!canSend}
            icon={<Icon icon="arrowUp" size="md" color="inherit" />} />
        </form>
      } />
      {tooLong && (
        <p id="member-composer-limit" role="alert">메시지가 너무 깁니다. 내용을 줄여 주세요.</p>
      )}
    </footer>
  );
}
