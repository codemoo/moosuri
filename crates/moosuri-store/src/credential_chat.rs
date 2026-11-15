//! One terminal typed credential read per explicit chat run. No HTTP or model polling.
use super::*;
use crate::agent_execution::{KnowledgeToolInput, KnowledgeToolResult, StepLease};
use crate::chat::{self, Run};
use moosuri_core::{CredentialChatStatus, MessagePhase, RunState};
use moosuri_vault_protocol::broker::{
    GitHubRepository, Outcome, Projection, Resource, SignedReceipt,
};
const PREVIEW_OUTPUT_RESERVE: i64 = 8 * 1024;
/// Durable display boundary, independent of current feature/profile flags. The
/// resource recognizes R0 history; the item marker also protects newer records.
pub(crate) fn display_only_preview(c: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2 AND json_extract(repository,'$.google_inbox')='preview_v1') OR EXISTS(SELECT 1 FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item='credential-google-preview-result')",params![tenant,run],|r|r.get(0))?)
}
fn encode<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::Corrupt)
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn profile_enabled(profile: Option<&moosuri_core::RunToolProfile>, resource: &Resource) -> bool {
    profile.is_some_and(|p| match resource {
        Resource::GitHub(_) => p.credential_github,
        Resource::Gitea(_) => p.credential_gitea,
        Resource::GoogleBody(_) | Resource::GoogleModelRead(_) => false,
        Resource::GoogleInbox(g) => match g.google_inbox {
            moosuri_vault_protocol::broker::GoogleInboxVersion::CountsV1 => p.credential_google,
            moosuri_vault_protocol::broker::GoogleInboxVersion::PreviewV1 => {
                p.credential_google_preview
            }
        },
    })
}
fn resource_name(resource: &Resource) -> String {
    match resource {
        Resource::Gitea(r) => format!("{}/{}", r.repository.owner, r.repository.repository),
        Resource::GitHub(r) => format!("{}/{}", r.owner, r.repository),
        Resource::GoogleBody(_) => "INBOX plaintext body".into(),
        Resource::GoogleModelRead(_) => "model-readable Gmail".into(),
        Resource::GoogleInbox(g) => {
            if g.google_inbox == moosuri_vault_protocol::broker::GoogleInboxVersion::PreviewV1 {
                "INBOX preview"
            } else {
                "INBOX"
            }
            .into()
        }
    }
}
fn service(resource: &Resource) -> &'static str {
    match resource {
        Resource::Gitea(_) => "gitea",
        Resource::GitHub(_) => "github",
        Resource::GoogleInbox(_) | Resource::GoogleBody(_) | Resource::GoogleModelRead(_) => {
            "google"
        }
    }
}
fn service_label(resource: &Resource) -> &'static str {
    match resource {
        Resource::Gitea(_) => "Gitea",
        Resource::GitHub(_) => "GitHub",
        Resource::GoogleInbox(_) | Resource::GoogleBody(_) | Resource::GoogleModelRead(_) => {
            "Gmail INBOX"
        }
    }
}
fn literal_google_preview(text: &str) -> bool {
    if text.len() > 512 || text.contains(['\n', '\r']) {
        return false;
    }
    let text = text.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let english = text.strip_prefix("please ").unwrap_or(&text);
    let english = english.strip_suffix(" please").unwrap_or(english);
    if [
        "show my gmail inbox preview",
        "get my gmail inbox preview",
        "read my gmail inbox preview",
    ]
    .contains(&english)
    {
        return true;
    }
    for owner in ["내 ", "나의 "] {
        for service in ["gmail ", "지메일 "] {
            for inbox in ["inbox ", "받은편지함 ", "받은 편지함 "] {
                for action in [
                    "미리보기 보여줘",
                    "미리보기 보여 줘",
                    "미리보기 보여주세요",
                    "미리보기 보여 주세요",
                    "미리보기를 보여줘",
                    "미리보기 알려줘",
                ] {
                    if text == format!("{owner}{service}{inbox}{action}") {
                        return true;
                    }
                }
            }
        }
    }
    false
}
fn preview_summary(
    messages: &[moosuri_vault_protocol::broker::GoogleMessagePreview],
    unavailable: u8,
    more: bool,
) -> String {
    // Fixed plain-text result. ChatPanel renders message.text as a React text
    // child, without HTML/Markdown/link parsing; no model continuation is called.
    let mut text = String::from(
        "Gmail INBOX 미리보기 · 최대 5개\n표시 순서는 메일 도착 순서와 다를 수 있습니다.\n",
    );
    if messages.is_empty() && unavailable == 0 {
        text.push_str("이 INBOX 목록 응답에 메시지가 없습니다.\n");
    }
    for (i, m) in messages.iter().enumerate() {
        text.push_str(&format!(
            "\n{}. 제목: {}\n보낸 사람: {}\n날짜: {}\n미리보기: {}\n",
            i + 1,
            m.subject,
            m.from,
            m.date,
            m.snippet
        ));
        if m.truncated {
            text.push_str("일부 표시 문구가 길이 제한으로 잘렸습니다.\n");
        }
    }
    if unavailable > 0 {
        text.push_str(&format!(
            "\n목록 이후 삭제되거나 INBOX에서 이동되어 확인하지 못한 항목: {unavailable}개.\n"
        ));
    }
    if more {
        text.push_str("추가 페이지가 있지만 이 요청에서는 읽지 않았습니다.\n");
    }
    text
}
// A deliberately small whole-message command grammar. A matching topic inside
// negation, translation, quotation or source-analysis prose grants no authority.
fn literal_google(text: &str) -> bool {
    if text.len() > 512 || text.contains(['\n', '\r']) {
        return false;
    }
    let text = text.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let english = text.strip_prefix("please ").unwrap_or(&text);
    let english = english.strip_suffix(" please").unwrap_or(english);
    for action in ["read ", "show ", "check ", "get ", "tell me "] {
        if let Some(count) = english
            .strip_prefix(action)
            .and_then(|s| s.strip_prefix("my gmail inbox "))
            && [
                "count",
                "counts",
                "unread count",
                "unread counts",
                "message count",
                "message counts",
                "thread count",
                "thread counts",
                "unread message count",
                "unread message counts",
                "unread thread count",
                "unread thread counts",
                "message and thread counts",
                "total and unread message and thread counts",
            ]
            .contains(&count)
        {
            return true;
        }
    }
    for owner in ["내 ", "나의 "] {
        let Some(rest) = text.strip_prefix(owner) else {
            continue;
        };
        for service in ["gmail ", "지메일 "] {
            let Some(rest) = rest.strip_prefix(service) else {
                continue;
            };
            for inbox in [
                "inbox ",
                "받은편지함 ",
                "받은 편지함 ",
                "받은편지함(inbox) ",
            ] {
                let Some(rest) = rest.strip_prefix(inbox) else {
                    continue;
                };
                if [
                    "읽지 않은 메일 몇 개야",
                    "읽지 않은 메일이 몇 개야",
                    "읽지 않은 메일은 몇 개야",
                    "읽지않은 메일 몇 개야",
                ]
                .contains(&rest)
                {
                    return true;
                }
                for counts in [
                    "개수 ",
                    "개수를 ",
                    "읽지 않은 개수 ",
                    "읽지 않은 개수를 ",
                    "읽지 않은 메일 개수 ",
                    "읽지 않은 메일 개수를 ",
                    "읽지않은 개수 ",
                    "메시지와 스레드 개수 ",
                    "메시지와 스레드 개수를 ",
                ] {
                    if let Some(action) = rest.strip_prefix(counts)
                        && [
                            "알려줘",
                            "알려 줘",
                            "알려주세요",
                            "알려 주세요",
                            "조회해줘",
                            "조회해 줘",
                            "조회해 주세요",
                            "보여줘",
                            "보여 줘",
                        ]
                        .contains(&action)
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}
pub(crate) fn has(c: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, run],
        |r| r.get(0),
    )?)
}
/// Called inside the broker writer transaction for NEW authority only. Device
/// enrollment may use another live session; it cannot replace the original chat
/// session or a cancelled/revised wait. Existing commitments bypass this guard
/// solely for non-authorizing recovery/settlement.
pub(crate) fn dispatch_current(
    c: &Connection,
    tenant: &str,
    run_id: &str,
    step: u32,
    call: &str,
    nonce: &str,
) -> Result<bool> {
    if let (Ok(run), Ok(call), Ok(nonce)) = (run_id.parse(), call.parse(), nonce.parse())
        && let Some(current) = crate::native_task_mail::dispatch_current(
            c,
            tenant,
            &moosuri_vault_protocol::broker::Invocation {
                run,
                step,
                call,
                nonce,
            },
        )?
    {
        return Ok(current);
    }
    let wait:Option<(i32,i64,String,String,String,i64)>=c.query_row("SELECT attempt,step,broker_call,invocation_nonce,status,expires_at FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2",params![tenant,run_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((attempt, pinned_step, pinned_call, pinned_nonce, status, expires)) = wait else {
        return Ok(true);
    }; // settings-only request
    if !cfg!(feature = "credential-broker-chat")
        || status != "waiting"
        || expires <= now()?
        || pinned_step != i64::from(step)
        || pinned_call != call
        || pinned_nonce != nonce
    {
        return Ok(false);
    }
    let run = chat::load_run(c, tenant, run_id)?;
    let resource: String = c.query_row(
        "SELECT repository FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2",
        params![tenant, run_id],
        |r| r.get(0),
    )?;
    let resource: Resource = decode(&resource)?;
    if run.state != RunState::Queued
        || run.attempt != attempt
        || !profile_enabled(run.tool_profile.as_ref(), &resource)
        || chat::authority(c, &run).is_err()
    {
        return Ok(false);
    }
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM agent_execution a JOIN credential_chat_waits w ON w.tenant_id=a.tenant_id AND w.run_id=a.run_id JOIN agent_tool_slots t ON t.tenant_id=w.tenant_id AND t.run_id=w.run_id AND t.step=w.step AND t.call_id=w.call_id JOIN credential_broker_jobs j ON j.tenant_id=w.tenant_id AND j.request_id=w.request_id WHERE w.tenant_id=?1 AND w.run_id=?2 AND a.invalidated=0 AND a.deadline_ms>?3 AND t.result IS NULL AND j.run_id=w.run_id AND j.account_id=?4 AND j.auth_epoch=?5 AND j.expires_at>?6)",params![tenant,run_id,millis_now()?,run.account,run.epoch,now()?],|r|r.get(0))?;
    Ok(valid)
}
fn millis_now() -> Result<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Unavailable)?
        .as_millis()
        .try_into()
        .map_err(|_| StoreError::Unavailable)
}
pub(crate) fn status(c: &Connection, r: &Run) -> Result<Option<CredentialChatStatus>> {
    let row:Option<(String,String,String)>=c.query_row("SELECT request_id,repository,status FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?))).optional()?;
    row.map(|(request_id, repo, status)| {
        let resource: Resource = decode(&repo)?;
        Ok(CredentialChatStatus {
            request_id,
            service: if matches!(resource, Resource::Gitea(_)) {
                Some("gitea".into())
            } else if matches!(resource, Resource::GoogleInbox(_)) {
                Some("google".into())
            } else {
                None
            },
            repository: resource_name(&resource),
            status: if status == "settled" {
                match r.state {
                    RunState::Completed => "complete",
                    RunState::OutcomeUnknown => "outcome_unknown",
                    _ => "not_completed",
                }
                .into()
            } else {
                status
            },
        })
    })
    .transpose()
}
pub(crate) fn cancel(c: &Connection, r: &Run) -> Result<()> {
    if !has(c, &r.tenant, &r.id)? {
        return Ok(());
    }
    c.execute("UPDATE credential_broker_jobs SET expires_at=min(expires_at,?3) WHERE tenant_id=?1 AND run_id=?2 AND EXISTS(SELECT 1 FROM credential_chat_waits w WHERE w.tenant_id=?1 AND w.run_id=?2 AND w.status='waiting')",params![r.tenant,r.id,now()?])?;
    c.execute("UPDATE credential_broker_invocations SET cancelled=1 WHERE tenant_id=?1 AND run_id=?2 AND EXISTS(SELECT 1 FROM credential_chat_waits w WHERE w.tenant_id=?1 AND w.run_id=?2 AND w.status='waiting')",params![r.tenant,r.id])?;
    c.execute("UPDATE credential_chat_waits SET status='cancelled' WHERE tenant_id=?1 AND run_id=?2 AND status='waiting'",params![r.tenant,r.id])?;
    Ok(())
}
pub(crate) fn quarantine(c: &Connection) -> Result<()> {
    c.execute_batch("UPDATE credential_broker_jobs SET expires_at=min(expires_at,unixepoch()) WHERE (tenant_id,run_id) IN(SELECT tenant_id,run_id FROM credential_chat_waits);UPDATE credential_broker_invocations SET cancelled=1 WHERE (tenant_id,run_id) IN(SELECT tenant_id,run_id FROM credential_chat_waits);UPDATE credential_chat_waits SET status='cancelled' WHERE status='waiting';")?;
    Ok(())
}
/// Whole-message direct command grammar; mentions inside source/quotes/negation
/// cannot authorize local credential use merely by containing a repository name.
fn literal_gitea(text: &str, repo: &GitHubRepository) -> bool {
    if text.len() > 512 || text.contains(['\n', '\r']) {
        return false;
    }
    let normalized = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_ascii_lowercase();
    let text = normalized.strip_prefix("please ").unwrap_or(&normalized);
    let target = format!("{}/{}", repo.owner, repo.repository);
    [
        format!("gitea {target} metadata"),
        format!("gitea {target} issue count"),
        format!("gitea {target} open issue count"),
        format!("read gitea {target} open issue count"),
        format!("show gitea {target} open issue count"),
        format!("gitea {target}의 issue 수 읽어 줘"),
        format!("gitea {target}의 issue 수 읽어줘"),
        format!("gitea {target} 이슈 수 읽어 줘"),
        format!("gitea {target} 이슈 수 읽어줘"),
        format!("gitea {target}의 이슈 수 읽어 줘"),
        format!("gitea {target}의 이슈 수 읽어줘"),
    ]
    .iter()
    .any(|command| text == command)
}
fn literal_target(text: &str, repo: &GitHubRepository) -> bool {
    let target = format!("{}/{}", repo.owner, repo.repository);
    let text = text.to_ascii_lowercase();
    text.match_indices(&target).any(|(at, _)| {
        let end = at + target.len();
        let valid = |b: u8| b.is_ascii_alphanumeric() || b"._-/".contains(&b);
        (at == 0 || !valid(text.as_bytes()[at - 1]) || text[..at].ends_with("https://github.com/"))
            && (end == text.len() || !valid(text.as_bytes()[end]))
    })
}
impl Store {
    /// Called only for an already stored current model tool slot. Returns false for
    /// unsupported/mixed plans, so the ordinary bounded tool error path can answer.
    pub async fn wait_for_github_tool(&self, lease: &StepLease, call_id: String) -> Result<bool> {
        self.wait_for_credential_tool(lease, call_id).await
    }
    pub async fn wait_for_credential_tool(
        &self,
        lease: &StepLease,
        call_id: String,
    ) -> Result<bool> {
        let lease = lease.clone();
        self.call(move|db|{
   let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
   let run=match agent_execution::guard_step(&tx,&db.provider_key,&lease){Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
   if !cfg!(feature="credential-broker-chat") || crate::missions::is_mission(&tx,&run.tenant,&run.id)? || crate::media::has_context(&tx,&run.tenant,&run.id)? || crate::artifacts::agent::profile(&tx,&run.tenant,&run.id)?.is_some_and(|p|p.write) {return Ok(false)}
   let (input,saved):(String,Option<String>)=tx.query_row("SELECT input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step(),call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
   if saved.is_some() {return Ok(false)}
   let resource=match decode(&input)?{
    KnowledgeToolInput::CredentialGitea{owner,repository}=>{let repo=GitHubRepository{owner,repository};if !repo.valid()||!literal_gitea(&run.text,&repo){return Ok(false)}
      let scopes=tx.prepare("SELECT r.repository FROM credential_broker_repositories r JOIN credential_broker_devices d ON d.id=r.device_id JOIN credential_broker_bindings b USING(device_id,binding_id) WHERE d.tenant_id=?1 AND d.account_id=?2 AND d.status='active' AND b.active=1 AND json_extract(b.binding,'$.expires_at')>unixepoch()")?.query_map(params![run.tenant,run.account],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
      let mut matches=scopes.into_iter().filter_map(|s|decode::<Resource>(&s).ok()).filter(|r|matches!(r,Resource::Gitea(g) if g.repository==repo));let Some(resource)=matches.next()else{return Ok(false)};if matches.next().is_some(){return Ok(false)}resource},
    KnowledgeToolInput::CredentialGitHub{owner,repository}=>{let repo=GitHubRepository{owner,repository};if !repo.valid()||!literal_target(&run.text,&repo){return Ok(false)}Resource::GitHub(repo)},
    KnowledgeToolInput::CredentialGoogleInbox{}=>{if !literal_google(&run.text){return Ok(false)}Resource::google_inbox()},
    KnowledgeToolInput::CredentialGooglePreview{}=>{if !literal_google_preview(&run.text){return Ok(false)}Resource::google_preview()},
    _=>return Ok(false)
   };if !profile_enabled(run.tool_profile.as_ref(),&resource){return Ok(false)}
   let (calls,used):(i64,i64)=tx.query_row("SELECT (SELECT count(*) FROM agent_tool_slots t WHERE t.tenant_id=?1 AND t.run_id=?2),tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
   if calls!=1||used!=0||has(&tx,&run.tenant,&run.id)? {return Ok(false)}
   let (bytes,items):(i64,i64)=tx.query_row("SELECT coalesce(sum(length(CAST(text AS BLOB))),0),count(*) FROM chat_output WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
   let reserve=if resource==Resource::google_preview(){PREVIEW_OUTPUT_RESERVE}else{4*1024};
   if bytes>256*1024-reserve || items>=127{return Ok(false)}

   // Run/session authority is actual admission, never a caller-selected user ID.
   let user=chat::authority(&tx,&run)?;
   // Waiting returns worker/model capacity but never extends the run's original
   // cumulative deadline. Native mint/commit/entry inherit this same expiry.
   let deadline:i64=tx.query_row("SELECT deadline_ms FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
   let expires=deadline/1000;
   if expires<=now()?{return Ok(false)}
   let request_id=Uuid::new_v4().to_string();let broker_call=Uuid::new_v4().to_string();let nonce=Uuid::new_v4().to_string();let repository=encode(&resource)?;
   tx.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request,step) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10)",params![run.tenant,user.account.id,request_id,run.id,broker_call,nonce,repository,run.epoch,expires,lease.step()])?;
   tx.execute("INSERT INTO credential_chat_waits VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'waiting',?10)",params![run.tenant,run.id,run.attempt,lease.step(),call_id,broker_call,nonce,request_id,repository,expires])?;
   tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
   // A completed model plan stays durable; no new attempt/checkpoint or model is
   // started to wait for the device. Deleting this lease fences a retired runner.
   tx.execute("DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
   chat::transition(&tx,&run,RunState::Queued,None)?;
   tx.commit()?;Ok(true)
  }).await
    }
    /// Store-only result observation, bounded and slot-free. The narrow terminal
    /// result is formatted from the original signed projection, never another LLM.
    pub async fn settle_credential_chats(&self) -> Result<usize> {
        if !cfg!(feature = "credential-broker-chat") {
            return Ok(0);
        }
        self.call(|db|{
   let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
   let rows=tx.prepare("SELECT tenant_id,run_id,attempt,step,call_id,broker_call,invocation_nonce,request_id,repository,expires_at FROM credential_chat_waits WHERE status='waiting' LIMIT 32")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i32>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,i64>(9)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
   let mut changed=0;
   for (tenant,id,attempt,step,call,broker_call,nonce,request_id,repo,expires) in rows {
    let run=chat::load_run(&tx,&tenant,&id)?;
    if run.attempt!=attempt||run.state!=RunState::Queued {cancel(&tx,&run)?;changed+=1;continue}
    if chat::authority(&tx,&run).is_err(){chat::transition(&tx,&run,RunState::Cancelled,Some(RunFailure::AuthorityRevoked))?;changed+=1;continue}
    // No saved-source/other-tool plan was admitted for this terminal read. Still
    // honor explicit invalidation, and never fabricate a result for a missing slot.
    let invalid:bool=tx.query_row("SELECT invalidated FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![tenant,id],|r|r.get(0))?;
    if invalid {chat::transition(&tx,&run,RunState::Cancelled,Some(RunFailure::AuthorityRevoked))?;changed+=1;continue}
    let commitment:Option<Option<String>>=tx.query_row("SELECT receipt FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5",params![tenant,id,step,broker_call,nonce],|r|r.get(0)).optional()?;
    let resource:Resource=decode(&repo)?;
    let (mut text,result,mut state,status)=if let Some(Some(receipt))=commitment.as_ref() {
     let receipt:SignedReceipt=decode(receipt)?;let r=&receipt.receipt;let request=&r.commitment.request;
     let key:Vec<u8>=tx.query_row("SELECT public_key FROM credential_broker_devices WHERE id=?1 AND account_id=?2 AND tenant_id=?3",params![request.device.id.to_string(),run.account,tenant],|row|row.get(0))?;
     moosuri_credential_broker::auth::verify(&key,&moosuri_vault_protocol::broker::receipt_bytes(r),&receipt.signature).map_err(|_|StoreError::Unauthorized)?;
     let stored:String=tx.query_row("SELECT request FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2 AND run_id=?3",params![tenant,request_id,id],|row|row.get(0))?;
     let original:moosuri_vault_protocol::broker::Request=decode(&stored)?;
     if original!=*request{return Err(StoreError::Corrupt)}

     if request.operation!=resource.operation()||request.repository.as_ref()!=resource.repository()||request.device.owner.account.to_string()!=run.account||request.device.owner.tenant.to_string()!=tenant||request.device.owner.epoch!=run.epoch||request.invocation.run.to_string()!=id||request.invocation.step as i64!=step||request.invocation.call.to_string()!=broker_call||request.invocation.nonce.to_string()!=nonce {return Err(StoreError::Corrupt)}
     let (text,state,status)=match (&resource,r.outcome,&r.projection) {
      (Resource::Gitea(repo),Outcome::ProjectedSuccess,Projection::GiteaRepository{open_issues})=>(format!("Gitea {}/{}\n열린 issue: {}개\n신뢰 기기의 로컬 브라우저에서 로그인과 TOTP 후 확인한 인증된 읽기 영수증입니다.",repo.repository.owner,repo.repository.repository,open_issues),RunState::Completed,"complete"),
      (Resource::GitHub(repo),Outcome::ProjectedSuccess,Projection::GitHubRepository{id:repository_id,private,archived,stars,open_issues})=>(format!("GitHub {}/{}\nRepository ID: {}\nPrivate: {} · Archived: {}\nStars: {} · Open issues: {}\n신뢰 기기의 인증된 메타데이터 읽기 영수증으로 확인했습니다.",repo.owner,repo.repository,repository_id,private,archived,stars,open_issues),RunState::Completed,"complete"),
      (Resource::GoogleInbox(_),Outcome::ProjectedSuccess,Projection::GoogleInbox{messages_total,messages_unread,threads_total,threads_unread})=>(format!("Gmail INBOX\n메시지 전체: {messages_total} · 읽지 않음: {messages_unread}\n스레드 전체: {threads_total} · 읽지 않음: {threads_unread}\n신뢰 기기에서 계정과 실제 권한을 확인한 인증된 읽기 영수증입니다."),RunState::Completed,"complete"),
      (Resource::GoogleInbox(g),Outcome::ProjectedSuccess,Projection::GoogleInboxPreview{messages,unavailable,more_available}) if g.google_inbox==moosuri_vault_protocol::broker::GoogleInboxVersion::PreviewV1 => (preview_summary(messages,*unavailable,*more_available),RunState::Completed,"complete"),
      (_,Outcome::ProjectedSuccess,_)=>return Err(StoreError::Corrupt),
      (_,Outcome::Unknown,_) =>(format!("{} 읽기 결과가 미확정입니다. 원래 요청의 영수증만 확인하며 다시 실행하지 않습니다.",service_label(&resource)),RunState::OutcomeUnknown,"outcome_unknown"),
      (_,Outcome::Rejected,_) =>("기기 credential이 거절되었습니다. 신뢰 기기 연결에서 credential을 갱신한 뒤 새로 요청하세요. 이 요청을 재전송하지 않았습니다.".into(),RunState::Incomplete,"credential_rejected"),
      _ =>(format!("원래 {} 요청은 기기에서 실행되지 않았습니다. 다시 실행하지 않았으며, 필요하면 새 요청을 작성하세요.",service_label(&resource)),RunState::Incomplete,"not_dispatched"),
     };
     (text,KnowledgeToolResult::External(serde_json::json!({"service":service(&resource),"repository":resource,"request_id":request_id,"status":status,"projection":r.projection})),state,status)
    }else if expires<=now()? && commitment.is_some() {
     (format!("{} 읽기가 이미 시작됐을 수 있지만 영수증이 도착하지 않았습니다. 미확정으로 보존하며 다시 실행하지 않습니다.",service_label(&resource)),KnowledgeToolResult::Error(agent_execution::ToolReadError::OutcomeUnknown),RunState::OutcomeUnknown,"outcome_unknown")
    }else if expires<=now()? {
     (format!("{} credential 대기 시간이 만료되었습니다. 원래 요청을 실행하지 않았으며 다시 보내지 않습니다.",service_label(&resource)),KnowledgeToolResult::Error(agent_execution::ToolReadError::Unavailable),RunState::Incomplete,"expired")
    }else{continue};
    if resource==Resource::google_preview() {
        let used:i64=tx.query_row("SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM chat_output WHERE tenant_id=?1 AND run_id=?2",params![tenant,id],|r|r.get(0))?;
        let remaining=(256*1024_i64-used).max(0) as usize;
        // Also retire a retained R0 wait that reserved only4KiB. The complete
        // signed projection remains owner-visible in personal settings.
        if text.len()>remaining {
            text="Gmail INBOX 미리보기 영수증을 받았습니다. 대화 출력 한도로 메일 내용을 표시하지 못했습니다. 개인 설정에서 원래 미리보기 요청의 결과를 확인하세요.".into();
            if text.len()>remaining {text.clear();state=RunState::Incomplete;}
        }
    }
    let old:Option<String>=tx.query_row("SELECT result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![tenant,id,step,call],|r|r.get(0)).optional()?.ok_or(StoreError::Corrupt)?;
    if old.is_some(){return Err(StoreError::Corrupt)}
    tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![tenant,id,step,call,encode(&result)?])?;
    tx.execute("UPDATE credential_chat_waits SET status='settled' WHERE tenant_id=?1 AND run_id=?2",params![tenant,id])?;
    if !text.is_empty(){chat::apply_patches(&tx,&run,vec![OutputPatch::Append{item:if resource==Resource::google_preview(){"credential-google-preview-result".into()}else if matches!(resource,Resource::GoogleInbox(_)){"credential-google-result".into()}else{"credential-github-result".into()},text,phase:Some(MessagePhase::FinalAnswer),refusal:false}])?;}
    chat::transition(&tx,&run,state,if state==RunState::OutcomeUnknown{Some(RunFailure::ConnectionFailed)}else{None})?;
    let _=status;changed+=1;
   }
   tx.commit()?;Ok(changed)
  }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn google_inbox_literal_request_uses_only_current_personal_count_scope() {
        for text in [
            "내 Gmail INBOX 개수 알려줘",
            "나의 지메일 받은편지함 개수 알려주세요!",
            "내 Gmail 받은편지함 읽지 않은 메일 몇 개야?",
            "show my gmail inbox unread count",
            "Please show my Gmail inbox message count.",
            "check my Gmail inbox thread counts please!",
            "read my Gmail inbox counts",
        ] {
            assert!(literal_google(text), "affirmative grammar");
        }
        for text in [
            "Do not read my Gmail INBOX unread count",
            "Please don't show my Gmail inbox counts",
            "Translate my Gmail inbox unread count",
            "Explain the phrase show my Gmail inbox counts",
            "내 Gmail INBOX 개수 알려주지 마",
            "내 Gmail INBOX 개수 알려줘 문장을 번역해줘",
            "문서 내용: 내 Gmail INBOX 개수 알려줘",
            "내 Gmail 받은편지함 개수에 대한 문장을 분석해줘",
            "Gmail INBOX 개수",
            "내 Gmail 메일 보내줘",
            "내 Gmail INBOX counts for other@example.test",
            "문서 내용: \"내 Gmail INBOX 개수\"",
            "내 Gmail INBOX https://evil.invalid count",
        ] {
            assert!(!literal_google(text), "non-authorizing wrapper");
        }
    }
    #[test]
    fn credential_chat_literal_target_is_exact_and_accepts_the_canonical_github_url() {
        let repo = GitHubRepository {
            owner: "owner".into(),
            repository: "repo".into(),
        };
        assert!(literal_target("GitHub owner/repo metadata", &repo));
        assert!(literal_target("https://github.com/owner/repo 상태", &repo));
        for text in [
            "otherowner/repo metadata",
            "owner/repository",
            "https://evil.invalid/owner/repo",
            "owner/repo/issues",
        ] {
            assert!(!literal_target(text, &repo));
        }
    }
}

#[cfg(all(test, feature = "credential-broker-chat"))]
mod session_tests {
    use super::*;
    use crate::tests::{PASSWORD, TEST_LOCK, admin, directory};
    use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
    use moosuri_credential_broker::{
        auth::{SigningKey, authenticated_bytes, projection_digest},
        policy::LocalGrant,
    };
    use moosuri_vault_protocol::broker::{
        Binding, Command, Commitment, Device, Invocation, PairProof, Receipt, Reply, SignedCommand,
    };
    async fn configured(s: &Store) -> UserContext {
        let b = admin(s).await.user;
        let provider = s
            .create_provider(
                &b,
                ProviderChange {
                    name: "Synthetic".into(),
                    enabled: true,
                    config: ProviderConfig {
                        kind: ProviderKind::Compatible,
                        base_url: Some("http://127.0.0.1:11434/v1".into()),
                        allowed_ips: vec!["127.0.0.1".into()],
                        router_providers: vec![],
                        zero_data_retention: false,
                    },
                    credential: CredentialChange::Clear,
                },
            )
            .await
            .unwrap();
        s.record_model_probe(&b, provider.id.clone(), 1, "synthetic".into(), None)
            .await
            .unwrap();
        s.record_tool_probe(&b, provider.id.clone(), 1, "synthetic".into(), None)
            .await
            .unwrap();
        s.set_default_model(
            &b,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
        b
    }
    async fn command(s: &Store, d: &Device, k: &SigningKey, command: Command) -> Result<Reply> {
        let challenge = s.broker_auth_challenge(d.id).await?;
        s.broker_command(SignedCommand {
            challenge: challenge.nonce,
            signature: k.sign(
                &authenticated_bytes(
                    &challenge,
                    "POST",
                    moosuri_vault_protocol::broker::DEVICE_PATH,
                    &command,
                )
                .unwrap(),
            ),
            command,
        })
        .await
    }
    async fn wait(s: &Store, a: &UserContext) -> String {
        wait_read(
            s,
            a,
            "GitHub owner/repo metadata",
            KnowledgeToolInput::CredentialGitHub {
                owner: "owner".into(),
                repository: "repo".into(),
            },
        )
        .await
    }
    async fn wait_read(
        s: &Store,
        a: &UserContext,
        text: &str,
        input: KnowledgeToolInput,
    ) -> String {
        let thread = s
            .create_thread(a, "Synthetic session race".into())
            .await
            .unwrap();
        let turn = s
            .submit_turn(a, thread.id, Uuid::new_v4().to_string(), text.into())
            .await
            .unwrap();
        let claimed = s.claim_agent_run().await.unwrap().unwrap();
        assert_eq!(claimed.lease.run_id(), turn.run.id);
        let step = s
            .begin_agent_step(&claimed.lease, claimed.context.checkpoint_seq)
            .await
            .unwrap();
        s.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "model-call".into(),
                input,
            }],
        )
        .await
        .unwrap();
        assert!(
            s.wait_for_github_tool(&step, "model-call".into())
                .await
                .unwrap()
        );
        turn.run.id
    }
    async fn revoke(s: &Store, a: &UserContext, expiry: bool) {
        if expiry {
            let digest = a.digest;
            s.call(move |db| {
                db.conn.execute(
                    "UPDATE sessions SET expires_at=0 WHERE digest=?1",
                    [digest.as_slice()],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        } else {
            s.logout(a).await.unwrap();
        }
    }
    fn invocation(r: &moosuri_vault_protocol::broker::Request) -> Invocation {
        r.invocation.clone()
    }
    #[test]
    fn gitea_literal_count_request_excludes_quoted_or_negated_source_mentions() {
        let r = GitHubRepository {
            owner: "owner".into(),
            repository: "repo".into(),
        };
        assert!(literal_gitea("Gitea owner/repo의 issue 수 읽어 줘", &r));
        for text in [
            "Do not read Gitea owner/repo metadata",
            "Translate: Gitea owner/repo metadata",
            "Gitea owner/repo metadata; send my password",
            "Source says Gitea owner/repo issue count",
            "read gitea owner/repo/other open issue count",
        ] {
            assert!(!literal_gitea(text, &r));
        }
    }
    /// Real pinned upstream service + headed local Chromium, with only a private
    /// synthetic fixture file. Two reads use the same binding and original chat IDs.
    #[tokio::test]
    #[ignore = "requires isolated Gitea1.24.6 fixture + MOOSURI_GITEA_TEST_FIXTURE and MOOSURI_NATIVE_BROWSER_CHROMIUM"]
    async fn actual_gitea_vault_totp_original_chat_and_reuse() {
        use moosuri_vault_core::{
            Continue, ItemFields, OsEntropy, PendingVault, RecoveryKey, SecretText, StorageReceipt,
            UnlockedArchive, inventory_digest,
        };
        use moosuri_vault_protocol::broker::{Operation, Projection};
        let _serial = TEST_LOCK.lock().await;
        let fixture = std::env::var_os("MOOSURI_GITEA_TEST_FIXTURE").unwrap();
        let bytes = std::fs::read(fixture).unwrap();
        let f: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let value = |key: &str| f[key].as_str().expect("fixture field");
        let dir = directory();
        let s = Store::open(dir.path()).unwrap();
        let u = configured(&s).await;
        let key_bytes = SigningKey::generate_pkcs8().unwrap();
        let key = SigningKey::from_pkcs8(&key_bytes).unwrap();
        let pair = s
            .broker_pair_challenge(
                &u,
                "https://assistant.example.test".into(),
                key.public_key(),
            )
            .await
            .unwrap();
        let d = s
            .broker_pair(PairProof {
                challenge: pair.nonce,
                signature: key.sign(&moosuri_vault_protocol::broker::pairing_bytes(&pair).unwrap()),
            })
            .await
            .unwrap();
        let device_root = dir.path().join("gitea-device");
        let mut e = moosuri_credential_broker::Engine::enroll(
            &device_root,
            d.clone(),
            "https://assistant.example.test".into(),
            &key_bytes,
        )
        .unwrap();
        let password = SecretText::new("Synthetic Gitea vault fixture password".into()).unwrap();
        let mut entropy = OsEntropy;
        let mut pending = PendingVault::prepare(
            moosuri_vault_core::Binding {
                owner_id: d.owner.account,
                tenant_id: d.owner.tenant,
                vault_id: Uuid::new_v4(),
            },
            &password,
            &mut entropy,
        )
        .unwrap();
        pending
            .verify_recovery(&RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap())
            .unwrap();
        let (id, digest) = pending.candidate().unwrap();
        pending
            .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
            .unwrap();
        let mut active = pending.activate().unwrap();
        let fields:ItemFields=serde_json::from_value(serde_json::json!({"kind":"login","title":"Synthetic Gitea","username":value("username"),"url":value("origin"),"secret":value("password"),"totp":value("totp"),"notes":"","favorite":false,"expires_at":""})).unwrap();
        let item = active
            .seal_item(Uuid::new_v4(), 1, &fields, &mut entropy)
            .unwrap();
        let item_id = item.id;
        let digest = inventory_digest(std::slice::from_ref(&item)).unwrap();
        let archive = active.export_archive(&[item], digest, &Continue).unwrap();
        let mut vault = UnlockedArchive::unlock(&archive, &password).unwrap();
        let repo = GitHubRepository {
            owner: value("owner").into(),
            repository: value("repo").into(),
        };
        let (binding, resource) = e
            .archive_gitea_grant(
                &mut vault,
                item_id,
                value("origin").into(),
                repo.clone(),
                now().unwrap() + 600,
            )
            .unwrap();
        command(
            &s,
            &d,
            &key,
            Command::RegisterGitea {
                binding: binding.clone(),
                resource: resource.clone(),
            },
        )
        .await
        .unwrap();
        let executable =
            std::path::PathBuf::from(std::env::var_os("MOOSURI_NATIVE_BROWSER_CHROMIUM").unwrap());
        e.browser_start(&executable, false, now().unwrap())
            .await
            .unwrap();
        e.browser_observe(true, now().unwrap()).await.unwrap();
        for _ in 0..2 {
            let run = wait_read(
                &s,
                &u,
                &format!("Gitea {}/{} metadata", repo.owner, repo.repository),
                KnowledgeToolInput::CredentialGitea {
                    owner: repo.owner.clone(),
                    repository: repo.repository.clone(),
                },
            )
            .await;
            let Reply::Waiting { requests, .. } =
                command(&s, &d, &key, Command::Poll).await.unwrap()
            else {
                panic!("poll")
            };
            let request = requests
                .into_iter()
                .find(|r| r.invocation.run.to_string() == run)
                .unwrap();
            assert!(
                request.operation
                    == Operation::GiteaRepositoryRead {
                        site: resource.gitea_site
                    }
            );
            let invocation = request.invocation.clone();
            e.prepare_gitea(&vault, request.clone(), now().unwrap())
                .unwrap();
            let Reply::Commitment { commitment } = command(
                &s,
                &d,
                &key,
                Command::CommitCurrent {
                    invocation: invocation.clone(),
                    request_digest: moosuri_credential_broker::auth::hash(
                        &moosuri_vault_protocol::broker::request_bytes(&request),
                    ),
                },
            )
            .await
            .unwrap() else {
                panic!("commit")
            };
            e.accept_commitment(*commitment).unwrap();
            let receipt = e.execute_gitea(&mut vault, &invocation).await.unwrap();
            assert!(
                receipt.receipt.outcome
                    == moosuri_vault_protocol::broker::Outcome::ProjectedSuccess,
                "actual browser result must succeed"
            );
            assert!(
                matches!(receipt.receipt.projection,Projection::GiteaRepository{open_issues} if open_issues==3)
            );
            let serialized = serde_json::to_string(&receipt).unwrap();
            for secret in [value("password"), value("totp")] {
                assert!(!serialized.contains(secret));
            }
            command(
                &s,
                &d,
                &key,
                Command::Settle {
                    receipt: Box::new(receipt),
                },
            )
            .await
            .unwrap();
            e.delivered(invocation).unwrap();
            s.settle_credential_chats().await.unwrap();
            let run_copy = run.clone();
            let complete = s
                .call(move |db| {
                    let state: String = db.conn.query_row(
                        "SELECT state FROM chat_runs WHERE id=?1",
                        [run_copy],
                        |r| r.get(0),
                    )?;
                    Ok(state)
                })
                .await
                .unwrap();
            assert_eq!(complete, "completed");
        }
        e.browser_close(now().unwrap()).await.unwrap();
        // Full server SQLite/WAL and local metadata canaries, not just the projection.
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_file() {
                continue;
            }
            let data = std::fs::read(path).unwrap();
            for secret in [value("password"), value("totp")] {
                assert!(!data.windows(secret.len()).any(|b| b == secret.as_bytes()));
            }
        }
        for entry in std::fs::read_dir(&device_root).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_file() {
                continue;
            }
            let journal = std::fs::read(path).unwrap();
            for secret in [value("password"), value("totp")] {
                assert!(secret.len() > 8);
                assert!(
                    !journal
                        .windows(secret.len())
                        .any(|b| b == secret.as_bytes())
                );
            }
        }
    }
    #[tokio::test]
    async fn original_chat_session_is_fenced_at_mint_reissue_and_first_commit_without_sweep() {
        let _serial = TEST_LOCK.lock().await;
        let dir = directory();
        let s = Store::open(dir.path()).unwrap();
        let b = configured(&s).await;
        let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
        let pair = s
            .broker_pair_challenge(
                &b,
                "https://assistant.example.test".into(),
                key.public_key(),
            )
            .await
            .unwrap();
        let d = s
            .broker_pair(PairProof {
                challenge: pair.nonce,
                signature: key.sign(&moosuri_vault_protocol::broker::pairing_bytes(&pair).unwrap()),
            })
            .await
            .unwrap();
        let repo = GitHubRepository {
            owner: "owner".into(),
            repository: "repo".into(),
        };
        let binding: Binding = LocalGrant::select_github(
            d.clone(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            repo.clone(),
            1,
            1,
            now().unwrap() + 600,
        )
        .unwrap()
        .binding;
        command(
            &s,
            &d,
            &key,
            Command::RegisterRepository {
                binding,
                repository: repo.clone(),
            },
        )
        .await
        .unwrap();
        for expiry in [false, true] {
            for after_poll in [false, true] {
                let a = s
                    .login("admin@example.test".into(), PASSWORD.into())
                    .await
                    .unwrap()
                    .user;
                let run = wait(&s, &a).await;
                let request = if after_poll {
                    let Reply::Waiting { requests, .. } =
                        command(&s, &d, &key, Command::Poll).await.unwrap()
                    else {
                        panic!("no poll")
                    };
                    Some(
                        requests
                            .into_iter()
                            .find(|r| r.invocation.run.to_string() == run)
                            .unwrap(),
                    )
                } else {
                    None
                };
                revoke(&s, &a, expiry).await;
                // Enrollment B remains live, and no two-second settlement sweep runs here.
                assert!(s.list_threads(&b).await.is_ok());
                let Reply::Waiting { requests, .. } =
                    command(&s, &d, &key, Command::Poll).await.unwrap()
                else {
                    panic!("no poll")
                };
                assert!(!requests.iter().any(|r| r.invocation.run.to_string() == run));
                if let Some(request) = request {
                    assert!(matches!(
                        command(
                            &s,
                            &d,
                            &key,
                            Command::Commit {
                                invocation: invocation(&request)
                            }
                        )
                        .await,
                        Err(StoreError::Forbidden)
                    ));
                }
                let checked = run.clone();
                s.call(move |db| {
                    assert_eq!(
                        db.conn.query_row(
                            "SELECT count(*) FROM credential_broker_commitments WHERE run_id=?1",
                            [&checked],
                            |r| r.get::<_, i64>(0)
                        )?,
                        0
                    );
                    assert_eq!(
                        db.conn.query_row(
                            "SELECT count(*) FROM credential_broker_invocations WHERE run_id=?1",
                            [&checked],
                            |r| r.get::<_, i64>(0)
                        )?,
                        if after_poll { 1 } else { 0 }
                    );
                    Ok(())
                })
                .await
                .unwrap();
            }
        }
        // Already-adopted work is still recoverable/settleable after A loses authority.
        let a = s
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .unwrap()
            .user;
        let run = wait(&s, &a).await;
        let Reply::Waiting { requests, .. } = command(&s, &d, &key, Command::Poll).await.unwrap()
        else {
            panic!("no poll")
        };
        let request = requests
            .into_iter()
            .find(|r| r.invocation.run.to_string() == run)
            .unwrap();
        let Reply::Commitment { commitment } = command(
            &s,
            &d,
            &key,
            Command::Commit {
                invocation: invocation(&request),
            },
        )
        .await
        .unwrap() else {
            panic!("no commit")
        };
        let original: Commitment = *commitment;
        revoke(&s, &a, false).await;
        for cmd in [
            Command::Commit {
                invocation: invocation(&request),
            },
            Command::Recover {
                invocation: invocation(&request),
            },
        ] {
            let Reply::Commitment { commitment } = command(&s, &d, &key, cmd).await.unwrap() else {
                panic!("no recovery")
            };
            assert!(*commitment == original);
        }
        let receipt = Receipt {
            commitment: original,
            outcome: Outcome::Unknown,
            projection: Projection::None,
            projection_digest: projection_digest(Projection::None),
        };
        let signed = SignedReceipt {
            signature: key.sign(&moosuri_vault_protocol::broker::receipt_bytes(&receipt)),
            receipt,
        };
        assert!(matches!(
            command(
                &s,
                &d,
                &key,
                Command::Settle {
                    receipt: Box::new(signed)
                }
            )
            .await
            .unwrap(),
            Reply::Settled
        ));
        // Settings-only jobs still use B's own live session, not a nonexistent chat A.
        let id = Uuid::new_v4().to_string();
        s.request_github_metadata(&b, id, repo).await.unwrap();
        let Reply::Waiting { requests, .. } = command(&s, &d, &key, Command::Poll).await.unwrap()
        else {
            panic!("no poll")
        };
        assert_eq!(requests.len(), 1);
        assert!(matches!(
            command(
                &s,
                &d,
                &key,
                Command::Commit {
                    invocation: invocation(&requests[0])
                }
            )
            .await
            .unwrap(),
            Reply::Commitment { .. }
        ));
    }
    #[tokio::test]
    async fn preview_output_reserve_and_retained_near_cap_wait_settle_without_rollback() {
        let _serial = TEST_LOCK.lock().await;
        let dir = directory();
        let mut s = Store::open(dir.path()).unwrap();
        let user = configured(&s).await;
        let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
        let pair = s
            .broker_pair_challenge(
                &user,
                "https://assistant.example.test".into(),
                key.public_key(),
            )
            .await
            .unwrap();
        let device = s
            .broker_pair(PairProof {
                challenge: pair.nonce,
                signature: key.sign(&moosuri_vault_protocol::broker::pairing_bytes(&pair).unwrap()),
            })
            .await
            .unwrap();
        let grant = LocalGrant::select_google_preview(
            device.clone(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            moosuri_credential_broker::google::GoogleIdentity {
                subject: "synthetic-subject".into(),
                email: "synthetic@example.test".into(),
            },
            1,
            1,
            now().unwrap() + 600,
        )
        .unwrap();
        command(
            &s,
            &device,
            &key,
            Command::RegisterGooglePreview {
                binding: grant.binding,
            },
        )
        .await
        .unwrap();
        let messages = (0..5)
            .map(|_| moosuri_vault_protocol::broker::GoogleMessagePreview {
                selection: None,
                subject: "S".repeat(256),
                from: "F".repeat(256),
                date: "D".repeat(128),
                snippet: "N".repeat(512),
                truncated: true,
            })
            .collect::<Vec<_>>();
        let rendered = preview_summary(&messages, 0, true);
        assert!(rendered.len() > 4096 && rendered.len() <= PREVIEW_OUTPUT_RESERVE as usize);
        for (bytes, legacy, admitted) in [
            (248 * 1024, false, true),
            (252 * 1024, false, false),
            (252 * 1024, true, true),
        ] {
            let thread = s.create_thread(&user, "near cap".into()).await.unwrap();
            let turn = s
                .submit_turn(
                    &user,
                    thread.id.clone(),
                    Uuid::new_v4().to_string(),
                    "내 Gmail 받은편지함 미리보기 보여줘".into(),
                )
                .await
                .unwrap();
            let claim = s.claim_agent_run().await.unwrap().unwrap();
            assert_eq!(claim.lease.run_id(), turn.run.id);
            let step = s
                .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
                .await
                .unwrap();
            let initial = if legacy { 1 } else { bytes };
            let mut remaining = initial;
            let mut patches = Vec::new();
            while remaining > 0 {
                let n = remaining.min(8192);
                patches.push(OutputPatch::Append {
                    item: "padding".into(),
                    text: "x".repeat(n),
                    phase: Some(MessagePhase::Commentary),
                    refusal: false,
                });
                remaining -= n;
            }
            s.append_agent_output(&step, patches).await.unwrap();
            s.commit_agent_step(
                &step,
                PrivateCheckpoint {
                    bytes: b"{}".to_vec(),
                },
                vec![PendingKnowledgeCall {
                    call_id: "preview-call".into(),
                    input: KnowledgeToolInput::CredentialGooglePreview {},
                }],
            )
            .await
            .unwrap();
            assert_eq!(
                s.wait_for_credential_tool(&step, "preview-call".into())
                    .await
                    .unwrap(),
                admitted
            );
            if !admitted {
                s.cancel_run(&user, turn.run.id, 1).await.unwrap();
                continue;
            }
            if legacy {
                let tenant = user.tenant.0.clone();
                let run = turn.run.id.clone();
                s.call(move|db|{db.conn.execute("UPDATE chat_output SET text=?3 WHERE tenant_id=?1 AND run_id=?2 AND item='padding'",params![tenant,run,"x".repeat(bytes)])?;Ok(())}).await.unwrap();
                drop(s);
                s = Store::open(dir.path()).unwrap();
            }
            let Reply::Waiting { requests, .. } =
                command(&s, &device, &key, Command::Poll).await.unwrap()
            else {
                panic!("waiting")
            };
            let request = requests
                .into_iter()
                .find(|r| r.invocation.run.to_string() == turn.run.id)
                .unwrap();
            let Reply::Commitment { commitment } = command(
                &s,
                &device,
                &key,
                Command::CommitCurrent {
                    invocation: request.invocation.clone(),
                    request_digest: moosuri_credential_broker::auth::hash(
                        &moosuri_vault_protocol::broker::request_bytes(&request),
                    ),
                },
            )
            .await
            .unwrap() else {
                panic!("commit")
            };
            let projection = Projection::GoogleInboxPreview {
                messages: messages.clone(),
                unavailable: 0,
                more_available: true,
            };
            let receipt = Receipt {
                commitment: *commitment,
                outcome: Outcome::ProjectedSuccess,
                projection: projection.clone(),
                projection_digest: projection_digest(projection),
            };
            let signed = SignedReceipt {
                signature: key.sign(&moosuri_vault_protocol::broker::receipt_bytes(&receipt)),
                receipt,
            };
            command(
                &s,
                &device,
                &key,
                Command::Settle {
                    receipt: Box::new(signed),
                },
            )
            .await
            .unwrap();
            assert_eq!(s.settle_credential_chats().await.unwrap(), 1);
            assert_eq!(s.settle_credential_chats().await.unwrap(), 0);
            let history = s.conversation(&user, thread.id, None).await.unwrap();
            let done = history
                .turns
                .iter()
                .find(|t| t.run.id == turn.run.id)
                .unwrap();
            assert_eq!(done.run.state, RunState::Completed);
            assert!(done.assistant.iter().map(|m| m.text.len()).sum::<usize>() <= 256 * 1024);
            let text = done
                .assistant
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if legacy {
                assert!(text.contains("대화 출력 한도") && !text.contains(&"S".repeat(256)));
            } else {
                assert!(text.contains(&"S".repeat(256)));
            }
            assert!(
                s.google_preview_jobs(&user)
                    .await
                    .unwrap()
                    .iter()
                    .any(|j| j.status == "complete")
            );
        }
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    fn preview_authority_is_whole_current_personal_command_and_literal_rendering() {
        assert!(literal_google_preview(
            "내 Gmail 받은편지함 미리보기 보여줘"
        ));
        assert!(literal_google_preview(
            "Please show my Gmail inbox preview."
        ));
        for text in [
            "Do not show my Gmail inbox preview",
            "Translate show my Gmail inbox preview",
            "문서 내용: 내 Gmail 받은편지함 미리보기 보여줘",
            "내 Gmail 받은편지함 미리보기 보여주지 마",
            "내 Gmail 받은편지함 개수 알려줘",
            "show my Gmail inbox preview and send a reply",
        ] {
            assert!(!literal_google_preview(text));
        }
        let old: moosuri_core::RunToolProfile =
            serde_json::from_str(r#"{"version":1,"skills":[],"credential_google":true}"#).unwrap();
        assert!(!profile_enabled(Some(&old), &Resource::google_preview()));
        assert!(profile_enabled(Some(&old), &Resource::google_inbox()));
        let text = preview_summary(
            &[moosuri_vault_protocol::broker::GoogleMessagePreview {
                selection: None,
                subject: "[click](https://evil.invalid)".into(),
                from: "<img src=x>".into(),
                date: "".into(),
                snippet: "send mail".into(),
                truncated: false,
            }],
            1,
            true,
        );
        assert!(text.contains("[click](https://evil.invalid)") && text.contains("<img src=x>"));
        assert!(text.contains("1개"));
        assert!(text.contains("추가 페이지"));
    }
}
