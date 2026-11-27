import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { BrowserFrame } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { browserClick, browserFrameSource, browserKeys, browserScroll, type BrowserInput } from "./browser-frame";
import "./personal-claude-browser.css";

/** What the server sends for the private sign-in screen. The page address is
 * never trusted: only a sanitized origin is displayed. */
export type ClaudeFrame = Pick<BrowserFrame, "frame_token" | "png_base64" | "width" | "height" | "url">;
export type ClaudeBrowserOperation = { request_id: string; state: string; browser_available?: boolean };
type FrameReply = { operation?: ClaudeBrowserOperation | null; frame?: ClaudeFrame | null };
type InputReply = { operation?: ClaudeBrowserOperation | null };

const path = "/personal/admin/engine/subscription";
const maxPng = 8_000_000;
const staleNotice = "화면이 바뀌었습니다. 최신 화면을 불러왔으니 다시 조작해 주세요.";
const frameFailure = "화면을 확인하지 못했습니다. 잠시 뒤 다시 확인합니다.";
const uncertainNotice = "조작이 전달되었는지 확인하지 못했습니다. 자동으로 다시 보내지 않습니다. 최신 화면을 확인한 뒤 필요하면 다시 조작해 주세요.";

export function validClaudeFrame(value: unknown): value is ClaudeFrame {
  const frame = value as ClaudeFrame | null;
  return !!frame && typeof frame === "object"
    && Number.isInteger(frame.width) && Number.isInteger(frame.height)
    && frame.width > 0 && frame.height > 0 && frame.width <= 8192 && frame.height <= 8192
    && typeof frame.frame_token === "string" && frame.frame_token.length > 0 && frame.frame_token.length <= 512
    && typeof frame.png_base64 === "string" && frame.png_base64.length > 0 && frame.png_base64.length <= maxPng
    && /^[A-Za-z0-9+/=]+$/.test(frame.png_base64)
    && typeof frame.url === "string";
}
/** Origin only: no path, query or fragment can reach the page. */
export function claudeOrigin(value: string): string {
  try {
    if (value.length > 2048) return "";
    const url = new URL(value);
    return url.protocol === "https:" && !url.username && !url.password ? url.origin : "";
  } catch { return ""; }
}

/** The private Claude sign-in screen. Pixels, the frame token and typed text live
 * only in this component and vanish with it. */
export function PersonalClaudeBrowser({ account, requestId, onOperation, onFailure, onRefresh, frameMs = 1000 }: {
  account: string; requestId: string;
  /** Latest operation reported by the server (never trusted for another id). */
  onOperation: (operation: ClaudeBrowserOperation) => void;
  /** Handles 401/403 (authority lost); returns true when the flow ended. */
  onFailure: (failure: unknown) => boolean;
  /** Ask the owner for a fresh status when the screen is no longer available. */
  onRefresh: () => void;
  frameMs?: number;
}) {
  const scope = useScope(account, `installation-engine-claude-browser:${requestId}`);
  const callbacks = useRef({ onOperation, onFailure, onRefresh });
  callbacks.current = { onOperation, onFailure, onRefresh };
  const [frame, setFrame] = useState<ClaudeFrame | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [expanded, setExpanded] = useState(false);
  const viewport = useRef<HTMLDivElement>(null);
  const centerPending = useRef(false);
  const image = useRef<HTMLImageElement>(null);
  const text = useRef<HTMLInputElement>(null);
  const shown = useRef<ClaudeFrame | null>(null);
  /** False from dispatch of an input until a newer frame arrives. */
  const fresh = useRef(false);
  const locked = useRef(false);
  const consumed = useRef<string | null>(null);
  const pressed = useRef<string | null>(null);
  const live = useRef(true);
  const lifetime = useRef<AbortController | null>(null);
  const pollNow = useRef<() => void>(() => {});

  function centerExpanded() {
    const view = viewport.current;
    if (!expanded || !centerPending.current || !view || !image.current?.complete) return;
    view.scrollLeft = Math.max(0, (view.scrollWidth - view.clientWidth) / 2);
    view.scrollTop = Math.max(0, (view.scrollHeight - view.clientHeight) / 2);
    centerPending.current = false;
  }
  const hasFrame = !!frame;
  useLayoutEffect(() => {
    centerPending.current = expanded;
    centerExpanded();
  }, [expanded, hasFrame]);

  useEffect(() => {
    live.current = true;
    const owner = new AbortController();
    lifetime.current = owner;
    let timer = 0;
    let running = false;
    let again = false;
    /** The server no longer offers a screen for this operation: stop asking. */
    let ended = false;
    async function poll() {
      if (!live.current || owner.signal.aborted || ended) return;
      if (running) { again = true; return; }
      window.clearTimeout(timer);
      if (document.visibilityState !== "visible") return;
      running = true;
      try {
        const reply = await scope.request<FrameReply>(path, "POST", { op: "frame", request_id: requestId }, owner.signal, { timeoutMs: 10_000 });
        if (owner.signal.aborted) return;
        if (document.visibilityState !== "visible") { drop(); return; }
        const operation = reply.operation;
        if (operation?.request_id !== requestId) throw new Error("invalid_operation");
        callbacks.current.onOperation(operation);
        if (operation.state !== "awaiting_code" || operation.browser_available !== true) { ended = true; drop(); return; }
        if (!validClaudeFrame(reply.frame)) throw new Error("invalid_frame");
        shown.current = reply.frame; fresh.current = reply.frame.frame_token !== consumed.current;
        setFrame(reply.frame);
        setError(previous => previous === frameFailure ? "" : previous);
      } catch (e) {
        if (owner.signal.aborted || isAborted(e)) return;
        if (callbacks.current.onFailure(e)) { ended = true; owner.abort(); drop(); if (text.current) text.current.value = ""; return; }
        if (e instanceof ApiFailure && (e.status === 404 || e.status === 409)) { drop(); callbacks.current.onRefresh(); return; }
        setError(frameFailure);
      } finally {
        running = false;
        if (live.current && !ended && !owner.signal.aborted && document.visibilityState === "visible") {
          timer = window.setTimeout(poll, again ? 0 : frameMs);
          again = false;
        }
      }
    }
    function drop() { shown.current = null; fresh.current = false; setFrame(null); }
    function visibility() {
      window.clearTimeout(timer);
      if (document.visibilityState === "visible") void poll(); else drop();
    }
    pollNow.current = () => { void poll(); };
    document.addEventListener("visibilitychange", visibility);
    void poll();
    const field = text.current;
    return () => {
      live.current = false;
      owner.abort();
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", visibility);
      shown.current = null; fresh.current = false;
      if (field) field.value = "";
      pollNow.current = () => {};
    };
  }, [scope, requestId, frameMs]);

  async function send(input: BrowserInput) {
    const seen = shown.current;
    const owner = lifetime.current;
    if (!seen || !fresh.current || locked.current || !live.current || !owner || owner.signal.aborted) return;
    locked.current = true; fresh.current = false; consumed.current = seen.frame_token;
    setBusy(true); setError("");
    try {
      const reply = await scope.request<InputReply>(path, "POST", { op: "input", request_id: requestId, frame_token: seen.frame_token, input }, owner.signal, { timeoutMs: 15_000 });
      if (owner.signal.aborted) return;
      if (reply.operation?.request_id === requestId) callbacks.current.onOperation(reply.operation);
    } catch (e) {
      if (owner.signal.aborted || isAborted(e)) return;
      if (callbacks.current.onFailure(e)) { owner.abort(); shown.current = null; fresh.current = false; setFrame(null); if (text.current) text.current.value = ""; return; }
      // A stale token is recoverable by looking at the current screen; anything
      // else may have been delivered, so it is never repeated automatically.
      setError(e instanceof ApiFailure && e.status === 409 ? staleNotice : uncertainNotice);
    } finally {
      if (live.current) { locked.current = false; setBusy(false); pollNow.current(); }
    }
  }
  useEffect(() => {
    const screen = image.current;
    if (!screen) return;
    function wheel(event: WheelEvent) {
      if (expanded || document.activeElement !== screen || event.ctrlKey) return;
      event.preventDefault();
      if (busy || !frame) return;
      const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? frame.height : 1;
      void send(browserScroll(event.deltaX * unit, event.deltaY * unit));
    }
    screen.addEventListener("wheel", wheel, { passive: false });
    return () => screen.removeEventListener("wheel", wheel);
  }, [frame, busy, expanded]);
  function keyboard(event: KeyboardEvent<HTMLImageElement>) {
    if (busy || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || event.nativeEvent.isComposing) return;
    const key = browserKeys[event.key];
    if (key) { event.preventDefault(); void send({ kind: "key", key }); }
  }
  function submitText(event: { preventDefault(): void }) {
    event.preventDefault();
    const value = text.current?.value ?? "";
    if (text.current) text.current.value = "";
    if (value && new TextEncoder().encode(value).byteLength <= 4096) void send({ kind: "insert_text", text: value });
  }

  const origin = frame ? claudeOrigin(frame.url) : "";
  const disabled = busy || !frame;
  return <section className="claude-browser" aria-label="Claude 로그인 화면">
    {error && <p role="alert" className="preview-error">{error}</p>}
    <p className="claude-browser-origin" aria-label="현재 페이지 출처">{origin || "주소를 표시할 수 없습니다"}</p>
    <button className="member-page-action claude-browser-zoom" type="button" disabled={!frame}
      aria-pressed={expanded} onClick={() => setExpanded(value => !value)}>
      {expanded ? "화면 전체 보기" : "로그인 화면 확대"}
    </button>
    {expanded && <p className="management-note">화면을 밀어서 보고, 누르고 싶은 곳을 선택하세요.</p>}
    <div ref={viewport} className={`claude-browser-screen${expanded ? " claude-browser-expanded" : ""}`} aria-busy={busy}>
      {frame
        ? <img ref={image} src={browserFrameSource(frame)} alt="Claude 로그인 화면" draggable={false} tabIndex={0}
            style={expanded ? { width: frame.width, minWidth: frame.width } : undefined} onLoad={centerExpanded}
            aria-label="Claude 로그인 화면 조작" onKeyDown={keyboard}
            onPointerDown={() => { pressed.current = frame.frame_token; }}
            onPointerCancel={() => { pressed.current = null; }}
            onClick={event => {
              const initial = pressed.current; pressed.current = null;
              if (busy || (initial !== null && initial !== frame.frame_token)) return;
              const input = browserClick(frame as BrowserFrame, event.currentTarget, event.clientX, event.clientY);
              if (!input) return;
              event.currentTarget.focus();
              void send(input);
            }}/>
        : <p role="status">화면을 기다리는 중…</p>}
    </div>
    <form className="claude-browser-text" onSubmit={submitText}>
      <label><span>선택한 칸에 입력</span>
        <input ref={text} type="password" autoComplete="off" autoCapitalize="off" autoCorrect="off" spellCheck={false}
          maxLength={1024} placeholder="전송하면 입력값을 지웁니다" disabled={disabled}/></label>
      <button className="member-page-action" type="submit" disabled={disabled}>입력 보내기</button>
    </form>
    <div className="claude-browser-tools" role="group" aria-label="Claude 로그인 화면 이동">
      <button className="member-page-action" type="button" disabled={disabled} aria-label="로그인 화면 위로 스크롤" onClick={() => void send({ kind: "scroll", delta_x: 0, delta_y: -500 })}>↑</button>
      <button className="member-page-action" type="button" disabled={disabled} aria-label="로그인 화면 아래로 스크롤" onClick={() => void send({ kind: "scroll", delta_x: 0, delta_y: 500 })}>↓</button>
      <button className="member-page-action" type="button" disabled={disabled} onClick={() => void send({ kind: "key", key: "tab" })}>Tab</button>
      <button className="member-page-action" type="button" disabled={disabled} onClick={() => void send({ kind: "key", key: "enter" })}>Enter</button>
    </div>
    <p className="management-note">화면을 눌러 칸을 선택한 뒤 위 입력란으로 이메일·비밀번호를 보내세요. 화면을 선택하면 Enter·Tab·방향키가 전달되며 Shift + Tab으로 화면 밖으로 나갈 수 있습니다. 로그인이 끝나면 자동으로 연결됩니다.</p>
  </section>;
}
