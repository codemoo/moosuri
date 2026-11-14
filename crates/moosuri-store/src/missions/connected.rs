//! Explicit saved owner read grants. Never fabricate a logged-in UserContext.
use super::*;
use crate::{ExternalToolPermit, GoogleAppPin};
use moosuri_connectors::gmail::{SecretString, TokenSet};
use moosuri_core::{
    CalendarAgentPin, CalendarReadRequest, CalendarRunProfile, ConnectedMissionReads,
    ExternalToolRequest, MailToolRequest, MissionMailPin, RunToolProfile,
};

/// Reject clear required effects; this conservative gate never grants an action.
pub(crate) fn unsupported_effect(text: &str) -> bool {
    let lower = text.to_lowercase();
    let (mail, _) = moosuri_core::mission_read_domains(&lower);
    let lower = lower
        .replace("do not send", "")
        .replace("don't send", "")
        .replace("never send", "")
        .replace("보내지", "")
        .replace("발송하지", "");
    let words = lower
        .split(|c: char| !c.is_ascii_alphabetic())
        .collect::<Vec<_>>();
    let delivery = words
        .iter()
        .any(|w| matches!(*w, "send" | "forward" | "reply"))
        || ["발송해", "보내줘", "보내 줘", "전송해", "답장해"]
            .iter()
            .any(|w| lower.contains(w));
    let local = (words.contains(&"here") || lower.contains("여기")) && !lower.contains('@');
    let draft = words.iter().enumerate().any(|(i, w)| {
        matches!(*w, "create" | "write" | "prepare")
            && words.iter().skip(i + 1).take(3).any(|w| *w == "draft")
    }) || ["초안을작성", "초안작성", "초안을만들"]
        .iter()
        .any(|w| lower.split_whitespace().collect::<String>().contains(w));
    let calendar_write = words.iter().enumerate().any(|(i, w)| {
        matches!(
            *w,
            "create" | "update" | "edit" | "delete" | "reschedule" | "move"
        ) && words.iter().enumerate().skip(i + 1).take(3).any(|(j, w)| {
            matches!(*w, "event" | "meeting" | "appointment")
                || *w == "calendar"
                    && !words.get(j + 1).is_some_and(|w| {
                        matches!(
                            *w,
                            "briefing" | "summary" | "digest" | "report" | "overview"
                        )
                    })
        })
    });
    let compact = lower.split_whitespace().collect::<String>();
    let calendar_write = calendar_write
        || [
            "일정등록",
            "일정을등록",
            "일정추가",
            "일정을추가",
            "일정수정",
            "일정을수정",
            "일정삭제",
            "일정을삭제",
            "일정변경해",
            "일정을변경해",
            "일정옮겨",
            "일정을옮겨",
        ]
        .iter()
        .any(|w| compact.contains(w));
    // The write predicate already requires an event/meeting/Calendar noun.
    // A direct "create a meeting" must not need the extra word "Calendar".
    mail && (delivery && !local || draft) || calendar_write
}
pub(crate) fn admit_profile(
    c: &Connection,
    u: &UserContext,
    input: &MissionCreate,
    mut profile: RunToolProfile,
) -> Result<RunToolProfile> {
    let text = format!("{}\n{}", input.task, input.constraints);
    if unsupported_effect(&text) {
        return Err(StoreError::Forbidden);
    }
    let (mail, calendar) = moosuri_core::mission_read_domains(&text);
    if !mail && !calendar {
        return Ok(profile);
    }
    let app_revision = c
        .query_row(
            "SELECT revision FROM google_setup WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let mut grant = ConnectedMissionReads {
        app_revision,
        mail: vec![],
        calendar: vec![],
        primary_ids: vec![],
    };
    if mail {
        grant.mail=c.prepare("SELECT id,epoch,email,oauth_client_id FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND status='active' ORDER BY id LIMIT 9")?.query_map(params![u.tenant.0,u.account.id],|r|Ok(MissionMailPin{connection:moosuri_core::MailConnectionPin{id:r.get(0)?,epoch:r.get(1)?,email:r.get(2)?},client_id:r.get::<_,Option<String>>(3)?.unwrap_or_default()}))?.collect::<rusqlite::Result<_>>()?;
        if grant.mail.is_empty()
            || grant.mail.len() > 8
            || grant.mail.iter().any(|p| p.client_id.is_empty())
        {
            return Err(StoreError::Unavailable);
        }
        profile.mail_connections = grant.mail.iter().map(|p| p.connection.clone()).collect();
    }
    if calendar {
        let pins=c.prepare("SELECT id,title,epoch,client_id,primary_id FROM calendar_connections WHERE tenant_id=?1 AND account_id=?2 AND auth_epoch=?3 AND status='active' ORDER BY id LIMIT 9")?.query_map(params![u.tenant.0,u.account.id,u.epoch],|r|Ok((CalendarAgentPin{id:r.get(0)?,title:r.get(1)?,epoch:r.get(2)?,client_id:r.get(3)?,app_revision},r.get::<_,String>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if pins.is_empty() || pins.len() > 8 {
            return Err(StoreError::Unavailable);
        }
        for (pin, primary) in pins {
            grant.calendar.push(pin);
            grant.primary_ids.push(primary)
        }
    }
    if !profile.mail_connections.is_empty() {
        profile.version = if profile.web.is_some() { 3 } else { 2 };
    }
    if !pins_current(c, &u.tenant.0, &u.account.id, u.epoch, &grant)? {
        return Err(StoreError::Unavailable);
    }
    profile.mission_reads = Some(grant);
    Ok(profile)
}
fn profile(r: &Run) -> Option<&ConnectedMissionReads> {
    r.tool_profile.as_ref()?.mission_reads.as_ref()
}
fn pins_current(
    c: &Connection,
    t: &str,
    a: &str,
    epoch: i64,
    p: &ConnectedMissionReads,
) -> Result<bool> {
    let revision = c
        .query_row(
            "SELECT revision FROM google_setup WHERE singleton=1",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    if revision != p.app_revision || p.calendar.len() != p.primary_ids.len() {
        return Ok(false);
    }
    for pin in &p.mail {
        let row:Option<(i64,String,String,String)>=c.query_row("SELECT epoch,email,coalesce(oauth_client_id,''),scopes FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND status='active'",params![t,a,pin.connection.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((e, email, client, scopes)) = row else {
            return Ok(false);
        };
        let scopes: Vec<String> = parse(&scopes)?;
        if e != pin.connection.epoch
            || email != pin.connection.email
            || client != pin.client_id
            || !scopes
                .iter()
                .any(|s| s == moosuri_connectors::gmail::READ_SCOPE)
        {
            return Ok(false);
        }
    }
    for (pin, primary) in p.calendar.iter().zip(&p.primary_ids) {
        let row:Option<String>=c.query_row("SELECT scopes FROM calendar_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND epoch=?4 AND auth_epoch=?5 AND client_id=?6 AND primary_id=?7 AND status='active'",params![t,a,pin.id,pin.epoch,epoch,pin.client_id,primary],|r|r.get(0)).optional()?;
        let Some(scopes) = row else { return Ok(false) };
        let scopes: Option<Vec<String>> = parse(&scopes)?;
        if scopes.as_ref().is_none_or(|s| {
            moosuri_connectors::calendar::SCOPES
                .iter()
                .any(|required| !s.iter().any(|v| v == required))
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn current(c: &Connection, r: &Run) -> Result<bool> {
    match profile(r) {
        Some(p) => pins_current(c, &r.tenant, &r.account, r.epoch, p),
        None => Ok(true),
    }
}
pub(crate) fn saved_current(
    c: &Connection,
    t: &str,
    a: &str,
    epoch: i64,
    model: &str,
) -> Result<bool> {
    let value: serde_json::Value = parse(model)?;
    let Some(v) = value.pointer("/agent_profile/mission_reads") else {
        return Ok(true);
    };
    let p: ConnectedMissionReads =
        serde_json::from_value(v.clone()).map_err(|_| StoreError::Corrupt)?;
    pins_current(c, t, a, epoch, &p)
}
pub(crate) fn admit_run(c: &Connection, r: &Run) -> Result<()> {
    let Some(p) = profile(r) else { return Ok(()) };
    if !current(c, r)? {
        return Err(StoreError::Conflict);
    }
    for pin in &p.mail {
        c.execute(
            "INSERT OR IGNORE INTO mail_run_dependencies VALUES(?1,?2,?3,?4)",
            params![r.tenant, r.id, pin.connection.id, pin.connection.epoch],
        )?;
    }
    for pin in &p.calendar {
        c.execute(
            "INSERT OR IGNORE INTO calendar_run_dependencies VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                r.tenant,
                r.id,
                pin.id,
                pin.epoch,
                pin.app_revision,
                pin.client_id
            ],
        )?;
    }
    if !p.calendar.is_empty() {
        let timezone: Option<String> = c
            .query_row(
                "SELECT time_zone FROM calendar_connections WHERE tenant_id=?1 AND id=?2",
                params![r.tenant, p.calendar[0].id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let cp = CalendarRunProfile {
            version: 1,
            reference: r.created,
            timezone,
            connections: p.calendar.clone(),
        };
        c.execute(
            "INSERT INTO calendar_agent_profiles VALUES(?1,?2,?3,1,?4)",
            params![r.tenant, r.id, r.account, json(&cp)?],
        )?;
    }
    Ok(())
}
/// An ID is usable only after fresh successful typed evidence in this occurrence.
fn observed_mail(
    c: &Connection,
    r: &Run,
    id: &str,
    thread: bool,
    connection: &str,
) -> Result<bool> {
    let rows=c.prepare("SELECT s.result FROM agent_tool_slots s JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE s.tenant_id=?1 AND s.run_id=?2 AND d.state='completed' AND json_extract(s.input,'$.request.adapter')='mail' AND json_extract(s.input,'$.request.request.connection_id')=?3 AND s.result IS NOT NULL")?.query_map(params![r.tenant,r.id,connection],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for raw in rows {
        let value: serde_json::Value = parse(&raw)?;
        let Some(v) = value.get("value") else {
            continue;
        };
        let messages = v.get("messages").and_then(|v| v.as_array());
        if messages.is_some_and(|m| {
            m.iter().any(|m| {
                if thread {
                    m.get("threadId")
                        .or_else(|| m.get("thread_id"))
                        .and_then(|v| v.as_str())
                        == Some(id)
                } else {
                    m.get("id").and_then(|v| v.as_str()) == Some(id)
                }
            })
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}
pub(crate) fn allowed(c: &Connection, r: &Run, q: &ExternalToolRequest) -> Result<bool> {
    let Some(p) = profile(r) else {
        return Ok(false);
    };
    if !current(c, r)? {
        return Ok(false);
    }
    match q {
        ExternalToolRequest::Mail(q) => {
            if !p.mail.iter().any(|p| p.connection.id == q.connection_id()) {
                return Ok(false);
            };
            match q {
                MailToolRequest::Search { .. } => Ok(true),
                MailToolRequest::Read { message_id, .. } => {
                    observed_mail(c, r, message_id, false, q.connection_id())
                }
                MailToolRequest::Thread { thread_id, .. } => {
                    observed_mail(c, r, thread_id, true, q.connection_id())
                }
                _ => Ok(false),
            }
        }
        ExternalToolRequest::Calendar(q) => {
            if matches!(q, CalendarReadRequest::Status { .. }) {
                return Ok(false);
            }
            let id = crate::calendar::agent::request_connection(c, r, q)?;
            Ok(id.is_some_and(|id| p.calendar.iter().any(|p| p.id == id)))
        }
        _ => Ok(false),
    }
}
/// Trusted carrier scoped to a started read and exact owner grant, never serialized.
pub struct MissionReadCredentials {
    pub connection_id: String,
    pub primary_id: Option<String>,
    pub revision: i64,
    pub expires_at: i64,
    pub access: SecretString,
    pub refresh: SecretString,
}
fn connection_id(c: &Connection, r: &Run, q: &ExternalToolRequest) -> Result<(String, bool)> {
    match q {
        ExternalToolRequest::Mail(q) => Ok((q.connection_id().into(), false)),
        ExternalToolRequest::Calendar(q) => Ok((
            crate::calendar::agent::request_connection(c, r, q)?.ok_or(StoreError::Forbidden)?,
            true,
        )),
        _ => Err(StoreError::Forbidden),
    }
}
fn aad(r: &Run, id: &str, revision: i64, calendar: bool) -> Result<Vec<u8>> {
    if calendar {
        serde_json::to_vec(&(
            "moosuri/calendar/tokens/v1",
            &r.tenant,
            &r.account,
            r.epoch,
            id,
            revision,
        ))
    } else {
        serde_json::to_vec(&(
            "moosuri/gmail/v1",
            "tokens",
            &r.tenant,
            &r.account,
            id,
            revision,
        ))
    }
    .map_err(|_| StoreError::InvalidInput)
}
fn guard(
    c: &Connection,
    key: &[u8; 32],
    p: &ExternalToolPermit,
    app: &GoogleAppPin,
) -> Result<(Run, String, bool)> {
    let r = p.guard_started(c, key)?;
    if !is_mission(c, &r.tenant, &r.id)? || !allowed(c, &r, p.request())? {
        return Err(StoreError::Forbidden);
    }
    let (id, cal) = connection_id(c, &r, p.request())?;
    let profile = profile(&r).ok_or(StoreError::Forbidden)?;
    if profile.app_revision != app.revision {
        return Err(StoreError::Conflict);
    }
    if cal {
        crate::google_setup::guard_app(c, app)?;
        if !profile
            .calendar
            .iter()
            .any(|p| p.id == id && p.client_id == app.client_id)
        {
            return Err(StoreError::Conflict);
        }
    } else {
        crate::google_setup::guard_mail_app(c, &r.tenant, &id, app)?;
        if !profile
            .mail
            .iter()
            .any(|p| p.connection.id == id && p.client_id == app.client_id)
        {
            return Err(StoreError::Conflict);
        }
    }
    Ok((r, id, cal))
}
impl Store {
    pub async fn mission_read_credentials(
        &self,
        p: &ExternalToolPermit,
        app: GoogleAppPin,
    ) -> Result<MissionReadCredentials> {
        let p = p.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let result=(||{let(r,id,cal)=guard(&tx,&db.provider_key,&p,&app)?;let sql=if cal{"SELECT credential_revision,credential,expires_at,primary_id FROM calendar_connections WHERE tenant_id=?1 AND id=?2"}else{"SELECT credential_revision,credential,expires_at,NULL FROM mail_connections WHERE tenant_id=?1 AND id=?2"};let(revision,cipher,expires,primary):(i64,Vec<u8>,i64,Option<String>)=tx.query_row(sql,params![r.tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;let plain=providers::decrypt(&db.provider_key,&cipher,&aad(&r,&id,revision,cal)?)?;let(access,refresh)=if cal{let v:serde_json::Value=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;(v["access"].as_str().ok_or(StoreError::Corrupt)?.to_owned(),v["refresh"].as_str().ok_or(StoreError::Corrupt)?.to_owned())}else{serde_json::from_slice::<(String,String)>(&plain).map_err(|_|StoreError::Corrupt)?};Ok(MissionReadCredentials{connection_id:id,primary_id:primary,revision,expires_at:expires,access:SecretString::new(access).map_err(|_|StoreError::Corrupt)?,refresh:SecretString::new(refresh).map_err(|_|StoreError::Corrupt)?})})();tx.commit()?;result}).await
    }
    pub async fn refresh_mission_read(
        &self,
        p: &ExternalToolPermit,
        app: GoogleAppPin,
        old_revision: i64,
        tokens: TokenSet,
    ) -> Result<()> {
        let p = p.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let result=(||{let(r,id,cal)=guard(&tx,&db.provider_key,&p,&app)?;let table=if cal{"calendar_connections"}else{"mail_connections"};let(revision,cipher,scopes):(i64,Vec<u8>,String)=tx.query_row(&format!("SELECT credential_revision,credential,scopes FROM {table} WHERE tenant_id=?1 AND id=?2"),params![r.tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;if revision!=old_revision{return Err(StoreError::Conflict)}let old=providers::decrypt(&db.provider_key,&cipher,&aad(&r,&id,revision,cal)?)?;let old_refresh=Zeroizing::new(if cal{serde_json::from_slice::<serde_json::Value>(&old).map_err(|_|StoreError::Corrupt)?["refresh"].as_str().ok_or(StoreError::Corrupt)?.to_owned()}else{serde_json::from_slice::<(String,String)>(&old).map_err(|_|StoreError::Corrupt)?.1});let refresh=tokens.refresh_token.as_ref().map(|v|v.expose()).unwrap_or(&old_refresh);let current_scopes:Option<Vec<String>>=parse(&scopes)?;let old_scopes=current_scopes.unwrap_or_default();let scopes=tokens.scopes.unwrap_or_else(||old_scopes.clone());if scopes.iter().any(|s|!old_scopes.contains(s)){return Err(StoreError::Forbidden)}let required=if cal{moosuri_connectors::calendar::SCOPES.as_slice()}else{&[moosuri_connectors::gmail::READ_SCOPE]};if required.iter().any(|s|!scopes.iter().any(|v|v==s)){return Err(StoreError::Forbidden)}let plain=Zeroizing::new(if cal{serde_json::to_vec(&serde_json::json!({"access":tokens.access_token.expose(),"refresh":refresh}))}else{serde_json::to_vec(&(tokens.access_token.expose(),refresh))}.map_err(|_|StoreError::InvalidInput)?);let cipher=providers::encrypt(&db.provider_key,&plain,&aad(&r,&id,revision+1,cal)?)?;let expires=now()?.checked_add(i64::try_from(tokens.expires_in).map_err(|_|StoreError::InvalidInput)?).ok_or(StoreError::InvalidInput)?;let scopes=if cal{json(&Some(scopes))?}else{json(&scopes)?};if tx.execute(&format!("UPDATE {table} SET credential=?3,credential_revision=credential_revision+1,expires_at=?4,scopes=?5 WHERE tenant_id=?1 AND id=?2 AND credential_revision=?6 AND status='active'"),params![r.tenant,id,cipher,expires,scopes,revision])?!=1{return Err(StoreError::Conflict)}Ok(())})();tx.commit()?;result}).await
    }
}

#[cfg(test)]
mod tests;
/// Public task evidence is a human projection; opaque adapter handles stay private.
pub(crate) fn receipt(v: &serde_json::Value) -> Option<MissionReceipt> {
    fn text(v: &serde_json::Value, key: &str, max: usize) -> String {
        v.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(max)
            .collect()
    }
    fn mail(v: &serde_json::Value) -> String {
        let from = v
            .get("from")
            .and_then(|v| v.as_array())
            .map(|v| {
                v.iter()
                    .take(3)
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        format!(
            "{}\n{}\n{}",
            text(v, "subject", 240),
            from.chars().take(240).collect::<String>(),
            text(v, "text", 800)
        )
    }
    fn event(v: &serde_json::Value) -> String {
        let f = &v["fields"];
        format!(
            "{}\n{} ~ {}",
            text(f, "title", 240),
            text(f, "start", 64),
            text(f, "end", 64)
        )
    }
    let (label, detail) = if let Some(m) = v.get("mail") {
        ("Gmail 읽기", mail(m))
    } else if let Some(messages) = v.get("messages").and_then(|v| v.as_array()) {
        if messages.iter().any(|m| m.get("mail").is_some()) {
            (
                "Gmail 대화 읽기",
                messages
                    .iter()
                    .take(3)
                    .filter_map(|m| m.get("mail"))
                    .map(mail)
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            )
        } else {
            (
                "Gmail 검색",
                format!(
                    "이번 조회의 메일 검색 결과 {}개를 확인했어요.{}",
                    messages.len(),
                    if v.get("nextPageToken")
                        .or_else(|| v.get("next_page_token"))
                        .is_some_and(|v| !v.is_null())
                    {
                        " 일부 결과만 확인했어요."
                    } else {
                        ""
                    }
                ),
            )
        }
    } else if let Some(events) = v.get("events").and_then(|v| v.as_array()) {
        let detail = events
            .iter()
            .take(5)
            .map(|e| event(e.get("event").unwrap_or(e)))
            .collect::<Vec<_>>()
            .join("\n\n");
        (
            "Calendar 읽기",
            if detail.is_empty() {
                "이번 범위의 Calendar 조회를 확인했어요.".into()
            } else {
                detail
            },
        )
    } else if let Some(e) = v.get("event") {
        ("Calendar 일정 읽기", event(e))
    } else {
        return None;
    };
    Some(MissionReceipt {
        label: label.into(),
        detail: detail.chars().take(4096).collect(),
    })
}
/// Private projections retire independently of minimal dispatch/accounting receipts.
pub(crate) fn withdraw_private(c: &Connection, t: &str, id: &str) -> Result<()> {
    let enabled:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND id=?2 AND json_type(model,'$.agent_profile.mission_reads')='object')",params![t,id],|r|r.get(0))?;
    if !enabled {
        return Ok(());
    }
    c.execute(
        "UPDATE missions SET candidate=NULL,round_text=NULL WHERE tenant_id=?1 AND run_id=?2",
        params![t, id],
    )?;
    c.execute("UPDATE agent_steps SET checkpoint=NULL,state=CASE WHEN state='dispatched' THEN 'unknown' ELSE state END WHERE tenant_id=?1 AND run_id=?2",params![t,id])?;
    c.execute("UPDATE agent_tool_slots SET input='{\"tool\":\"invalid_arguments\"}',result='{\"kind\":\"error\",\"value\":\"unavailable\"}' WHERE tenant_id=?1 AND run_id=?2",params![t,id])?;
    c.execute(
        "DELETE FROM chat_output WHERE tenant_id=?1 AND run_id=?2",
        params![t, id],
    )?;
    c.execute(
        "DELETE FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2",
        params![t, id],
    )?;
    Ok(())
}
pub(crate) fn withdraw_provider(c: &Connection, provider: &str) -> Result<()> {
    let runs=c.prepare("SELECT tenant_id,id FROM chat_runs WHERE json_extract(model,'$.provider_id')=?1 AND json_type(model,'$.agent_profile.mission_reads')='object'")?.query_map([provider],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, r) in runs {
        withdraw_private(c, &t, &r)?;
    }
    Ok(())
}
impl Store {
    pub async fn mark_mission_read_needs_reconnect(
        &self,
        p: &ExternalToolPermit,
        app: GoogleAppPin,
        revision: i64,
    ) -> Result<()> {
        let p = p.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let result=(||{let(r,id,cal)=guard(&tx,&db.provider_key,&p,&app)?;let table=if cal{"calendar_connections"}else{"mail_connections"};if tx.execute(&format!("UPDATE {table} SET status='needs_reconnect',credential=NULL,epoch=epoch+1,credential_revision=credential_revision+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND credential_revision=?4 AND status='active'"),params![r.tenant,r.account,id,revision])?==1{if cal{crate::calendar::agent::invalidate(&tx,&r.tenant,&id)?}else{let runs=tx.prepare("SELECT run_id FROM mail_run_dependencies WHERE tenant_id=?1 AND connection_id=?2")?.query_map(params![r.tenant,id],|r|r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;crate::agent_execution::purge_runs(&tx,&r.tenant,runs)?;}}Ok(())})();tx.commit()?;result}).await
    }
}
