//! Durable member research outcomes. The source of truth is the member's own thread: browser runs
//! (`personal_browser_jobs` + their saved observation text) and explicitly completed Gmail inbox
//! reads. An outcome stores only criteria and source *references*; offers, facts, drafts and receipts
//! are re-derived from currently readable sources on every read, so a withdrawn mail connection, a
//! revoked browser authority, a changed auth epoch or a new volume generation removes the evidence
//! immediately. Mail metadata is read through `member_google_thread_history` and is never written
//! here, copied into chat output, or passed to a model.
use super::*;
use moosuri_core::personal_browser::research_outcomes as ro;

const PAGE: i64 = 20;
const MAX_ACTIVE: i64 = 48;
const AUTO_BROWSER: i64 = 4;
const AUTO_MAIL: usize = 2;
const QUESTION: &str = "비교할 공개 페이지의 URL을 알려 주세요. 이 질문은 한 번만 합니다.";

struct Own {
    tenant: String,
    account: String,
    volume: String,
    generation: i64,
}
impl Own {
    fn of(owner: VolumeOwner) -> Self {
        Self {
            tenant: owner.tenant.to_string(),
            account: owner.account.to_string(),
            volume: owner.volume.to_string(),
            generation: owner.generation as i64,
        }
    }
}

struct Row {
    id: String,
    kind: String,
    title: String,
    message: String,
    criteria: ro::Criteria,
    auth_epoch: i64,
    host_epoch: i64,
    revision: i64,
    auto: bool,
    draft: bool,
    question: Option<String>,
    pending_url: Option<String>,
    withdrawn: bool,
    created_at: i64,
    digest: Vec<u8>,
}
const ROW: &str = "SELECT id,kind,title,message,criteria,auth_epoch,host_epoch,revision,auto_sources,draft_requested,
    question_id,pending_url,withdrawn,created_at,request_digest FROM member_research_outcomes";
fn row_of(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    let raw: String = r.get(4)?;
    let criteria = serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Row {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        message: r.get(3)?,
        criteria,
        auth_epoch: r.get(5)?,
        host_epoch: r.get(6)?,
        revision: r.get(7)?,
        auto: r.get(8)?,
        draft: r.get(9)?,
        question: r.get(10)?,
        pending_url: r.get(11)?,
        withdrawn: r.get(12)?,
        created_at: r.get(13)?,
        digest: r.get(14)?,
    })
}

fn canonical(value: &str) -> Result<()> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil() && id.to_string() == value)
        .map(|_| ())
        .ok_or(StoreError::InvalidInput)
}
fn message_ok(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= 8192
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
fn thread_owned(c: &Connection, user: &UserContext, thread: &str) -> Result<()> {
    canonical(thread)?;
    let found: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND id=?2 AND account_id=?3 AND mission_internal=0)",
        params![user.tenant.0, thread, user.account.id],
        |r| r.get(0),
    )?;
    if found {
        Ok(())
    } else {
        Err(StoreError::NotFound)
    }
}

/// A public https page only: no credentials, no IP literal, no local or internal names.
fn public_url(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() || text.len() > 2048 || text.chars().any(char::is_control) {
        return Err(StoreError::InvalidInput);
    }
    let mut url = url::Url::parse(text).map_err(|_| StoreError::InvalidInput)?;
    let host = match url.host() {
        Some(url::Host::Domain(d)) => d.to_ascii_lowercase(),
        _ => return Err(StoreError::InvalidInput),
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !host.contains('.')
        || host == "localhost"
        || [
            ".local",
            ".localhost",
            ".internal",
            ".lan",
            ".home",
            ".corp",
        ]
        .iter()
        .any(|s| host.ends_with(s))
    {
        return Err(StoreError::InvalidInput);
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
}
fn digest_text(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ----------------------------------------------------------------- sources

struct Snapshot {
    view: Workspace,
    text: String,
    observed_at: i64,
}

/// The saved observation of one browser run, only while this member's current authority still
/// covers it (same account, thread, volume and generation, and the run's auth epoch is current).
pub(super) fn reading_aad(
    owner: VolumeOwner,
    user: &UserContext,
    thread: &str,
    run: &str,
) -> Vec<u8> {
    format!(
        "moosuri/member-browser-reading/v1/{}/{}/{}/{}/{}/{}/{}",
        owner.tenant, owner.account, owner.volume, owner.generation, user.epoch, thread, run
    )
    .into_bytes()
}
fn snapshot(
    c: &Connection,
    key: &[u8; 32],
    owner: VolumeOwner,
    user: &UserContext,
    thread: &str,
    run: &str,
) -> Result<Option<Snapshot>> {
    let own = Own::of(owner);
    let found = c
        .query_row(
            "SELECT b.view,r.auth_epoch,r.created_at,
               b.research_reading,
               (SELECT max(h.at) FROM personal_task_history h WHERE h.tenant_id=b.tenant_id AND h.run_id=b.run_id
                  AND h.output_id IS NOT NULL)
             FROM personal_browser_jobs b JOIN chat_runs r ON r.tenant_id=b.tenant_id AND r.id=b.run_id
             WHERE b.tenant_id=?1 AND b.run_id=?2 AND b.thread_id=?3 AND b.volume_id=?4 AND b.generation=?5
               AND r.account_id=?6",
            params![own.tenant, run, thread, own.volume, own.generation, own.account],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((encoded, epoch, created, text, at)) = found else {
        return Ok(None);
    };
    if epoch != user.epoch {
        return Ok(None);
    }
    let view: Workspace = serde_json::from_str(&encoded).map_err(|_| StoreError::Corrupt)?;
    if view.state == "closed" && view.reason.as_deref() == Some("browser_authority_expired") {
        return Ok(None);
    }
    let Some(cipher) = text else { return Ok(None) };
    let bytes = providers::decrypt(key, &cipher, &reading_aad(owner, user, thread, run))?;
    let (url, _, reading): (String, String, String) =
        serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt)?;
    if view.url != url {
        return Ok(None);
    }
    Ok(Some(Snapshot {
        view,
        text: reading,
        observed_at: at.unwrap_or(created),
    }))
}

/// The thread's most recent readable browser runs, oldest first.
fn default_browser(
    c: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    thread: &str,
) -> Result<Vec<String>> {
    let own = Own::of(owner);
    let mut ids = c
        .prepare(
            "SELECT b.run_id FROM personal_browser_jobs b JOIN chat_runs r ON r.tenant_id=b.tenant_id AND r.id=b.run_id
             WHERE b.tenant_id=?1 AND b.thread_id=?2 AND b.volume_id=?3 AND b.generation=?4
               AND r.account_id=?5 AND r.auth_epoch=?6
               AND coalesce(json_extract(b.view,'$.reason'),'')!='browser_authority_expired'
               AND b.research_reading IS NOT NULL
             ORDER BY r.sequence DESC LIMIT ?7",
        )?
        .query_map(
            params![own.tenant, thread, own.volume, own.generation, own.account, user.epoch, AUTO_BROWSER],
            |r| r.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.reverse();
    Ok(ids)
}
fn completed_mail(reads: &[crate::MemberGoogleThreadRead]) -> Vec<&crate::MemberGoogleThreadRead> {
    reads
        .iter()
        .filter(|r| r.state == "completed" && r.source.is_some() && r.result.is_some())
        .collect()
}

struct Pin {
    digest: String,
    kind: String,
    ref_id: String,
    host: Option<String>,
}
fn pins(c: &Connection, id: &str) -> Result<Vec<Pin>> {
    Ok(c.prepare("SELECT kind,ref_id,host,digest FROM member_research_outcome_sources WHERE outcome_id=?1 ORDER BY position")?
        .query_map([id], |r| {
            Ok(Pin {
                kind: r.get(0)?,
                ref_id: r.get(1)?,
                host: r.get(2)?, digest: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
fn pin_all(
    c: &Connection,
    key: &[u8; 32],
    owner: VolumeOwner,
    user: &UserContext,
    thread: &str,
    id: &str,
    browser: &[String],
    mail: &[String],
) -> Result<()> {
    c.execute(
        "DELETE FROM member_research_outcome_sources WHERE outcome_id=?1",
        [id],
    )?;
    let time = now()?;
    let mut position = 0i64;
    for run in browser {
        let s = snapshot(c, key, owner, user, thread, run)?.ok_or(StoreError::InvalidInput)?;
        c.execute(
            "INSERT INTO member_research_outcome_sources(outcome_id,position,kind,ref_id,host,digest,pinned_at)
             VALUES(?1,?2,'browser',?3,?4,?5,?6)",
            params![id, position, run, host_of(&s.view.url), digest_text(&s.text), time],
        )?;
        position += 1;
    }
    for read in mail {
        c.execute(
            "INSERT INTO member_research_outcome_sources(outcome_id,position,kind,ref_id,host,digest,pinned_at)
             VALUES(?1,?2,'mail',?3,NULL,'',?4)",
            params![id, position, read, time],
        )?;
        position += 1;
    }
    Ok(())
}

// -------------------------------------------------------------------- view

fn closed(
    row: &Row,
    thread: &str,
    kind: ro::OutcomeKind,
    code: &str,
    text: &str,
) -> ro::OutcomeView {
    ro::OutcomeView {
        id: row.id.clone(),
        thread_id: thread.into(),
        kind,
        title: row.title.clone(),
        message: row.message.clone(),
        state: "closed".into(),
        revision: row.revision,
        criteria: row.criteria.clone(),
        citations: vec![],
        facts: vec![],
        offers: vec![],
        missing: vec![ro::Missing {
            code: code.into(),
            text: text.into(),
        }],
        conflicts: vec![],
        timing: None,
        draft: None,
        receipts: vec![],
        handoff: None,
        next: ro::Next::none(),
        question: None,
        created_at: row.created_at,
    }
}

fn view_of(
    c: &Connection,
    key: &[u8; 32],
    owner: VolumeOwner,
    user: &UserContext,
    thread: &str,
    row: &Row,
    mail: &[crate::MemberGoogleThreadRead],
) -> Result<ro::OutcomeView> {
    let kind = ro::OutcomeKind::parse(&row.kind).ok_or(StoreError::Corrupt)?;
    if row.withdrawn {
        return Ok(closed(
            row,
            thread,
            kind,
            "withdrawn",
            "이 비교는 철회되었습니다. 원본 기록은 그대로 있습니다.",
        ));
    }
    if row.auth_epoch != user.epoch {
        return Ok(closed(
            row,
            thread,
            kind,
            "authority_changed",
            "인증 상태가 바뀌어 이 비교의 근거를 닫았습니다. 새로 요청해 주세요.",
        ));
    }
    let mut citations = Vec::new();
    let mut browser = Vec::new();
    let mut mails = Vec::new();
    let mut unreadable = 0usize;
    let list = pins(c, &row.id)?;
    for (index, pin) in list.iter().enumerate() {
        let n = index as u32 + 1;
        match pin.kind.as_str() {
            "browser" => match snapshot(c, key, owner, user, thread, &pin.ref_id)? {
                Some(s) => {
                    let host = host_of(&s.view.url);
                    let changed = pin.digest != digest_text(&s.text)
                        || matches!((&pin.host, &host), (Some(p), Some(h)) if p != h);
                    citations.push(ro::Citation {
                        n,
                        kind: "browser".into(),
                        label: if s.view.title.trim().is_empty() {
                            host.clone().unwrap_or_else(|| "브라우저 페이지".into())
                        } else {
                            s.view.title.clone()
                        },
                        host: host.clone(),
                        observed_at: Some(s.observed_at),
                        status: String::from(if changed { "changed" } else { "current" }),
                        run_id: Some(pin.ref_id.clone()),
                        request_id: None,
                    });
                    // The discovery page may quote prices, eligibility and even a receipt. Only
                    // the independently opened merchant or publisher page can supply those facts.
                    if host
                        .as_deref()
                        .is_some_and(|h| h == "duckduckgo.com" || h.ends_with(".duckduckgo.com"))
                    {
                        continue;
                    }
                    browser.push(ro::BrowserEvidence {
                        cite: n,
                        run_id: pin.ref_id.clone(),
                        workspace_id: s.view.id.clone(),
                        revision: s.view.revision,
                        view_state: s.view.state.clone(),
                        reason: s.view.reason.clone(),
                        title: s.view.title.clone(),
                        host,
                        pinned_host: pin.host.clone(),
                        observed_at: s.observed_at,
                        text: if changed { String::new() } else { s.text },
                    });
                }
                None => {
                    unreadable += 1;
                    citations.push(ro::Citation {
                        n,
                        kind: "browser".into(),
                        label: "더 이상 읽을 수 없는 브라우저 기록".into(),
                        host: pin.host.clone(),
                        observed_at: None,
                        status: "withdrawn".into(),
                        run_id: Some(pin.ref_id.clone()),
                        request_id: None,
                    });
                }
            },
            _ => {
                let read = mail
                    .iter()
                    .find(|m| m.request_id.to_string() == pin.ref_id)
                    .filter(|m| m.state == "completed");
                match read.and_then(|m| m.result.as_ref().zip(m.source.as_ref())) {
                    Some((inbox, source)) => {
                        citations.push(ro::Citation {
                            n,
                            kind: "mail".into(),
                            label: format!("Gmail 받은편지함 읽기 ({})", source.mailbox),
                            host: None,
                            observed_at: read.map(|m| m.created_at),
                            status: "current".into(),
                            run_id: None,
                            request_id: Some(pin.ref_id.clone()),
                        });
                        for m in &inbox.messages {
                            mails.push(ro::MailEvidence {
                                cite: n,
                                subject: m.subject.clone(),
                                from: m.from.clone(),
                                date: m.date.clone(),
                                received_at_ms: m.received_at_ms,
                            });
                        }
                    }
                    None => {
                        unreadable += 1;
                        citations.push(ro::Citation {
                            n,
                            kind: "mail".into(),
                            label: "철회되었거나 더 이상 읽을 수 없는 메일 조회".into(),
                            host: None,
                            observed_at: None,
                            status: "withdrawn".into(),
                            run_id: None,
                            request_id: Some(pin.ref_id.clone()),
                        });
                    }
                }
            }
        }
    }
    let mut eval = ro::evaluate(ro::EvaluationInput {
        kind,
        criteria: &row.criteria,
        browser: &browser,
        mail: &mails,
        mail_expected: kind == ro::OutcomeKind::Application,
        today: ro::today_of(now()?),
        draft_requested: row.draft,
    });
    let mut question = None;
    if unreadable > 0 && !browser.is_empty() {
        eval.missing.push(ro::Missing {
            code: "source_withdrawn".into(),
            text: "일부 근거를 더 이상 읽을 수 없어 제외했습니다.".into(),
        });
    }
    if browser.is_empty() {
        // Without a readable page there is nothing to compare. Ask for a URL once, then expose the
        // existing browser; never invent a result.
        eval.facts.clear();
        eval.offers.clear();
        eval.conflicts.clear();
        eval.draft = None;
        eval.receipts.clear();
        eval.missing.retain(|m| m.code == "mail_not_read");
        eval.state = String::from(if unreadable > 0 {
            "withdrawn"
        } else if row.question.is_some() {
            "needs_input"
        } else {
            "needs_source"
        });
        eval.next = if let Some(q) = &row.question {
            question = Some(ro::Question {
                id: q.clone(),
                text: QUESTION.into(),
            });
            ro::Next {
                kind: "ask_url".into(),
                text: QUESTION.into(),
                url: None,
            }
        } else if let Some(url) = &row.pending_url {
            ro::Next {
                kind: "open_browser".into(),
                text: "이 주소를 기존 브라우저로 열어 읽은 뒤 ‘다시 확인’을 눌러 주세요.".into(),
                url: Some(url.clone()),
            }
        } else {
            ro::Next {
                kind: "open_browser".into(),
                text: "기존 브라우저로 비교할 페이지를 다시 열어 주세요.".into(),
                url: None,
            }
        };
    }
    Ok(ro::OutcomeView {
        id: row.id.clone(),
        thread_id: thread.into(),
        kind,
        title: row.title.clone(),
        message: row.message.clone(),
        state: eval.state,
        revision: row.revision,
        criteria: row.criteria.clone(),
        citations,
        facts: eval.facts,
        offers: eval.offers,
        missing: eval.missing,
        conflicts: eval.conflicts,
        timing: eval.timing,
        draft: eval.draft,
        receipts: eval.receipts,
        handoff: eval.handoff,
        next: eval.next,
        question,
        created_at: row.created_at,
    })
}

fn fetch(c: &Connection, owner: VolumeOwner, thread: &str, id: &str) -> Result<Option<Row>> {
    let own = Own::of(owner);
    Ok(c.query_row(
        &format!("{ROW} WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND thread_id=?6"),
        params![id, own.tenant, own.account, own.volume, own.generation, thread],
        row_of,
    )
    .optional()?)
}
/// An id that exists under another owner, volume, generation or thread is a conflict, not a lookup.
fn foreign(c: &Connection, owner: VolumeOwner, thread: &str, id: &str) -> Result<bool> {
    let own = Own::of(owner);
    let other: Option<bool> = c
        .query_row(
            "SELECT NOT(tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND thread_id=?6)
             FROM member_research_outcomes WHERE id=?1",
            params![id, own.tenant, own.account, own.volume, own.generation, thread],
            |r| r.get(0),
        )
        .optional()?;
    Ok(other == Some(true))
}

// ------------------------------------------------------------------ requests

fn parse_request(request: &ro::OutcomeRequest) -> Result<()> {
    canonical(&request.request_id)?;
    let shopping = request.kind == ro::OutcomeKind::Shopping;
    if !message_ok(&request.message)
        || !request.criteria.valid()
        || request.browser_runs.len() > ro::MAX_BROWSER_SOURCES
        || request.mail_reads.len() > ro::MAX_MAIL_SOURCES
        || (shopping && !request.mail_reads.is_empty())
    {
        return Err(StoreError::InvalidInput);
    }
    for id in request.browser_runs.iter().chain(request.mail_reads.iter()) {
        canonical(id)?;
    }
    let mut seen = request.browser_runs.clone();
    seen.sort();
    seen.dedup();
    if seen.len() != request.browser_runs.len() {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn title_of(request: &ro::OutcomeRequest) -> String {
    let head: String = request.message.trim().chars().take(40).collect();
    let head = head.replace(['\n', '\r', '\t'], " ");
    match request.kind {
        ro::OutcomeKind::Shopping => format!("구매 비교 · {head}"),
        ro::OutcomeKind::Application => format!("지원 준비 · {head}"),
    }
}

/// Every mail read this outcome may cite must be a completed read of this thread.
fn mail_ids(
    reads: &[crate::MemberGoogleThreadRead],
    thread: &str,
    explicit: &[String],
) -> Result<Vec<String>> {
    let ok = completed_mail(reads);
    if explicit.is_empty() {
        return Ok(ok
            .iter()
            .filter(|r| r.thread_id.to_string() == thread)
            .take(AUTO_MAIL)
            .map(|r| r.request_id.to_string())
            .collect());
    }
    for id in explicit {
        if !ok
            .iter()
            .any(|r| r.request_id.to_string() == *id && r.thread_id.to_string() == thread)
        {
            return Err(StoreError::InvalidInput);
        }
    }
    Ok(explicit.to_vec())
}

fn outcome_mail(
    c: &Connection,
    key: &[u8; 32],
    user: &UserContext,
    owner: VolumeOwner,
    thread: &str,
    kind: ro::OutcomeKind,
) -> Result<Vec<crate::MemberGoogleThreadRead>> {
    if kind == ro::OutcomeKind::Shopping {
        return Ok(Vec::new());
    }
    crate::mail::member_thread_read_history(
        c,
        key,
        user,
        owner,
        Uuid::parse_str(thread).map_err(|_| StoreError::InvalidInput)?,
    )
}

impl Store {
    pub async fn research_outcome_page(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        thread: String,
        request: Option<Uuid>,
    ) -> Result<ro::OutcomePage> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            expire_workers(&tx, owner)?;
            let own = Own::of(owner);
            let ids: Vec<String> = match request {
                Some(id) => {
                    let id = id.to_string();
                    if fetch(&tx, owner, &thread, &id)?.is_none() {
                        return Err(StoreError::NotFound);
                    }
                    vec![id]
                }
                None => {
                    let mut recent = tx
                        .prepare(
                            "SELECT id FROM member_research_outcomes
                             WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4 AND thread_id=?5
                             ORDER BY admitted_order DESC LIMIT ?6",
                        )?
                        .query_map(
                            params![own.tenant, own.account, own.volume, own.generation, thread, PAGE],
                            |r| r.get::<_, String>(0),
                        )?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    recent.reverse();
                    recent
                }
            };
            let mut outcomes = Vec::new();
            let mut mail = None;
            for id in ids {
                let row = fetch(&tx, owner, &thread, &id)?.ok_or(StoreError::NotFound)?;
                if row.kind == "application" && mail.is_none() {
                    mail = Some(outcome_mail(&tx, &db.provider_key, &user, owner, &thread, ro::OutcomeKind::Application)?);
                }
                outcomes.push(view_of(&tx, &db.provider_key, owner, &user, &thread, &row, mail.as_deref().unwrap_or(&[]))?);
            }
            tx.commit()?;
            Ok(ro::OutcomePage { thread_id: thread, outcomes })
        })
        .await
    }

    pub async fn create_research_outcome(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: String,
        request: ro::OutcomeRequest,
    ) -> Result<ro::OutcomeView> {
        parse_request(&request)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            expire_workers(&tx, owner)?;
            let digest = Sha256::digest(
                serde_json::to_vec(&(&thread, &request)).map_err(|_| StoreError::InvalidInput)?,
            );
            if foreign(&tx, owner, &thread, &request.request_id)? {
                return Err(StoreError::Conflict);
            }
            let mail = outcome_mail(&tx, &db.provider_key, &user, owner, &thread, request.kind)?;
            if let Some(row) = fetch(&tx, owner, &thread, &request.request_id)? {
                // A repeated request answers with the stored outcome; a different body is a conflict.
                if row.digest != digest.as_slice() {
                    return Err(StoreError::Conflict);
                }
                let view = view_of(&tx, &db.provider_key, owner, &user, &thread, &row, &mail)?;
                tx.commit()?;
                return Ok(view);
            }
            let own = Own::of(owner);
            let active: i64 = tx.query_row(
                "SELECT count(*) FROM member_research_outcomes WHERE tenant_id=?1 AND account_id=?2
                 AND volume_id=?3 AND generation=?4 AND withdrawn=0",
                params![own.tenant, own.account, own.volume, own.generation],
                |r| r.get(0),
            )?;
            if active >= MAX_ACTIVE {
                return Err(StoreError::BudgetExhausted);
            }
            let auto = request.browser_runs.is_empty() && request.mail_reads.is_empty();
            let browser = if request.browser_runs.is_empty() {
                default_browser(&tx, owner, &user, &thread)?
            } else {
                request.browser_runs.clone()
            };
            let mails = if request.kind == ro::OutcomeKind::Application {
                mail_ids(&mail, &thread, &request.mail_reads)?
            } else {
                Vec::new()
            };
            let urls: Vec<_> = request.message.split_whitespace().filter_map(|part| {
                let token = part.trim_matches(|c| "()<>,.!?[]‘’“”".contains(c) || c == '"');
                public_url(token).ok()
            }).collect();
            let url = (urls.len() == 1).then(|| urls[0].as_str());
            let question = (browser.is_empty() && url.is_none()).then(|| Uuid::new_v4().to_string());
            let time = now()?;
            tx.execute(
                "INSERT INTO member_research_outcomes(id,tenant_id,account_id,volume_id,generation,thread_id,kind,title,message,
                    criteria,request_digest,auth_epoch,host_epoch,auto_sources,question_id,created_at,updated_at,admitted_order)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?16,?17)",
                params![
                    request.request_id,
                    own.tenant,
                    own.account,
                    own.volume,
                    own.generation,
                    thread,
                    request.kind.as_str(),
                    title_of(&request),
                    request.message,
                    serde_json::to_string(&request.criteria).map_err(|_| StoreError::InvalidInput)?,
                    digest.as_slice(),
                    user.epoch,
                    host_epoch,
                    auto,
                    question,
                    time,
                    chat::next_order(&tx)?
                ],
            )?;
            let draft = request.kind == ro::OutcomeKind::Application && ["초안", "draft"].iter().any(|word| request.message.to_lowercase().contains(word));
            tx.execute("UPDATE member_research_outcomes SET pending_url=?2,draft_requested=?3 WHERE id=?1",
                       params![request.request_id,url,draft])?;
            super::activity::admit(&tx,owner,&user,&thread,&request.request_id,request.kind.as_str(),&request.message,
                if question.is_some() { "needs_input" } else { "ready" })?;
            pin_all(&tx, &db.provider_key, owner, &user, &thread, &request.request_id, &browser, &mails)?;
            let row = fetch(&tx, owner, &thread, &request.request_id)?.ok_or(StoreError::Corrupt)?;
            let view = view_of(&tx, &db.provider_key, owner, &user, &thread, &row, &mail)?;
            tx.commit()?;
            Ok(view)
        })
        .await
    }

    pub async fn control_research_outcome(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: String,
        action: ro::OutcomeAction,
    ) -> Result<ro::OutcomeView> {
        canonical(&action.request_id)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            expire_workers(&tx, owner)?;
            let row = fetch(&tx, owner, &thread, &action.request_id)?.ok_or(StoreError::NotFound)?;
            let kind = ro::OutcomeKind::parse(&row.kind).ok_or(StoreError::Corrupt)?;
            let mail = outcome_mail(&tx, &db.provider_key, &user, owner, &thread, kind)?;
            // An older in-flight gateway cannot change what a newer grant already touched.
            if host_epoch < row.host_epoch || action.revision <= 0 {
                return Err(StoreError::Conflict);
            }
            let name = serde_json::to_value(action.action)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or(StoreError::InvalidInput)?;
            let argument = match action.action {
                ro::OutcomeActionKind::Refine => {
                    let criteria = action.criteria.as_ref().ok_or(StoreError::InvalidInput)?;
                    if !criteria.valid() {
                        return Err(StoreError::InvalidInput);
                    }
                    serde_json::to_string(criteria).map_err(|_| StoreError::InvalidInput)?
                }
                ro::OutcomeActionKind::Answer => {
                    let question = action.question_id.as_deref().ok_or(StoreError::InvalidInput)?;
                    canonical(question)?;
                    format!("{question}\n{}", public_url(action.text.as_deref().ok_or(StoreError::InvalidInput)?)?)
                }
                ro::OutcomeActionKind::Draft => {
                    if kind != ro::OutcomeKind::Application {
                        return Err(StoreError::InvalidInput);
                    }
                    String::new()
                }
                ro::OutcomeActionKind::Refresh | ro::OutcomeActionKind::Withdraw => String::new(),
            };
            let applied: Option<(String, String)> = tx
                .query_row(
                    "SELECT action,arg FROM member_research_outcome_actions WHERE outcome_id=?1 AND from_revision=?2",
                    params![row.id, action.revision],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((done, arg)) = applied {
                if done == name && arg == argument {
                    let view = view_of(&tx, &db.provider_key, owner, &user, &thread, &row, &mail)?;
                    tx.commit()?;
                    return Ok(view);
                }
                return Err(StoreError::Conflict);
            }
            if row.revision != action.revision || row.withdrawn || row.auth_epoch != user.epoch {
                return Err(StoreError::Conflict);
            }
            let time = now()?;
            match action.action {
                ro::OutcomeActionKind::Refine => {
                    tx.execute(
                        "UPDATE member_research_outcomes SET criteria=?2 WHERE id=?1",
                        params![row.id, argument],
                    )?;
                }
                ro::OutcomeActionKind::Refresh => {
                    if row.auto {
                        // Adopt whatever is readable now. A changed page host is NOT accepted by
                        // refreshing: the member starts a new comparison for a different source.
                        let current = pins(&tx, &row.id)?;
                        let keep: Vec<&Pin> = current.iter().filter(|p| p.host.is_some()).collect();
                        let mut browser = default_browser(&tx, owner, &user, &thread)?;
                        for pin in keep {
                            if !browser.contains(&pin.ref_id)
                                && snapshot(&tx, &db.provider_key, owner, &user, &thread, &pin.ref_id)?.is_some()
                            {
                                browser.insert(0, pin.ref_id.clone());
                            }
                        }
                        browser.truncate(ro::MAX_BROWSER_SOURCES);
                        let mails = if kind == ro::OutcomeKind::Application {
                            mail_ids(&mail, &thread, &[])?
                        } else {
                            Vec::new()
                        };
                        // Keep pinned hosts of sources that are still the same run.
                        let hosts: Vec<(String, Option<String>)> =
                            current.iter().map(|p| (p.ref_id.clone(), p.host.clone())).collect();
                        pin_all(&tx, &db.provider_key, owner, &user, &thread, &row.id, &browser, &mails)?;
                        for (ref_id, host) in hosts {
                            if host.is_some() {
                                tx.execute(
                                    "UPDATE member_research_outcome_sources SET host=?3 WHERE outcome_id=?1 AND ref_id=?2",
                                    params![row.id, ref_id, host],
                                )?;
                            }
                        }
                    }
                }
                ro::OutcomeActionKind::Draft => {
                    tx.execute(
                        "UPDATE member_research_outcomes SET draft_requested=1 WHERE id=?1",
                        [&row.id],
                    )?;
                }
                ro::OutcomeActionKind::Answer => {
                    let (question, url) = argument.split_once('\n').ok_or(StoreError::InvalidInput)?;
                    if row.question.as_deref() != Some(question) {
                        return Err(StoreError::Conflict);
                    }
                    tx.execute(
                        "UPDATE member_research_outcomes SET question_id=NULL,pending_url=?2 WHERE id=?1",
                        params![row.id, url],
                    )?;
                }
                ro::OutcomeActionKind::Withdraw => {
                    tx.execute(
                        "UPDATE member_research_outcomes SET withdrawn=1 WHERE id=?1",
                        [&row.id],
                    )?;
                    tx.execute(
                        "DELETE FROM member_research_outcome_sources WHERE outcome_id=?1",
                        [&row.id],
                    )?;
                }
            }
            tx.execute(
                "UPDATE member_research_outcomes SET revision=revision+1,updated_at=?2,host_epoch=max(host_epoch,?3) WHERE id=?1",
                params![row.id, time, host_epoch],
            )?;
            tx.execute(
                "INSERT INTO member_research_outcome_actions(outcome_id,from_revision,action,arg) VALUES(?1,?2,?3,?4)",
                params![row.id, action.revision, name, argument],
            )?;
            let row = fetch(&tx, owner, &thread, &row.id)?.ok_or(StoreError::Corrupt)?;
            let view = view_of(&tx, &db.provider_key, owner, &user, &thread, &row, &mail)?;
            tx.commit()?;
            Ok(view)
        })
        .await
    }
}

#[cfg(test)]
mod tests;
