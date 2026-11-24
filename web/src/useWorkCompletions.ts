import { useCallback, useEffect, useRef, useState } from "react";
import { ApiFailure, RequestScope } from "./api";
import { WORK_PENDING_KEY, parsePage, type ConversationWorkGroup } from "./conversation-work";

type Pending = { thread: string; group: string; task: string; revision: number };
type Completed = { run: string; title: string; occurrence: number };


function restore(account: string, session: string | undefined): Map<string, Pending> {
  try {
    const saved = JSON.parse(sessionStorage.getItem(WORK_PENDING_KEY) ?? "null");
    if (session && saved?.account === account && saved?.session === session && Array.isArray(saved.items)) {
      return new Map(saved.items.slice(-48).filter((v: Pending) => v && typeof v.task === "string" &&
        typeof v.thread === "string" && typeof v.group === "string" && Number.isSafeInteger(v.revision))
        .map((v: Pending) => [v.task, v]));
    }
  } catch { /* Readers can use the current tab when storage is unavailable. */ }
  return new Map();
}

/** Watch only work this member has seen queued. Historic receipts never generate a fresh notice. */
export function useWorkCompletions(account: string, session: string | undefined, visibleThread: string | null) {
  const pending = useRef(new Map<string, Pending>());
  const revisions = useRef(new Map<string, number>());
  const [completed, setCompleted] = useState<Completed[]>([]);
  const [watching, setWatching] = useState(false);
  const lifetime = useRef<RequestScope | null>(null);
  const persist = useCallback(() => {
    setWatching(pending.current.size > 0);
    try {
      if (!session || pending.current.size === 0) sessionStorage.removeItem(WORK_PENDING_KEY);
      else sessionStorage.setItem(WORK_PENDING_KEY, JSON.stringify({ account, session, items: [...pending.current.values()].slice(-48) }));
    } catch { /* The in-memory monitor still works. */ }
  }, [account, session]);
  useEffect(() => {
    const scope = new RequestScope(account);
    lifetime.current = scope;
    pending.current = restore(account, session);
    revisions.current.clear();
    setCompleted([]);
    persist();
    return () => { lifetime.current = null; scope.close(); };
  }, [account, session, persist]);
  const observe = useCallback((group: ConversationWorkGroup) => {
    let changed = false;
    const notices: Completed[] = [];
    for (const task of group.tasks) {
      if ((revisions.current.get(task.id) ?? 0) > task.revision) continue;
      revisions.current.set(task.id, task.revision);
      const previous = pending.current.get(task.id);
      if (previous && previous.revision > task.revision) continue;
      if (task.state === "queued") {
        if (previous?.revision === task.revision) continue;
        pending.current.set(task.id, { thread: group.thread_id, group: group.id, task: task.id, revision: task.revision });
        changed = true;
      } else if (previous) {
        pending.current.delete(task.id);
        changed = true;
        if (task.state === "completed" && task.run_id) notices.push({ run: task.run_id, title: task.title, occurrence: task.runs.length });
      }
    }
    if (notices.length) setCompleted(items => [...items.filter(i => !notices.some(n => n.run === i.run)), ...notices].slice(-6));
    if (changed) persist();
  }, [persist]);

  useEffect(() => {
    const scope = lifetime.current;
    if (!watching || !scope) return;
    let active = true, reading = false, offset = 0;
    const check = async () => {
      if (!active || reading) return;
      const groups = [...new Map([...pending.current.values()].filter(p => p.thread !== visibleThread)
        .map(p => [p.group, p])).values()];
      if (!groups.length) return;
      const item = groups[offset++ % groups.length];
      reading = true;
      try {
        const path = `/personal/tasks/conversations/${encodeURIComponent(item.thread)}?request_id=${encodeURIComponent(item.group)}`;
        const page = parsePage(await scope.request<unknown>(path));
        if (active && page?.thread_id === item.thread) {
          for (const group of page.groups) if (group.id === item.group) observe(group);
        }
      } catch (error) {
        if (active && error instanceof ApiFailure && error.status === 404) {
          for (const itemInGroup of pending.current.values()) if (itemInGroup.group === item.group) pending.current.delete(itemInGroup.task);
          persist();
        }
      } finally { reading = false; }
    };
    const timer = window.setInterval(() => void check(), 3000);
    return () => { active = false; window.clearInterval(timer); };
  }, [watching, visibleThread, observe, persist]);
  return { observe, completed, dismiss: (run: string) => setCompleted(items => items.filter(item => item.run !== run)) };
}
