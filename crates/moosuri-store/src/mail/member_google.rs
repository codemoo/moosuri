//! Private member readonly credentials. No mail_connections/send authority.
//! Secret carriers are deliberately neither Debug nor Serialize. Only status
//! DTOs may reach the browser; raw grants must come from UID-verified broker IPC.
use super::*;
use crate::personal_volume::VolumeOwner;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize, ser::SerializeStruct};
pub(in crate::mail) mod briefs;
pub(super) mod calendar;
pub(super) mod reads;
#[cfg(test)]
const READ: &str = moosuri_connectors::gmail::READ_SCOPE;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberGoogleService {
    Gmail,
    Drive,
    Docs,
    Slides,
    Sheets,
    /// Dedicated primary-calendar write grant. Never part of `WORKSPACE`, never a
    /// Drive/read-service capability.
    Calendar,
}

impl MemberGoogleService {
    pub const WORKSPACE: [Self; 4] = [Self::Drive, Self::Docs, Self::Slides, Self::Sheets];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Drive => "drive",
            Self::Docs => "docs",
            Self::Slides => "slides",
            Self::Sheets => "sheets",
            Self::Calendar => "calendar",
        }
    }
    /// True only for the dedicated write-capable grant.
    pub fn writes(self) -> bool {
        self == Self::Calendar
    }
    pub fn scope(self) -> &'static str {
        match self {
            Self::Calendar => "https://www.googleapis.com/auth/calendar.events.owned",
            Self::Gmail => moosuri_connectors::gmail::READ_SCOPE,
            Self::Drive => "https://www.googleapis.com/auth/drive.metadata.readonly",
            Self::Docs => "https://www.googleapis.com/auth/documents.readonly",
            Self::Slides => "https://www.googleapis.com/auth/presentations.readonly",
            Self::Sheets => "https://www.googleapis.com/auth/spreadsheets.readonly",
        }
    }
    pub fn binding_purpose(self, purpose: &str) -> String {
        if self == Self::Gmail {
            purpose.to_owned()
        } else {
            format!("{}:{purpose}", self.as_str())
        }
    }
}
use MemberGoogleService as Service;
#[derive(Serialize)]
pub struct MemberGoogleStatus {
    pub status: String,
    pub connected: bool,
    pub mailbox: Option<String>,
    pub connection_id: Option<Uuid>,
    pub epoch: i64,
    pub credential_version: i64,
    pub app_revision: Option<Uuid>,
    pub refresh_pending: bool,
    pub expires_at: Option<i64>,
    pub needs_refresh: bool,
    pub read_only: bool,
    pub send_enabled: bool,
}
pub struct MemberGoogleGrant {
    request_id: Uuid,
    raw: Zeroizing<Vec<u8>>,
}
impl MemberGoogleGrant {
    pub fn from_broker_reply(request_id: Uuid, raw: Zeroizing<Vec<u8>>) -> Result<Self> {
        if request_id.is_nil() || raw.len() > 128 * 1024 {
            return Err(StoreError::InvalidInput);
        }
        Ok(Self { request_id, raw })
    }
}
pub(super) struct Expected {
    service: Service,
    pub owner: VolumeOwner,
    pub app: Uuid,
}
#[derive(Clone, Copy)]
pub(super) struct ExchangePin {
    service: Service,
    pub owner: VolumeOwner,
    pub app: Uuid,
    pub version: i64,
    pub epoch: i64,
    pub request_id: Uuid,
}
struct Row {
    service: Service,
    owner: VolumeOwner,
    app: Uuid,
    app_version: i64,
    id: Option<Uuid>,
    epoch: i64,
    version: i64,
    auth_epoch: i64,
    digest: Vec<u8>,
    mailbox: Option<String>,
    status: String,
    cipher: Option<Vec<u8>>,
    expires: Option<i64>,
    nonce: Option<Vec<u8>>,
    refresh_id: Option<Uuid>,
    phase: String,
}
fn parse_id(s: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&s).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn row(c: &Connection, u: &UserContext, service: Service) -> Result<Option<Row>> {
    row_ids(c, &u.tenant.0, &u.account.id, service)
}
/// Same lookup keyed by ids only. Used by the purpose-limited background reader, which has no session.
fn row_ids(c: &Connection, tenant: &str, account: &str, service: Service) -> Result<Option<Row>> {
    c.query_row("SELECT volume_id,generation,app_revision,app_version,connection_id,epoch,credential_version,auth_epoch,consent_digest,mailbox,status,cipher,expires_at,refresh_nonce,refresh_request_id,refresh_phase FROM member_google_grants WHERE tenant_id=?1 AND account_id=?2 AND service=?3",params![tenant,account,service.as_str()],|r|Ok(Row{service,owner:VolumeOwner{account:parse_id(account.to_owned())?,tenant:parse_id(tenant.to_owned())?,volume:parse_id(r.get(0)?)?,generation:r.get::<_,i64>(1)? as u64},app:parse_id(r.get(2)?)?,app_version:r.get(3)?,id:r.get::<_,Option<String>>(4)?.map(parse_id).transpose()?,epoch:r.get(5)?,version:r.get(6)?,auth_epoch:r.get(7)?,digest:r.get(8)?,mailbox:r.get(9)?,status:r.get(10)?,cipher:r.get(11)?,expires:r.get(12)?,nonce:r.get(13)?,refresh_id:r.get::<_,Option<String>>(14)?.map(parse_id).transpose()?,phase:r.get(15)?})).optional().map_err(Into::into)
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
fn matches(r: &Row, o: VolumeOwner, app: Uuid) -> bool {
    r.owner.account == o.account
        && r.owner.tenant == o.tenant
        && r.owner.volume == o.volume
        && r.owner.generation == o.generation
        && r.app == app
}
fn status(r: Option<&Row>, expected: Option<Uuid>, u: &UserContext) -> MemberGoogleStatus {
    let changed =
        r.is_some_and(|r| expected.is_some_and(|a| a != r.app) || r.auth_epoch != u.epoch);
    MemberGoogleStatus {
        status: if changed {
            "app_changed".into()
        } else {
            r.map(|r| r.status.clone()).unwrap_or("absent".into())
        },
        connected: r.is_some_and(|r| !changed && r.status == "active"),
        mailbox: r.and_then(|r| r.mailbox.clone()),
        connection_id: r.and_then(|r| r.id),
        epoch: r.map(|r| r.epoch).unwrap_or(0),
        credential_version: r.map(|r| r.version).unwrap_or(0),
        app_revision: r.map(|r| r.app),
        refresh_pending: r.is_some_and(|r| r.phase == "dispatched"),
        expires_at: r.and_then(|r| r.expires),
        needs_refresh: r.is_some_and(|r| !changed && r.status == "active" && needs_refresh(r)),
        // Public DTO invariant shared by every service: no mail send/Drive write. The
        // Calendar write capability is gated by `Service::writes` plus the ledger.
        read_only: true,
        send_enabled: false,
    }
}
fn needs_refresh(r: &Row) -> bool {
    r.expires
        .is_none_or(|e| e <= now().unwrap_or(i64::MAX).saturating_add(60))
}
/// Single enforcement point for losing a member Google source (disconnect, app
/// replacement, refused/interrupted refresh, access loss found at restart). Purges
/// only that member's work scoped to that service; the other service, other
/// members and already-dispatched mail history are untouched.
pub(super) fn source_lost(
    c: &Connection,
    tenant: &str,
    account: &str,
    service: &str,
) -> Result<()> {
    if let Some(s) = [Service::Gmail, Service::Calendar]
        .into_iter()
        .find(|s| s.as_str() == service)
    {
        super::member_briefs::withdraw(c, tenant, account, Some(s))?;
    }
    match service {
        "gmail" => {
            reads::withdraw(c, tenant, account)?;
            super::member_mail_work::withdraw_account(c, tenant, account)?;
        }
        "calendar" => calendar::withdraw(c, tenant, account)?,
        _ => {}
    }
    Ok(())
}
pub(super) fn restart(c: &Connection) -> Result<()> {
    let lost: Vec<(String, String, String)> = c
        .prepare("SELECT tenant_id,account_id,service FROM member_google_grants WHERE refresh_phase='dispatched' OR status IN('needs_reauth','disconnected')")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (t, a, s) in &lost {
        source_lost(c, t, a, s)?;
    }
    c.execute("UPDATE member_google_thread_reads SET phase='unknown',cipher=NULL,updated_at=unixepoch() WHERE phase='dispatched'",[])?;
    c.execute("UPDATE member_google_grants SET status='needs_reauth',cipher=NULL,expires_at=NULL,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='unknown' WHERE refresh_phase='dispatched'", [])?;
    calendar::restart(c)?;
    super::member_briefs::restart(c)?;
    Ok(())
}
fn clear_flows(c: &Connection, u: &UserContext, service: Service) -> Result<()> {
    c.execute("DELETE FROM mail_oauth_sessions WHERE state_hash IN(SELECT state_hash FROM member_google_oauth WHERE tenant_id=?1 AND account_id=?2 AND service=?3)",params![u.tenant.0,u.account.id,service.as_str()])?;
    Ok(())
}
fn sync_app(
    c: &Connection,
    u: &UserContext,
    o: VolumeOwner,
    app: Uuid,
    service: Service,
) -> Result<Row> {
    if app.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    let current: Option<(String,i64)> = c.query_row("SELECT app_revision,app_version FROM member_google_apps WHERE tenant_id=?1 AND account_id=?2 ORDER BY app_version DESC LIMIT 1", params![u.tenant.0,u.account.id], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    let app_id = app.to_string();
    let version = match current {
        Some((previous, version)) if previous == app_id => version,
        previous => {
            let next = previous.as_ref().map_or(1, |(_, v)| v.saturating_add(1));
            let used:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM member_google_apps WHERE tenant_id=?1 AND account_id=?2 AND app_revision=?3)",params![u.tenant.0,u.account.id,app_id],|r|r.get(0))?;
            if used || next > 64 {
                return Err(StoreError::Conflict);
            }
            c.execute(
                "INSERT INTO member_google_apps VALUES(?1,?2,?3,?4)",
                params![u.tenant.0, u.account.id, app_id, next],
            )?;
            if previous.is_some() {
                c.execute("UPDATE member_google_grants SET app_revision=?3,app_version=?4,epoch=epoch+1,credential_version=credential_version+1,status='needs_reauth',cipher=NULL,expires_at=NULL,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='idle' WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id,app_id,next])?;
                c.execute("DELETE FROM mail_oauth_sessions WHERE state_hash IN(SELECT state_hash FROM member_google_oauth WHERE tenant_id=?1 AND account_id=?2)",params![u.tenant.0,u.account.id])?;
                source_lost(c, &u.tenant.0, &u.account.id, "gmail")?;
                source_lost(c, &u.tenant.0, &u.account.id, "calendar")?;
            }
            next
        }
    };
    c.execute("INSERT INTO member_google_grants VALUES(?1,?2,?3,?4,?5,?6,NULL,0,0,?7,?8,NULL,'absent',NULL,NULL,NULL,NULL,'idle',?9) ON CONFLICT(tenant_id,account_id,service) DO NOTHING",params![u.tenant.0,u.account.id,o.volume.to_string(),o.generation as i64,app_id,version,u.epoch,u.digest.as_slice(),service.as_str()])?;
    let r = row(c, u, service)?.ok_or(StoreError::Corrupt)?;
    if !matches(&r, o, app) {
        return Err(StoreError::Conflict);
    }
    Ok(r)
}
pub(super) fn claim_in(
    c: &Connection,
    u: &UserContext,
    state: &[u8],
    e: Expected,
) -> Result<ExchangePin> {
    authority(c, u, e.owner)?;
    let r = row(c, u, e.service)?.ok_or(StoreError::Conflict)?;
    if !matches(&r, e.owner, e.app) {
        return Err(StoreError::Conflict);
    }
    let saved:(String,i64,String,i64,i64)=c.query_row("SELECT volume_id,generation,app_revision,app_version,epoch FROM member_google_oauth WHERE state_hash=?1 AND tenant_id=?2 AND account_id=?3 AND service=?4",params![state,u.tenant.0,u.account.id,e.service.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or(StoreError::Conflict)?;
    if saved
        != (
            e.owner.volume.to_string(),
            e.owner.generation as i64,
            e.app.to_string(),
            r.app_version,
            r.epoch,
        )
    {
        return Err(StoreError::Conflict);
    }
    let mut h = Sha256::new();
    h.update(b"moosuri/member-google/exchange/v1\0");
    h.update(state);
    let hash = h.finalize();
    let mut id: [u8; 16] = hash[..16].try_into().map_err(|_| StoreError::Corrupt)?;
    id[6] = (id[6] & 15) | 0x50;
    id[8] = (id[8] & 63) | 0x80;
    Ok(ExchangePin {
        service: e.service,
        owner: e.owner,
        app: e.app,
        version: r.app_version,
        epoch: r.epoch,
        request_id: Uuid::from_bytes(id),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireOwner {
    account_id: Uuid,
    tenant_id: Uuid,
    volume_id: Uuid,
    generation: i64,
    peer_uid: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    v: u32,
    owner: WireOwner,
    app_revision: Uuid,
    purpose: String,
    subject: Option<String>,
    mailbox: String,
    token_tag: String,
    expires_at: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTokens {
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    expires_in: u64,
    scopes: Vec<String>,
    mailbox: String,
    access_binding: String,
    refresh_binding: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    v: u32,
    request_id: Uuid,
    owner: WireOwner,
    app_revision: Uuid,
    state: String,
    result: WireTokens,
}
fn owns(w: &WireOwner, o: VolumeOwner) -> bool {
    w.account_id == o.account
        && w.tenant_id == o.tenant
        && w.volume_id == o.volume
        && w.generation == o.generation as i64
        && w.peer_uid == unsafe { libc::geteuid() }
}
fn claims(
    binding: &str,
    o: VolumeOwner,
    app: Uuid,
    service: Service,
    purpose: &str,
    mailbox: &str,
) -> Result<Claims> {
    if binding.len() > 4096 {
        return Err(StoreError::InvalidInput);
    }
    let (payload, sig) = binding.split_once('.').ok_or(StoreError::InvalidInput)?;
    if URL_SAFE_NO_PAD
        .decode(sig)
        .map_err(|_| StoreError::InvalidInput)?
        .len()
        != 32
    {
        return Err(StoreError::InvalidInput);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| StoreError::InvalidInput)?;
    let c: Claims = serde_json::from_slice(&bytes).map_err(|_| StoreError::InvalidInput)?;
    let now = now()?;
    if c.v != 1
        || !owns(&c.owner, o)
        || c.app_revision != app
        || c.purpose != service.binding_purpose(purpose)
        || match service {
            Service::Gmail => c.subject.is_some(),
            _ => c.subject.as_ref().is_none_or(|s| {
                s.is_empty()
                    || s.len() > 255
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            }),
        }
        || !c.mailbox.eq_ignore_ascii_case(mailbox)
        || c.expires_at <= now as u64
        || c.expires_at > (now as u64 + 401 * 24 * 3600)
        || URL_SAFE_NO_PAD
            .decode(&c.token_tag)
            .map_err(|_| StoreError::InvalidInput)?
            .len()
            != 32
    {
        return Err(StoreError::Forbidden);
    }
    Ok(c)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    access_token: SecretString,
    refresh_token: SecretString,
    access_binding: String,
    refresh_binding: String,
}
impl Serialize for Payload {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut out = s.serialize_struct("PrivateReadonlyGoogle", 4)?;
        out.serialize_field("access_token", self.access_token.expose())?;
        out.serialize_field("refresh_token", self.refresh_token.expose())?;
        out.serialize_field("access_binding", &self.access_binding)?;
        out.serialize_field("refresh_binding", &self.refresh_binding)?;
        out.end()
    }
}
fn grant(
    g: MemberGoogleGrant,
    expected: Uuid,
    o: VolumeOwner,
    app: Uuid,
    service: Service,
    previous: Option<(&str, Option<&str>, &SecretString)>,
) -> Result<(Payload, String, i64)> {
    if g.request_id != expected {
        return Err(StoreError::Conflict);
    }
    let r: Reply = serde_json::from_slice(&g.raw).map_err(|_| StoreError::InvalidInput)?;
    if r.v != 1
        || r.request_id != expected
        || r.app_revision != app
        || !owns(&r.owner, o)
        || r.state != "ok"
        || r.result.scopes != [service.scope()]
        || r.result.expires_in == 0
        || r.result.expires_in > 86400
    {
        return Err(StoreError::Forbidden);
    }
    let w = r.result;
    let email = email_valid(w.mailbox)?;
    if previous.is_some_and(|(mailbox, _, _)| !mailbox.eq_ignore_ascii_case(&email)) {
        return Err(StoreError::Conflict);
    }
    let access_claims = claims(&w.access_binding, o, app, service, "access", &email)?;
    let expires = i64::try_from(access_claims.expires_at).map_err(|_| StoreError::InvalidInput)?;
    if expires
        > now()?
            .checked_add(w.expires_in as i64 + 60)
            .ok_or(StoreError::InvalidInput)?
    {
        return Err(StoreError::Forbidden);
    }
    let refresh_claims = claims(&w.refresh_binding, o, app, service, "refresh", &email)?;
    if refresh_claims.subject != access_claims.subject
        || previous.is_some_and(|(_, subject, _)| subject != refresh_claims.subject.as_deref())
    {
        return Err(StoreError::Forbidden);
    }
    let refresh = match w.refresh_token {
        Some(v) => v,
        None => SecretString::new(previous.ok_or(StoreError::Forbidden)?.2.expose().to_owned())
            .map_err(|_| StoreError::InvalidInput)?,
    };
    Ok((
        Payload {
            access_token: w.access_token,
            refresh_token: refresh,
            access_binding: w.access_binding,
            refresh_binding: w.refresh_binding,
        },
        email,
        expires,
    ))
}
fn aad(r: &Row) -> Result<Vec<u8>> {
    let domain = if r.service == Service::Gmail {
        "moosuri/member-google/readonly/v1".to_owned()
    } else {
        // Distinct AAD domain: a write-scope ciphertext can never decrypt as a read grant.
        let kind = if r.service.writes() {
            "write"
        } else {
            "readonly"
        };
        format!("moosuri/member-google/{}/{kind}/v1", r.service.as_str())
    };
    serde_json::to_vec(&(
        domain,
        r.owner.tenant,
        r.owner.account,
        r.owner.volume,
        r.owner.generation,
        r.app,
        r.app_version,
        r.id,
        r.epoch,
        r.version,
        r.auth_epoch,
        &r.digest,
    ))
    .map_err(|_| StoreError::Corrupt)
}
fn decrypt(key: &[u8; 32], r: &Row) -> Result<Payload> {
    let bytes = providers::decrypt(
        key,
        r.cipher.as_deref().ok_or(StoreError::Conflict)?,
        &aad(r)?,
    )?;
    serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt)
}
fn encrypt(key: &[u8; 32], r: &Row, p: &Payload) -> Result<Vec<u8>> {
    let bytes = Zeroizing::new(serde_json::to_vec(p).map_err(|_| StoreError::Corrupt)?);
    providers::encrypt(key, &bytes, &aad(r)?)
}
pub struct MemberGoogleBinding {
    service: Service,
    pub access_token: SecretString,
    pub access_binding: String,
    pub mailbox: String,
    pub connection_id: Uuid,
    pub epoch: i64,
    pub credential_version: i64,
    pub expires_at: i64,
    pub needs_refresh: bool,
    user: UserContext,
    owner: VolumeOwner,
    app: Uuid,
    app_version: i64,
}
pub struct MemberGoogleRefresh {
    service: Service,
    subject: Option<String>,
    pub request_id: Uuid,
    pub refresh_token: SecretString,
    pub refresh_binding: String,
    user: UserContext,
    owner: VolumeOwner,
    app: Uuid,
    app_version: i64,
    id: Uuid,
    epoch: i64,
    version: i64,
    nonce: [u8; 32],
}
fn live(
    c: &Connection,
    u: &UserContext,
    o: VolumeOwner,
    app: Uuid,
    service: Service,
    allow_expired: bool,
) -> Result<Row> {
    let u = authority(c, u, o)?;
    let r = row(c, &u, service)?.ok_or(StoreError::NotFound)?;
    if !matches(&r, o, app)
        || r.status != "active"
        || r.auth_epoch != u.epoch
        || !allow_expired && r.expires.is_none_or(|e| e <= now().unwrap_or(i64::MAX))
    {
        return Err(StoreError::Conflict);
    }
    Ok(r)
}
impl Store {
    pub async fn start_member_google_oauth(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        app: Uuid,
    ) -> Result<MailOAuthStart> {
        self.start_member_service_oauth(u, o, Service::Gmail, app)
            .await
    }
    pub async fn start_member_service_oauth(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        app: Uuid,
    ) -> Result<MailOAuthStart> {
        self.start_member_service_oauth_for_account(u, o, service, app, false)
            .await
    }
    pub async fn start_member_service_oauth_for_account(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        app: Uuid,
        match_member_account: bool,
    ) -> Result<MailOAuthStart> {
        self.start_member_service_oauth_with_selection(
            u,
            o,
            service,
            app,
            match_member_account,
            None,
        )
        .await
    }
    pub async fn start_member_service_oauth_with_selection(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        app: Uuid,
        match_member_account: bool,
        selected_email: Option<String>,
    ) -> Result<MailOAuthStart> {
        if match_member_account && selected_email.is_some() {
            return Err(StoreError::InvalidInput);
        }
        let selected_email = selected_email.map(email_valid).transpose()?;
        let user = u.clone();
        let pin = self
            .call(move |db| {
                let tx = db
                    .conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                let u = authority(&tx, &user, o)?;
                let r = sync_app(&tx, &u, o, app, service)?;
                tx.commit()?;
                Ok((r.app_version, r.epoch))
            })
            .await?;
        let start = self.start_mail_oauth(u).await?;
        let state = start.state.clone();
        let user = u.clone();
        let bound = self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &user, o)?;
            let r = row(&tx, &u, service)?.ok_or(StoreError::Conflict)?;
            if !matches(&r, o, app) || (r.app_version, r.epoch) != pin {
                return Err(StoreError::Conflict);
            }
            let digest = hash(&state);
            let exact: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_oauth_sessions WHERE state_hash=?1 AND tenant_id=?2 AND account_id=?3 AND session_digest=?4 AND auth_epoch=?5 AND compose=0 AND claimed=0 AND expires_at>unixepoch())", params![digest,u.tenant.0,u.account.id,u.digest.as_slice(),u.epoch], |r| r.get(0))?;
            if !exact { return Err(StoreError::Conflict); }
            tx.execute("INSERT INTO member_google_oauth VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![digest,u.tenant.0,u.account.id,o.volume.to_string(),o.generation as i64,app.to_string(),pin.0,pin.1,service.as_str()])?;
            if match_member_account || selected_email.is_some() {
                tx.execute("INSERT INTO member_google_account_choices(state_hash,selected_email) VALUES(?1,?2)", params![digest,selected_email])?;
            }
            tx.commit()?;
            Ok(())
        }).await;
        if let Err(e) = bound {
            let _ = self.cancel_mail_oauth_state(start.state.clone()).await;
            return Err(e);
        }
        Ok(start)
    }
    pub async fn claim_member_google_oauth(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        app: Uuid,
        handle: String,
    ) -> Result<MailOAuthExchange> {
        self.claim_member_service_oauth(u, o, Service::Gmail, app, handle)
            .await
    }
    pub async fn claim_member_service_oauth(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        app: Uuid,
        handle: String,
    ) -> Result<MailOAuthExchange> {
        self.claim_mail_oauth_inner(
            u,
            handle,
            None,
            Some(Expected {
                owner: o,
                app,
                service,
            }),
        )
        .await
    }
    pub async fn complete_member_google_oauth(
        &self,
        e: MailOAuthExchange,
        g: MemberGoogleGrant,
    ) -> Result<MemberGoogleStatus> {
        self.call(move |db| {
            let p = e.member_google.ok_or(StoreError::Forbidden)?;
            let service=p.service;
            if e.compose || e.app.is_some() { return Err(StoreError::Forbidden); }
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = match authority(&tx, &e.user, p.owner) {
                Ok(u) => u,
                Err(err) => {
                    tx.execute("DELETE FROM mail_oauth_sessions WHERE state_hash=?1", [&e.state])?;
                    tx.commit()?;
                    return Err(err);
                }
            };
            let mut r = row(&tx, &u, service)?.ok_or(StoreError::Conflict)?;
            if !matches(&r, p.owner, p.app) || (r.app_version, r.epoch) != (p.version, p.epoch) {
                return Err(StoreError::Conflict);
            }
            let flow = claim_in(&tx, &u, &e.state, Expected { owner: p.owner, app: p.app, service })?;
            if flow.request_id != p.request_id { return Err(StoreError::Conflict); }
            let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM mail_oauth_sessions WHERE state_hash=?1 AND claimed=1 AND expires_at>unixepoch() AND session_digest=?2 AND auth_epoch=?3)",params![e.state,u.digest.as_slice(),u.epoch],|r|r.get(0))?;
            if !valid { return Err(StoreError::Conflict); }
            let (payload, mailbox, expires) = grant(g, p.request_id, p.owner, p.app, service, None)?;
            let choice: Option<Option<String>> = tx.query_row(
                "SELECT selected_email FROM member_google_account_choices WHERE state_hash=?1",
                [&e.state], |row| row.get(0),
            ).optional()?;
            if choice.is_some_and(|email| !mailbox.eq_ignore_ascii_case(email.as_deref().unwrap_or(&u.account.email))) {
                tx.execute("DELETE FROM mail_oauth_sessions WHERE state_hash=?1", [&e.state])?;
                tx.commit()?;
                return Err(StoreError::Conflict);
            }
            r.id = Some(Uuid::new_v4());
            r.epoch = r.epoch.checked_add(1).ok_or(StoreError::Corrupt)?;
            r.version = r.version.checked_add(1).ok_or(StoreError::Corrupt)?;
            r.auth_epoch = u.epoch;
            r.digest = u.digest.to_vec();
            let cipher = encrypt(&db.provider_key, &r, &payload)?;
            tx.execute("UPDATE member_google_grants SET connection_id=?3,epoch=?4,credential_version=?5,auth_epoch=?6,consent_digest=?7,mailbox=?8,status='active',cipher=?9,expires_at=?10,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='idle' WHERE tenant_id=?1 AND account_id=?2 AND app_version=?11 AND epoch=?12 AND service=?13",params![u.tenant.0,u.account.id,r.id.map(|v|v.to_string()),r.epoch,r.version,u.epoch,u.digest.as_slice(),mailbox,cipher,expires,p.version,p.epoch,service.as_str()])?;
            if service==Service::Gmail {reads::connected(&tx,&u,p.owner,p.app,&e.flow_id(),r.id.ok_or(StoreError::Corrupt)?,r.epoch)?;}
            clear_flows(&tx, &u, service)?;
            let result = status(row(&tx, &u, service)?.as_ref(), Some(p.app), &u);
            tx.commit()?;
            Ok(result)
        }).await
    }
    pub async fn member_google_status(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        app: Uuid,
    ) -> Result<MemberGoogleStatus> {
        self.member_service_status(u, o, Service::Gmail, app).await
    }
    pub async fn member_service_status(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        app: Uuid,
    ) -> Result<MemberGoogleStatus> {
        let u = u.clone();
        self.call(move |db| {
            let u = authority(&db.conn, &u, o)?;
            if app.is_nil() {
                return Err(StoreError::InvalidInput);
            }
            let r = row(&db.conn, &u, service)?;
            if r.as_ref()
                .is_some_and(|r| r.owner.volume != o.volume || r.owner.generation != o.generation)
            {
                return Err(StoreError::Conflict);
            }
            Ok(status(r.as_ref(), Some(app), &u))
        })
        .await
    }
    pub async fn member_google_status_local(
        &self,
        u: &UserContext,
        o: VolumeOwner,
    ) -> Result<MemberGoogleStatus> {
        self.member_service_status_local(u, o, Service::Gmail).await
    }
    pub async fn member_service_status_local(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
    ) -> Result<MemberGoogleStatus> {
        let u = u.clone();
        self.call(move |db| {
            let u = authority(&db.conn, &u, o)?;
            let r = row(&db.conn, &u, service)?;
            if r.as_ref()
                .is_some_and(|r| r.owner.volume != o.volume || r.owner.generation != o.generation)
            {
                return Err(StoreError::Conflict);
            }
            Ok(status(r.as_ref(), None, &u))
        })
        .await
    }
    pub async fn member_google_read_binding(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        app: Uuid,
    ) -> Result<MemberGoogleBinding> {
        self.member_service_read_binding(u, o, Service::Gmail, app)
            .await
    }
    pub async fn member_service_read_binding(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        app: Uuid,
    ) -> Result<MemberGoogleBinding> {
        let u = u.clone();
        self.call(move |db| {
            let r = live(&db.conn, &u, o, app, service, true)?;
            let p = decrypt(&db.provider_key, &r)?;
            Ok(MemberGoogleBinding {
                service,
                access_token: p.access_token,
                access_binding: p.access_binding,
                expires_at: r.expires.ok_or(StoreError::Corrupt)?,
                needs_refresh: needs_refresh(&r),
                mailbox: r.mailbox.ok_or(StoreError::Corrupt)?,
                connection_id: r.id.ok_or(StoreError::Corrupt)?,
                epoch: r.epoch,
                credential_version: r.version,
                user: u,
                owner: o,
                app,
                app_version: r.app_version,
            })
        })
        .await
    }
    pub async fn check_member_google_read(&self, b: &MemberGoogleBinding) -> Result<()> {
        let u = b.user.clone();
        let service = b.service;
        let (o, app, id, epoch, version, app_version) = (
            b.owner,
            b.app,
            b.connection_id,
            b.epoch,
            b.credential_version,
            b.app_version,
        );
        self.call(move |db| {
            let r = live(&db.conn, &u, o, app, service, false)?;
            if r.id != Some(id)
                || r.epoch != epoch
                || r.version != version
                || r.app_version != app_version
            {
                return Err(StoreError::Conflict);
            }
            Ok(())
        })
        .await
    }
    pub async fn claim_member_google_refresh(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        app: Uuid,
    ) -> Result<MemberGoogleRefresh> {
        self.claim_member_service_refresh(u, o, Service::Gmail, app)
            .await
    }
    pub async fn claim_member_service_refresh(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        app: Uuid,
    ) -> Result<MemberGoogleRefresh> {
        let u = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r = live(&tx, &u, o, app, service, true)?;
            if r.phase != "idle" { return Err(StoreError::Conflict); }
            let p = decrypt(&db.provider_key, &r)?;
            let verified=claims(&p.refresh_binding, o, app, service, "refresh", r.mailbox.as_deref().ok_or(StoreError::Corrupt)?)?;
            let mut nonce = [0u8; 32];
            OsRng.try_fill_bytes(&mut nonce).map_err(|_| StoreError::Unavailable)?;
            let request_id = Uuid::new_v4();
            tx.execute("UPDATE member_google_grants SET refresh_nonce=?3,refresh_request_id=?4,refresh_phase='dispatched' WHERE tenant_id=?1 AND account_id=?2 AND epoch=?5 AND credential_version=?6 AND refresh_phase='idle' AND service=?7",params![u.tenant.0,u.account.id,nonce.as_slice(),request_id.to_string(),r.epoch,r.version,service.as_str()])?;
            tx.commit()?;
            Ok(MemberGoogleRefresh {
                service, subject:verified.subject,
                request_id, refresh_token: p.refresh_token, refresh_binding: p.refresh_binding,
                user: u, owner: o, app, app_version: r.app_version,
                id: r.id.ok_or(StoreError::Corrupt)?, epoch: r.epoch, version: r.version, nonce,
            })
        }).await
    }
    pub async fn settle_member_google_refresh(
        &self,
        lease: MemberGoogleRefresh,
        g: Option<MemberGoogleGrant>,
    ) -> Result<MemberGoogleStatus> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let service=lease.service;
            lease.owner.check_identity(&tx, None)?;
            let r = row(&tx, &lease.user, service)?.ok_or(StoreError::Conflict)?;
            if !matches(&r, lease.owner, lease.app) || r.app_version != lease.app_version
                || r.id != Some(lease.id) || r.epoch != lease.epoch || r.version != lease.version
                || r.nonce.as_deref() != Some(lease.nonce.as_slice())
                || r.refresh_id != Some(lease.request_id) || r.phase != "dispatched" {
                return Err(StoreError::Conflict);
            }
            let valid = authority(&tx, &lease.user, lease.owner);
            let payload = g.ok_or(StoreError::Conflict).and_then(|g| grant(
                g, lease.request_id, lease.owner, lease.app, service,
                Some((r.mailbox.as_deref().ok_or(StoreError::Corrupt)?,lease.subject.as_deref(),&lease.refresh_token)),
            ));
            let result = match (valid, payload) {
                (Ok(u), Ok((payload, mailbox, expires))) => {
                    let mut next = r;
                    next.version = next.version.checked_add(1).ok_or(StoreError::Corrupt)?;
                    let cipher = encrypt(&db.provider_key, &next, &payload)?;
                    tx.execute("UPDATE member_google_grants SET credential_version=?3,mailbox=?4,cipher=?5,expires_at=?6,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='idle' WHERE tenant_id=?1 AND account_id=?2 AND epoch=?7 AND credential_version=?8 AND service=?9",params![u.tenant.0,u.account.id,next.version,mailbox,cipher,expires,next.epoch,lease.version,service.as_str()])?;
                    status(row(&tx, &u, service)?.as_ref(), Some(lease.app), &u)
                }
                (user, _) => {
                    // A dispatched refresh may have rotated externally. No old token retry.
                    let lost = tx.execute("UPDATE member_google_grants SET status='needs_reauth',cipher=NULL,expires_at=NULL,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='unknown' WHERE tenant_id=?1 AND account_id=?2 AND epoch=?3 AND credential_version=?4 AND service=?5",params![lease.user.tenant.0,lease.user.account.id,lease.epoch,lease.version,service.as_str()])?;
                    if lost == 1 {
                        source_lost(&tx, &lease.user.tenant.0, &lease.user.account.id, service.as_str())?;
                    }
                    if let Err(err) = user { tx.commit()?; return Err(err); }
                    status(row(&tx, &lease.user, service)?.as_ref(), Some(lease.app), &lease.user)
                }
            };
            tx.commit()?;
            Ok(result)
        }).await
    }
    pub async fn disconnect_member_google(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        expected_id: Uuid,
        expected_epoch: i64,
    ) -> Result<MemberGoogleStatus> {
        self.disconnect_member_service(u, o, Service::Gmail, expected_id, expected_epoch)
            .await
    }
    pub async fn disconnect_member_service(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        service: Service,
        expected_id: Uuid,
        expected_epoch: i64,
    ) -> Result<MemberGoogleStatus> {
        let u = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &u, o)?;
            let r = row(&tx, &u, service)?.ok_or(StoreError::Conflict)?;
            if r.owner.volume != o.volume || r.owner.generation != o.generation
                || r.id != Some(expected_id) || r.epoch != expected_epoch {
                return Err(StoreError::Conflict);
            }
            let changed = tx.execute("UPDATE member_google_grants SET epoch=epoch+1,credential_version=credential_version+1,status='disconnected',cipher=NULL,expires_at=NULL,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='idle' WHERE tenant_id=?1 AND account_id=?2 AND connection_id=?3 AND epoch=?4 AND service=?5",params![u.tenant.0,u.account.id,expected_id.to_string(),expected_epoch,service.as_str()])?;
            if changed != 1 { return Err(StoreError::Conflict); }
            source_lost(&tx, &u.tenant.0, &u.account.id, service.as_str())?;
            clear_flows(&tx, &u, service)?;
            let result = status(row(&tx, &u, service)?.as_ref(), None, &u);
            tx.commit()?;
            Ok(result)
        }).await
    }
}

#[cfg(test)]
mod tests;
