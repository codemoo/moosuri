//! Bounded public-page collections. Routes store count only; all URLs come from
//! the current user/accepted mission task and all IO has durable child receipts.
use super::*;
use crate::agent_execution::{self, StepLease, ToolReadError};
use moosuri_core::{ExternalToolRequest, WebToolRequest};
use serde_json::{Value, json};
use std::collections::BTreeSet;
const PREFIX: &str = "public-pages-v1:";
pub(super) fn count(route: &str) -> Option<usize> {
    route
        .strip_prefix(PREFIX)?
        .parse::<usize>()
        .ok()
        .filter(|n| (2..=4).contains(n))
}
fn route(count: usize) -> String {
    format!("{PREFIX}{count}")
}
fn active_profile(run: &chat::Run) -> bool {
    crate::agent_execution::skills::current_skill(
        run.tool_profile.as_ref(),
        "public-page-comparison",
    ) && run
        .tool_profile
        .as_ref()
        .is_some_and(|p| p.web.as_ref().is_some_and(|w| w.read))
}
/// Deliberately literal: URLs end at whitespace or quoting/bracket delimiters.
/// Ambiguous punctuation is not stripped or guessed. Duplicate/extra URLs reject.
fn targets(text: &str, urls: &[String]) -> bool {
    if !(2..=4).contains(&urls.len()) {
        return false;
    }
    let mut found = vec![];
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let next = [rest.find("https://"), rest.find("http://")]
            .into_iter()
            .flatten()
            .min();
        let Some(next) = next else { break };
        let start = at + next;
        let end = text[start..]
            .find(|c: char| c.is_whitespace() || "`\"'<>[](){}".contains(c))
            .map(|n| start + n)
            .unwrap_or(text.len());
        found.push(text[start..end].to_owned());
        at = end;
    }
    let fresh = found.iter().collect::<BTreeSet<_>>();
    let supplied = urls.iter().collect::<BTreeSet<_>>();
    fresh.len() == urls.len()
        && found.len() == urls.len()
        && fresh == supplied
        && found == urls
        && urls
            .iter()
            .map(|raw| moosuri_connectors::web::validate_public_url(raw))
            .collect::<std::result::Result<BTreeSet<_>, _>>()
            .is_ok_and(|normalized| normalized.len() == urls.len())
        && urls.iter().all(|raw| !raw.contains('#'))
}
fn task(c: &Connection, r: &chat::Run) -> Result<String> {
    if crate::missions::is_mission(c, &r.tenant, &r.id)? {
        crate::missions::learned_task_text(c, r)
    } else {
        Ok(r.text.clone())
    }
}
fn requested(
    c: &Connection,
    r: &chat::Run,
    input: &Input,
) -> Result<Option<(Vec<String>, Option<SkillPin>)>> {
    if !active_profile(r)
        || crate::artifacts::agent::profile(c, &r.tenant, &r.id)?.is_some_and(|p| p.write)
    {
        return Ok(None);
    }
    let (urls, pin) = match input {
        Input::CollectPages { urls } => (urls, None),
        Input::LearnedSkill { invocation } => {
            let LearnedSkillArguments::Pages { urls } = &invocation.arguments else {
                return Ok(None);
            };
            if !r
                .tool_profile
                .as_ref()
                .is_some_and(|p| p.skills.contains(&invocation.pin))
            {
                return Ok(None);
            }
            let Some(learned) = available(c, &r.tenant, &r.account, &invocation.pin)? else {
                return Ok(None);
            };
            if count(&learned) != Some(urls.len()) {
                return Ok(None);
            }
            (urls, Some(invocation.pin.clone()))
        }
        _ => return Ok(None),
    };
    if !targets(&task(c, r)?, urls) {
        return Ok(None);
    }
    Ok(Some((urls.clone(), pin)))
}
#[derive(Clone)]
pub struct PageCollectionPermit {
    lease: StepLease,
    call: String,
    nonce: Vec<u8>,
    urls: Vec<String>,
    pin: Option<SkillPin>,
}
impl PageCollectionPermit {
    pub fn len(&self) -> usize {
        self.urls.len()
    }
    pub fn is_empty(&self) -> bool {
        self.urls.is_empty()
    }
}
#[derive(Clone)]
pub struct PageReadPermit {
    parent: PageCollectionPermit,
    ordinal: usize,
    nonce: Vec<u8>,
}
impl PageReadPermit {
    pub fn url(&self) -> &str {
        &self.parent.urls[self.ordinal]
    }
}
pub enum PageCollectionStart {
    Complete(Output),
    Dispatch(PageCollectionPermit),
}
fn guard(
    c: &Connection,
    key: &[u8; 32],
    current: &moosuri_core::WebToolProfile,
    p: &PageCollectionPermit,
) -> Result<chat::Run> {
    let r = agent_execution::guard_step(c, key, &p.lease)?;
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM external_tool_dispatches d JOIN agent_tool_slots s USING(tenant_id,run_id,step,call_id) WHERE d.tenant_id=?1 AND d.run_id=?2 AND d.step=?3 AND d.call_id=?4 AND d.nonce=?5 AND d.state='started' AND s.result IS NULL)",params![r.tenant,r.id,p.lease.step,p.call,p.nonce],|r|r.get(0))?;
    let input: String=c.query_row("SELECT input FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,p.lease.step,p.call],|r|r.get(0))?;
    let Some((urls, pin)) = requested(
        c,
        &r,
        &serde_json::from_str(&input).map_err(|_| StoreError::Corrupt)?,
    )?
    else {
        return Err(StoreError::Forbidden);
    };
    if !valid
        || urls != p.urls
        || pin != p.pin
        || !p.urls.iter().all(|url| {
            crate::web::allowed(
                r.tool_profile.as_ref(),
                current,
                &WebToolRequest::Read { url: url.clone() },
            )
        })
    {
        return Err(StoreError::Conflict);
    }
    Ok(r)
}
fn read_guard(
    c: &Connection,
    key: &[u8; 32],
    current: &moosuri_core::WebToolProfile,
    p: &PageReadPermit,
) -> Result<chat::Run> {
    let r = guard(c, key, current, &p.parent)?;
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM learned_page_dispatches WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND ordinal=?5 AND nonce=?6)",params![r.tenant,r.id,p.parent.lease.step,p.parent.call,p.ordinal as i64,p.nonce],|r|r.get(0))?;
    if !valid {
        return Err(StoreError::Conflict);
    }
    Ok(r)
}
fn utf8_prefix(text: &str, limit: usize) -> &str {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1
    }
    &text[..end]
}
/// Preserve only actual observation fields, with explicit bounded-excerpt coverage.
/// The original complete adapter result remains in the private child receipt.
fn projection(result: &Output) -> Value {
    match result {
        Output::External(v) if v["web_page"].is_object() => {
            let p = &v["web_page"];
            let text = p["text"].as_str().unwrap_or("");
            json!({"web_page":{"url":p["url"],"title":p["title"],"retrieved_at_unix_ms":p["retrieved_at_unix_ms"],"text":utf8_prefix(text,512),"truncated":p["truncated"]!=false || text.len()>512,"warning":"http_only; bounded_collection_excerpt; no_semantic_success_claim"}})
        }
        Output::Error(error) => json!({"error":error}),
        _ => json!({"error":"unavailable"}),
    }
}
impl Store {
    pub async fn prepare_page_collection(
        &self,
        lease: &StepLease,
        call: String,
    ) -> Result<PageCollectionStart> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=agent_execution::guard_step(&tx,&db.provider_key,&lease)?;
            let (input,saved):(String,Option<String>)=tx.query_row("SELECT input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,lease.step,call],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if let Some(saved)=saved {return Ok(PageCollectionStart::Complete(serde_json::from_str(&saved).map_err(|_|StoreError::Corrupt)?))}
            let started:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4)",params![r.tenant,r.id,lease.step,call],|r|r.get(0))?;
            if started {
                let result=Output::Error(ToolReadError::OutcomeUnknown);
                tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,lease.step,call,encode(&result)?])?;tx.commit()?;return Ok(PageCollectionStart::Complete(result))
            }
            let input:Input=serde_json::from_str(&input).map_err(|_|StoreError::Corrupt)?;
            let request=requested(&tx,&r,&input)?;
            let used:i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|r.get(0))?;
            if request.is_none() {
                if used>=12{return Err(StoreError::BudgetExhausted)}
                let result=Output::Error(ToolReadError::InvalidInput);
                tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;
                tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,lease.step,call,encode(&result)?])?;
                tx.commit()?;return Ok(PageCollectionStart::Complete(result))
            }
            let (urls,pin)=request.unwrap();if used+urls.len() as i64>12{return Err(StoreError::BudgetExhausted)}
            if let Some(pin)=&pin {
                if crate::missions::is_mission(&tx,&r.tenant,&r.id)? {crate::recurring_missions::use_learned_skill(&tx,&r,pin)?;}
                tx.execute("INSERT OR IGNORE INTO learned_skill_uses VALUES(?1,?2,?3)",params![r.tenant,r.id,pin.id])?;
            }
            let nonce:Vec<u8>=Sha256::digest(Uuid::new_v4().as_bytes()).to_vec();
            tx.execute("INSERT INTO external_tool_dispatches VALUES(?1,?2,?3,?4,?5,'started')",params![r.tenant,r.id,lease.step,call,nonce])?;
            tx.commit()?;Ok(PageCollectionStart::Dispatch(PageCollectionPermit{lease,call,nonce,urls,pin}))
        }).await
    }
    pub async fn begin_collection_page(
        &self,
        p: &PageCollectionPermit,
        ordinal: usize,
    ) -> Result<PageReadPermit> {
        let p = p.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=guard(&tx,&db.provider_key,&db.web_profile,&p)?;
            if ordinal>=p.urls.len(){return Err(StoreError::InvalidInput)}
            let previous:i64=tx.query_row("SELECT count(*) FROM learned_page_dispatches WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND state='completed' AND ordinal<?5",params![r.tenant,r.id,p.lease.step,p.call,ordinal as i64],|r|r.get(0))?;
            if previous!=ordinal as i64{return Err(StoreError::Conflict)}
            let used:i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|r.get(0))?;
            if used>=12{return Err(StoreError::BudgetExhausted)}
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM learned_page_dispatches WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND ordinal=?5)",params![r.tenant,r.id,p.lease.step,p.call,ordinal as i64],|r|r.get(0))?;
            if exists{return Err(StoreError::Conflict)}
            let nonce:Vec<u8>=Sha256::digest(Uuid::new_v4().as_bytes()).to_vec();
            tx.execute("INSERT INTO learned_page_dispatches VALUES(?1,?2,?3,?4,?5,?6,'started')",params![r.tenant,r.id,p.lease.step,p.call,ordinal as i64,nonce])?;
            let generation=knowledge::generation(&tx,&r.tenant)?;
            tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1,generation=?3 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id,generation])?;
            tx.commit()?;Ok(PageReadPermit{parent:p,ordinal,nonce})
        }).await
    }
    pub async fn check_collection_page(&self, p: &PageReadPermit) -> Result<()> {
        let p = p.clone();
        self.call(move |db| read_guard(&db.conn, &db.provider_key, &db.web_profile, &p).map(|_| ()))
            .await
    }
    pub async fn complete_collection_page(
        &self,
        p: &PageReadPermit,
        mut result: Output,
    ) -> Result<Output> {
        let p = p.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let r=read_guard(&tx,&db.provider_key,&db.web_profile,&p)?;
            let saved:Option<String>=tx.query_row("SELECT result FROM learned_skill_receipts WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND ordinal=?5",params![r.tenant,r.id,p.parent.lease.step,p.parent.call,p.ordinal as i64],|r|r.get(0)).optional()?;
            if let Some(saved)=saved{return serde_json::from_str(&saved).map_err(|_|StoreError::Corrupt)}
            if encode(&result)?.len()>agent_execution::TOOL_RESULT_LIMIT {result=Output::Error(ToolReadError::TooLargeNarrowRead)}
            let input=Input::External{request:ExternalToolRequest::Web(WebToolRequest::Read{url:p.url().into()})};
            tx.execute("INSERT INTO learned_skill_receipts VALUES(?1,?2,?3,?4,?5,?6,?7)",params![r.tenant,r.id,p.parent.lease.step,p.parent.call,p.ordinal as i64,encode(&input)?,encode(&result)?])?;
            tx.execute("UPDATE learned_page_dispatches SET state='completed' WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND ordinal=?5",params![r.tenant,r.id,p.parent.lease.step,p.parent.call,p.ordinal as i64])?;
            tx.execute("UPDATE agent_execution SET checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn finish_page_collection(&self, p: &PageCollectionPermit) -> Result<Output> {
        let p = p.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=agent_execution::guard_step(&tx,&db.provider_key,&p.lease)?;
            let saved:Option<String>=tx.query_row("SELECT s.result FROM agent_tool_slots s JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE s.tenant_id=?1 AND s.run_id=?2 AND s.step=?3 AND s.call_id=?4 AND d.nonce=?5",params![r.tenant,r.id,p.lease.step,p.call,p.nonce],|r|r.get(0)).optional()?.flatten();
            if let Some(saved)=saved{return serde_json::from_str(&saved).map_err(|_|StoreError::Corrupt)}
            let r=guard(&tx,&db.provider_key,&db.web_profile,&p)?;
            let rows=tx.prepare("SELECT ordinal,result FROM learned_skill_receipts WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 ORDER BY ordinal")?.query_map(params![r.tenant,r.id,p.lease.step,p.call],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            if rows.len()!=p.urls.len() || rows.iter().enumerate().any(|(i,(n,_))|i as i64!=*n){return Err(StoreError::Conflict)}
            let mut entries=vec![];let mut all=true;
            for (ordinal,raw) in rows {
                let ordinal=usize::try_from(ordinal).map_err(|_|StoreError::Corrupt)?;
                if ordinal>=p.urls.len(){return Err(StoreError::Corrupt)}
                let result:Output=serde_json::from_str(&raw).map_err(|_|StoreError::Corrupt)?;
                let value=projection(&result);all &= value["web_page"].is_object();
                entries.push(json!({"requested_url":p.urls[ordinal],"receipt":{"step":p.lease.step,"call_id":p.call,"ordinal":ordinal},"result":value}));
            }
            let mut result=Output::External(json!({"public_pages":{"entries":entries,"all_reads_observed":all,"coverage":"bounded_http_excerpts","semantic_success_verified":false}}));
            if encode(&result)?.len()>agent_execution::TOOL_RESULT_LIMIT {result=Output::Error(ToolReadError::TooLargeNarrowRead)}
            tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,p.lease.step,p.call,encode(&result)?])?;
            tx.execute("UPDATE external_tool_dispatches SET state='completed' WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,p.lease.step,p.call])?;
            tx.execute("UPDATE agent_execution SET checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;tx.commit()?;Ok(result)
        }).await
    }
}
/// Qualification is bounded evidence adoption, never a semantic-comparison grade.
pub(super) fn proof(
    c: &Connection,
    t: &str,
    a: &str,
    run: &str,
    calls: &[(Input, Output)],
    receipt: &str,
) -> Result<Option<Proof>> {
    let [(Input::CollectPages { urls }, Output::External(value))] = calls else {
        return Ok(None);
    };
    let r = chat::load_run(c, t, run)?;
    if r.account != a || !active_profile(&r) || !targets(&r.text, urls) {
        return Ok(None);
    }
    let rows=c.prepare("SELECT s.step,s.call_id FROM agent_tool_slots s JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE s.tenant_id=?1 AND s.run_id=?2 AND d.state='completed' AND s.result=?3")?.query_map(params![t,run,encode(&Output::External(value.clone()))?],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let [(step, call)] = rows.as_slice() else {
        return Ok(None);
    };
    let Some(entries) = value["public_pages"]["entries"]
        .as_array()
        .filter(|v| v.len() == urls.len())
    else {
        return Ok(None);
    };
    if value["public_pages"]["all_reads_observed"] != true
        || value["public_pages"]["semantic_success_verified"] != false
    {
        return Ok(None);
    }
    let terminal: Option<i64> = c.query_row(
        "SELECT max(step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state='completed'",
        params![t, run],
        |r| r.get(0),
    )?;
    let Some(terminal) = terminal else {
        return Ok(None);
    };
    if terminal <= *step {
        return Ok(None);
    }
    let output=c.prepare("SELECT text FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item LIKE ?3 AND refusal=0 AND (phase IS NULL OR phase='final_answer') ORDER BY ordinal")?.query_map(params![t,run,format!("step-{terminal}-%")],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?.join("\n");
    for (ordinal, entry) in entries.iter().enumerate() {
        let raw:Option<(String,String)>=c.query_row("SELECT x.input,x.result FROM learned_skill_receipts x JOIN learned_page_dispatches d USING(tenant_id,run_id,step,call_id,ordinal) WHERE x.tenant_id=?1 AND x.run_id=?2 AND x.step=?3 AND x.call_id=?4 AND x.ordinal=?5 AND d.state='completed'",params![t,run,step,call,ordinal as i64],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((input, result)) = raw else {
            return Ok(None);
        };
        let expected = Input::External {
            request: ExternalToolRequest::Web(WebToolRequest::Read {
                url: urls[ordinal].clone(),
            }),
        };
        if input != encode(&expected)?
            || entry["requested_url"] != urls[ordinal]
            || entry["receipt"] != json!({"step":step,"call_id":call,"ordinal":ordinal})
        {
            return Ok(None);
        }
        let result: Output = serde_json::from_str(&result).map_err(|_| StoreError::Corrupt)?;
        let projected = projection(&result);
        if entry["result"] != projected {
            return Ok(None);
        }
        let page = &projected["web_page"];
        let (Some(url), Some(title), Some(text)) = (
            page["url"].as_str(),
            page["title"].as_str(),
            page["text"].as_str(),
        ) else {
            return Ok(None);
        };
        if moosuri_connectors::web::validate_public_url(url).is_err()
            || title.trim().is_empty()
            || text.trim().is_empty()
            || page["retrieved_at_unix_ms"].as_u64().is_none_or(|n| n == 0)
            || ![url, title, utf8_prefix(text, 80)]
                .iter()
                .all(|v| output.contains(v))
        {
            return Ok(None);
        }
    }
    // Invocation authority remains bound to the exact current literal spelling.
    // Independence is about actual normalized targets: host case, default ports
    // and dot-path aliases cannot manufacture a second successful example.
    let mut canonical = urls
        .iter()
        .map(|raw| moosuri_connectors::web::validate_public_url(raw))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| StoreError::Corrupt)?;
    canonical.sort();
    Ok(Some(Proof {
        route: route(urls.len()),
        input: hash(&encode(&canonical)?),
        receipt: receipt.into(),
        subject: None,
    }))
}
#[cfg(test)]
mod tests;
