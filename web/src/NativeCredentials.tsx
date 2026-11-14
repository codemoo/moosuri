import {useEffect,useState} from 'react';
import {useScope} from './account-scope';
import {ApiFailure,errorText,isAborted} from './api';
type Job={request_id:string;repository:{owner:string;repository:string};status:string;projection:null|{git_hub_repository:{id:number;private:boolean;archived:boolean;stars:number;open_issues:number}}};
type GoogleJob={request_id:string;status:string;projection:null|{google_inbox:{messages_total:number;messages_unread:number;threads_total:number;threads_unread:number}}};
type Status={devices:[string,string][];jobs:Job[];google_jobs?:GoogleJob[]};
export function NativeCredentials({accountId,onExpired,back}:{accountId:string;onExpired:()=>void;back:()=>void}){
 const [pendingGoogle,setPendingGoogle]=useState<string|null>(null);
 const scope=useScope(accountId);const [state,setState]=useState<Status|null>(null);const [error,setError]=useState('');const [available,setAvailable]=useState(true);const [key,setKey]=useState('');const [challenge,setChallenge]=useState('');const [owner,setOwner]=useState('');const [repository,setRepository]=useState('');const [busy,setBusy]=useState(false);const [pending,setPending]=useState<{request_id:string;repository:{owner:string;repository:string}}|null>(null);
 function fail(e:unknown){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}if(e instanceof ApiFailure&&e.status===404){setAvailable(false);return}setError(errorText(e))}
 async function refresh(){try{setState(await scope.request<Status>('/credential-broker/status'));}catch(e){fail(e)}}
 useEffect(()=>{if(!available)return;void refresh();const timer=setInterval(()=>void refresh(),5000);return()=>clearInterval(timer)},[scope,available]);
 async function pair(){if(busy||!/[a-fA-F0-9]{64}/.test(key.trim())||key.trim().length!==64)return;setBusy(true);setError('');try{const public_key=key.trim().match(/../g)!.map(v=>parseInt(v,16));const value=await scope.request('/credential-broker/pair','POST',{public_key});setChallenge(JSON.stringify(value));}catch(e){fail(e)}finally{setBusy(false)}}
 async function request(input=pending??{request_id:crypto.randomUUID(),repository:{owner:owner.trim().toLowerCase(),repository:repository.trim().toLowerCase()}}){if(busy)return;setBusy(true);setPending(input);setError('');try{await scope.request('/credential-broker/github','POST',input);setPending(null);await refresh();}catch(e){fail(e)}finally{setBusy(false)}}
 async function requestGoogle(){if(busy)return;const request_id=pendingGoogle??crypto.randomUUID();setPendingGoogle(request_id);setBusy(true);setError('');try{await scope.request('/credential-broker/google-inbox','POST',{request_id});setPendingGoogle(null);await refresh()}catch(e){fail(e)}finally{setBusy(false)}}
 return <main className="admin-scroll settings-page"><button onClick={back}>뒤로</button><h2>신뢰 기기 credential · 실험 기능</h2>
 <p>개인 API token은 신뢰 기기의 볼트 사본에서만 사용합니다. 비서와 모델은 token·볼트 암호를 받지 않습니다. 현재 지원: GitHub 저장소 하나의 메타데이터와 기기에서 연결한 Google 계정의 Gmail INBOX 개수 읽기.</p>
 {!available?<p role="status">이 설치에서는 실험 기능이 꺼져 있습니다.</p>:<>
 <section className="admin-card"><h3>신뢰 기기 연결</h3><p>직접 빌드한 native companion을 --pair로 시작하고 공개키를 붙여 넣으세요. 기기에서 계정·origin을 확인하고 live 볼트 또는 명시적인 암호화 archive의 API-key 항목과 정확한 저장소 또는 Google 계정을 한 번 선택합니다. 같은 범위의 다음 요청은 자동 재사용합니다.</p>
 <label>기기 공개키<input value={key} onChange={e=>setKey(e.target.value)} autoComplete="off"/></label><button disabled={busy||key.trim().length!==64} onClick={()=>void pair()}>기기 연결 challenge 만들기</button>
 {challenge&&<><p>60초 안에 이 공개 challenge를 companion에 붙여 넣으세요. 비밀번호·token을 여기에 입력하지 마세요.</p><textarea aria-label="공개 pairing challenge" readOnly value={challenge}/></>}
 <ul>{state?.devices.map(([id,status])=><li key={id}>{id} · {status}<button disabled={busy||status==='revoked'||status==='revocation_pending'} onClick={async()=>{setBusy(true);try{await scope.request(`/credential-broker/${id}/revoke`,'POST',{});await refresh()}catch(e){fail(e)}finally{setBusy(false)}}}>이 기기 철회</button></li>)}</ul></section>
 <section className="admin-card"><h3>GitHub 저장소 상태 읽기</h3><p>정확히 연결한 저장소의 ID·공개/비공개·보관·star·열린 issue 수만 반환합니다. 잠김·offline이면 원래 요청을 기다리며, 전송 후 응답을 잃으면 미확정으로 남기고 다시 실행하지 않습니다.</p>
 <label>소유자<input value={owner} disabled={busy||!!pending} onChange={e=>setOwner(e.target.value)}/></label><label>저장소<input value={repository} disabled={busy||!!pending} onChange={e=>setRepository(e.target.value)}/></label>
 <button disabled={busy||!owner||!repository} onClick={()=>void request()}>{pending?'같은 요청 결과 확인':'저장소 메타데이터 읽기'}</button>
 <ul>{state?.jobs.map(j=><li key={j.request_id}>{j.repository.owner}/{j.repository.repository} · {j.status}{j.projection&&<pre>{JSON.stringify(j.projection,null,2)}</pre>}</li>)}</ul></section>
 <section className="admin-card"><h3>Gmail 받은편지함 개수 읽기</h3><p>기기에서 한 번 연결한 Google 계정의 INBOX 메시지·스레드 전체/읽지 않은 개수만 조회합니다. 매번 실제 읽기 권한과 Google 계정을 확인합니다. 메일 본문·제목·계정 주소·token은 반환하지 않습니다.</p><button disabled={busy} onClick={()=>void requestGoogle()}>{pendingGoogle?'같은 Gmail 요청 결과 확인':'Gmail INBOX 개수 읽기'}</button><ul>{state?.google_jobs?.map(j=><li key={j.request_id}>Gmail INBOX · {j.status}{j.projection?.google_inbox&&<p>메시지 {j.projection.google_inbox.messages_total}개 · 읽지 않음 {j.projection.google_inbox.messages_unread}개 / 스레드 {j.projection.google_inbox.threads_total}개 · 읽지 않음 {j.projection.google_inbox.threads_unread}개</p>}</li>)}</ul></section>
 <p>Native 서명 배포는 제공하지 않습니다. Archive는 명시적인 암호화 사본이며 원래 볼트의 이후 회전을 자동 감지하지 않습니다. 항목·key revision이 달라진 사본은 재연결이 필요합니다. Google 로그인·메일 발송·변경·브라우저 로그인은 지원하지 않습니다. OAuth refresh credential은 신뢰 기기의 볼트 항목에서만 사용합니다.</p></>}
 {error&&<p role="alert">{error}</p>}</main>
}
