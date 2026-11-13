//! Collection opt-in creates a background principal, never a synthetic session.
//! Jobs retain opaque pins/accounting only; source/selection plaintext is not persisted.
use super::*;
use moosuri_core::{
    EvidenceRef, MemoryDraft, MemoryExtractionChange, MemoryExtractionOrigin,
    MemoryExtractionStatus, ModelChoice, RunUsage,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
const POLICY: &str = "literal-paragraph-v1";
#[derive(Clone, Serialize, Deserialize)]
pub struct ExtractionUnit {
    pub index: usize,
    pub text: String,
    pub start_byte: u32,
    pub end_byte: u32,
}
#[derive(Clone)]
pub struct ExtractionLease {
    tenant: String,
    job: String,
    nonce: Vec<u8>,
    generation: i64,
}
pub struct ExtractionClaim {
    pub lease: ExtractionLease,
    pub units: Vec<ExtractionUnit>,
    pub reported_speaker: String,
    pub binding: ProviderBinding,
}
struct Grant {
    account: String,
    revision: i64,
    enabled: bool,
    mode: String,
    speaker: String,
    epoch: i64,
    model: ModelChoice,
    calls: u32,
    bytes: u32,
    output: u32,
}
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn grant(conn: &Connection, tenant: &str, collection: &str) -> Result<Option<Grant>> {
    conn.query_row("SELECT account_id,revision,enabled,attribution_mode,reported_speaker,auth_epoch,model,daily_calls,daily_bytes,daily_output FROM automatic_memory_grants WHERE tenant_id=?1 AND collection_id=?2",params![tenant,collection],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,bool>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?,r.get::<_,u32>(7)?,r.get::<_,u32>(8)?,r.get::<_,u32>(9)?))) .optional()?.map(|v|Ok::<Grant,StoreError>(Grant{account:v.0,revision:v.1,enabled:v.2,mode:v.3,speaker:v.4,epoch:v.5,model:decode(&v.6)?,calls:v.7,bytes:v.8,output:v.9})).transpose()
}
fn eligible(conn: &Connection, tenant: &str, collection: &str, g: &Grant) -> Result<bool> {
    Ok(g.enabled && conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts a JOIN knowledge_collections c ON c.tenant_id=a.tenant_id WHERE a.tenant_id=?1 AND a.id=?2 AND a.auth_epoch=?3 AND a.recovery_required=0 AND a.must_change_password=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?5) AND c.id=?4 AND c.connected=1 AND c.deleted=0)",params![tenant,g.account,g.epoch,collection,now()?],|r|r.get(0))?)
}
fn speaker_label(s: &str) -> bool {
    !s.trim().is_empty()
        && s.len() <= 160
        && !s.chars().any(|c| c.is_control() || ",;<>|&/\\".contains(c))
        && ![" and ", " 및 ", " 와 ", " 과 "]
            .iter()
            .any(|p| s.contains(p))
}
fn attribution(g: &Grant, source: &moosuri_core::SourceDocument) -> Option<String> {
    if g.mode == "reported" {
        source.source.speaker.clone().filter(|s| speaker_label(s))
    } else if source
        .source
        .speaker
        .as_ref()
        .is_none_or(|s| s == &g.speaker)
        && speaker_label(&g.speaker)
    {
        Some(g.speaker.clone())
    } else {
        None
    }
}
fn units(text: &str) -> Vec<ExtractionUnit> {
    // Complete paragraphs only; CRLF and whitespace bytes remain in each unit.
    let mut result = vec![];
    let mut offset = 0;
    let mut used = 0;
    while offset < text.len() {
        let start = offset;
        let rest = &text[start..];
        // One scan also bounds work for CRLF documents after the input budget
        // is full; searching separately for absent LF separators would rescan.
        let end = rest
            .bytes()
            .enumerate()
            .find_map(|(i, byte)| match byte {
                b'\n' if rest.as_bytes()[i..].starts_with(b"\n\n") => Some(i + 2),
                b'\r' if rest.as_bytes()[i..].starts_with(b"\r\n\r\n") => Some(i + 4),
                _ => None,
            })
            .unwrap_or(rest.len());
        offset += end;
        let part = &text[start..offset];
        if part.trim().is_empty() || part.len() > 4096 || used + part.len() > 8192 {
            continue;
        }
        used += part.len();
        result.push(ExtractionUnit {
            index: result.len(),
            text: part.into(),
            start_byte: start as u32,
            end_byte: offset as u32,
        });
        if result.len() == 16 {
            break;
        }
    }
    result
}
fn available_units(
    conn: &Connection,
    tenant: &str,
    source: &str,
    text: &str,
) -> Result<Vec<ExtractionUnit>> {
    let mut available = vec![];
    for mut unit in units(text) {
        let hash: [u8; 32] = Sha256::digest(unit.text.as_bytes()).into();
        let suppressed:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_suppressions WHERE tenant_id=?1 AND source_id=?2 AND quote_hash=?3)",params![tenant,source,hash.as_slice()],|r|r.get(0))?;
        if !suppressed {
            unit.index = available.len();
            available.push(unit)
        }
    }
    Ok(available)
}
pub(crate) fn origin(
    conn: &Connection,
    tenant: &str,
    memory: &str,
    revision: i64,
) -> Result<Option<MemoryExtractionOrigin>> {
    if revision != 1 {
        return Ok(None);
    }
    conn.query_row("SELECT source_id,source_revision,reported_speaker,model FROM automatic_memory_origins WHERE tenant_id=?1 AND memory_id=?2",params![tenant,memory],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional()?.map(|(source_id,source_revision,reported_speaker,encoded)|{
        let model:ModelChoice=decode(&encoded)?;
        Ok::<_,StoreError>(MemoryExtractionOrigin{source_id,source_revision,reported_speaker,policy_revision:POLICY.into(),model:model.model,provider_revision:model.provider_revision})
    }).transpose()
}
pub(crate) fn restart(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE automatic_memory_jobs SET state='outcome_unknown',reason='interrupted',nonce=NULL,deadline=NULL WHERE state IN('acquired','dispatched')",[])?;
    Ok(())
}
pub(crate) fn invalidate_source(conn: &Connection, tenant: &str, source: &str) -> Result<()> {
    conn.execute("UPDATE automatic_memory_jobs SET state='cancelled',reason='source_changed',nonce=NULL,deadline=NULL WHERE tenant_id=?1 AND source_id=?2 AND state IN('queued','acquired','dispatched')",params![tenant,source])?;
    Ok(())
}
/// Retain only content-free dedup after a memory's text/history is forgotten.
pub(crate) fn forget_memory(conn: &Connection, tenant: &str, memory: &str) -> Result<()> {
    let sources = conn
        .prepare(
            "SELECT DISTINCT source_id FROM knowledge_evidence WHERE tenant_id=?1 AND memory_id=?2",
        )?
        .query_map(params![tenant, memory], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for source in sources {
        invalidate_source(conn, tenant, &source)?;
    }
    conn.execute(
        "DELETE FROM automatic_memory_origins WHERE tenant_id=?1 AND memory_id=?2",
        params![tenant, memory],
    )?;
    Ok(())
}
/// Source withdrawal also removes origin attribution from purged historical facts.
pub(crate) fn withdraw_source(conn: &Connection, tenant: &str, source: &str) -> Result<()> {
    invalidate_source(conn, tenant, source)?;
    conn.execute(
        "DELETE FROM automatic_memory_origins WHERE tenant_id=?1 AND source_id=?2",
        params![tenant, source],
    )?;
    Ok(())
}
pub(crate) fn invalidate_collection(
    conn: &Connection,
    tenant: &str,
    collection: &str,
) -> Result<()> {
    conn.execute("UPDATE automatic_memory_grants SET enabled=0,revision=revision+1 WHERE tenant_id=?1 AND collection_id=?2",params![tenant,collection])?;
    conn.execute("UPDATE automatic_memory_jobs SET state='cancelled',reason='collection_disabled',nonce=NULL,deadline=NULL WHERE tenant_id=?1 AND collection_id=?2 AND state IN('queued','acquired','dispatched')",params![tenant,collection])?;
    Ok(())
}
pub(crate) fn enqueue(
    conn: &Connection,
    tenant: &str,
    collection: &str,
    source: &str,
) -> Result<()> {
    let Some(g) = grant(conn, tenant, collection)? else {
        return Ok(());
    };
    if !eligible(conn, tenant, collection, &g)? {
        return Ok(());
    }
    let revision: i64 = conn.query_row(
        "SELECT revision FROM knowledge_sources WHERE tenant_id=?1 AND id=?2 AND deleted=0",
        params![tenant, source],
        |r| r.get(0),
    )?;
    let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM automatic_memory_jobs WHERE tenant_id=?1 AND source_id=?2 AND source_revision=?3 AND grant_revision=?4)",params![tenant,source,revision,g.revision],|r|r.get(0))?;
    if exists {
        return Ok(());
    }
    conn.execute("UPDATE automatic_memory_jobs SET state='cancelled',reason='superseded',nonce=NULL,deadline=NULL WHERE tenant_id=?1 AND source_id=?2 AND state='queued'",params![tenant,source])?;
    let count:i64=conn.query_row("SELECT count(*) FROM automatic_memory_jobs WHERE tenant_id=?1 AND state IN('queued','acquired','dispatched')",[tenant],|r|r.get(0))?;
    let state = if count >= 32 { "skipped" } else { "queued" };
    conn.execute("INSERT INTO automatic_memory_jobs(tenant_id,id,collection_id,source_id,source_revision,grant_revision,state,reason,model,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![tenant,Uuid::new_v4().to_string(),collection,source,revision,g.revision,state,if count>=32 {Some("queue_full")}else{None},encode(&g.model)?,now()?])?;
    Ok(())
}
fn status(
    conn: &Connection,
    tenant: &str,
    collection: &str,
    source: Option<&str>,
) -> Result<MemoryExtractionStatus> {
    let g = grant(conn, tenant, collection)?;
    let grant_revision = g.as_ref().map(|g| g.revision);
    let active = g
        .as_ref()
        .map(|g| eligible(conn, tenant, collection, g))
        .transpose()?
        .unwrap_or(false);
    let latest:Option<(String,Option<String>,u32)>=conn.query_row("SELECT state,reason,saved_count FROM automatic_memory_jobs WHERE tenant_id=?1 AND collection_id=?2 AND (?3 IS NULL OR source_id=?3) AND grant_revision=?4 ORDER BY created_at DESC,rowid DESC LIMIT 1",params![tenant,collection,source,grant_revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let (state, reason, saved) = latest.unwrap_or(("idle".into(), None, 0));
    Ok(match g {
        Some(g) => MemoryExtractionStatus {
            revision: g.revision,
            enabled: active,
            attribution_mode: g.mode,
            reported_speaker: g.speaker,
            daily_calls: g.calls,
            daily_input_bytes: g.bytes,
            daily_output_tokens: g.output,
            state,
            reason,
            saved_count: saved,
        },
        None => MemoryExtractionStatus {
            revision: 0,
            enabled: false,
            attribution_mode: "reported".into(),
            reported_speaker: String::new(),
            daily_calls: 10,
            daily_input_bytes: 24000,
            daily_output_tokens: 4000,
            state,
            reason,
            saved_count: saved,
        },
    })
}
struct Job {
    collection: String,
    source: String,
    revision: i64,
    grant_revision: i64,
    state: String,
    deadline: Option<i64>,
}
fn job(conn: &Connection, lease: &ExtractionLease) -> Result<Job> {
    conn.query_row("SELECT collection_id,source_id,source_revision,grant_revision,state,deadline FROM automatic_memory_jobs WHERE tenant_id=?1 AND id=?2 AND nonce=?3",params![lease.tenant,lease.job,lease.nonce],|r|Ok(Job{collection:r.get(0)?,source:r.get(1)?,revision:r.get(2)?,grant_revision:r.get(3)?,state:r.get(4)?,deadline:r.get(5)?})).optional()?.ok_or(StoreError::Conflict)
}
fn checked(
    conn: &Connection,
    key: &[u8; 32],
    lease: &ExtractionLease,
) -> Result<(Job, Grant, moosuri_core::SourceDocument)> {
    if knowledge::generation(conn, &lease.tenant)? != lease.generation {
        return Err(StoreError::Conflict);
    }
    let j = job(conn, lease)?;
    let g = grant(conn, &lease.tenant, &j.collection)?.ok_or(StoreError::Conflict)?;
    if g.revision != j.grant_revision
        || !eligible(conn, &lease.tenant, &j.collection, &g)?
        || j.deadline.is_none_or(|d| d <= now().unwrap_or(i64::MAX))
        || !matches!(j.state.as_str(), "acquired" | "dispatched")
    {
        return Err(StoreError::Conflict);
    }
    providers::binding_for_choice(conn, key, &g.model)?;
    let source = knowledge::sources::document(conn, &lease.tenant, &j.source, None)?;
    if source.source.revision != j.revision {
        return Err(StoreError::Conflict);
    }
    Ok((j, g, source))
}
impl Store {
    pub async fn memory_extraction_status(
        &self,
        user: &UserContext,
        collection: String,
    ) -> Result<MemoryExtractionStatus> {
        let user = user.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &user, true, false)?;
            knowledge::collection(&db.conn, &u.tenant.0, &collection)?;
            status(&db.conn, &u.tenant.0, &collection, None)
        })
        .await
    }
    pub async fn source_memory_extraction_status(
        &self,
        user: &UserContext,
        source: String,
    ) -> Result<MemoryExtractionStatus> {
        let user = user.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &user, true, false)?;
            let d = knowledge::sources::document(&db.conn, &u.tenant.0, &source, None)?;
            status(
                &db.conn,
                &u.tenant.0,
                &d.source.collection_id,
                Some(&source),
            )
        })
        .await
    }
    pub async fn set_memory_extraction(
        &self,
        user: &UserContext,
        collection: String,
        change: MemoryExtractionChange,
    ) -> Result<MemoryExtractionStatus> {
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;let group=knowledge::collection(&db.conn,&u.tenant.0,&collection)?;
            if !group.connected||group.deleted{return Err(StoreError::Conflict)}
            if (change.attribution_mode=="reported"&&!change.reported_speaker.is_empty())||!matches!(change.attribution_mode.as_str(),"reported"|"specified")||change.daily_calls>10||change.daily_input_bytes>24000||change.daily_output_tokens>4000||change.daily_calls==0||change.daily_input_bytes==0||change.daily_output_tokens<1024||(change.attribution_mode=="specified"&&!speaker_label(&change.reported_speaker)) {return Err(StoreError::InvalidInput)}
            let model=if change.enabled {
                let model=providers::selected_choice(&db.conn,&u)?;if !model.function_tools{return Err(StoreError::Conflict)}
                providers::binding_for_choice(&db.conn,&db.provider_key,&model)?;model
            } else {grant(&db.conn,&u.tenant.0,&collection)?.ok_or(StoreError::Conflict)?.model};
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let revision=grant(&tx,&u.tenant.0,&collection)?.map(|g|g.revision).unwrap_or(0);
            if revision!=change.expected_revision{return Err(StoreError::Conflict)}
            invalidate_collection(&tx,&u.tenant.0,&collection)?;
            tx.execute("INSERT INTO automatic_memory_grants VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(tenant_id,collection_id) DO UPDATE SET account_id=excluded.account_id,revision=excluded.revision,enabled=excluded.enabled,attribution_mode=excluded.attribution_mode,reported_speaker=excluded.reported_speaker,auth_epoch=excluded.auth_epoch,model=excluded.model,daily_calls=excluded.daily_calls,daily_bytes=excluded.daily_bytes,daily_output=excluded.daily_output",params![u.tenant.0,collection,u.account.id,revision+1,change.enabled,change.attribution_mode,change.reported_speaker,u.epoch,encode(&model)?,change.daily_calls,change.daily_input_bytes,change.daily_output_tokens])?;
            let result=status(&tx,&u.tenant.0,&collection,None)?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn request_memory_extraction(
        &self,
        user: &UserContext,
        source: String,
        revision: i64,
    ) -> Result<MemoryExtractionStatus> {
        let user = user.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &user, true, false)?;
            let d = knowledge::sources::document(&db.conn, &u.tenant.0, &source, None)?;
            if d.source.revision != revision {
                return Err(StoreError::Conflict);
            }
            let g = grant(&db.conn, &u.tenant.0, &d.source.collection_id)?
                .ok_or(StoreError::Conflict)?;
            if !eligible(&db.conn, &u.tenant.0, &d.source.collection_id, &g)? {
                return Err(StoreError::Conflict);
            }
            enqueue(&db.conn, &u.tenant.0, &d.source.collection_id, &source)?;
            status(
                &db.conn,
                &u.tenant.0,
                &d.source.collection_id,
                Some(&source),
            )
        })
        .await
    }
    pub async fn claim_memory_extraction(&self) -> Result<Option<ExtractionClaim>> {
        self.call(move|db|{
        let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE automatic_memory_jobs SET state='outcome_unknown',reason='interrupted',nonce=NULL,deadline=NULL WHERE state IN('acquired','dispatched') AND deadline<=?1",[now()?])?;
        let rows=tx.prepare("SELECT tenant_id,id,collection_id,source_id,source_revision,grant_revision FROM automatic_memory_jobs WHERE state='queued' AND not_before<=?1 AND tenant_id NOT IN(SELECT tenant_id FROM automatic_memory_jobs WHERE state IN('acquired','dispatched')) ORDER BY created_at,rowid LIMIT 32")?.query_map([now()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        for (tenant,id,collection,source,revision,gr) in rows {
            let Some(g)=grant(&tx,&tenant,&collection)? else{continue};
            let doc=knowledge::sources::document(&tx,&tenant,&source,None);
            if !eligible(&tx,&tenant,&collection,&g)?||g.revision!=gr||doc.as_ref().is_err() || doc.as_ref().is_ok_and(|d|d.source.revision!=revision) {tx.execute("UPDATE automatic_memory_jobs SET state='cancelled',reason='authority_changed' WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;continue}
            let doc=doc?;
            let Some(speaker)=attribution(&g,&doc) else{tx.execute("UPDATE automatic_memory_jobs SET state='skipped',reason='unknown_attribution' WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;continue};
            let units=available_units(&tx,&tenant,&source,&doc.text)?;if units.is_empty(){tx.execute("UPDATE automatic_memory_jobs SET state='skipped',reason='input_bound' WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;continue}
            let binding=match providers::binding_for_choice(&tx,&db.provider_key,&g.model){Ok(v)=>v,Err(_)=>{tx.execute("UPDATE automatic_memory_jobs SET state='cancelled',reason='provider_changed' WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;continue}};
            let mut nonce=[0u8;32];OsRng.try_fill_bytes(&mut nonce).map_err(|_|StoreError::Unavailable)?;
            tx.execute("UPDATE automatic_memory_jobs SET state='acquired',nonce=?3,deadline=?4 WHERE tenant_id=?1 AND id=?2",params![tenant,id,nonce.as_slice(),now()?+120])?;
            let generation=knowledge::generation(&tx,&tenant)?;
            tx.commit()?;return Ok(Some(ExtractionClaim{lease:ExtractionLease{generation,tenant,job:id,nonce:nonce.to_vec()},units,reported_speaker:speaker,binding}))
        }
        tx.commit()?;Ok(None)
    }).await
    }
    pub async fn admit_memory_extraction(
        &self,
        lease: &ExtractionLease,
        input_bytes: usize,
    ) -> Result<bool> {
        let lease = lease.clone();
        self.call(move|db|{
        if input_bytes==0||input_bytes>24000{return Err(StoreError::InvalidInput)}
        let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (j,g,_)=checked(&tx,&db.provider_key,&lease)?;if j.state!="acquired"{return Err(StoreError::Conflict)}
        let day=now()?.div_euclid(86400);
        let (calls,bytes,output):(u32,u32,u32)=tx.query_row("SELECT calls,input_bytes,output_tokens FROM automatic_memory_budget WHERE tenant_id=?1 AND account_id=?2 AND day=?3",params![lease.tenant,g.account,day],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.unwrap_or((0,0,0));
        if calls>=g.calls||bytes as usize+input_bytes>g.bytes as usize||output+1024>g.output {
            tx.execute("UPDATE automatic_memory_jobs SET state='queued',reason='daily_budget',nonce=NULL,deadline=NULL,not_before=?3 WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.job,(day+1)*86400])?;tx.commit()?;return Ok(false)
        }
        tx.execute("INSERT INTO automatic_memory_budget VALUES(?1,?2,?3,1,?4,1024) ON CONFLICT(tenant_id,account_id,day) DO UPDATE SET calls=calls+1,input_bytes=input_bytes+excluded.input_bytes,output_tokens=output_tokens+1024",params![lease.tenant,g.account,day,input_bytes as i64])?;
        tx.execute("UPDATE automatic_memory_jobs SET state='dispatched',reason=NULL,input_bytes=?3,dispatch_nonce=nonce WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.job,input_bytes as i64])?;
        tx.commit()?;Ok(true)
    }).await
    }
    pub async fn check_memory_extraction(&self, lease: &ExtractionLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| checked(&db.conn, &db.provider_key, &lease).map(|_| ()))
            .await
    }
    pub async fn finish_memory_extraction(
        &self,
        lease: &ExtractionLease,
        selected: Vec<usize>,
        usage: RunUsage,
    ) -> Result<u32> {
        let lease = lease.clone();
        self.call(move|db|{
        if selected.len()>4||selected.iter().collect::<BTreeSet<_>>().len()!=selected.len(){return Err(StoreError::InvalidInput)}
        let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let completed:Option<u32>=tx.query_row("SELECT saved_count FROM automatic_memory_jobs WHERE tenant_id=?1 AND id=?2 AND nonce=?3 AND state='completed'",params![lease.tenant,lease.job,lease.nonce],|r|r.get(0)).optional()?;
        if let Some(saved)=completed {tx.commit()?;return Ok(saved)}
        let(j,g,doc)=checked(&tx,&db.provider_key,&lease)?;if j.state!="dispatched"{return Err(StoreError::Conflict)}
        let speaker=attribution(&g,&doc).ok_or(StoreError::Conflict)?;let units=available_units(&tx,&lease.tenant,&j.source,&doc.text)?;let mut saved=0;
        let speaker_hash:[u8;32]=Sha256::digest(speaker.as_bytes()).into();
        for index in selected {
            let unit=units.get(index).ok_or(StoreError::InvalidInput)?;let hash:[u8;32]=Sha256::digest(unit.text.as_bytes()).into();
            let suppressed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_suppressions WHERE tenant_id=?1 AND source_id=?2 AND quote_hash=?3) OR EXISTS(SELECT 1 FROM automatic_memory_facts WHERE tenant_id=?1 AND source_id=?2 AND quote_hash=?3 AND reported_speaker_hash=?4)",params![lease.tenant,j.source,hash.as_slice(),speaker_hash.as_slice()],|r|r.get(0))?;
            if suppressed{continue}
            let draft=MemoryDraft{text:unit.text.clone(),subject:speaker.clone(),speaker:Some(speaker.clone()),valid_from:doc.source.valid_from,valid_until:doc.source.valid_until,evidence:vec![EvidenceRef{source_id:j.source.clone(),source_revision:j.revision,start_byte:unit.start_byte,end_byte:unit.end_byte,quote:unit.text.clone()}]};
            let memory=knowledge::memories::write_in_transaction(&tx,&lease.tenant,None,Uuid::new_v4().to_string(),draft)?;
            tx.execute("INSERT INTO automatic_memory_facts VALUES(?1,?2,?3,?4,?5)",params![lease.tenant,j.source,hash.as_slice(),speaker_hash.as_slice(),memory.id])?;
            tx.execute("INSERT INTO automatic_memory_origins VALUES(?1,?2,?3,?4,?5,?6,?7)",params![lease.tenant,memory.id,j.source,j.revision,speaker,encode(&g.model)?,g.revision])?;saved+=1;
        }
        tx.execute("UPDATE automatic_memory_jobs SET state='completed',reason=?3,saved_count=?4,usage=?5,deadline=NULL WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.job,if units.iter().map(|u|u.text.len()).sum::<usize>()<doc.text.len(){Some("partial_input")}else if saved==0 {Some("no_new_facts")}else{None},saved,encode(&usage)?])?;
        tx.commit()?;Ok(saved)
    }).await
    }
    /// Content-free late usage can be recorded even after authority revocation.
    pub async fn record_memory_extraction_usage(
        &self,
        lease: &ExtractionLease,
        usage: RunUsage,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
            db.conn.execute("UPDATE automatic_memory_jobs SET usage=?4 WHERE tenant_id=?1 AND id=?2 AND dispatch_nonce=?3",params![lease.tenant,lease.job,lease.nonce,encode(&usage)?])?;Ok(())
        }).await
    }
    pub async fn fail_memory_extraction(
        &self,
        lease: &ExtractionLease,
        reason: String,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
        if !["interrupted","transport_failed","invalid_decision","request_bound"].contains(&reason.as_str()){return Err(StoreError::InvalidInput)}
        db.conn.execute("UPDATE automatic_memory_jobs SET state='failed',reason=?4,nonce=NULL,deadline=NULL WHERE tenant_id=?1 AND id=?2 AND nonce=?3 AND state IN('acquired','dispatched')",params![lease.tenant,lease.job,lease.nonce,reason])?;Ok(())
    }).await
    }
}
#[cfg(test)]
mod tests;
