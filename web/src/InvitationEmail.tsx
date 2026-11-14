import { useEffect,useLayoutEffect,useRef,useState } from "react";
import type { InvitationDelivery,InvitationPreviewInput,InvitationSenderStatus } from "../shared/api-types";
import {useScope} from "./account-scope";
import {ApiFailure,errorText,isAborted} from "./api";
const labels:Record<string,string>={created:"계정 생성됨",delivery_pending:"전체 내용 확인 · 발송 허락 대기",dispatch_committed:"발송 요청 중 · 다시 보내지 않습니다",sent:"Gmail 발송 접수",delivery_unknown:"발송 결과 불명 · 자동 재발송하지 않습니다",failed_definite:"발송을 시작하지 못했습니다",expired:"초대 발송 권한 만료",activated:"이미 활성화된 계정"};
export function InvitationEmail({accountId,onExpired,target,onComplete}:{accountId:string;onExpired:()=>void;target?:{account_id:string;revision:number;temporary_password:string};onComplete?:()=>void}){
 const scope=useScope(accountId);
 const [sender,setSender]=useState<InvitationSenderStatus|null>(null),[items,setItems]=useState<InvitationDelivery[]>([]),[preview,setPreview]=useState<InvitationDelivery|null>(null),[include,setInclude]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState("");
 const attempt=useRef<InvitationPreviewInput|null>(null),alive=useRef(true),op=useRef(0);
 useLayoutEffect(()=>{alive.current=true;return()=>{alive.current=false;op.current++}},[scope]);
 useLayoutEffect(()=>{attempt.current=null;setPreview(null);setInclude(false);op.current++;setBusy(false);},[target?.account_id,target?.revision]);
 function fail(e:unknown){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401)onExpired();else setError(errorText(e));}
 async function refresh(){const [s,i]=await Promise.all([scope.request<InvitationSenderStatus>("/admin/invitations/sender"),scope.request<InvitationDelivery[]>("/admin/invitations/")]);setSender(s);setItems(i);if(preview)setPreview(await scope.request<InvitationDelivery>(`/admin/invitations/${preview.id}`));}
 useEffect(()=>{void refresh().catch(fail);},[scope]);
 useEffect(()=>{
  const deadlines=[...items,...(preview?[preview]:[])].filter(v=>v.text&&v.state==="delivery_pending").map(v=>v.expires_at*1000);
  if(!deadlines.length)return;
  const timer=setTimeout(()=>{
   const scrub=(v:InvitationDelivery)=>v.expires_at*1000<=Date.now()&&v.state==="delivery_pending"?{...v,text:null,state:"expired"}:v;
   setItems(v=>v.map(scrub));setPreview(v=>v?scrub(v):v);attempt.current=null;
  },Math.min(2147483647,Math.max(0,Math.min(...deadlines)-Date.now())));
  return()=>clearTimeout(timer);
 },[items,preview]);
 async function prepare(){if(!target||busy)return;setBusy(true);setError("");const generation=++op.current;
  const input=attempt.current??{request_id:crypto.randomUUID(),account_id:target.account_id,expected_revision:target.revision,temporary_password:include?target.temporary_password:null};attempt.current=input;
  try{const p=await scope.request<InvitationDelivery>("/admin/invitations/","POST",input);if(alive.current&&op.current===generation){setPreview(p);setItems(v=>[p,...v.filter(x=>x.id!==p.id)]);}}
  catch(e){if(op.current===generation){setError("계정은 유지됩니다. 초대 준비 결과를 확인하지 못했습니다. 같은 요청으로 다시 확인하거나 발송 상태를 새로 읽으세요.");fail(e);}}
  finally{if(op.current===generation)setBusy(false);}
 }
 async function send(){if(!preview||!preview.text||busy)return;const generation=++op.current;setBusy(true);setError("");
  try{const p=await scope.request<InvitationDelivery>(`/admin/invitations/${preview.id}/approve`,"POST",{digest:preview.digest});if(alive.current&&op.current===generation){setPreview(p);setItems(v=>[p,...v.filter(x=>x.id!==p.id)]);attempt.current=null;onComplete?.();}}
  catch(e){if(op.current===generation){setError("발송 응답을 확인하지 못했습니다. 상태를 새로 읽으세요. 자동으로 다시 보내지 않습니다.");setPreview(v=>v?{...v,text:null}:v);fail(e);}}
  finally{if(op.current===generation)setBusy(false);}
 }
 return <section aria-label="초대 이메일" className="admin-card">
  <h3>초대 이메일</h3>
  <p>계정 생성·암호 복사는 이메일과 별개입니다. 모든 발송은 전체 확인 후 직접 허락해야 합니다.</p>
  <p>{sender?.configured?`운영자가 설정한 발신자: ${sender.from}. 실제 연결·scope는 발송 시 확인합니다.`:"시스템 발신자가 설정되지 않았습니다. 로컬 암호 복사로 초대할 수 있습니다."}</p>
  {error&&<p role="alert">{error}</p>}
  <button disabled={busy} onClick={()=>void refresh().catch(fail)}>초대 발송 상태 새로 읽기</button>
  {target&&sender?.configured&&!preview&&<>
   <label><input type="checkbox" checked={include} disabled={busy||!!attempt.current} onChange={e=>setInclude(e.target.checked)}/>이번 임시 비밀번호를 이메일에 포함</label>
   <p>이 credential의 발송은 한 번뿐입니다. 응답 불명·만료 후 다시 초대하려면 명시적으로 비밀번호를 재발급하세요.</p>
   <button disabled={busy} onClick={()=>void prepare()}>초대 이메일 전체 내용 준비</button>
  </>}
  {preview&&<article aria-label="초대 이메일 발송 확인">
   <p>{labels[preview.state]??preview.state}</p>
   {preview.text&&<><dl><dt>From</dt><dd>{preview.from}</dd><dt>To</dt><dd>{preview.to}</dd><dt>Cc / Bcc / 첨부</dt><dd>없음</dd><dt>제목</dt><dd>{preview.subject}</dd></dl><pre className="mail-body">{preview.text}</pre><p>이 확인본의 발송 허락 만료: {new Date(preview.expires_at*1000).toISOString()}</p></>}
   {preview.state==="delivery_pending"&&preview.text&&<button disabled={busy||preview.expires_at<=Date.now()/1000} onClick={()=>void send()}>전체 내용을 허락하고 초대 이메일 보내기</button>}
   {preview.provider_message_id&&<p>Gmail 접수 ID: {preview.provider_message_id}. 상대 수신·열람 확인은 아닙니다.</p>}
  </article>}
  <ul>{items.map(i=><li key={i.id}>{i.to||"삭제된 계정"} · {labels[i.state]??i.state} <button disabled={busy} onClick={()=>{void scope.request<InvitationDelivery>(`/admin/invitations/${i.id}`).then(v=>{setPreview(v);attempt.current=null;}).catch(fail);}}>이 발송 상태 보기</button></li>)}</ul>
 </section>;
}
