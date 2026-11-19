import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import {
  browserClick, browserFrameSource, browserKeys, browserLocation, browserScroll,
  type BrowserInput,
} from "./browser-frame";
import { browserCanWatch, browserStateLabel, type PersonalBrowserFrame } from "./personal-browser";
import type { PersonalBrowser } from "./usePersonalBrowser";
import "./personal-browser.css";

const browserReasons: Partial<Record<string, string>> = {
  new_guidance: "새로 받은 추가 지시를 반영하려면 화면을 다시 확인해 주세요.",
  controller_expired: "조작권이 만료되었습니다. 이어서 조작하려면 제어권을 가져오세요.",
  browser_model_unavailable: "페이지는 열렸지만 어시스턴트의 답변을 받을 수 없습니다.",
  browser_unavailable: "브라우저 연결이 끊겼습니다. 마지막 동작이 반영됐는지 확인할 수 없습니다.",
  browser_worker_missing: "브라우저 세션이 종료되었습니다.",
  browser_stale: "페이지가 바뀌었습니다. 최신 화면을 확인해 주세요.",
};

export function PersonalBrowserCard({ browser, pending, onOpen }: {
  browser: PersonalBrowser; pending: boolean; onOpen: () => void;
}) {
  const workspace = browser.status?.workspace;
  if (!workspace && !pending) return null;
  const preview = browser.preview;
  return (
    <article className="member-browser-card" aria-label="브라우저 작업">
      <div>
        <strong>{workspace ? browserStateLabel[workspace.state] : "브라우저 작업 요청 중"}</strong>
        {workspace?.title && <span>{workspace.title}</span>}
      </div>
      {preview && <img src={browserFrameSource(preview)} alt="" />}
      {workspace && <button type="button" onClick={onOpen}>
        {["closed", "outcome_unknown"].includes(workspace.state) ? "브라우저 작업 보기" : "미리 보기 열기"}
      </button>}
    </article>
  );
}

function FrameView({ frame, controlling, busy, send, skipClickUntil }: {
  frame: PersonalBrowserFrame; controlling: boolean; busy: boolean;
  send: (input: BrowserInput) => void;
  skipClickUntil: RefObject<number>;
}) {
  const screen = useRef<HTMLImageElement>(null);
  const touch = useRef<{ x: number; y: number; proof: string } | null>(null);
  const pressed = useRef<string | null>(null);
  const keys: typeof browserKeys = { ...browserKeys, Delete: "delete", Home: "home", End: "end",
    PageUp: "page_up", PageDown: "page_down", " ": "space" };
  function keyDown(event: KeyboardEvent<HTMLImageElement>) {
    if (!controlling || busy || document.activeElement !== event.currentTarget ||
        event.altKey || event.ctrlKey || event.metaKey || event.shiftKey ||
        event.nativeEvent.isComposing) return;
    const key = keys[event.key];
    if (key) {
      event.preventDefault();
      event.stopPropagation();
      send({ kind: "key", key });
    }
  }
  useEffect(() => {
    const element = screen.current;
    if (!element || !controlling) return;
    function wheel(event: WheelEvent) {
      if (document.activeElement !== element || event.ctrlKey) return;
      event.preventDefault();
      if (busy) return;
      const scale = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? frame.height : 1;
      send(browserScroll(event.deltaX * scale, event.deltaY * scale));
    }
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [frame, controlling, busy, send]);

  return (
    <img ref={screen} src={browserFrameSource(frame)} alt="개인 브라우저의 현재 화면"
      width={frame.width} height={frame.height} draggable={false}
      tabIndex={controlling ? 0 : undefined} data-controlling={controlling || undefined}
      onKeyDown={keyDown}
      onPointerDown={() => { pressed.current = frame.frame_token; }}
      onPointerCancel={() => { pressed.current = null; }}
      onClick={(event) => {
        const initialProof = pressed.current;
        pressed.current = null;
        if (!controlling || busy || Date.now() < skipClickUntil.current ||
            (initialProof !== null && initialProof !== frame.frame_token)) return;
        event.currentTarget.focus({ preventScroll: true });
        const input = browserClick(frame, event.currentTarget, event.clientX, event.clientY);
        if (input) send(input);
      }}
      onTouchStart={(event) => {
        const point = event.touches.length === 1 ? event.touches[0] : null;
        touch.current = controlling && !busy && point
          ? { x: point.clientX, y: point.clientY, proof: frame.frame_token } : null;
      }}
      onTouchCancel={() => { touch.current = null; }}
      onTouchEnd={(event) => {
        const start = touch.current;
        touch.current = null;
        const point = event.changedTouches[0];
        if (!controlling || busy || !start || !point || start.proof !== frame.frame_token) return;
        const x = start.x - point.clientX;
        const y = start.y - point.clientY;
        if (Math.hypot(x, y) < 12) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (!bounds.width || !bounds.height) return;
        skipClickUntil.current = Date.now() + 450;
        send(browserScroll(x * frame.width / bounds.width, y * frame.height / bounds.height));
      }}
    />
  );
}

export function PersonalBrowserViewer({ browser, onClose }: {
  browser: PersonalBrowser; onClose: () => void;
}) {
  const [width, setWidth] = useState<number>();
  const [notice, setNotice] = useState("");
  const [modal, setModal] = useState(() => window.matchMedia?.("(max-width: 860px)").matches ?? false);
  const panel = useRef<HTMLElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const text = useRef<HTMLInputElement>(null);
  const composing = useRef(false);
  const drag = useRef<number | null>(null);
  const focusFrame = useRef(false);
  const skipClickUntil = useRef(0);
  const value = browser.status?.workspace;
  const frame = browser.frame;
  const busy = !!browser.busy;
  const canUseFrame = browser.canUseFrame && !busy;
  const available = !!value && !["stopping", "closed"].includes(value.state);
  const watchable = browserCanWatch(value ?? null);
  const canTake = watchable && !!value && ["reading", "ready", "completed", "user_control"].includes(value.state);
  const location = browserLocation(frame?.url ?? value?.url);
  const title = frame?.url === "about:blank" ? "빈 페이지" : location?.hostname ?? "브라우저 작업";

  useEffect(() => {
    close.current?.focus({ preventScroll: true });
    const viewport = window.matchMedia?.("(max-width: 860px)");
    const update = (event: MediaQueryListEvent) => setModal(event.matches);
    viewport?.addEventListener("change", update);
    return () => viewport?.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    if (browser.controls && frame && focusFrame.current) {
      if (document.activeElement === document.body) {
        panel.current?.querySelector<HTMLImageElement>(".member-browser-screen img")?.focus({ preventScroll: true });
      }
      focusFrame.current = false;
    }
  }, [frame, browser.controls]);

  function input(event: BrowserInput) {
    if (!browser.controls || !frame || !canUseFrame) return;
    focusFrame.current = document.activeElement?.matches(".member-browser-screen img") ?? false;
    browser.mutate({ kind: "input", input: event, frame });
  }
  function panelKeys(event: KeyboardEvent<HTMLElement>) {
    if (event.key === "Escape" && !event.nativeEvent.isComposing) {
      event.stopPropagation();
      onClose();
    }
    if (event.key !== "Tab" || !modal) return;
    const focusable = Array.from(event.currentTarget.querySelectorAll<HTMLElement>(
      "button:not([disabled]), input:not([disabled]), img[tabindex='0']",
    ));
    const first = focusable[0], last = focusable.at(-1);
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }
  async function copy() {
    if (!location) return;
    try {
      await navigator.clipboard.writeText(location.href);
      setNotice("현재 주소를 복사했습니다.");
    } catch {
      setNotice("주소를 복사하지 못했습니다.");
    }
  }
  return (
    <aside ref={panel} className="member-browser-viewer" aria-label="개인 브라우저 작업 공간"
      role={modal ? "dialog" : undefined} aria-modal={modal || undefined}
      style={width ? { width: `clamp(360px, ${width}px, calc(100% - 280px))` } : undefined}
      onKeyDown={panelKeys}>
      <div className="member-browser-resize" role="separator" aria-orientation="vertical"
        aria-label="브라우저 뷰어 너비 조절" tabIndex={0}
        onPointerDown={(event) => {
          drag.current = event.pointerId;
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          const bounds = panel.current?.parentElement?.getBoundingClientRect();
          if (drag.current === event.pointerId && bounds) setWidth(bounds.right - event.clientX);
        }}
        onLostPointerCapture={() => { drag.current = null; }}
        onKeyDown={(event) => {
          if (!["ArrowLeft", "ArrowRight"].includes(event.key)) return;
          event.preventDefault();
          const previous = panel.current?.getBoundingClientRect().width ?? 500;
          setWidth(previous + (event.key === "ArrowLeft" ? 40 : -40));
        }}
      />
      <header className="member-browser-header">
        <div className="member-browser-location">
          <h2 title={location?.href}>{title}</h2>
          {value && <p>{browserStateLabel[value.state]}</p>}
        </div>
        <div className="member-browser-actions">
          {location && <button type="button" onClick={() => void copy()}>주소 복사</button>}
          {available && <button type="button" disabled={busy} onClick={() => browser.mutate({ kind: "close" })}>작업 중지</button>}
          <button type="button" ref={close} onClick={onClose}>뷰어 닫기</button>
        </div>
      </header>
      {!!value?.tabs.length && (
        <div className="member-browser-tabs" role="group" aria-label="브라우저 탭">
          {value.tabs.map((tab) => {
            const active = tab.id === value.active_tab_id;
            const label = tab.title || browserLocation(tab.url)?.hostname || "빈 탭";
            return (
              <span key={tab.id}>
                <button type="button" aria-current={active ? "true" : undefined}
                  disabled={!browser.controls || busy || active}
                  onClick={() => browser.mutate({ kind: "tabs", action: "switch", tab_id: tab.id })}>
                  {label}
                </button>
                {browser.controls && active && <button type="button" aria-label={`${label} 탭 닫기`}
                  disabled={busy} onClick={() => browser.mutate({ kind: "tabs", action: "close", tab_id: tab.id })}>×</button>}
              </span>
            );
          })}
        </div>
      )}
      <div className="member-browser-content">
        {notice && <p role="status">{notice}</p>}
        {browser.error && <p className="member-browser-error" role="alert">{browser.error}</p>}
        {value?.reason && browserReasons[value.reason] && <p role="status">{browserReasons[value.reason]}</p>}
        {(watchable || value?.state === "starting") && (
          <div className="member-browser-screen">
            {frame ? <FrameView frame={frame} controlling={browser.controls} busy={!canUseFrame}
              send={input} skipClickUntil={skipClickUntil} /> :
              <p role="status">{busy ? "화면을 갱신하는 중…" : "브라우저 화면을 기다리는 중…"}</p>}
          </div>
        )}
        {browser.controls && (
          <div className="member-browser-input">
            <p>직접 조작하는 동안 무수리의 브라우저 사용이 일시 정지됩니다.</p>
            <form onSubmit={(event) => {
              event.preventDefault();
              if (composing.current || !canUseFrame || !text.current) return;
              const value = text.current.value;
              if (!value || new TextEncoder().encode(value).length > 4096) return;
              text.current.value = "";
              input({ kind: "insert_text", text: value });
            }}>
              <label htmlFor="member-browser-text">선택한 칸에 입력</label>
              <div>
                <input id="member-browser-text" ref={text} type="password" maxLength={1024}
                  autoComplete="off" placeholder="전송 후 입력값을 지웁니다"
                  onCompositionStart={() => { composing.current = true; }}
                  onCompositionEnd={() => { composing.current = false; }} />
                <button type="submit" disabled={!canUseFrame}>입력</button>
              </div>
            </form>
            <div className="member-browser-actions" role="group" aria-label="브라우저 키와 스크롤">
              {(["tab", "backspace", "enter"] as const).map((key) => (
                <button type="button" key={key} disabled={!canUseFrame}
                  onClick={() => input({ kind: "key", key })}>
                  {{ tab: "Tab", backspace: "Backspace", enter: "Enter" }[key]}
                </button>
              ))}
              <button type="button" disabled={!canUseFrame} aria-label="브라우저 위로 스크롤"
                onClick={() => input(browserScroll(0, -500))}>↑</button>
              <button type="button" disabled={!canUseFrame} aria-label="브라우저 아래로 스크롤"
                onClick={() => input(browserScroll(0, 500))}>↓</button>
            </div>
          </div>
        )}
        {value && ["ready", "completed"].includes(value.state) && !browser.controls && !!value.links?.length && (
          <div className="member-browser-links">
            <h3>페이지에서 확인한 링크</h3>
            {value.links.filter((link) => browserLocation(link.url)).map((link) => (
              <button key={link.url} type="button" disabled={busy} title={link.url}
                onClick={() => browser.mutate({ kind: "tabs", action: "follow", url: link.url })}>
                {link.text || browserLocation(link.url)?.hostname}
              </button>
            ))}
          </div>
        )}
      </div>
      <footer className="member-browser-actions member-browser-footer">
        <button type="button" disabled={busy} onClick={browser.refresh}>새로고침</button>
        {browser.canRetryResume ? (
          <button type="button" className="member-browser-primary" disabled={busy}
            onClick={() => browser.mutate({ kind: "resume" })}>완료 결과 다시 확인</button>
        ) : browser.controls ? (
          <button type="button" className="member-browser-primary" disabled={!canUseFrame || !location}
            onClick={() => browser.mutate({ kind: "resume" })}>완료</button>
        ) : canTake && (
          <button type="button" className="member-browser-primary" disabled={busy}
            onClick={() => browser.mutate({ kind: "takeover" })}>브라우저 제어</button>
        )}
      </footer>
    </aside>
  );
}
