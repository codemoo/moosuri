/// <reference lib="webworker" />
import init,{VaultEngine} from "./wasm/vault_crypto.js";
import type {Command,Operation,Owner,Receipt,State,VaultHead,CurrentSession} from "./types";
const scope=self as unknown as DedicatedWorkerGlobalScope;
let engine:VaultEngine|undefined,owner:Owner|undefined,closed=false,published=false;
let queue=Promise.resolve(),loaded:Promise<unknown>|undefined,admittedHead:VaultHead|undefined,pollQueued=false;
const controllers=new Set<AbortController>();
function fixed(code:string):Error{return new Error(code)}
function local<T>(command:Command):T{
  if(!engine||closed)throw fixed("vault_locked");
  const result=JSON.parse(engine.rpc(JSON.stringify(command))) as {ok:boolean;value:T;error:string};
  if(!result.ok)throw fixed(result.error);return result.value;
}
async function api<T>(path:string,method="GET",body?:unknown):Promise<T>{
  if(closed)throw fixed("vault_locked");
  const controller=new AbortController();controllers.add(controller);
  try{
    const response=await fetch(path,{method,credentials:"same-origin",cache:"no-store",signal:controller.signal,headers:{...(owner?{"X-Moosuri-Vault-Account":owner.account}:{}),...(body===undefined?{}:{"Content-Type":"application/json"})},body:body===undefined?undefined:JSON.stringify(body)});
    if(response.status===401||response.status===403){lock("vault_unauthorized");throw fixed("vault_unauthorized")}
    if(response.status===404)throw fixed("vault_not_found");
    if(!response.ok)throw fixed(response.status===409?"vault_conflict":response.status===413?"vault_limit":"vault_unavailable");
    const reader=response.body?.getReader(),chunks:Uint8Array[]=[];let length=0;
    if(reader){for(;;){const {done,value}=await reader.read();if(done)break;length+=value.byteLength;if(length>32*1024*1024){await reader.cancel();throw fixed("vault_limit")}chunks.push(value)}}
    if(closed)throw fixed("vault_locked");const bytes=new Uint8Array(length);let offset=0;for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.byteLength}
    const text=new TextDecoder("utf-8",{fatal:true}).decode(bytes);return text?JSON.parse(text) as T:undefined as T;
  }catch(e){if(e instanceof Error&&e.message.startsWith("vault_"))throw e;throw fixed("vault_unavailable")}
  finally{controllers.delete(controller)}
}
function headOfState(value:State):VaultHead{
  const header=value.header as {revision:number;envelope_epoch:number;generation:number}|null;
  return header?{header_sha256:value.header_sha256,revision:header.revision,envelope_epoch:header.envelope_epoch,generation:header.generation}:{header_sha256:"",revision:0,envelope_epoch:0,generation:0};
}
function validHead(head:VaultHead):boolean{
  if(!head||typeof head.header_sha256!=="string")return false;
  const fields=[head.revision,head.envelope_epoch,head.generation];
  return head.header_sha256===""?fields.every(v=>v===0):/^[0-9a-f]{64}$/.test(head.header_sha256)&&fields.every(v=>Number.isSafeInteger(v)&&v>0);
}
function sameHead(a:VaultHead,b:VaultHead){return (["header_sha256","revision","envelope_epoch","generation"] as const).every(k=>a[k]===b[k])}
async function currentSession(){
  const current=await api<CurrentSession>("/session/current");
  if(!owner||!current.owner||(["account","tenant","installation","epoch"] as const).some(key=>current.owner[key]!==owner![key])){lock("vault_unauthorized");throw fixed("vault_unauthorized")}
  if(!validHead(current.head)){lock("vault_unavailable");throw fixed("vault_unavailable")}
  return current;
}
function changed():never{lock("vault_generation_changed");throw fixed("vault_generation_changed")}
async function checkHead(allowOwnSettlement=false){
  const current=await currentSession();
  if(!admittedHead||sameHead(current.head,admittedHead))return;
  // A lost own key-commit ACK must remain reconcilable. Only Rust can compare
  // the exact canonical receipt to the locally owned candidate; no activation.
  if(allowOwnSettlement&&published){
    const operation=local<Operation>({op:"pending"});
    if(operation.kind==="key"){
      try{
        const record=await api<{receipt:Receipt}>(`/api/operations/${encodeURIComponent(operation.id)}`);
        local({op:"check_pending_head",receipt:record.receipt,head:current.head});return;
      }catch(e){if(closed)throw e;if(e instanceof Error&&e.message==="vault_unavailable")throw e}
    }
  }
  changed();
}
function lock(reason:string){
  if(closed)return;closed=true;for(const c of controllers)c.abort();controllers.clear();engine?.free();engine=undefined;scope.postMessage({kind:"locked",reason});
}
async function state(){const value=await api<State>("/api/state");if(!owner||value.owner.account!==owner.account||value.owner.epoch!==owner.epoch||value.owner.installation!==owner.installation)throw fixed("vault_unauthorized");return value}
async function load(){const value=await state();const head=headOfState(value);if(admittedHead&&!sameHead(head,admittedHead))changed();local({op:"load",state:value});admittedHead=head;return {initialized:value.header!==null,pending:value.pending}}
async function adopt(receipt:Receipt){
  if(receipt.state!=="committed"){
    if(receipt.state==="aborted"||receipt.state==="conflict"){local({op:"abort",receipt});published=false;await load();throw fixed("vault_conflict")}
    throw fixed("vault_unavailable");
  }
  const current=await state();
  const outcome=local<{refresh_unlock?:boolean}>({op:"acknowledge",receipt,state:current});published=false;
  if(outcome.refresh_unlock){lock("vault_saved_refresh_unlock");return {saved:true}}
  admittedHead=headOfState(current);return {saved:true};
}
async function publish(){
  const op=local<Operation>({op:"pending"});published=true;
  const prepared=await api<Receipt>("/api/operations","POST",op);
  return adopt(prepared.state==="pending"?await api<Receipt>(`/api/operations/${encodeURIComponent(op.id)}/commit`,"POST",{}):prepared);
}
async function reconcile(){
  const op=local<Operation>({op:"pending"});
  try{const r=await api<{operation:Operation;receipt:Receipt}>(`/api/operations/${encodeURIComponent(op.id)}`);return adopt(r.receipt.state==="pending"?await api<Receipt>(`/api/operations/${encodeURIComponent(op.id)}/commit`,"POST",{}):r.receipt)}
  catch(e){if(e instanceof Error&&e.message==="vault_not_found")return publish();throw e}
}
async function cancel(){
  if(!published){local({op:"abort",receipt:null});return {saved:false}}
  const op=local<Operation>({op:"pending"});
  let receipt:Receipt;
  try{receipt=await api<Receipt>(`/api/operations/${encodeURIComponent(op.id)}/cancel`,"POST",op)}
  catch(e){throw e}
  if(receipt.state==="committed")return adopt(receipt);
  local({op:"abort",receipt});published=false;await load();return {saved:false};
}
async function dispatch(command:Command):Promise<unknown>{
  if(command.op==="initialize"){
    if(engine||owner)throw fixed("vault_state");
    owner=command.owner as Owner;loaded??=init();await loaded;if(closed)throw fixed("vault_locked");
    await currentSession();engine=new VaultEngine(JSON.stringify(owner));return load();
  }
  if(command.op==="close"){lock("vault_locked");return null}
  await checkHead(["publish","reconcile","cancel"].includes(command.op));
  if(command.op==="publish")return publish();
  if(command.op==="reconcile")return reconcile();
  if(command.op==="cancel")return cancel();
  if(command.op==="refresh")return load();
  if(command.op==="discard_old"){
    const id=command.id;if(typeof id!=="string")throw fixed("vault_invalid");
    const record=await api<{operation:Operation}>(`/api/operations/${encodeURIComponent(id)}`);
    const receipt=await api<Receipt>(`/api/operations/${encodeURIComponent(id)}/cancel`,"POST",record.operation);
    if(receipt.state==="committed"){lock("vault_refresh_unlock");return null}return load();
  }
  if(command.op==="archive_file"){
    const file=command.file;if(!(file instanceof File)||file.size>26*1024*1024)throw fixed("vault_limit");
    const archive=await file.text();if(closed)throw fixed("vault_locked");await checkHead();
    return local({op:"archive_preview",archive,secret:command.secret,recovery:command.recovery===true});
  }
  if(command.op==="archive_export"){
    // Pin a fresh coherent source snapshot. Never silently export a mixed or
    // changing inventory; a mutation after the final check is a later snapshot.
    const before=await state();local({op:"load",state:before});
    const encrypted=local<string>({op:"archive_export"});
    const after=await state();
    if(before.header_sha256!==after.header_sha256||before.inventory_sha256!==after.inventory_sha256)throw fixed("vault_conflict");
    return encrypted;
  }
  if(command.op==="csv_file"){
    const file=command.file;if(!(file instanceof File)||file.size>10*1024*1024)throw fixed("vault_limit");
    const csv=await file.text();if(closed)throw fixed("vault_locked");await checkHead();return local({op:"csv_preview",csv,username:command.username??null});
  }
  if(command.op==="totp")return local({...command,now:Math.floor(Date.now()/1000)});
  if(command.op==="check_pending_head"||command.op==="acknowledge"||command.op==="load"||command.op==="abort"||command.op==="pending")throw fixed("vault_unsupported");
  return local(command);
}
scope.onmessage=(event:MessageEvent<{id:number;epoch:number;command:Command}>)=>{
  const {id,epoch,command}=event.data;
  if(!Number.isSafeInteger(id)||!Number.isSafeInteger(epoch)||!command||typeof command.op!=="string")return;
  queue=queue.then(async()=>{
    if(closed)return;
    try{const value=await dispatch(command);if(!closed){await checkHead();if(!closed)scope.postMessage({id,epoch,ok:true,value})}}
    catch(e){if(!closed)scope.postMessage({id,epoch,ok:false,error:e instanceof Error&&e.message.startsWith("vault_")?e.message:"vault_unavailable",uncertain:published})}
  });
};
// Serialize polls with admission/own-commit adoption: an older response must not
// race and retire a newly acknowledged own head or release a delayed secret.
setInterval(()=>{
  if(!engine||closed||pollQueued)return;pollQueued=true;
  queue=queue.then(async()=>{try{if(!closed)await checkHead(true)}catch{if(!closed)lock("vault_unavailable")}finally{pollQueued=false}});
},5000);
