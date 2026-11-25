import {useEffect,useState} from "react";
import {useScope} from "./account-scope";
const windows=new Set<{window:Window;origin:string;account:string;googleEmail?:string}>();
// A popup may request only its own non-secret account hint. A normal vault launch
// has no hint and a different window or origin cannot consume it.
window.addEventListener("message", event => {
  if (event.data?.kind !== "moosuri-vault-google-context") return;
  for (const entry of windows) {
    if (entry.googleEmail && entry.window === event.source && event.origin === entry.origin &&
        event.data.account === entry.account && !entry.window.closed) {
      entry.window.postMessage({ kind: "moosuri-vault-google-account", account: entry.account, email: entry.googleEmail }, entry.origin);
      break;
    }
  }
});
export function lockVaultWindows(account?:string|null){
  for(const value of windows){if(account&&value.account!==account)continue;try{value.window.postMessage({kind:"moosuri-vault-lock",account:value.account},value.origin)}catch{}windows.delete(value)}
}
/** Exact serialized origin only, distinct from this app's origin and host; https (http only for loopback dev). */
export function trustedVaultOrigin(raw:string,current:Location=window.location):string{
  const u=new URL(raw);
  const local=u.hostname==="localhost"||u.hostname==="127.0.0.1";
  if(u.origin!==raw||u.origin===current.origin||u.hostname===current.hostname||!(u.protocol==="https:"||(local&&u.protocol==="http:")))throw new Error("vault_unavailable");
  return u.origin;
}
/** Shared by launch and tests: registers the popup for logout lock, then posts the one-time handoff. */
export function postVaultForm(popup:Window,target:string,handoff:{token:string;vault_origin:string},accountId:string,googleEmail?:string){
  const origin=trustedVaultOrigin(handoff.vault_origin);
  if(!/^[0-9a-f]{64}$/.test(handoff.token))throw new Error("vault_unavailable");
  for(const w of windows)if(w.window.closed)windows.delete(w);
  if(windows.size>=16)throw new Error("vault_window_limit");
  windows.add({window:popup,origin,account:accountId,googleEmail});
  const form=document.createElement("form");form.method="POST";form.action=`${origin}/session`;form.target=target;form.acceptCharset="UTF-8";
  const input=document.createElement("input");input.type="hidden";input.name="handoff";input.value=handoff.token;form.append(input);
  document.body.append(form);
  try{form.submit()}finally{form.remove()}
}
export function VaultLaunch({accountId,label="볼트 열기",unavailable}:{accountId:string;label?:string;unavailable?:string}){
  const scope=useScope(accountId),[available,setAvailable]=useState<boolean|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState("");
  useEffect(()=>{let live=true;setAvailable(null);void scope.request<{available:boolean}>("/vault").then(v=>{if(live)setAvailable(v.available===true)}).catch(()=>{if(live)setAvailable(false)});return()=>{live=false}},[scope]);
  if(available===null)return unavailable?<p role="status">내 볼트 연결 확인 중…</p>:null;
  if(!available)return unavailable?<p role="status">{unavailable}</p>:null;
  async function open(){
    if(busy)return;setBusy(true);setError("");
    const target=`moosuri-vault-${crypto.randomUUID()}`;const popup=window.open("about:blank",target);
    if(!popup){setError("새 창을 허용한 뒤 다시 열어 주세요.");setBusy(false);return}
    try{
      const handoff=await scope.request<{token:string;vault_origin:string}>("/vault/handoff","POST",{});
      postVaultForm(popup,target,handoff,accountId);
    }catch{popup.close();setError("볼트 연결을 확인하지 못했습니다. 다시 열어 주세요.")}
    finally{setBusy(false)}
  }
  return <span><button className="text-button" disabled={busy} onClick={()=>void open()}>{label}</button>{error&&<small role="status">{error}</small>}</span>;
}
