import { postVaultForm } from "./VaultLaunch";
import type { GoogleReadIntent } from "./PersonalGoogle";
import { ApiFailure, type RequestOptions } from "./api";

type Receipt = {job:string; challenge:string; state:string; flow:string|null; handle:string|null; email:string|null};
type Request = <T>(path:string, method?:string, body?:unknown, signal?:AbortSignal, options?:RequestOptions)=>Promise<T>;
const base = "/personal/google/kiri";
// Timeouts and proxy/server errors cannot prove whether Deliver was accepted.
const definiteRejection = (error:unknown) => error instanceof ApiFailure
  && error.status >= 400 && error.status < 500 && ![408,429].includes(error.status);
const waiting: Record<string,string> = {
  validating:"볼트에서 선택한 Gmail 계정을 Kiri가 확인하고 있습니다.",
  opening:"볼트에서 선택한 계정으로 Kiri를 여는 중입니다.",
  vault:"볼트에서 Gmail 계정을 먼저 확인하고 있습니다.",
  verifying:"Google의 추가 본인 확인을 기다리고 있습니다. 휴대전화에 Google 승인 요청이 왔다면 확인해 주세요.",
  signing_in:"Kiri에서 Google에 로그인하고 있습니다.",
  signed_in:"Google 로그인을 확인했습니다. 서비스 연결을 준비하고 있습니다.",
  consent:"Kiri에서 Google 서비스 권한을 연결하고 있습니다.",
};
const ended: Record<string,string> = {
  blocked:"Google이 Kiri의 로그인 시도를 차단했습니다.",
  needs_human:"Google이 추가 본인 확인을 요청해 연결을 마치지 못했습니다.",
  setup_required:"Kiri에서 Google 로그인을 확인했습니다. 이 설치에 Google Workspace 앱이 등록되면 서비스 권한을 연결할 수 있습니다.",
  denied:"Google 서비스 연결이 취소됐습니다.",
  unavailable:"Kiri에서 연결 결과를 확인할 수 없습니다.",
};
export async function connectInKiri(request:Request, account:string, email:string, service:string, vaultOrigin:string,
  read:GoogleReadIntent|undefined, signal:AbortSignal, status:(message:string)=>void) {
  const job = crypto.randomUUID(), target = "moosuri-vault-" + job;
  const popup = window.open("about:blank", target);
  if (!popup) throw Error("볼트를 열 수 없습니다. 새 창을 허용한 뒤 다시 연결하세요.");
  let delivering = false, delivered = false, deliveryUncertain = false, handed = false, finished = false;
  let retryEnvelope: {envelope:unknown;google_email:string}|undefined;
  let deliveryRejected: unknown;
  const call = (op:string, extra:Record<string,unknown>={})=>request<Receipt>(base,"POST",{op,job,...extra},signal,{timeoutMs:15000});
  const close = ()=>popup.close();
  signal.addEventListener("abort",close,{once:true});
  try {
    // Start only reserves a member-bound challenge. The server does not launch
    // Kiri until the vault has selected or stored a credential and delivers it.
    status(waiting.vault);
    let current = await call("start",{service,read});
    let deadline = Date.now()+725000;
    while (!signal.aborted && Date.now()<deadline) {
      if (current.job!==job) throw Error(ended.unavailable);
      if (current.state==="complete" && /^[0-9a-f]{64}$/.test(current.handle??"") && /^[0-9a-f]{64}$/.test(current.flow??"")) {
        if (!current.email || !/^[^@\s]+@[^@\s]+$/u.test(current.email)) throw Error(ended.unavailable);
        finished=true;return {handle:current.handle!,flow:current.flow!,email:current.email};
      }
      if(deliveryRejected) throw deliveryRejected;
      if (!Object.hasOwn(waiting,current.state)) {
        finished=true;throw Error(ended[current.state]??ended.unavailable);
      }
      status(waiting[current.state]);
      // If Deliver was accepted but its HTTP response was lost, the next
      // canonical status proves that Kiri already owns the sealed handoff.
      // Continue polling instead of treating the popup's close as cancellation.
      if (current.state!=="vault" && !delivered) {
        delivered=true;deadline=Date.now()+425000;close();
      }
      if (!delivering && !delivered && !deliveryUncertain && popup.closed) throw Error("볼트를 닫아 Google 연결을 취소했습니다.");
      if (current.state==="vault" && !handed) {
        const handoff=await request<{token:string;vault_origin:string}>("/personal/vault/handoff","POST",{},signal);
        if (handoff.vault_origin!==vaultOrigin || signal.aborted) throw Error(ended.unavailable);
        postVaultForm(popup,target,handoff,account,email,{job:current.challenge,receive:async (envelope,google_email)=>{
          if (signal.aborted || delivering || delivered || deliveryUncertain) return;
          retryEnvelope={envelope,google_email};
          delivering=true;
          try {
            await call("deliver",{envelope,google_email});delivered=true;deliveryUncertain=false;deadline=Date.now()+425000;close();
          } catch(error) {
            // Only a definite client rejection terminates the handoff. An
            // uncertain response is reconciled before closing or cancelling.
            deliveryUncertain=!definiteRejection(error);
            if(!deliveryUncertain) deliveryRejected=error;
            throw error;
          } finally { delivering=false; }
        }});
        handed=true;
      }
      await new Promise(resolve=>setTimeout(resolve,1300));
      if (signal.aborted) throw Error(ended.unavailable);
      const reconcilingDelivery=deliveryUncertain;
      try { current=await call("status"); }
      catch(error) {
        // Do not turn an accepted-but-unacknowledged Deliver into Cancel merely
        // because its first reconciliation read also failed. Retry within the
        // existing bounded job deadline.
        if (reconcilingDelivery && !signal.aborted && !definiteRejection(error)) continue;
        throw error;
      }
      if (reconcilingDelivery) {
        deliveryUncertain=false;
        if (current.state!=="vault") {delivered=true;deadline=Date.now()+425000;close();}
        else if(retryEnvelope) {
          // Status proves no delivery was accepted. Replay the exact sealed
          // packet; never ask the vault to generate a conflicting envelope.
          delivering=true;
          try {
            await call("deliver",retryEnvelope);delivered=true;deadline=Date.now()+425000;close();
          } catch(error) {
            deliveryUncertain=!definiteRejection(error);
            if(!deliveryUncertain) throw error;
          } finally {delivering=false;}
        }
      }
      if (["signing_in","signed_in","consent"].includes(current.state)) {delivered=true;close();}
    }
    throw Error(ended.unavailable);
  } finally {
    close();signal.removeEventListener("abort",close);
    if (!finished) void request(base,"POST",{op:"cancel",job}).catch(()=>{});
  }
}
