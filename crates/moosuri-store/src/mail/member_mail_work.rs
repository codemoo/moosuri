//! Member-private Gmail research -> local reply draft.
//!
//! A work row is derived from exactly one completed member readonly thread read
//! (`member_google_thread_reads`) and one message of that snapshot. The cited
//! source metadata and the draft text are encrypted in the row, never enter chat
//! history, recall or memory, and are destroyed when the readonly source is
//! disconnected, replaced, or the session epoch changes.
//!
//! Provider effects reuse the standard ledger: the final content is an ordinary
//! `mail_actions` row (pending_approval -> approved -> standard dispatcher) on an
//! independently authorized standard `mail_connections` row for the SAME mailbox,
//! and a Gmail draft is written only by the standard journaled adapter. This
//! module never mints a provider receipt: it only records what the adapter
//! returns, and an uncertain write stays `unknown`.
use super::*;
use crate::personal_volume::VolumeOwner;
use moosuri_core::{GmailDraftRef, GmailDraftSave};
use serde::{Deserialize, Serialize};

pub const MEMBER_MAIL_WORK_MAX_BODY: usize = 12_000;
const DOMAIN: &str = "moosuri/member-mail-work/v1";
const MAX_SUBJECT: usize = 256;
const MAX_RECIPIENTS: usize = 5;
const MAX_ROWS: i64 = 256;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailWorkInput {
    /// Client request id: the replay key of this preparation.
    pub id: Uuid,
    pub thread_id: Uuid,
    pub read_request_id: Uuid,
    pub message_id: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub body: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailWorkPatch {
    #[serde(default)]
    pub to: Option<Vec<String>>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}
/// Exact work revision plus the exact final-content approval being acted on.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailWorkApproval {
    pub revision: i64,
    pub action_revision: i64,
    pub action_digest: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailWorkSource {
    pub read_request_id: Uuid,
    pub connection_id: Uuid,
    pub epoch: i64,
    pub mailbox: String,
    pub message_id: String,
    pub subject: String,
    pub from: String,
    pub date: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailWorkDraft {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailConnectionChoice {
    pub connection_id: String,
    pub epoch: i64,
    pub mailbox: String,
    pub can_save_draft: bool,
    pub can_send: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailPreview {
    pub action_id: String,
    pub action_revision: i64,
    pub action_digest: String,
    pub connection_id: String,
    pub connection_epoch: i64,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub expires_at: i64,
    pub action_state: String,
    /// The metadata-only snapshot carries no RFC Message-ID/Gmail thread id, so
    /// the provider message is never threaded. Stated, not hidden.
    pub threaded: bool,
    pub can_save_draft: bool,
    pub can_approve_send: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemberMailDelivery {
    /// Local draft only. `reason`: readonly_only | mailbox_mismatch |
    /// connection_inactive | wrong_scope.
    NeedsConnection {
        reason: String,
        requirements: Vec<String>,
    },
    Ready {
        connections: Vec<MemberMailConnectionChoice>,
    },
    FinalPreview {
        preview: MemberMailPreview,
    },
    Approved {
        preview: MemberMailPreview,
        gmail_draft_id: Option<String>,
    },
    /// kind: gmail_draft | sent. Only adapter/ledger-returned ids appear here.
    Receipt {
        kind: String,
        provider_draft_id: Option<String>,
        provider_message_id: Option<String>,
        provider_thread_id: Option<String>,
        preview: Option<MemberMailPreview>,
    },
    /// operation: gmail_draft | send. Never retried as a blind second write.
    Unknown {
        operation: String,
        preview: Option<MemberMailPreview>,
    },
    Failed {
        operation: String,
        reason: String,
    },
    Stale {
        reason: String,
    },
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMailWork {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub revision: i64,
    pub digest: String,
    /// local_draft | withdrawn
    pub state: String,
    pub source: Option<MemberMailWorkSource>,
    pub draft: Option<MemberMailWorkDraft>,
    pub delivery: MemberMailDelivery,
    pub created_at: i64,
    pub updated_at: i64,
}
impl MemberMailWork {
    pub fn valid(&self) -> bool {
        let live = self.state == "local_draft";
        !self.id.is_nil()
            && !self.thread_id.is_nil()
            && self.revision > 0
            && self.digest.len() == 64
            && (live || self.state == "withdrawn")
            && live == (self.source.is_some() && self.draft.is_some())
            && self.draft.as_ref().is_none_or(|d| {
                d.body.len() <= MEMBER_MAIL_WORK_MAX_BODY
                    && d.subject.chars().count() <= MAX_SUBJECT
                    && !d.to.is_empty()
                    && d.to.len() <= MAX_RECIPIENTS
            })
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum MemberMailWorkPrepared {
    Draft {
        work: Box<MemberMailWork>,
    },
    /// Nothing was persisted. `missing`: to | body.
    NeedsDetails {
        missing: Vec<String>,
    },
    /// The cited read is not (or no longer) a live completed source.
    SourceUnavailable {
        read_state: String,
    },
}
pub enum MemberMailSaveAdmission {
    Recorded(Box<MemberMailWork>),
    Gone(Box<MemberMailWork>),
    Fresh(MemberMailSaveLease),
}
/// Authority to call the standard provider-draft adapter exactly for this action.
pub struct MemberMailSaveLease {
    user: UserContext,
    owner: VolumeOwner,
    id: Uuid,
    attempt: String,
    action_id: String,
    action_digest: String,
    request: GmailDraftSave,
}
impl MemberMailSaveLease {
    pub fn user(&self) -> &UserContext {
        &self.user
    }
    pub fn action_id(&self) -> &str {
        &self.action_id
    }
    pub fn request(&self) -> &GmailDraftSave {
        &self.request
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cited {
    message_id: String,
    subject: String,
    from: String,
    date: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    cited: Cited,
    to: Vec<String>,
    subject: String,
    body: String,
}
struct ActionLink {
    id: String,
    revision: i64,
    digest: String,
    connection_id: String,
    connection_epoch: i64,
}
struct Work {
    id: Uuid,
    thread_id: String,
    read_request_id: String,
    volume: String,
    generation: i64,
    auth_epoch: i64,
    source_connection: String,
    source_epoch: i64,
    app: String,
    mailbox: String,
    message_id: String,
    revision: i64,
    digest: String,
    phase: String,
    cipher: Option<Vec<u8>>,
    action: Option<ActionLink>,
    attempt: Option<String>,
    /// (draft id, message id, action digest) exactly as returned by the adapter.
    draft: Option<(String, String, String)>,
    created: i64,
    updated: i64,
}
struct Pin {
    connection: String,
    epoch: i64,
    app: String,
    mailbox: String,
    volume: String,
    generation: i64,
    read: String,
}
impl Work {
    fn pin(&self) -> Pin {
        Pin {
            connection: self.source_connection.clone(),
            epoch: self.source_epoch,
            app: self.app.clone(),
            mailbox: self.mailbox.clone(),
            volume: self.volume.clone(),
            generation: self.generation,
            read: self.read_request_id.clone(),
        }
    }
}
struct Conn {
    id: String,
    email: String,
    epoch: i64,
    active: bool,
    native: bool,
    compose: bool,
    send: bool,
}
enum Built {
    Ready(Payload),
    Missing(Vec<&'static str>),
}
enum Gate {
    Live(Box<Work>),
    Gone(Box<Work>),
}

fn uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|_| StoreError::Corrupt)
}
fn authority(c: &Connection, u: &UserContext, o: VolumeOwner) -> Result<UserContext> {
    o.check_identity(c, None)?;
    let current = validate(c, u, true, false)?;
    if current.account.id != o.account.to_string()
        || current.tenant.0 != o.tenant.to_string()
        || current.helper
    {
        return Err(StoreError::Forbidden);
    }
    Ok(current)
}
fn owned(w: Work, o: VolumeOwner) -> Result<Work> {
    if w.volume != o.volume.to_string() || w.generation != o.generation as i64 {
        return Err(StoreError::Conflict);
    }
    Ok(w)
}
fn message_id_valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn text_ok(s: &str, multiline: bool) -> bool {
    !s.chars()
        .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
}
/// One bare mailbox out of `Name <a@b.c>` or `a@b.c`. Anything else is refused:
/// no lists, groups, comments or header-ish punctuation.
fn address(raw: &str) -> Option<String> {
    let t = raw.trim();
    let inner = match (t.rfind('<'), t.rfind('>')) {
        (Some(a), Some(b)) if a < b && b == t.len() - 1 => &t[a + 1..b],
        (None, None) => t,
        _ => return None,
    };
    let email = email_valid(inner.to_owned()).ok()?;
    let (local, domain) = email.split_once('@')?;
    (!local.is_empty()
        && !email.contains(['<', '>', ',', ';', '"', '(', ')', '\\', '[', ']'])
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.'))
    .then_some(email)
}
fn recipients(list: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in list {
        let a = address(raw).ok_or(StoreError::InvalidInput)?;
        if !out.contains(&a) {
            out.push(a);
        }
    }
    if out.len() > MAX_RECIPIENTS {
        return Err(StoreError::InvalidInput);
    }
    Ok(out)
}
fn subject_ok(s: &str) -> Result<String> {
    let s = s.trim();
    if s.is_empty() || s.chars().count() > MAX_SUBJECT || !text_ok(s, false) {
        return Err(StoreError::InvalidInput);
    }
    Ok(s.to_owned())
}
fn body_ok(s: &str) -> Result<String> {
    if s.len() > MEMBER_MAIL_WORK_MAX_BODY || !text_ok(s, true) {
        return Err(StoreError::InvalidInput);
    }
    Ok(s.trim().to_owned())
}
fn default_subject(cited: &str) -> String {
    let s = cited.trim();
    let s = if s.is_empty() { "(no subject)" } else { s };
    let s = if s.to_ascii_lowercase().starts_with("re:") {
        s.to_owned()
    } else {
        format!("Re: {s}")
    };
    s.chars().take(MAX_SUBJECT).collect()
}
/// Invalid values are errors; absent required details are a typed `Missing`.
fn build(cited: &Cited, to: &[String], subject: Option<&str>, body: &str) -> Result<Built> {
    let body = body_ok(body)?;
    let mut to = recipients(to)?;
    if to.is_empty() {
        to.extend(address(&cited.from));
    }
    let subject = match subject {
        Some(s) => subject_ok(s)?,
        None => default_subject(&cited.subject),
    };
    let mut missing = Vec::new();
    if to.is_empty() {
        missing.push("to");
    }
    if body.is_empty() {
        missing.push("body");
    }
    if !missing.is_empty() {
        return Ok(Built::Missing(missing));
    }
    Ok(Built::Ready(Payload {
        cited: cited.clone(),
        to,
        subject,
        body,
    }))
}
fn content_digest(thread: &str, read: &str, mailbox: &str, p: &Payload) -> Result<String> {
    let mut h = Sha256::new();
    h.update(DOMAIN.as_bytes());
    h.update(b"/content\0");
    h.update(
        serde_json::to_vec(&(thread, read, mailbox, p)).map_err(|_| StoreError::InvalidInput)?,
    );
    Ok(format!("{:x}", h.finalize()))
}
fn seal_aad(u: &UserContext, w: &Work) -> Vec<u8> {
    format!(
        "{DOMAIN}/{}/{}/{}/{}/{}/{}/{}/{}/{}/{}/{}",
        u.tenant.0,
        u.account.id,
        w.id,
        w.thread_id,
        w.read_request_id,
        w.volume,
        w.generation,
        w.source_connection,
        w.source_epoch,
        w.revision,
        w.message_id
    )
    .into_bytes()
}
fn seal(key: &[u8; 32], u: &UserContext, w: &Work, p: &Payload) -> Result<Vec<u8>> {
    let bytes = Zeroizing::new(serde_json::to_vec(p).map_err(|_| StoreError::InvalidInput)?);
    if bytes.len() > 65536 {
        return Err(StoreError::InvalidInput);
    }
    providers::encrypt(key, &bytes, &seal_aad(u, w))
}
fn open_payload(key: &[u8; 32], u: &UserContext, w: &Work) -> Result<Payload> {
    let plain = providers::decrypt(
        key,
        w.cipher.as_deref().ok_or(StoreError::Conflict)?,
        &seal_aad(u, w),
    )?;
    serde_json::from_slice(&plain).map_err(|_| StoreError::Corrupt)
}

const COLUMNS: &str = "thread_id,read_request_id,volume_id,generation,auth_epoch,source_connection_id,source_epoch,app_revision,source_mailbox,source_message_id,revision,digest,phase,cipher,action_id,action_revision,action_digest,mail_connection_id,mail_connection_epoch,provider_attempt,provider_draft_id,provider_draft_message_id,provider_draft_digest,created_at,updated_at";
fn load_opt(c: &Connection, u: &UserContext, id: Uuid) -> Result<Option<Work>> {
    Ok(c.query_row(
        &format!(
            "SELECT {COLUMNS} FROM member_mail_work WHERE tenant_id=?1 AND account_id=?2 AND id=?3"
        ),
        params![u.tenant.0, u.account.id, id.to_string()],
        |r| {
            let action = match r.get::<_, Option<String>>(14)? {
                Some(aid) => Some(ActionLink {
                    id: aid,
                    revision: r.get(15)?,
                    digest: r.get(16)?,
                    connection_id: r.get(17)?,
                    connection_epoch: r.get(18)?,
                }),
                None => None,
            };
            let draft = match r.get::<_, Option<String>>(20)? {
                Some(d) => Some((d, r.get::<_, String>(21)?, r.get::<_, String>(22)?)),
                None => None,
            };
            Ok(Work {
                id,
                thread_id: r.get(0)?,
                read_request_id: r.get(1)?,
                volume: r.get(2)?,
                generation: r.get(3)?,
                auth_epoch: r.get(4)?,
                source_connection: r.get(5)?,
                source_epoch: r.get(6)?,
                app: r.get(7)?,
                mailbox: r.get(8)?,
                message_id: r.get(9)?,
                revision: r.get(10)?,
                digest: r.get(11)?,
                phase: r.get(12)?,
                cipher: r.get(13)?,
                action,
                attempt: r.get(19)?,
                draft,
                created: r.get(23)?,
                updated: r.get(24)?,
            })
        },
    )
    .optional()?)
}
fn load(c: &Connection, u: &UserContext, id: Uuid) -> Result<Work> {
    load_opt(c, u, id)?.ok_or(StoreError::NotFound)
}

/// Live permission gate: the exact readonly grant, app, epoch, session epoch and
/// completed read must all still be the ones this work was derived from.
fn live(c: &Connection, u: &UserContext, p: &Pin) -> Result<bool> {
    type Grant = (
        Option<String>,
        i64,
        String,
        String,
        i64,
        Option<String>,
        String,
        i64,
    );
    let grant: Option<Grant> = c
        .query_row(
            "SELECT connection_id,epoch,app_revision,status,auth_epoch,mailbox,volume_id,generation FROM member_google_grants WHERE tenant_id=?1 AND account_id=?2 AND service='gmail'",
            params![u.tenant.0, u.account.id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .optional()?;
    let Some((connection, epoch, app, status, auth, mailbox, volume, generation)) = grant else {
        return Ok(false);
    };
    if connection.as_deref() != Some(p.connection.as_str())
        || epoch != p.epoch
        || app != p.app
        || status != "active"
        || auth != u.epoch
        || !mailbox.is_some_and(|m| m.eq_ignore_ascii_case(&p.mailbox))
        || volume != p.volume
        || generation != p.generation
    {
        return Ok(false);
    }
    let phase: Option<String> = c
        .query_row(
            "SELECT phase FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",
            params![u.tenant.0, u.account.id, p.read],
            |r| r.get(0),
        )
        .optional()?;
    Ok(phase.as_deref() == Some("completed"))
}
fn cancel_action(c: &Connection, u: &UserContext, w: &Work) -> Result<()> {
    if let Some(l) = &w.action {
        cancel_action_id(c, &u.tenant.0, &u.account.id, &l.id)?;
    }
    Ok(())
}
/// Only an unsent (pending/approved) action is cancelled; dispatched, sent and
/// unknown actions and their receipts are history and stay untouched.
fn cancel_action_id(c: &Connection, tenant: &str, account: &str, action: &str) -> Result<()> {
    let n = c.execute(
        "UPDATE mail_actions SET version=version+1,state='cancelled',content=NULL,raw=NULL WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state IN('pending_approval','approved')",
        params![tenant, account, action],
    )?;
    if n > 0 {
        c.execute(
            "UPDATE mail_send_evidence SET withdrawn=1 WHERE tenant_id=?1 AND action_id=?2",
            params![tenant, action],
        )?;
        crate::learned_skills::draft::invalidate_action(c, tenant, action)?;
    }
    Ok(())
}
/// Eager form of `refresh`'s lazy withdrawal, for every transition that loses the
/// member Gmail source (disconnect, app replacement, refused refresh, restart).
/// Scoped to one member; other members' work and finished history are untouched.
pub(super) fn withdraw_account(c: &Connection, tenant: &str, account: &str) -> Result<()> {
    let rows: Vec<(String, Option<String>)> = c
        .prepare("SELECT id,action_id FROM member_mail_work WHERE tenant_id=?1 AND account_id=?2 AND phase!='withdrawn'")?
        .query_map(params![tenant, account], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (id, action) in rows {
        if let Some(a) = action {
            cancel_action_id(c, tenant, account, &a)?;
        }
        c.execute(
            "UPDATE member_mail_work SET phase='withdrawn',cipher=NULL,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND phase!='withdrawn'",
            params![tenant, account, id],
        )?;
    }
    Ok(())
}
/// Destroys the cited metadata and draft text and cancels any unsent final
/// approval. Provider ids already returned by an adapter are kept as facts.
fn withdraw(c: &Connection, u: &UserContext, w: &mut Work) -> Result<()> {
    cancel_action(c, u, w)?;
    c.execute(
        "UPDATE member_mail_work SET phase='withdrawn',cipher=NULL,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND phase!='withdrawn'",
        params![u.tenant.0, u.account.id, w.id.to_string()],
    )?;
    w.phase = "withdrawn".into();
    w.cipher = None;
    Ok(())
}
fn refresh(c: &Connection, u: &UserContext, mut w: Work) -> Result<Work> {
    if w.phase == "local_draft" && (w.auth_epoch != u.epoch || !live(c, u, &w.pin())?) {
        withdraw(c, u, &mut w)?;
    }
    Ok(w)
}
fn gate(
    c: &Connection,
    u: &UserContext,
    o: VolumeOwner,
    id: Uuid,
    rev: Option<i64>,
) -> Result<Gate> {
    let w = refresh(c, u, owned(load(c, u, id)?, o)?)?;
    if w.phase != "local_draft" {
        return Ok(Gate::Gone(Box::new(w)));
    }
    if rev.is_some_and(|r| r != w.revision) {
        return Err(StoreError::Conflict);
    }
    Ok(Gate::Live(Box::new(w)))
}
fn conns(c: &Connection, u: &UserContext) -> Result<Vec<Conn>> {
    type Raw = (String, String, i64, String, String, bool);
    let rows: Vec<Raw> = c
        .prepare("SELECT id,email,epoch,status,scopes,native_send FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 ORDER BY id LIMIT 16")?
        .query_map(params![u.tenant.0, u.account.id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, email, epoch, status, scopes, native)| {
            let scopes: Vec<String> = parse(&scopes)?;
            Ok(Conn {
                id,
                email,
                epoch,
                active: status == "active",
                native,
                compose: scopes
                    .iter()
                    .any(|s| s == moosuri_connectors::gmail::COMPOSE_SCOPE),
                send: scopes
                    .iter()
                    .any(|s| s == moosuri_connectors::gmail::SEND_SCOPE),
            })
        })
        .collect()
}
fn compatible(c: &Conn, mailbox: &str) -> bool {
    c.active && !c.native && c.email.eq_ignore_ascii_case(mailbox) && (c.compose || c.send)
}
fn requirements() -> Vec<String> {
    vec![
        "standard_gmail_connection_same_mailbox".into(),
        "scope_gmail_send_to_approve_and_send".into(),
        "scope_gmail_compose_to_save_gmail_draft".into(),
    ]
}
fn unavailable(all: &[Conn], mailbox: &str) -> MemberMailDelivery {
    let standard: Vec<&Conn> = all.iter().filter(|c| !c.native).collect();
    let same = |c: &&Conn| c.email.eq_ignore_ascii_case(mailbox);
    let reason = if standard.is_empty() {
        "readonly_only"
    } else if !standard.iter().any(same) {
        "mailbox_mismatch"
    } else if !standard.iter().any(|c| same(c) && c.active) {
        "connection_inactive"
    } else {
        "wrong_scope"
    };
    MemberMailDelivery::NeedsConnection {
        reason: reason.into(),
        requirements: requirements(),
    }
}
fn preview(
    a: &MailAction,
    l: &ActionLink,
    conn: Option<&Conn>,
    w: &Work,
    at: i64,
) -> Option<MemberMailPreview> {
    let content = a.content.as_ref()?;
    let usable = conn.filter(|c| c.active && !c.native && c.epoch == l.connection_epoch);
    let open = matches!(a.state.as_str(), "pending_approval" | "approved") && a.expires_at > at;
    Some(MemberMailPreview {
        action_id: a.id.clone(),
        action_revision: a.revision,
        action_digest: a.digest.clone(),
        connection_id: a.connection_id.clone(),
        connection_epoch: l.connection_epoch,
        from: content.from.clone(),
        to: content.to.clone(),
        subject: content.subject.clone(),
        body: content.text.clone(),
        expires_at: a.expires_at,
        action_state: a.state.clone(),
        threaded: false,
        can_save_draft: open
            && usable.is_some_and(|c| c.compose)
            && w.draft.is_none()
            && w.attempt.is_none(),
        can_approve_send: open && a.state == "pending_approval" && usable.is_some_and(|c| c.send),
    })
}
fn delivery(c: &Connection, u: &UserContext, w: &Work) -> Result<MemberMailDelivery> {
    let live_work = w.phase == "local_draft";
    let all = conns(c, u)?;
    let Some(l) = &w.action else {
        if !live_work {
            return Ok(MemberMailDelivery::Stale {
                reason: "source_withdrawn".into(),
            });
        }
        let connections: Vec<_> = all
            .iter()
            .filter(|x| compatible(x, &w.mailbox))
            .map(|x| MemberMailConnectionChoice {
                connection_id: x.id.clone(),
                epoch: x.epoch,
                mailbox: x.email.clone(),
                can_save_draft: x.compose,
                can_send: x.send,
            })
            .collect();
        return Ok(if connections.is_empty() {
            unavailable(&all, &w.mailbox)
        } else {
            MemberMailDelivery::Ready { connections }
        });
    };
    let a = super::action(c, u, &l.id)?;
    let at = now()?;
    let conn = all.iter().find(|x| x.id == l.connection_id);
    let shown = if live_work {
        preview(&a, l, conn, w, at)
    } else {
        None
    };
    Ok(match a.state.as_str() {
        "sent" => MemberMailDelivery::Receipt {
            kind: "sent".into(),
            provider_draft_id: None,
            provider_message_id: a.provider_message_id.clone(),
            provider_thread_id: a.provider_thread_id.clone(),
            preview: shown,
        },
        "outcome_unknown" => MemberMailDelivery::Unknown {
            operation: "send".into(),
            preview: shown,
        },
        "failed_definite" => MemberMailDelivery::Failed {
            operation: "send".into(),
            reason: "provider_rejected".into(),
        },
        "pending_approval" | "approved" | "dispatch_committed" if !live_work => {
            MemberMailDelivery::Stale {
                reason: "source_withdrawn".into(),
            }
        }
        "pending_approval" | "approved" | "dispatch_committed" => {
            if a.revision != l.revision || a.digest != l.digest {
                MemberMailDelivery::Stale {
                    reason: "action_changed".into(),
                }
            } else if a.state != "dispatch_committed" && a.expires_at <= at {
                MemberMailDelivery::Stale {
                    reason: "expired".into(),
                }
            } else if conn.is_none_or(|x| !x.active || x.epoch != l.connection_epoch) {
                MemberMailDelivery::Stale {
                    reason: "connection_changed".into(),
                }
            } else if let Some(preview) = shown {
                if a.state != "pending_approval" {
                    // The send approval is the active operation; a saved Gmail
                    // draft id stays visible beside it.
                    MemberMailDelivery::Approved {
                        preview,
                        gmail_draft_id: w.draft.as_ref().map(|d| d.0.clone()),
                    }
                } else if let Some((id, message, _)) = &w.draft {
                    MemberMailDelivery::Receipt {
                        kind: "gmail_draft".into(),
                        provider_draft_id: Some(id.clone()),
                        provider_message_id: Some(message.clone()),
                        provider_thread_id: None,
                        preview: Some(preview),
                    }
                } else if w.attempt.is_some() {
                    MemberMailDelivery::Unknown {
                        operation: "gmail_draft".into(),
                        preview: Some(preview),
                    }
                } else {
                    MemberMailDelivery::FinalPreview { preview }
                }
            } else {
                MemberMailDelivery::Stale {
                    reason: "action_changed".into(),
                }
            }
        }
        _ if !live_work => MemberMailDelivery::Stale {
            reason: "source_withdrawn".into(),
        },
        other => MemberMailDelivery::Stale {
            reason: other.into(),
        },
    })
}
fn assemble(c: &Connection, key: &[u8; 32], u: &UserContext, w: &Work) -> Result<MemberMailWork> {
    let (source, draft) = if w.phase == "local_draft" {
        let p = open_payload(key, u, w)?;
        (
            Some(MemberMailWorkSource {
                read_request_id: uuid(&w.read_request_id)?,
                connection_id: uuid(&w.source_connection)?,
                epoch: w.source_epoch,
                mailbox: w.mailbox.clone(),
                message_id: p.cited.message_id,
                subject: p.cited.subject,
                from: p.cited.from,
                date: p.cited.date,
            }),
            Some(MemberMailWorkDraft {
                to: p.to,
                subject: p.subject,
                body: p.body,
            }),
        )
    } else {
        (None, None)
    };
    Ok(MemberMailWork {
        id: w.id,
        thread_id: uuid(&w.thread_id)?,
        revision: w.revision,
        digest: w.digest.clone(),
        state: w.phase.clone(),
        source,
        draft,
        delivery: delivery(c, u, w)?,
        created_at: w.created,
        updated_at: w.updated,
    })
}
fn linked<'a>(w: &'a Work, e: &MemberMailWorkApproval) -> Result<&'a ActionLink> {
    let l = w.action.as_ref().ok_or(StoreError::Conflict)?;
    if e.revision != w.revision || e.action_revision != l.revision || e.action_digest != l.digest {
        return Err(StoreError::Conflict);
    }
    Ok(l)
}
/// The ledger row must still be this work's exact, unexpired, unchanged final
/// content on the exact connection epoch that was shown.
fn open_action(c: &Connection, u: &UserContext, l: &ActionLink) -> Result<MailAction> {
    let a = super::action(c, u, &l.id)?;
    if !matches!(a.state.as_str(), "pending_approval" | "approved")
        || a.content.is_none()
        || a.expires_at <= now()?
        || a.revision != l.revision
        || a.digest != l.digest
    {
        return Err(StoreError::Conflict);
    }
    let all = conns(c, u)?;
    let conn = all
        .iter()
        .find(|x| x.id == l.connection_id)
        .ok_or(StoreError::Conflict)?;
    if !conn.active || conn.native || conn.epoch != l.connection_epoch {
        return Err(StoreError::Conflict);
    }
    Ok(a)
}

impl Store {
    /// A background worker has no browser session. Verify its exact already-approved action
    /// against this volume, the current persisted source grant, and the frozen message bytes.
    pub async fn member_mail_dispatch_mailbox(
        &self,
        owner: VolumeOwner,
        app: Uuid,
        action: String,
        connection: String,
        epoch: i64,
        digest: [u8; 32],
    ) -> Result<Option<String>> {
        self.call(move |db| {
            owner.check_identity(&db.conn,None)?;
            Ok(db.conn.query_row("SELECT w.source_mailbox FROM member_mail_work w
                JOIN mail_actions a ON a.tenant_id=w.tenant_id AND a.account_id=w.account_id AND a.id=w.action_id
                JOIN accounts u ON u.tenant_id=w.tenant_id AND u.id=w.account_id AND u.auth_epoch=w.auth_epoch
                JOIN member_google_thread_reads r ON r.tenant_id=w.tenant_id AND r.account_id=w.account_id AND r.request_id=w.read_request_id
                JOIN member_google_grants g ON g.tenant_id=w.tenant_id AND g.account_id=w.account_id AND g.service='gmail'
                WHERE w.tenant_id=?1 AND w.account_id=?2 AND w.volume_id=?3 AND w.generation=?4 AND w.action_id=?5
                AND w.mail_connection_id=?6 AND w.mail_connection_epoch=?7 AND w.action_digest=?8 AND w.app_revision=?9
                AND w.phase='local_draft' AND w.action_work_revision=w.revision AND a.state='dispatch_committed'
                AND a.auth_epoch=w.auth_epoch AND a.digest=w.action_digest AND a.revision=w.action_revision
                AND a.connection_id=w.mail_connection_id AND a.connection_epoch=w.mail_connection_epoch
                AND r.phase='completed' AND r.volume_id=w.volume_id AND r.generation=w.generation AND r.app_revision=w.app_revision
                AND g.status='active' AND g.connection_id=w.source_connection_id AND g.epoch=w.source_epoch
                AND g.app_revision=w.app_revision AND g.volume_id=w.volume_id AND g.generation=w.generation
                AND lower(g.mailbox)=lower(w.source_mailbox)
                AND EXISTS(SELECT 1 FROM mail_connections c WHERE c.id=?6 AND c.tenant_id=w.tenant_id AND c.account_id=w.account_id
                    AND c.status='active' AND c.epoch=?7 AND lower(c.email)=lower(g.mailbox))",
                params![owner.tenant.to_string(),owner.account.to_string(),owner.volume.to_string(),owner.generation as i64,
                    action,connection,epoch,digest_string(digest),app.to_string()],|r|r.get(0)).optional()?)
        }).await
    }
}
impl Store {
    /// Idempotent per `input.id`. Reads nothing from the provider: it consumes a
    /// completed, still-live member snapshot and cites its message.
    pub async fn prepare_member_mail_work(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        input: MemberMailWorkInput,
    ) -> Result<MemberMailWorkPrepared> {
        if input.id.is_nil()
            || input.thread_id.is_nil()
            || input.read_request_id.is_nil()
            || !message_id_valid(&input.message_id)
        {
            return Err(StoreError::InvalidInput);
        }
        // Syntax of caller details is validated before any state is touched.
        let probe = Cited {
            message_id: input.message_id.clone(),
            subject: String::new(),
            from: String::new(),
            date: String::new(),
        };
        build(&probe, &input.to, input.subject.as_deref(), &input.body)?;
        let replay = {
            let (user, input) = (u.clone(), input.clone());
            self.call(move |db| {
                let tx = db
                    .conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                let u = authority(&tx, &user, o)?;
                crate::chat::thread_exists(&tx, &u, &input.thread_id.to_string())?;
                let Some(w) = load_opt(&tx, &u, input.id)? else {
                    return Ok(None);
                };
                if w.thread_id != input.thread_id.to_string()
                    || w.read_request_id != input.read_request_id.to_string()
                    || w.message_id != input.message_id
                {
                    return Err(StoreError::Conflict);
                }
                let w = refresh(&tx, &u, owned(w, o)?)?;
                if w.phase == "local_draft" {
                    let p = open_payload(&db.provider_key, &u, &w)?;
                    let same =
                        match build(&p.cited, &input.to, input.subject.as_deref(), &input.body)? {
                            Built::Ready(c) => {
                                content_digest(&w.thread_id, &w.read_request_id, &w.mailbox, &c)?
                                    == w.digest
                            }
                            Built::Missing(_) => false,
                        };
                    if !same {
                        return Err(StoreError::Conflict);
                    }
                }
                let work = assemble(&tx, &db.provider_key, &u, &w)?;
                tx.commit()?;
                Ok(Some(MemberMailWorkPrepared::Draft {
                    work: Box::new(work),
                }))
            })
            .await?
        };
        if let Some(done) = replay {
            return Ok(done);
        }
        let reads = self
            .member_google_thread_history(u, o, input.thread_id)
            .await?;
        let read = reads
            .into_iter()
            .find(|r| r.request_id == input.read_request_id)
            .ok_or(StoreError::NotFound)?;
        let state = read.state.clone();
        let (Some(source), Some(result)) = (read.source.as_ref(), read.result.as_ref()) else {
            return Ok(MemberMailWorkPrepared::SourceUnavailable { read_state: state });
        };
        if state != "completed" {
            return Ok(MemberMailWorkPrepared::SourceUnavailable { read_state: state });
        }
        let message = result
            .messages
            .iter()
            .find(|m| m.id == input.message_id)
            .ok_or(StoreError::InvalidInput)?;
        let cited = Cited {
            message_id: message.id.clone(),
            subject: message.subject.clone(),
            from: message.from.clone(),
            date: message.date.clone(),
        };
        let payload = match build(&cited, &input.to, input.subject.as_deref(), &input.body)? {
            Built::Ready(p) => p,
            Built::Missing(m) => {
                return Ok(MemberMailWorkPrepared::NeedsDetails {
                    missing: m.into_iter().map(str::to_owned).collect(),
                });
            }
        };
        let pin = Pin {
            connection: source.connection_id.to_string(),
            epoch: source.epoch,
            app: source.app_revision.to_string(),
            mailbox: source.mailbox.clone(),
            volume: o.volume.to_string(),
            generation: o.generation as i64,
            read: input.read_request_id.to_string(),
        };
        let user = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            crate::chat::thread_exists(&tx, &u, &input.thread_id.to_string())?;
            let (thread, read) = (input.thread_id.to_string(), input.read_request_id.to_string());
            let digest = content_digest(&thread, &read, &pin.mailbox, &payload)?;
            if let Some(w) = load_opt(&tx, &u, input.id)? {
                if w.thread_id != thread || w.read_request_id != read || w.digest != digest {
                    return Err(StoreError::Conflict);
                }
                let w = refresh(&tx, &u, owned(w, o)?)?;
                let work = assemble(&tx, &db.provider_key, &u, &w)?;
                tx.commit()?;
                return Ok(MemberMailWorkPrepared::Draft { work: Box::new(work) });
            }
            if !live(&tx, &u, &pin)? {
                return Ok(MemberMailWorkPrepared::SourceUnavailable {
                    read_state: "withdrawn".into(),
                });
            }
            if tx.query_row(
                "SELECT count(*) FROM member_mail_work WHERE tenant_id=?1 AND account_id=?2",
                params![u.tenant.0, u.account.id],
                |r| r.get::<_, i64>(0),
            )? >= MAX_ROWS
            {
                return Err(StoreError::Busy);
            }
            let w = Work {
                id: input.id,
                thread_id: thread,
                read_request_id: read,
                volume: pin.volume.clone(),
                generation: pin.generation,
                auth_epoch: u.epoch,
                source_connection: pin.connection.clone(),
                source_epoch: pin.epoch,
                app: pin.app.clone(),
                mailbox: pin.mailbox.clone(),
                message_id: input.message_id.clone(),
                revision: 1,
                digest,
                phase: "local_draft".into(),
                cipher: None,
                action: None,
                attempt: None,
                draft: None,
                created: 0,
                updated: 0,
            };
            crate::personal_browser::activity::admit(&tx, o, &u, &input.thread_id.to_string(), &input.id.to_string(), "mail_draft", "읽은 메일의 회신 초안", "local_draft")?;
            let cipher = seal(&db.provider_key, &u, &w, &payload)?;
            tx.execute(
                "INSERT INTO member_mail_work(tenant_id,account_id,id,thread_id,read_request_id,volume_id,generation,auth_epoch,source_connection_id,source_epoch,app_revision,source_mailbox,source_message_id,revision,digest,phase,cipher,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,1,?14,'local_draft',?15,unixepoch(),unixepoch())",
                params![
                    u.tenant.0,
                    u.account.id,
                    w.id.to_string(),
                    w.thread_id,
                    w.read_request_id,
                    w.volume,
                    w.generation,
                    u.epoch,
                    w.source_connection,
                    w.source_epoch,
                    w.app,
                    w.mailbox,
                    w.message_id,
                    w.digest,
                    cipher
                ],
            )?;
            let saved = load(&tx, &u, w.id)?;
            let work = assemble(&tx, &db.provider_key, &u, &saved)?;
            tx.commit()?;
            Ok(MemberMailWorkPrepared::Draft { work: Box::new(work) })
        })
        .await
    }

    pub async fn member_mail_work(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        id: Uuid,
    ) -> Result<MemberMailWork> {
        let user = u.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            let w = refresh(&tx, &u, owned(load(&tx, &u, id)?, o)?)?;
            let work = assemble(&tx, &db.provider_key, &u, &w)?;
            tx.commit()?;
            Ok(work)
        })
        .await
    }

    pub async fn member_mail_work_list(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        thread: Uuid,
    ) -> Result<Vec<MemberMailWork>> {
        let user = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            crate::chat::thread_exists(&tx, &u, &thread.to_string())?;
            let ids: Vec<String> = tx
                .prepare("SELECT id FROM member_mail_work WHERE tenant_id=?1 AND account_id=?2 AND thread_id=?3 AND volume_id=?4 AND generation=?5 ORDER BY created_at DESC,id DESC LIMIT 20")?
                .query_map(
                    params![u.tenant.0, u.account.id, thread.to_string(), o.volume.to_string(), o.generation as i64],
                    |r| r.get(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut out = Vec::new();
            for id in ids {
                let w = refresh(&tx, &u, load(&tx, &u, uuid(&id)?)?)?;
                out.push(assemble(&tx, &db.provider_key, &u, &w)?);
            }
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// Local edit. Refused once any external effect exists; any unsent final
    /// approval is cancelled so its digest can never be approved again.
    pub async fn revise_member_mail_work(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        id: Uuid,
        expected_revision: i64,
        patch: MemberMailWorkPatch,
    ) -> Result<MemberMailWork> {
        let user = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            let mut w = match gate(&tx, &u, o, id, Some(expected_revision))? {
                Gate::Live(w) => *w,
                Gate::Gone(w) => {
                    let work = assemble(&tx, &db.provider_key, &u, &w)?;
                    tx.commit()?;
                    return Ok(work);
                }
            };
            if w.attempt.is_some() || w.draft.is_some() {
                return Err(StoreError::Conflict);
            }
            if let Some(l) = &w.action
                && !matches!(
                    super::action(&tx, &u, &l.id)?.state.as_str(),
                    "pending_approval" | "approved" | "rejected" | "cancelled" | "expired"
                )
            {
                return Err(StoreError::Conflict);
            }
            let old = open_payload(&db.provider_key, &u, &w)?;
            let to = patch.to.unwrap_or_else(|| old.to.clone());
            let subject = patch.subject.or_else(|| Some(old.subject.clone()));
            let body = patch.body.unwrap_or_else(|| old.body.clone());
            let Built::Ready(next) = build(&old.cited, &to, subject.as_deref(), &body)? else {
                return Err(StoreError::InvalidInput);
            };
            cancel_action(&tx, &u, &w)?;
            let prior = w.revision;
            w.revision = prior.checked_add(1).ok_or(StoreError::Corrupt)?;
            w.digest = content_digest(&w.thread_id, &w.read_request_id, &w.mailbox, &next)?;
            let cipher = seal(&db.provider_key, &u, &w, &next)?;
            let changed = tx.execute(
                "UPDATE member_mail_work SET revision=?4,digest=?5,cipher=?6,action_id=NULL,action_revision=NULL,action_digest=NULL,action_work_revision=NULL,mail_connection_id=NULL,mail_connection_epoch=NULL,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?7 AND phase='local_draft'",
                params![u.tenant.0, u.account.id, id.to_string(), w.revision, w.digest, cipher, prior],
            )?;
            if changed != 1 {
                return Err(StoreError::Conflict);
            }
            let saved = load(&tx, &u, id)?;
            let work = assemble(&tx, &db.provider_key, &u, &saved)?;
            tx.commit()?;
            Ok(work)
        })
        .await
    }

    /// Creates (or supersedes) the standard final-content approval on the chosen
    /// same-mailbox standard connection. Nothing is sent or saved here.
    pub async fn preview_member_mail_work(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        id: Uuid,
        expected_revision: i64,
        connection_id: String,
        connection_epoch: i64,
    ) -> Result<MemberMailWork> {
        let user = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            let w = match gate(&tx, &u, o, id, Some(expected_revision))? {
                Gate::Live(w) => *w,
                Gate::Gone(w) => {
                    let work = assemble(&tx, &db.provider_key, &u, &w)?;
                    tx.commit()?;
                    return Ok(work);
                }
            };
            if w.attempt.is_some() || w.draft.is_some() {
                return Err(StoreError::Conflict);
            }
            let all = conns(&tx, &u)?;
            let chosen = all
                .iter()
                .find(|x| x.id == connection_id && x.epoch == connection_epoch)
                .ok_or(StoreError::Conflict)?;
            if !chosen.active || chosen.native || !chosen.email.eq_ignore_ascii_case(&w.mailbox) {
                return Err(StoreError::Conflict);
            }
            if !chosen.compose && !chosen.send {
                return Err(StoreError::Forbidden);
            }
            let p = open_payload(&db.provider_key, &u, &w)?;
            let replace = match &w.action {
                Some(l) => {
                    let state = super::action(&tx, &u, &l.id)?.state;
                    matches!(state.as_str(), "pending_approval" | "approved")
                        .then(|| (l.id.clone(), l.revision))
                }
                None => None,
            };
            let a = super::draft(
                &tx,
                &u,
                MailDraftInput {
                    connection_id: chosen.id.clone(),
                    content: MailDraftContent {
                        from: chosen.email.clone(),
                        to: p.to,
                        cc: vec![],
                        bcc: vec![],
                        subject: p.subject,
                        text: p.body,
                        reply: None,
                    },
                    origin_thread_id: Some(w.thread_id.clone()),
                },
                None,
                replace,
            )?;
            tx.execute(
                "UPDATE member_mail_work SET action_id=?4,action_revision=?5,action_digest=?6,action_work_revision=?7,mail_connection_id=?8,mail_connection_epoch=?9,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?7 AND phase='local_draft'",
                params![u.tenant.0, u.account.id, id.to_string(), a.id, a.revision, a.digest, w.revision, chosen.id, chosen.epoch],
            )?;
            let saved = load(&tx, &u, id)?;
            let work = assemble(&tx, &db.provider_key, &u, &saved)?;
            tx.commit()?;
            Ok(work)
        })
        .await
    }

    /// Marks the intent to call the standard journaled draft adapter. Replaying a
    /// recorded receipt never reaches the adapter; an unresolved attempt hands the
    /// same adapter back so it can only read back, never blindly POST twice.
    pub async fn begin_member_mail_provider_save(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        id: Uuid,
        expected: MemberMailWorkApproval,
    ) -> Result<MemberMailSaveAdmission> {
        let user = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            let w = match gate(&tx, &u, o, id, Some(expected.revision))? {
                Gate::Live(w) => *w,
                Gate::Gone(w) => {
                    let work = assemble(&tx, &db.provider_key, &u, &w)?;
                    tx.commit()?;
                    return Ok(MemberMailSaveAdmission::Gone(Box::new(work)));
                }
            };
            let l = linked(&w, &expected)?;
            if w.draft.is_some() {
                let work = assemble(&tx, &db.provider_key, &u, &w)?;
                tx.commit()?;
                return Ok(MemberMailSaveAdmission::Recorded(Box::new(work)));
            }
            open_action(&tx, &u, l)?;
            let all = conns(&tx, &u)?;
            if !all.iter().any(|x| x.id == l.connection_id && x.compose) {
                return Err(StoreError::Forbidden);
            }
            let attempt = match &w.attempt {
                Some(a) => a.clone(),
                None => {
                    let a = Uuid::new_v4().to_string();
                    let n = tx.execute(
                        "UPDATE member_mail_work SET provider_attempt=?4,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND provider_attempt IS NULL AND provider_draft_id IS NULL",
                        params![u.tenant.0, u.account.id, id.to_string(), a],
                    )?;
                    if n != 1 {
                        return Err(StoreError::Conflict);
                    }
                    a
                }
            };
            tx.commit()?;
            Ok(MemberMailSaveAdmission::Fresh(MemberMailSaveLease {
                user: u,
                owner: o,
                id,
                attempt,
                action_id: l.id.clone(),
                action_digest: l.digest.clone(),
                request: GmailDraftSave {
                    expected: MailApproval {
                        revision: l.revision,
                        digest: l.digest.clone(),
                    },
                    draft_id: None,
                    expected_provider_digest: None,
                },
            }))
        })
        .await
    }

    /// Records exactly what the adapter returned. `definite_failure` is only for
    /// refusals before any provider write could have started.
    pub async fn finish_member_mail_provider_save(
        &self,
        lease: MemberMailSaveLease,
        receipt: Option<GmailDraftRef>,
        definite_failure: bool,
    ) -> Result<MemberMailWork> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (tenant, account) = (&lease.user.tenant.0, &lease.user.account.id);
            match &receipt {
                Some(r) => {
                    let n = tx.execute(
                        "UPDATE member_mail_work SET provider_draft_id=?5,provider_draft_message_id=?6,provider_draft_digest=?7,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND provider_attempt=?4 AND provider_draft_id IS NULL",
                        params![tenant, account, lease.id.to_string(), lease.attempt, r.id, r.message_id, lease.action_digest],
                    )?;
                    if n != 1 {
                        let same: bool = tx.query_row(
                            "SELECT provider_draft_id=?4 FROM member_mail_work WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
                            params![tenant, account, lease.id.to_string(), r.id],
                            |row| row.get::<_, Option<bool>>(0).map(|v| v.unwrap_or(false)),
                        )?;
                        if !same {
                            return Err(StoreError::Conflict);
                        }
                    }
                }
                None if definite_failure => {
                    tx.execute(
                        "UPDATE member_mail_work SET provider_attempt=NULL,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND provider_attempt=?4 AND provider_draft_id IS NULL",
                        params![tenant, account, lease.id.to_string(), lease.attempt],
                    )?;
                }
                None => {}
            }
            // The receipt is committed even if the session vanished meanwhile.
            let current = authority(&tx, &lease.user, lease.owner);
            let result = match current {
                Ok(u) => {
                    let w = refresh(&tx, &u, owned(load(&tx, &u, lease.id)?, lease.owner)?)?;
                    Some(assemble(&tx, &db.provider_key, &u, &w)?)
                }
                Err(_) => None,
            };
            tx.commit()?;
            result.ok_or(StoreError::Unauthorized)
        })
        .await
    }

    /// Final-content approval through the standard ledger. Needs the shown
    /// revision/digest, a live source, the pinned connection epoch and the send
    /// scope; the standard dispatcher alone performs (and receipts) the send.
    pub async fn approve_member_mail_work(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        id: Uuid,
        expected: MemberMailWorkApproval,
    ) -> Result<MemberMailWork> {
        enum Step {
            Done(Box<MemberMailWork>),
            Decide(String),
        }
        let user = u.clone();
        let exact = expected.clone();
        let step = self
            .call(move |db| {
                let tx = db
                    .conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                let u = authority(&tx, &user, o)?;
                let w = match gate(&tx, &u, o, id, Some(exact.revision))? {
                    Gate::Live(w) => *w,
                    Gate::Gone(w) => {
                        let work = assemble(&tx, &db.provider_key, &u, &w)?;
                        tx.commit()?;
                        return Ok(Step::Done(Box::new(work)));
                    }
                };
                let l = linked(&w, &exact)?;
                let a = open_action(&tx, &u, l)?;
                let all = conns(&tx, &u)?;
                if !all.iter().any(|x| x.id == l.connection_id && x.send) {
                    return Err(StoreError::Forbidden);
                }
                if a.state == "approved" {
                    let work = assemble(&tx, &db.provider_key, &u, &w)?;
                    tx.commit()?;
                    return Ok(Step::Done(Box::new(work)));
                }
                Ok(Step::Decide(l.id.clone()))
            })
            .await?;
        match step {
            Step::Done(work) => Ok(*work),
            Step::Decide(action) => {
                self.decide_mail(
                    u,
                    action,
                    MailApproval {
                        revision: expected.action_revision,
                        digest: expected.action_digest,
                    },
                    true,
                )
                .await?;
                self.member_mail_work(u, o, id).await
            }
        }
    }

    /// Drops the local draft and cancels any unsent approval. A Gmail draft that
    /// was already saved stays in Gmail and keeps its receipt.
    pub async fn discard_member_mail_work(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        id: Uuid,
        expected_revision: i64,
    ) -> Result<MemberMailWork> {
        let user = u.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            let mut w = match gate(&tx, &u, o, id, Some(expected_revision))? {
                Gate::Live(w) => *w,
                Gate::Gone(w) => *w,
            };
            if w.phase == "local_draft" {
                withdraw(&tx, &u, &mut w)?;
            }
            let work = assemble(&tx, &db.provider_key, &u, &w)?;
            tx.commit()?;
            Ok(work)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, directory};
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde_json::{Value, json};
    const PASSWORD: &str = "Synthetic-work-password-42";
    const CHANGED: &str = "Changed-work-password-42";
    const MAILBOX: &str = "member@example.test";

    fn wire_owner(o: VolumeOwner) -> Value {
        json!({"account_id":o.account,"tenant_id":o.tenant,"volume_id":o.volume,"generation":o.generation,"peer_uid":unsafe{libc::geteuid()}})
    }
    fn binding(o: VolumeOwner, app: Uuid, purpose: &str, expires: i64) -> String {
        let payload = json!({"v":1,"owner":wire_owner(o),"app_revision":app,"purpose":purpose,"mailbox":MAILBOX,"token_tag":URL_SAFE_NO_PAD.encode([7u8;32]),"expires_at":expires});
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap()),
            URL_SAFE_NO_PAD.encode([8u8; 32])
        )
    }
    fn carrier(id: Uuid, o: VolumeOwner, app: Uuid) -> MemberGoogleGrant {
        let reply = json!({"v":1,"request_id":id,"owner":wire_owner(o),"app_revision":app,"state":"ok","result":{"access_token":"SYNTHETIC-ACCESS","refresh_token":"SYNTHETIC-REFRESH","expires_in":3600,"scopes":[moosuri_connectors::gmail::READ_SCOPE],"mailbox":MAILBOX,"access_binding":binding(o,app,"access",now().unwrap()+3600),"refresh_binding":binding(o,app,"refresh",now().unwrap()+86400)}});
        MemberGoogleGrant::from_broker_reply(
            id,
            Zeroizing::new(serde_json::to_vec(&reply).unwrap()),
        )
        .unwrap()
    }
    fn inbox() -> MemberGoogleInbox {
        MemberGoogleInbox {
            mailbox: MAILBOX.into(),
            messages: vec![MemberGoogleInboxMessage {
                id: "m1".into(),
                received_at_ms: 1,
                subject: "Application status".into(),
                from: "Recruiter <recruiter@example.test>".into(),
                date: "Mon, 1 Jan".into(),
            }],
            has_more: false,
            scope: "one_inbox_page_metadata_only".into(),
        }
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        s: Store,
        u: UserContext,
        o: VolumeOwner,
        thread: Uuid,
        read: Uuid,
        grant: MemberGoogleStatus,
    }
    async fn fixture() -> Fixture {
        let dir = directory();
        let o = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        o.initialize(
            dir.path(),
            "Work member".into(),
            MAILBOX.into(),
            PASSWORD.into(),
        )
        .await
        .unwrap();
        let s = o.open_verified(dir.path()).await.unwrap();
        let first = s.login(MAILBOX.into(), PASSWORD.into()).await.unwrap();
        let u = s
            .change_password(&first.user, CHANGED.into())
            .await
            .unwrap()
            .user;
        let app = Uuid::new_v4();
        let start = s.start_member_google_oauth(&u, o, app).await.unwrap();
        let handle = s
            .capture_mail_oauth(
                start.state,
                SecretString::new("synthetic-code".into()).unwrap(),
            )
            .await
            .unwrap();
        let exchange = s
            .claim_member_google_oauth(&u, o, app, handle)
            .await
            .unwrap();
        let id = exchange.member_google_request_id().unwrap();
        let grant = s
            .complete_member_google_oauth(exchange, carrier(id, o, app))
            .await
            .unwrap();
        let thread: Uuid = s
            .create_thread(&u, "Original".into())
            .await
            .unwrap()
            .id
            .parse()
            .unwrap();
        let intent = MemberGoogleReadIntent {
            thread_id: thread,
            request_id: Uuid::new_v4(),
        };
        match s
            .begin_member_google_thread_read(
                &u,
                o,
                app,
                intent.clone(),
                grant.connection_id.unwrap(),
                grant.epoch,
            )
            .await
            .unwrap()
        {
            MemberGoogleReadAdmission::Fresh(lease) => {
                s.settle_member_google_thread_read(lease, Some(inbox()))
                    .await
                    .unwrap();
            }
            MemberGoogleReadAdmission::Existing(_) => panic!("fresh read expected"),
        }
        Fixture {
            _dir: dir,
            s,
            u,
            o,
            thread,
            read: intent.request_id,
            grant,
        }
    }
    /// Standard-domain connection fixture. Compose is granted only through the
    /// existing compose OAuth path; the readonly member grant never gains scope.
    async fn mail_connection(
        s: &Store,
        u: &UserContext,
        email: &str,
        compose: bool,
    ) -> MailConnection {
        let pin = GoogleAppPin {
            revision: None,
            client_id: "synthetic-app".into(),
            redirect_uri: "http://127.0.0.1/callback".into(),
        };
        let start = if compose {
            s.start_mail_compose_oauth_with_app(u, pin.clone())
                .await
                .unwrap()
        } else {
            s.start_mail_oauth(u).await.unwrap()
        };
        let handle = s
            .capture_mail_oauth(
                start.state,
                SecretString::new("synthetic-code".into()).unwrap(),
            )
            .await
            .unwrap();
        let exchange = if compose {
            s.claim_mail_oauth_with_app(u, handle, pin).await.unwrap()
        } else {
            s.claim_mail_oauth(u, handle).await.unwrap()
        };
        let mut scopes: Vec<String> = moosuri_connectors::gmail::MINIMAL_SCOPES
            .iter()
            .map(|v| v.to_string())
            .collect();
        if compose {
            scopes.push(moosuri_connectors::gmail::COMPOSE_SCOPE.into());
        }
        s.complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("SYNTHETIC-MAIL-ACCESS".into()).unwrap(),
                refresh_token: Some(SecretString::new("SYNTHETIC-MAIL-REFRESH".into()).unwrap()),
                expires_in: 3600,
                scopes: Some(scopes),
            },
            email.into(),
        )
        .await
        .unwrap()
    }
    fn input(f: &Fixture, id: Uuid, body: &str) -> MemberMailWorkInput {
        MemberMailWorkInput {
            id,
            thread_id: f.thread,
            read_request_id: f.read,
            message_id: "m1".into(),
            to: vec![],
            subject: None,
            body: body.into(),
        }
    }
    fn draft_of(p: MemberMailWorkPrepared) -> MemberMailWork {
        match p {
            MemberMailWorkPrepared::Draft { work } => *work,
            _ => panic!("draft expected"),
        }
    }
    fn shown(w: &MemberMailWork) -> MemberMailPreview {
        match &w.delivery {
            MemberMailDelivery::FinalPreview { preview }
            | MemberMailDelivery::Approved { preview, .. } => preview.clone(),
            MemberMailDelivery::Receipt {
                preview: Some(p), ..
            }
            | MemberMailDelivery::Unknown {
                preview: Some(p), ..
            } => p.clone(),
            _ => panic!("preview expected"),
        }
    }
    fn approval(w: &MemberMailWork) -> MemberMailWorkApproval {
        let p = shown(w);
        MemberMailWorkApproval {
            revision: w.revision,
            action_revision: p.action_revision,
            action_digest: p.action_digest,
        }
    }

    #[tokio::test]
    async fn prepare_cites_the_read_source_replays_and_stays_local_on_readonly() {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let id = Uuid::new_v4();
        assert!(matches!(
            f.s.prepare_member_mail_work(&f.u, f.o, input(&f, id, "")).await.unwrap(),
            MemberMailWorkPrepared::NeedsDetails { missing } if missing == ["body"]
        ));
        assert!(
            f.s.member_mail_work_list(&f.u, f.o, f.thread)
                .await
                .unwrap()
                .is_empty()
        );
        let first = draft_of(
            f.s.prepare_member_mail_work(&f.u, f.o, input(&f, id, "Thank you, I am interested."))
                .await
                .unwrap(),
        );
        let source = first.source.as_ref().unwrap();
        assert_eq!(
            (source.mailbox.as_str(), source.message_id.as_str()),
            (MAILBOX, "m1")
        );
        assert_eq!(source.read_request_id, f.read);
        assert_eq!(source.connection_id, f.grant.connection_id.unwrap());
        let draft = first.draft.as_ref().unwrap();
        assert_eq!(draft.to, ["recruiter@example.test"]);
        assert_eq!(draft.subject, "Re: Application status");
        assert!(matches!(
            &first.delivery,
            MemberMailDelivery::NeedsConnection { reason, .. } if reason == "readonly_only"
        ));
        let again = draft_of(
            f.s.prepare_member_mail_work(&f.u, f.o, input(&f, id, "Thank you, I am interested."))
                .await
                .unwrap(),
        );
        assert_eq!(again.digest, first.digest);
        assert_eq!(
            f.s.member_mail_work_list(&f.u, f.o, f.thread)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(matches!(
            f.s.prepare_member_mail_work(&f.u, f.o, input(&f, id, "Different text"))
                .await,
            Err(StoreError::Conflict)
        ));
        let mut wrong = input(&f, Uuid::new_v4(), "Hello");
        wrong.message_id = "not-in-the-read".into();
        assert!(matches!(
            f.s.prepare_member_mail_work(&f.u, f.o, wrong).await,
            Err(StoreError::InvalidInput)
        ));
        // The chat receipt references the local work. It contains no recipient, cited message or draft body.
        assert!(f.s.mail_actions(&f.u).await.unwrap().is_empty());
        let turns =
            f.s.conversation(&f.u, f.thread.to_string(), None)
                .await
                .unwrap()
                .turns;
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].user_text, "읽은 메일의 회신 초안");
        let visible = serde_json::to_string(&turns).unwrap();
        assert!(
            !visible.contains("Thank")
                && !visible.contains("Application")
                && !visible.contains(MAILBOX)
        );
        let blob: Vec<u8> =
            f.s.call(move |db| {
                Ok(db
                    .conn
                    .query_row("SELECT cipher FROM member_mail_work", [], |r| {
                        r.get::<_, Vec<u8>>(0)
                    })?)
            })
            .await
            .unwrap();
        assert!(!blob.windows(5).any(|w| w == b"Thank" || w == b"Appli"));
        let foreign = VolumeOwner {
            volume: Uuid::new_v4(),
            ..f.o
        };
        assert!(f.s.member_mail_work(&f.u, foreign, id).await.is_err());
    }

    #[tokio::test]
    async fn source_withdrawal_destroys_the_draft_and_cancels_an_unsent_approval() {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let conn = mail_connection(&f.s, &f.u, MAILBOX, false).await;
        let work = draft_of(
            f.s.prepare_member_mail_work(&f.u, f.o, input(&f, Uuid::new_v4(), "Hello"))
                .await
                .unwrap(),
        );
        assert!(matches!(
            &work.delivery,
            MemberMailDelivery::Ready { connections } if connections.len() == 1 && connections[0].can_send && !connections[0].can_save_draft
        ));
        let previewed =
            f.s.preview_member_mail_work(
                &f.u,
                f.o,
                work.id,
                work.revision,
                conn.id.clone(),
                conn.epoch,
            )
            .await
            .unwrap();
        let p = shown(&previewed);
        assert!(p.can_approve_send && !p.can_save_draft && !p.threaded);
        assert_eq!(p.from, MAILBOX);
        f.s.disconnect_member_google(&f.u, f.o, f.grant.connection_id.unwrap(), f.grant.epoch)
            .await
            .unwrap();
        let after = f.s.member_mail_work(&f.u, f.o, work.id).await.unwrap();
        assert_eq!(after.state, "withdrawn");
        assert!(after.source.is_none() && after.draft.is_none());
        assert!(
            matches!(&after.delivery, MemberMailDelivery::Stale { reason } if reason == "source_withdrawn")
        );
        let action = f.s.mail_action(&f.u, p.action_id.clone()).await.unwrap();
        assert_eq!(action.state, "cancelled");
        assert!(action.content.is_none());
        let late =
            f.s.approve_member_mail_work(&f.u, f.o, work.id, approval(&previewed))
                .await
                .unwrap();
        assert_eq!(late.state, "withdrawn");
        assert_eq!(
            f.s.mail_action(&f.u, p.action_id).await.unwrap().state,
            "cancelled"
        );
    }

    /// A previewed, still-unsent final approval for a live local draft.
    async fn pending(f: &Fixture) -> (MemberMailWork, String) {
        let conn = mail_connection(&f.s, &f.u, MAILBOX, false).await;
        let work = draft_of(
            f.s.prepare_member_mail_work(&f.u, f.o, input(f, Uuid::new_v4(), "Hello"))
                .await
                .unwrap(),
        );
        let previewed =
            f.s.preview_member_mail_work(
                &f.u,
                f.o,
                work.id,
                work.revision,
                conn.id.clone(),
                conn.epoch,
            )
            .await
            .unwrap();
        (work, shown(&previewed).action_id)
    }
    /// Raw (work phase, action state): bypasses the lazy withdrawal on read.
    async fn raw(f: &Fixture, work: Uuid, action: &str) -> (String, String) {
        let (w, a) = (work.to_string(), action.to_string());
        f.s.call(move |db| {
            let phase: String = db.conn.query_row(
                "SELECT phase FROM member_mail_work WHERE id=?1",
                [&w],
                |r| r.get(0),
            )?;
            let state: String =
                db.conn
                    .query_row("SELECT state FROM mail_actions WHERE id=?1", [&a], |r| {
                        r.get(0)
                    })?;
            Ok((phase, state))
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn refused_refresh_withdraws_pending_mail_work_immediately_without_a_read() {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let (work, action) = pending(&f).await;
        assert_eq!(
            raw(&f, work.id, &action).await,
            ("local_draft".into(), "pending_approval".into())
        );
        let lease =
            f.s.claim_member_google_refresh(&f.u, f.o, f.grant.app_revision.unwrap())
                .await
                .unwrap();
        let status = f.s.settle_member_google_refresh(lease, None).await.unwrap();
        assert!(!status.connected);
        assert_eq!(
            raw(&f, work.id, &action).await,
            ("withdrawn".into(), "cancelled".into())
        );
        let cipher: Option<Vec<u8>> =
            f.s.call(|db| {
                Ok(db
                    .conn
                    .query_row("SELECT cipher FROM member_mail_work", [], |r| r.get(0))?)
            })
            .await
            .unwrap();
        assert!(cipher.is_none());
    }

    #[tokio::test]
    async fn restart_after_an_interrupted_refresh_withdraws_pending_mail_work() {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let (work, action) = pending(&f).await;
        let _lease =
            f.s.claim_member_google_refresh(&f.u, f.o, f.grant.app_revision.unwrap())
                .await
                .unwrap();
        f.s.call(|db| {
            crate::mail::restart_member_google(&db.conn)?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            raw(&f, work.id, &action).await,
            ("withdrawn".into(), "cancelled".into())
        );
    }

    #[tokio::test]
    async fn source_loss_never_rewrites_an_already_sent_action() {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let (work, action) = pending(&f).await;
        let a = action.clone();
        f.s.call(move |db| {
            db.conn
                .execute("UPDATE mail_actions SET state='sent' WHERE id=?1", [&a])?;
            Ok(())
        })
        .await
        .unwrap();
        f.s.disconnect_member_google(&f.u, f.o, f.grant.connection_id.unwrap(), f.grant.epoch)
            .await
            .unwrap();
        // The local draft text is gone, but the sent receipt is history and stays.
        assert_eq!(
            raw(&f, work.id, &action).await,
            ("withdrawn".into(), "sent".into())
        );
    }

    #[tokio::test]
    async fn source_loss_is_scoped_to_the_member_and_the_gmail_service() {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let (work, action) = pending(&f).await;
        let (tenant, account) = (f.u.tenant.0.clone(), f.u.account.id.clone());
        f.s.call(move |db| {
            // Another member, and this member's Calendar source, are not Gmail work.
            withdraw_account(&db.conn, "other-tenant", "other-account")?;
            withdraw_account(&db.conn, &tenant, "other-account")?;
            crate::mail::member_google::source_lost(&db.conn, &tenant, &account, "calendar")?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            raw(&f, work.id, &action).await,
            ("local_draft".into(), "pending_approval".into())
        );
        let (tenant, account) = (f.u.tenant.0.clone(), f.u.account.id.clone());
        f.s.call(move |db| {
            crate::mail::member_google::source_lost(&db.conn, &tenant, &account, "gmail")?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            raw(&f, work.id, &action).await,
            ("withdrawn".into(), "cancelled".into())
        );
    }

    #[tokio::test]
    async fn wrong_mailbox_scope_and_changed_preview_cannot_be_approved() {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let other = mail_connection(&f.s, &f.u, "someone-else@example.test", true).await;
        let work = draft_of(
            f.s.prepare_member_mail_work(&f.u, f.o, input(&f, Uuid::new_v4(), "Hello"))
                .await
                .unwrap(),
        );
        assert!(matches!(
            &work.delivery,
            MemberMailDelivery::NeedsConnection { reason, .. } if reason == "mailbox_mismatch"
        ));
        assert!(matches!(
            f.s.preview_member_mail_work(&f.u, f.o, work.id, work.revision, other.id, other.epoch)
                .await,
            Err(StoreError::Conflict)
        ));
        let conn = mail_connection(&f.s, &f.u, MAILBOX, false).await;
        let first =
            f.s.preview_member_mail_work(
                &f.u,
                f.o,
                work.id,
                work.revision,
                conn.id.clone(),
                conn.epoch,
            )
            .await
            .unwrap();
        // No compose scope: no Gmail draft may be started, and nothing is recorded.
        assert!(matches!(
            f.s.begin_member_mail_provider_save(&f.u, f.o, work.id, approval(&first))
                .await,
            Err(StoreError::Forbidden)
        ));
        let second =
            f.s.preview_member_mail_work(
                &f.u,
                f.o,
                work.id,
                work.revision,
                conn.id.clone(),
                conn.epoch,
            )
            .await
            .unwrap();
        assert_ne!(shown(&first).action_digest, shown(&second).action_digest);
        assert!(matches!(
            f.s.approve_member_mail_work(&f.u, f.o, work.id, approval(&first))
                .await,
            Err(StoreError::Conflict)
        ));
        assert_eq!(
            f.s.mail_action(&f.u, shown(&second).action_id)
                .await
                .unwrap()
                .state,
            "pending_approval"
        );
        let approved =
            f.s.approve_member_mail_work(&f.u, f.o, work.id, approval(&second))
                .await
                .unwrap();
        assert!(matches!(
            &approved.delivery,
            MemberMailDelivery::Approved { .. }
        ));
        let replay =
            f.s.approve_member_mail_work(&f.u, f.o, work.id, approval(&second))
                .await
                .unwrap();
        assert!(matches!(
            &replay.delivery,
            MemberMailDelivery::Approved { .. }
        ));
    }

    #[tokio::test]
    async fn gmail_draft_receipt_comes_only_from_the_adapter_and_unknown_is_never_resolved_locally()
    {
        let _g = TEST_LOCK.lock().await;
        let f = fixture().await;
        let conn = mail_connection(&f.s, &f.u, MAILBOX, true).await;
        let work = draft_of(
            f.s.prepare_member_mail_work(&f.u, f.o, input(&f, Uuid::new_v4(), "Hello"))
                .await
                .unwrap(),
        );
        let previewed =
            f.s.preview_member_mail_work(
                &f.u,
                f.o,
                work.id,
                work.revision,
                conn.id.clone(),
                conn.epoch,
            )
            .await
            .unwrap();
        assert!(shown(&previewed).can_save_draft);
        let begin = |a: MemberMailWorkApproval| {
            let (s, u, o, id) = (f.s.clone(), f.u.clone(), f.o, work.id);
            async move {
                match s
                    .begin_member_mail_provider_save(&u, o, id, a)
                    .await
                    .unwrap()
                {
                    MemberMailSaveAdmission::Fresh(lease) => lease,
                    _ => panic!("fresh lease expected"),
                }
            }
        };
        let lease = begin(approval(&previewed)).await;
        assert_eq!(lease.action_id(), shown(&previewed).action_id);
        // A lost response: unknown, and no local receipt exists.
        let unknown =
            f.s.finish_member_mail_provider_save(lease, None, false)
                .await
                .unwrap();
        assert!(
            matches!(&unknown.delivery, MemberMailDelivery::Unknown { operation, .. } if operation == "gmail_draft")
        );
        assert!(!shown(&unknown).can_save_draft);
        // Retry hands out the same journaled adapter; a refusal before IO clears the marker.
        let retry = begin(approval(&unknown)).await;
        let cleared =
            f.s.finish_member_mail_provider_save(retry, None, true)
                .await
                .unwrap();
        assert!(matches!(
            &cleared.delivery,
            MemberMailDelivery::FinalPreview { .. }
        ));
        let lease = begin(approval(&cleared)).await;
        let saved =
            f.s.finish_member_mail_provider_save(
                lease,
                Some(GmailDraftRef {
                    id: "draft-1".into(),
                    message_id: "message-1".into(),
                }),
                false,
            )
            .await
            .unwrap();
        assert!(matches!(
            &saved.delivery,
            MemberMailDelivery::Receipt { kind, provider_draft_id: Some(id), .. } if kind == "gmail_draft" && id == "draft-1"
        ));
        assert!(shown(&saved).can_approve_send && !shown(&saved).can_save_draft);
        assert!(matches!(
            f.s.begin_member_mail_provider_save(&f.u, f.o, work.id, approval(&saved))
                .await
                .unwrap(),
            MemberMailSaveAdmission::Recorded(_)
        ));
        // An external effect exists: the local draft is no longer editable or re-previewable.
        assert!(matches!(
            f.s.revise_member_mail_work(
                &f.u,
                f.o,
                work.id,
                saved.revision,
                MemberMailWorkPatch {
                    to: None,
                    subject: None,
                    body: Some("Changed".into())
                }
            )
            .await,
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            f.s.preview_member_mail_work(&f.u, f.o, work.id, saved.revision, conn.id, conn.epoch)
                .await,
            Err(StoreError::Conflict)
        ));
    }
}
