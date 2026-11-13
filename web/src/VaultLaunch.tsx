import {useEffect,useState} from "react";
import {useScope} from "./account-scope";
const windows=new Set<{window:Window;origin:string;account:string}>();
export function lockVaultWindows(account?:string|null){
  for(const value of windows){if(account&&value.account!==account)continue;try{value.window.postMessage({kind:"moosuri-vault-lock",account:value.account},value.origin)}catch{}windows.delete(value)}
}
export function VaultLaunch({accountId}:{accountId:string}){
  const scope=useScope(accountId),[available,setAvailable]=useState(false),[busy,setBusy]=useState(false),[error,setError]=useState("");
  useEffect(()=>{let live=true;void scope.request<{available:boolean}>("/vault").then(v=>{if(live)setAvailable(v.available===true)}).catch(()=>{});return()=>{live=false}},[scope]);
  if(!available)return null;
  async function open(){
    if(busy)return;setBusy(true);setError("");
    const target=`moosuri-vault-${crypto.randomUUID()}`;const popup=window.open("about:blank",target);
    if(!popup){setError("새 창을 허용한 뒤 다시 열어 주세요.");setBusy(false);return}
    try{
      const handoff=await scope.request<{token:string;vault_origin:string}>("/vault/handoff","POST",{});
      const origin=new URL(handoff.vault_origin).origin;
      if(handoff.token.length!==64||origin!==handoff.vault_origin)throw new Error("vault_unavailable");
      for(const w of windows)if(w.window.closed)windows.delete(w);
      if(windows.size>=16)throw new Error("vault_window_limit");windows.add({window:popup,origin,account:accountId});
      const form=document.createElement("form");form.method="POST";form.action=`${origin}/session`;form.target=target;
      const input=document.createElement("input");input.type="hidden";input.name="handoff";input.value=handoff.token;form.append(input);document.body.append(form);form.submit();form.remove();
    }catch{popup.close();setError("볼트 연결을 확인하지 못했습니다. 다시 열어 주세요.")}
    finally{setBusy(false)}
  }
  return <span><button className="text-button" disabled={busy} onClick={()=>void open()}>볼트 열기</button>{error&&<small role="status">{error}</small>}</span>;
}
