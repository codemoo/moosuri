import {useEffect,useRef,useState} from "react";
import {useScope} from "./account-scope";
import {ApiFailure,isAborted} from "./api";
import "./mail-reply-watches.css";
type Target={id:string;title:string;mailbox:string};
type Reply={message_id:string;from:string[];subject:string;text:string;partial:boolean};
export type ReplyWatch={id:string;thread_id:string;title:string;mailbox:string;state:string;cadence_seconds:number;expires_at:number;last_checked_at:number|null;next_check_at:number|null;reason:string|null;replies:Reply[]};
type Inventory={targets:Target[];watches:ReplyWatch[]};
type Props={accountId:string;threadId?:string;onExpired:()=>void;refreshKey?:string};
const record=(v:unknown):v is Record<string,unknown>=>v!==null&&typeof v==="object"&&!Array.isArray(v);
const str=(v:unknown)=>typeof v==="string";
function inventory(v:unknown):v is Inventory {
 return record(v)&&Array.isArray(v.targets)&&v.targets.length<=32&&v.targets.every(t=>record(t)&&str(t.id)&&str(t.title)&&str(t.mailbox))&&Array.isArray(v.watches)&&v.watches.length<=128&&v.watches.every(w=>record(w)&&[w.id,w.thread_id,w.title,w.mailbox,w.state].every(str)&&Number.isSafeInteger(w.cadence_seconds)&&Number.isSafeInteger(w.expires_at)&&(w.last_checked_at===null||Number.isSafeInteger(w.last_checked_at))&&(w.next_check_at===null||Number.isSafeInteger(w.next_check_at))&&(w.reason===null||str(w.reason))&&Array.isArray(w.replies)&&w.replies.length<=4&&w.replies.every(r=>record(r)&&[r.message_id,r.subject,r.text].every(str)&&typeof r.partial==="boolean"&&Array.isArray(r.from)&&r.from.every(str)));
}
const statuses:Record<string,string>={active:"답장 기다리는 중",replied:"새 답장 도착 · 감시 완료",stopped:"감시 중지",expired:"감시 기간 종료",withdrawn:"연결 또는 권한 변경으로 중지",blocked:"감시 중단 · 확인 필요"};
const reasons:Record<string,string>={check_failed:"최근 조회에 실패했어요. 다시 확인할 예정이에요.",check_timed_out:"메일 조회가 지연되어 다시 확인할 예정이에요.",connection_unavailable:"메일 연결을 확인해 주세요.",thread_unavailable:"메일 대화를 더 이상 조회할 수 없어요.",anchor_unavailable:"기준 메일을 찾을 수 없어 감시를 멈췄어요.",coverage_unavailable:"메일 조회 범위를 확인할 수 없어 감시를 멈췄어요.",too_many_replies:"새 답장이 한 번에 너무 많이 왔어요. Gmail에서 직접 확인해 주세요."};
const time=(v:number)=>new Date(v*1000).toLocaleString();
export function MailReplyWatches(props:Props){return <Contents key={`${props.accountId}/${props.threadId??"all"}`} {...props}/>}
function Contents({accountId,threadId,onExpired,refreshKey}:Props){
 const scope=useScope(accountId,`mail-replies/${threadId??"all"}`);
 const [data,setData]=useState<Inventory>({targets:[],watches:[]}),[error,setError]=useState(""),[busy,setBusy]=useState(false),[cadence,setCadence]=useState(1800),[days,setDays]=useState(7);
 const alive=useRef(true),reading=useRef<Promise<void>|null>(null),generation=useRef(0),writing=useRef(false);
 const pending=useRef<{path:string;body:unknown;target:string}|null>(null);
 useEffect(()=>{alive.current=true;return()=>{alive.current=false;generation.current++}},[]);
 function failure(e:unknown){if(!alive.current||isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}setError(e instanceof ApiFailure&&e.status===409?"감시 상태나 대상이 바뀌었어요. 현재 카드를 확인해 주세요.":"감시 상태를 확인하지 못했어요. 표시된 정보가 최신이 아닐 수 있어요. 다시 시도해 주세요.")}
 async function load(){if(reading.current)return reading.current;const version=generation.current;const work=scope.request<unknown>(`/mail/reply-watches${threadId?`?thread_id=${encodeURIComponent(threadId)}`:""}`).then(v=>{if(!inventory(v))throw new Error("Invalid reply watch inventory");if(alive.current&&version===generation.current){setData(v);setError("")}});reading.current=work;try{await work}finally{if(reading.current===work)reading.current=null}}
 useEffect(()=>{void load().catch(failure);const timer=setInterval(()=>{if(!writing.current)void load().catch(failure)},5000);return()=>clearInterval(timer)},[scope,refreshKey]);
 async function send(target:string,path:string,body:unknown){if(writing.current)return;writing.current=true;setBusy(true);generation.current++;
 if(pending.current?.target!==target)pending.current={path,body,target};
 try{await scope.request(pending.current.path,"POST",pending.current.body);pending.current=null;await reading.current?.catch(()=>{});await load()}
 catch(e){if(e instanceof ApiFailure&&e.status>=400&&e.status<500)pending.current=null;failure(e)}
 finally{writing.current=false;if(alive.current)setBusy(false)}
 }
 if(!data.targets.length&&!data.watches.length&&!error)return null;
 return <section className="mail-reply-watches" aria-label="메일 답장 감시">
 {error&&<p role="alert">{error} <button disabled={busy} onClick={()=>void load().catch(failure)}>다시 확인</button></p>}
 {data.watches.map(w=><article className="reply-watch" key={w.id}>
 <p className="reply-watch-state"><span aria-hidden="true">{w.state==="active"?"◷":w.state==="replied"?"↳":"·"}</span> {statuses[w.state]??"상태 확인 필요"}</p>
 <h3>{w.title}</h3><p className="reply-watch-meta">{w.mailbox}</p>
 {w.state==="active"&&<p>{w.last_checked_at?`${time(w.last_checked_at)} 확인` : "첫 확인 대기 중"} · {Math.round(w.cadence_seconds/60)}분 간격</p>}
 {w.reason&&reasons[w.reason]&&<p role="status">{reasons[w.reason]}</p>}
 {w.state==="active"&&<details><summary>확인 일정</summary><p>다음 조회: {w.next_check_at?time(w.next_check_at):"대기 중"}<br/>종료: {time(w.expires_at)} 또는 첫 새 답장 도착</p><p>새 답장이 없으면 모델을 호출하지 않아요. Gmail API 조회만 발생해요.</p></details>}
 {w.replies.map(r=><div className="reply-preview" key={r.message_id}><p><strong>{r.from.join(", ")}</strong></p><p>{r.subject}</p><pre>{r.text||"표시할 본문이 없어요."}</pre>{r.partial&&<p className="reply-watch-meta">본문 일부 미리보기 · 첨부파일은 포함하지 않아요.</p>}</div>)}
 {!threadId&&<a href={`/app?thread=${encodeURIComponent(w.thread_id)}#mail-replies`}>원래 대화 열기</a>}
 {["active","blocked","replied"].includes(w.state)&&<button disabled={busy} onClick={()=>void send(`stop:${w.id}`,`/mail/reply-watches/${w.id}/stop`,{})}>{w.state==="replied"?"결과 닫기":"감시 중지"}</button>}
 </article>)}
 {threadId&&data.targets.length>0&&<details className="reply-watch-start"><summary>메일 답장 기다리기</summary><p>이 대화에서 확인한 메일 이후의 새 수신 메일을 읽고 알려드려요. 첫 답장을 찾으면 종료해요.</p>
 <div className="reply-watch-options"><label>확인 간격<select disabled={busy} value={cadence} onChange={e=>{setCadence(Number(e.target.value));pending.current=null}}><option value={300}>5분</option><option value={1800}>30분</option><option value={3600}>1시간</option></select></label><label>최대 기간<select disabled={busy} value={days} onChange={e=>{setDays(Number(e.target.value));pending.current=null}}><option value={1}>1일</option><option value={7}>7일</option><option value={30}>30일</option></select></label></div>
 {data.targets.map(t=><div className="reply-watch-target" key={t.id}><span><strong>{t.title}</strong><small>{t.mailbox}</small></span><button disabled={busy} onClick={()=>void send(`start:${t.id}`,"/mail/reply-watches",{request_id:crypto.randomUUID(),thread_id:threadId,target_id:t.id,cadence_seconds:cadence,duration_days:days})}>답장 기다리기</button></div>)}
 </details>}
 </section>
}
