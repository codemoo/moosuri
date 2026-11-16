import { useEffect, useMemo, useRef, useState } from "react";
import type { ChatTurn, RunInstructionReceipt, RunInstructionTarget, MissionView } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { emptyInstructions, instructionBytes, restoreInstruction, sameInstruction, validInstructionReceipt, type InstructionMemory, type PendingInstruction } from "./run-instructions";

type Props = {
  accountId: string; threadId: string | null; turns: ChatTurn[]; state?: InstructionMemory; ready: boolean;
  explicitTarget?: RunInstructionTarget; receipts?: RunInstructionReceipt[]; showModes?: boolean;
  update?: (change: (current: InstructionMemory) => InstructionMemory) => void; onExpired: () => void;
};
export function useRunInstructions(props: Props) {
  const scope = useScope(props.accountId, props.threadId);
  const callbacks = useRef(props); callbacks.current = props;
  const owner = useMemo(() => ({ active: true }), [scope]);
  const busy = useRef(false);
  const transport = useRef<{ request_id: string; controller: AbortController } | null>(null);
  const [working, setWorking] = useState(false), [error, setError] = useState(""), [missing, setMissing] = useState(false);
  const [poll, setPoll] = useState(0);
  const [local, setLocal] = useState<{ owner: typeof owner; values: RunInstructionReceipt[] }>({ owner, values: [] });
  const state = props.state ?? emptyInstructions();
  const supported = [...props.turns].reverse().find(turn => turn.run.thread_id === props.threadId && turn.run.supports_instructions === true &&
    (turn.run.state === "queued" || turn.run.state === "running" || turn.run.instruction_target?.supports_instructions === true))?.run;
  const supplied = props.explicitTarget?.supports_instructions === true ? props.explicitTarget : supported ? supported.instruction_target ?? { run_id: supported.id, thread_id: supported.thread_id, attempt: supported.attempt } : undefined;
  const liveTarget = supplied ? { run_id: supplied.run_id, thread_id: supplied.thread_id, attempt: supplied.attempt } : undefined;
  const hasDraft = !!state.draft || !!state.pending;
  const pinnedTarget = hasDraft ? state.target ?? liveTarget : liveTarget;
  // An unsent draft can follow its same resumed carrier. Sending is still an
  // explicit new request; an uncertain POST keeps its original attempt/UUID.
  // Never move text to a different task or rewind it on a stale snapshot.
  const target = !state.pending && liveTarget && pinnedTarget &&
    liveTarget.run_id === pinnedTarget.run_id && liveTarget.thread_id === pinnedTarget.thread_id &&
    liveTarget.attempt > pinnedTarget.attempt ? liveTarget : pinnedTarget;
  const mode = state.mode === "next" ? "next" : liveTarget || hasDraft ? "instructions" : "next";
  const eligible = !!liveTarget && !!target && target.run_id === liveTarget.run_id && target.thread_id === liveTarget.thread_id && target.attempt === liveTarget.attempt;
  const canonical = [...props.turns.flatMap(turn => turn.run.instructions ?? []), ...(props.receipts ?? [])].filter(validInstructionReceipt);
  const receipts = new Map<string, RunInstructionReceipt>();
  for (const receipt of [...(local.owner === owner ? local.values : []), ...canonical]) {
    const prior = receipts.get(receipt.request_id);
    if (!prior || prior.state === "received") receipts.set(receipt.request_id, receipt);
  }
  const all = [...receipts.values()].sort((a, b) => a.sequence - b.sequence);
  useEffect(() => {
    owner.active = true; busy.current = false; setWorking(false); setError(""); setMissing(false); setLocal({ owner, values: [] });
    return () => { owner.active = false; };
  }, [owner]);
  function update(change: (value: InstructionMemory) => InstructionMemory) { callbacks.current.update?.(change); }
  useEffect(() => {
    if (!state.draft || state.pending || !target || target === pinnedTarget) return;
    update(current => current.pending || !current.draft || current.target !== pinnedTarget
      ? current : { ...current, target });
  }, [props.state?.draft, props.state?.pending, props.state?.target, liveTarget?.run_id, liveTarget?.thread_id, liveTarget?.attempt]);
  function accept(receipt: RunInstructionReceipt, pending?: PendingInstruction) {
    if (!owner.active) return;
    setLocal(current => ({ owner, values: [...(current.owner === owner ? current.values : []).filter(value => value.request_id !== receipt.request_id), receipt].slice(-8) }));
    if (pending) update(current => current.pending?.request_id !== pending.request_id ? current : receipt.state === "closed"
      ? restoreInstruction(current, pending) : { ...current, pending: undefined });
    const waiting = transport.current;
    if (waiting?.request_id === receipt.request_id) {
      transport.current = null; busy.current = false; setWorking(false); waiting.controller.abort();
    }
    setMissing(false); setError("");
  }
  // Canonical SSE/snapshots settle lost HTTP receipts without replaying writes.
  useEffect(() => {
    const pending = props.state?.pending;
    if (!pending) return;
    const receipt = canonical.find(value => sameInstruction(value, pending));
    if (receipt) accept(receipt, pending);
  }, [props.turns, props.receipts, props.state?.pending?.request_id]);
  async function read(pending: PendingInstruction) {
    if (busy.current) return;
    busy.current = true; setWorking(true); setError("");
    try {
      const value = await scope.request<unknown>(`/runs/${encodeURIComponent(pending.run_id)}/instructions/${encodeURIComponent(pending.request_id)}`);
      if (!validInstructionReceipt(value) || !sameInstruction(value, pending)) throw new Error("Invalid instruction receipt");
      accept(value, pending);
    } catch (failure) {
      if (!owner.active || isAborted(failure)) return;
      if (failure instanceof ApiFailure && failure.status === 401) callbacks.current.onExpired();
      else { setMissing(failure instanceof ApiFailure && failure.status === 404); setError("추가 지시 접수를 확인하지 못했습니다. 자동으로 다시 보내지 않습니다."); }
    } finally { if (owner.active) { busy.current = false; setWorking(false); } }
  }
  // Navigation/reload only observes the original UUID. An absent receipt never POSTs automatically.
  useEffect(() => {
    const pending = props.state?.pending;
    if (pending?.status === "uncertain") void read(pending);
  }, [scope, props.state?.pending?.request_id, props.state?.pending?.status]);
  // Only outstanding received receipts need observation; no model call or mutation.
  useEffect(() => {
    const received = all.filter(value => value.state === "received");
    if (!received.length || !owner.active) return;
    const controller = new AbortController();
    const timer = window.setTimeout(async () => {
      for (const receipt of received) {
        try {
          const value = await scope.request<unknown>(`/runs/${encodeURIComponent(receipt.run_id)}/instructions/${encodeURIComponent(receipt.request_id)}`, "GET", undefined, controller.signal);
          if (validInstructionReceipt(value) && sameInstruction(value, { ...receipt, status: "uncertain" }) && owner.active) accept(value);
        } catch (failure) { if (owner.active && failure instanceof ApiFailure && failure.status === 401) callbacks.current.onExpired(); }
      }
      if (owner.active && !controller.signal.aborted) setPoll(value => value + 1);
    }, 2000);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [scope, poll, all.map(value => `${value.request_id}:${value.state}`).join("|")]);
  async function post(original?: PendingInstruction) {
    const current = callbacks.current.state ?? emptyInstructions();
    if (busy.current || (!original && (!eligible || current.pending || !current.draft.trim() || instructionBytes(current.draft) > 2000))) return;
    const pending: PendingInstruction = original ?? { ...target!, request_id: crypto.randomUUID(), text: current.draft, status: "sending" };
    const change = callbacks.current.update;
    const operation = { request_id: pending.request_id, controller: new AbortController() };
    transport.current = operation;
    busy.current = true; setWorking(true); setError(""); setMissing(false);
    change?.(value => original && value.pending?.request_id !== original.request_id ? value : { ...value, mode: "instructions", target: { run_id: pending.run_id, thread_id: pending.thread_id, attempt: pending.attempt }, draft: original ? value.draft : "", pending: { ...pending, status: "sending" } });
    try {
      const value = await scope.request<unknown>(`/runs/${encodeURIComponent(pending.run_id)}/instructions`, "POST", { request_id: pending.request_id, expected_attempt: pending.attempt, text: pending.text }, operation.controller.signal, { timeoutMs: 30000 });
      if (!validInstructionReceipt(value) || !sameInstruction(value, pending)) throw new Error("Invalid instruction receipt");
      accept(value, pending);
    } catch (failure) {
      const definite = failure instanceof ApiFailure && failure.status < 500;
      change?.(value => definite ? restoreInstruction(value, pending) : value.pending?.request_id === pending.request_id ? { ...value, pending: { ...pending, status: "uncertain" } } : value);
      if (owner.active && !isAborted(failure)) {
        if (failure instanceof ApiFailure && failure.status === 401) callbacks.current.onExpired();
        else setError(definite ? "작업이 끝났거나 추가 지시를 받을 수 없습니다. 글은 추가 지시 초안에 유지했습니다." : "추가 지시 접수를 확인하지 못했습니다. 원래 요청의 상태만 확인합니다.");
      }
    } finally {
      if (transport.current === operation) {
        transport.current = null;
        if (owner.active) { busy.current = false; setWorking(false); }
      }
    }
  }
  const canSubmit = !!props.update && props.ready && eligible && !state.pending && !working && !!state.draft.trim() && instructionBytes(state.draft) <= 2000;
  return {
    mode, target, eligible, supported: !!liveTarget, text: state.draft, canSubmit, submit: () => post(),
    chooseMode(next: "instructions" | "next") { update(value => ({ ...value, mode: next, target: value.draft || value.pending ? value.target : liveTarget })); },
    setText(text: string) { update(value => ({ ...value, draft: text, target: value.draft || value.pending ? value.target ?? liveTarget : liveTarget })); },
    receipts: all,
    controls: <>
      {props.showModes !== false && (liveTarget || state.draft || state.pending || mode === "instructions") && <div className="instruction-modes" role="group" aria-label="입력 용도">
        <button type="button" aria-pressed={mode === "instructions"} onClick={() => update(value => ({ ...value, mode: "instructions", target: value.draft || value.pending ? value.target : liveTarget }))}>진행 중인 작업에 추가</button>
        <button type="button" aria-pressed={mode === "next"} onClick={() => update(value => ({ ...value, mode: "next" }))}>다음 메시지 준비</button>
      </div>}
      {mode === "instructions" && !eligible && <p role="status">이 작업은 지금 추가 지시를 받을 수 없습니다. 초안은 유지됩니다.</p>}
      {error && <p role="alert">{error}</p>}
      {state.pending && <div className="instruction-recovery"><p role="status">{state.pending.status === "sending" ? "추가 지시 접수 중…" : "추가 지시 접수 확인 필요"}</p>
        <button type="button" disabled={working} onClick={() => void read(state.pending!)}>접수 상태 확인</button>
        {missing && <button type="button" disabled={working} onClick={() => void post(state.pending)}>같은 추가 지시 다시 접수</button>}
      </div>}
    </>,
  };
}


export function MissionInstructions({ accountId, mission, state, update, onExpired }: {
  accountId: string; mission: MissionView; state?: InstructionMemory;
  update?: (change: (value: InstructionMemory) => InstructionMemory) => void; onExpired: () => void;
}) {
  const [fallback, setFallback] = useState<InstructionMemory>(emptyInstructions);
  const current = state ?? fallback;
  const instructions = useRunInstructions({ accountId, threadId: mission.id, turns: [], state: current, update: update ?? setFallback,
    explicitTarget: mission.instruction_target, receipts: mission.instructions, ready: true, showModes: false, onExpired });
  if (!mission.instruction_target?.supports_instructions && !current.draft && !current.pending && !mission.instructions?.length) return null;
  return <details className="mission-live-instructions"><summary>진행 중인 일에 추가 지시</summary>
    <InstructionReceipts receipts={instructions.receipts}/>
    {instructions.controls}
    <form onSubmit={event => { event.preventDefault(); if (instructions.canSubmit) void instructions.submit(); }}>
      <label>업무에 추가 지시<textarea value={instructions.text} onChange={event => instructions.setText(event.target.value)}/></label>
      <button type="submit" disabled={!instructions.canSubmit}>같은 업무에 추가 지시 보내기</button>
      <p>글만 원래 업무에 보완합니다. 처음 허용한 범위·종료 시각·실행 한도나 메일 최종 발송 허락을 늘리지 않습니다.</p>
    </form>
  </details>;
}

export function InstructionReceipts({ receipts }: { receipts: RunInstructionReceipt[] }) {
  if (!receipts.length) return null;
  return <ul className="instruction-receipts" aria-label="추가 지시 상태">{receipts.map(receipt => <li key={receipt.request_id}>
    <p>{receipt.text}</p><span role="status">{receipt.state === "received" ? "추가 지시 접수됨 · 아직 반영 전" : receipt.state === "applied" ? "추가 지시 반영됨" : "추가 지시 미반영 종료"}</span>
  </li>)}</ul>;
}
