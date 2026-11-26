import {StrictMode,useEffect,useLayoutEffect,useRef,useState} from "react";
import {createRoot} from "react-dom/client";
import {WorkerClient,VaultFailure} from "./worker-client";
import type {Draft,EntryDetails,Owner,Preview,Receipt,Summary,Command,CurrentSession} from "./types";
import {VaultField,VaultIcon,VaultList,itemName} from "./workspace";
import {sealGoogleLogin, validJob, type KiriRecipient} from "./kiri-google";
import {validGoogleEmail, type GoogleLogin} from "./google-login";
import {MAX_ITEMS,MAX_TERMS,formatMiB,initialSelection,scopeLabel,selectable,selectionState,selector} from "./import-filter";
import type {FilterMode} from "./import-filter";
import {newPasswordIssue} from "./password";
import "./style.css";
const empty:Draft={kind:"login",title:"",username:"",url:"",notes:"",favorite:false,expires_at:"",secret:"",totp:""};
const emptyPreview:Preview={eligible:0,duplicates:0,filtered:0,rows:[]};
type Field="secret"|"username"|"url"|"notes";
type Otp = { code: string; valid_from: number; valid_until: number };
function validCode(value: Otp, now: number) {
  return value.valid_from * 1000 <= now && now < value.valid_until * 1000;
}
type Detail=EntryDetails&{item:string;revision:number;failed:boolean};
const errorText=(e:unknown)=>e instanceof Error&&e.message==="vault_authentication_failed"?"비밀번호 또는 복구 키를 확인해 주세요.":e instanceof Error&&e.message==="vault_conflict"?"다른 창에서 변경되었습니다. 잠근 뒤 다시 열어 주세요.":e instanceof Error&&e.message==="vault_limit"?"파일 크기, 항목 수 또는 전체 저장 용량이 한도를 넘었습니다. 선택을 줄이거나 필요 없는 항목을 정리해 주세요.":e instanceof Error&&["vault_invalid_input","vault_unsupported","vault_kdf_policy"].includes(e.message)?"입력 또는 파일 형식을 확인해 주세요.":"연결을 확인하지 못했습니다. 잠시 뒤 다시 시도해 주세요.";
function App(){
  // busyRef is the synchronous guard: React state lags a render behind a fast double click.
  const searchOwner=useRef(0),importCancel=useRef(false),busyRef=useRef(false);
  const client=useRef<WorkerClient|null>(null),generation=useRef(0),ownerRef=useRef<Owner|null>(null),pendingStartup=useRef<Promise<unknown>|null>(null);
  const [assistant,setAssistant]=useState(""),[connected,setConnected]=useState(false),[initialized,setInitialized]=useState(true),[unlocked,setUnlocked]=useState(false),[busy,setBusy]=useState(false),[notice,setNotice]=useState("");
  const [googleEmail,setGoogleEmail]=useState("");
  const [googleJob,setGoogleJob]=useState("");
  const [googlePassword,setGooglePassword]=useState("");
  const [newGoogleEmail,setNewGoogleEmail]=useState("");
  const [googleAttempt,setGoogleAttempt]=useState(0);
  const sentJob=useRef("");
  const [googleSelection,setGoogleSelection]=useState<{ email: string; items: GoogleLogin[] } | null>(null);
  const [password,setPassword]=useState(""),[useRecovery,setUseRecovery]=useState(false),[recovery,setRecovery]=useState<string|null>(null),[verify,setVerify]=useState("");
  const [items,setItems]=useState<Summary[]>([]),[query,setQuery]=useState(""),[selected,setSelected]=useState<string|null>(null),[draft,setDraft]=useState<Draft|null>(null),[editing,setEditing]=useState<string|null>(null);
  const [revealed,setRevealed]=useState<{value:string;until:number}|null>(null),[uncertain,setUncertain]=useState(false),[oldPending,setOldPending]=useState<Receipt[]>([]),[settings,setSettings]=useState(false),[newPassword,setNewPassword]=useState("");
  const [preview,setPreview]=useState<Preview|null>(null),[rows,setRows]=useState<number[]>([]),[username,setUsername]=useState(""),[filterMode,setFilterMode]=useState<FilterMode>("all"),[terms,setTerms]=useState(""),[imported,setImported]=useState(0),[importing,setImporting]=useState(false),[scope,setScope]=useState("");
  const [importMode,setImportMode]=useState<"csv"|"archive"|"legacy_v1">("csv"),[archiveFile,setArchiveFile]=useState<File|null>(null),[archiveSecret,setArchiveSecret]=useState(""),[archiveRecovery,setArchiveRecovery]=useState(false),[archiveChecked,setArchiveChecked]=useState(false);
  const [otp,setOtp]=useState<Otp|null>(null),[clock,setClock]=useState(Date.now());
  const [checkingSession,setCheckingSession]=useState(true),[confirmDelete,setConfirmDelete]=useState(false);
  const [detail,setDetail]=useState<Detail|null>(null),[detailAttempt,setDetailAttempt]=useState(0);
  const listScroll=useRef(0),revealOwner=useRef(0),disconnecting=useRef(false);
  const visibleItems = googleEmail ? (googleSelection?.email === googleEmail ? googleSelection.items : []) : items;
  const selectedItem=visibleItems.find(i=>i.id===selected),frozen=busy||recovery!==null||uncertain;
  const isWorkspace=connected&&unlocked&&recovery===null;
  const screen=draft?"editor":preview?"import":settings?"settings":selectedItem?"detail":"list";
  const showingDetail=isWorkspace&&screen==="detail";
  const fields=detail?.item===selectedItem?.id&&detail?.revision===selectedItem?.revision?detail:null;
  function hideReveal(){revealOwner.current++;setRevealed(null)}
  function clearSelection(){setSelected(null);setDetail(null);setOtp(null);setConfirmDelete(false);hideReveal()}
  function lock(reason="vault_locked"){
    setGoogleEmail("");setGoogleJob("");setGooglePassword("");sentJob.current="";
    setGoogleSelection(null);
    generation.current++;client.current?.close();client.current=null;pendingStartup.current=null;busyRef.current=false;
    setUnlocked(false);setBusy(false);setScope("");setPassword("");setRecovery(null);setVerify("");setItems([]);setQuery("");clearSelection();setDraft(null);setEditing(null);
    setPreview(null);setRows([]);setImporting(false);setUsername("");setFilterMode("all");setTerms("");setImported(0);importCancel.current=true;
    setSettings(false);setNewPassword("");setUncertain(false);setArchiveFile(null);setArchiveSecret("");setArchiveRecovery(false);setArchiveChecked(false);setUseRecovery(false);listScroll.current=0;
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
    if(busy||busyRef.current)return;busyRef.current=true;const epoch=generation.current;setBusy(true);setNotice("");
    try{await action()}catch(e){if(epoch===generation.current){if(e instanceof VaultFailure&&e.uncertain){setUncertain(true);setNotice("저장 결과를 확인하지 못했습니다. 같은 저장을 다시 확인해 주세요.")}else setNotice(errorText(e));}}
    finally{if(epoch===generation.current){busyRef.current=false;setBusy(false)}}
  }
  async function list(text=query){const epoch=generation.current,sequence=++searchOwner.current;const result=await run<Summary[]>({op:"list",query:text});if(epoch===generation.current&&sequence===searchOwner.current)setItems(result)}
  useEffect(() => {
    if (!connected || !assistant || !window.opener || !ownerRef.current) return;
    const account = ownerRef.current.account;
    const receive = (event: MessageEvent) => {
      if (event.origin === assistant && event.source === window.opener &&
          event.data?.kind === "moosuri-vault-google-account" && event.data.account === account &&
          ownerRef.current?.account === account && typeof event.data.email === "string" &&
          event.data.email.length <= 254 && /^[^@\s]+@[^@\s]+$/u.test(event.data.email)) {
        if (validJob(event.data.job)) {
          setGoogleEmail(event.data.email);
          setNewGoogleEmail(event.data.email);
          setGoogleJob(event.data.job);
        }
      }
    };
    window.addEventListener("message", receive);
    window.opener.postMessage({ kind: "moosuri-vault-google-context", account }, assistant);
    return () => window.removeEventListener("message", receive);
  }, [assistant, connected, unlocked]);
  useEffect(()=>{
    if(!unlocked||frozen)return;
    const epoch=generation.current;
    const timer=setTimeout(()=>{
      if (epoch !== generation.current) return;
      if (!googleEmail) { void list(query).catch(()=>{}); return; }
      const sequence = ++searchOwner.current;
      void run<GoogleLogin[]>({ op: "google_accounts" }).then(matches => {
        if (epoch !== generation.current || sequence !== searchOwner.current) return;
        setGoogleSelection({ email: googleEmail, items: matches });
      }).catch(() => {
        if (epoch === generation.current && sequence === searchOwner.current) {
          setGoogleSelection(null);
          setNotice("저장된 Google 로그인을 확인하지 못했습니다.");
        }
      });
    },250);
    return()=>{clearTimeout(timer);searchOwner.current++};
  },[query,unlocked,frozen,googleEmail,googleAttempt]);
  async function sendToKiri(item: GoogleLogin) {
    if (!googleJob || sentJob.current === googleJob || !ownerRef.current || !window.opener) return;
    const epoch = generation.current, account = ownerRef.current.account;
    sentJob.current = googleJob;
    try {
      const r = await fetch("/session/kiri-recipient?challenge="+encodeURIComponent(googleJob), {cache:"no-store",credentials:"same-origin"});
      if (!r.ok || epoch !== generation.current) throw Error();
      const recipient: KiriRecipient = await r.json();
      const envelope = await sealGoogleLogin(recipient, account, googleEmail, googleJob, item, run, {tenant:ownerRef.current!.tenant,origin:assistant});
      if (epoch !== generation.current) return;
      window.opener.postMessage({kind:"moosuri-vault-kiri-sealed", job:googleJob, account, email:item.email, envelope}, assistant);
      setNotice("Kiri에서 이 계정으로 로그인하고 있습니다.");
    } catch {
      if (epoch === generation.current) setNotice("Kiri에 로그인 항목을 전달하지 못했습니다. 연결 화면에서 다시 시도하세요.");
    }
  }
  useEffect(() => {
    if (unlocked && !frozen && googleJob && googleSelection?.email === googleEmail && googleSelection.items.length === 1)
      void sendToKiri(googleSelection.items[0]);
  }, [googleSelection, googleJob, googleEmail, unlocked, frozen]);
  async function saved(){const epoch=generation.current;await list();if(epoch!==generation.current)return;setInitialized(true);setUnlocked(true);setRecovery(null);setVerify("");setDraft(null);setEditing(null);setConfirmDelete(false);setUncertain(false);setOldPending([]);if(importing){const status=await run<{committed:number}>({op:"csv_status"});setImported(status.committed)}setNotice("저장했습니다.")}
  async function publish(){await run({op:"publish"});await saved()}
  useEffect(()=>{
    let live=true;
    void fetch("/public/config",{cache:"no-store"}).then(r=>r.json()).then(v=>{if(live&&typeof v.assistant_origin==="string")setAssistant(v.assistant_origin)}).catch(()=>{});
    void start().catch(()=>{if(live){client.current?.close();client.current=null;setConnected(false)}}).finally(()=>{if(live)setCheckingSession(false)});
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
    setOtp(null);
    if (!showingDetail || !selectedItem?.has_totp || frozen) return;
    const epoch = generation.current;
    let live = true, working = false;
    let current: Otp | null = null;
    const update = async () => {
      if (!live || document.hidden || epoch !== generation.current) return;
      const now = Date.now();
      setClock(now);
      if (current && validCode(current, now)) return;
      current = null;
      setOtp(null);
      if (working) return;
      working = true;
      try {
        const value = await run<Otp>({op: "totp", item: selectedItem.id});
        const receivedAt = Date.now();
        if (live && epoch === generation.current && validCode(value, receivedAt)) {
          current = value;
          setClock(receivedAt);
          setOtp(value);
        }
      } catch {
        // Leave the code hidden until the next refresh succeeds.
      } finally { working = false; }
    };
    void update();
    const timer = setInterval(update, 1000);
    window.addEventListener("focus", update);
    document.addEventListener("visibilitychange", update);
    return () => {
      live = false;
      clearInterval(timer);
      window.removeEventListener("focus", update);
      document.removeEventListener("visibilitychange", update);
    };
  },[selected,showingDetail,frozen,selectedItem?.has_totp,selectedItem?.revision]);
  async function reveal(field:Field){if(!selected)return;await perform(async()=>{const sequence=++revealOwner.current;const value=await run<string>({op:"reveal",item:selected,field});if(sequence===revealOwner.current)setRevealed({value,until:Date.now()+15000})})}
  async function copy(field:Field|"code"){if(!selected)return;await perform(async()=>{
    const epoch=generation.current;
    if(field==="code"){
      for(let attempt=0;attempt<2;attempt++){
        const value=await run<Otp>({op:"totp",item:selected});
        if(epoch!==generation.current)return;
        if(!validCode(value,Date.now()))continue;
        await navigator.clipboard.writeText(value.code);
        if(epoch!==generation.current)return;
        if(validCode(value,Date.now())){setNotice("현재 인증 코드를 복사했습니다.");return}
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
    if(result.needs_verification){setVerify("");setRecovery(result.recovery)}else await publish();
  })}
  async function importSelected(){await perform(async()=>{
    importCancel.current=false;
    const epoch=generation.current,status=()=>run<{committed:number;selected:number}>({op:"csv_status"});
    let started=importing;
    try{
      // Selecting again is idempotent in the engine: it keeps the confirmed count.
      if(!started){await run({op:importMode!=="csv"?"archive_select":"csv_select",rows});started=true;setImporting(true);setImported((await status()).committed)}
      // A null from csv_next means every selected item is confirmed; a capacity
      // problem is an error, never null.
      for(;!importCancel.current;){const next=await run<{remaining:number}|null>({op:"csv_next"});if(!next)break;await run({op:"publish"});setImported(v=>v+1)}
    }catch(e){
      if(!started||epoch!==generation.current||(e instanceof VaultFailure&&e.uncertain))throw e;
      // Items are saved one by one, so an interruption leaves a partial but valid vault.
      let known="";
      try{const s=await status();setImported(s.committed);known=` 저장이 확인된 항목은 ${s.committed}개/${s.selected}개입니다.`}catch{}
      setNotice(`${errorText(e)}${known} 저장한 항목은 유지되며 '남은 항목 계속 저장'으로 이어서 저장할 수 있습니다.`);
      return;
    }
    const done=await status();
    await run({op:"csv_cancel"});setImporting(false);setPreview(null);setRows([]);setScope("");await saved();
    if(importCancel.current)setNotice(`가져오기를 중단했습니다. 저장이 확인된 항목 ${done.committed}개/${done.selected}개는 유지됩니다. 나머지는 같은 파일을 다시 가져오면 이미 저장된 항목은 중복으로 건너뛰고 이어서 저장할 수 있습니다.`);
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
  function adopt(result:Preview,label:string){
    // Select everything only when all of it fits; otherwise nothing, so no row is
    // dropped without the user choosing. The engine also refuses a known
    // over-limit selection before sealing anything.
    const start=initialSelection(result.rows,result.capacity??MAX_ITEMS);
    setPreview(result);setRows(start.rows);setScope(label);
    setNotice(start.overCapacity?`선택할 수 있는 항목 ${start.available}개가 남은 자리 ${Math.max(0,result.capacity??MAX_ITEMS)}개보다 많아 아무것도 선택하지 않았습니다. 저장할 항목을 직접 고르세요.`:"");
  }
  function resetPreview(){
    return perform(async()=>{await run({op:"csv_cancel"});setPreview(emptyPreview);setRows([]);setScope("");setNotice("")});
  }
  function closeImport(){
    return perform(async()=>{
      await run({op:"csv_cancel"});setPreview(null);setRows([]);setScope("");setImporting(false);
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
  async function disconnect() {
    if (disconnecting.current) return;
    disconnecting.current = true;
    lock();
    setBusy(true);
    try {
      const response = await fetch("/session/logout", {method:"POST", credentials:"same-origin"});
      if (!response.ok && response.status !== 401) throw new Error("vault_disconnect_unconfirmed");
      setConnected(false);
      setNotice("");
    } catch {
      setNotice("볼트를 잠갔습니다. 연결 끊기를 완료하지 못했습니다. 다시 시도해 주세요.");
    } finally {
      disconnecting.current = false;
      setBusy(false);
    }
  }
  const importTitle=importMode==="legacy_v1"?"구 v1 볼트 파일 이전":importMode==="archive"?"암호화 파일 가져오기":"CSV 가져오기";
  const previewLocked=preview!==null&&(preview.rows.length>0||preview.filtered>0||importing);
  const capacity=preview?.capacity??MAX_ITEMS,existingCount=preview?.existing??0,sel=selectionState(rows.length,existingCount,capacity,preview?.limit??MAX_ITEMS);
  const picked=selector(filterMode,username,terms),chosenRows=new Set(rows),selectableRows=preview?selectable(preview.rows):[];
  const titles = {list:"암호 볼트", detail:selectedItem ? itemName(selectedItem) : "",
    editor:editing ? "항목 수정" : "새 항목", settings:"볼트 설정", import:importTitle};
  const setupIssue = initialized ? null : newPasswordIssue(password);
  const changeIssue = settings ? newPasswordIssue(newPassword) : null;

  function back() {
    setNotice("");
    if (screen === "editor") closeEditor();
    else if (screen === "import") void closeImport();
    else if (screen === "settings") { setSettings(false); setNewPassword(""); }
    else clearSelection();
  }

  return <div className={isWorkspace ? "vault-shell" : "member-auth vault-gate-shell"}>
    <main className={isWorkspace ? "member-page vault-workspace" : "vault-gate-body"}>
    {isWorkspace ? <>
      <nav className="vault-nav" aria-label="볼트 탐색">
        {screen === "list" ? assistant && <a href={`${assistant}/app`} onClick={()=>lock()}>
          <VaultIcon name="back"/>무수리로 돌아가기
        </a> : <button type="button" aria-label="뒤로" disabled={frozen} onClick={back}>
          <VaultIcon name="back"/>{screen === "import" ? "볼트 설정" : screen === "editor" && editing ? "항목 상세" : "암호 볼트"}
        </button>}
        <button type="button" onClick={()=>lock()}><VaultIcon name="lock"/>잠그기</button>
      </nav>
      <header className="member-page-heading">
        <h1>{googleJob ? "Google 연결" : titles[screen]}</h1>
        {screen === "list" && !googleJob && <button className="primary" type="button" disabled={frozen}
          onClick={()=>openListAction(()=>{setDraft({...empty});setEditing(null)})}>
          <VaultIcon name="add"/>새 항목
        </button>}
        {showingDetail && selectedItem && <button type="button" disabled={frozen || confirmDelete}
          onClick={()=>void openEdit(selectedItem)}><VaultIcon name="edit"/>수정</button>}
      </header>
    </> : <div className="preview-brand">
      <img src="/moosuri-icon-192.png" width="48" height="48" alt=""/>Moosuri
    </div>}
    {notice&&<p className="notice" role="status">{notice}</p>}
    {uncertain&&<div className="reconcile"><strong>저장을 확인하는 중입니다</strong><p>암호문은 새로 만들지 않고 같은 저장을 확인합니다.</p><button disabled={busy} onClick={()=>void perform(async()=>{await run({op:"reconcile"});await saved()})}>저장 결과 확인</button><button disabled={busy} onClick={()=>void perform(async()=>{await run({op:"cancel"});if(importing){const status=await run<{committed:number}>({op:"csv_status"});setImported(status.committed)}setUncertain(false);setRecovery(null);await run({op:"refresh"});if(unlocked)await list();setNotice("확인된 저장 상태를 반영했습니다.")})}>미완료 저장 취소</button></div>}
    {checkingSession ? <p role="status">볼트 연결 확인 중…</p> : !connected ?
      <section className="gate preview-gate">
        <h1>무수리에서 볼트를 열어 주세요</h1>
        <p>로그인한 뒤 연결에서 암호 볼트를 선택하세요.</p>
        {assistant && <a className="primary link-button" href={`${assistant}/app`}>무수리로 돌아가기</a>}
      </section> : recovery !== null ?
      <section className="gate preview-gate recovery">
        <h1>복구 키를 안전하게 보관하세요</h1>
        <p>비밀번호를 잊었을 때 이 키로 볼트를 열 수 있습니다. 따로 보관한 뒤 입력해 주세요.</p>
        <label>새 복구 키<textarea value={recovery} readOnly spellCheck={false}/></label>
        <label>기록한 복구 키<input autoComplete="off" spellCheck={false} value={verify} onChange={e=>setVerify(e.target.value)}/></label>
        <div className="actions">
          <button className="primary" disabled={busy || uncertain || !verify} onClick={()=>void perform(async()=>{
            await run({op:"verify_recovery",recovery:verify.trim()}); await publish();
          })}>저장하고 열기</button>
          <button disabled={busy || uncertain} onClick={()=>void perform(async()=>{
            await run({op:"cancel"});setRecovery(null);setVerify("");
          })}>취소</button>
        </div>
      </section> : !unlocked ?
      <section className="gate preview-gate">
        <h1>{initialized ? "볼트 열기" : "내 볼트 만들기"}</h1>
        <p>{initialized ? (useRecovery ? "보관해 둔 복구 키를 입력하세요." : "볼트 비밀번호를 입력하세요.") :
          "로그인 정보와 인증 코드를 보관할 볼트 비밀번호를 정하세요."}</p>
        <form onSubmit={e=>{
          e.preventDefault();
          if (!password || setupIssue) return;
          void perform(async()=>{
            if (initialized) {
              await run({op:"unlock",secret:password,recovery:useRecovery});
              setPassword("");setUnlocked(true);if(!googleEmail)await list();
            } else {
              const result=await run<{recovery:string}>({op:"setup",password});
              setPassword("");setRecovery(result.recovery);
            }
          });
        }}>
          <label>{useRecovery ? "복구 키" : "볼트 비밀번호"}
            <input type="password" autoComplete={initialized ? (useRecovery ? "off" : "current-password") : "new-password"}
              value={password} onChange={e=>setPassword(e.target.value)} required maxLength={initialized ? 1024 : undefined}
              aria-describedby={initialized ? undefined : "setup-password-hint"}
              aria-invalid={(!initialized && !!password && !!setupIssue) || undefined}/>
          </label>
          {!initialized && <p className="hint" id="setup-password-hint">{setupIssue && password ? setupIssue : "8자 이상 · 다음 단계에서 복구 키를 받습니다."}</p>}
          {initialized && <label className="check"><input type="checkbox" checked={useRecovery}
            onChange={e=>{setUseRecovery(e.target.checked);setPassword("")}}/>복구 키로 열기</label>}
          <button className="primary" disabled={busy || !password || !!setupIssue}>{busy ? "확인 중…" : initialized ? "열기" : "만들기"}</button>
        </form>
      </section> :
      <>
        {screen==="list"&&<section className="vault-list" aria-label="보관한 항목">
          {googleEmail && <div className="notice" role="status">
            <strong>Gmail 계정 먼저 확인</strong>
            <p>내 볼트에 저장된 Google 계정에서 연결할 계정을 확인합니다.</p>
            {googleSelection?.email !== googleEmail ? <p>저장된 계정을 확인하고 있습니다. <button onClick={()=>setGoogleAttempt(n=>n+1)}>다시 확인</button></p>
              : googleSelection.items.length === 0 ? <form onSubmit={e=>{
                e.preventDefault();
                if (!googlePassword || !validGoogleEmail(newGoogleEmail)) return;
                void perform(async()=>{
                  const current=await run<GoogleLogin[]>({op:"google_accounts"});
                  if (!current.length) {
                    await run({op:"put",item:null,fields:{...empty,title:"Google",username:newGoogleEmail,
                      url:"https://accounts.google.com",secret:googlePassword}});
                    setGooglePassword("");await publish();
                  } else setGooglePassword("");
                  setGoogleAttempt(n=>n+1);
                });
              }}>
                <p>Google 계정이 볼트에 없습니다. 먼저 계정을 저장하면 그 다음 Kiri가 로그인을 시작합니다.</p>
                <label>Google 계정 이메일<input type="email" autoComplete="username" required disabled={frozen} value={newGoogleEmail}
                  onChange={e=>setNewGoogleEmail(e.target.value)}/></label>
                <label>Google 계정 비밀번호<input type="password" autoComplete="off" required disabled={frozen} value={googlePassword}
                  onChange={e=>setGooglePassword(e.target.value)}/></label>
                <button className="primary" disabled={frozen||!googlePassword||!validGoogleEmail(newGoogleEmail)}>볼트에 저장하고 계속</button>
              </form> : googleSelection.items.length > 1 ? <><p>Google 계정이 여러 개 있습니다. 연결할 계정을 선택하세요.</p>
                {googleSelection.items.map(item=><button key={item.id} disabled={frozen||sentJob.current===googleJob} onClick={()=>void sendToKiri(item)}>
                  {item.email} · {item.title} · 항목 {item.id.slice(-6)}
                </button>)}</>
                : <p>저장된 Gmail 계정을 확인했습니다. 이제 Kiri에서 로그인을 시작합니다.</p>}
          </div>}
          {!googleJob && <>
          <div className="vault-list-tools">
            {!googleEmail && <label className="vault-search">
              <VaultIcon name="search"/><span className="vault-sr">항목 찾기</span>
              <input type="search" value={query} disabled={frozen} onChange={e=>setQuery(e.target.value)}
                placeholder="이름, 사용자 이름, 주소 검색"/>
            </label>}
            <button type="button" aria-label="볼트 설정" disabled={frozen} onClick={()=>openListAction(()=>setSettings(true))}>
              <VaultIcon name="settings"/>설정
            </button>
          </div>
          <VaultList items={visibleItems} query={googleEmail || query} disabled={frozen} onOpen={openItem}/></>}
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
              <label className="editor-wide">메모<textarea value={draft.notes} onChange={e=>setDraft({...draft,notes:e.target.value})}/></label>
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
          {otp&&validCode(otp,clock)&&<div className="otp">
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
    {isWorkspace && screen === "settings" && <section className="settings">
      <section className="settings-group">
        <h2>볼트 보호 설정</h2>
        <div className="vault-card settings-body">
          <label>새 볼트 비밀번호<input type="password" value={newPassword} autoComplete="new-password"
            aria-describedby="change-password-hint" aria-invalid={(!!newPassword && !!changeIssue) || undefined}
            onChange={e=>setNewPassword(e.target.value)}/></label>
          <p id="change-password-hint" className="hint">{changeIssue && newPassword ? changeIssue : "8자 이상 입력하세요."}</p>
          <button className="primary" disabled={frozen || !!changeIssue} onClick={()=>void keyChange("prepare_password")}>비밀번호 변경</button>
          <div className="settings-row">
            <div><h3>복구 키</h3><p>새 키를 확인하고 저장하면 기존 복구 키는 만료됩니다.</p></div>
            <button disabled={frozen} onClick={()=>void keyChange("prepare_recovery")}>복구 키 새로 만들기</button>
          </div>
          <div className="settings-row">
            <div><h3>보호 키 교체</h3><p>키 노출이 의심되면 위에 새 비밀번호를 입력하세요. 모든 항목을 다시 암호화하고 새 복구 키를 발급합니다.</p></div>
            <button disabled={frozen || !!changeIssue} onClick={()=>void keyChange("prepare_rotation")}>모든 항목의 보호 키 교체</button>
          </div>
        </div>
      </section>
      <section className="settings-group">
        <h2>가져오기 · 내보내기</h2>
        <div className="vault-card vault-settings-links">
          <button disabled={frozen} onClick={()=>openImport("csv")}>CSV 가져오기<VaultIcon name="next"/></button>
          <button disabled={frozen} onClick={()=>openImport("archive")}>암호화 파일 가져오기<VaultIcon name="next"/></button>
          <button disabled={frozen} onClick={()=>openImport("legacy_v1")}>구 v1 볼트 파일 이전<VaultIcon name="next"/></button>
          <button disabled={frozen} onClick={()=>void exportArchive()}>암호화 파일 내보내기<VaultIcon name="next"/></button>
        </div>
        <p className="hint">내보낸 파일은 당시 비밀번호나 복구 키로 열립니다. 항목 이름과 내용은 암호화되며, 소유자 식별자와 항목 수는 파일에서 확인할 수 있습니다.</p>
      </section>
      <details className="vault-card security-info">
        <summary>볼트 보안 안내</summary>
        <p>볼트는 이 기기에서 암호화됩니다. 비밀번호와 복구 키는 신뢰하는 기기와 볼트 사이트에서만 입력하세요.</p>
        <p>비밀번호를 바꿔도 항목을 암호화한 키는 유지됩니다. 보호 키를 교체하면 현재 보관함의 항목을 새 키로 암호화합니다. 이미 내려받은 사본은 당시 비밀번호나 복구 키로 계속 열 수 있습니다.</p>
      </details>
      <button className="quiet" onClick={()=>void disconnect()}>연결 끊기</button>
    </section>}
    {isWorkspace&&screen==="import"&&preview&&<section className="panel import" aria-label={importTitle}><p>파일은 이 기기에서만 읽습니다. 원본 파일은 브라우저가 지울 수 없으므로 완료 후 직접 정리해 주세요.</p>{importMode!=="csv"?<><p>원래 서버 없이 내보낼 때의 볼트 비밀번호 또는 복구 키로 확인합니다. 선택한 항목은 지금 볼트의 새 항목으로 저장됩니다. 전체 파일을 확인하기 전에는 항목을 표시하지 않습니다.</p>{importMode==="archive"&&<p>이 볼트는 항목을 최대 {MAX_ITEMS.toLocaleString("ko-KR")}개까지 보관하며, 암호화 파일은 26MiB까지 확인합니다. 항목 하나가 매우 크면 개수보다 파일 크기 한도가 먼저 적용될 수 있습니다.</p>}{importMode==="legacy_v1"&&<p>이전 무수리의 musuri-vault v1 암호화 내보내기 파일만 지원합니다. 당시 비밀번호 또는 MR1 복구 키가 필요하며, 옛 형식의 한도인 최대 256개·26MiB까지 파일 전체를 검증합니다. 이전한 항목을 포함해 이 볼트에는 최대 {MAX_ITEMS.toLocaleString("ko-KR")}개까지 보관할 수 있습니다. 파일에 담긴 범위만 이전하며 원래 서버의 최신·전체 자료임을 보장하지 않습니다.</p>}{!archiveChecked&&<><label>암호화 볼트 파일<input type="file" accept=".json,application/json" disabled={frozen||importing} onChange={e=>{setArchiveFile(e.target.files?.[0]??null);e.target.value=""}}/></label><label>{archiveRecovery?"파일의 복구 키":"파일의 볼트 비밀번호"}<input type="password" autoComplete="off" value={archiveSecret} disabled={frozen} maxLength={1024} onChange={e=>setArchiveSecret(e.target.value)}/></label><label className="check"><input type="checkbox" checked={archiveRecovery} disabled={frozen} onChange={e=>{setArchiveRecovery(e.target.checked);setArchiveSecret("")}}/>복구 키 사용</label><button disabled={frozen||!archiveFile||!archiveSecret} onClick={()=>void perform(async()=>{try{const result=await run<Preview>({op:importMode==="legacy_v1"?"legacy_v1_file":"archive_file",file:archiveFile,secret:archiveSecret,recovery:archiveRecovery});adopt(result,importMode==="legacy_v1"?"구 v1 파일에 담긴 항목 전체":"암호화 파일의 항목 전체");setArchiveChecked(true)}finally{setArchiveFile(null);setArchiveSecret("")}})}>파일 전체 확인</button></>}</>:<><label>가져올 항목<select value={filterMode} disabled={frozen||importing||previewLocked} onChange={e=>setFilterMode(e.target.value as FilterMode)}><option value="all">파일의 모든 항목</option><option value="any">사용자 이름에 포함된 글자로 선택 (여러 개, 대소문자 무시)</option><option value="exact">사용자 이름이 정확히 같은 항목만</option></select></label>{filterMode==="exact"&&<label>사용자 이름<input value={username} disabled={frozen||importing||previewLocked} onChange={e=>setUsername(e.target.value)}/></label>}{filterMode==="any"&&<label>포함할 글자 (쉼표 또는 줄바꿈으로 구분, 최대 {MAX_TERMS}개, 각 2글자 이상)<textarea rows={3} value={terms} disabled={frozen||importing||previewLocked} onChange={e=>setTerms(e.target.value)} autoComplete="off" spellCheck={false}/></label>}{!picked.ok&&<p className="hint" role="status">{picked.message}</p>}<p className="hint">{previewLocked?"미리보기가 만들어진 뒤에는 조건을 바꿀 수 없습니다. 조건을 바꾸려면 아래 '조건 바꾸기'로 미리보기를 지운 뒤 파일을 다시 선택해 주세요.":"조건을 정한 뒤 파일을 선택해 주세요."} 조건에 맞지 않는 줄은 내용을 읽거나 검사하지 않고 건너뜁니다. 이 볼트는 항목을 최대 {MAX_ITEMS.toLocaleString("ko-KR")}개까지 보관하며, 항목이 매우 크면 전체 저장 용량 한도가 먼저 적용될 수 있습니다.</p>{previewLocked&&!importing&&<button type="button" disabled={frozen} onClick={()=>void resetPreview()}>조건 바꾸기 (미리보기 지우기)</button>}<label className="file">Apple 암호 CSV 선택<input type="file" accept=".csv,text/csv" disabled={frozen||importing||previewLocked||!picked.ok} onChange={e=>{const file=e.target.files?.[0];e.target.value="";if(file&&picked.ok&&!previewLocked){const chosen=picked.value,label=scopeLabel(filterMode,username,terms);void perform(async()=>{await run({op:"csv_cancel"});setPreview(emptyPreview);setRows([]);setScope("");const result=await run<Preview>({op:"csv_file",file,username:chosen.username,usernames:chosen.usernames});adopt(result,label)})}}}/></label></>}{(preview.rows.length>0||preview.filtered>0||archiveChecked)&&<>{scope&&<p>가져오기 범위: {scope}</p>}<p>선택한 항목 {rows.length}개 · 선택할 수 있는 항목 {preview.eligible}개 · 이미 있는 중복 {preview.duplicates}개{preview.filtered>0?` · 조건에 맞지 않아 제외 ${preview.filtered}개`:""}{importing?` · 저장 완료 ${imported}개 · 남음 ${Math.max(0,rows.length-imported)}개`:""}</p><p role="status">저장 후 보관함 {sel.total.toLocaleString("ko-KR")} / {sel.limit.toLocaleString("ko-KR")}개 (현재 {existingCount.toLocaleString("ko-KR")}개, 남은 자리 {capacity.toLocaleString("ko-KR")}개{preview.bytes_limit?` · 저장 용량 ${formatMiB(preview.bytes_used??0)} / ${formatMiB(preview.bytes_limit)} 사용 중`:""})</p><p className="hint">항목은 하나씩 저장됩니다. 중간에 끊기거나 실패하면 일부만 저장될 수 있으며, 저장이 확인된 개수를 알려 드립니다. 저장된 항목은 유지되고, 이어서 저장하거나 같은 파일을 다시 가져오면 이미 저장된 항목은 중복으로 건너뜁니다.</p>{!importing&&sel.over&&<p className="notice" role="alert">선택한 항목이 남은 자리보다 {rows.length-capacity}개 많습니다. {rows.length-capacity}개 이상 선택을 해제해 주세요. 저장을 시작하기 전에 확인되는 한도입니다.</p>}{!importing&&sel.empty&&<p className="hint">저장할 항목이 선택되지 않았습니다.</p>}{selectableRows.length>0&&<div className="actions"><button type="button" disabled={frozen||importing} onClick={()=>setRows(selectableRows.slice(0,Math.max(0,capacity)))}>남은 자리까지 선택</button><button type="button" disabled={frozen||importing||rows.length===0} onClick={()=>setRows([])}>모두 해제</button></div>}<div className="import-rows">{preview.rows.filter(r=>r.status==="new"||r.status==="changed").map(r=><label className="check" key={r.row}><input type="checkbox" disabled={frozen||importing} checked={chosenRows.has(r.row)} onChange={e=>setRows(v=>e.target.checked?[...v,r.row]:v.filter(n=>n!==r.row))}/>{r.row}행 · {r.status==="changed"?"같은 계정의 다른 내용":"새 항목"}{r.blank_password?" · 비밀번호 없음":""}{r.unsupported_url?" · 주소 확인 필요":""}{r.invalid_otp?" · 인증 코드 설정 확인 필요":""}</label>)}</div><button className="primary" disabled={frozen||!rows.length||(!importing&&sel.over)} onClick={()=>void importSelected()}>{importing?"남은 항목 계속 저장":`선택한 ${rows.length}개 항목 저장`}</button></>}{busy&&importMode!=="csv"&&<button onClick={()=>lock()}>잠그고 가져오기 중단</button>}{busy&&importing&&<button onClick={()=>{importCancel.current=true}}>가져오기 중단</button>}<button disabled={busy||uncertain} onClick={()=>void closeImport()}>가져오기 닫기</button></section>}
    {!isWorkspace && connected && recovery === null && <details className="gate-security">
      <summary>볼트 비밀번호와 복구 키</summary>
      <p>볼트 비밀번호는 무수리 로그인과 별개입니다. 관리자도 내용을 열 수 없습니다. 신뢰하는 기기에서 복구 키를 따로 보관하세요.</p>
    </details>}
    {!isWorkspace && connected && <footer className="gate-links">
      {assistant && <a href={`${assistant}/app`} onClick={()=>lock()}>무수리로 돌아가기</a>}
      {recovery !== null && <button type="button" onClick={()=>lock()}>잠그기</button>}
      <button type="button" disabled={disconnecting.current} onClick={()=>void disconnect()}>연결 끊기</button>
    </footer>}
    </main>
  </div>

}
createRoot(document.getElementById("root")!).render(<StrictMode><App/></StrictMode>);
