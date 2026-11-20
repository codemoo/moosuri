import { useCallback, useEffect, useRef, useState } from "react";
import { ApiFailure, isAborted, RequestScope } from "./api";
import { browserStateLabel, type PersonalBrowserState } from "./personal-browser";

export type TaskSummary = {
  id: string; thread_id: string; title: string; state: PersonalBrowserState;
  next_actor: "assistant" | "member" | "none"; created_at: number;
};
export type TaskPage = { items: TaskSummary[]; next_before: string | null };
export type TaskDetail = {
  task: TaskSummary; request: string; thread_title: string; reason: string | null;
  history: { state: PersonalBrowserState; at: number | null }[];
  result: string | null; observation: string | null;
};
type Props = {
  accountId: string; taskId?: string; onExpired: () => void; onOpenTask: (id: string) => void;
  onBack: () => void; onThread: (threadId: string, runId: string) => void;
};
const dateFormat = new Intl.DateTimeFormat("ko-KR", { dateStyle: "medium", timeStyle: "short" });
function TaskTime({ at }: { at: number }) {
  const date = new Date(at * 1000);
  return <time dateTime={date.toISOString()}>{dateFormat.format(date)}</time>;
}

const nextStep: Record<PersonalBrowserState, string> = {
  starting: "어시스턴트가 브라우저를 준비하고 있습니다.",
  reading: "어시스턴트가 페이지를 읽고 있습니다.",
  ready: "원래 대화에서 페이지를 확인하고 작업을 이어가세요.",
  user_control: "원래 대화에서 직접 조작을 마치고 어시스턴트에게 돌려주세요.",
  resuming: "어시스턴트가 변경된 페이지를 읽고 있습니다.",
  stopping: "어시스턴트가 작업을 중지하고 있습니다.",
  completed: "작업이 끝났습니다.",
  closed: "브라우저가 닫혔습니다.",
  outcome_unknown: "원래 대화에서 마지막으로 확인된 내용을 살펴보세요.",
};
const reasons: Record<string, string> = {
  browser_model_unavailable: "페이지를 열었지만 모델 응답을 받지 못했습니다.",
  browser_busy: "브라우저가 다른 작업을 처리하고 있습니다.",
  browser_authority_expired: "이 작업의 브라우저 세션이 종료됐습니다.",
  browser_worker_missing: "브라우저 연결이 종료됐습니다.",
  browser_interrupted: "서버가 재시작되어 마지막 동작의 결과를 확인할 수 없습니다.",
  browser_unavailable: "연결이 끊겨 마지막 동작의 결과를 확인할 수 없습니다.",
  browser_stale: "확인 중이던 페이지가 변경됐습니다.",
  new_guidance: "추가로 받은 지시가 있습니다.",
  controller_expired: "직접 조작 시간이 끝났습니다.",
};
const actorLabel = { assistant: "어시스턴트 진행", member: "내 확인 필요", none: "종료" };

function TaskReader({ accountId, taskId, onBack, onThread, onExpired, onTask }: Props & {
  taskId: string; onTask: (task: TaskSummary) => void;
}) {
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [issue, setIssue] = useState("");
  const [reload, setReload] = useState(0);
  const [loading, setLoading] = useState(true);
  const expired = useRef(onExpired);
  expired.current = onExpired;
  useEffect(() => {
    const scope = new RequestScope(accountId);
    let timer: ReturnType<typeof setTimeout>;
    async function read() {
      setLoading(true);
      try {
        const received = await scope.request<TaskDetail>(`/personal/tasks/${encodeURIComponent(taskId)}`);
        setDetail(received);
        onTask(received.task);
        setIssue("");
        if (["starting", "reading", "resuming", "stopping", "ready", "user_control"].includes(received.task.state))
          timer = setTimeout(() => { void read(); }, 5000);
      } catch (error) {
        if (isAborted(error)) return;
        if (error instanceof ApiFailure && error.status === 401) { expired.current(); return; }
        if (error instanceof ApiFailure && [403, 404].includes(error.status)) {
          setDetail(null);
          setIssue("이 작업을 찾을 수 없습니다.");
        } else setIssue("작업을 불러오지 못했습니다.");
      } finally {
        setLoading(false);
      }
    }
    void read();
    return () => { scope.close(); clearTimeout(timer); };
  }, [accountId, taskId, reload, onTask]);
  return <section className="personal-tasks" aria-label="맡긴 일 상세">
    <button type="button" onClick={onBack}>맡긴 일 목록</button>
    <h2>{detail?.task.title || "작업 상세"}</h2>
    {issue && <p role="alert">{issue}</p>}
    {!detail && loading && <p role="status">작업을 불러오는 중…</p>}
    {detail && <>
      <div className="task-meta"><span className="task-state">{browserStateLabel[detail.task.state]}</span>
        <TaskTime at={detail.task.created_at} /></div>
      <p>{nextStep[detail.task.state]}</p>
      {detail.reason && reasons[detail.reason] && <p>{reasons[detail.reason]}</p>}
      <button type="button" onClick={() => onThread(detail.task.thread_id, detail.task.id)}>
        원래 대화 열기 · {detail.thread_title}
      </button>
      <h3>요청 내용</h3><p className="task-result">{detail.request}</p>
      {detail.result && <><h3>작업 결과</h3><p className="task-result">{detail.result}</p></>}
      {detail.observation && <><h3>확인한 내용</h3><p className="task-result">{detail.observation}</p></>}
      <h3>진행 기록</h3><ol className="task-history">{detail.history.map((event, index) =>
        <li key={index}><span>{event.state === "closed" ? "브라우저 종료" : browserStateLabel[event.state]}</span>{" "}
          {event.at !== null ? <TaskTime at={event.at} /> : <small>시각 미기록</small>}</li>)}</ol>
    </>}
    <button type="button" disabled={loading} onClick={() => setReload(value => value + 1)}>
      {issue ? "다시 시도" : "상태 새로고침"}
    </button>
  </section>;
}

function TaskList({ accountId, onExpired, onOpenTask, latest }: Props & { latest: TaskSummary | null }) {
  const [rows, setRows] = useState<TaskSummary[]>([]);
  useEffect(() => {
    if (latest) setRows(previous => previous.map(row => row.id === latest.id ? latest : row));
  }, [latest]);
  const [before, setBefore] = useState<string | null>(null);
  const [issue, setIssue] = useState(false);
  const failedCursor = useRef<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(true);
  const current = useRef<RequestScope | null>(null);
  const pending = useRef(false);
  const expired = useRef(onExpired);
  expired.current = onExpired;
  const read = useCallback(async (cursor: string | null) => {
    const scope = current.current;
    if (!scope || pending.current) return;
    pending.current = true;
    setLoading(true);
    setIssue(false);
    try {
      const page = await scope.request<TaskPage>("/personal/tasks" + (cursor ? "?before=" + encodeURIComponent(cursor) : ""));
      setRows(previous => cursor ? [...previous, ...page.items] : page.items);
      setBefore(page.next_before);
      setLoaded(true);
    } catch (error) {
      if (isAborted(error)) return;
      if (error instanceof ApiFailure && error.status === 401) expired.current();
      else {
        if (error instanceof ApiFailure && error.status === 403) {
          setRows([]);
          setBefore(null);
        }
        failedCursor.current = cursor;
        setIssue(true);
      }
    } finally {
      if (current.current === scope) { setLoading(false); pending.current = false; }
    }
  }, []);
  useEffect(() => {
    const scope = new RequestScope(accountId);
    current.current = scope;
    pending.current = false;
    void read(null);
    return () => { current.current = null; scope.close(); };
  }, [accountId, read]);
  return <section className="personal-tasks" aria-label="맡긴 일 목록">
    <h2>맡긴 일</h2>
    <p>대화에서 맡긴 브라우저 작업의 진행 상황과 결과를 확인하세요.</p>
    <button type="button" disabled={loading} onClick={() => { void read(null); }}>목록 새로고침</button>
    {rows.length > 0 && <ul className="task-list">{rows.map(task =>
      <li key={task.id}><button className="task-row" type="button" onClick={() => onOpenTask(task.id)}>
        <strong>{task.title}</strong>
        <span className="task-state">{browserStateLabel[task.state]}</span>
        <span className="task-meta">{actorLabel[task.next_actor]} · <TaskTime at={task.created_at} /></span>
      </button></li>)}</ul>}
    {issue && <p role="alert">작업 목록을 불러오지 못했습니다.</p>}
    {!issue && loaded && rows.length === 0 && <p>아직 맡긴 브라우저 작업이 없습니다.</p>}
    {loading && <p role="status">작업 목록을 불러오는 중…</p>}
    {(before || issue) && <button type="button" disabled={loading} onClick={() => { void read(issue ? failedCursor.current : before); }}>
      {issue ? "다시 시도" : "이전 작업 더 보기"}
    </button>}
  </section>;
}
function MemberTasks(props: Props) {
  // Keep fetched pages when returning from a detail. A direct detail link needs no list request.
  const [latest, setLatest] = useState<TaskSummary | null>(null);
  const [visitedList, setVisitedList] = useState(!props.taskId);
  useEffect(() => { if (!props.taskId) setVisitedList(true); }, [props.taskId]);
  return <>
    {(visitedList || !props.taskId) && <div hidden={!!props.taskId}><TaskList {...props} latest={latest} /></div>}
    {props.taskId && <TaskReader key={props.taskId} {...props} taskId={props.taskId} onTask={setLatest} />}
  </>;
}
export function PersonalTasks(props: Props) {
  return <MemberTasks key={props.accountId} {...props} />;
}
