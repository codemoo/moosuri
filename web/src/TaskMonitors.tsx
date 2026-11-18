import "./task-monitors.css";
import {useEffect,useRef,useState} from "react";
import {useScope} from "./account-scope";
import {ApiFailure,isAborted} from "./api";
type Scope={kind:"exact_sources";source_ids:string[]}|{kind:"connected_thread";connection_id:string;config_revision:number;credential_epoch:number;thread_id:string};
type Observation={ordinal:number;mission_id:string;state:string;created_at:number;resolved_at:number|null;note:string|null};
export type TaskMonitor={id:string;parent_mission_id:string;linked_mission_ids:string[];revision:number;state:string;scope:Scope;sources:{id:string;title:string;revision:number}[];cadence_seconds:number|null;expires_at:number|null;reusable:boolean;reason:string|null;last_checked_at:number|null;existing_monitor_id?:string|null;observations:Observation[]};
const record=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==="object"&&!Array.isArray(v);
function validMonitor(v:unknown):v is TaskMonitor {
 if(!record(v)||typeof v.id!=="string"||typeof v.parent_mission_id!=="string"||typeof v.state!=="string"||!Number.isSafeInteger(v.revision)||!Array.isArray(v.linked_mission_ids)||!v.linked_mission_ids.every(id=>typeof id==="string")||!record(v.scope)||!["exact_sources","connected_thread"].includes(String(v.scope.kind)))return false;
 return Array.isArray(v.sources)&&v.sources.every(s=>record(s)&&typeof s.id==="string"&&typeof s.title==="string"&&Number.isSafeInteger(s.revision))&&Array.isArray(v.observations)&&v.observations.every(o=>record(o)&&Number.isSafeInteger(o.ordinal)&&typeof o.mission_id==="string"&&typeof o.state==="string"&&(o.resolved_at===null||Number.isSafeInteger(o.resolved_at))&&(o.note===null||typeof o.note==="string"));
}
type Props={accountId:string;missionId?:string;missionIds?:string[];onExpired:()=>void};
const results:Record<string,string>={queued:"확인 대기",planning:"확인 준비 중",executing:"자료 확인 중",verifying:"결과 확인 중",achieved:"변경 확인 완료",partial:"일부 결과 · 검토 필요",failed:"확인을 마치지 못했어요",unknown:"결과 확인 필요",cancelled:"확인 취소",quarantined:"자료 변경으로 재검토 필요",missing_evidence:"근거 확인 필요",paused:"잠시 멈춤",waiting:"연결 대기"};
const states:Record<string,string>={proposed:"후속 확인 제안",active:"허용한 범위 확인 중",paused:"후속 확인 일시 중지",stopped:"확인 중지",withdrawn:"권한 또는 자료 연결 철회",expired:"허용 기간 만료"};
const reasons:Record<string,string>={coverage_limited_needs_review:"변경 자료가 검토 한도를 넘었어요. 전체를 확인하지 않았으니 직접 검토해 주세요.",membership_removed_needs_review:"스레드 자료가 제거되었어요. 남은 자료를 다시 읽지 않았으니 직접 검토해 주세요.",permission_or_source_unavailable:"자료·연결·계정 권한 또는 유효 기간이 바뀌어 중지했어요.",permission_unavailable:"원래 허용한 권한을 더 사용할 수 없어 중지했어요."};
export function TaskMonitors(props:Props){return <Contents key={props.accountId} {...props}/>}
function Contents({accountId,missionId,missionIds,onExpired}:Props){
 const scope=useScope(accountId,"task-monitors");const [items,setItems]=useState<TaskMonitor[]>([]),[error,setError]=useState("");const alive=useRef(true),reading=useRef<Promise<void>|null>(null),generation=useRef(0);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++}},[]);
 function failure(e:unknown){if(!alive.current||isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}setError(e instanceof ApiFailure&&e.status===409?"확인 상태가 바뀌었어요. 새로고침 후 현재 제안을 확인해 주세요.":"처리 결과를 확인하지 못했어요. 같은 요청으로 다시 시도할 수 있어요.")}
 // Confirmed writes need an inventory started after the write, not an older poll.
 async function load(fresh=false){if(fresh){generation.current++;await reading.current?.catch(()=>{})}if(reading.current)return reading.current;const version=generation.current;const work=scope.request<unknown>("/task-monitors").then(v=>{if(!Array.isArray(v)||v.length>128||!v.every(validMonitor))throw new Error("Invalid monitor inventory");if(alive.current&&version===generation.current)setItems(v)});reading.current=work;try{await work}finally{if(reading.current===work)reading.current=null}}
 useEffect(()=>{void load().catch(failure);const timer=setInterval(()=>void load().catch(failure),5000);return()=>clearInterval(timer)},[scope]);
 const selected=missionIds??(missionId?[missionId]:null);const shown=items.filter(m=>!selected||selected.some(id=>m.parent_mission_id===id||m.linked_mission_ids.includes(id)));if(!shown.length&&!error)return null;
 return <section className="task-monitors" aria-label="완료한 일의 후속 확인"><h3>새 답장이나 변경이 있으면 알려드릴까요?</h3>{error&&<p role="alert">{error}</p>}{shown.map(m=><Monitor key={m.id} monitor={m} scope={scope} refresh={load} failure={failure}/>)}<button onClick={()=>void load().catch(failure)}>후속 확인 새로고침</button></section>
}
function Monitor({monitor:m,scope,refresh,failure}:{monitor:TaskMonitor;scope:ReturnType<typeof useScope>;refresh:(fresh?:boolean)=>Promise<void>;failure:(e:unknown)=>void}){
 const [busy,setBusy]=useState(false),[cadence,setCadence]=useState(m.cadence_seconds??300),[days,setDays]=useState(7),[reuse,setReuse]=useState(false),[notes,setNotes]=useState<Record<number,string>>({});
 const pending=useRef<{path:string;body:unknown}|null>(null);
 async function send(path:string,body:unknown){if(busy)return;setBusy(true);if(pending.current?.path!==path)pending.current={path,body};try{await scope.request(pending.current.path,"POST",pending.current.body);pending.current=null;await refresh(true)}catch(e){if(e instanceof ApiFailure&&e.status>=400&&e.status<500)pending.current=null;failure(e)}finally{setBusy(false)}}
 const draft=()=>{if(!busy)pending.current=null};
 return <article><p><strong>{states[m.state]??m.state}</strong> · <a href={`/app/missions#mission-${m.parent_mission_id}`}>원래 맡긴 일</a>{m.linked_mission_ids.length>1&&` · 같은 대화의 후속 확인에 연결됨`}</p>
 {m.scope.kind==="connected_thread"?<p>이 자료와 같은 대화에 나중에 들어오는 답장도 확인해요.</p>:<p>아래 저장 자료가 수정되면 확인해요. 새 답장은 감지할 수 없어요.</p>}
 <ul>{m.sources.map(s=><li key={s.id}>{s.title}</li>)}</ul>
 <details><summary>무엇을 확인하나요?</summary><p>허용한 저장 자료의 변화만 확인하고, 변경이 없으면 모델을 호출하지 않아요. 한 번에 확인할 수 있는 양을 넘으면 직접 검토가 필요하다고 알려드려요. 메일 발송이나 일정 변경은 하지 않아요.</p></details>
 {m.reason&&<p role="status">{reasons[m.reason]??(m.reason==="coalesced_existing_monitor"?"이미 진행 중인 후속 확인에 연결했어요. 기존 확인 간격과 종료 시각을 유지해요.":"후속 확인을 중지했어요.")}</p>}
 {m.last_checked_at&&<p>최근 자료 변경 확인: {new Date(m.last_checked_at*1000).toLocaleString()}</p>}
 {m.state==="proposed"&&<form onSubmit={e=>{e.preventDefault();void send(`/task-monitors/${m.id}/accept`,{request_id:crypto.randomUUID(),expected_revision:m.revision,cadence_seconds:cadence,expires_at:m.existing_monitor_id?m.expires_at:Math.floor(Date.now()/1000)+days*86400,reuse,read:true,check:true,notify:true})}}>
 {!m.existing_monitor_id&&<><label>확인 간격<select disabled={busy} value={cadence} onChange={e=>{setCadence(Number(e.target.value));draft()}}><option value={300}>5분</option><option value={3600}>1시간</option><option value={86400}>하루</option></select></label>
 <label>허용 기간<select disabled={busy} value={days} onChange={e=>{setDays(Number(e.target.value));draft()}}><option value={1}>1일</option><option value={7}>7일</option><option value={30}>30일</option></select></label></>}
 {m.existing_monitor_id&&<p>이미 진행 중인 후속 확인에 연결해요. 기존 확인 간격과 종료 시각을 유지해요.</p>}
 <label><input disabled={busy} type="checkbox" checked={reuse} onChange={e=>{setReuse(e.target.checked);draft()}}/>유효 기간 안에 같은 범위·같은 확인 방식으로 완료한 일에도 재사용</label>
 <p>시작하면 위 자료 읽기·변경 확인·알림을 허용해요. 재사용하면 같은 범위의 기존 확인 일정에 연결돼요.</p><button disabled={busy}>후속 확인 시작</button></form>}
 {["active","paused"].includes(m.state)&&<><p>{m.cadence_seconds===300?"5분":m.cadence_seconds===3600?"1시간":m.cadence_seconds===86400?"하루":`${Math.round((m.cadence_seconds??60)/60)}분`} 간격 · {m.expires_at&&new Date(m.expires_at*1000).toLocaleString()}까지 · {m.reusable?"같은 범위에서 재사용 허용":"이번 일에 허용"}</p><button disabled={busy} onClick={()=>void send(`/task-monitors/${m.id}/stop`,{expected_revision:m.revision,revoke_policy:true})}>후속 확인과 재사용 허용 철회</button></>}
 {m.observations.map(o=><div key={o.ordinal}><p><a href={`/app/missions#mission-${o.mission_id}`}>변경 확인 결과 {o.ordinal}</a> · {results[o.state]??"결과 확인 필요"}</p>{o.resolved_at?<p>확인했어요 · 추가 조치 없음{o.note&&` · ${o.note}`}</p>:<><label>다음 변경 확인에만 참고할 메모 (선택)<input maxLength={1000} disabled={busy} value={notes[o.ordinal]??""} onChange={e=>{setNotes({...notes,[o.ordinal]:e.target.value});draft()}}/></label><p>이 확인에만 남는 메모이며 권한이나 전역 기억을 늘리지 않아요.</p><button disabled={busy} onClick={()=>void send(`/task-monitors/${m.id}/observations/${o.ordinal}/resolve`,{note:notes[o.ordinal]??""})}>확인했어요 · 추가 조치 없음</button></>}</div>)}
 </article>
}
