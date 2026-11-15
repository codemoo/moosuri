//! Bounded original-request action contracts. Existing domain ledgers own effects.
use super::*;
use moosuri_core::{MissionActionRequest, MissionEvidence, MissionLinkedAction};

pub(crate) fn requested(text: &str) -> (bool, bool) {
    let lower = text
        .to_lowercase()
        .replace("do not send", "")
        .replace("don't send", "")
        .replace("never send", "")
        .replace("보내지 말고", "")
        .replace("발송하지 말고", "");
    if [
        "do not",
        "don't",
        "never",
        "하지 마",
        "하지마",
        "등록하지",
        "추가하지",
        "옮기지",
    ]
    .iter()
    .any(|v| lower.contains(v))
    {
        return (false, false);
    }
    let compact = lower.split_whitespace().collect::<String>();
    let (mail, calendar) = moosuri_core::mission_read_domains(text);
    let words = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>();
    let imperative = |verbs: &[&str]| {
        words.iter().enumerate().any(|(i, w)| {
            verbs.contains(w)
                && (i == 0 || matches!(words[i - 1], "and" | "then" | "please" | "to"))
        })
    };
    let mail = mail
        && (imperative(&["prepare", "write", "create", "save"]) && words.contains(&"draft")
            || imperative(&["reply", "send"])
            || [
                "초안을만들어",
                "초안만들어",
                "초안을작성해",
                "초안작성해",
                "회신초안을",
                "답장초안을",
                "메일보내줘",
                "메일을보내줘",
            ]
            .iter()
            .any(|v| compact.contains(v)));
    let calendar = calendar && calendar_operation(&lower).is_some();
    (mail, calendar)
}

/// An imperative must own a Calendar object in its own clause. Other output
/// verbs (create a report/draft) never supply Calendar execution authority.
pub(super) fn calendar_operation(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    let mut operations = Vec::new();
    for clause in lower
        .split([',', ';', '.', '\n'])
        .flat_map(|s| s.split(" and "))
        .flat_map(|s| s.split(" then "))
    {
        let words = clause
            .split(|c: char| !c.is_alphabetic())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let offset = usize::from(words.first() == Some(&"please"));
        let Some(verb) = words.get(offset) else {
            continue;
        };
        let operation = match *verb {
            "create" => Some("create"),
            "move" | "reschedule" => Some("update"),
            _ => None,
        };
        if let Some(operation) = operation {
            let object = words
                .iter()
                .skip(offset + 1)
                .take_while(|w| !matches!(**w, "to" | "on" | "at" | "from" | "for"))
                .copied()
                .collect::<Vec<_>>();
            // Narrow supported object-head grammar. A later Calendar noun in
            // "a table of Calendar events" is never effect authority.
            let mut head = object.as_slice();
            if head
                .first()
                .is_some_and(|w| matches!(*w, "a" | "an" | "the" | "my"))
            {
                head = &head[1..];
            }
            if head.first() == Some(&"primary") {
                head = &head[1..];
            }
            if head.first() == Some(&"calendar") {
                head = &head[1..];
            }
            if head
                .first()
                .is_some_and(|w| matches!(*w, "event" | "meeting" | "appointment"))
                && (head.len() == 1
                    || head
                        .get(1)
                        .is_some_and(|w| matches!(*w, "named" | "called" | "titled")))
            {
                operations.push(operation);
            }
        }
        let compact = clause.split_whitespace().collect::<String>();
        for (needle, operation) in [
            ("일정등록해", "create"),
            ("일정을등록해", "create"),
            ("일정추가해", "create"),
            ("일정을추가해", "create"),
            ("일정옮겨", "update"),
            ("일정을옮겨", "update"),
            ("일정변경해", "update"),
            ("일정을변경해", "update"),
        ] {
            if compact.contains(needle) {
                operations.push(operation);
            }
        }
    }
    if operations.len() == 1 {
        operations.pop()
    } else {
        None
    }
}
/// A prepared identity cannot be reinterpreted by a new acceptance snapshot.
/// Explicitly start a new task to request different effects; keep old receipts.
pub(super) fn require_revisable(c: &Connection, r: &Run) -> Result<()> {
    let prepared:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND payload_digest IS NOT NULL)",params![r.tenant,r.id],|r|r.get(0))?;
    if prepared {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

pub(crate) fn admit(c: &Connection, r: &Run) -> Result<()> {
    let request = acceptance::current(c, &r.tenant, &r.id)?;
    let (mail, calendar) = requested(&format!("{}\n{}", request.task, request.constraints));
    for (kind, enabled) in [("mail", mail), ("calendar", calendar)] {
        if enabled {
            c.execute("INSERT OR IGNORE INTO mission_action_contracts(tenant_id,run_id,acceptance_revision,account_id,kind,action_id) VALUES(?1,?2,1,?3,?4,?5)",params![r.tenant,r.id,r.account,kind,Uuid::new_v4().to_string()])?;
        }
    }
    Ok(())
}
pub(crate) fn admit_revision(c: &Connection, r: &Run, revision: i64) -> Result<()> {
    if revision == 1 {
        return Ok(());
    }
    require_revisable(c, r)?;
    // Current acceptance can narrow the admitted kinds, never add effect rights.
    let q = acceptance::current(c, &r.tenant, &r.id)?;
    let (mail, calendar) = requested(&format!("{}\n{}", q.task, q.constraints));
    for (kind, enabled) in [("mail", mail), ("calendar", calendar)] {
        let original:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND kind=?3 AND acceptance_revision=1)",params![r.tenant,r.id,kind],|r|r.get(0))?;
        // A prepared/unknown action survives edits; a new acceptance cannot copy it.
        let prepared:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND kind=?3 AND payload_digest IS NOT NULL)",params![r.tenant,r.id,kind],|r|r.get(0))?;
        if enabled && original && !prepared {
            c.execute("INSERT OR IGNORE INTO mission_action_contracts(tenant_id,run_id,acceptance_revision,account_id,kind,action_id) VALUES(?1,?2,?3,?4,?5,?6)",params![r.tenant,r.id,revision,r.account,kind,Uuid::new_v4().to_string()])?;
        }
    }
    Ok(())
}
fn evidence(c: &Connection, r: &Run, refs: &[MissionEvidence]) -> Result<Vec<serde_json::Value>> {
    if refs.len() > 12 {
        return Err(StoreError::InvalidInput);
    }
    let mut out = vec![];
    for e in refs {
        if e.step < acceptance::floor(c, &r.tenant, &r.id)? {
            return Err(StoreError::Conflict);
        }
        let raw:Option<String>=c.query_row("SELECT t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed'",params![r.tenant,r.id,e.step,e.call_id],|r|r.get(0)).optional()?.flatten();
        let Some(raw) = raw else {
            return Err(StoreError::InvalidInput);
        };
        match parse::<KnowledgeToolResult>(&raw)? {
            KnowledgeToolResult::Source(s) => {
                out.push(serde_json::to_value(s).map_err(|_| StoreError::Corrupt)?)
            }
            KnowledgeToolResult::External(v) => out.push(v),
            KnowledgeToolResult::EvidenceCalculation { evidence, .. } => out.push(
                crate::learned_skills::draft::source_observation(c, r, e, &evidence)?,
            ),
            _ => return Err(StoreError::InvalidInput),
        }
    }
    Ok(out)
}
fn contract(
    c: &Connection,
    r: &Run,
    kind: &str,
    request: &MissionActionRequest,
) -> Result<(String, String)> {
    authority(c, r)?;
    let revision = acceptance::revision(c, &r.tenant, &r.id)?;
    let (id,old):(String,Option<String>)=c.query_row("SELECT action_id,payload_digest FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision=?3 AND account_id=?4 AND kind=?5",params![r.tenant,r.id,revision,r.account,kind],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::Forbidden)?;
    let digest = format!("{:x}", Sha256::digest(json(request)?.as_bytes()));
    if old.as_ref().is_some_and(|v| v != &digest) {
        return Err(StoreError::Conflict);
    }
    Ok((id, digest))
}
fn save_link(
    c: &Connection,
    r: &Run,
    id: &str,
    digest: &str,
    revision: i64,
    action_digest: &str,
    refs: &[MissionEvidence],
) -> Result<()> {
    c.execute("UPDATE mission_action_contracts SET payload_digest=?4,action_revision=?5,action_digest=?6,evidence=?7 WHERE tenant_id=?1 AND run_id=?2 AND action_id=?3",params![r.tenant,r.id,id,digest,revision,action_digest,json(&refs)?])?;
    Ok(())
}
fn mail(c: &Connection, r: &Run, request: &MissionActionRequest) -> Result<KnowledgeToolResult> {
    let MissionActionRequest::PrepareMail {
        intent_quote,
        input,
        evidence: refs,
    } = request
    else {
        return Err(StoreError::InvalidInput);
    };
    let (id, digest) = contract(c, r, "mail", request)?;
    let current = acceptance::current(c, &r.tenant, &r.id)?;
    let source = format!("{}\n{}", current.task, current.constraints);
    if intent_quote.is_empty() || !source.contains(intent_quote) || !requested(intent_quote).0 {
        return Err(StoreError::Forbidden);
    }
    let owner_values = super::inputs::values(c, r)?;
    if let Some(to) = owner_values
        .iter()
        .find(|w| w.field == moosuri_core::MissionInputField::MailTo)
        .and_then(|w| w.value.as_ref())
        && (input.content.to != vec![to.clone()]
            || !input.content.cc.is_empty()
            || !input.content.bcc.is_empty())
    {
        return Err(StoreError::InvalidInput);
    }
    let observations = evidence(c, r, refs)?;
    let pin = r
        .tool_profile
        .as_ref()
        .and_then(|p| p.mission_reads.as_ref())
        .and_then(|p| {
            p.mail
                .iter()
                .find(|p| p.connection.id == input.connection_id)
        })
        .map(|p| &p.connection)
        .ok_or(StoreError::Forbidden)?;
    let connection = crate::mail::connection_owned(c, &r.tenant, &r.account, &pin.id)?;
    if connection.status != "active"
        || connection.epoch != pin.epoch
        || connection.email != input.content.from
        || connection.native_send
    {
        return Err(StoreError::Conflict);
    }
    // Each derived recipient must be an actual sender in exactly one cited mail.
    // CC/BCC require literal current-owner selection; no silent reply-all.
    for address in input
        .content
        .to
        .iter()
        .chain(&input.content.cc)
        .chain(&input.content.bcc)
    {
        if source.contains(address)
            || input.content.to.len() == 1
                && input.content.cc.is_empty()
                && input.content.bcc.is_empty()
                && owner_values.iter().any(|w| {
                    w.field == moosuri_core::MissionInputField::MailTo
                        && w.value.as_ref() == Some(address)
                })
        {
            continue;
        }
        let senders = observations
            .iter()
            .filter_map(|v| v.get("mail"))
            .filter_map(|v| v.get("from"))
            .filter_map(|v| v.as_array())
            .flatten()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();
        if input.content.to.len() != 1
            || !input.content.cc.is_empty()
            || !input.content.bcc.is_empty()
            || senders.len() != 1
            || senders[0] != address
        {
            return Err(StoreError::InvalidInput);
        }
    }
    if let Some(reply) = &input.content.reply
        && (!reply.references.is_empty()
            || !observations.iter().any(|v| {
                v["thread_id"].as_str() == Some(&reply.thread_id)
                    && v["mail"]["message_id"].as_str() == Some(&reply.in_reply_to)
                    && v["mail"]["from"].as_array().is_some_and(|senders| {
                        senders.len() == 1
                            && senders[0].as_str() == input.content.to.first().map(String::as_str)
                    })
            }))
    {
        return Err(StoreError::InvalidInput);
    }
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM mail_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3)",
        params![r.tenant, r.account, id],
        |r| r.get(0),
    )?;
    if !exists {
        let mut input = (**input).clone();
        input.origin_thread_id = Some(r.thread.clone());
        crate::mail::draft_owned(
            c,
            &r.tenant,
            &r.account,
            r.epoch,
            input,
            Some(&r.id),
            None,
            Some(&id),
        )?;
        c.execute(
            "INSERT OR IGNORE INTO mail_run_dependencies VALUES(?1,?2,?3,?4)",
            params![r.tenant, r.id, pin.id, pin.epoch],
        )?;
        // Draft availability outlives model inference; later approval is separate.
        c.execute(
            "UPDATE mail_actions SET expires_at=?3 WHERE tenant_id=?1 AND id=?2",
            params![r.tenant, id, now()? + 7 * 86400],
        )?;
        let (revision, approval_digest): (i64, String) = c.query_row(
            "SELECT revision,digest FROM mail_actions WHERE tenant_id=?1 AND id=?2",
            params![r.tenant, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        save_link(c, r, &id, &digest, revision, &approval_digest, refs)?;
    }
    let link = view(c, &r.tenant, &r.id)?
        .into_iter()
        .find(|v| v.id == id)
        .ok_or(StoreError::Conflict)?;
    Ok(KnowledgeToolResult::External(
        serde_json::json!({"action_id":link.id,"revision":link.revision,"digest":link.digest,"state":link.state,"local_draft":true,"review_url":link.url,"requires_user_approval":true,"sent":false}),
    ))
}
pub(crate) fn local(
    c: &Connection,
    r: &Run,
    request: &MissionActionRequest,
) -> Result<KnowledgeToolResult> {
    let result = match request {
        MissionActionRequest::PrepareMail { .. } => mail(c, r, request),
        _ => Err(StoreError::Forbidden),
    };
    match result {
        Ok(v) => Ok(v),
        Err(
            StoreError::InvalidInput
            | StoreError::Forbidden
            | StoreError::NotFound
            | StoreError::Conflict,
        ) => Ok(KnowledgeToolResult::Error(
            crate::ToolReadError::InvalidInput,
        )),
        Err(e) => Err(e),
    }
}
pub(crate) fn settlement_current(
    c: &Connection,
    tenant: &str,
    account: &str,
    epoch: i64,
    id: &str,
) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM mission_action_contracts l JOIN missions m ON m.tenant_id=l.tenant_id AND m.run_id=l.run_id JOIN accounts a ON a.tenant_id=m.tenant_id AND a.id=m.account_id JOIN agent_execution e ON e.tenant_id=m.tenant_id AND e.run_id=m.run_id WHERE l.tenant_id=?1 AND l.account_id=?2 AND l.action_id=?3 AND a.auth_epoch=?4 AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>unixepoch()) AND e.invalidated=0 AND m.state NOT IN('deleted','quarantined','cancelled'))",params![tenant,account,id,epoch],|r|r.get(0))?)
}
pub(crate) fn view(c: &Connection, tenant: &str, run: &str) -> Result<Vec<MissionLinkedAction>> {
    let valid:bool=c.query_row("SELECT NOT EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2 AND invalidated=1)",params![tenant,run],|r|r.get(0))?;
    if !valid {
        return Ok(vec![]);
    }
    let rows=c.prepare("SELECT kind,action_id,coalesce(action_revision,0),coalesce(action_digest,'') FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND payload_digest IS NOT NULL ORDER BY acceptance_revision,kind")?.query_map(params![tenant,run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|(kind,id,revision,digest)|{
        let (state,current,provider_receipt)=if kind=="mail" {
            c.query_row("SELECT state,revision=?3 AND digest=?4,coalesce(provider_message,(SELECT receipt FROM mail_provider_saves s WHERE s.tenant_id=mail_actions.tenant_id AND s.action_id=mail_actions.id AND s.action_revision=mail_actions.revision AND s.state='saved' ORDER BY created_at LIMIT 1)) FROM mail_actions WHERE tenant_id=?1 AND id=?2",params![tenant,id,revision,digest],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?
        } else {
            c.query_row("SELECT state,digest=?3,receipt FROM calendar_actions WHERE tenant_id=?1 AND id=?2",params![tenant,id,digest],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.unwrap_or(("prepared".into(),true,None))
        };
        Ok(MissionLinkedAction{url:if kind=="mail"{format!("/settings/mail#action-{id}")}else{"/settings/calendar".into()},kind,id,revision,digest,state,current,provider_receipt})
    }).collect()
}

/// Guarded carrier for one saved tool slot, never an authenticated session.
#[derive(Clone)]
pub struct MissionActionPermit {
    lease: crate::StepLease,
    call_id: String,
    request: MissionActionRequest,
}
pub enum MissionActionStart {
    Complete(KnowledgeToolResult),
    Dispatch(MissionActionPermit),
}
impl MissionActionPermit {
    pub fn request(&self) -> &MissionActionRequest {
        &self.request
    }
}
impl Store {
    pub async fn prepare_mission_action(
        &self,
        lease: &crate::StepLease,
        call_id: String,
    ) -> Result<MissionActionStart> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=crate::agent_execution::guard_step(&tx,&db.provider_key,&lease)?;
            let(raw,saved):(String,Option<String>)=tx.query_row("SELECT input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,lease.step,call_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if let Some(saved)=saved{return Ok(MissionActionStart::Complete(parse(&saved)?))}
            let KnowledgeToolInput::MissionAction{request}=parse(&raw)? else{return Err(StoreError::InvalidInput)};
            let counted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4)",params![r.tenant,r.id,lease.step,call_id],|r|r.get(0))?;
            if !counted {
                let used:i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|r.get(0))?;
                if used>=12{return Err(StoreError::BudgetExhausted)}
                // Existing slot accounting, not provider dispatch authority.
                tx.execute("INSERT INTO external_tool_dispatches VALUES(?1,?2,?3,?4,?5,'completed')",params![r.tenant,r.id,lease.step,call_id,Sha256::digest(random_token()?.as_bytes()).to_vec()])?;
                tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;
            }
            let validation=(|| {match &request { MissionActionRequest::SaveProviderDraft{..}=>provider_contract(&tx,&r,&request), _=>{let(id,digest)=contract(&tx,&r,"calendar",&request)?;calendar_context(&tx,&r,&request)?;Ok((id,digest))} }})();
            let(id,digest)=match validation {
                Ok(v)=>v,
                Err(StoreError::InvalidInput|StoreError::Forbidden|StoreError::Conflict|StoreError::NotFound)=>{let result=KnowledgeToolResult::Error(crate::ToolReadError::InvalidInput);tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,lease.step,call_id,json(&result)?])?;tx.commit()?;return Ok(MissionActionStart::Complete(result))},
                Err(e)=>return Err(e),
            };
            if let Some(result)=if matches!(request,MissionActionRequest::Calendar{..}) {calendar_recorded(&tx,&r,&id)?} else {None}{tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,lease.step,call_id,json(&result)?])?;tx.commit()?;return Ok(MissionActionStart::Complete(result))}
            let old:Option<String>=tx.query_row("SELECT payload_digest FROM mission_action_contracts WHERE tenant_id=?1 AND action_id=?2",params![r.tenant,id],|r|r.get(0))?;
            if old.is_none()&&matches!(request,MissionActionRequest::Calendar{..}){save_link(&tx,&r,&id,&digest,1,"",&[])?;}
            tx.commit()?;Ok(MissionActionStart::Dispatch(MissionActionPermit{lease,call_id,request}))
        }).await
    }
    pub async fn complete_mission_action(
        &self,
        p: &MissionActionPermit,
        result: KnowledgeToolResult,
    ) -> Result<KnowledgeToolResult> {
        let p = p.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let r=crate::agent_execution::guard_step(&tx,&db.provider_key,&p.lease)?;match &p.request{MissionActionRequest::SaveProviderDraft{..}=>{provider_contract(&tx,&r,&p.request)?;},_=>{contract(&tx,&r,"calendar",&p.request)?;}}tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND result IS NULL",params![r.tenant,r.id,p.lease.step,p.call_id,json(&result)?])?;tx.commit()?;Ok(result)}).await
    }
}
fn calendar_recorded(c: &Connection, r: &Run, id: &str) -> Result<Option<KnowledgeToolResult>> {
    let row:Option<(String,String,Option<String>)>=c.query_row("SELECT state,event_id,receipt FROM calendar_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![r.tenant,r.account,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    Ok(row.map(|(state,event_id,receipt)|KnowledgeToolResult::External(serde_json::json!({"action_id":id,"state":state,"event_id":event_id,"receipt":receipt,"sent":false}))))
}

pub struct MissionCalendarCredentials {
    pub connection: moosuri_core::CalendarConnection,
    pub access: moosuri_connectors::gmail::SecretString,
    pub refresh: moosuri_connectors::gmail::SecretString,
    pub expires_at: i64,
    pub app: crate::GoogleAppPin,
}
fn calendar_context(
    c: &Connection,
    r: &Run,
    q: &MissionActionRequest,
) -> Result<(
    moosuri_core::CalendarRunProfile,
    moosuri_core::CalendarAgentPin,
    Option<moosuri_core::CalendarEvent>,
    String,
)> {
    let MissionActionRequest::Calendar {
        decision,
        evidence: refs,
    } = q
    else {
        return Err(StoreError::InvalidInput);
    };
    let input = acceptance::current(c, &r.tenant, &r.id)?;
    let owner_text = format!("{}\n{}", input.task, input.constraints);
    let intent = decision
        .intent_quote
        .as_deref()
        .ok_or(StoreError::InvalidInput)?;
    if intent.is_empty()
        || !owner_text.contains(intent)
        || !requested(intent).1
        || decision.decision != "command"
        || !matches!(decision.operation.as_deref(), Some("create" | "update"))
    {
        return Err(StoreError::Forbidden);
    }
    if decision.operation.as_deref() == Some("update")
        && (decision.move_date.is_none()
            || decision.start.is_some()
            || decision.end.is_some()
            || decision.duration_minutes.is_some()
            || decision.title_quote.is_some()
            || decision.last_date.is_some()
            || decision.all_day_quote.is_some())
    {
        return Err(StoreError::InvalidInput);
    }
    let recurring:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2)",params![r.tenant,r.id],|r|r.get(0))?;
    if recurring {
        return Err(StoreError::Forbidden);
    }
    let original: MissionCreate = parse(&c.query_row(
        "SELECT payload FROM missions WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
        |r| r.get::<_, String>(0),
    )?)?;
    let original_text = format!("{}\n{}", original.task, original.constraints);
    let proposed = decision.operation.as_deref();
    if calendar_operation(&original_text) != proposed
        || calendar_operation(&owner_text) != proposed
        || calendar_operation(intent) != proposed
    {
        return Err(StoreError::Forbidden);
    }
    let observations = evidence(c, r, refs)?;
    // Source text and typed answers supply values, never operation authority.
    let mut base = owner_text;
    for observation in &observations {
        base.push('\n');
        base.push_str(&json(observation)?);
    }
    let answers = super::inputs::values(c, r)?;
    for answer in &answers {
        use moosuri_core::MissionInputField as F;
        let Some(v) = answer.value.as_deref() else {
            continue;
        };
        let valid = match answer.field {
            F::MailTo | F::Context | F::SourceChoice | F::CalendarTarget => true,
            F::CalendarDate => {
                if proposed == Some("create") {
                    decision.date.as_deref() == Some(v) && decision.date_quote.as_deref() == Some(v)
                } else {
                    decision.move_date.as_deref() == Some(v)
                        && decision.move_date_quote.as_deref() == Some(v)
                }
            }
            F::CalendarStart => {
                decision.start.as_deref() == Some(v) && decision.start_quote.as_deref() == Some(v)
            }
            F::CalendarDuration => {
                decision.duration_quote.as_deref() == Some(v)
                    && decision
                        .duration_minutes
                        .map(|n| format!("{n} minutes"))
                        .as_deref()
                        == Some(v)
            }
        };
        if !valid {
            return Err(StoreError::InvalidInput);
        }
    }
    // An answer for date/start/duration cannot be borrowed as title, target,
    // account, all-day mode, end time, or any other effect parameter.
    for quote in [
        &decision.connection_quote,
        &decision.title_quote,
        &decision.target_quote,
        &decision.all_day_quote,
        &decision.last_date_quote,
        &decision.end_quote,
    ]
    .into_iter()
    .flatten()
    {
        if !base.contains(quote) {
            return Err(StoreError::InvalidInput);
        }
    }
    let mut values = base;
    for answer in answers {
        if matches!(
            answer.field,
            moosuri_core::MissionInputField::CalendarDate
                | moosuri_core::MissionInputField::CalendarStart
                | moosuri_core::MissionInputField::CalendarDuration
        ) && let Some(v) = answer.value
        {
            values.push('\n');
            values.push_str(&v);
        }
    }
    crate::calendar::agent::validate_decision(&values, decision)?;
    let p = crate::calendar::agent::profile(c, &r.tenant, &r.id)?.ok_or(StoreError::Forbidden)?;
    let pin = crate::calendar::agent::selected(&p, decision)?.clone();
    let target = if decision.operation.as_deref() == Some("update") {
        let mut events = vec![];
        for v in &observations {
            if v.get("partial").and_then(|v| v.as_bool()) == Some(true)
                || v.get("next_page_token").is_some_and(|v| !v.is_null())
            {
                return Err(StoreError::InvalidInput);
            }
            if let Some(e) = v.get("event") {
                events.push(e.clone())
            }
            if let Some(list) = v.get("events").and_then(|v| v.as_array()) {
                events.extend(list.iter().map(|v| v.get("event").unwrap_or(v).clone()))
            }
        }
        let mut unique = std::collections::BTreeMap::new();
        for event in events {
            let e: moosuri_core::CalendarEvent =
                serde_json::from_value(event).map_err(|_| StoreError::InvalidInput)?;
            if decision.target_quote.as_ref() == Some(&e.fields.title) {
                unique.insert(e.id.clone(), e);
            }
        }
        // A target answer selects an existing observation; it cannot supply a
        // new title, account or operation. Preserve the chosen exact ETag.
        if let Some(selection) = super::inputs::values(c, r)?
            .into_iter()
            .find(|w| w.field == moosuri_core::MissionInputField::CalendarTarget)
        {
            let selected = selection
                .candidates
                .iter()
                .find(|c| Some(&c.id) == selection.value.as_ref())
                .ok_or(StoreError::Conflict)?;
            unique.retain(|id, event| id == &selected.id && event.etag == selected.revision);
        }
        if unique.len() != 1 {
            return Err(StoreError::InvalidInput);
        }
        let e = unique
            .into_values()
            .next()
            .ok_or(StoreError::InvalidInput)?;
        let observed:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2 AND connection_id=?3 AND event_id=?4 AND etag=?5 AND step>=?6)",params![r.tenant,r.id,pin.id,e.id,e.etag,acceptance::floor(c,&r.tenant,&r.id)?],|r|r.get(0))?;
        if !observed || !e.writable {
            return Err(StoreError::Conflict);
        }
        Some(e)
    } else {
        None
    };
    Ok((p, pin, target, values))
}
impl Store {
    pub async fn mission_calendar_credentials(
        &self,
        p: &MissionActionPermit,
        app: crate::GoogleAppPin,
    ) -> Result<MissionCalendarCredentials> {
        let p = p.clone();
        self.call(move|db|{
            let r=crate::agent_execution::guard_step(&db.conn,&db.provider_key,&p.lease)?;
            contract(&db.conn,&r,"calendar",&p.request)?;
            let(_,pin,_,_)=calendar_context(&db.conn,&r,&p.request)?;
            crate::google_setup::guard_app(&db.conn,&app)?;
            if pin.client_id!=app.client_id||pin.app_revision!=app.revision{return Err(StoreError::Conflict)}
            let connection=crate::calendar::connection_owned(&db.conn,&r.tenant,&r.account,r.epoch,&pin.id)?;
            if connection.status!="active"||connection.epoch!=pin.epoch{return Err(StoreError::Conflict)}
            let(cipher,expires):(Vec<u8>,i64)=db.conn.query_row("SELECT credential,expires_at FROM calendar_connections WHERE tenant_id=?1 AND id=?2 AND client_id=?3",params![r.tenant,pin.id,pin.client_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let aad=json(&("moosuri/calendar/tokens/v1",&r.tenant,&r.account,r.epoch,&pin.id,connection.revision))?;
            let plain=providers::decrypt(&db.provider_key,&cipher,aad.as_bytes())?;
            let v:serde_json::Value=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;
            Ok(MissionCalendarCredentials{connection,access:moosuri_connectors::gmail::SecretString::new(v["access"].as_str().ok_or(StoreError::Corrupt)?.into()).map_err(|_|StoreError::Corrupt)?,refresh:moosuri_connectors::gmail::SecretString::new(v["refresh"].as_str().ok_or(StoreError::Corrupt)?.into()).map_err(|_|StoreError::Corrupt)?,expires_at:expires,app})
        }).await
    }
    pub async fn refresh_mission_calendar(
        &self,
        p: &MissionActionPermit,
        b: &MissionCalendarCredentials,
        tokens: moosuri_connectors::gmail::TokenSet,
    ) -> Result<()> {
        let p = p.clone();
        let old = b.connection.clone();
        let app = b.app.clone();
        let refresh = b.refresh.expose().to_string();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=crate::agent_execution::guard_step(&tx,&db.provider_key,&p.lease)?;contract(&tx,&r,"calendar",&p.request)?;
            crate::google_setup::guard_app(&tx,&app)?;
            let(_,pin,_,_)=calendar_context(&tx,&r,&p.request)?;
            let current=crate::calendar::connection_owned(&tx,&r.tenant,&r.account,r.epoch,&pin.id)?;
            if current.status!="active"||current.epoch!=old.epoch||current.revision!=old.revision||pin.client_id!=app.client_id||pin.app_revision!=app.revision{return Err(StoreError::Conflict)}
            if tokens.scopes.as_ref().is_some_and(|s|s.iter().any(|v|!moosuri_connectors::calendar::SCOPES.contains(&v.as_str()))||moosuri_connectors::calendar::SCOPES.iter().any(|v|!s.iter().any(|s|s==v))){return Err(StoreError::Forbidden)}
            let plain=Zeroizing::new(json(&serde_json::json!({"access":tokens.access_token.expose(),"refresh":tokens.refresh_token.as_ref().map(|v|v.expose()).unwrap_or(&refresh)}))?);
            let aad=json(&("moosuri/calendar/tokens/v1",&r.tenant,&r.account,r.epoch,&pin.id,old.revision+1))?;
            let cipher=providers::encrypt(&db.provider_key,plain.as_bytes(),aad.as_bytes())?;
            let expires=now()?.checked_add(i64::try_from(tokens.expires_in).map_err(|_|StoreError::InvalidInput)?).ok_or(StoreError::InvalidInput)?;
            tx.execute("UPDATE calendar_connections SET credential=?3,credential_revision=credential_revision+1,expires_at=?4 WHERE tenant_id=?1 AND id=?2",params![r.tenant,pin.id,cipher,expires])?;
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn mission_calendar_target(
        &self,
        p: &MissionActionPermit,
    ) -> Result<Option<moosuri_core::CalendarEvent>> {
        let p = p.clone();
        self.call(move |db| {
            let r = crate::agent_execution::guard_step(&db.conn, &db.provider_key, &p.lease)?;
            contract(&db.conn, &r, "calendar", &p.request)?;
            Ok(calendar_context(&db.conn, &r, &p.request)?.2)
        })
        .await
    }
    pub async fn admit_mission_calendar_change(
        &self,
        p: &MissionActionPermit,
        b: &MissionCalendarCredentials,
        primary: moosuri_core::CalendarInfo,
        prior: Option<moosuri_core::CalendarEvent>,
    ) -> Result<crate::CalendarAdmission> {
        let p = p.clone();
        let connection = b.connection.clone();
        let app = b.app.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r = crate::agent_execution::guard_step(&tx, &db.provider_key, &p.lease)?;
            let (id, digest) = contract(&tx, &r, "calendar", &p.request)?;
            let (profile, pin, target, text) = calendar_context(&tx, &r, &p.request)?;
            if pin.id != connection.id
                || pin.epoch != connection.epoch
                || pin.client_id != app.client_id
                || pin.app_revision != app.revision
            {
                return Err(StoreError::Conflict);
            }
            let MissionActionRequest::Calendar {
                decision,
                evidence: refs,
            } = &p.request
            else {
                return Err(StoreError::InvalidInput);
            };
            if target.as_ref().map(|e| (&e.id, &e.etag)) != prior.as_ref().map(|e| (&e.id, &e.etag))
            {
                return Err(StoreError::Conflict);
            }
            let fields = crate::calendar::agent::fields(&profile, decision, prior.as_ref(), &text)?;
            let mutation = if let Some(old) = &prior {
                moosuri_core::CalendarMutation::Update {
                    event_id: old.id.clone(),
                    etag: old.etag.clone(),
                    fields,
                }
            } else {
                moosuri_core::CalendarMutation::Create { fields }
            };
            let change = moosuri_core::CalendarChange {
                request_id: id.clone(),
                connection_id: pin.id,
                expected_epoch: connection.epoch,
                expected_revision: connection.revision,
                mutation,
            };
            let action_digest = format!("{:x}", Sha256::digest(json(&change)?.as_bytes()));
            let admission = crate::calendar::admit_change_owned(
                &tx,
                crate::calendar::CalendarOwner {
                    tenant: r.tenant.clone(),
                    account: r.account.clone(),
                    epoch: r.epoch,
                    session_digest: vec![],
                },
                None,
                &connection,
                &app,
                change,
                primary,
                prior,
            )?;
            save_link(&tx, &r, &id, &digest, 1, &action_digest, refs)?;
            tx.commit()?;
            Ok(admission)
        })
        .await
    }
}
pub(crate) fn verification(c: &Connection, r: &Run) -> Result<(Vec<String>, bool)> {
    let revision = acceptance::revision(c, &r.tenant, &r.id)?;
    let required=c.prepare("SELECT kind,action_id,payload_digest FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision=?3")?.query_map(params![r.tenant,r.id,revision],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let links = view(c, &r.tenant, &r.id)?;
    let prior_prepared:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision!=?3 AND payload_digest IS NOT NULL)",params![r.tenant,r.id,revision],|r|r.get(0))?;
    let mut report = vec![];
    let mut unknown = false;
    if prior_prepared {
        report.push(
            "Prepared action belongs to a different acceptance; explicit new task required".into(),
        );
    }
    let request = acceptance::current(c, &r.tenant, &r.id)?;
    let needs_provider = provider_requested(&format!("{}\n{}", request.task, request.constraints));
    for (kind, id, digest) in required {
        let link = links.iter().find(|l| l.id == id);
        if digest.is_none() || link.is_none() {
            report.push(format!("Requested {kind} action has no current receipt"));
            continue;
        }
        let link = link.ok_or(StoreError::Corrupt)?;
        if !link.current {
            report.push(format!("Requested {kind} action revision changed"));
            continue;
        }
        if kind == "mail" && needs_provider {
            let(saved,unresolved):(bool,bool)=c.query_row("SELECT EXISTS(SELECT 1 FROM mail_provider_saves WHERE tenant_id=?1 AND action_id=?2 AND action_revision=?3 AND state='saved'),EXISTS(SELECT 1 FROM mail_provider_saves WHERE tenant_id=?1 AND action_id=?2 AND action_revision=?3 AND state IN('started','outcome_unknown','quarantined'))",params![r.tenant,id,link.revision],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if !saved || unresolved {
                report.push("Requested Gmail provider draft has no confirmed receipt".into());
                unknown |= unresolved;
            }
        }
        if kind == "calendar" && link.state != "applied" {
            unknown = matches!(
                link.state.as_str(),
                "outcome_unknown" | "dispatch_committed"
            );
            report.push(format!("Requested Calendar action is {}", link.state));
        }
        if kind == "mail"
            && !matches!(
                link.state.as_str(),
                "pending_approval" | "approved" | "sent"
            )
        {
            report.push(format!("Requested mail draft is {}", link.state));
        }
    }
    Ok((report, unknown))
}

pub(crate) fn provider_requested(text: &str) -> bool {
    let lower = text.to_lowercase();
    let compact = lower.split_whitespace().collect::<String>();
    requested(text).0
        && (lower.contains("gmail") && lower.contains("draft") && lower.contains("save")
            || ["gmail초안", "gmail에초안", "gmail에새초안"]
                .iter()
                .any(|v| compact.contains(v))
                && compact.contains("저장"))
}
fn provider_contract(
    c: &Connection,
    r: &Run,
    q: &MissionActionRequest,
) -> Result<(String, String)> {
    authority(c, r)?;
    let MissionActionRequest::SaveProviderDraft {
        intent_quote,
        action_id,
        expected,
    } = q
    else {
        return Err(StoreError::InvalidInput);
    };
    let input = acceptance::current(c, &r.tenant, &r.id)?;
    let text = format!("{}\n{}", input.task, input.constraints);
    let original: MissionCreate = parse(&c.query_row(
        "SELECT payload FROM missions WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
        |r| r.get::<_, String>(0),
    )?)?;
    if !provider_requested(&format!("{}\n{}", original.task, original.constraints)) {
        return Err(StoreError::Forbidden);
    }
    if intent_quote.is_empty() || !text.contains(intent_quote) || !provider_requested(intent_quote)
    {
        return Err(StoreError::Forbidden);
    }
    let digest:Option<String>=c.query_row("SELECT payload_digest FROM mission_action_contracts WHERE tenant_id=?1 AND account_id=?2 AND run_id=?3 AND acceptance_revision=?4 AND kind='mail' AND action_id=?5 AND action_revision=?6 AND action_digest=?7",params![r.tenant,r.account,r.id,acceptance::revision(c,&r.tenant,&r.id)?,action_id,expected.revision,expected.digest],|r|r.get(0)).optional()?.flatten();
    let digest = digest.ok_or(StoreError::Forbidden)?;
    Ok((action_id.clone(), digest))
}
pub struct MissionProviderCredentials {
    pub connection_id: String,
    pub epoch: i64,
    pub revision: i64,
    pub access: moosuri_connectors::gmail::SecretString,
    pub refresh: moosuri_connectors::gmail::SecretString,
    pub expires_at: i64,
    pub app: crate::GoogleAppPin,
}
fn provider_owner(r: &Run) -> crate::mail::provider_saves::ProviderDraftOwner {
    crate::mail::provider_saves::ProviderDraftOwner {
        tenant: r.tenant.clone(),
        account: r.account.clone(),
        epoch: r.epoch,
    }
}
fn provider_input(q: &MissionActionRequest) -> Result<moosuri_core::GmailDraftSave> {
    let MissionActionRequest::SaveProviderDraft { expected, .. } = q else {
        return Err(StoreError::InvalidInput);
    };
    Ok(moosuri_core::GmailDraftSave {
        expected: expected.clone(),
        draft_id: None,
        expected_provider_digest: None,
    })
}
impl Store {
    pub async fn mission_provider_credentials(
        &self,
        p: &MissionActionPermit,
        app: crate::GoogleAppPin,
    ) -> Result<MissionProviderCredentials> {
        let p = p.clone();
        self.call(move|db|{
            let r=crate::agent_execution::guard_step(&db.conn,&db.provider_key,&p.lease)?;
            let(id,_)=provider_contract(&db.conn,&r,&p.request)?;
            let connection_id:String=db.conn.query_row("SELECT connection_id FROM mail_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![r.tenant,r.account,id],|r|r.get(0))?;
            let pin=r.tool_profile.as_ref().and_then(|p|p.mission_reads.as_ref()).and_then(|p|p.mail.iter().find(|p|p.connection.id==connection_id)).ok_or(StoreError::Forbidden)?;
            crate::google_setup::guard_mail_app(&db.conn,&r.tenant,&connection_id,&app)?;
            if pin.client_id!=app.client_id||r.tool_profile.as_ref().and_then(|p|p.mission_reads.as_ref()).and_then(|p|p.app_revision)!=app.revision{return Err(StoreError::Conflict)}
            let connection=crate::mail::connection_owned(&db.conn,&r.tenant,&r.account,&connection_id)?;
            if connection.status!="active"||connection.epoch!=pin.connection.epoch||!connection.compose_enabled||connection.native_send{return Err(StoreError::Conflict)}
            let(revision,cipher,expires):(i64,Vec<u8>,i64)=db.conn.query_row("SELECT credential_revision,credential,expires_at FROM mail_connections WHERE tenant_id=?1 AND id=?2",params![r.tenant,connection_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            let aad=json(&("moosuri/gmail/v1","tokens",&r.tenant,&r.account,&connection_id,revision))?;
            let plain=providers::decrypt(&db.provider_key,&cipher,aad.as_bytes())?;
            let(access,refresh):(String,String)=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;
            Ok(MissionProviderCredentials{connection_id,epoch:connection.epoch,revision,access:moosuri_connectors::gmail::SecretString::new(access).map_err(|_|StoreError::Corrupt)?,refresh:moosuri_connectors::gmail::SecretString::new(refresh).map_err(|_|StoreError::Corrupt)?,expires_at:expires,app})
        }).await
    }
    pub async fn refresh_mission_provider(
        &self,
        p: &MissionActionPermit,
        b: &MissionProviderCredentials,
        tokens: moosuri_connectors::gmail::TokenSet,
    ) -> Result<()> {
        let p = p.clone();
        let id = b.connection_id.clone();
        let revision = b.revision;
        let epoch = b.epoch;
        let app = b.app.clone();
        let refresh = Zeroizing::new(b.refresh.expose().to_string());
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=crate::agent_execution::guard_step(&tx,&db.provider_key,&p.lease)?;provider_contract(&tx,&r,&p.request)?;
            crate::google_setup::guard_mail_app(&tx,&r.tenant,&id,&app)?;
            let connection=crate::mail::connection_owned(&tx,&r.tenant,&r.account,&id)?;
            let current:i64=tx.query_row("SELECT credential_revision FROM mail_connections WHERE tenant_id=?1 AND id=?2",params![r.tenant,id],|r|r.get(0))?;
            if connection.status!="active"||connection.epoch!=epoch||current!=revision||!connection.compose_enabled{return Err(StoreError::Conflict)}
            let old:Vec<String>=parse(&tx.query_row("SELECT scopes FROM mail_connections WHERE tenant_id=?1 AND id=?2",params![r.tenant,id],|r|r.get::<_,String>(0))?)?;
            if tokens.scopes.as_ref().is_some_and(|s|s.iter().any(|v|!old.contains(v))||[moosuri_connectors::gmail::READ_SCOPE,moosuri_connectors::gmail::COMPOSE_SCOPE].iter().any(|v|!s.iter().any(|s|s==v))){return Err(StoreError::Forbidden)}
            let plain=Zeroizing::new(json(&(tokens.access_token.expose(),tokens.refresh_token.as_ref().map(|v|v.expose()).unwrap_or(&refresh)))?);
            let aad=json(&("moosuri/gmail/v1","tokens",&r.tenant,&r.account,&id,revision+1))?;
            let cipher=providers::encrypt(&db.provider_key,plain.as_bytes(),aad.as_bytes())?;
            let expires=now()?.checked_add(i64::try_from(tokens.expires_in).map_err(|_|StoreError::InvalidInput)?).ok_or(StoreError::InvalidInput)?;
            tx.execute("UPDATE mail_connections SET credential=?3,credential_revision=credential_revision+1,expires_at=?4 WHERE tenant_id=?1 AND id=?2",params![r.tenant,id,cipher,expires])?;
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn mission_provider_save(
        &self,
        p: &MissionActionPermit,
        b: &MissionProviderCredentials,
        dispatch: bool,
    ) -> Result<Option<crate::ProviderDraftSave>> {
        let p = p.clone();
        let app = b.app.clone();
        let epoch = b.epoch;
        let revision = b.revision;
        let connection = b.connection_id.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=crate::agent_execution::guard_step(&tx,&db.provider_key,&p.lease)?;
            let(id,_)=provider_contract(&tx,&r,&p.request)?;
            let expected_connection:String=tx.query_row("SELECT connection_id FROM mail_actions WHERE tenant_id=?1 AND id=?2",params![r.tenant,id],|r|r.get(0))?;
            if expected_connection!=connection{return Err(StoreError::Forbidden)}
            crate::google_setup::guard_mail_app(&tx,&r.tenant,&connection,&app)?;
            let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND credential_revision=?4 AND epoch=?5 AND status='active')",params![r.tenant,r.account,connection,revision,epoch],|r|r.get(0))?;
            if !current{return Err(StoreError::Conflict)}
            let input=provider_input(&p.request)?;
            let result=if dispatch{Some(crate::mail::provider_saves::begin(&tx,&provider_owner(&r),&id,&input,&app,epoch,revision)?)}else{crate::mail::provider_saves::recorded(&tx,&provider_owner(&r),&id,&input)?};
            tx.commit()?;Ok(result)
        }).await
    }
}
