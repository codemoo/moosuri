import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type {
  BrowserControl,
  BrowserFrame,
  BrowserWorkspaceStatus,
  BrowserWorkspaceView,
} from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";
import "./browser-workspace.css";

type Props = {
  accountId: string;
  threadId: string | null;
  onExpired: () => void;
  onChanged?: () => void;
};
type Input =
  | { kind: "click"; x: number; y: number }
  | { kind: "insert_text"; text: string }
  | { kind: "key"; key: string }
  | { kind: "scroll"; delta_x: number; delta_y: number };
const labels: Record<BrowserWorkspaceView["state"], string> = {
  starting: "브라우저 준비 중",
  ready: "브라우저 준비됨",
  waiting_for_user: "직접 확인을 기다려요",
  user_control: "사용자가 조작 중",
  closed: "브라우저 닫힘",
  expired: "브라우저 만료됨",
  outcome_unknown: "브라우저 상태 확인 필요",
};
const terminal = (workspace: BrowserWorkspaceView) =>
  ["closed", "expired", "outcome_unknown"].includes(workspace.state);
const refreshingFrame = "페이지가 바뀌고 있습니다. 화면을 다시 확인할게요.";
function publicUrl(text: string) {
  try {
    const url = new URL(text);
    return text.length <= 2048 &&
      ["http:", "https:"].includes(url.protocol) &&
      !url.username &&
      !url.password
      ? url.href
      : null;
  } catch {
    return null;
  }
}
function safeLocation(text?: string | null) {
  return text ? (publicUrl(text) ?? "주소를 표시할 수 없습니다") : "";
}

/** The keyed lifetime also removes pixels and uncontrolled secrets synchronously
 * when the caller changes accounts or threads. No frames or capabilities escape it. */
export function BrowserWorkspace(props: Props) {
  return props.threadId ? (
    <Workspace
      key={`${props.accountId}:${props.threadId}`}
      {...props}
      threadId={props.threadId}
    />
  ) : null;
}
function Workspace(props: Props & { threadId: string }) {
  const scope = useScope(props.accountId, props.threadId);
  const callbacks = useRef(props);
  callbacks.current = props;
  const base = `/threads/${encodeURIComponent(props.threadId)}/browser`;
  const [status, setStatus] = useState<BrowserWorkspaceStatus | null>(null);
  const [open, setOpen] = useState(false);
  const [frame, setFrame] = useState<BrowserFrame | null>(null);
  const [controlling, setControlling] = useState(false);
  const [busy, setBusy] = useState(false);
  const [resumeUncertain, setResumeUncertain] = useState(false);
  const pendingResume = useRef<{
    workspace_id: string;
    revision: number;
    control_token: string;
  } | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState(false);
  const [location, setLocation] = useState("");
  const textField = useRef<HTMLInputElement>(null);
  const imageField = useRef<HTMLImageElement>(null);
  const restoreImageFocus = useRef(false);
  const live = useRef(true);
  const blocked = useRef(false);
  const lifetime = useRef(new AbortController());
  const generation = useRef(0);
  const workspace = useRef<BrowserWorkspaceView | null>(null);
  const pixels = useRef<BrowserFrame | null>(null);
  const token = useRef<string | null>(null);
  const opened = useRef(false);
  const locked = useRef(false);
  const pollNow = useRef<() => void>(() => {});
  const seenNotice = useRef(false);

  function clearPrivate() {
    generation.current++;
    token.current = null;
    pixels.current = null;
    if (textField.current) textField.current.value = "";
    setFrame(null);
    setControlling(false);
    if (pendingResume.current) setResumeUncertain(true);
  }
  function current(epoch: number) {
    return (
      live.current &&
      !blocked.current &&
      epoch === generation.current &&
      document.visibilityState === "visible"
    );
  }
  async function request<T>(
    suffix: string,
    method = "GET",
    body?: unknown,
  ): Promise<T> {
    const controller = new AbortController();
    const owner = lifetime.current;
    const abort = () => controller.abort();
    owner.signal.addEventListener("abort", abort, { once: true });
    if (owner.signal.aborted) controller.abort();
    const timeout = window.setTimeout(
      abort,
      suffix === "" || suffix === "/navigate" ? 75000 : 45000,
    );
    try {
      return await scope.request<T>(
        `${base}${suffix}`,
        method,
        body,
        controller.signal,
      );
    } finally {
      window.clearTimeout(timeout);
      owner.signal.removeEventListener("abort", abort);
    }
  }
  function boundary(value: unknown) {
    if (value instanceof ApiFailure && [401, 403, 404].includes(value.status)) {
      clearPrivate();
      pendingResume.current = null;
      setResumeUncertain(false);
      workspace.current = null;
      setStatus(null);
      setLocation("");
      blocked.current = true;
      lifetime.current.abort();
      if (value.status === 401) callbacks.current.onExpired();
      return true;
    }
    return false;
  }
  function apply(next: BrowserWorkspaceView | null) {
    const old = workspace.current;
    if (next && next.thread_id !== props.threadId)
      throw new Error("invalid_workspace");
    if (
      !next ||
      !old ||
      next.id !== old.id ||
      next.revision !== old.revision ||
      terminal(next) ||
      (token.current && next.state !== "user_control")
    ) {
      // A fresh takeover in another tab invalidates this capability and its pixels.
      if (token.current || pixels.current) clearPrivate();
    }
    workspace.current = next;
    setStatus({ configured: true, workspace: next });
    if (next?.url && (!old || old.url !== next.url || old.id !== next.id))
      setLocation(safeLocation(next.url));
  }
  async function readFrame(epoch: number) {
    const value = workspace.current;
    if (
      !value ||
      terminal(value) ||
      value.state === "starting" ||
      !opened.current ||
      !current(epoch)
    )
      return;
    let result: BrowserFrame;
    try {
      result = await request<BrowserFrame>("/frame", "POST", {
        workspace_id: value.id,
        revision: value.revision,
        ...(token.current ? { control_token: token.current } : {}),
      });
    } catch (failure) {
      if (!(failure instanceof ApiFailure) || failure.status !== 409) throw failure;
      // Navigation can invalidate a capture after a successful human input. Check
      // authority again before retaining control; retry only observation, never input.
      const next = await request<BrowserWorkspaceStatus>("");
      if (!current(epoch)) return;
      if (!next.configured) throw failure;
      apply(next.workspace);
      if (!current(epoch)) return;
      pixels.current = null;
      setFrame(null);
      setError(refreshingFrame);
      return;
    }
    if (
      !current(epoch) ||
      !opened.current ||
      workspace.current?.id !== result.workspace_id ||
      workspace.current?.revision !== result.revision
    )
      return;
    if (
      !Number.isInteger(result.width) ||
      !Number.isInteger(result.height) ||
      result.width <= 0 ||
      result.height <= 0 ||
      !result.frame_token ||
      !/^[A-Za-z0-9+/=]+$/.test(result.png_base64)
    )
      throw new Error("invalid_frame");
    pixels.current = result;
    setFrame(result);
    setError((previous) => previous === refreshingFrame ? "" : previous);
  }
  useEffect(() => {
    live.current = true;
    let timer: number | undefined;
    let running = false;
    let rerun = false;
    let relevant = true;
    async function poll() {
      if (running) {
        rerun = true;
        return;
      }
      window.clearTimeout(timer);
      if (
        !live.current ||
        blocked.current ||
        document.visibilityState !== "visible"
      )
        return;
      if (locked.current) {
        timer = window.setTimeout(poll, 2000);
        return;
      }
      running = true;
      let epoch = generation.current;
      try {
        const next = await request<BrowserWorkspaceStatus>("");
        if (!current(epoch)) return;
        if (!next.configured) {
          relevant = false;
          clearPrivate();
          workspace.current = null;
          setStatus({ configured: false, workspace: null });
          return;
        }
        apply(next.workspace);
        epoch = generation.current;
        await readFrame(epoch);
      } catch (value) {
        if (current(epoch) && !boundary(value)) {
          // An invalid/expired controller must never keep sending old credentials.
          if (token.current || pixels.current) clearPrivate();
          if (opened.current && !isAborted(value))
            setError("화면을 확인하지 못했습니다. 잠시 뒤 다시 확인합니다.");
        }
      } finally {
        running = false;
        if (
          live.current &&
          !blocked.current &&
          relevant &&
          document.visibilityState === "visible"
        ) {
          timer = window.setTimeout(poll, rerun ? 0 : 2000);
          rerun = false;
        }
      }
    }
    pollNow.current = () => {
      void poll();
    };
    function visibility() {
      window.clearTimeout(timer);
      lifetime.current.abort();
      lifetime.current = new AbortController();
      clearPrivate();
      if (document.visibilityState === "visible") {
        relevant = true;
        void poll();
      }
    }
    document.addEventListener("visibilitychange", visibility);
    void poll();
    return () => {
      live.current = false;
      generation.current++;
      token.current = null;
      pendingResume.current = null;
      pixels.current = null;
      if (textField.current) textField.current.value = "";
      lifetime.current.abort();
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", visibility);
      pollNow.current = () => {};
    };
  }, [scope, base]);

  useEffect(() => {
    if (frame && controlling && restoreImageFocus.current) {
      if (
        document.activeElement === document.body ||
        document.activeElement === imageField.current
      )
        imageField.current?.focus({ preventScroll: true });
      restoreImageFocus.current = false;
    }
  }, [frame, controlling]);

  function showPanel() {
    opened.current = true;
    setOpen(true);
    setError("");
    pollNow.current();
  }
  function hidePanel() {
    opened.current = false;
    restoreImageFocus.current = false;
    setOpen(false);
    clearPrivate();
    // Leaving the viewer does not release the server pause or resume an agent.
    lifetime.current.abort();
    lifetime.current = new AbortController();
  }
  async function mutate(
    kind: "takeover" | "input" | "navigate" | "resume" | "close" | "create",
    input?: Input,
  ) {
    if (
      locked.current ||
      blocked.current ||
      document.visibilityState !== "visible"
    )
      return;
    const value = workspace.current;
    const retryResume = kind === "resume" ? pendingResume.current : null;
    const capability = retryResume?.control_token ?? token.current;
    const seenFrame = pixels.current;
    const url = publicUrl(location);
    if (kind !== "create" && !value) return;
    if (["input", "navigate", "resume"].includes(kind) && !capability) return;
    if (kind === "input" && (!seenFrame || !input)) return;
    if (["navigate", "create"].includes(kind) && !url) {
      setError("http 또는 https 주소를 입력해 주세요.");
      return;
    }
    restoreImageFocus.current = document.activeElement === imageField.current;
    locked.current = true;
    setBusy(true);
    setError("");
    clearPrivate();
    const epoch = generation.current;
    const identity = retryResume
      ? {
          workspace_id: retryResume.workspace_id,
          revision: retryResume.revision,
        }
      : value
        ? { workspace_id: value.id, revision: value.revision }
        : {};
    if (kind === "resume" && value && capability) {
      pendingResume.current = {
        workspace_id: retryResume?.workspace_id ?? value.id,
        revision: retryResume?.revision ?? value.revision,
        control_token: capability,
      };
    } else {
      pendingResume.current = null;
      setResumeUncertain(false);
    }
    const body =
      kind === "create"
        ? { url }
        : kind === "input"
          ? {
              ...identity,
              control_token: capability,
              frame_token: seenFrame!.frame_token,
              input,
            }
          : kind === "navigate"
            ? { ...identity, control_token: capability, url }
            : kind === "resume"
              ? { ...identity, control_token: capability }
              : identity;
    try {
      const result = await request<
        BrowserControl | BrowserWorkspaceView | undefined
      >(kind === "create" ? "" : `/${kind}`, "POST", body);
      if (!current(epoch)) return;
      if (kind === "takeover") {
        const control = result as BrowserControl;
        if (
          !control?.workspace ||
          control.workspace.thread_id !== props.threadId ||
          !control.control_token
        )
          throw new Error("invalid_control");
        apply(control.workspace);
        token.current = control.control_token;
        setControlling(true);
        if (!seenNotice.current) {
          seenNotice.current = true;
          setNotice(true);
        }
      } else if (kind === "create" || kind === "navigate") {
        if (result && "id" in result) apply(result);
        if (kind === "navigate") {
          token.current = capability;
          setControlling(true);
        }
      } else if (kind === "input") {
        token.current = capability;
        setControlling(true);
      } else if (kind === "resume") {
        // The server records a visible user turn; normal chat SSE owns its result.
        pendingResume.current = null;
        setResumeUncertain(false);
        callbacks.current.onChanged?.();
      } else if (kind === "close" && value)
        apply({ ...value, state: "closed" });
      if (!["resume", "close"].includes(kind))
        await readFrame(generation.current);
    } catch (value) {
      if (current(epoch) && !boundary(value)) {
        clearPrivate();
        if (kind === "resume") {
          if (value instanceof ApiFailure && value.status < 500)
            pendingResume.current = null;
          setResumeUncertain(!!pendingResume.current);
        }
        setError(
          value instanceof ApiFailure && value.status === 409
            ? "다른 탭에서 조작했거나 화면이 바뀌었습니다. 최신 화면을 확인하고 다시 조작권을 가져오세요."
            : "처리 결과를 확인하지 못했습니다. 자동으로 다시 실행하지 않습니다. 최신 화면을 확인해 주세요.",
        );
      }
    } finally {
      if (live.current) {
        locked.current = false;
        setBusy(false);
        pollNow.current();
      }
    }
  }
  function keyboard(event: KeyboardEvent<HTMLImageElement>) {
    if (
      document.activeElement !== event.currentTarget ||
      !token.current ||
      busy ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey ||
      event.shiftKey ||
      event.nativeEvent.isComposing
    )
      return;
    const keys: Record<string, string> = {
      Enter: "enter",
      Tab: "tab",
      Backspace: "backspace",
      Escape: "escape",
      ArrowUp: "arrow_up",
      ArrowDown: "arrow_down",
      ArrowLeft: "arrow_left",
      ArrowRight: "arrow_right",
    };
    const key = keys[event.key];
    if (key) {
      event.preventDefault();
      void mutate("input", { kind: "key", key });
    }
  }
  if (!status?.configured) return null;
  const value = status.workspace;
  const ended = !value || terminal(value);
  const canTake = value && !ended && value.state !== "starting";
  return (
    <div className="browser-workspace">
      <div className="browser-workspace-card">
        <span
          className={`browser-state-dot ${value?.state ?? "empty"}`}
          aria-hidden="true"
        />
        <div>
          <strong>{value ? labels[value.state] : "브라우저 작업"}</strong>
          {value?.title && <span>{value.title}</span>}
        </div>
        <button className="text-button" onClick={showPanel}>
          {ended ? "브라우저 열기" : "화면 보기 ↗"}
        </button>
      </div>
      {open && (
        <section
          className="browser-workspace-panel"
          aria-label="개인 브라우저 작업 공간"
        >
          <header>
            <div>
              <span className="eyebrow">PRIVATE BROWSER</span>
              <h2>{value ? labels[value.state] : "브라우저 열기"}</h2>
            </div>
            <button
              className="text-button"
              onClick={hidePanel}
              aria-label="브라우저 화면 접기"
            >
              접기
            </button>
          </header>
          <div className="browser-workspace-content">
            {value?.reason && <p className="browser-reason">{value.reason}</p>}
            {error && (
              <p className="browser-error" role="alert">
                {error}
              </p>
            )}
            {controlling && notice && (
              <p className="browser-privacy">
                직접 조작하는 동안 에이전트의 관측은 멈춥니다. 로그인 세션과 쿠키는
                서버의 전용 브라우저에 종료·만료까지 유지됩니다.{" "}
                <button
                  className="text-button"
                  onClick={() => setNotice(false)}
                >
                  알겠어요
                </button>
              </p>
            )}
            {value && (
              <p className="browser-current-url" aria-label="현재 페이지 주소">
                {safeLocation(frame?.url ?? value.url)}
              </p>
            )}
            <form
              className="browser-address"
              onSubmit={(event) => {
                event.preventDefault();
                void mutate(ended ? "create" : "navigate");
              }}
            >
              <label htmlFor={`browser-url-${props.threadId}`}>
                {ended ? "열 주소" : "이동할 주소"}
              </label>
              <div>
                <input
                  id={`browser-url-${props.threadId}`}
                  aria-label={ended ? "열 주소" : "이동할 주소"}
                  type="url"
                  maxLength={2048}
                  placeholder="https://example.com"
                  value={location}
                  onChange={(event) => setLocation(event.target.value)}
                  disabled={busy || (!ended && !controlling)}
                  autoComplete="off"
                />
                {(ended || controlling) && (
                  <button className="secondary" disabled={busy}>
                    {ended ? "새 브라우저 열기" : "이동"}
                  </button>
                )}
              </div>
            </form>
            {!ended && (
              <div className="browser-screen">
                {frame ? (
                  <img
                    ref={imageField}
                    src={`data:image/png;base64,${frame.png_base64}`}
                    alt="개인 브라우저의 현재 화면"
                    draggable={false}
                    tabIndex={controlling ? 0 : -1}
                    aria-label={
                      controlling ? "브라우저 화면 조작" : "브라우저 화면 보기"
                    }
                    onKeyDown={keyboard}
                    onClick={(event) => {
                      if (!controlling || busy) return;
                      const bounds =
                        event.currentTarget.getBoundingClientRect();
                      if (!bounds.width || !bounds.height) return;
                      event.currentTarget.focus();
                      void mutate("input", {
                        kind: "click",
                        x: Math.min(
                          frame.width - 1,
                          Math.max(
                            0,
                            Math.floor(
                              ((event.clientX - bounds.left) * frame.width) /
                                bounds.width,
                            ),
                          ),
                        ),
                        y: Math.min(
                          frame.height - 1,
                          Math.max(
                            0,
                            Math.floor(
                              ((event.clientY - bounds.top) * frame.height) /
                                bounds.height,
                            ),
                          ),
                        ),
                      });
                    }}
                    onWheel={(event) => {
                      if (
                        !controlling ||
                        busy ||
                        document.activeElement !== event.currentTarget
                      )
                        return;
                      const scale =
                        event.deltaMode === 1
                          ? 16
                          : event.deltaMode === 2
                            ? frame.height
                            : 1;
                      void mutate("input", {
                        kind: "scroll",
                        delta_x: Math.max(
                          -2000,
                          Math.min(2000, Math.round(event.deltaX * scale)),
                        ),
                        delta_y: Math.max(
                          -2000,
                          Math.min(2000, Math.round(event.deltaY * scale)),
                        ),
                      });
                    }}
                  />
                ) : (
                  <p role="status">
                    {busy ? "화면을 갱신하는 중…" : "화면을 기다리는 중…"}
                  </p>
                )}
              </div>
            )}
            {controlling && (
              <form
                className="browser-text"
                onSubmit={(event) => {
                  event.preventDefault();
                  const text = textField.current?.value ?? "";
                  if (textField.current) textField.current.value = "";
                  if (text && new TextEncoder().encode(text).byteLength <= 4096)
                    void mutate("input", { kind: "insert_text", text });
                }}
              >
                <label htmlFor={`browser-text-${props.threadId}`}>
                  선택한 칸에 입력
                </label>
                <div>
                  <input
                    id={`browser-text-${props.threadId}`}
                    ref={textField}
                    type="password"
                    maxLength={1024}
                    autoComplete="off"
                    placeholder="전송 후 입력값을 지웁니다"
                    disabled={busy || !frame}
                  />
                  <button className="secondary" disabled={busy || !frame}>
                    입력
                  </button>
                </div>
                <small>
                  화면을 선택한 뒤 Enter·Tab·방향키를 사용할 수 있어요. Shift +
                  Tab으로 화면 밖으로 이동합니다. 텍스트는 위 입력란으로
                  보내세요.
                </small>
              </form>
            )}
            {controlling && (
              <div
                className="browser-screen-tools"
                role="group"
                aria-label="브라우저 화면 이동"
              >
                <button
                  className="secondary"
                  disabled={busy || !frame}
                  onClick={() =>
                    void mutate("input", {
                      kind: "scroll",
                      delta_x: 0,
                      delta_y: -500,
                    })
                  }
                  aria-label="브라우저 위로 스크롤"
                >
                  ↑
                </button>
                <button
                  className="secondary"
                  disabled={busy || !frame}
                  onClick={() =>
                    void mutate("input", {
                      kind: "scroll",
                      delta_x: 0,
                      delta_y: 500,
                    })
                  }
                  aria-label="브라우저 아래로 스크롤"
                >
                  ↓
                </button>
                <button
                  className="secondary"
                  disabled={busy || !frame}
                  onClick={() =>
                    void mutate("input", { kind: "key", key: "tab" })
                  }
                >
                  Tab
                </button>
                <button
                  className="secondary"
                  disabled={busy || !frame}
                  onClick={() =>
                    void mutate("input", { kind: "key", key: "enter" })
                  }
                >
                  Enter
                </button>
              </div>
            )}
            {!ended && (
              <p className="browser-help">
                화면을 접거나 탭을 닫아도 대기는 유지됩니다. 조작을 마치면 완료
                버튼으로 원래 요청을 이어가세요.
              </p>
            )}
          </div>
          <div className="browser-actions">
              {resumeUncertain && (
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() => void mutate("resume")}
                >
                  완료 결과 다시 확인
                </button>
              )}
              {canTake && !controlling && !resumeUncertain && (
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() => void mutate("takeover")}
                >
                  직접 조작하기
                </button>
              )}
              {controlling && (
                <button
                  className="primary"
                  disabled={busy || !frame}
                  onClick={() => void mutate("resume")}
                >
                  조작 완료 · 대화 이어가기
                </button>
              )}
              {value && !ended && (
                <button
                  className="text-button"
                  disabled={busy}
                  onClick={() => void mutate("close")}
                >
                  브라우저 종료
                </button>
              )}
          </div>
        </section>
      )}
    </div>
  );
}
