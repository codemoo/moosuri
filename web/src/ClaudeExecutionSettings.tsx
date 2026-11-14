import {useEffect,useRef,useState} from "react";
import {useScope} from "./account-scope";
import {ApiFailure,isAborted} from "./api";
type Binding={id:string;revision:number;label:string;status:string;cleanup_state?:string};
type State={runtime_configured:boolean;bindings:Binding[];text_only:true};
export function ClaudeExecutionSettings({accountId,onExpired,onVerified}:{accountId:string;onExpired:()=>void;onVerified:()=>void}){
 const scope=useScope(accountId);const [state,setState]=useState<State|null>(null);const [label,setLabel]=useState("내 Claude");const [models,setModels]=useState<Record<string,string>>({});const [busy,setBusy]=useState(false);const [notice,setNotice]=useState("");const pending=useRef(false);
 function fail(e:unknown){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401)onExpired();else setNotice("공식 CLI 연결을 확인하지 못했습니다. 서버의 격리 실행 구성과 공식 로그인 상태를 확인하세요.");}
 useEffect(()=>{let active=true;setState(null);setModels({});setNotice("");void scope.request<State>("/settings/claude").then(v=>{if(active)setState(v);}).catch(e=>{if(active)fail(e);});return()=>{active=false;};},[scope]);
 async function action(kind:"create"|"refresh"|"probe"|"disconnect",b?:Binding){if(pending.current)return;pending.current=true;setBusy(true);setNotice("");try{
  if(kind==="create")await scope.request("/settings/claude","POST",{label:label.trim()});
  if(b&&kind==="probe"){await scope.request(`/settings/claude/${encodeURIComponent(b.id)}/probe`,"POST",{revision:b.revision,model:(models[b.id]??"").trim()});setNotice("정확한 모델의 텍스트 응답을 확인했습니다. 아래에서 사용할 모델을 선택하세요.");onVerified();}
  if(b&&kind==="disconnect"){await scope.request(`/settings/claude/${encodeURIComponent(b.id)}`,"DELETE",{revision:b.revision});setNotice("개인 연결을 로컬에서 해제했습니다. 원격 로그인 해제는 확인되지 않았습니다. 공식 CLI 또는 Claude 계정 설정에서 해제할 수 있습니다.");onVerified();}
  setState(await scope.request<State>("/settings/claude"));
 }catch(e){fail(e);}finally{pending.current=false;setBusy(false);}}
 return <section className="admin-card" aria-label="내 공식 Claude Code 연결"><h3>내 Claude · 공식 CLI 텍스트</h3>
 <p>각자 공식 Claude Code에 로그인하여 본인 계정의 공식 인증과 결제로 사용하는 선택 통합입니다. 현재 텍스트 대화만 지원합니다. 도구·첨부·기억 저장·일정·백그라운드 업무는 제공하지 않습니다.</p>
 {!state?<p>서버 연결 상태 확인 중</p>:!state.runtime_configured?<p>설치 관리자가 검증된 Linux 격리 실행을 구성해야 연결할 수 있습니다. 기본 앱에는 외부 CLI가 필요하지 않습니다.</p>:<>
 <label>연결 이름<input value={label} onChange={e=>setLabel(e.target.value)} maxLength={80} disabled={busy}/></label><button disabled={busy||!label.trim()} onClick={()=>void action("create")}>내 Claude 연결 추가</button>
 {state.bindings.map(b=><div key={b.id} role="group" aria-label={b.label}><strong>{b.label}</strong><p>{b.cleanup_state==="pending"?"로컬 실행 차단됨 · 개인 로그인 파일 정리 대기":b.status==="active"?"텍스트 확인됨":b.status==="pending"?"공식 로그인과 모델 확인 대기":b.status==="disconnected"?"로컬 연결 해제됨":"다시 연결 필요"}</p>
 {(b.status==="pending"||b.status==="active")&&<><details><summary>서버에서 공식 로그인하기</summary><p>서버의 터미널에서 아래 도우미를 실행하세요. 설치 관리자가 제공한 같은 실행 프로필 경로를 사용합니다. 비밀번호·토큰을 명령줄에 넣지 마세요. 로그인은 공식 CLI 화면에서 완료합니다.</p><code>{`moosuri subscriptions claude-login --server ${window.location.origin} --email 내주소 --binding ${b.id} --revision ${b.revision} --profile-file /private/path/claude-profile.json`}</code></details>
 <label>정확한 모델 ID<input aria-label={`Claude 모델 ${b.label}`} value={models[b.id]??""} onChange={e=>setModels(old=>({...old,[b.id]:e.target.value}))} disabled={busy} placeholder="공식 계정에서 사용할 정확한 버전 ID"/></label><p>실시간 모델 목록과 관측된 추론 강도는 아직 제공하지 않습니다. 별칭을 가용 모델로 표시하지 않습니다. 아래 확인은 본인 구독을 실제로 사용합니다.</p><button disabled={busy||!(models[b.id]??"").trim()} onClick={()=>void action("probe",b)}>Claude 텍스트 연결 확인</button></>}
 <button disabled={busy||b.status==="disconnected"} onClick={()=>void action("disconnect",b)}>Claude 연결 해제</button></div>)}
 </>}
 <button disabled={busy} onClick={()=>void action("refresh")}>Claude 연결 새로고침</button>{notice&&<p role="status">{notice}</p>}
 </section>;
}
