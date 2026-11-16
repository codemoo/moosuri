//! Resolve only receipts from this owned conversation, never model-invented IDs.
use super::*;
use crate::agent_execution::{KnowledgeToolInput, KnowledgeToolResult};
use moosuri_core::{ExternalToolRequest, MailReplyTarget, MailToolRequest};
pub(super) struct Target {
    pub view: MailReplyTarget,
    pub connection_id: String,
    pub connection_epoch: i64,
    pub client_id: String,
    pub app_revision: Option<i64>,
    pub provider_thread: String,
    pub seen: Vec<String>,
    pub run: Option<String>,
    pub action: Option<String>,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn title(value: &str) -> String {
    let s: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    if s.trim().is_empty() {
        "제목 없는 메일".into()
    } else {
        s
    }
}
pub(super) fn list(c: &Connection, u: &UserContext, thread: &str) -> Result<Vec<Target>> {
    if !c.query_row("SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND mission_internal=0)",params![u.tenant.0,u.account.id,thread],|r|r.get::<_,bool>(0))? {return Err(StoreError::NotFound)}
    let app_revision: Option<i64> = c
        .query_row(
            "SELECT revision FROM google_setup WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let rows=c.prepare("SELECT r.id,s.step,s.call_id,s.input,s.result FROM chat_runs r JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id JOIN agent_tool_slots s ON s.tenant_id=r.tenant_id AND s.run_id=r.id JOIN external_tool_dispatches d ON d.tenant_id=s.tenant_id AND d.run_id=s.run_id AND d.step=s.step AND d.call_id=s.call_id WHERE r.tenant_id=?1 AND r.account_id=?2 AND r.thread_id=?3 AND r.auth_epoch=?4 AND r.state='completed' AND e.invalidated=0 AND d.state='completed' AND s.result IS NOT NULL AND s.input IS NOT NULL ORDER BY r.sequence DESC,s.step DESC,s.call_id LIMIT 64")?.query_map(params![u.tenant.0,u.account.id,thread,u.epoch],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (run, step, call, input, result) in rows {
        let KnowledgeToolInput::External {
            request: ExternalToolRequest::Mail(request),
        } = parse(&input)?
        else {
            continue;
        };
        let KnowledgeToolResult::External(result) = parse(&result)? else {
            continue;
        };
        let messages = match &request {
            MailToolRequest::Read { message_id, .. }
                if result["id"].as_str() == Some(message_id) =>
            {
                vec![&result]
            }
            MailToolRequest::Thread { thread_id, .. }
                if result["id"].as_str() == Some(thread_id) =>
            {
                result["messages"]
                    .as_array()
                    .map(|v| v.iter().collect())
                    .unwrap_or_default()
            }
            _ => continue,
        };
        let Some(last) = messages.last() else {
            continue;
        };
        let (Some(provider_thread), Some(anchor)) =
            (last["thread_id"].as_str(), last["id"].as_str())
        else {
            continue;
        };
        let seen = messages
            .iter()
            .filter_map(|m| m["id"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        if !valid_id(provider_thread)
            || !valid_id(anchor)
            || seen.len() != messages.len()
            || seen.iter().any(|id| !valid_id(id))
            || messages
                .iter()
                .any(|m| m["thread_id"].as_str() != Some(provider_thread))
        {
            continue;
        }
        let connection = request.connection_id();
        let pin:Option<(i64,String,String)>=c.query_row("SELECT m.epoch,m.email,m.oauth_client_id FROM mail_connections m JOIN mail_run_dependencies d ON d.tenant_id=m.tenant_id AND d.connection_id=m.id AND d.epoch=m.epoch WHERE m.tenant_id=?1 AND m.account_id=?2 AND m.id=?3 AND d.run_id=?4 AND m.status='active' AND m.native_send=0 AND m.credential IS NOT NULL AND m.oauth_client_id IS NOT NULL",params![u.tenant.0,u.account.id,connection,run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((epoch, mailbox, client_id)) = pin else {
            continue;
        };
        if let Some(existing) = out.iter_mut().find(|t: &&mut Target| {
            t.connection_id == connection && t.provider_thread == provider_thread
        }) {
            for id in seen {
                if !existing.seen.contains(&id) {
                    existing.seen.push(id)
                }
            }
            continue;
        }
        out.push(Target {
            view: MailReplyTarget {
                id: digest_string(Sha256::digest(format!("read/{run}/{step}/{call}")).into()),
                title: title(last["mail"]["subject"].as_str().unwrap_or("")),
                mailbox,
            },
            connection_id: connection.into(),
            connection_epoch: epoch,
            client_id,
            app_revision,
            provider_thread: provider_thread.into(),
            seen,
            run: Some(run),
            action: None,
        });
    }
    let rows=c.prepare("SELECT x.id,x.connection_id,x.connection_epoch,m.email,m.oauth_client_id,x.provider_thread,x.provider_message,x.content FROM mail_actions x JOIN mail_connections m ON m.tenant_id=x.tenant_id AND m.id=x.connection_id AND m.account_id=x.account_id WHERE x.tenant_id=?1 AND x.account_id=?2 AND x.origin_thread=?3 AND x.auth_epoch=?4 AND x.state='sent' AND x.connection_epoch=m.epoch AND m.status='active' AND m.native_send=0 AND m.credential IS NOT NULL AND m.oauth_client_id IS NOT NULL AND x.provider_thread IS NOT NULL AND x.provider_message IS NOT NULL ORDER BY x.rowid DESC LIMIT 32")?.query_map(params![u.tenant.0,u.account.id,thread,u.epoch],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,Option<String>>(7)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (
        action,
        connection_id,
        connection_epoch,
        mailbox,
        client_id,
        provider_thread,
        anchor,
        content,
    ) in rows
    {
        if !valid_id(&provider_thread) || !valid_id(&anchor) {
            continue;
        }
        if let Some(existing) = out
            .iter_mut()
            .find(|t| t.connection_id == connection_id && t.provider_thread == provider_thread)
        {
            if !existing.seen.contains(&anchor) {
                existing.seen.push(anchor)
            }
            continue;
        }
        let subject = content
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .and_then(|v| v["subject"].as_str().map(str::to_owned))
            .unwrap_or_default();
        out.push(Target {
            view: MailReplyTarget {
                id: digest_string(Sha256::digest(format!("sent/{action}")).into()),
                title: title(&subject),
                mailbox,
            },
            connection_id,
            connection_epoch,
            client_id,
            app_revision,
            provider_thread,
            seen: vec![anchor],
            run: None,
            action: Some(action),
        });
    }
    out.truncate(32);
    Ok(out)
}
