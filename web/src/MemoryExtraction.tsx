import {useEffect,useRef,useState} from "react";
import type {MemoryExtractionStatus} from "../shared/api-types";
import {useScope} from "./account-scope";
import {ApiFailure,isAborted} from "./api";
import "./memory-extraction.css";
type Props={accountId:string;collectionId:string;connected:boolean;onExpired:()=>void;source?:{id:string;revision:number}};
export function MemoryExtraction(props:Props){return <Controls key={`${props.accountId}:${props.collectionId}:${props.source?.id??"collection"}:${props.source?.revision??""}`} {...props}/>}
function Controls({accountId,collectionId,connected,onExpired,source}:Props){
 const scope=useScope(accountId,`extraction:${collectionId}:${source?.id??""}`);
 const [status,setStatus]=useState<MemoryExtractionStatus|null>(null);const [mode,setMode]=useState("reported");const [speaker,setSpeaker]=useState("");const [busy,setBusy]=useState(false);const [error,setError]=useState("");
 const generation=useRef(0);
 const owner=useRef<{generation:number;kind:"read"|"mutation";controller:AbortController}|null>(null);
 const statusPath=source?`/knowledge/sources/${encodeURIComponent(source.id)}/extraction`:`/knowledge/collections/${encodeURIComponent(collectionId)}/extraction`;
 function fail(e:unknown){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401)onExpired();else setError("기억 추출 상태를 확인하지 못했습니다. 다시 열어 확인해 주세요.");}
 async function request(kind:"read"|"mutation",path:string,method="GET",body?:unknown,initialize=false){
  // Ref ownership is synchronous: two handlers can run before busy is rendered.
  // A mutation supersedes an older read, but no request supersedes a mutation.
  if(owner.current&&(kind==="read"||owner.current.kind==="mutation"))return;
  owner.current?.controller.abort();
  const current={generation:++generation.current,kind,controller:new AbortController()};
  owner.current=current;setBusy(true);setError("");
  const admitted=()=>owner.current===current&&generation.current===current.generation&&!current.controller.signal.aborted;
  try{
   const next=await scope.request<MemoryExtractionStatus>(path,method,body,current.controller.signal);
   if(!admitted())return;
   setStatus(next);if(initialize){setMode(next.attribution_mode);setSpeaker(next.reported_speaker)}
  }catch(e){if(admitted())fail(e)}finally{if(admitted()){owner.current=null;setBusy(false)}}
 }
 useEffect(()=>{
  void request("read",statusPath,"GET",undefined,true);
  return()=>{generation.current++;owner.current?.controller.abort();owner.current=null};
 },[scope,statusPath]);
 async function refresh(){await request("read",statusPath)}
 async function save(enabled:boolean){if(!status)return;await request("mutation",`/knowledge/collections/${encodeURIComponent(collectionId)}/extraction`,"PUT",{expected_revision:status.revision,enabled,attribution_mode:mode,reported_speaker:mode==="specified"?speaker:"",daily_calls:status.daily_calls,daily_input_bytes:status.daily_input_bytes,daily_output_tokens:status.daily_output_tokens})}
 async function extract(){if(!source||!status?.enabled)return;await request("mutation",statusPath,"POST",{expected_revision:source.revision})}
 const reason=status?.reason;
 const notice=reason==="partial_input"?`이번에는 자료 일부를 확인해 기억 ${status?.saved_count??0}개를 저장했습니다. 큰 문단이나 나머지 구간은 처리하지 않았습니다.`:reason==="daily_budget"?"오늘의 추출 한도에 도달했습니다. 한도가 초기화되면 다시 확인합니다.":reason==="unknown_attribution"?"작성자를 하나로 확인할 수 없어 이 자료는 건너뛰었습니다.":reason==="no_new_facts"?"이번 자료에서 새로 저장할 사실을 찾지 못했습니다.":reason==="provider_changed"||reason==="authority_changed"?"연결이나 계정 설정이 바뀌었습니다. 필요하면 다시 켜 주세요.":status?.state==="completed"?`근거가 연결된 기억 ${status.saved_count}개를 저장했습니다.`:status?.state==="queued"||status?.state==="acquired"||status?.state==="dispatched"?"자료를 확인하고 있습니다. 이 화면을 다시 열어 결과를 확인해 주세요.":reason?"이번 자료의 추출을 마치지 못했습니다. 원문과 연결 상태를 확인해 주세요.":"";
 return <section className="memory-extraction" aria-label="자료에서 기억 만들기"><h4>자료에서 기억 만들기</h4>{error&&<p role="alert">{error}</p>}
 {status&&<><p>켜면 앞으로 추가되거나 바뀐 자료에서 작성자가 직접 말한 사실을 원문 근거와 함께 기억합니다. 과거 자료 전체를 읽지는 않습니다.</p>
 {!source&&<><label>작성자 기준<select aria-label="작성자 기준" value={mode} onChange={e=>setMode(e.target.value)} disabled={busy}><option value="reported">자료에 표시된 작성자</option><option value="specified">지정한 사람</option></select></label>
 {mode==="specified"&&<label>기억에 표시할 사람<input aria-label="기억에 표시할 사람" value={speaker} onChange={e=>setSpeaker(e.target.value)} disabled={busy}/></label>}
 <p className="fine">자료에 표시된 이름으로 기억을 묶습니다. 이름을 직접 지정할 수도 있습니다.</p>
 <button disabled={busy||!connected||(mode==="specified"&&!speaker.trim())} onClick={()=>void save(!status.enabled)}>{status.enabled?"자동 기억 끄기":"자동 기억 켜기"}</button>
 {status.enabled&&<button disabled={busy||!connected} onClick={()=>void save(true)}>작성자 기준 저장</button>}</>}
 {source&&<button disabled={busy||!connected||!status.enabled} onClick={()=>void extract()}>이 자료에서 기억 찾기</button>}
 <p className="fine">현재 {status.enabled?"켜짐":"꺼짐"}. 로그아웃해도 계속됩니다. 비밀번호나 연결을 바꾸면 다시 켜 주세요. 하루 최대 {status.daily_calls}회, 설정한 사용 한도 안에서 처리합니다.</p>
 <button disabled={busy} onClick={()=>void refresh()}>결과 새로고침</button> <a href="/app/memory">저장된 기억과 근거 보기</a>
 {notice&&<p role="status">{notice}</p>}<p className="fine">저장된 기억은 기억 화면에서 근거를 확인하고 수정하거나 잊게 할 수 있습니다.</p></>}
 </section>
}
