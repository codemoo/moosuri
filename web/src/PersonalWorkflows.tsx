import { useRef, useState, type FormEvent } from "react";
import { ApiFailure, isAborted, type RequestScope } from "./api";

export type WorkScope = "memory" | "conversation";
export type WorkSchedule = {
  id: string; title: string; query: string; scope: WorkScope; period: number;
  next_due: number | null; enabled: boolean; revision: number;
};
export type WorkReceipt = { workflow_id: string; run_id: string | null };
const intervals: Record<number, string> = { 1800: "30분마다", 3600: "1시간마다", 86400: "24시간마다", 604800: "7일마다" };
const scopeLabel: Record<WorkScope,string> = { memory: "저장된 기억", conversation: "최근 텍스트 대화" };
const format = new Intl.DateTimeFormat("ko-KR", {month:"short",day:"numeric",hour:"numeric",minute:"numeric"});

export function personalResearchIntent(text: string): { scope: WorkScope; query: string } | null {
  const explicit = text.trim().match(/^(?:내\s*)?(기억|대화)\s*검색\s*[:：]\s*(.+)$/u);
  const natural = text.trim().match(/^(?:(?:내|저장된|지난|이전)\s*)?(기억|대화)(?:에서|\s*기록에서)\s+(.+?)\s*(?:찾아\s*줘|검색해\s*줘|조사해\s*줘)[.!?]?$/u);
  const match = explicit ?? natural;
  if (!match) return null;
  const query = match[2].replace(/^["'‘“]|["'’”]$/gu, "").trim();
  return query ? {scope:match[1] === "기억" ? "memory" : "conversation",query} : null;
}

type Props = {
  schedules: WorkSchedule[]; getScope: () => RequestScope | null;
  onChanged: () => Promise<void>; onOpen: (id: string) => void; onExpired: () => void;
};
export function PersonalWorkflows({schedules,getScope,onChanged,onOpen,onExpired}:Props) {
  const [form,setForm] = useState(false);
  const [scope,setScope] = useState<WorkScope>("memory");
  const [query,setQuery] = useState("");
  const [name,setName] = useState("");
  const [period,setPeriod] = useState(0);
  const [start,setStart] = useState("");
  const [all,setAll] = useState(false);
  const [expanded,setExpanded] = useState<string | null>(null);
  const [remove,setRemove] = useState(false);
  const [busy,setBusy] = useState(false);
  const [issue,setIssue] = useState("");
  const [notice,setNotice] = useState("");
  const pending = useRef(false);
  const receiptKey = useRef({ input:"", id:"" });
  function stableId(input:string) {
    if (input !== receiptKey.current.input) receiptKey.current={input,id:crypto.randomUUID()};
    return receiptKey.current.id;
  }
  async function submit(path:string, body:unknown, message:string, close=false) {
    const requests=getScope();
    if (!requests || pending.current) return;
    pending.current=true; setBusy(true);setIssue("");setNotice("");
    try {
      const result=await requests.request<WorkReceipt>(path,"POST",body,undefined,{timeoutMs:30000});
      receiptKey.current={input:"",id:""};
      if (close) {setForm(false);setName("");setQuery("");setStart("");}
      setRemove(false);
      await onChanged();
      if (result.run_id) onOpen(result.run_id);
      else setNotice(message);
    } catch (e) {
      if (isAborted(e)) return;
      if (e instanceof ApiFailure && e.status===401) {onExpired();return;}
      if (e instanceof ApiFailure && [404,409].includes(e.status)) {
        await onChanged();setIssue("예약 상태가 바뀌었습니다. 현재 상태를 확인해 주세요.");
      } else if (e instanceof ApiFailure && [400,422].includes(e.status)) setIssue("검색 조건과 첫 실행 시각을 확인해 주세요.");
      else if (e instanceof ApiFailure && e.status===429) setIssue("예약 가능한 작업 수를 모두 사용했습니다.");
      else setIssue("처리 결과를 확인하지 못했습니다. 같은 내용으로 다시 시도할 수 있습니다.");
    } finally {pending.current=false;setBusy(false);}
  }
  function create(e:FormEvent) {
    e.preventDefault();
    const value=query.trim(), title=name.trim() || value+" 조사";
    if (!value) return;
    const first_due=period && start ? Math.floor(new Date(start).getTime()/1000) : null;
    const body={title,scope,query:value,period,first_due};
    void submit("/personal/tasks/workflows",{...body,request_id:stableId(JSON.stringify(body))},"예약 작업을 추가했습니다.",true);
  }
  function act(s:WorkSchedule, action:"run"|"pause"|"resume"|"remove"){
    const body=action==="run"?{action,request_id:stableId(s.id+"/run")}:{action,revision:s.revision};
    void submit("/personal/tasks/workflows/"+encodeURIComponent(s.id),body,action==="pause"?"예약을 일시정지했습니다.":action==="resume"?"예약을 재개했습니다.":"예약을 해제했습니다.");
  }
  return <div className="task-controls">
    <button className="task-create" type="button" aria-expanded={form} onClick={()=>{setForm(!form);setIssue("");}}>자료 조사 맡기기</button>
    {form && <form className="task-form" onSubmit={create}>
      <label>찾을 곳<select value={scope} onChange={e=>setScope(e.target.value as WorkScope)}>{Object.entries(scopeLabel).map(([key,label])=><option key={key} value={key}>{label}</option>)}</select></label>
      <label>검색어<input required maxLength={128} value={query} onChange={e=>setQuery(e.target.value)} /></label>
      <label>업무 이름 (선택)<input maxLength={80} value={name} onChange={e=>setName(e.target.value)} /></label>
      <label>실행 주기<select value={period} onChange={e=>setPeriod(Number(e.target.value))}><option value={0}>지금 한 번</option>{Object.entries(intervals).map(([n,label])=><option key={n} value={n}>{label}</option>)}</select></label>
      {period>0 && <label>첫 실행 (선택)<input type="datetime-local" value={start} onChange={e=>setStart(e.target.value)} /><small>비워 두면 한 주기 뒤에 시작합니다.</small></label>}
      <button type="submit" disabled={busy || !query.trim()}>{period ? "예약 추가" : "조사 실행"}</button>
    </form>}
    <details className="task-schedules" open>
      <summary>예약된 작업 <span>{schedules.length}개</span></summary>
      {!schedules.length && <p>등록한 예약이 없습니다.</p>}
      {(all ? schedules : schedules.slice(0,3)).map(s=><div className="task-schedule" key={s.id}>
        <button type="button" aria-expanded={expanded===s.id} onClick={()=>{setExpanded(expanded===s.id?null:s.id);setRemove(false);}}>
          <span className="schedule-clock" aria-hidden="true">◷</span><span><strong>{s.title}</strong><small>{intervals[s.period]}{s.enabled?"":" · 일시정지"}</small></span>
        </button>
        {expanded===s.id && <div className="schedule-settings">
          <p>{scopeLabel[s.scope]} · {s.query}</p>
          {s.enabled && s.next_due && <p>다음 실행 <time dateTime={new Date(s.next_due*1000).toISOString()}>{format.format(s.next_due*1000)}</time></p>}
          <div className="task-actions">
            <button disabled={busy} onClick={()=>act(s,"run")}>지금 실행</button>
            <button disabled={busy} onClick={()=>act(s,s.enabled?"pause":"resume")}>{s.enabled?"일시정지":"예약 재개"}</button>
            <button disabled={busy} onClick={()=>setRemove(!remove)}>예약 해제</button>
            {remove && <button disabled={busy} onClick={()=>act(s,"remove")}>예약 해제 확인</button>}
          </div>
        </div>}
      </div>)}
      {schedules.length>3 && <button className="schedule-all" type="button" onClick={()=>setAll(!all)}>{all?"예약 접기":"예약 작업 모두 보기"}</button>}
    </details>
    {issue && <p role="alert">{issue}</p>}{notice && <p role="status">{notice}</p>}
  </div>;
}
