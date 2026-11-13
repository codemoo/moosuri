import { WebEvidence } from "./WebEvidence";
import { BrowserWorkspace } from "./BrowserWorkspace";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import type {
  ChatEvidence,
  ChatEvent,
  ChatPage,
  ChatTurn,
  ModelSettings,
  RunUsage,
  RunView,
} from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";
import {
  acknowledge,
  active,
  applyEvents,
  parseEvent,
  parsePage,
  validChoice,
  validTurn,
} from "./chat-state";
import { StreamFailure } from "./event-stream";
import "./chat.css";

export type PendingSend = {
  request_id: string;
  text: string;
  status: "sending" | "accepted" | "uncertain";
};
export type ChatMemory = {
  drafts: Record<string, string>;
  pending: Record<string, PendingSend>;
};
type Props = {
  accountId: string;
  name: string;
  threadId: string | null;
  memory: ChatMemory;
  onDraft: (slot: string, text: string) => void;
  onPending: (
    slot: string,
    value: PendingSend | null,
    expectedId?: string,
  ) => void;
  onRestoreDraft: (slot: string, text: string) => void;
  onCreated: (threadId: string, requestId: string) => void;
  onSuspend: () => void;
  createThread: () => Promise<string>;
  onExpired: () => void;
  onThreadMissing: (id: string) => void;
  onThreadUpdated: () => void;
  openSettings: () => void;
  onOpenEvidence?: (reference: ChatEvidence) => void;
};
const labels: Record<RunView["state"], string> = {
  queued: "대기 중",
  running: "답변 중",
  completed: "완료",
  failed: "응답 실패",
  cancelled: "중지됨",
  incomplete: "미완료",
  refused: "답변 제한",
  outcome_unknown: "결과 확인 필요",
};
function runExplanation(run: RunView) {
  if (run.error === "knowledge_invalidated")
    return "참고한 자료가 변경되거나 삭제되어 관련 답변을 지웠습니다. 현재 자료로 다시 질문해 주세요.";
  if (run.error === "budget_exhausted")
    return "이번 요청의 실행 한도에 도달했습니다. 범위를 줄여 새로 요청해 주세요.";
  if (run.state === "outcome_unknown")
    return "이전 요청의 결과를 확인할 수 없습니다. 서버가 다시 시작되었을 수 있으며, 제공업체에서 사용량이 청구될 수 있습니다. 자동으로 다시 전송하지 않습니다.";
  if (run.state === "cancelled")
    return "중지했습니다. 이미 전송된 요청의 사용량은 청구될 수 있습니다.";
  if (run.state === "incomplete") return "응답이 끝까지 완료되지 않았습니다.";
  if (run.state === "refused") return "모델이 답변을 제공하지 않았습니다.";
  if (run.state === "failed") {
    if (run.error === "timed_out")
      return "모델 응답을 기다리는 시간이 초과되었습니다.";
    if (run.error === "credentials_rejected")
      return "모델 연결 인증에 실패했습니다. 관리자에게 연결 확인을 요청하세요.";
    if (run.error === "protocol_failed")
      return "모델의 응답 형식을 확인하지 못했습니다.";
    if (run.error === "rate_limited")
      return "모델 제공업체의 요청 한도에 도달했습니다. 잠시 뒤 다시 시도해 주세요.";
    if (run.error === "connection_failed")
      return "모델 제공업체에 연결하지 못했습니다.";
    return "응답을 완료하지 못했습니다. 아래에서 같은 설정으로 다시 시도할 수 있습니다.";
  }
  return "";
}
function chatError(error: unknown) {
  if (error instanceof ApiFailure) {
    if (error.status === 409)
      return "대화 상태나 모델 설정이 변경되었습니다. 기록과 내 모델 설정을 다시 확인해 주세요.";
    if (error.status === 429)
      return "요청이 많습니다. 잠시 뒤 다시 시도해 주세요.";
    if (error.status === 400)
      return "메시지를 접수하지 못했습니다. 입력과 모델 설정을 확인해 주세요.";
  }
  return "서버 응답을 확인하지 못했습니다. 기록을 다시 확인해 주세요.";
}
export function usageLabel(usage: RunUsage) {
  if (usage.input_tokens === null && usage.output_tokens === null)
    return "사용량 미보고";
  return `입력 ${usage.input_tokens === null ? "미보고" : usage.input_tokens.toLocaleString()} · 출력 ${usage.output_tokens === null ? "미보고" : usage.output_tokens.toLocaleString()}`;
}

export function ChatPanel(props: Props) {
  const { accountId, threadId, memory } = props;
  const callbacks = useRef(props);
  callbacks.current = props;
  const currentThread = useRef(threadId);
  currentThread.current = threadId;
  const readScope = useScope(accountId, threadId);
  const sendScope = useScope(accountId);
  const slot = threadId ?? "new";
  const draft = memory.drafts[slot] ?? "";
  const pending = memory.pending[slot];
  const pendingRef = useRef(memory.pending);
  pendingRef.current = memory.pending;
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  const turnsRef = useRef<ChatTurn[]>([]);
  const historyEpoch = useRef(0);
  const [loading, setLoading] = useState(!!threadId);
  const [historyReady, setHistoryReady] = useState(!threadId);
  const displayedThread = useRef<string | null | undefined>(undefined);
  const [error, setError] = useState("");
  const [connection, setConnection] = useState("");
  const [refresh, setRefresh] = useState(0);
  const [nextBefore, setNextBefore] = useState<number | null>(null);
  const [olderBusy, setOlderBusy] = useState(false);
  const olderPending = useRef(false);
  const olderController = useRef<AbortController | null>(null);
  const [model, setModel] = useState<ModelSettings | null>(null);
  const [modelLoading, setModelLoading] = useState(true);
  const [modelRefresh, setModelRefresh] = useState(0);
  const [modelError, setModelError] = useState("");
  const [actionBusy, setActionBusy] = useState<string | null>(null);
  const actionLock = useRef(false);
  const posts = useRef(new Map<string, AbortController>());
  const scroll = useRef<HTMLDivElement>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const nearBottom = useRef(true);
  const [unseen, setUnseen] = useState(false);
  const composing = useRef(false);
  const anchor = useRef<{
    id: string | null;
    top: number;
    height: number;
    scrollTop: number;
  } | null>(null);
  const waiters = useRef<
    Array<{ thread: string; resolve: (page: ChatPage | null) => void }>
  >([]);
  const flushRef = useRef<() => void>(() => {});

  function settle(page: ChatPage) {
    for (const turn of page.turns)
      callbacks.current.onPending(page.thread_id, null, turn.run.request_id);
  }
  function resolveWaiters(thread: string, page: ChatPage | null) {
    for (const waiter of waiters.current.filter(
      (waiter) => waiter.thread === thread,
    ))
      waiter.resolve(page);
    waiters.current = waiters.current.filter(
      (waiter) => waiter.thread !== thread,
    );
  }
  function refreshSnapshot(): Promise<ChatPage | null> {
    const thread = currentThread.current;
    if (!thread) return Promise.resolve(null);
    return new Promise((resolve) => {
      waiters.current.push({ thread, resolve });
      setRefresh((value) => value + 1);
    });
  }
  function boundary(error: unknown, thread?: string) {
    if (error instanceof ApiFailure && error.status === 401) {
      callbacks.current.onExpired();
      return true;
    }
    if (
      thread &&
      error instanceof ApiFailure &&
      (error.status === 403 || error.status === 404)
    ) {
      callbacks.current.onThreadMissing(thread);
      return true;
    }
    return false;
  }

  useEffect(() => {
    const controller = new AbortController();
    setModelLoading(true);
    setModelError("");
    void sendScope
      .request<ModelSettings>(
        "/settings/model",
        "GET",
        undefined,
        controller.signal,
      )
      .then((value) => {
        if (
          !value ||
          (value.effective !== null && !validChoice(value.effective))
        )
          throw new StreamFailure();
        setModel(value);
        setModelLoading(false);
      })
      .catch((error) => {
        if (!isAborted(error)) {
          boundary(error);
          setModelError("모델 설정을 확인하지 못했습니다.");
          setModelLoading(false);
        }
      });
    return () => controller.abort();
  }, [sendScope, modelRefresh]);

  useEffect(() => {
    const check = () => {
      if (document.visibilityState !== "visible") return;
      setModelRefresh((value) => value + 1);
      setRefresh((value) => value + 1);
    };
    window.addEventListener("focus", check);
    window.addEventListener("online", check);
    document.addEventListener("visibilitychange", check);
    return () => {
      window.removeEventListener("focus", check);
      window.removeEventListener("online", check);
      document.removeEventListener("visibilitychange", check);
    };
  }, []);
  useEffect(
    () => () => {
      if (threadId) {
        posts.current.get(threadId)?.abort();
        resolveWaiters(threadId, null);
      }
    },
    [threadId],
  );
  useEffect(
    () => () => {
      for (const controller of posts.current.values()) controller.abort();
      olderController.current?.abort();
      for (const waiter of waiters.current) waiter.resolve(null);
      waiters.current = [];
      callbacks.current.onSuspend();
    },
    [],
  );

  useEffect(() => {
    let live = true;
    let cursor = 0;
    let queue: ChatEvent[] = [];
    let queuedBytes = 0;
    let frame: number | null = null;
    let usingRAF = false;
    let connectionController: AbortController | null = null;
    let wake: (() => void) | null = null;
    let needsSnapshot = false;
    let stopped = false;
    const controller = new AbortController();
    const thread = threadId;
    const changed = displayedThread.current !== thread;
    displayedThread.current = thread;
    if (changed) {
      historyEpoch.current++;
      nearBottom.current = true;
      anchor.current = null;
      setUnseen(false);
      turnsRef.current = [];
      setTurns([]);
      setNextBefore(null);
    }
    setError("");
    setConnection("");
    setLoading(!!thread);
    setHistoryReady(!thread);
    function cancelFrame() {
      if (frame !== null) {
        if (usingRAF) window.cancelAnimationFrame(frame);
        else window.clearTimeout(frame);
        frame = null;
      }
    }
    function flush() {
      cancelFrame();
      if (!live || !queue.length || !thread) return;
      const events = queue;
      queue = [];
      queuedBytes = 0;
      const result = applyEvents(turnsRef.current, events, thread);
      turnsRef.current = result.turns;
      setTurns(result.turns);
      if (!nearBottom.current) setUnseen(true);
      for (const event of events) {
        if (event.change.kind === "turn")
          callbacks.current.onPending(
            thread,
            null,
            event.change.turn.run.request_id,
          );
      }
      if (
        events.some(
          (event) =>
            event.change.kind === "turn" ||
            (event.change.kind === "state" && !active(event.change.run)),
        )
      )
        callbacks.current.onThreadUpdated();
      if (result.needsSnapshot) {
        needsSnapshot = true;
        connectionController?.abort();
      }
    }
    flushRef.current = flush;
    function schedule() {
      if (frame !== null) return;
      usingRAF =
        typeof window.requestAnimationFrame === "function" &&
        document.visibilityState === "visible";
      frame = usingRAF
        ? window.requestAnimationFrame(flush)
        : window.setTimeout(flush, 20);
    }
    async function snapshot() {
      if (!thread) return false;
      // A reconnect can miss the reset event while offline. A fresh cursor may
      // already be beyond that reset, so no older cached page can be retained.
      historyEpoch.current++;
      olderController.current?.abort();
      cancelFrame();
      queue = [];
      queuedBytes = 0;
      turnsRef.current = [];
      setTurns([]);
      setNextBefore(null);
      setHistoryReady(false);
      try {
        const page = parsePage(
          await readScope.request<unknown>(
            `/threads/${encodeURIComponent(thread)}/messages`,
            "GET",
            undefined,
            controller.signal,
          ),
          thread,
        );
        if (!live) return false;
        cancelFrame();
        queue = [];
        cursor = page.cursor;
        turnsRef.current = page.turns;
        setTurns(turnsRef.current);
        setNextBefore(page.next_before);
        setLoading(false);
        setHistoryReady(true);
        settle(page);
        resolveWaiters(thread, page);
        const unresolved = pendingRef.current[thread];
        if (
          unresolved?.status === "sending" &&
          !posts.current.has(thread) &&
          !page.turns.some(
            (turn) => turn.run.request_id === unresolved.request_id,
          )
        )
          callbacks.current.onPending(
            thread,
            { ...unresolved, status: "uncertain" },
            unresolved.request_id,
          );
        return true;
      } catch (error) {
        if (!live || isAborted(error)) return false;
        boundary(error, thread);
        setError("대화 기록을 불러오지 못했습니다. 새로고침해 주세요.");
        setLoading(false);
        resolveWaiters(thread, null);
        return false;
      }
    }
    async function delay(ms: number) {
      await new Promise<void>((resolve) => {
        const id = window.setTimeout(() => {
          wake = null;
          resolve();
        }, ms);
        wake = () => {
          window.clearTimeout(id);
          resolve();
        };
      });
    }
    void (async () => {
      if (!thread || !(await snapshot())) return;
      for (let attempt = 0; live && !stopped && attempt < 6; attempt++) {
        needsSnapshot = false;
        connectionController = new AbortController();
        setConnection(attempt === 0 ? "연결 중…" : "연결을 다시 확인하는 중…");
        try {
          await readScope.stream(
            `/threads/${encodeURIComponent(thread)}/events?after=${cursor}`,
            (incoming) => {
              if (!live) return;
              let data: unknown;
              try {
                data = JSON.parse(incoming.data);
              } catch {
                throw new StreamFailure();
              }
              if (incoming.event === "reset") {
                historyEpoch.current++;
                olderController.current?.abort();
                cancelFrame();
                queue = [];
                queuedBytes = 0;
                turnsRef.current = [];
                setTurns([]);
                setNextBefore(null);
                setHistoryReady(false);
                needsSnapshot = true;
                throw new StreamFailure();
              }
              if (incoming.event === "closed") {
                if (!data || typeof data !== "object" || !("code" in data))
                  throw new StreamFailure();
                const code = data.code;
                if (code === "unauthorized") callbacks.current.onExpired();
                else if (code === "forbidden" || code === "not_found")
                  callbacks.current.onThreadMissing(thread);
                else if (code === "unavailable")
                  setError(
                    "대화를 지금 사용할 수 없습니다. 기록을 새로고침해 주세요.",
                  );
                else throw new StreamFailure();
                stopped = true;
                connectionController?.abort();
                return;
              }
              if (incoming.event !== "change") throw new StreamFailure();
              const event = parseEvent(data, incoming.id);
              if (
                (event.change.kind === "turn" &&
                  event.change.turn.run.thread_id !== thread) ||
                (event.change.kind === "state" &&
                  event.change.run.thread_id !== thread)
              )
                throw new StreamFailure();
              if (event.sequence <= cursor) return;
              if (event.sequence !== cursor + 1) {
                needsSnapshot = true;
                throw new StreamFailure();
              }
              queuedBytes += incoming.data.length;
              if (queue.length >= 1024 || queuedBytes > 1024 * 1024) {
                needsSnapshot = true;
                throw new StreamFailure();
              }
              cursor = event.sequence;
              queue.push(event);
              setConnection("");
              schedule();
            },
            connectionController.signal,
            () => { if (live) setConnection(""); },
          );
        } catch (error) {
          if (!live || stopped) return;
          if (boundary(error, thread)) return;
          if (error instanceof StreamFailure) needsSnapshot = true;
        }
        flush();
        if (!live || stopped) return;
        if (needsSnapshot && !(await snapshot())) return;
        setConnection(
          "연결이 끊겼습니다. 전송을 다시 실행하지 않고 기록만 확인합니다.",
        );
        await delay(Math.min(500 * 2 ** attempt, 8000));
      }
      if (live && !stopped)
        setConnection(
          "자동 재연결을 멈췄습니다. 기록 새로고침으로 다시 연결하세요.",
        );
    })();
    return () => {
      flush();
      live = false;
      controller.abort();
      connectionController?.abort();
      olderController.current?.abort();
      cancelFrame();
      wake?.();
      queue = [];
      flushRef.current = () => {};
    };
  }, [threadId, readScope, refresh]);

  useLayoutEffect(() => {
    const element = scroll.current;
    if (!element) return;
    if (anchor.current) {
      const saved = anchor.current;
      const target = saved.id
        ? [...element.querySelectorAll<HTMLElement>("[data-run-id]")].find(
            (item) => item.dataset.runId === saved.id,
          )
        : null;
      element.scrollTop = target
        ? element.scrollTop + target.getBoundingClientRect().top - saved.top
        : saved.scrollTop + element.scrollHeight - saved.height;
      anchor.current = null;
    } else if (nearBottom.current) element.scrollTop = element.scrollHeight;
  }, [turns, pending]);
  useLayoutEffect(() => {
    const element = textarea.current;
    if (element) {
      element.style.height = "auto";
      element.style.height = `${Math.min(180, Math.max(48, element.scrollHeight))}px`;
    }
  }, [draft]);

  async function older() {
    if (!threadId || !nextBefore || olderPending.current) return;
    olderPending.current = true;
    setOlderBusy(true);
    const thread = threadId;
    const epoch = historyEpoch.current;
    const controller = new AbortController();
    olderController.current = controller;
    const element = scroll.current;
    const first = element
      ? [...element.querySelectorAll<HTMLElement>("[data-run-id]")].find(
          (item) =>
            item.getBoundingClientRect().bottom >=
            element.getBoundingClientRect().top,
        )
      : null;
    const saved = element
      ? {
          id: first?.dataset.runId ?? null,
          top: first?.getBoundingClientRect().top ?? 0,
          height: element.scrollHeight,
          scrollTop: element.scrollTop,
        }
      : null;
    try {
      const page = parsePage(
        await readScope.request<unknown>(
          `/threads/${encodeURIComponent(thread)}/messages?before=${nextBefore}`,
          "GET",
          undefined,
          controller.signal,
        ),
        thread,
      );
      if (epoch !== historyEpoch.current || controller.signal.aborted) return;
      flushRef.current();
      const existing = new Map(
        turnsRef.current.map((turn) => [turn.run.id, turn]),
      );
      for (const turn of page.turns)
        if (!existing.has(turn.run.id)) existing.set(turn.run.id, turn);
      anchor.current = saved;
      nearBottom.current = false;
      turnsRef.current = [...existing.values()].sort(
        (a, b) => a.sequence - b.sequence,
      );
      setTurns(turnsRef.current);
      setNextBefore(page.next_before);
    } catch (error) {
      if (!isAborted(error) && !boundary(error, thread))
        setError("이전 대화를 불러오지 못했습니다.");
    } finally {
      olderPending.current = false;
      setOlderBusy(false);
    }
  }
  async function reconcile(thread: string, requestId: string) {
    const page =
      currentThread.current === thread
        ? await refreshSnapshot()
        : await sendScope
            .request<unknown>(`/threads/${encodeURIComponent(thread)}/messages`)
            .then((value) => parsePage(value, thread))
            .catch(() => null);
    const known = page?.turns.some((turn) => turn.run.request_id === requestId);
    if (known) callbacks.current.onPending(thread, null, requestId);
    else {
      const current = pendingRef.current[thread];
      if (current?.request_id === requestId)
        callbacks.current.onPending(
          thread,
          { ...current, status: "uncertain" },
          requestId,
        );
    }
  }
  async function post(thread: string, value: PendingSend) {
    const controller = new AbortController();
    posts.current.set(thread, controller);
    try {
      const result = await sendScope.request<unknown>(
        `/threads/${encodeURIComponent(thread)}/messages`,
        "POST",
        { request_id: value.request_id, text: value.text },
        controller.signal,
      );
      if (
        !validTurn(result) ||
        result.run.thread_id !== thread ||
        result.run.request_id !== value.request_id
      )
        throw new StreamFailure();
      callbacks.current.onPending(
        thread,
        { ...value, status: "accepted" },
        value.request_id,
      );
      // A duplicate POST may return a running turn containing text whose SSE
      // deltas are still queued. Its body has no cursor, so never merge it with
      // that stream. Reload the committed page and resume at its exact cursor.
      if (currentThread.current === thread) {
        const page = await refreshSnapshot();
        if (page) callbacks.current.onPending(thread, null, value.request_id);
      }
      callbacks.current.onThreadUpdated();
    } catch (error) {
      if (isAborted(error)) {
        callbacks.current.onPending(
          thread,
          { ...value, status: "uncertain" },
          value.request_id,
        );
        return;
      }
      if (boundary(error, thread)) return;
      if (error instanceof ApiFailure && error.status < 500) {
        callbacks.current.onPending(thread, null, value.request_id);
        callbacks.current.onRestoreDraft(thread, value.text);
        if (currentThread.current === thread) {
          setError(chatError(error));
          void refreshSnapshot();
          setModelRefresh((value) => value + 1);
        }
      } else {
        callbacks.current.onPending(
          thread,
          { ...value, status: "uncertain" },
          value.request_id,
        );
        if (currentThread.current === thread)
          setError(
            "전송 접수를 확인하지 못했습니다. 기록을 먼저 확인하며, 자동으로 다시 전송하지 않습니다.",
          );
        await reconcile(thread, value.request_id);
      }
    } finally {
      if (posts.current.get(thread) === controller)
        posts.current.delete(thread);
    }
  }
  async function send(resubmit?: PendingSend) {
    flushRef.current();
    const originalSlot = currentThread.current ?? "new";
    const text =
      resubmit?.text ?? callbacks.current.memory.drafts[originalSlot] ?? "";
    if (
      posts.current.has(originalSlot) ||
      !text.trim() ||
      new TextEncoder().encode(text).byteLength > 8192 ||
      (!resubmit &&
        (pendingRef.current[originalSlot] ||
          turnsRef.current.some((turn) => active(turn.run)))) ||
      loading ||
      !historyReady ||
      !model?.effective ||
      modelLoading
    )
      return;
    setError("");
    const value: PendingSend = {
      request_id: resubmit?.request_id ?? crypto.randomUUID(),
      text,
      status: "sending",
    };
    callbacks.current.onPending(originalSlot, value, resubmit?.request_id);
    if (!resubmit) callbacks.current.onDraft(originalSlot, "");
    let thread = currentThread.current;
    if (!thread) {
      const guard = new AbortController();
      posts.current.set("new", guard);
      try {
        thread = await callbacks.current.createThread();
        callbacks.current.onCreated(thread, value.request_id);
      } catch (error) {
        callbacks.current.onPending("new", null, value.request_id);
        callbacks.current.onRestoreDraft("new", value.text);
        if (!isAborted(error) && !boundary(error))
          setError(
            "새 대화 생성 결과를 확인하지 못했습니다. 메시지는 아직 전송하지 않았습니다. 대화 목록을 새로고침해 주세요.",
          );
        return;
      } finally {
        posts.current.delete("new");
      }
    }
    await post(thread, value);
  }
  async function action(run: RunView, kind: "cancel" | "retry") {
    flushRef.current();
    const current = turnsRef.current.find(
      (turn) => turn.run.id === run.id,
    )?.run;
    if (
      actionLock.current ||
      !current ||
      current.attempt !== run.attempt ||
      !(kind === "cancel" ? current.can_cancel : current.can_retry)
    )
      return;
    actionLock.current = true;
    const epoch = historyEpoch.current;
    setActionBusy(run.id);
    setError("");
    try {
      const result = await readScope.request<unknown>(
        `/runs/${encodeURIComponent(run.id)}/${kind}`,
        "POST",
        { expected_attempt: run.attempt },
      );
      if (
        !validTurn(result) ||
        result.run.id !== run.id ||
        result.run.thread_id !== currentThread.current
      )
        throw new StreamFailure();
      if (epoch !== historyEpoch.current) {
        void refreshSnapshot();
        return;
      }
      flushRef.current();
      turnsRef.current = acknowledge(turnsRef.current, result);
      setTurns(turnsRef.current);
      callbacks.current.onThreadUpdated();
    } catch (error) {
      if (!isAborted(error) && !boundary(error, run.thread_id)) {
        setError(
          "요청 결과를 확인하지 못했습니다. 자동으로 다시 실행하지 않고 기록만 확인합니다.",
        );
        void refreshSnapshot();
      }
    } finally {
      actionLock.current = false;
      setActionBusy(null);
    }
  }
  const bytes = new TextEncoder().encode(draft).byteLength;
  const hasActive = turns.some((turn) => active(turn.run));
  const canSend =
    !!draft.trim() &&
    bytes <= 8192 &&
    !!model?.effective &&
    !modelLoading &&
    !loading &&
    historyReady &&
    !hasActive &&
    !pending;
  const optimistic =
    pending &&
    !turns.some((turn) => turn.run.request_id === pending.request_id);
  return (
    <section className="chat-panel" aria-label="대화">
      <div className="chat-status-row">
        {model?.effective ? (
          <span>
            {model.effective.model} · {model.effective.effort ?? "기본 강도"}
          </span>
        ) : (
          <button className="text-button" onClick={props.openSettings}>
            내 모델 설정
          </button>
        )}
        <button
          className="text-button"
          onClick={() => {
            void refreshSnapshot();
            setModelRefresh((value) => value + 1);
          }}
        >
          기록 새로고침
        </button>
      </div>
      <BrowserWorkspace
        accountId={accountId}
        threadId={threadId}
        onExpired={props.onExpired}
        onChanged={props.onThreadUpdated}
      />
      {error && (
        <p className="notice chat-notice" role="alert">
          {error}
        </p>
      )}
      <div
        className="chat-scroll"
        ref={scroll}
        onScroll={() => {
          const element = scroll.current;
          if (element) {
            nearBottom.current =
              element.scrollHeight - element.scrollTop - element.clientHeight <
              96;
            if (nearBottom.current) setUnseen(false);
          }
        }}
      >
        {nextBefore && (
          <button
            className="text-button older-button"
            disabled={olderBusy}
            onClick={older}
          >
            {olderBusy ? "이전 대화 불러오는 중…" : "이전 대화 보기"}
          </button>
        )}
        {loading && (
          <p className="chat-loading" role="status">
            대화 기록을 불러오는 중…
          </p>
        )}
        {!loading && turns.length === 0 && !optimistic && (
          <div className="chat-welcome">
            <span className="eyebrow">생각을 이어 가는 공간</span>
            <h2>Hello, {props.name.split(" ")[0]}.</h2>
            <p>궁금한 점이나 함께 생각할 일을 적어 주세요.</p>
            {!modelLoading && !model?.effective && (
              <div className="chat-model-missing">
                <strong>사용할 모델을 먼저 선택해 주세요</strong>
                <p>
                  관리자가 확인한 모델이나 설치 기본값을 내 모델 설정에서 선택할
                  수 있습니다.
                </p>
                <button className="text-button" onClick={props.openSettings}>
                  내 모델 설정 열기 ↗
                </button>
              </div>
            )}
          </div>
        )}
        <div className="chat-turns">
          {turns.map((turn) => (
            <article
              className="chat-turn"
              key={turn.run.id}
              data-run-id={turn.run.id}
            >
              <div className="user-message">
                <span className="sr-only">나</span>
                <p>{turn.user_text}</p>
              </div>
              <div className="assistant-reply">
                {turn.assistant
                  .filter((message) => message.text)
                  .map((message) => (
                    <div
                      className={`assistant-message ${message.phase === "commentary" ? "commentary-message" : ""}`}
                      key={`${turn.run.attempt}:${message.id}`}
                    >
                      {message.phase && (
                        <span className="message-phase">
                          {message.phase === "commentary"
                            ? "진행 설명"
                            : "답변"}
                        </span>
                      )}
                      {message.refusal && (
                        <span className="message-phase">답변 제한</span>
                      )}
                      <p>{message.text}</p>
                    </div>
                  ))}
                {active(turn.run) &&
                  !turn.assistant.some((message) => message.text) && (
                    <p className="run-waiting" role="status">
                      {labels[turn.run.state]}…
                    </p>
                  )}
                <div className="run-footer">
                  <span>{labels[turn.run.state]}</span>
                  {turn.run.can_cancel && (
                    <button
                      className="text-button"
                      disabled={actionBusy !== null}
                      onClick={() => action(turn.run, "cancel")}
                    >
                      {actionBusy === turn.run.id ? "처리 중…" : "응답 중지"}
                    </button>
                  )}
                  {turn.run.can_retry && (
                    <button
                      className="text-button"
                      disabled={actionBusy !== null}
                      onClick={() => action(turn.run, "retry")}
                    >
                      {actionBusy === turn.run.id
                        ? "처리 중…"
                        : "같은 설정으로 다시 시도"}
                    </button>
                  )}
                </div>
                {runExplanation(turn.run) && (
                  <p className="run-explanation">{runExplanation(turn.run)}</p>
                )}
                {turn.assistant.some((message) => message.text) && turn.run.evidence.length > 0 && (
                  <div className="chat-evidence" aria-label="답변에 제공한 자료">
                    <span>확인한 자료</span>
                    {turn.run.evidence.map((reference) => (
                      <button
                        className="text-button"
                        key={`${reference.kind}:${reference.id}:${reference.revision}`}
                        onClick={() => props.onOpenEvidence?.(reference)}
                      >
                        {reference.title} ↗
                      </button>
                    ))}
                  </div>
                )}
                <WebEvidence observations={turn.run.web_evidence}/>
                <details className="run-model">
                  <summary>실행 정보</summary>
                  <p>
                    {turn.run.model.provider_name} · {turn.run.model.model} ·{" "}
                    {turn.run.model.effort ?? "기본 강도"}
                  </p>
                  <p>{usageLabel(turn.run.usage)}</p>
                </details>
              </div>
            </article>
          ))}
          {optimistic && (
            <article className="chat-turn optimistic-turn">
              <div className="user-message">
                <span className="sr-only">나</span>
                <p>{pending.text}</p>
              </div>
              <p className="run-waiting">
                {pending.status === "uncertain"
                  ? "전송 접수 확인 필요"
                  : pending.status === "accepted"
                    ? "접수됨 · 기록 확인 중…"
                    : "전송 접수 중…"}
              </p>
            </article>
          )}
        </div>
      </div>
      {unseen && (
        <button
          className="new-response-button"
          onClick={() => {
            nearBottom.current = true;
            setUnseen(false);
            if (scroll.current)
              scroll.current.scrollTop = scroll.current.scrollHeight;
          }}
        >
          새 응답 보기 ↓
        </button>
      )}
      {connection && (
        <p className="chat-connection" role="status">
          {connection}
        </p>
      )}
      {pending?.status === "uncertain" && (
        <div className="uncertain-send">
          <p>
            처음 요청의 접수를 확인하지 못했습니다. 다시 제출할 때도 같은 요청
            ID를 사용합니다.
          </p>
          <button
            className="secondary"
            disabled={
              loading ||
              hasActive ||
              !model?.effective ||
              posts.current.has(slot)
            }
            onClick={() => send(pending)}
          >
            같은 요청 다시 제출
          </button>
        </div>
      )}
      <footer className="chat-composer-area">
        {modelError && (
          <p className="composer-help">
            {modelError}{" "}
            <button className="text-button" onClick={props.openSettings}>
              설정 확인
            </button>
          </p>
        )}
        <form
          className="chat-composer"
          onSubmit={(event: FormEvent) => {
            event.preventDefault();
            void send();
          }}
        >
          <label className="sr-only" htmlFor="chat-message">
            메시지
          </label>
          <textarea
            id="chat-message"
            ref={textarea}
            rows={2}
            value={draft}
            placeholder="메시지를 입력하세요…"
            onChange={(event) => props.onDraft(slot, event.target.value)}
            onCompositionStart={() => {
              composing.current = true;
            }}
            onCompositionEnd={() => {
              composing.current = false;
            }}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.nativeEvent.isComposing &&
                !composing.current &&
                event.nativeEvent.keyCode !== 229
              ) {
                event.preventDefault();
                if (canSend) void send();
              }
            }}
            aria-describedby={bytes > 8192 ? "chat-composer-help" : undefined}
          />
          <div className="chat-composer-bottom">
            <span>
              {hasActive
                ? "응답 중에도 다음 초안을 작성할 수 있습니다"
                : modelLoading
                  ? "모델 확인 중…"
                  : !model?.effective
                    ? "내 모델 설정에서 사용할 모델을 선택하세요"
                    : "Enter로 전송 · Shift+Enter로 줄바꿈"}
            </span>
            <button
              className="primary"
              type="submit"
              aria-label="메시지 전송"
              disabled={!canSend}
            >
              ↑
            </button>
          </div>
        </form>
        {bytes > 8192 && (
          <p id="chat-composer-help" className="composer-help input-limit">
            메시지는 UTF-8 기준 8,192바이트까지 전송할 수 있습니다.
          </p>
        )}
      </footer>
    </section>
  );
}
