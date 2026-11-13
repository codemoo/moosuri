import {useEffect,useRef,useState,type FormEvent} from "react";
import {useScope} from "./account-scope";
import {ApiFailure,isAborted} from "./api";
import type {MissionView,MissionCheck,MissionControl,MissionCreate} from "../shared/api-types";
import "./missions.css";
type Props={accountId:string;onExpired:()=>void;back:()=>void};
const active=(m:MissionView)=>["queued","planning","executing","verifying"].includes(m.state);
const labels:Record<string,string>={queued:"차례를 기다리는 중",planning:"계획을 세우는 중",executing:"자료를 확인하는 중",verifying:"결과를 확인하는 중",achieved:"선택한 확인 기준을 통과했어요",partial:"일부 결과 · 검토 필요",missing_evidence:"근거를 확인하지 못했어요",failed:"완료하지 못했어요",unsupported:"지원하지 않는 실행이 필요해요",unknown:"실행 결과를 확인할 수 없어요",paused:"잠시 멈춤",cancelled:"취소됨",quarantined:"다시 시작이 필요해요"};
const reportText=(s:string)=>s==="Additional constraints need owner review"?"추가 조건은 직접 검토가 필요해요.":s.includes("unsupported action")?"이 작업에는 현재 지원하지 않는 실행이 필요해요. 자료 읽기와 계산 범위로 구체화해 새 일로 맡겨 주세요.":s.startsWith("Check ")?"처음 요청한 확인 기준을 모두 충족하지 못했어요. 결과를 검토해 주세요.":s.startsWith("Missing receipt")?"제시된 근거를 실제 실행 기록에서 찾지 못했어요.":s.startsWith("Receipt ")||s.startsWith("A tool failed")?"일부 자료를 읽거나 계산하지 못했어요. 요청을 좁혀 다시 맡길 수 있어요.":s.startsWith("No current-run")?"결과를 뒷받침할 근거를 확인하지 못했어요.":s.includes("expired")?"요청 유효 기간이 끝났어요. 결과를 검토하거나 새 일로 맡겨 주세요.":s.includes("unknown outcome")?"진행 중이던 실행 결과를 확인할 수 없어요. 새 일로 맡겨 주세요.":"추가 확인이 필요해요. 결과와 근거를 검토해 주세요.";
export function Missions(props:Props){return <Contents key={props.accountId} {...props}/>}
function Contents({accountId,onExpired,back}:Props){
 const scope=useScope(accountId,"missions");const [items,setItems]=useState<MissionView[]>([]);const [task,setTask]=useState("");const [constraints,setConstraints]=useState("");const [review,setReview]=useState("");const [expected,setExpected]=useState("");const [durable,setDurable]=useState(true);const [steering,setSteering]=useState<Record<string,string>>({});const [busy,setBusy]=useState(false);const [error,setError]=useState("");
 const id=useRef(crypto.randomUUID());const pending=useRef<MissionCreate|null>(null);const control=useRef<{id:string;input:MissionControl}|null>(null);const alive=useRef(true);const reading=useRef<Promise<MissionView[]>|null>(null);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[]);
 function failure(e:unknown){if(!alive.current||isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}setError(e instanceof ApiFailure&&e.status===409?"내용이 바뀌었어요. 새로고침 후 현재 상태를 확인해 주세요.":"처리 결과를 확인하지 못했어요. 새로고침 후 같은 요청을 다시 확인해 주세요.")}
 async function load():Promise<MissionView[]>{
  if(reading.current)return reading.current;
  const work=scope.request<MissionView[]>("/missions").then(v=>{
   if(alive.current){const old=control.current;if(old&&v.find(m=>m.id===old.id)?.revision!==old.input.expected_revision)control.current=null;setItems(v)}return v;
  });reading.current=work;
  try{return await work}finally{if(reading.current===work)reading.current=null}
 }
 useEffect(()=>{const read=()=>void load().catch(failure);read();const timer=setInterval(read,2500);return()=>clearInterval(timer)},[scope]);
 async function create(e:FormEvent){e.preventDefault();if(busy)return;setBusy(true);setError("");const checks:MissionCheck[]=[...(expected.trim()?[{kind:"decimal_equals" as const,expected:expected.trim()}]:[]),...((review.trim()||constraints.trim()||!expected.trim())?[{kind:"user_review" as const,description:review.trim()||"요청과 제약 조건을 충족하는지 직접 검토"}]:[])];pending.current??={request_id:id.current,task,constraints,checks,execution_mode:durable?"durable":"current_session"};try{await scope.request<MissionView>("/missions","POST",pending.current);if(!alive.current)return;pending.current=null;id.current=crypto.randomUUID();setTask("");setConstraints("");setReview("");setExpected("");await load()}catch(e){failure(e)}finally{if(alive.current)setBusy(false)}}
 async function act(m:MissionView,kind:"pause"|"resume"|"cancel"|"steer"){
  if(busy)return;setBusy(true);setError("");
  const action=kind==="steer"?{kind,instruction:steering[m.id]??""}:{kind};
  try{
   // A poll is observation, not permission to replay a different old intent.
   // CAS resolves an earlier uncertain effect racing this fresh read.
   for(let attempt=0;attempt<2;attempt++){
    const current=(await load()).find(v=>v.id===m.id);if(!alive.current||!current)return;
    const eligible=kind==="resume"||kind==="steer"?current.state==="paused"&&!current.uncertain:kind==="pause"?active(current):active(current)||current.state==="paused";
    if(!eligible)return;
    const old=control.current;
    const same=old?.id===m.id&&old.input.expected_revision===current.revision&&JSON.stringify(old.input.action)===JSON.stringify(action);
    const operation=same?old!:{id:m.id,input:{request_id:crypto.randomUUID(),expected_revision:current.revision,action}};
    control.current=operation;
    try{await scope.request(`/missions/${operation.id}/control`,"POST",operation.input);if(control.current===operation)control.current=null;await load();return}
    catch(e){
     if(e instanceof ApiFailure&&e.status>=400&&e.status<500){if(control.current===operation)control.current=null;if(e.status===409&&attempt===0)continue}
     throw e;
    }
   }
  }catch(e){failure(e)}finally{if(alive.current)setBusy(false)}
 }

 const draft=()=>{if(!busy){pending.current=null;id.current=crypto.randomUUID()}};
 return <section className="missions"><header><button onClick={back}>← 대화로</button><h2>맡긴 일</h2><p>자료를 읽고 결과와 근거를 남겨요. 보관된 자료·공개 웹 읽기와 계산을 사용할 수 있어요.</p></header>
 <form onSubmit={create}><label>맡길 일<textarea required maxLength={8000} value={task} onChange={e=>{setTask(e.target.value);draft()}}/></label><label>지켜야 할 조건<textarea maxLength={4000} value={constraints} onChange={e=>{setConstraints(e.target.value);draft()}}/></label><label>완료 후 확인할 내용<input value={review} maxLength={1000} onChange={e=>{setReview(e.target.value);draft()}}/></label><details><summary>계산 결과를 정확히 확인할 때</summary><label>예상하는 정확한 값<input value={expected} maxLength={128} onChange={e=>{setExpected(e.target.value);draft()}}/></label><p>계산 영수증과 결과가 모두 일치해야 완료로 표시해요. 일반 내용 검토는 직접 확인이 필요해요.</p></details><label className="mission-mode"><input type="checkbox" checked={durable} onChange={e=>{setDurable(e.target.checked);draft()}}/>창을 닫거나 로그아웃해도 계속</label><p>이 요청에 한해 최대 하루 동안 실행해요. 멈춘 시간도 유효 기간에 포함돼요.</p><button disabled={busy||!task.trim()} type="submit">일 맡기기</button></form>
 {error&&<p role="alert">{error}</p>}<button disabled={busy} onClick={()=>void load().catch(failure)}>상태 새로고침</button>
 {items.map(m=><article key={m.id}><h3>{m.request.task}</h3><p role="status">{labels[m.state]??m.state}</p><ol>{m.plan.map((s,i)=><li key={i}>{s}</li>)}</ol>{m.result&&<div className="mission-result"><p>{m.result.text}</p></div>}{m.receipts?.length>0&&<details><summary>실제로 확인한 근거</summary>{m.receipts.map((r,i)=><div key={i}><strong>{r.label}</strong><pre>{r.detail}</pre></div>)}</details>}{m.report.length>0&&<ul>{[...new Set(m.report.map(reportText))].map((s,i)=><li key={i}>{s}</li>)}</ul>}{m.uncertain&&<p>이미 시작한 실행은 결과를 확인할 수 없어 이어서 실행하지 않아요. 내용을 확인한 뒤 새 일로 맡겨 주세요.</p>}{active(m)&&<button disabled={busy} onClick={()=>void act(m,"pause")}>잠시 멈추기</button>}{m.state==="paused"&&!m.uncertain&&<><button disabled={busy} onClick={()=>void act(m,"resume")}>이어서 진행</button><label>추가 지시<input value={steering[m.id]??""} onChange={e=>setSteering({...steering,[m.id]:e.target.value})}/></label><button disabled={busy||!steering[m.id]?.trim()} onClick={()=>void act(m,"steer")}>추가 지시하고 진행</button><p>처음 조건과 남은 실행 한도는 그대로 유지돼요.</p></>}{(active(m)||m.state==="paused")&&<button disabled={busy} onClick={()=>void act(m,"cancel")}>취소</button>}{!active(m)&&m.state!=="paused"&&m.state!=="achieved"&&<p>결과를 검토하거나 조건을 구체화해 새 일로 맡겨 주세요.</p>}<button disabled={busy} onClick={async()=>{setBusy(true);try{await scope.request(`/missions/${m.id}`,"DELETE");await load()}catch(e){failure(e)}finally{if(alive.current)setBusy(false)}}}>기록 삭제</button></article>)}
 </section>
}
