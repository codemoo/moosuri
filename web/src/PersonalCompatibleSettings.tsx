import {useEffect, useRef, useState, type FormEvent} from "react";
import type {ModelCatalog, ModelChoice, ProviderSummary, ProvidersResponse} from "../shared/api-types";
import {useScope} from "./account-scope";
import {ApiFailure, isAborted} from "./api";
import "./personal-management.css";

const path = "/settings/compatible-providers";
type Candidate = {name:string; base_url:string; models:{id:string;name:string}[]};
type Props = {accountId:string; onExpired:()=>void; onVerified:()=>void};
type Draft = {name:string; base:string; key:string; previous?:ProviderSummary; workstationModel?:string};
const blank = ():Draft => ({name:"",base:"",key:""});
const visionKey=(choice:ModelChoice)=>JSON.stringify([choice.provider_id,choice.provider_revision,choice.model,choice.effort]);
const providerErrors = new Map([
  ["model_credentials_rejected", "서버에서 API 키를 거절했습니다. 연결 설정에서 키를 확인해 주세요."],
  ["model_rate_limited", "서버의 요청 한도에 도달했습니다. 잠시 뒤 다시 확인해 주세요."],
  ["model_identity_unverified", "서버가 다른 모델 ID로 응답했습니다. 정확한 모델 ID를 가져온 뒤 다시 확인해 주세요."],
  ["model_configuration_invalid", "서버 주소나 연결 설정을 확인해 주세요."],
]);
const loadError = "저장된 연결을 불러오지 못했습니다. 연결 목록을 새로고침해 주세요.";
const catalogError = "모델 목록을 가져오지 못했습니다. 연결 주소와 API 키를 확인하거나 알고 있는 모델 ID를 입력해 주세요.";
const textError = "텍스트 응답을 확인하지 못했습니다. 모델 ID와 서버 상태를 확인해 주세요.";
const imageError = "이 모델의 이미지 읽기를 확인하지 못했습니다. 다른 이미지 지원 모델로 확인하거나 글만 보내세요.";
const saveError = "연결을 저장했는지 확인하지 못했습니다. 먼저 연결 목록을 새로고침한 뒤 이름과 API 주소를 확인해 주세요.";

export function PersonalCompatibleSettings({accountId, onExpired, onVerified}:Props) {
  const scope = useScope(accountId);
  const [providers,setProviders] = useState<ProviderSummary[] | null>(null);
  const [draft,setDraft] = useState<Draft|null>(null);
  const [candidates,setCandidates] = useState<Candidate[]|null>(null);
  const [discoveredModels,setDiscoveredModels] = useState<Record<string,string>>({});
  const [catalogs,setCatalogs] = useState<Record<string, string[]>>({});
  const [models,setModels] = useState<Record<string,string>>({});
  const [visionProofs,setVisionProofs] = useState<Record<string,boolean>>({});
  const [busy,setBusy] = useState(false);
  const [notice,setNotice] = useState("");
  const [error,setError] = useState("");
  const [accessDenied,setAccessDenied] = useState(false);
  const pending = useRef(false);
  function fail(e:unknown, fallback:string) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) {onExpired(); return;}
    if (e instanceof ApiFailure && e.status === 403) {
      setAccessDenied(true);
      setDraft(null);
      setProviders(null);
      return;
    }
    const reason = e instanceof ApiFailure ? providerErrors.get(e.code) : undefined;
    setError(reason ?? fallback);
  }
  async function reload() {
    const result = await scope.request<ProvidersResponse>(path);
    if (!Array.isArray(result.providers)) throw new Error("invalid providers");
    setProviders(result.providers);
  }
  useEffect(()=>{ void reload().catch(e => fail(e, loadError)); },[scope]);
  async function work(fn:()=>Promise<void>, fallback:string) {
    if (pending.current) return;
    pending.current = true; setBusy(true); setError(""); setNotice("");
    try { await fn(); } catch (e) { fail(e, fallback); }
    finally {pending.current = false; setBusy(false);}
  }
  function replace(p:ProviderSummary) {
    setProviders(old=>[...(old ?? []).filter(v=>v.id!==p.id),p]);
    setCatalogs(old=>({...old,[p.id]:[]}));
    setModels(old=>({...old,[p.id]:""}));
  }
  async function catalog(p:ProviderSummary) {
    const list = await scope.request<ModelCatalog>(`${path}/${encodeURIComponent(p.id)}/models`, "POST", {revision:p.revision});
    const ids = list.models.map(m=>m.id);
    setCatalogs(old=>({...old,[p.id]:ids}));
    setModels(old=>({...old,[p.id]:ids.includes(old[p.id]) ? old[p.id] : ids[0] ?? ""}));
    return ids.length;
  }
  async function loadModels(p:ProviderSummary, prefix = "") {
    const count = await catalog(p);
    setNotice(prefix + (count ? `${p.name}에서 모델 ${count}개를 불러왔습니다. 확인할 모델을 고르세요.`
      : `${p.name}에서 모델 목록이 비어 있습니다. 알고 있는 모델 ID를 입력해 확인할 수 있습니다.`));
  }
  async function save(value:Draft, preferred?:string) {
    const key = value.key.trim();
    const existing = value.previous;
    const changedAddress = existing && value.base.trim() !== existing.config.base_url;
    if (existing?.has_credential && changedAddress && !key) {
      setError("주소를 바꾸려면 새 주소에서 쓸 API 키를 입력하거나 연결을 해제한 뒤 새로 추가하세요."); return;
    }
    const receipt = await scope.request<ProviderSummary>(existing ? `${path}/${encodeURIComponent(existing.id)}` : path,
      existing ? "PUT" : "POST", {name:value.name.trim(),base_url:value.base.trim(),enabled:true,
        credential:key ? {action:"replace",value:key} : {action:existing ? "keep":"clear"},
        ...(existing ? {expected_revision:existing.revision}:{})});
    const saved = "연결을 저장했습니다. ";
    replace(receipt); setDraft(null); setNotice(saved + "사용할 모델을 확인하세요."); onVerified();
    if (preferred) {
      setModels(old=>({...old,[receipt.id]:preferred}));
      try { await verify(receipt,preferred,saved); } catch (e) { fail(e, textError); }
    } else {
      try { await loadModels(receipt,saved); } catch (e) { fail(e, catalogError); }
    }
  }
  async function verify(p:ProviderSummary, model:string, prefix = "") {
    await scope.request(`${path}/${encodeURIComponent(p.id)}/probe`,"POST",{revision:p.revision, model,effort:null});
    onVerified(); setNotice(`${prefix}${model}의 텍스트 응답을 확인했습니다.`);
    try { await reload(); } catch (e) { fail(e, loadError); }
  }
  async function verifyVision(p:ProviderSummary, choice:ModelChoice) {
    const result=await scope.request<{vision:boolean}>(`${path}/${encodeURIComponent(p.id)}/vision-probe`,"POST",{revision:p.revision,model:choice.model,effort:choice.effort});
    if(result?.vision!==true)throw new ApiFailure(502,"vision_probe_failed");
    setVisionProofs(old=>({...old,[visionKey(choice)]:true}));
    onVerified(); setNotice(`${choice.model} · ${choice.effort??"모델 기본값"}의 이미지 읽기를 확인했습니다. 대화로 돌아가 직접 다시 보내세요.`);
  }
  async function discover() {
    const result = await scope.request<{candidates:Candidate[]}>(path+"/discover");
    setCandidates(result.candidates);
    setDiscoveredModels({});
    if (!result.candidates.length) setNotice("무수리 설치 서버에서 실행 중인 Ollama 또는 LM Studio를 찾지 못했습니다. 해당 프로그램에서 모델 서버를 시작한 뒤 다시 찾아보세요.");
  }
  async function importWorkstation(file: File) {
    if (file.size > 4096) throw new Error("invalid workstation file");
    const data = JSON.parse(await file.text());
    const urls = ["http://127.0.0.1:11434/v1", "http://127.0.0.1:1234/v1"];
    if (data?.format !== "moosuri-workstation-model-v1" || data.temporary !== true || data.remote_admin_only !== true ||
        !urls.includes(data.base_url) || !["Workstation Ollama","Workstation LM Studio"].includes(data.name) ||
        typeof data.model !== "string" || !data.model || data.model.length > 200 || /[\u0000-\u001f]/.test(data.model) ||
        data.credential?.action !== "replace" || typeof data.credential.value !== "string" ||
        !/^[a-zA-Z0-9_-]{64}$/.test(data.credential.value)) throw new Error("invalid workstation file");
    const existing = providers?.find(p => p.name === data.name && p.config.base_url === data.base_url);
    setDraft({name:data.name,base:data.base_url,key:data.credential.value,workstationModel:data.model,previous:existing});
    setNotice("연결 파일을 읽었습니다. 모델을 확인한 뒤 저장하세요.");
  }
  function submit(e:FormEvent) { e.preventDefault(); if (draft) void work(()=>save(draft,draft.workstationModel),saveError); }
  if (accessDenied) return null;
  if (providers === null) return error ? <section className="admin-card" aria-label="개인 LLM 연결">
    <p role="alert">{error}</p>
    <button type="button" disabled={busy} onClick={()=>void work(reload,loadError)}>연결 목록 새로고침</button>
  </section> : null;
  return <section className="admin-card" aria-label="개인 LLM 연결">
    <div className="section-heading"><div><h3>OpenAI 호환 연결</h3><p>Ollama, LM Studio 또는 외부 API 서버의 모델을 내 계정에 추가할 수 있습니다.</p></div>
      <button type="button" disabled={busy} onClick={()=>{setError(""); setNotice(""); setDraft(blank());}}>주소로 연결</button>
    </div>
    <div className="compatible-actions">
      <button type="button" disabled={busy} onClick={()=>void work(discover,"로컬 모델 검색을 완료하지 못했습니다. 잠시 뒤 다시 찾아보세요.")}>서버에서 로컬 모델 찾기</button>
      <button type="button" disabled={busy} onClick={()=>void work(async()=>{
        await reload();
        setNotice("저장된 연결 목록을 새로고침했습니다.");
      }, loadError)}>연결 목록 새로고침</button>
    </div>
    <details><summary>다른 컴퓨터의 로컬 모델 연결</summary>
      <p>Ollama 또는 LM Studio가 실행 중인 컴퓨터의 터미널에서 아래 명령을 실행하세요. 마지막 인수를 무수리 서버의 SSH 호스트 이름으로 바꾸세요.</p>
      <pre className="login-command"><code>curl -fsSL https://raw.githubusercontent.com/codemoo/moosuri/main/install.sh | bash -s -- --workstation-model --ssh-host MY_SERVER</code></pre>
      <p>서버 관리자 계정으로 연결합니다. 도우미에서 모델을 고른 뒤 출력된 임시 연결 파일을 여세요. 모델을 사용하는 동안 도우미를 실행해 두세요.</p>
      <label>워크스테이션 연결 파일<input type="file" accept=".json,application/json" disabled={busy} onChange={event=>{
        const file=event.target.files?.[0]; event.target.value="";
        if (file) void work(()=>importWorkstation(file),"연결 파일을 읽지 못했습니다. 모델 연결 도우미에서 출력한 JSON 파일을 선택해 주세요.");
      }}/></label>
    </details>
    {candidates?.map(c=> {
      const existing = providers.find(p=>p.enabled && p.config.base_url === c.base_url);
      const model = discoveredModels[c.base_url] ?? c.models[0]?.id;
      return <div key={c.base_url} className="compatible-actions">
        <strong>{c.name}</strong>
        {existing ? <span>연결 저장됨</span> : c.models.length > 1 ? (
          <label>모델
            <select aria-label={`${c.name} 모델`} value={model} disabled={busy} onChange={event=>{
              setDiscoveredModels(old=>({...old,[c.base_url]:event.target.value}));
            }}>
              {c.models.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}
            </select>
          </label>
        ) : <span>{c.models[0]?.name ?? "설치된 모델 없음"}</span>}
        {!existing && model && <button type="button" disabled={busy}
          onClick={()=>void work(()=>save({name:c.name,base:c.base_url,key:""},model),saveError)}>연결 저장 및 모델 확인</button>}
      </div>;
    })}
    {draft && <form className="provider-form" onSubmit={submit}>
      {draft.workstationModel && <p>연결할 모델 · {draft.workstationModel}</p>}
      <label>연결 이름<input readOnly={!!draft.workstationModel} required maxLength={100} value={draft.name} onChange={e=>setDraft({...draft,name:e.target.value})} /></label>
      <label>API 기본 주소<input type="url" readOnly={!!draft.workstationModel} required maxLength={2048} placeholder="https://example.com/v1" value={draft.base} onChange={e=>setDraft({...draft,base:e.target.value})} /></label>
      {!draft.workstationModel && <label>API 키 (선택)<input type="password" autoComplete="off" placeholder={draft.previous?.has_credential ? "비워 두면 기존 키 유지" : "서버에서 발급받은 키"} value={draft.key} onChange={e=>setDraft({...draft,key:e.target.value})} /></label>}
      <div><button disabled={busy}>연결 저장</button><button type="button" disabled={busy} onClick={()=>setDraft(null)}>취소</button></div>
    </form>}
    {providers.filter(p=>p.enabled).map(p=><div role="group" aria-label={p.name} key={p.id+":"+p.revision}>
      <h4>{p.name}</h4><p className="provider-address">{p.config.base_url}</p>
      <button type="button" disabled={busy} onClick={()=>void work(()=>loadModels(p),catalogError)}>모델 불러오기</button>
      <label>확인할 모델 ID<input list={`custom-list-${p.id}`} value={models[p.id]??""} onChange={e=>setModels(old=>({...old,[p.id]:e.target.value}))}/></label>
      <datalist id={`custom-list-${p.id}`}>{catalogs[p.id]?.map(id=><option key={id}>{id}</option>)}</datalist>
      <button type="button" disabled={busy || !models[p.id]?.trim()} onClick={()=>void work(()=>verify(p,models[p.id].trim()),textError)}>텍스트 연결 확인</button>
      {(p.choices??[]).filter(choice=>choice.provider_id===p.id&&choice.provider_revision===p.revision).map(choice=><div key={visionKey(choice)}>
        <span>{choice.model} · {choice.effort??"모델 기본값"}</span>{" "}
        {visionProofs[visionKey(choice)]?<span role="status">이미지 읽기 확인됨</span>:<button type="button" disabled={busy} onClick={()=>void work(()=>verifyVision(p,choice),imageError)}>이미지 연결 확인 · {choice.model} · {choice.effort??"모델 기본값"}</button>}
      </div>)}
      <button type="button" disabled={busy} onClick={()=>setDraft({name:p.name,base:p.config.base_url??"",key:"",previous:p})}>연결 수정</button>
      <button type="button" disabled={busy} onClick={()=>{
        if (!window.confirm(`${p.name} 연결을 해제할까요?\n저장된 API 키가 삭제되고 이 연결의 모델은 사용 가능한 모델 목록에서 제거됩니다.`)) return;
        void work(async()=>{
        const result=await scope.request<ProviderSummary>(`${path}/${encodeURIComponent(p.id)}`,"DELETE",{revision:p.revision});
        replace(result); if(draft?.previous?.id===p.id)setDraft(null); onVerified(); setNotice(`${p.name} 연결을 해제했습니다.`);
        },"연결 해제 결과를 확인하지 못했습니다. 연결 목록을 새로고침해 주세요.");
      }}>연결 해제</button>
    </div>)}
    {busy && <p role="status">요청을 처리하는 중…</p>}
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
  </section>;
}
