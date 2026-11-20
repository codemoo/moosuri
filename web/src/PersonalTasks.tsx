import { useCallback, useEffect, useRef, useState } from "react";
import { PersonalWorkflows, type WorkSchedule } from "./PersonalWorkflows";
import { ApiFailure, isAborted, RequestScope } from "./api";
import { browserStateLabel, type PersonalBrowserState } from "./personal-browser";

type TaskState = PersonalBrowserState | "queued" | "running" | "failed" | "cancelled" | "incomplete" | "refused" | "awaiting_consent" | "dispatched" | "withdrawn" | "unknown";
export type TaskSummary = {
  id: string; thread_id: string; title: string; state: TaskState; kind?: "text" | "browser" | "research" | "gmail"; preview?: string | null;
  next_actor: "assistant" | "member" | "none"; created_at: number; scheduled_at?: number | null;
};
export type TaskPage = { items: TaskSummary[]; next_before: string | null; schedules?: WorkSchedule[] };
export type TaskDetail = {
  task: TaskSummary; request: string; thread_title: string; reason: string | null;
  history: { state: string; at: number | null; title?: string | null; detail?: string | null }[];
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
const states:Record<string,string> = { starting:"시작됨", queued:"접수됨", running:"답변 작성 중", completed:"완료됨", failed:"실패", cancelled:"취소됨", incomplete:"일부 완료", refused:"응답 거절", outcome_unknown:"결과 확인 필요", awaiting_consent:"Google 연결 대기", ready:"조회 준비됨", dispatched:"조회 요청 전달", unknown:"조회 결과 확인 필요", withdrawn:"조회 권한 철회", prepared:"대화 문맥 준비", searched:"검색 완료", source_read:"자료 확인" };
function stateLabel(task:TaskSummary,state:string=task.state) {
  if ((!task.kind || task.kind==="browser") && state in browserStateLabel) return browserStateLabel[state as PersonalBrowserState];
  return states[state] ?? state;
}
const shortTime = new Intl.DateTimeFormat("ko-KR",{hour:"numeric",minute:"2-digit"});
const calendarDay = new Intl.DateTimeFormat("ko-KR",{year:"numeric",month:"long",day:"numeric"});
function dayLabel(at:number,today:Date,yesterday:Date) {
  const date=new Date(at*1000);
  if(date.toDateString()===today.toDateString()) return "오늘";
  if(date.toDateString()===yesterday.toDateString()) return "어제";
  return calendarDay.format(date);
}

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
const actorLabel = { assistant: "어시스턴트 진행", member: "내 확인 필요", none: "" };
function taskOrigin(task: TaskSummary) {
  switch (task.kind) {
    case "research":
      if (task.scheduled_at != null) return "예약 실행";
      return task.thread_id ? "대화에서 실행" : "직접 실행";
    case "text": return "대화 답변";
    case "gmail": return "Gmail 조회";
    default: return "브라우저";
  }
}

function TaskReader({ accountId, taskId, onBack, onThread, onExpired, onTask }: Props & {
  taskId: string; onTask: (task: TaskSummary) => void;
}) {
  const [step,setStep] = useState<number | null>(null);
  const stepPanel=useRef<HTMLDivElement>(null);
  const stepTrigger=useRef<HTMLButtonElement | null>(null);
  useEffect(()=>{if(step!==null)stepPanel.current?.focus({preventScroll:true});},[step]);
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
        if (["starting", "reading", "resuming", "stopping", "ready", "user_control", "running", "queued", "dispatched"].includes(received.task.state))
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
  const selected=step===null?null:detail?.history[step];
  const isBrowser=!detail?.task.kind || detail.task.kind==="browser";
  const stepBody=selected && detail && (selected.detail ?? (selected.state==="starting" ? detail.request :
    selected.state==="completed" ? detail.result : (isBrowser ? nextStep[selected.state as PersonalBrowserState] : null)) ?? stateLabel(detail.task,selected.state));
  return <section className="personal-tasks task-reader" aria-label="맡긴 일 상세">
    <button className="task-back" type="button" onClick={onBack}>맡긴 일 목록</button>
    {detail && <div className="task-meta"><span className="task-state">{stateLabel(detail.task)}</span> · {taskOrigin(detail.task)}<TaskTime at={detail.task.created_at} /></div>}
    <h2>{detail?.task.title || "작업 상세"}</h2>
    {issue && <p role="alert">{issue}</p>}
    {!detail && loading && <p role="status">작업을 불러오는 중…</p>}
    {detail && <>
      {isBrowser && <p>{nextStep[detail.task.state as PersonalBrowserState]}</p>}
      {detail.reason && reasons[detail.reason] && <p>{reasons[detail.reason]}</p>}
      <div className="task-receipt">
        <h3>요청 내용</h3><p className="task-result">{detail.request}</p>
        {detail.task.scheduled_at != null && <><h3>예약 회차</h3><TaskTime at={detail.task.scheduled_at}/></>}
        {detail.result && <><h3>작업 결과</h3><p className="task-result">{detail.result}</p></>}
        {detail.observation && <><h3>확인한 내용</h3><p className="task-result">{detail.observation}</p></>}
      </div>
      <h3>진행 기록</h3><div className="task-trace" data-selected={!!selected}>
        <ol className="task-history">{detail.history.map((event,index)=>
          <li key={index}><button type="button" aria-pressed={index===step} onClick={e=>{stepTrigger.current=e.currentTarget;setStep(index);}}>
            <span className="task-marker" aria-hidden="true">{index===0 ? "•" : "✓"}</span>
            <span>{event.title || (event.state==="closed" ? "브라우저 종료" : stateLabel(detail.task,event.state))}</span>
            <small>{event.at!==null?shortTime.format(event.at*1000):"시각 미기록"}</small>
          </button></li>)}</ol>
        {selected && <div className="task-step-detail" role="region" aria-label="단계 상세" tabIndex={-1} ref={stepPanel}>
          <button className="task-step-back" onClick={()=>{setStep(null);requestAnimationFrame(()=>stepTrigger.current?.focus());}}>진행 기록으로</button>
          <h4>{selected.title || stateLabel(detail.task,selected.state)}</h4>
          {selected.at!==null && <TaskTime at={selected.at}/>}<p>{stepBody}</p>
        </div>}
      </div>
      {detail.task.thread_id && <button type="button" onClick={() => onThread(detail.task.thread_id, detail.task.kind==="gmail"?"":detail.task.id)}>
        원래 대화 열기 · {detail.thread_title}
      </button>}
    </>}
    <button type="button" disabled={loading} onClick={() => setReload(value => value + 1)}>
      {issue ? "다시 시도" : "상태 새로고침"}
    </button>
  </section>;
}

function TaskList({ accountId, onExpired, onOpenTask, latest, taskId }: Props & { latest: TaskSummary | null }) {
  const [schedules,setSchedules] = useState<WorkSchedule[]>([]);
  const lastOpened=useRef<string|null>(null);
  const listRoot=useRef<HTMLElement>(null);
  useEffect(()=>{if(!taskId && lastOpened.current){const id=lastOpened.current;Array.from(listRoot.current?.querySelectorAll<HTMLButtonElement>("[data-task-id]")??[]).find(el=>el.dataset.taskId===id)?.focus({preventScroll:true});}},[taskId]);
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
      if (!cursor) setSchedules(page.schedules ?? []);
      setBefore(page.next_before);
      setLoaded(true);
    } catch (error) {
      if (isAborted(error)) return;
      if (error instanceof ApiFailure && error.status === 401) expired.current();
      else {
        if (error instanceof ApiFailure && error.status === 403) {
          setRows([]);setSchedules([]);
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
  const nextDue=schedules.filter(s=>s.enabled && s.next_due!=null).map(s=>s.next_due!);
  const due=nextDue.length?Math.min(...nextDue):null;
  useEffect(()=>{
    if(!due || taskId || before || issue) return;
    const timer=setTimeout(()=>void read(null),Math.max(7000,Math.min(2147483000,(due-Date.now()/1000+6)*1000)));
    return ()=>clearTimeout(timer);
  },[due,schedules,taskId,before,issue,read]);
  const today=new Date(), yesterday=new Date(today);
  yesterday.setDate(today.getDate()-1);
  const groups=new Map<string,TaskSummary[]>();
  for(const task of rows){const day=dayLabel(task.created_at,today,yesterday);const group=groups.get(day);if(group)group.push(task);else groups.set(day,[task]);}
  function openTask(id:string){lastOpened.current=id;onOpenTask(id);}
  return <section ref={listRoot} className="personal-tasks" aria-label="맡긴 일 목록">
    <div className="task-heading"><h2>맡긴 일</h2><button type="button" disabled={loading} onClick={() => { void read(null); }}>목록 새로고침</button></div>
    <PersonalWorkflows schedules={schedules} getScope={()=>current.current} onChanged={()=>read(null)} onOpen={openTask} onExpired={onExpired}/>
    {Array.from(groups,([day,items])=><details className="task-day" key={day} open><summary><h3>{day}</h3></summary><ul className="task-list">{items.map(task=>
      <li key={task.id}><button className="task-row" data-task-id={task.id} type="button" onClick={() => openTask(task.id)}>
        <span className="task-marker" aria-hidden="true">{task.state==="completed"?"✓":"•"}</span>
        <span className="task-row-copy"><strong>{task.title}</strong>{task.preview && <span className="task-preview">{task.preview}</span>}
        <span className="task-meta">{[taskOrigin(task), stateLabel(task), actorLabel[task.next_actor]].filter(Boolean).join(" · ")}</span></span>
        <time dateTime={new Date(task.created_at*1000).toISOString()}>{shortTime.format(task.created_at*1000)}</time>
      </button></li>)}</ul></details>)}
    {issue && <p role="alert">작업 목록을 불러오지 못했습니다.</p>}
    {!issue && loaded && rows.length === 0 && <p>아직 맡긴 일이 없습니다.</p>}
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
