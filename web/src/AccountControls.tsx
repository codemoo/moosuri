import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { AccountManagementAction, AccountManagementInput, AccountManagementReceipt, ManagedAccount } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, errorText, isAborted } from "./api";
import "./accounts.css";

export function AccountControls({accountId, target, onExpired, onChanged, onPassword}: {
  accountId: string;
  target: ManagedAccount;
  onExpired: () => void;
  onChanged: () => void;
  onPassword: (password: string, receipt: AccountManagementReceipt) => void;
}) {
  const scope=useScope(accountId,target.user.id);
  const operation=useMemo(()=>({pending:false,closed:false}),[scope]);
  const [deleting,setDeleting]=useState<number|null>(null);
  const [busy,setBusy]=useState(false);
  const [pending,setPending]=useState<AccountManagementInput|null>(null);
  const [error,setError]=useState("");
  const [notice,setNotice]=useState("");
  const latest=useRef(target.revision);latest.current=target.revision;
  useLayoutEffect(()=>{
    operation.closed=false;setBusy(false);setPending(null);setError("");setNotice("");setDeleting(null);
    return ()=>{operation.closed=true;};
  },[operation]);
  async function submit(input: AccountManagementInput) {
    if(operation.pending||operation.closed)return;
    operation.pending=true;
    setBusy(true);setError("");setNotice("");setPending(input);
    try {
      const result=await scope.request<{receipt:AccountManagementReceipt;temporary_password:string|null}>(`/admin/users/${target.user.id}/manage`,"POST",input);
      if(operation.closed)return;
      if(result.receipt.request_id!==input.request_id || result.receipt.account_id!==target.user.id || result.receipt.revision!==input.expected_revision+1) {
        throw new Error("unexpected account receipt");
      }
      setPending(null);
      if(target.user.id===accountId){onExpired();return;}
      if(latest.current!==input.expected_revision && latest.current!==result.receipt.revision){
        setNotice("Account details changed while this action was running. The list has been refreshed.");
      } else if(input.action.kind==="reset_password") {
        if(result.temporary_password)onPassword(result.temporary_password,result.receipt);
        else setNotice("The password was already issued and cannot be shown again. Use Reset password to issue a new one if it was lost.");
      } else if(input.action.kind==="delete") setNotice("Account access has ended. Deletion delivery and cleanup status appear below after refresh.");
      else setNotice(input.action.kind==="resume" ? "Account resumed. Previous sessions and background work stay stopped." : "Saved. Existing sessions have ended and background work has stopped.");
      onChanged();
    } catch(e) {
      if(isAborted(e))return;
      if(e instanceof ApiFailure && e.status===401){onExpired();return;}
      if(e instanceof ApiFailure && [400,403,404,409].includes(e.status)) {
        setPending(null);onChanged();
        setError(e.status===409 ? "The account changed. Review the refreshed details before choosing an action." : errorText(e));
      } else setError("Could not confirm this action. Retry the same request to check its saved result.");
    } finally {operation.pending=false;if(!operation.closed)setBusy(false);}
  }
  function act(action:AccountManagementAction){
    void submit({request_id:crypto.randomUUID(),expected_revision:target.revision,action});
  }
  return <details className="account-controls">
    <summary>Manage {target.user.name}</summary>
    {target.deletion ? <div role="status">
      <p>Account permanently deactivated. Private content cleanup: {target.deletion.local_complete ? "complete" : "pending; cleanup retries on server restart"}.</p>
      <p>Separate vault deletion: {target.deletion.vault_acknowledged ? "acknowledged by the vault service" : "pending; the vault service must connect before delivery is confirmed"}.</p>
      {target.deletion.encrypted_history_retained && <p className="fine">Encrypted historical journals and backups can remain under the installation’s retention policy. Durable deletion fences prevent their reuse through this account. Already delivered external actions cannot be recalled.</p>}
      <button onClick={onChanged}>Refresh deletion status</button>
    </div> : target.recovery_required ? <p role="status">This account needs local operator recovery. Web actions are unavailable.</p> : <>
      <p>Changes sign out all devices and stop background work. Suspension, role changes and password resets retain saved content. Permanent deletion removes private content.</p>
      {target.vault_delivery && <p role="status">Vault revocation delivery: {target.vault_delivery}.</p>}
      <div className="account-action-buttons">
        {target.user.id!==accountId && <>
          <button disabled={busy||pending!==null} onClick={()=>act({kind:target.status==="suspended" ? "resume":"suspend"})}>{target.status==="suspended" ? "Resume account":"Suspend account"}</button>
          <button disabled={busy||pending!==null} onClick={()=>act({kind:"change_role",role:target.user.role==="admin" ? "member":"admin"})}>{target.user.role==="admin" ? "Make member":"Make administrator"}</button>
          <button disabled={busy||pending!==null} onClick={()=>act({kind:"reset_password"})}>Reset password</button>
          <button disabled={busy||pending!==null} onClick={()=>setDeleting(target.revision)}>Delete account</button>
        </>}
        <button disabled={busy||pending!==null} onClick={()=>act({kind:"end_sessions"})}>Sign out all devices</button>
      </div>
      {deleting!==null && <div role="group" aria-label="Permanent account deletion">
        <p>Permanently delete {target.user.name}’s private conversations, sources, files and saved credentials? Access ends immediately. Vault delivery can remain pending while its separate service is offline. Encrypted backups and journal history follow retention policy.</p>
        <button disabled={busy||pending!==null} onClick={()=>void submit({request_id:crypto.randomUUID(),expected_revision:deleting,action:{kind:"delete"}})}>Permanently delete account</button>
        <button disabled={busy||pending!==null} onClick={()=>setDeleting(null)}>Keep account</button>
      </div>}
      {target.user.id===accountId && <p className="fine">Use your password settings to change your own password. Another administrator manages your role and suspension.</p>}
    </>}
    {busy && <p role="status">Saving…</p>}
    {error && <p role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {pending && !busy && <button onClick={()=>void submit(pending)}>Retry action</button>}
  </details>;
}
