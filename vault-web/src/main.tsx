import {StrictMode,useEffect,useLayoutEffect,useRef,useState} from "react";
import {createRoot} from "react-dom/client";
import {WorkerClient,VaultFailure} from "./worker-client";
import type {Draft,EntryDetails,Owner,Preview,Receipt,Summary,Command,CurrentSession} from "./types";
import {VaultField,VaultIcon,VaultList,itemName} from "./workspace";
import "./style.css";
const empty:Draft={kind:"login",title:"",username:"",url:"",notes:"",favorite:false,expires_at:"",secret:"",totp:""};
const emptyPreview:Preview={eligible:0,duplicates:0,filtered:0,rows:[]};
type Field="secret"|"username"|"url"|"notes";
type Detail=EntryDetails&{item:string;revision:number;failed:boolean};
const errorText=(e:unknown)=>e instanceof Error&&e.message==="vault_authentication_failed"?"비밀번호 또는 복구 키를 확인해 주세요.":e instanceof Error&&e.message==="vault_conflict"?"다른 창에서 변경되었습니다. 잠근 뒤 다시 열어 주세요.":e instanceof Error&&e.message==="vault_limit"?"파일 또는 선택한 항목이 저장 가능한 범위를 넘었습니다.":e instanceof Error&&["vault_invalid_input","vault_unsupported","vault_kdf_policy"].includes(e.message)?"입력 또는 파일 형식을 확인해 주세요.":"연결을 확인하지 못했습니다. 잠시 뒤 다시 시도해 주세요.";
function App(){
  const searchOwner=useRef(0),importCancel=useRef(false);
  const client=useRef<WorkerClient|null>(null),generation=useRef(0),ownerRef=useRef<Owner|null>(null),pendingStartup=useRef<Promise<unknown>|null>(null);
  const [assistant,setAssistant]=useState(""),[connected,setConnected]=useState(false),[initialized,setInitialized]=useState(true),[unlocked,setUnlocked]=useState(false),[busy,setBusy]=useState(false),[notice,setNotice]=useState("");
  const [password,setPassword]=useState(""),[useRecovery,setUseRecovery]=useState(false),[recovery,setRecovery]=useState<string|null>(null),[verify,setVerify]=useState("");
  const [items,setItems]=useState<Summary[]>([]),[query,setQuery]=useState(""),[selected,setSelected]=useState<string|null>(null),[draft,setDraft]=useState<Draft|null>(null),[editing,setEditing]=useState<string|null>(null);
  const [revealed,setRevealed]=useState<{value:string;until:number}|null>(null),[uncertain,setUncertain]=useState(false),[oldPending,setOldPending]=useState<Receipt[]>([]),[settings,setSettings]=useState(false),[newPassword,setNewPassword]=useState("");
  const [preview,setPreview]=useState<Preview|null>(null),[rows,setRows]=useState<number[]>([]),[username,setUsername]=useState(""),[filterExact,setFilterExact]=useState(false),[imported,setImported]=useState(0),[importing,setImporting]=useState(false);
  const [importMode,setImportMode]=useState<"csv"|"archive"|"legacy_v1">("csv"),[archiveFile,setArchiveFile]=useState<File|null>(null),[archiveSecret,setArchiveSecret]=useState(""),[archiveRecovery,setArchiveRecovery]=useState(false),[archiveChecked,setArchiveChecked]=useState(false);
  const [otp,setOtp]=useState<{code:string;valid_from:number;valid_until:number}|null>(null),[clock,setClock]=useState(Date.now());
  const [searchOpen,setSearchOpen]=useState(false),[confirmDelete,setConfirmDelete]=useState(false);
  const [detail,setDetail]=useState<Detail|null>(null),[detailAttempt,setDetailAttempt]=useState(0);
  const listScroll=useRef(0),revealOwner=useRef(0);
  const selectedItem=items.find(i=>i.id===selected),frozen=busy||recovery!==null||uncertain;
  const isWorkspace=connected&&unlocked&&recovery===null;
  const screen=draft?"editor":preview?"import":settings?"settings":selectedItem?"detail":"list";
  const showingDetail=isWorkspace&&screen==="detail";
  const fields=detail?.item===selectedItem?.id&&detail?.revision===selectedItem?.revision?detail:null;
  function hideReveal(){revealOwner.current++;setRevealed(null)}
  function clearSelection(){setSelected(null);setDetail(null);setOtp(null);setConfirmDelete(false);hideReveal()}
  function lock(reason="vault_locked"){
    generation.current++;client.current?.close();client.current=null;pendingStartup.current=null;
    setUnlocked(false);setBusy(false);setPassword("");setRecovery(null);setVerify("");setItems([]);setQuery("");clearSelection();setDraft(null);setEditing(null);
    setPreview(null);setRows([]);setImporting(false);setUsername("");setFilterExact(false);setImported(0);importCancel.current=true;
    setSettings(false);setNewPassword("");setUncertain(false);setArchiveFile(null);setArchiveSecret("");setArchiveRecovery(false);setArchiveChecked(false);setSearchOpen(false);listScroll.current=0;
    setNotice(reason==="vault_saved_refresh_unlock"?"변경사항을 저장했습니다. 다른 변경도 있어 다시 열어 주세요.":reason==="vault_unauthorized"?"볼트 인증을 확인할 수 없어 잠갔습니다.":reason==="vault_unavailable"?"연결을 확인할 수 없어 볼트를 잠갔습니다.":reason==="vault_generation_changed"?"다른 창에서 볼트 보호 설정이 바뀌어 잠갔습니다. 현재 비밀번호 또는 복구 키로 다시 열어 주세요.":"볼트가 잠겼습니다.");
    if(reason==="vault_unauthorized")setConnected(false);
  }
  async function start(){
    if(client.current)return;
    if(pendingStartup.current){await pendingStartup.current;return}
    const epoch=generation.current;
    const work=(async()=>{
      const r=await fetch("/session/current",{credentials:"same-origin",cache:"no-store"});
      if(!r.ok)throw new VaultFailure("vault_unauthorized");const current=await r.json() as CurrentSession,owner=current.owner;
      if(epoch!==generation.current)return;ownerRef.current=owner;
      const worker=new Worker(new URL("./crypto-worker.ts",import.meta.url),{type:"module"});
      const scope=new WorkerClient(worker,epoch,reason=>{if(epoch===generation.current)lock(reason)});client.current=scope;
      const result=await scope.request<{initialized:boolean;pending:Receipt[]}>({op:"initialize",owner});
      if(epoch!==generation.current)return;setInitialized(result.initialized);setOldPending(result.pending);setConnected(true);
    })();pendingStartup.current=work;
    try{await work}catch(e){if(epoch===generation.current){(client.current as WorkerClient|null)?.close();client.current=null}throw e}finally{if(pendingStartup.current===work)pendingStartup.current=null}
  }
  async function run<T>(command:Command){const epoch=generation.current;await start();if(epoch!==generation.current||!client.current)throw new VaultFailure("vault_locked");const value=await client.current.request<T>(command);if(epoch!==generation.current)throw new VaultFailure("vault_locked");return value}
  async function perform(action:()=>Promise<void>){
    if(busy)return;const epoch=generation.current;setBusy(true);setNotice("");
    try{await action()}catch(e){if(epoch===generation.current){if(e instanceof VaultFailure&&e.uncertain){setUncertain(true);setNotice("저장 결과를 확인하지 못했습니다. 같은 저장을 다시 확인해 주세요.")}else setNotice(errorText(e));}}
    finally{if(epoch===generation.current)setBusy(false)}
  }
  async function list(text=query){const epoch=generation.current,sequence=++searchOwner.current;const result=await run<Summary[]>({op:"list",query:text});if(epoch===generation.current&&sequence===searchOwner.current)setItems(result)}
  useEffect(()=>{
    if(!unlocked||frozen)return;
    const epoch=generation.current;
    const timer=setTimeout(()=>{if(epoch===generation.current)void list(query).catch(()=>{})},250);
    return()=>{clearTimeout(timer);searchOwner.current++};
  },[query,unlocked,frozen]);
  async function saved(){const epoch=generation.current;await list();if(epoch!==generation.current)return;setInitialized(true);setUnlocked(true);setRecovery(null);setVerify("");setDraft(null);setEditing(null);setConfirmDelete(false);setUncertain(false);setOldPending([]);if(importing){const status=await run<{committed:number}>({op:"csv_status"});setImported(status.committed)}setNotice("저장했습니다.")}
  async function publish(){await run({op:"publish"});await saved()}
  useEffect(()=>{
    let live=true;
    void fetch("/public/config",{cache:"no-store"}).then(r=>r.json()).then(v=>{if(live&&typeof v.assistant_origin==="string")setAssistant(v.assistant_origin)}).catch(()=>{});
    void start().catch(()=>{if(live){client.current?.close();client.current=null;setConnected(false)}});
    return()=>{live=false;generation.current++;pendingStartup.current=null;client.current?.close();client.current=null};
  },[]);
  useEffect(()=>{
    const timer=setInterval(()=>setClock(Date.now()),1000);return()=>clearInterval(timer);
  },[]);
  useEffect(()=>{
    if(!unlocked&&recovery===null)return;let timer:ReturnType<typeof setTimeout>;
    const reset=()=>{clearTimeout(timer);timer=setTimeout(()=>lock(),5*60*1000)};reset();
    window.addEventListener("pointerdown",reset,{passive:true});window.addEventListener("keydown",reset);
    return()=>{clearTimeout(timer);window.removeEventListener("pointerdown",reset);window.removeEventListener("keydown",reset)};
  },[unlocked,recovery!==null]);
  useEffect(()=>{
    const close=()=>hideReveal();window.addEventListener("blur",close);
    const onVisibilityChange=()=>{if(document.hidden)close()};
    document.addEventListener("visibilitychange",onVisibilityChange);
    const message=(e:MessageEvent)=>{if(e.origin===assistant&&e.source===window.opener&&e.data?.kind==="moosuri-vault-lock"&&e.data?.account===ownerRef.current?.account)lock("vault_unauthorized")};
    window.addEventListener("message",message);return()=>{window.removeEventListener("blur",close);window.removeEventListener("message",message);document.removeEventListener("visibilitychange",onVisibilityChange)};
  },[assistant]);
  useEffect(()=>{if(revealed&&clock>=revealed.until)hideReveal()},[clock,revealed]);
  useLayoutEffect(()=>{
    if(isWorkspace)window.scrollTo({top:screen==="list"?listScroll.current:0,behavior:"instant"});
  },[isWorkspace,screen]);
  useEffect(()=>{
    if(!showingDetail||!selectedItem)return;
    const {id,revision}=selectedItem;
    const epoch=generation.current;
    let live=true;
    void run<EntryDetails>({op:"view_details",item:id}).then(fields=>{
      if(live&&epoch===generation.current)setDetail({...fields,item:id,revision,failed:false});
    }).catch(()=>{
      if(live&&epoch===generation.current)setDetail({item:id,revision,host:"",has_url:false,username:"",failed:true});
    });
    return()=>{live=false};
  },[showingDetail,selectedItem?.id,selectedItem?.revision,detailAttempt]);
  useEffect(()=>{
    setOtp(null);if(!showingDetail||!selectedItem?.has_totp||frozen)return;let live=true,last=0,working=false;const epoch=generation.current;
    const update=async()=>{if(working||document.hidden||Date.now()<last||epoch!==generation.current)return;working=true;try{const value=await run<{code:string;valid_from:number;valid_until:number}>({op:"totp",item:selectedItem.id});if(live&&epoch===generation.current){setOtp(value);last=value.valid_until*1000}}catch{}finally{working=false}};
    void update();const timer=setInterval(()=>void update(),1000);window.addEventListener("focus",update);
    return()=>{live=false;clearInterval(timer);window.removeEventListener("focus",update)};
  },[selected,showingDetail,frozen,selectedItem?.has_totp]);
  async function reveal(field:Field){if(!selected)return;await perform(async()=>{const sequence=++revealOwner.current;const value=await run<string>({op:"reveal",item:selected,field});if(sequence===revealOwner.current)setRevealed({value,until:Date.now()+15000})})}
  async function copy(field:Field|"code"){if(!selected)return;await perform(async()=>{
    const epoch=generation.current;
    if(field==="code"){
      for(let attempt=0;attempt<2;attempt++){
        const value=await run<{code:string;valid_until:number}>({op:"totp",item:selected});
        if(epoch!==generation.current)return;
        if(Date.now()>=value.valid_until*1000)continue;
        await navigator.clipboard.writeText(value.code);
        if(epoch!==generation.current)return;
        if(Date.now()<value.valid_until*1000){setNotice("현재 인증 코드를 복사했습니다.");return}
      }
      setNotice("코드가 갱신되었습니다. 다시 복사해 주세요.");
    }else{
      const value=await run<string>({op:"reveal",item:selected,field});
      if(epoch!==generation.current)return;
      await navigator.clipboard.writeText(value);
      if(epoch===generation.current)setNotice("복사했습니다. 클립보드에는 기기 설정에 따라 남을 수 있습니다.");
    }
  })}
  async function keyChange(op:string){await perform(async()=>{
    const result=await run<{recovery:string|null;needs_verification:boolean}>({op,...(op==="prepare_password"||op==="prepare_rotation"?{password:newPassword}:{})});setNewPassword("");
    if(result.needs_verification)setRecovery(result.recovery);else await publish();
  })}
  async function importSelected(){await perform(async()=>{
    importCancel.current=false;
    if(!importing){await run({op:importMode!=="csv"?"archive_select":"csv_select",rows});setImported(0);setImporting(true)}
    for(;!importCancel.current;){const next=await run<{remaining:number}|null>({op:"csv_next"});if(!next)break;await run({op:"publish"});setImported(v=>v+1)}
    await run({op:"csv_cancel"});setImporting(false);setPreview(null);setRows([]);await saved();if(importCancel.current)setNotice("가져오기를 중단했습니다. 이미 저장한 항목은 유지됩니다.");
  })}
  async function exportArchive(){await perform(async()=>{
    const encrypted=await run<string>({op:"archive_export"});
    const url=URL.createObjectURL(new Blob([encrypted],{type:"application/json"}));
    const link=document.createElement("a");link.href=url;link.download="moosuri-vault.encrypted.json";link.click();
    setTimeout(()=>URL.revokeObjectURL(url),0);setNotice("암호화 파일을 내려받았습니다. 내보낼 때 사용한 볼트 비밀번호 또는 복구 키가 필요합니다.");
  })}
  function openItem(item:Summary){
    listScroll.current=window.scrollY;
    setSelected(item.id);setDetail(null);setConfirmDelete(false);hideReveal();setNotice("");
  }
  function openListAction(action:()=>void){
    listScroll.current=window.scrollY;
    setNotice("");action();
  }
  function openImport(mode:typeof importMode){
    setImportMode(mode);setArchiveChecked(false);setPreview(emptyPreview);
  }
  function closeImport(){
    return perform(async()=>{
      await run({op:"csv_cancel"});setPreview(null);setRows([]);setImporting(false);
      setArchiveFile(null);setArchiveSecret("");setArchiveRecovery(false);setArchiveChecked(false);
    });
  }
  function closeEditor(){setDraft(null);setEditing(null);setNotice("")}
  function openEdit(item:Summary){
    hideReveal();
    return perform(async()=>{
      const f=await run<Draft>({op:"edit_fields",item:item.id});
      setDraft({...f,secret:null,totp:null});setEditing(item.id);setConfirmDelete(false);
    });
  }
  function deleteItem(item:Summary){
    return perform(async()=>{await run({op:"delete",item:item.id});await publish();clearSelection()});
  }
  function disconnect(){lock();void fetch("/session/logout",{method:"POST",credentials:"same-origin"}).finally(()=>setConnected(false))}
  const importTitle=importMode==="legacy_v1"?"구 v1 볼트 파일 이전":importMode==="archive"?"암호화 파일 가져오기":"CSV 가져오기";
  const titles={list:"안전한 로그인 정보 저장소",detail:selectedItem?itemName(selectedItem):"",editor:editing?"항목 수정":"새 항목",settings:"볼트 설정",import:importTitle};
  return <main className={`vault-shell${isWorkspace?" vault-unlocked":""}`}>
    {isWorkspace?<header className="vault-nav">
      {screen==="list"&&assistant?
        <a className="vault-icon-button" href={`${assistant}/app`} aria-label="비서로 돌아가기" onClick={()=>lock()}><VaultIcon name="back"/></a>:
        <button className="vault-icon-button" type="button" aria-label={screen==="list"?"볼트 잠그기":"뒤로"} disabled={frozen} onClick={()=>{
          if(screen==="editor")closeEditor();
          else if(screen==="import")void closeImport();
          else if(screen==="settings"){setSettings(false);setNotice("")}
          else if(screen==="detail"){clearSelection();setNotice("")}
          else lock();
        }}><VaultIcon name="back"/></button>}
      <h1>{titles[screen]}</h1>
      <div className="vault-nav-actions">
        <button className="vault-icon-button" type="button" aria-label="잠그기" title="잠그기" onClick={()=>lock()}><VaultIcon name="lock"/></button>
        {screen==="list"&&<button className="vault-icon-button" type="button" aria-label="새 항목" disabled={frozen} onClick={()=>openListAction(()=>{
          setDraft({...empty});setEditing(null);
        })}><VaultIcon name="add"/></button>}
        {showingDetail&&selectedItem&&<button className="vault-icon-button" type="button" aria-label="수정" disabled={frozen||confirmDelete} onClick={()=>void openEdit(selectedItem)}><VaultIcon name="edit"/></button>}
      </div>
    </header>:<>
      <header><a className="brand" href={assistant||"#"}>무수리 <span>볼트</span></a><div className="header-actions">{connected&&<><button onClick={()=>lock()} disabled={!unlocked&&!recovery}>잠그기</button><button className="quiet" onClick={disconnect}>연결 끊기</button></>}</div></header>
      <div className="intro"><span className="eyebrow">나만 여는 보관함</span><h1>소중한 정보는 내 기기에서</h1><p>계정 로그인과 볼트 열기는 별개입니다. 볼트 비밀번호와 복구 키로만 내용을 열 수 있습니다.</p></div>
    </>}
    {notice&&<p className="notice" role="status">{notice}</p>}
    {uncertain&&<div className="reconcile"><strong>저장을 확인하는 중입니다</strong><p>암호문은 새로 만들지 않고 같은 저장을 확인합니다.</p><button disabled={busy} onClick={()=>void perform(async()=>{await run({op:"reconcile"});await saved()})}>저장 결과 확인</button><button disabled={busy} onClick={()=>void perform(async()=>{await run({op:"cancel"});if(importing){const status=await run<{committed:number}>({op:"csv_status"});setImported(status.committed)}setUncertain(false);setRecovery(null);await run({op:"refresh"});if(unlocked)await list();setNotice("확인된 저장 상태를 반영했습니다.")})}>미완료 저장 취소</button></div>}
    {!connected?<section className="gate"><h2>볼트 인증이 필요합니다</h2><p>이 사이트를 열려면 무수리에서 발급한 볼트 세션이 필요합니다.</p>{assistant&&<a className="primary link-button" href={`${assistant}/app`}>무수리로 돌아가기</a>}</section>:
      recovery!==null?<section className="gate recovery"><h2>복구 키를 안전하게 보관하세요</h2><p>볼트 비밀번호를 잊었을 때 필요합니다. 기록한 키를 아래에 한 번 입력해 확인해 주세요.</p><textarea aria-label="새 복구 키" value={recovery} readOnly spellCheck={false}/><label>기록한 복구 키<input autoComplete="off" spellCheck={false} value={verify} onChange={e=>setVerify(e.target.value)}/></label><div className="actions"><button className="primary" disabled={busy||!verify} onClick={()=>void perform(async()=>{await run({op:"verify_recovery",recovery:verify.trim()});await publish()})}>저장하고 열기</button><button disabled={busy||uncertain} onClick={()=>void perform(async()=>{await run({op:"cancel"});setRecovery(null);setVerify("")})}>취소</button></div></section>:
      !unlocked?<section className="gate"><h2>{initialized?"볼트 열기":"내 볼트 만들기"}</h2><form onSubmit={e=>{e.preventDefault();void perform(async()=>{
        if(initialized){await run({op:"unlock",secret:password,recovery:useRecovery});setPassword("");setUnlocked(true);await list()}
        else{const result=await run<{recovery:string}>({op:"setup",password});setPassword("");setRecovery(result.recovery)}
      })}}><label>{useRecovery?"복구 키":"볼트 비밀번호"}<input type="password" autoComplete={initialized?"current-password":"new-password"} value={password} onChange={e=>setPassword(e.target.value)} required minLength={initialized?undefined:12} maxLength={1024}/></label>{!initialized&&<p className="hint">기억하기 어려운 긴 비밀번호를 정하고 복구 키를 별도로 보관하세요. 관리자가 계정 비밀번호를 바꿔도 볼트는 열리지 않습니다.</p>}{initialized&&<label className="check"><input type="checkbox" checked={useRecovery} onChange={e=>{setUseRecovery(e.target.checked);setPassword("")}}/>복구 키로 열기</label>}<button className="primary" disabled={busy||!password}>{busy?"확인 중…":initialized?"열기":"만들기"}</button></form></section>:
      <>
        {screen==="list"&&<section className="vault-list" aria-label="보관한 항목">
          <div className="vault-list-tools">
            <button type="button" className="vault-icon-button" aria-label="항목 찾기" aria-expanded={searchOpen} disabled={frozen} onClick={()=>{
              setSearchOpen(v=>!v);
              if(searchOpen)setQuery("");
            }}><VaultIcon name="search"/></button>
            <button type="button" className="vault-icon-button" aria-label="볼트 설정" disabled={frozen} onClick={()=>openListAction(()=>setSettings(true))}><VaultIcon name="menu"/></button>
          </div>
          {searchOpen&&<label className="vault-search"><span className="vault-sr">항목 찾기</span>
            <input type="search" value={query} disabled={frozen} onChange={e=>setQuery(e.target.value)} placeholder="이름, 사용자 이름, 주소" autoFocus/>
          </label>}
          <VaultList items={items} query={query} disabled={frozen} onOpen={openItem}/>
        </section>}
        {screen==="editor"&&draft&&<section aria-label={titles.editor}>
          <form onSubmit={e=>{e.preventDefault();void perform(async()=>{await run({op:"put",item:editing,fields:draft});await publish()})}} className="editor">
            <fieldset disabled={frozen}>
              <label>종류<select value={draft.kind} disabled={frozen} onChange={e=>setDraft({...draft,kind:e.target.value as Draft["kind"]})}>
                <option value="login">로그인</option><option value="api_key">API 키</option><option value="note">보안 메모</option>
              </select></label>
              <label>이름<input value={draft.title} onChange={e=>setDraft({...draft,title:e.target.value})} maxLength={512} required/></label>
              <label>사용자 이름<input value={draft.username} onChange={e=>setDraft({...draft,username:e.target.value})} autoComplete="off"/></label>
              <label>주소<input value={draft.url} onChange={e=>setDraft({...draft,url:e.target.value})} autoComplete="off"/></label>
              <label>비밀번호 또는 키<input type="password" value={draft.secret??""} onChange={e=>setDraft({...draft,secret:e.target.value})} autoComplete="new-password" placeholder={editing?"입력하지 않으면 유지됩니다":""}/></label>
              <button type="button" disabled={frozen} onClick={()=>void perform(async()=>{setDraft({...draft,secret:await run<string>({op:"generate_password",length:24})})})}>안전한 비밀번호 만들기</button>
              <label>메모<textarea value={draft.notes} onChange={e=>setDraft({...draft,notes:e.target.value})}/></label>
              <label>인증 코드 설정<input type="password" value={draft.totp??""} onChange={e=>setDraft({...draft,totp:e.target.value})} placeholder={editing?"입력하지 않으면 유지됩니다":"otpauth 주소 또는 Base32 키"} autoComplete="off"/></label>
              <label>만료일<input type="date" value={draft.expires_at} onChange={e=>setDraft({...draft,expires_at:e.target.value})}/></label>
              <label className="check"><input type="checkbox" checked={draft.favorite} onChange={e=>setDraft({...draft,favorite:e.target.checked})}/>즐겨찾기</label>
              <div className="actions"><button className="primary" disabled={frozen}>저장</button><button type="button" disabled={frozen} onClick={closeEditor}>닫기</button></div>
            </fieldset>
          </form>
        </section>}
        {showingDetail&&selectedItem&&<section className="vault-detail" aria-label="보관 항목 상세">
          {(fields?.has_url||selectedItem.kind==="login")&&<div className="vault-card">
            <VaultField name="주소" disabled={frozen} onReveal={fields?.has_url?()=>void reveal("url"):undefined} onCopy={fields?.has_url?()=>void copy("url"):undefined}>
              {fields?.has_url?fields.host||"저장된 주소":fields&&!fields.failed?"주소 없음":itemName(selectedItem)}
            </VaultField>
          </div>}
          {fields?.failed&&<div className="vault-card vault-row vault-field">
            <span className="vault-value">항목 정보를 불러오지 못했습니다.</span>
            <button type="button" disabled={frozen} onClick={()=>{setDetail(null);setDetailAttempt(v=>v+1)}}>다시 불러오기</button>
          </div>}
          <div className="vault-card">
            {(fields?.username||selectedItem.kind==="login")&&<VaultField name="사용자 이름" disabled={frozen} onCopy={fields?.username?()=>void copy("username"):undefined}>
              {fields?.username||(fields&&!fields.failed?"사용자 이름 없음":"사용자 이름")}
            </VaultField>}
            <VaultField name={selectedItem.kind==="login"?"비밀번호":"비밀번호 또는 키"} disabled={frozen} onReveal={()=>void reveal("secret")} onCopy={()=>void copy("secret")}>
              <span className="vault-sr">가려짐</span><span className="vault-mask" aria-hidden="true">••••••••••</span>
            </VaultField>
          </div>
          {revealed&&<div className="vault-card revealed">
            <pre>{revealed.value||"비어 있습니다."}</pre><small>잠시 뒤 표시가 닫힙니다.</small>
            <button type="button" onClick={hideReveal}>지금 닫기</button>
          </div>}
          {selectedItem.kind==="login"&&<section className="vault-permission" aria-labelledby="permission-heading">
            <h2 id="permission-heading" className="vault-caption">에이전트 권한</h2>
            <div className="vault-card vault-row vault-permission-status">
              <span>직접 사용</span>
              <small>이 로그인은 보관함에서 직접 열어 확인하거나 복사할 수 있습니다.</small>
            </div>
          </section>}
          {otp&&<div className="otp">
            <span>인증 코드</span><strong>{otp.code}</strong>
            <progress aria-label="인증 코드 남은 시간" value={Math.max(0,otp.valid_until-clock/1000)} max={otp.valid_until-otp.valid_from}/>
            <small>{Math.max(0,Math.ceil(otp.valid_until-clock/1000))}초 뒤 갱신</small>
            <button disabled={frozen} onClick={()=>void copy("code")}>현재 코드 복사</button>
          </div>}
          <details className="vault-card vault-extra">
            <summary>메모{selectedItem.expires_at&&" 및 만료일"}</summary>
            <VaultField name="메모" disabled={frozen} onReveal={()=>void reveal("notes")} onCopy={()=>void copy("notes")}>메모 내용</VaultField>
            {selectedItem.expires_at&&<p className="vault-row">만료일 {selectedItem.expires_at}{selectedItem.expires_at<new Date().toISOString().slice(0,10)?" · 만료됨":""}</p>}
          </details>
          <div className="vault-delete">
            {confirmDelete?<div className="vault-card vault-delete-confirm" role="group" aria-label="삭제 확인">
              <p>이 항목을 삭제할까요?</p>
              <div className="actions">
                <button type="button" autoFocus disabled={frozen} onClick={()=>setConfirmDelete(false)}>취소</button>
                <button type="button" className="danger" disabled={frozen} onClick={()=>void deleteItem(selectedItem)}>항목 삭제</button>
              </div>
            </div>:<button type="button" className="danger" disabled={frozen} onClick={()=>{hideReveal();setConfirmDelete(true)}}>삭제</button>}
          </div>
        </section>}
      </>}
    {connected&&oldPending.length>0&&!uncertain&&<section className="panel"><h2>이전에 끝나지 않은 저장이 있습니다</h2><p>새 저장을 시작하기 전에 상태를 확인하거나 미완료 저장을 취소해 주세요.</p>{oldPending.map(r=><button key={r.operation} disabled={busy} onClick={()=>void perform(async()=>{const result=await run<{pending:Receipt[]}>({op:"discard_old",id:r.operation});setOldPending(result.pending);setNotice("미완료 저장을 취소했습니다.")})}>미완료 저장 취소</button>)}</section>}
    {isWorkspace&&screen==="settings"&&<section className="panel settings">
      <div className="vault-card vault-settings-links">
        <button type="button" disabled={frozen} onClick={()=>openImport("csv")}>CSV 가져오기<VaultIcon name="next"/></button>
        <button type="button" disabled={frozen} onClick={()=>openImport("archive")}>암호화 파일 가져오기<VaultIcon name="next"/></button>
        <button type="button" disabled={frozen} onClick={()=>openImport("legacy_v1")}>구 v1 볼트 파일 이전<VaultIcon name="next"/></button>
        <button type="button" disabled={frozen} onClick={()=>void exportArchive()}>암호화 파일 내보내기</button>
        <button type="button" onClick={disconnect}>연결 끊기</button>
      </div>
      <h2>볼트 보호 설정</h2><label>새 볼트 비밀번호<input type="password" value={newPassword} onChange={e=>setNewPassword(e.target.value)} autoComplete="new-password"/></label><div className="actions"><button disabled={frozen||preview!==null||newPassword.length<12} onClick={()=>void keyChange("prepare_password")}>비밀번호 변경</button><button disabled={frozen||preview!==null} onClick={()=>void keyChange("prepare_recovery")}>복구 키 새로 만들기</button><button disabled={frozen||preview!==null||newPassword.length<12} onClick={()=>void keyChange("prepare_rotation")}>모든 항목의 보호 키 교체</button></div><p className="hint">비밀번호만 변경하면 이미 보관된 옛 사본의 보호 키는 유지됩니다. 키 노출이 의심되면 보호 키를 교체하세요. 이미 내려받은 사본을 소프트웨어로 지울 수는 없습니다.</p></section>}
    {isWorkspace&&screen==="import"&&preview&&<section className="panel import" aria-label={importTitle}><p>파일은 이 기기에서만 읽습니다. 원본 파일은 브라우저가 지울 수 없으므로 완료 후 직접 정리해 주세요.</p>{importMode!=="csv"?<><p>원래 서버 없이 내보낼 때의 볼트 비밀번호 또는 복구 키로 확인합니다. 선택한 항목은 지금 볼트의 새 항목으로 저장됩니다. 전체 파일을 확인하기 전에는 항목을 표시하지 않습니다.</p>{importMode==="legacy_v1"&&<p>이전 무수리의 musuri-vault v1 암호화 내보내기 파일만 지원합니다. 당시 비밀번호 또는 MR1 복구 키가 필요하며, 최대 256개·26MiB까지 파일 전체를 검증합니다. 파일에 담긴 범위만 이전하며 원래 서버의 최신·전체 자료임을 보장하지 않습니다.</p>}{!archiveChecked&&<><label>암호화 볼트 파일<input type="file" accept=".json,application/json" disabled={frozen||importing} onChange={e=>{setArchiveFile(e.target.files?.[0]??null);e.target.value=""}}/></label><label>{archiveRecovery?"파일의 복구 키":"파일의 볼트 비밀번호"}<input type="password" autoComplete="off" value={archiveSecret} disabled={frozen} maxLength={1024} onChange={e=>setArchiveSecret(e.target.value)}/></label><label className="check"><input type="checkbox" checked={archiveRecovery} disabled={frozen} onChange={e=>{setArchiveRecovery(e.target.checked);setArchiveSecret("")}}/>복구 키 사용</label><button disabled={frozen||!archiveFile||!archiveSecret} onClick={()=>void perform(async()=>{try{const result=await run<Preview>({op:importMode==="legacy_v1"?"legacy_v1_file":"archive_file",file:archiveFile,secret:archiveSecret,recovery:archiveRecovery});setPreview(result);setArchiveChecked(true);setRows(result.rows.filter(r=>r.status==="new"||r.status==="changed").map(r=>r.row))}finally{setArchiveFile(null);setArchiveSecret("")}})}>파일 전체 확인</button></>}</>:<><label className="check"><input type="checkbox" checked={filterExact} disabled={frozen||importing} onChange={e=>setFilterExact(e.target.checked)}/>특정 사용자 이름만 선택</label>{filterExact&&<label>사용자 이름<input value={username} disabled={frozen||importing} onChange={e=>setUsername(e.target.value)}/></label>}<label className="file">Apple 암호 CSV 선택<input type="file" accept=".csv,text/csv" disabled={frozen||importing} onChange={e=>{const file=e.target.files?.[0];e.target.value="";if(file)void perform(async()=>{const result=await run<Preview>({op:"csv_file",file,username:filterExact?username:null});setPreview(result);setRows(result.rows.filter(r=>r.status==="new"||r.status==="changed").map(r=>r.row))})}}/></label></>}{(preview.rows.length>0||archiveChecked)&&<><p>선택할 수 있는 항목 {preview.eligible}개 · 중복 {preview.duplicates}개{importing?` · 저장 완료 ${imported}개 · 남음 ${Math.max(0,rows.length-imported)}개`:""}</p><div className="import-rows">{preview.rows.map(r=><label className="check" key={r.row}><input type="checkbox" disabled={frozen||importing||r.status==="duplicate"||r.status==="filtered"} checked={rows.includes(r.row)} onChange={e=>setRows(v=>e.target.checked?[...v,r.row]:v.filter(n=>n!==r.row))}/>{r.row}행 · {r.status==="changed"?"같은 계정의 다른 내용":r.status==="duplicate"?"중복":r.status==="filtered"?"선택 조건 제외":"새 항목"}{r.blank_password?" · 비밀번호 없음":""}{r.unsupported_url?" · 주소 확인 필요":""}{r.invalid_otp?" · 인증 코드 설정 확인 필요":""}</label>)}</div><button className="primary" disabled={frozen||!rows.length} onClick={()=>void importSelected()}>{importing?"남은 항목 계속 저장":"선택한 항목 저장"}</button></>}{busy&&importMode!=="csv"&&<button onClick={()=>lock()}>잠그고 가져오기 중단</button>}{busy&&importing&&<button onClick={()=>{importCancel.current=true}}>가져오기 중단</button>}<button disabled={busy||uncertain} onClick={()=>void closeImport()}>가져오기 닫기</button></section>}
    {(!isWorkspace||screen==="settings")&&<footer>내용은 기기에서 암호화됩니다. 이 웹앱을 제공하는 서버와 사용하는 기기는 신뢰해야 합니다. 암호화 파일에는 내보낼 당시의 보호 설정이 남습니다. 이후 비밀번호나 보호 키를 바꿔도 옛 파일은 옛 비밀번호·복구 키로 열립니다. 파일 이름은 고정이며 항목 이름은 암호화됩니다. 소유자 식별자·항목 수는 파일에서 보일 수 있습니다. 비서의 자동 사용 연결은 아직 지원하지 않습니다.</footer>}
  </main>
}
createRoot(document.getElementById("root")!).render(<StrictMode><App/></StrictMode>);
