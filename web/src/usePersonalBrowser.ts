import { useEffect, useRef, useState } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { validBrowserFrame, type BrowserInput } from "./browser-frame";
import {
  browserCanWatch, browserEndpoint, type PersonalBrowserControl,
  type PersonalBrowserFrame, type PersonalBrowserStatus, type PersonalBrowserWorkspace,
} from "./personal-browser";

type Action =
  | { kind: "takeover" | "resume" | "close" }
  | { kind: "input"; input: BrowserInput; frame: PersonalBrowserFrame }
  | { kind: "tabs"; action: "switch" | "close"; tab_id: string }
  | { kind: "tabs"; action: "follow"; url: string };
type Identity = { workspace_id: string; revision: number; request_id: string };
type ResumeReceipt = Identity & { control_token: string; frame_token: string };
const interval = 1800;
const controlRefreshMs = 6500;
const proofMaxAgeMs = 30000;
const proofRefreshMarginMs = 5000;
const proofDispatchMarginMs = 1000;
const statusFailure = "브라우저 연결 상태를 확인하지 못했습니다.";
const frameFailure = "브라우저 화면을 불러오지 못했습니다.";
const paused = "조작권이나 화면이 바뀌었습니다. 최신 화면을 확인해 주세요.";
const workerBusy = "브라우저가 화면을 처리하고 있습니다. 잠시 후 다시 시도해 주세요.";
const frameBusy = "브라우저 화면을 준비하는 중입니다.";

function frameMatches(workspace: PersonalBrowserWorkspace | null, frame: PersonalBrowserFrame) {
  return browserCanWatch(workspace) && workspace?.id === frame.workspace_id &&
    workspace.revision === frame.revision && workspace.active_tab_id === frame.tab_id;
}

export function usePersonalBrowser(
  account: string, thread: string | null, watching: boolean,
  onExpired: () => void, onRunChanged: (thread: string) => void,
) {
  const scope = useScope(account, thread);
  const callbacks = useRef({ onExpired, onRunChanged });
  callbacks.current = { onExpired, onRunChanged };
  const watch = useRef(watching);
  watch.current = watching;
  const [view, setView] = useState<{
    thread: string;
    status: PersonalBrowserStatus | null;
    frame: PersonalBrowserFrame | null;
    preview: Pick<PersonalBrowserFrame, "png_base64"> | null;
    controls: boolean;
    canUseFrame: boolean;
    busy: Action["kind"] | null;
    error: string;
    canRetryResume: boolean;
  } | null>(null);
  const driver = useRef<{
    thread: string;
    observe: (freshFrame?: boolean) => void;
    setWatching: (open: boolean) => void;
    mutate: (action: Action) => void;
    publish: (status: PersonalBrowserStatus) => void;
  } | null>(null);

  useEffect(() => {
    if (!thread) return;
    const base = browserEndpoint(thread);
    let live = true;
    let status: PersonalBrowserStatus | null = null;
    let frame: PersonalBrowserFrame | null = null;
    let takingFrame = false;
    let capturedAt = 0;
    let proof: { token: string; expiresAt: number } | null = null;
    let preview: Pick<PersonalBrowserFrame, "png_base64"> | null = null;
    let token: string | null = null;
    let busy: Action["kind"] | null = null;
    let error = "";
    let resume: ResumeReceipt | null = null;
    let seenPreview = "";
    let cardPreviewAllowed = true;
    let watchingEpoch = 0;
    let wasWatching = watch.current;
    let observing: AbortController | null = null;
    let rerun = false;
    let timer: number | undefined;
    let denied = false;
    let pageHidden = false;
    const visible = () => !pageHidden && document.visibilityState === "visible";
    const workspace = () => status?.configured ? status.workspace : null;
    const proofFresh = () => !!frame && !takingFrame && !!proof &&
      proof.expiresAt - performance.now() > proofDispatchMarginMs;

    function render() {
      if (!live) return;
      setView({ thread: thread!, status, frame, preview, controls: !!token && watch.current,
        canUseFrame: proofFresh(),
        busy, error, canRetryResume: !!resume });
    }

    function apply(value: PersonalBrowserStatus, ownControl?: string) {
      const previous = workspace();
      if (value.workspace && value.workspace.thread_id !== thread) throw new Error("wrong_thread");
      if (previous && value.workspace?.id === previous.id && value.workspace.revision < previous.revision) return;
      status = value;
      const next = workspace();
      if (!next || !previous || next.id !== previous.id || next.active_tab_id !== previous.active_tab_id) {
        proof = null;
      }
      if (!next || !previous || next.id !== previous.id || next.revision !== previous.revision ||
          next.active_tab_id !== previous.active_tab_id) {
        frame = null;
        preview = null;
        token = null;
      }
      if (next?.state !== "user_control") token = null;
      else if (ownControl && watch.current && visible()) token = ownControl;
      if (resume && (!next || resume.workspace_id !== next.id ||
          ["resuming", "completed", "stopping", "closed", "outcome_unknown"].includes(next.state))) {
        resume = null;
        if (error.startsWith("완료 결과")) error = "";
      }
      if (next && (!previous || previous.run_id !== next.run_id || previous.state !== next.state)) {
        callbacks.current.onRunChanged(thread!);
      }
      render();
    }

    function handleAccess(failure: unknown, statusRead: boolean) {
      if (!(failure instanceof ApiFailure) || ![401, 403, 404].includes(failure.status)) return false;
      if (!statusRead && failure.status !== 401) return false;
      token = null;
      frame = null;
      preview = null;
      resume = null;
      status = null;
      denied = true;
      if (failure.status === 401) callbacks.current.onExpired();
      if (failure.status === 403) error = "이 세션에서 브라우저에 접근할 수 없습니다.";
      if (failure.status === 404 && watch.current) error = "이 대화의 브라우저를 찾을 수 없습니다.";
      render();
      return true;
    }

    async function observe() {
      window.clearTimeout(timer);
      if (!live || denied || !visible() || busy) return;
      if (observing) {
        rerun = true;
        return;
      }
      const current = new AbortController();
      observing = current;
      let readingFrame = false;
      try {
        const result = await scope.request<PersonalBrowserStatus>(base, "GET", undefined,
          current.signal, { timeoutMs: 12000 });
        if (!live || current.signal.aborted) return;
        if (error === statusFailure) error = "";
        apply(result);
        const value = workspace();
        if (!value || !browserCanWatch(value)) return;
        const previewKey = `${value.id}:${value.revision}:${value.active_tab_id}`;
        const previewable = ["reading", "ready", "completed"].includes(value.state);
        const showPreview = cardPreviewAllowed && previewable && seenPreview !== previewKey;
        const now = performance.now();
        const watchFrame = watch.current && (!token || !frame ||
          now - capturedAt >= controlRefreshMs ||
          !!proof && proof.expiresAt - now <= proofRefreshMarginMs);
        if (!watchFrame && !showPreview) return;
        readingFrame = true;
        takingFrame = true;
        const requestedAt = performance.now();
        render();
        const nextFrame = await scope.request<PersonalBrowserFrame>(`${base}/frame`, "GET", undefined,
          current.signal, { timeoutMs: 12000 });
        if (!live || current.signal.aborted || !visible()) return;
        if (!validBrowserFrame(nextFrame) || !frameMatches(workspace(), nextFrame)) {
          frame = null;
          preview = null;
          error = frameFailure;
          render();
          return;
        }
        if (watch.current) {
          frame = nextFrame;
          capturedAt = requestedAt;
          if (nextFrame.proof_valid_for_ms !== undefined && Number.isFinite(nextFrame.proof_valid_for_ms)) {
            proof = { token: nextFrame.frame_token,
              expiresAt: requestedAt + Math.max(0, Math.min(proofMaxAgeMs, nextFrame.proof_valid_for_ms)) };
          } else if (proof?.token !== nextFrame.frame_token) {
            // Older runtimes can reissue unchanged proofs without extending their life.
            proof = { token: nextFrame.frame_token, expiresAt: requestedAt + proofMaxAgeMs };
          }
        }
        if (showPreview || (watch.current && previewable)) {
          preview = { png_base64: nextFrame.png_base64 };
          seenPreview = previewKey;
        }
        if ([frameBusy, frameFailure].includes(error)) error = "";
        render();
      } catch (failure) {
        if (!live || current.signal.aborted || isAborted(failure) || handleAccess(failure, !readingFrame)) return;
        if (failure instanceof ApiFailure && failure.code === "browser_busy") {
          if (error !== workerBusy) error = frameBusy;
          render();
          return;
        }
        frame = null;
        if (readingFrame) preview = null;
        else token = null;
        error = readingFrame ? frameFailure : statusFailure;
        render();
      } finally {
        if (takingFrame) {
          takingFrame = false;
          render();
        }
        observing = null;
        if (live && !denied && visible() && (status?.configured || rerun || !!error)) {
          timer = window.setTimeout(observe, rerun ? 0 : interval);
          rerun = false;
        }
      }
    }

    async function mutate(action: Action) {
      if (!live || denied || !visible() || !watch.current || busy) return;
      const value = workspace();
      if (!value || ["stopping", "closed"].includes(value.state) ||
          (value.state === "outcome_unknown" && action.kind !== "close")) return;
      const heldToken = token;
      const seenFrame = frame;
      const previousResume = resume;
      const suffix = action.kind;
      let body: Identity & Record<string, unknown> = {
        workspace_id: value.id, revision: value.revision, request_id: crypto.randomUUID(),
      };
      switch (action.kind) {
        case "input":
          if (!heldToken || !proofFresh() || !seenFrame || seenFrame.frame_token !== action.frame.frame_token ||
              !frameMatches(value, seenFrame)) return;
          body = { ...body, control_token: heldToken, frame_token: seenFrame.frame_token, input: action.input };
          break;
        case "resume":
          if (!resume && (!heldToken || !proofFresh() || !seenFrame || !frameMatches(value, seenFrame))) return;
          if (!resume && heldToken && seenFrame) {
            resume = { ...body, control_token: heldToken, frame_token: seenFrame.frame_token };
          }
          body = resume!;
          break;
        case "tabs":
          if (action.action === "follow") body = { ...body, action: action.action, url: action.url };
          else {
            if (!heldToken) return;
            body = { ...body, control_token: heldToken, action: action.action, tab_id: action.tab_id };
          }
          break;
      }
      const epoch = watchingEpoch;
      observing?.abort();
      frame = null;
      busy = suffix;
      error = "";
      render();
      try {
        if (action.kind === "takeover") {
          const result = await scope.request<PersonalBrowserControl>(`${base}/takeover`, "POST", body,
            undefined, { timeoutMs: 45000 });
          if (!live) return;
          if (!result.control_token) throw new Error("missing_controller");
          apply({ configured: true, workspace: result.workspace },
            epoch === watchingEpoch ? result.control_token : undefined);
        } else {
          const result = await scope.request<PersonalBrowserStatus>(`${base}/${suffix}`, "POST", body,
            undefined, { timeoutMs: 90000 });
          if (!live) return;
          if (action.kind === "resume") {
            resume = null;
            token = null;
          }
          apply(result, epoch === watchingEpoch && action.kind !== "resume" && action.kind !== "close"
            ? heldToken ?? undefined : undefined);
        }
      } catch (failure) {
        if (!live || isAborted(failure) || handleAccess(failure, false)) return;
        frame = null;
        if (failure instanceof ApiFailure && failure.code === "browser_busy") {
          if (action.kind === "resume") resume = previousResume;
          error = workerBusy;
          return;
        }
        token = null;
        if (failure instanceof ApiFailure && failure.status < 500) {
          if (action.kind === "resume") resume = null;
          error = failure.code === "browser_budget_exhausted"
            ? "이 작업에서 사용할 수 있는 페이지 읽기 횟수를 모두 사용했습니다."
            : failure.status === 409 && action.kind === "close"
            ? "브라우저에서 처리 중인 동작이 있습니다. 잠시 후 작업 중지를 다시 눌러 주세요."
            : [403, 409].includes(failure.status) ? paused : "브라우저 요청을 처리하지 못했습니다.";
        } else {
          error = action.kind === "resume"
            ? "완료 결과를 확인하지 못했습니다. 같은 요청의 결과를 다시 확인할 수 있습니다."
            : "브라우저 요청 결과를 확인하지 못했습니다. 최신 화면을 다시 확인해 주세요.";
        }
      } finally {
        if (live) {
          busy = null;
          render();
          void observe();
        }
      }
    }

    function changeVisibility() {
      watchingEpoch++;
      observing?.abort();
      token = null;
      frame = null;
      proof = null;
      preview = null;
      seenPreview = "";
      render();
      if (visible()) void observe();
      else window.clearTimeout(timer);
    }
    function focus() {
      if (visible()) void observe();
    }
    function pageHide() {
      pageHidden = true;
      changeVisibility();
    }
    function pageShow() {
      pageHidden = false;
      changeVisibility();
    }
    const cleanup = scope.onClose(() => {
      live = false;
      observing?.abort();
      window.clearTimeout(timer);
      resume = null;
      token = null;
      frame = null;
      proof = null;
      preview = null;
      status = null;
      setView(null);
    });
    driver.current = {
      thread,
      observe: (freshFrame = false) => {
        if (freshFrame) {
          denied = false;
          frame = null;
          seenPreview = "";
          if ([statusFailure, frameFailure, frameBusy, workerBusy, paused].includes(error)) error = "";
          render();
        }
        void observe();
      },
      setWatching: (open) => {
        if (open === wasWatching) return;
        wasWatching = open;
        watchingEpoch++;
        cardPreviewAllowed = open;
        if (!open) {
          observing?.abort();
          frame = null;
          token = null;
        }
        render();
        void observe();
      },
      mutate: (action) => { void mutate(action); },
      publish: (next) => {
        if (!live) return;
        denied = false;
        observing?.abort();
        apply(next);
        void observe();
      },
    };
    document.addEventListener("visibilitychange", changeVisibility);
    window.addEventListener("focus", focus);
    window.addEventListener("pagehide", pageHide);
    window.addEventListener("pageshow", pageShow);
    void observe();
    return () => {
      live = false;
      observing?.abort();
      window.clearTimeout(timer);
      driver.current = null;
      cleanup();
      document.removeEventListener("visibilitychange", changeVisibility);
      window.removeEventListener("focus", focus);
      window.removeEventListener("pagehide", pageHide);
      window.removeEventListener("pageshow", pageShow);
    };
  }, [scope, thread]);

  useEffect(() => {
    driver.current?.setWatching(watching);
  }, [watching, thread]);

  const current = view && view.thread === thread ? view : null;
  return {
    status: current?.status ?? null,
    frame: watching ? current?.frame ?? null : null,
    preview: current?.preview ?? null,
    controls: watching && !!current?.controls,
    canUseFrame: watching && !!current?.canUseFrame,
    busy: current?.busy ?? null,
    error: current?.error ?? "",
    canRetryResume: !!current?.canRetryResume,
    refresh: () => driver.current?.observe(true),
    mutate: (action: Action) => driver.current?.mutate(action),
    publish: (target: string, status: PersonalBrowserStatus) => {
      if (driver.current?.thread === target) driver.current.publish(status);
    },
  };
}

export type PersonalBrowser = ReturnType<typeof usePersonalBrowser>;
