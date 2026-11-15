import {useEffect,useLayoutEffect,useRef,useState} from 'react';
import {useScope} from './account-scope';
import {ApiFailure,errorText,isAborted} from './api';
type Preview={selection?:string;subject:string;from:string;date:string;snippet:string;truncated:boolean};
type PreviewJob={request_id:string;run_id:string;expires_at:number;status:string;projection:null|{google_inbox_preview:{messages:Preview[];unavailable:number;more_available:boolean}}};
type BodyJob={request_id:string;status:string;resource:{google_body:string};projection:null|{google_inbox_body:{text:string;truncated:boolean;unavailable:boolean}}};
type MailStatus={google_preview_jobs?:PreviewJob[];google_body_jobs?:BodyJob[]};
/** Literal private display surface. No body is sent into chat input or model context. */
export function NativeMailBodies({accountId,runId,onExpired}:{accountId:string;runId?:string;onExpired:()=>void}){
 const scope=useScope(accountId,runId);const [state,setState]=useState<MailStatus>({});const [error,setError]=useState('');const [busy,setBusy]=useState(false);
 const [pending,setPending]=useState<{request_id:string;preview_request_id:string;selection:string}|null>(null);
 const epoch=useRef(0),sequence=useRef(0);
 useLayoutEffect(()=>{const reset=()=>{epoch.current++;sequence.current++;setState({});setPending(null);setError('');setBusy(false)};reset();return scope.onClose(reset)},[scope]);
 function fail(e:unknown){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}setError(errorText(e))}
 async function refresh(){const lifetime=epoch.current,request=++sequence.current;try{const value=await scope.request<MailStatus>('/credential-broker/status');if(lifetime===epoch.current&&request===sequence.current)setState(value)}catch(e){if(lifetime===epoch.current&&request===sequence.current)fail(e)}}
 useEffect(()=>{void refresh();const timer=setInterval(()=>void refresh(),5000);return()=>clearInterval(timer)},[scope]);
 async function open(preview_request_id:string,selection:string){if(busy)return;const lifetime=epoch.current;
  const input=pending??{request_id:crypto.randomUUID(),preview_request_id,selection};setPending(input);setBusy(true);setError('');
  try{await scope.request('/credential-broker/google-body','POST',input);if(lifetime===epoch.current){setPending(null);await refresh()}}catch(e){if(lifetime===epoch.current)fail(e)}finally{if(lifetime===epoch.current)setBusy(false)}
 }
 const previews=(state.google_preview_jobs??[]).filter(j=>!runId||j.run_id===runId);
 const labels:Record<string,string>={complete:'본문',not_dispatched:'요청이 취소됐어요',credential_rejected:'계정을 다시 연결해 주세요',outcome_unknown:'결과를 확인할 수 없어요',in_flight_or_unknown:'기기에서 읽는 중이에요',authority_changed:'연결 상태가 바뀌었어요',expired:'선택 시간이 지났어요',waiting_for_unlock:'기기의 볼트를 열어 주세요',waiting_for_credential:'기기 연결을 기다리고 있어요'};
 return <section aria-label="기기 Gmail 본문 읽기">{!runId&&<details><summary>본문 읽기 안내</summary><p>처음에는 연결한 기기에서 본문 읽기를 켜 주세요. 이후에는 메일을 선택하면 바로 열립니다. 선택 시간이 지난 메일은 새 미리보기에서 다시 열 수 있어요.</p><p>일반 텍스트 본문을 표시합니다. HTML만 있는 메일과 첨부파일은 아직 지원하지 않습니다. 본문은 비서 서버와 이 화면에 전달되며, 이후 대화나 기억·학습에 자동으로 추가하지 않습니다.</p></details>}
 {pending&&<button disabled={busy} onClick={()=>void open(pending.preview_request_id,pending.selection)}>같은 본문 요청 결과 확인</button>}
 {previews.map(j=><div key={j.request_id}>{j.projection?.google_inbox_preview&&<>
 {j.projection.google_inbox_preview.unavailable>0&&<p>삭제되거나 이동되어 표시하지 못한 메일이 {j.projection.google_inbox_preview.unavailable}개 있습니다.</p>}
 {j.projection.google_inbox_preview.messages.length===0&&j.projection.google_inbox_preview.unavailable===0&&<p>이 INBOX 목록 응답에 메시지가 없습니다.</p>}
 <ol>{j.projection.google_inbox_preview.messages.map((m,i)=><li key={m.selection??i}><p>제목: {m.subject||'(제목 없음)'}</p><p>보낸 사람: {m.from||'(보낸 사람 없음)'}</p><p>날짜: {m.date||'(날짜 없음)'}</p><p style={{whiteSpace:'pre-wrap'}}>미리보기: {m.snippet}</p>{m.truncated&&<p>일부 표시 문구가 길이 제한으로 잘렸습니다.</p>}
 {m.selection&&j.expires_at*1000>Date.now()?<button disabled={busy||!!pending} onClick={()=>void open(j.request_id,m.selection!)}>이 메일 본문 열기</button>:<p>{m.selection?'본문 선택 기한이 지났습니다. 새 미리보기에서 다시 선택하세요.':'이전 미리보기에는 본문 선택 정보가 없습니다. 새 미리보기를 읽으세요.'}</p>}
 {(state.google_body_jobs??[]).filter(b=>b.resource.google_body===m.selection).map(b=><div key={b.request_id}><p role="status">{labels[b.status]??'연결 상태를 확인해 주세요'}</p>{b.status==='expired'&&<p>본문 선택 기한이 지났습니다. 새 미리보기에서 다시 선택하세요.</p>}
 {b.status==='waiting_for_credential'&&<p>연결한 기기에서 이 Google 계정의 본문 읽기를 켜 주세요. 지금 선택한 메일을 기다리고 있어요.</p>}
 {b.projection?.google_inbox_body&&<>{b.projection.google_inbox_body.unavailable?<p>이 메일이 삭제되었거나 현재 INBOX에 없습니다.</p>:<p style={{whiteSpace:'pre-wrap'}}>{b.projection.google_inbox_body.text||'(빈 본문)'}</p>}{b.projection.google_inbox_body.truncated&&<p>본문이 길어 앞부분만 표시했어요.</p>}</>}
 {b.status==='outcome_unknown'&&<p>결과를 확인할 수 없습니다. HTML만 있는 메일·첨부 본문·지원하지 않는 문자 인코딩은 읽지 않습니다.</p>}
 </div>)}
 </li>)}</ol>{j.projection.google_inbox_preview.more_available&&<p>추가 페이지가 있지만 읽지 않았습니다.</p>}</>}</div>)}
 {error&&<p role="alert">{error}</p>}
 </section>
}
