import { TaskMonitors } from "./TaskMonitors";
import { MissionInstructions } from "./RunInstructions";
import type { InstructionMemory } from "./run-instructions";
import {useEffect,useRef,useState,type FormEvent} from "react";
import {useScope} from "./account-scope";
import {ApiFailure,isAborted} from "./api";
import type {MissionView,MissionAcceptanceUpdate,MissionCheck,MissionControl,MissionCreate,RecurringMissionCreate,RecurringMissionView,ScheduleRule,ScheduleControl,ScheduleEdit} from "../shared/api-types";
import "./missions.css";
import {MissionInput} from "./MissionInput";
type Props={accountId:string;onExpired:()=>void;back:()=>void;instructionDrafts?:Record<string,InstructionMemory>;onInstruction?:(id:string,change:(current:InstructionMemory)=>InstructionMemory)=>void};
const active=(m:MissionView)=>["queued","planning","executing","verifying"].includes(m.state);
const labels:Record<string,string>={waiting:"필요한 연결이나 답변을 기다리는 중",queued:"차례를 기다리는 중",planning:"계획을 세우는 중",executing:"자료를 확인하는 중",verifying:"결과를 확인하는 중",achieved:"선택한 확인 기준을 통과했어요",partial:"일부 결과 · 검토 필요",missing_evidence:"근거를 확인하지 못했어요",failed:"완료하지 못했어요",unsupported:"지원하지 않는 실행이 필요해요",unknown:"실행 결과를 확인할 수 없어요",paused:"잠시 멈춤",cancelled:"취소됨",quarantined:"다시 시작이 필요해요"};
const scheduleLabels:Record<string,string>={active:"실행 예정",paused:"잠시 멈춤",completed:"완료",expired:"기간 만료",revoked:"삭제됨"};
const scheduleLabel=(state:string)=>scheduleLabels[state]??"상태 확인 필요";
const reportText=(s:string)=>s==="Additional constraints need owner review"?"추가 조건은 직접 검토가 필요해요.":s.includes("unsupported action")?"이 작업에는 현재 지원하지 않는 실행이 필요해요. 자료 읽기와 계산 범위로 구체화해 새 일로 맡겨 주세요.":s.startsWith("Check ")?"처음 요청한 확인 기준을 모두 충족하지 못했어요. 결과를 검토해 주세요.":s.startsWith("Missing receipt")?"제시된 근거를 실제 실행 기록에서 찾지 못했어요.":s.startsWith("Receipt ")||s.startsWith("A tool failed")?"일부 자료를 읽거나 계산하지 못했어요. 요청을 좁혀 다시 맡길 수 있어요.":s.startsWith("No current-run")?"결과를 뒷받침할 근거를 확인하지 못했어요.":s.includes("expired")?"요청 유효 기간이 끝났어요. 결과를 검토하거나 새 일로 맡겨 주세요.":s.includes("unknown outcome")?"진행 중이던 실행 결과를 확인할 수 없어요. 새 일로 맡겨 주세요.":"추가 확인이 필요해요. 결과와 근거를 검토해 주세요.";
function modelSettingsError(e:unknown) {
 if(!(e instanceof ApiFailure))return null;
 if(e.code==="tools_required")return "맡긴 일에는 도구가 필요해요. 내 모델에서 도구를 확인하거나 도구 사용 가능한 모델을 저장해 주세요.";
 if(e.code==="model_required")return "사용할 모델이 없어요. 내 모델에서 연결과 모델 선택을 확인해 주세요.";
 return null;
}
export function Missions(props:Props){return <Contents key={props.accountId} {...props}/>}
function Contents({accountId,onExpired,back,instructionDrafts,onInstruction}:Props){
 const scope=useScope(accountId,"missions");const [items,setItems]=useState<MissionView[]>([]);const [task,setTask]=useState("");const [constraints,setConstraints]=useState("");const [review,setReview]=useState("");const [expected,setExpected]=useState("");const [durable,setDurable]=useState(true);const [steering,setSteering]=useState<Record<string,string>>({});const [busy,setBusy]=useState(false);const [error,setError]=useState("");
 const [reportTitle,setReportTitle]=useState("");const [modelSettingsNeeded,setModelSettingsNeeded]=useState(false);
 const [editing,setEditing]=useState<string|null>(null);
 const id=useRef(crypto.randomUUID());const pending=useRef<MissionCreate|null>(null);const control=useRef<{id:string;input:MissionControl}|null>(null);const alive=useRef(true);const reading=useRef<Promise<MissionView[]>|null>(null),generation=useRef(0);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++}},[]);
 function failure(e:unknown){if(!alive.current||isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}const modelError=modelSettingsError(e);setModelSettingsNeeded(!!modelError);setError(modelError??(e instanceof ApiFailure&&e.status===409?"내용이 바뀌었어요. 새로고침 후 현재 상태를 확인해 주세요.":"처리 결과를 확인하지 못했어요. 새로고침 후 같은 요청을 다시 확인해 주세요."))}
 // Confirmed writes need an inventory started after the write, not an older poll.
 async function load(fresh=false):Promise<MissionView[]>{
  if(fresh){generation.current++;await reading.current?.catch(()=>{})}
  if(reading.current)return reading.current;
  const version=generation.current;
  const work=scope.request<MissionView[]>("/missions").then(v=>{
   if(alive.current&&version===generation.current){const old=control.current;if(old&&v.find(m=>m.id===old.id)?.revision!==old.input.expected_revision)control.current=null;setItems(v)}return v;
  });reading.current=work;
  try{return await work}finally{if(reading.current===work)reading.current=null}
 }
 useEffect(()=>{const read=()=>void load().catch(failure);read();const timer=setInterval(read,2500);return()=>clearInterval(timer)},[scope]);
 const focused=useRef("");
 useEffect(()=>{const target=window.location.hash.slice(1);if(focused.current===target||!items.some(m=>`mission-${m.id}`===target))return;const element=document.getElementById(target);if(element){focused.current=target;element.scrollIntoView?.({block:"start"});element.focus({preventScroll:true})}},[items]);
 async function create(e:FormEvent){e.preventDefault();if(busy)return;setBusy(true);setError("");const checks:MissionCheck[]=[...(reportTitle.trim()?[{kind:"private_report" as const,title:reportTitle.trim()}]:[]),...(expected.trim()?[{kind:"decimal_equals" as const,expected:expected.trim()}]:[]),...((review.trim()||constraints.trim()||!expected.trim())?[{kind:"user_review" as const,description:review.trim()||"요청과 제약 조건을 충족하는지 직접 검토"}]:[])];pending.current??={request_id:id.current,task,constraints,checks,execution_mode:durable?"durable":"current_session"};try{await scope.request<MissionView>("/missions","POST",pending.current);if(!alive.current)return;pending.current=null;id.current=crypto.randomUUID();setTask("");setConstraints("");setReview("");setExpected("");setReportTitle("");await load(true)}catch(e){failure(e)}finally{if(alive.current)setBusy(false)}}
 async function act(m:MissionView,kind:"pause"|"resume"|"cancel"|"steer"){
  if(busy)return;setBusy(true);setError("");
  const action=kind==="steer"?{kind,instruction:steering[m.id]??""}:{kind};
  try{
   // A poll is observation, not permission to replay a different old intent.
   // CAS resolves an earlier uncertain effect racing this fresh read.
   for(let attempt=0;attempt<2;attempt++){
    const current=(await load(true)).find(v=>v.id===m.id);if(!alive.current||!current)return;
    const eligible=kind==="resume"||kind==="steer"?current.state==="paused"&&!current.uncertain:kind==="pause"?active(current):active(current)||current.state==="paused"||current.state==="waiting";
    if(!eligible)return;
    const old=control.current;
    const same=old?.id===m.id&&old.input.expected_revision===current.revision&&JSON.stringify(old.input.action)===JSON.stringify(action);
    const operation=same?old!:{id:m.id,input:{request_id:crypto.randomUUID(),expected_revision:current.revision,action}};
    control.current=operation;
    try{await scope.request(`/missions/${operation.id}/control`,"POST",operation.input);if(control.current===operation)control.current=null;await load(true);return}
    catch(e){
     if(e instanceof ApiFailure&&e.status>=400&&e.status<500){if(control.current===operation)control.current=null;if(e.status===409&&attempt===0)continue}
     throw e;
    }
   }
  }catch(e){failure(e)}finally{if(alive.current)setBusy(false)}
 }

 const draft=()=>{if(!busy){pending.current=null;id.current=crypto.randomUUID()}};
 return <section className="missions"><header><button onClick={back}>← 대화로</button><h2>맡긴 일</h2><p>Gmail·메일 또는 Calendar·캘린더·일정을 명시하면 선택한 계정으로 읽고, 요청한 메일 초안과 기본 개인 일정 생성·날짜 변경을 연결해요. 연결이 없으면 원래 카드에서 기다리고 계정이 여러 개면 한 번 선택해요. 메일은 최종 확인 화면에서 매번 허락한 뒤 발송해요.</p><p>자료를 읽고 결과와 근거를 남겨요. 보관된 자료·공개 웹 읽기와 계산을 사용할 수 있어요.</p></header>
 <form onSubmit={create}><label>맡길 일<textarea required maxLength={8000} value={task} onChange={e=>{setTask(e.target.value);draft()}}/></label><label>지켜야 할 조건<textarea maxLength={4000} value={constraints} onChange={e=>{setConstraints(e.target.value);draft()}}/></label><label>완료 후 확인할 내용<input value={review} maxLength={1000} onChange={e=>{setReview(e.target.value);draft()}}/></label><details><summary>계산 결과를 정확히 확인할 때</summary><label>예상하는 정확한 값<input value={expected} maxLength={128} onChange={e=>{setExpected(e.target.value);draft()}}/></label><p>계산 영수증과 결과가 모두 일치해야 완료로 표시해요. 일반 내용 검토는 직접 확인이 필요해요.</p></details><label>저장할 비공개 보고서 제목 (선택)<input maxLength={200} value={reportTitle} onChange={e=>{setReportTitle(e.target.value);draft()}}/></label><p>제목을 입력하면 실제 읽기 근거와 계산 결과를 연결해 문서 하나를 저장해요. 내용의 정확성과 누락은 직접 검토해 주세요.</p><label className="mission-mode"><input type="checkbox" checked={durable} onChange={e=>{setDurable(e.target.checked);draft()}}/>창을 닫거나 로그아웃해도 계속</label><p>이 요청에 한해 최대 하루 동안 실행해요. 멈춘 시간도 유효 기간에 포함돼요.</p><button disabled={busy||!task.trim()} type="submit">일 맡기기</button></form>
 <details><summary>예약해서 맡기기</summary><RecurringMissions accountId={accountId} onExpired={onExpired}/></details>
 {error&&<p role="alert">{error}{modelSettingsNeeded&&<> <a href="/settings/model" target="_blank" rel="noreferrer">내 모델 설정</a></>}</p>}<button disabled={busy} onClick={()=>void load().catch(failure)}>상태 새로고침</button>
 <TaskMonitors accountId={accountId} onExpired={onExpired}/>
 {items.map(m=><article id={`mission-${m.id}`} tabIndex={-1} key={m.id}><h3>{m.request.task}</h3><p role="status">{labels[m.state]??m.state}</p><MissionInstructions accountId={accountId} mission={m} state={instructionDrafts?.[m.id]} update={onInstruction?change=>onInstruction(m.id,change):undefined} onExpired={onExpired}/>{m.input_wait&&<MissionInput key={m.input_wait.request_id} mission={m} scope={scope} changed={()=>load(true)} failure={failure}/>}{m.wait&&<ConnectionWait mission={m} scope={scope} changed={()=>load(true)} failure={failure}/ >}{m.acceptance_history&&<details><summary>처음 요청과 조건 변경 이력</summary>{m.acceptance_history.map(v=><div key={v.revision}><strong>확인 기준 {v.revision}</strong><p>{v.request.task}</p><p>{v.request.constraints}</p><ul>{v.request.checks.map((c,i)=><li key={i}>{c.kind==="decimal_equals"?`정확한 값 ${c.expected}`:c.kind==="user_review"?c.description:c.kind==="source_quote"?c.quote:c.kind==="private_report"?`비공개 보고서 ${c.title}`:c.text}</li>)}</ul></div>)}</details>}<ol>{m.plan.map((s,i)=><li key={i}>{s}</li>)}</ol>{m.result&&<div className="mission-result"><p>{m.result.text}</p></div>}{m.compound?.output&&<p><a href={m.compound.output.url}>보고서 열기 · 버전 {m.compound.output.revision}</a></p>}{m.compound?.output&&["partial","achieved"].includes(m.state)&&<AdoptReport accountId={accountId} mission={m} onExpired={onExpired}/>} {m.compound?.review_required&&m.compound.output&&<p>문서를 저장했어요. 출처별 주장·누락·충돌과 비교 내용은 검토가 필요해요.</p>}{m.actions?.map(a=><p key={a.id}><a href={a.url}>{a.kind==="mail"?"메일 최종 확인 열기":"Calendar 작업 열기"}</a> · {a.current?({pending_approval:"초안 저장 · 발송 허락 대기",approved:"발송 허락됨",sent:"Gmail 접수 확인 · 배달 여부는 별도",applied:"일정 변경 확인",outcome_unknown:"결과 불명 · 자동 재실행 없음",dispatch_committed:"결과 확인 중"}[a.state]??a.state):"준비한 확인본과 현재 수정본이 달라요"}{a.kind==="mail"&&a.provider_receipt&&a.state!=="sent"&&" · Gmail 초안 저장 확인"}</p>)}{m.actions?.some(a=>a.kind==="mail"&&a.current&&["pending_approval","approved","sent"].includes(a.state))&&["partial","achieved"].includes(m.state)&&<AdoptDraft accountId={accountId} mission={m} onExpired={onExpired}/>}{m.receipts?.length>0&&<details><summary>실제로 확인한 근거</summary>{m.receipts.map((r,i)=><div key={i}><strong>{r.label}</strong><pre>{r.detail}</pre></div>)}</details>}{m.report.length>0&&<ul>{[...new Set(m.report.map(reportText))].map((s,i)=><li key={i}>{s}</li>)}</ul>}{m.uncertain&&<p>이미 시작한 실행은 결과를 확인할 수 없어 이어서 실행하지 않아요. 내용을 확인한 뒤 새 일로 맡겨 주세요.</p>}{active(m)&&<button disabled={busy} onClick={()=>void act(m,"pause")}>잠시 멈추기</button>}{m.state==="paused"&&!m.uncertain&&<><button disabled={busy} onClick={()=>void act(m,"resume")}>이어서 진행</button><label>추가 지시<input value={steering[m.id]??""} onChange={e=>setSteering({...steering,[m.id]:e.target.value})}/></label><button disabled={busy||!steering[m.id]?.trim()} onClick={()=>void act(m,"steer")}>추가 지시하고 진행</button><p>추가 지시는 현재 조건과 남은 실행 한도를 바꾸지 않아요.</p><button disabled={busy} onClick={()=>setEditing(m.id)}>조건과 확인 기준 변경</button></>}{editing===m.id&&<AcceptanceEditor key={m.id} mission={m} scope={scope} onExpired={onExpired} saved={async()=>{setEditing(null);await load(true)}} close={()=>setEditing(null)}/>}{(active(m)||m.state==="paused"||m.state==="waiting")&&<button disabled={busy} onClick={()=>void act(m,"cancel")}>취소</button>}{!active(m)&&m.state!=="paused"&&m.state!=="waiting"&&m.state!=="achieved"&&<p>결과를 검토하거나 조건을 구체화해 새 일로 맡겨 주세요.</p>}<button disabled={busy} onClick={async()=>{setBusy(true);try{await scope.request(`/missions/${m.id}`,"DELETE");await load(true)}catch(e){failure(e)}finally{if(alive.current)setBusy(false)}}}>기록 삭제</button></article>)}
 </section>
}

function AcceptanceEditor({mission,scope,onExpired,saved,close}:{mission:MissionView;scope:ReturnType<typeof useScope>;onExpired:()=>void;saved:()=>Promise<void>;close:()=>void}){
 const baseline=useRef(mission);const [task,setTask]=useState(mission.request.task),[constraints,setConstraints]=useState(mission.request.constraints),[review,setReview]=useState(mission.request.checks.find(c=>c.kind==="user_review")?.description??""),[expected,setExpected]=useState(mission.request.checks.find(c=>c.kind==="decimal_equals")?.expected??"");
 const [busy,setBusy]=useState(false),[error,setError]=useState("");const pending=useRef<MissionAcceptanceUpdate|null>(null);
 function change(){pending.current=null}
 async function save(e:FormEvent){e.preventDefault();if(busy)return;setBusy(true);setError("");
  const retained=baseline.current.request.checks.filter(c=>c.kind==="source_quote"||c.kind==="output_contains"||c.kind==="private_report");
  pending.current??={request_id:crypto.randomUUID(),expected_revision:baseline.current.revision,task,constraints,checks:[...retained,...(expected.trim()?[{kind:"decimal_equals" as const,expected:expected.trim()}]:[]),...(review.trim()||constraints.trim()||(!expected.trim()&&!retained.length)?[{kind:"user_review" as const,description:review.trim()||"요청과 조건을 직접 검토"}]:[])]};
  try{await scope.request(`/missions/${mission.id}/acceptance`,"PUT",pending.current);pending.current=null;await saved()}
  catch(e){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}if(e instanceof ApiFailure&&e.status>=400&&e.status<500)pending.current=null;setError(e instanceof ApiFailure&&e.status===409?"현재 상태가 바뀌었어요. 입력은 보존했어요. 상태를 확인하고 편집을 다시 열어 주세요.":e instanceof ApiFailure&&e.status===403?"처음 맡긴 읽기 범위를 벗어나거나 현재 권한을 사용할 수 없어요. 범위를 유지해 조건을 수정해 주세요.":"조건을 저장하지 못했어요. 입력을 보존했으니 같은 내용으로 다시 시도할 수 있어요.")}
  finally{setBusy(false)}
 }
 return <form aria-label="미션 조건 변경" onSubmit={save}><p>저장하면 새 기준으로 다시 계획해요. 이미 쓴 호출·보완 횟수와 종료 시각은 유지돼요. 연결 계정이나 도구 권한은 늘리지 않아요.</p><label>변경할 일<textarea required disabled={busy} maxLength={8000} value={task} onChange={e=>{setTask(e.target.value);change()}}/></label><label>변경할 조건<textarea disabled={busy} maxLength={4000} value={constraints} onChange={e=>{setConstraints(e.target.value);change()}}/></label><label>변경할 확인 내용<input disabled={busy} maxLength={1000} value={review} onChange={e=>{setReview(e.target.value);change()}}/></label><label>변경할 정확한 값<input disabled={busy} maxLength={128} value={expected} onChange={e=>{setExpected(e.target.value);change()}}/></label>{baseline.current.request.checks.some(c=>c.kind==="source_quote"||c.kind==="output_contains")&&<p>기존 원문 인용·필수 문구 확인 기준도 유지해요.</p>}{error&&<p role="alert">{error}</p>}<button disabled={busy||!task.trim()||mission.state!=="paused"} type="submit">새 조건으로 다시 계획</button>{mission.state!=="paused"&&<p>미션 상태가 바뀌었어요. 입력은 보존했어요. 현재 상태를 확인한 뒤 정지하고 편집을 다시 열어 주세요.</p>}<button disabled={busy} type="button" onClick={close}>편집 닫기</button></form>
}

function RecurringMissions({accountId,onExpired}:Pick<Props,"accountId"|"onExpired">){
 const scope=useScope(accountId,"recurring-missions");
 const [items,setItems]=useState<RecurringMissionView[]>([]),[task,setTask]=useState(""),[constraints,setConstraints]=useState(""),[expected,setExpected]=useState(""),[title,setTitle]=useState("예약한 읽기 업무");
 const [kind,setKind]=useState<ScheduleRule["kind"]>("daily"),[instant,setInstant]=useState(""),[clock,setClock]=useState("09:00"),[zone,setZone]=useState(Intl.DateTimeFormat().resolvedOptions().timeZone||"UTC"),[seconds,setSeconds]=useState(3600),[days,setDays]=useState([1]),[changes,setChanges]=useState(false);
 const [busy,setBusy]=useState(false),[error,setError]=useState(""),[editing,setEditing]=useState<RecurringMissionView|null>(null);const [modelSettingsNeeded,setModelSettingsNeeded]=useState(false);
 const pending=useRef<{path:string;method:"POST"|"PUT";body:RecurringMissionCreate|ScheduleEdit}|null>(null),control=useRef<{id:string;input:ScheduleControl}|null>(null),alive=useRef(true),generation=useRef(0);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++}},[]);
 function failure(e:unknown){if(!alive.current||isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}const modelError=modelSettingsError(e);setModelSettingsNeeded(!!modelError);setError(modelError??(e instanceof ApiFailure&&e.status===409?"예약 내용이 바뀌었어요. 현재 상태를 확인한 뒤 다시 조작해 주세요.":"처리 결과를 확인하지 못했어요. 같은 내용으로 다시 시도할 수 있어요."))}
 async function load(){const g=++generation.current;const raw=await scope.request<RecurringMissionView[]>("/recurring-missions");if(alive.current&&g===generation.current)setItems(Array.isArray(raw)?raw.filter(v=>v.schedule&&v.template):[])}
 useEffect(()=>{void load().catch(failure);const timer=setInterval(()=>void load().catch(failure),5000);return()=>clearInterval(timer)},[scope]);
 function draft(){if(!busy)pending.current=null;}
 function rule():ScheduleRule {
  switch(kind){
   case "once":return {kind,at:Math.floor(new Date(`${instant}Z`).getTime()/1000)};
   case "interval":return {kind,anchor:Math.floor(new Date(`${instant}Z`).getTime()/1000),seconds};
   case "daily":return {kind,time:clock,timezone:zone};
   case "weekly":return {kind,days,time:clock,timezone:zone};
  }
 }
 async function save(e:FormEvent){e.preventDefault();if(busy)return;setBusy(true);setError("");
  try{
   if(!pending.current){
    const schedule={title,message:"예약한 읽기 업무",rule:rule(),valid_until:null};
    if(editing)pending.current={path:`/recurring-missions/${editing.schedule.id}`,method:"PUT",body:{expected_revision:editing.schedule.revision,draft:schedule}};
    else {
     const checks:MissionCheck[]=[...(expected.trim()?[{kind:"decimal_equals" as const,expected:expected.trim()}]:[]),...(!expected.trim()||constraints.trim()?[{kind:"user_review" as const,description:"요청과 조건을 충족하는지 직접 검토"}]:[])];
     pending.current={path:"/recurring-missions",method:"POST",body:{request_id:crypto.randomUUID(),template:{request_id:crypto.randomUUID(),task,constraints,checks,execution_mode:"durable"},schedule,changes_only:changes}};
    }
   }
   const p=pending.current;await scope.request(p.path,p.method,p.body);if(!alive.current)return;
   pending.current=null;setEditing(null);setTask("");setConstraints("");setExpected("");await load();
  }catch(e){failure(e)}finally{if(alive.current)setBusy(false)}
 }
 async function act(m:RecurringMissionView,action:string){if(busy)return;setBusy(true);setError("");
  const old=control.current;const operation=old?.id===m.schedule.id&&old.input.action===action&&old.input.expected_revision===m.schedule.revision?old:{id:m.schedule.id,input:{expected_revision:m.schedule.revision,action}};
  control.current=operation;
  try{await scope.request(`/recurring-missions/${operation.id}/control`,"POST",operation.input);if(!alive.current)return;control.current=null;await load()}
  catch(e){if(e instanceof ApiFailure&&e.status>=400&&e.status<500){control.current=null;void load().catch(failure)}failure(e)}finally{if(alive.current)setBusy(false)}
 }
 function edit(m:RecurringMissionView){if(busy)return;draft();setEditing(m);setTask(m.template.task);setConstraints(m.template.constraints);setTitle(m.schedule.title);setChanges(m.changes_only);setExpected(m.template.checks.find(c=>c.kind==="decimal_equals")?.expected??"");
  const r=m.schedule.rule;setKind(r.kind);if(r.kind==="once"||r.kind==="interval")setInstant(new Date((r.kind==="once"?r.at:r.anchor)*1000).toISOString().slice(0,16));if(r.kind==="interval")setSeconds(r.seconds);if(r.kind==="daily"||r.kind==="weekly"){setClock(r.time);setZone(r.timezone)}if(r.kind==="weekly")setDays(r.days);
 }
 return <section aria-label="예약한 읽기 업무"><p>업무에 명시한 Gmail·Calendar는 연결된 내 계정에서 매 회차 새로 읽어요. 연결 철회 후에는 재연결하고 새 업무로 맡겨야 해요. 요청한 로컬 메일 초안을 준비할 수 있으며 발송 허락은 매번 별도예요. Calendar 변경과 자동 발송은 예약하지 않아요.</p><p>자료 읽기와 계산을 예약해요. 저장한 일과 모델은 고정되며 로그아웃해도 실행해요. 매 회차는 예정 시각부터 최대 하루와 원래 실행 한도를 사용해요.</p>
 <form onSubmit={save}><label>예약 이름<input required maxLength={160} disabled={busy} value={title} onChange={e=>{setTitle(e.target.value);draft()}}/></label>
 <label>예약할 일<textarea required maxLength={8000} disabled={busy||!!editing} value={task} onChange={e=>{setTask(e.target.value);draft()}}/></label>
 <label>예약 업무의 조건<textarea maxLength={4000} disabled={busy||!!editing} value={constraints} onChange={e=>{setConstraints(e.target.value);draft()}}/></label>
 <label>예약 계산의 정확한 예상값<input maxLength={128} disabled={busy||!!editing} value={expected} onChange={e=>{setExpected(e.target.value);draft()}}/></label>
 <label>실행 주기<select disabled={busy} value={kind} onChange={e=>{setKind(e.target.value as ScheduleRule["kind"]);draft()}}><option value="once">한 번</option><option value="interval">일정 간격</option><option value="daily">매일</option><option value="weekly">매주</option></select></label>
 {(kind==="once"||kind==="interval")&&<label>예약 기준 시각 (UTC)<input required type="datetime-local" disabled={busy} value={instant} onChange={e=>{setInstant(e.target.value);draft()}}/></label>}
 {kind==="interval"&&<label>간격 (초)<input required type="number" min={60} max={31536000} disabled={busy} value={seconds} onChange={e=>{setSeconds(Number(e.target.value));draft()}}/></label>}
 {(kind==="daily"||kind==="weekly")&&<><label>예약 시간<input required type="time" disabled={busy} value={clock} onChange={e=>{setClock(e.target.value);draft()}}/></label><label>예약 시간대<input required disabled={busy} value={zone} onChange={e=>{setZone(e.target.value);draft()}}/></label></>}
 {kind==="weekly"&&<fieldset><legend>실행 요일</legend>{["월","화","수","목","금","토","일"].map((d,i)=><label key={d}><input type="checkbox" disabled={busy} checked={days.includes(i+1)} onChange={e=>{setDays(e.target.checked?[...days,i+1].sort():days.filter(v=>v!==i+1));draft()}}/>{d}</label>)}</fieldset>}
 <label><input type="checkbox" disabled={busy||!!editing} checked={changes} onChange={e=>{setChanges(e.target.checked);draft()}}/>정확히 같은 확인 결과는 소식함 알림 생략</label><p>성공한 계산·원문 근거와 결과 본문의 정확한 일치만 비교해요. 문장의 의미가 같은지 판단하지 않으며, 검토 필요·실패·결과 불명과 비교할 수 없는 근거는 알림을 남겨요. 실행 기록은 항상 남아요.</p>
 <button disabled={busy||!task.trim()||kind==="weekly"&&days.length===0}>{editing?"예약 시간 저장":"업무 예약 저장"}</button>{editing&&<button type="button" disabled={busy} onClick={()=>{setEditing(null);pending.current=null;setTask("");setConstraints("");setExpected("")}}>예약 편집 닫기</button>}</form>
 {error&&<p role="alert">{error}{modelSettingsNeeded&&<> <a href="/settings/model" target="_blank" rel="noreferrer">내 모델 설정</a></>}</p>}
 {items.map(m=><article key={m.schedule.id}><h3>{m.schedule.title}</h3><p>{m.template.task}</p><p>예약 상태: {m.schedule.reason==="needs_skill"?(m.missing_skills?.length?"필요한 스킬이 꺼져 있거나 원래 버전을 사용할 수 없음":"필요한 스킬이 복구됨 · 직접 재개 필요"):scheduleLabel(m.schedule.state)} · {m.model.provider_name} / {m.model.model}{m.model.effort?` / ${m.model.effort}`:""}</p>{m.schedule.reason==="needs_skill"&&<p>필요한 스킬: {m.missing_skills?.join(", ")||"원래 읽은 지침"}. 제안·스킬에서 원래 버전을 다시 활성화한 뒤 다음 회차부터 재개하세요. 놓친 회차는 다시 실행하지 않습니다.</p>}<p>다음 회차: {m.schedule.next_fire_at?new Date(m.schedule.next_fire_at*1000).toLocaleString():"남은 예정 시각 없음"}</p>
 {m.schedule.state==="active"&&<button disabled={busy} onClick={()=>void act(m,"pause")}>예약 멈추기</button>}{m.schedule.state==="paused"&&<button disabled={busy} onClick={()=>void act(m,"resume")}>다음 회차부터 재개</button>}<button disabled={busy} onClick={()=>edit(m)}>예약 시간 변경</button><button disabled={busy} onClick={()=>void act(m,"revoke")}>업무 예약 삭제</button>
 <details><summary>회차와 건너뛴 기록</summary><ul>{m.history?.occurrences.filter(o=>o.state!=="delivered").map(o=><li key={o.id}>{o.local_slot} · {o.state} · {o.reason??""}</li>)}{m.history?.skipped_ranges.map((r,i)=><li key={i}>{r.from_slot} ~ {r.to_slot} · {r.count}회 건너뜀</li>)}</ul></details>
 <ul>{m.recent.map(o=><li key={o.occurrence_id}>{new Date(o.scheduled_for*1000).toLocaleString()} · {labels[o.state]??o.state}{o.exact_unchanged?" · 정확한 결과가 같아 알림 생략":""} <a href={`#mission-${o.mission_id}`}>실행 기록</a></li>)}</ul></article>)}
 </section>
}

function ConnectionWait({mission,scope,changed,failure}:{mission:MissionView;scope:ReturnType<typeof useScope>;changed:()=>Promise<MissionView[]>;failure:(e:unknown)=>void}){
 const wait=mission.wait!;const [busy,setBusy]=useState(false);const pending=useRef<{request_id:string;wait_request_id:string;expected_revision:number;slot_id:string;binding_id:string}|null>(null);
 if(wait.state==="resumed")return null;
 async function choose(binding:string){if(busy)return;setBusy(true);if(pending.current?.binding_id!==binding)pending.current={request_id:crypto.randomUUID(),wait_request_id:wait.request_id,expected_revision:mission.revision,slot_id:wait.slot_id,binding_id:binding};try{await scope.request(`/missions/${mission.id}/connection`,"POST",pending.current);pending.current=null;await changed()}catch(e){if(e instanceof ApiFailure&&e.status>=400&&e.status<500)pending.current=null;failure(e)}finally{setBusy(false)}}
 return <section aria-label="업무 연결 대기"><p role="status">{wait.state==="expired"?"만료":wait.state==="cancelled"?"연결 대기 종료":wait.state==="ready"?"연결 준비됨 · 앞선 업무가 끝나면 이어서 진행해요":wait.state==="committed"?"신뢰한 기기에서 조회 중 · 원래 응답을 기다리는 중":wait.cause==="vault_unlock"?"신뢰한 기기에서 볼트 잠금 해제":"계정 선택"}</p><p>원래 종료 시각: {new Date(wait.expires_at_ms).toLocaleString()}</p>{wait.state==="waiting"&&(wait.choices.length?<><p>이번 업무에서 읽을 계정 하나를 선택하세요.</p>{wait.choices.map(c=><div key={c.binding_id}><button disabled={busy} onClick={()=>void choose(c.binding_id)}>{c.alias||`Gmail 연결 ${c.binding_id.slice(-8)}`}</button><button disabled={busy} onClick={async()=>{const alias=window.prompt("직접 지정할 연결 별칭",c.alias??"");if(alias===null)return;setBusy(true);try{await scope.request("/connections/alias","PUT",{binding_id:c.binding_id,alias});await changed()}catch(e){failure(e)}finally{setBusy(false)}}}>별칭 지정</button></div>)}</>:<><p>{wait.slot_id==="native-mail-slot-v1"?"신뢰한 기기의 별도 모델 읽기 권한이 필요해요. 기기에서 연결하거나 볼트를 열면 같은 업무에서 이어서 진행해요.":"Gmail 읽기 연결이 필요해요. 연결을 마치면 같은 업무에서 이어서 진행해요."}</p><a href={`${wait.slot_id==="native-mail-slot-v1"?"/settings/credentials":"/settings/mail"}?mission_return=${encodeURIComponent(mission.id)}`}>{wait.slot_id==="native-mail-slot-v1"?"기기 Gmail 읽기 연결 설정":"Gmail 연결 설정"}</a></>)}</section>
}
function AdoptReport({accountId,mission,onExpired}:{accountId:string;mission:MissionView;onExpired:()=>void}) {
 const scope=useScope(accountId,"compound-adoption");const [busy,setBusy]=useState(false),[message,setMessage]=useState("");
 async function adopt(){if(busy)return;const out=mission.compound?.output;if(!out)return;setBusy(true);setMessage("");try{await scope.request(`/missions/${mission.id}/adopt-report`,"POST",{artifact_id:out.id,artifact_revision:out.revision});setMessage("채택한 수치·원문·문서 결과를 검증했어요. 서로 다른 원문과 입력의 두 결과가 모이면 제안 화면에서 사용할 수 있어요. 내용 검토 표시는 유지돼요.")}catch(e){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}setMessage("이 결과는 현재 학습 조건을 충족하지 않아요. 새 원문을 읽은 정확 계산과 변경되지 않은 문서가 필요해요. 기존 절차 재실행·공유 예제·일반 완료 기록은 학습 증거가 되지 않아요.")}finally{setBusy(false)}}
 return <div><button disabled={busy} onClick={()=>void adopt()}>검토한 수치 결과를 채택해 절차 학습</button>{message&&<p role="status">{message}</p>}</div>
}

function AdoptDraft({accountId,mission,onExpired}:{accountId:string;mission:MissionView;onExpired:()=>void}) {
 const scope=useScope(accountId,"compound-draft-adoption");const [busy,setBusy]=useState(false),[message,setMessage]=useState("");
 async function adopt(){const action=mission.actions?.find(a=>a.kind==="mail"&&a.current&&["pending_approval","approved","sent"].includes(a.state));if(busy||!action)return;setBusy(true);setMessage("");try{await scope.request(`/missions/${mission.id}/adopt-draft`,"POST",{action_id:action.id,revision:action.revision,digest:action.digest});setMessage("로컬 초안의 원문·수치와 정확한 버전을 검증했어요. 서로 다른 새 결과 두 개가 모이면 절차를 사용할 수 있어요. 내용·받는 사람 검토와 매번의 발송 허락은 계속 필요해요.")}catch(e){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}setMessage("현재 학습 조건을 충족하지 않아요. 새 원문에 근거한 정확 계산과 변경되지 않은 원래 로컬 초안이 필요해요. 재실행·공유 예제·발송 결과는 새 학습 증거가 되지 않아요.")}finally{setBusy(false)}}
 return <div><p>초안의 내용과 받는 사람은 직접 검토해 주세요. 절차 학습은 발송 허락을 대신하지 않아요.</p><button disabled={busy} onClick={()=>void adopt()}>검토한 로컬 초안의 수치 결과를 채택해 절차 학습</button>{message&&<p role="status">{message}</p>}</div>
}
