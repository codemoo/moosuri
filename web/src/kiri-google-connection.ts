import { postVaultForm } from "./VaultLaunch";
import type { GoogleReadIntent } from "./PersonalGoogle";
import { ApiFailure, type RequestOptions } from "./api";
import { clearGooglePending, writeGooglePending, type GoogleKiriPending } from "./kiri-google-pending";

type Receipt = {job:string; challenge:string; state:string; flow:string|null; handle:string|null; email:string|null};
type Request = <T>(path:string, method?:string, body?:unknown, signal?:AbortSignal, options?:RequestOptions)=>Promise<T>;
const base = "/personal/google/kiri";
// Timeouts and proxy/server errors cannot prove whether Deliver was accepted.
const definiteRejection = (error:unknown) => error instanceof ApiFailure
  && error.status >= 400 && error.status < 500 && ![408,429].includes(error.status);
const waiting: Record<string,string> = {
  preparing:"Google 연결을 준비하고 있습니다.",
  validating:"볼트에서 선택한 Gmail 계정을 Kiri가 확인하고 있습니다.",
  opening:"볼트에서 선택한 계정으로 Kiri를 여는 중입니다.",
  vault:"볼트에서 Gmail 계정을 먼저 확인하고 있습니다.",
  verifying:"Google의 추가 본인 확인을 기다리고 있습니다. 휴대전화에 Google 승인 요청이 왔다면 확인해 주세요.",
  signing_in:"Kiri에서 Google에 로그인하고 있습니다.",
  signed_in:"Google 로그인을 확인했습니다. 서비스 연결을 준비하고 있습니다.",
  setup:"이 설치의 Google 연결을 처음 준비하고 있습니다. 몇 분 정도 걸릴 수 있습니다.",
  consent:"Kiri에서 Google 서비스 권한을 연결하고 있습니다.",
};
const ended: Record<string,string> = {
  blocked:"Google이 Kiri의 로그인 시도를 차단했습니다.",
  needs_human:"Google이 추가 본인 확인을 요청해 연결을 마치지 못했습니다.",
  setup_required:"Kiri에서 Google 로그인을 확인했습니다. 이 설치에 Google Workspace 앱이 등록되면 서비스 권한을 연결할 수 있습니다.",
  setup_pending:"Google 연결 설정을 저장했습니다. 내 계정에 적용되는 중이니 잠시 후 다시 연결해 주세요.",
  setup_recovery_required:"이전 Google 설정의 결과를 확인해야 해서 중단했습니다. 중복으로 만들지 않고 기존 설정을 확인해야 합니다.",
  setup_cleanup_pending:"이전 Google 설정의 임시 권한 정리를 확인하지 못했습니다. 정리가 확인된 뒤 다시 연결할 수 있습니다.",
  setup_busy:"다른 Google 설정이 진행 중이거나 이전 작업을 정리하고 있습니다. 잠시 후 다시 연결해 주세요.",
  denied:"Google 서비스 연결이 취소됐습니다.",
  unavailable:"Kiri에서 연결 결과를 확인할 수 없습니다.",
};
const gone = (error:unknown) => error instanceof ApiFailure && (error.status===404 || error.status===410);

/** Explicitly cancel saved work on the server; the record is removed only after confirmed cancel/gone. */
export async function cancelGooglePending(request:Request, record:GoogleKiriPending):Promise<"cleared"|"kept"|"expired"> {
  try {
    await request(base,"POST",{op:"cancel",job:record.job},undefined,{timeoutMs:15000});
    clearGooglePending(record);return "cleared";
  } catch(error) {
    if (gone(error)) {clearGooglePending(record);return "cleared";}
    return error instanceof ApiFailure && error.status===401 ? "expired" : "kept";
  }
}

export type KiriOptions = {
  /** SessionView.chat_draft_scope; without it nothing is persisted. */
  scope?:string;
  /** Saved work to resume: only Status for this exact job is ever called, never Start. */
  resume?:GoogleKiriPending;
  persisted?:(durable:boolean)=>void;
  cancelled?:(confirmed:boolean)=>void;
};

export async function connectInKiri(request:Request, account:string, email:string, service:string, vaultOrigin:string,
  read:GoogleReadIntent|undefined, signal:AbortSignal, status:(message:string)=>void, options:KiriOptions={}) {
  const resume = options.resume;
  if (resume && (resume.account!==account || resume.service!==service || (!options.scope || resume.scope!==options.scope))) throw Error(ended.unavailable);
  const job = resume ? resume.job : crypto.randomUUID(), target = "moosuri-vault-" + job;
  const scope = resume ? resume.scope : options.scope;
  const identity = scope ? {account,scope,service,job} : undefined;
  const clear = ()=>{ if (identity) clearGooglePending(identity); };
  // Reserve the window during the explicit click. An async status/handoff must
  // not lose browser user activation. Already-delivered work closes it immediately.
  let popup: Window|null = window.open("about:blank", target);
  if (!resume) {
    if (!popup) throw Error("볼트를 열 수 없습니다. 새 창을 허용한 뒤 다시 연결하세요.");
    const now = Date.now();
    // Persist before Start so a lost Start response is recoverable after reload.
    const durable = identity ? writeGooglePending({...identity,created:now,updated:now,...(read?{read}:{})}) : false;
    options.persisted?.(durable);
  } else options.persisted?.(true);
  let delivering = false, delivered = false, deliveryUncertain = false, handed = false, finished = false, preserve = false;
  let retryEnvelope: {envelope:unknown;google_email:string}|undefined;
  let deliveryRejected: unknown, rejectionChecked = false, endedRun = false;
  const call = (op:string, extra:Record<string,unknown>={})=>request<Receipt>(base,"POST",{op,job,...extra},signal,{timeoutMs:15000});
  const close = ()=>popup?.close();
  signal.addEventListener("abort",close,{once:true});
  try {
    // Start prepares the one-use recipient. Chrome starts only after the vault
    // selected/stored the credential and the server reauthorized delivery.
    status(resume ? "저장된 Google 연결 상태를 확인하고 있습니다." : waiting.vault);
    let current: Receipt;
    try {
      current = resume ? await call("status") : await call("start",{service,read});
    } catch(error) {
      if (resume) {
        if (gone(error)) {finished=true;clear();throw Error("이전 Google 연결 작업이 만료되었거나 더 이상 없습니다. 새로 연결해 주세요.");}
        preserve=!signal.aborted;throw error;
      }
      // Only a definite refusal proves Start was not accepted. Preserve an
      // uncertain receipt for status reconciliation, unless the user cancelled.
      if (definiteRejection(error) && !(error instanceof ApiFailure && error.status===401)) {finished=true;clear();}
      else if (!signal.aborted) preserve=true;
      throw error;
    }
    let deadline = Date.now()+(resume&&current.state!=="vault"&&current.state!=="preparing"?1205000:725000);
    while (!signal.aborted && Date.now()<deadline) {
      if (current.job!==job) {
        preserve=true;
        throw Error(ended.unavailable);
      }
      if (current.state==="complete" && /^[0-9a-f]{64}$/.test(current.handle??"") && /^[0-9a-f]{64}$/.test(current.flow??"")) {
        if (!current.email || !/^[^@\s]+@[^@\s]+$/u.test(current.email)) throw Error(ended.unavailable);
        // The pending record stays until the caller's /complete is acknowledged.
        finished=true;return {job,handle:current.handle!,flow:current.flow!,email:current.email};
      }
      if(deliveryRejected) {
        // A conflicting handoff may only mean a late packet was accepted. Trust a
        // status read taken after the rejection before cancelling anything.
        if(!["preparing","vault"].includes(current.state)) deliveryRejected=undefined;
        else if(rejectionChecked) throw deliveryRejected;
      }
      if (!Object.hasOwn(waiting,current.state)) {
        finished=true;clear();throw Error(ended[current.state]??ended.unavailable);
      }
      status(waiting[current.state]);
      // If Deliver was accepted but its HTTP response was lost, the next
      // canonical status proves that Kiri already owns the sealed handoff.
      // Continue polling instead of treating the popup's close as cancellation.
      if (!["preparing","vault"].includes(current.state) && !delivered) {
        delivered=true;deadline=Date.now()+1205000;close();
      }
      if (popup && !delivering && !delivered && !deliveryUncertain && popup.closed) {
        if (resume) {preserve=true;throw Error("볼트 창을 닫았습니다. 저장된 연결에서 '이어서 진행'을 눌러 다시 열 수 있습니다.");}
        throw Error("볼트를 닫아 Google 연결을 취소했습니다.");
      }
      if (current.state==="vault" && !handed) {
        if (!popup) {
          popup = window.open("about:blank", target);
          if (!popup) {preserve=true;throw Error("볼트를 열 수 없습니다. 새 창을 허용한 뒤 '이어서 진행'을 다시 누르세요.");}
        }
        const handoff=await request<{token:string;vault_origin:string}>("/personal/vault/handoff","POST",{},signal);
        if (handoff.vault_origin!==vaultOrigin || signal.aborted) throw Error(ended.unavailable);
        postVaultForm(popup!,target,handoff,account,email,{job:current.challenge,receive:async (envelope,google_email)=>{
          if (endedRun || signal.aborted || delivering || delivered || deliveryUncertain) return;
          retryEnvelope={envelope,google_email};
          delivering=true;
          try {
            await call("deliver",{envelope,google_email});delivered=true;deliveryUncertain=false;deadline=Date.now()+1205000;close();
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
      const preRejection=deliveryRejected;
      try { current=await call("status"); if(preRejection && preRejection===deliveryRejected) rejectionChecked=true; }
      catch(error) {
        // A transient status failure cannot disprove accepted delivery/setup.
        // Keep polling the same job within its existing deadline. Never resend
        // Start or create another provider operation to recover a read error.
        if ((reconcilingDelivery || delivered) && !signal.aborted && !definiteRejection(error)) continue;
        // Resumed work survives a transient read failure; an explicit retry polls again.
        if (!signal.aborted && !definiteRejection(error)) preserve=true;
        throw error;
      }
      if (reconcilingDelivery) {
        deliveryUncertain=false;
        if (current.state!=="vault") {delivered=true;deadline=Date.now()+1205000;close();}
        else if(retryEnvelope) {
          // Status proves no delivery was accepted. Replay the exact sealed
          // packet; never ask the vault to generate a conflicting envelope.
          delivering=true;
          try {
            await call("deliver",retryEnvelope);delivered=true;deadline=Date.now()+1205000;close();
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
    endedRun=true;close();signal.removeEventListener("abort",close);
    if (!finished && !preserve) {
      // Forget the saved job only after the server confirms the cancel (or that
      // the job is gone); otherwise it stays resumable/cancellable after reload.
      const confirmed=await request(base,"POST",{op:"cancel",job},undefined,{timeoutMs:15000})
        .then(()=>true,error=>gone(error));
      if(confirmed)clear();
      options.cancelled?.(confirmed);
    }
  }
}
