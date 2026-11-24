//! Purpose-limited background read authority for the connected morning brief.
//!
//! The brief timer has no member session and must never fabricate one. This module is the only
//! place that may open a member Google grant without a `UserContext`, and it is deliberately narrow:
//!
//! * services: Gmail and Calendar only, and only to *read* (callers use inbox metadata / `list`);
//! * the grant must be pinned: exact connection id, connection epoch and app revision recorded when the
//!   member explicitly granted the brief, plus the account `auth_epoch` recorded then;
//! * the account must still be active with no forced password change or recovery;
//! * a changed epoch, disconnect, refresh failure, app change or password/session reset fails closed.
//!
//! Refresh mirrors the interactive protocol (dispatch marker committed before the broker call, an
//! unknown outcome withdraws the grant, no old-token retry).
use super::*;
use crate::mail::member_briefs::MemberBriefLease;

/// The exact Google connection a brief was granted against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberBriefSource {
    pub service: Service,
    pub connection_id: Uuid,
    pub epoch: i64,
    pub app_revision: Uuid,
}

fn briefable(service: Service) -> Result<()> {
    // Never Drive/Docs/Slides/Sheets. Calendar is the existing grant; callers only ever `list`.
    matches!(service, Service::Gmail | Service::Calendar)
        .then_some(())
        .ok_or(StoreError::Forbidden)
}

fn account_current(c: &Connection, owner: VolumeOwner, auth_epoch: i64) -> Result<()> {
    owner.check_identity(c, None)?;
    let ok: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts a WHERE a.id=?1 AND a.tenant_id=?2 AND a.auth_epoch=?3
            AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)",
        params![owner.account.to_string(), owner.tenant.to_string(), auth_epoch],
        |r| r.get(0),
    )?;
    ok.then_some(()).ok_or(StoreError::Unauthorized)
}

/// Grant-time capture with a live member session: the exact connection the member is looking at.
pub(in crate::mail) fn capture(
    c: &Connection,
    u: &UserContext,
    o: VolumeOwner,
    service: Service,
    shown_connection: Uuid,
    shown_epoch: i64,
) -> Result<MemberBriefSource> {
    briefable(service)?;
    let u = authority(c, u, o)?;
    let r = row(c, &u, service)?.ok_or(StoreError::Conflict)?;
    if r.status != "active"
        || r.auth_epoch != u.epoch
        || r.id != Some(shown_connection)
        || r.epoch != shown_epoch
        || r.owner.volume != o.volume
        || r.owner.generation != o.generation
    {
        return Err(StoreError::Conflict);
    }
    Ok(MemberBriefSource {
        service,
        connection_id: shown_connection,
        epoch: shown_epoch,
        app_revision: r.app,
    })
}

fn pinned(
    c: &Connection,
    o: VolumeOwner,
    auth_epoch: i64,
    s: &MemberBriefSource,
    allow_expired: bool,
) -> Result<Row> {
    briefable(s.service)?;
    account_current(c, o, auth_epoch)?;
    let r = row_ids(c, &o.tenant.to_string(), &o.account.to_string(), s.service)?
        .ok_or(StoreError::NotFound)?;
    if !matches(&r, o, s.app_revision)
        || r.status != "active"
        || r.auth_epoch != auth_epoch
        || r.id != Some(s.connection_id)
        || r.epoch != s.epoch
        || !allow_expired && r.expires.is_none_or(|e| e <= now().unwrap_or(i64::MAX))
    {
        return Err(StoreError::Conflict);
    }
    Ok(r)
}

/// In-transaction liveness of a pinned source (no secrets touched).
pub(in crate::mail) fn source_live(
    c: &Connection,
    o: VolumeOwner,
    auth_epoch: i64,
    s: &MemberBriefSource,
) -> bool {
    pinned(c, o, auth_epoch, s, true).is_ok()
}

/// A Google connection may also be used interactively. A background reader additionally needs
/// a currently claimed slot for this exact brief and pinned service.
fn claimed(
    c: &Connection,
    owner: VolumeOwner,
    epoch: i64,
    brief: Uuid,
    due: i64,
    source: &MemberBriefSource,
) -> Result<()> {
    let ok: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM member_connected_briefs b
        JOIN member_connected_brief_runs r ON r.tenant_id=b.tenant_id AND r.account_id=b.account_id AND r.brief_id=b.id
        JOIN threads t ON t.tenant_id=b.tenant_id AND t.account_id=b.account_id AND t.id=b.thread_id
        WHERE b.tenant_id=?1 AND b.account_id=?2 AND b.volume_id=?3 AND b.generation=?4 AND b.auth_epoch=?5
          AND b.id=?6 AND r.due_at=?7 AND r.state='claimed' AND t.mission_internal=0 AND b.expires_at>unixepoch()
          AND b.state='active'
          AND ((?8='gmail' AND b.gmail_connection=?9 AND b.gmail_epoch=?10 AND b.gmail_app=?11)
            OR (?8='calendar' AND b.calendar_connection=?9 AND b.calendar_epoch=?10 AND b.calendar_app=?11)))",
        params![owner.tenant.to_string(),owner.account.to_string(),owner.volume.to_string(),owner.generation as i64,
                epoch,brief.to_string(),due,source.service.as_str(),source.connection_id.to_string(),source.epoch,source.app_revision.to_string()],
        |row|row.get(0))?;
    ok.then_some(()).ok_or(StoreError::Conflict)
}

/// Background access token. Neither `Debug` nor `Serialize`.
pub struct MemberBackgroundBinding {
    pub service: Service,
    pub access_token: SecretString,
    pub access_binding: String,
    pub mailbox: String,
    pub app_revision: Uuid,
    pub needs_refresh: bool,
    source: MemberBriefSource,
    owner: VolumeOwner,
    auth_epoch: i64,
    brief: Uuid,
    due: i64,
}
pub struct MemberBackgroundRefresh {
    pub service: Service,
    pub request_id: Uuid,
    pub app_revision: Uuid,
    pub refresh_token: SecretString,
    pub refresh_binding: String,
    subject: Option<String>,
    owner: VolumeOwner,
    auth_epoch: i64,
    app_version: i64,
    source: MemberBriefSource,
    version: i64,
    nonce: [u8; 32],
}

impl Store {
    /// Decrypt the pinned grant for one read. Fails closed on any epoch/connection mismatch.
    pub async fn member_brief_binding(
        &self,
        lease: &MemberBriefLease,
        s: MemberBriefSource,
    ) -> Result<MemberBackgroundBinding> {
        let (owner, auth_epoch, brief, due) =
            (lease.owner, lease.auth_epoch, lease.brief_id, lease.due_at);
        self.call(move |db| {
            claimed(&db.conn, owner, auth_epoch, brief, due, &s)?;
            let r = pinned(&db.conn, owner, auth_epoch, &s, true)?;
            let p = decrypt(&db.provider_key, &r)?;
            Ok(MemberBackgroundBinding {
                service: s.service,
                access_token: p.access_token,
                access_binding: p.access_binding,
                mailbox: r.mailbox.clone().ok_or(StoreError::Corrupt)?,
                app_revision: r.app,
                needs_refresh: needs_refresh(&r),
                source: s,
                owner,
                auth_epoch,
                brief,
                due,
            })
        })
        .await
    }

    /// Re-check after provider IO: a disconnect/epoch change while reading discards the result.
    pub async fn check_member_brief_binding(&self, b: &MemberBackgroundBinding) -> Result<()> {
        let (owner, epoch, s) = (b.owner, b.auth_epoch, b.source);
        let (brief, due) = (b.brief, b.due);
        self.call(move |db| {
            claimed(&db.conn, owner, epoch, brief, due, &s)?;
            pinned(&db.conn, owner, epoch, &s, false).map(|_| ())
        })
        .await
    }

    pub async fn claim_member_brief_refresh(
        &self,
        lease: &MemberBriefLease,
        s: MemberBriefSource,
    ) -> Result<MemberBackgroundRefresh> {
        let (owner, auth_epoch, brief, due) =
            (lease.owner, lease.auth_epoch, lease.brief_id, lease.due_at);
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            claimed(&tx, owner, auth_epoch, brief, due, &s)?;
            let r = pinned(&tx, owner, auth_epoch, &s, true)?;
            if r.phase != "idle" {
                return Err(StoreError::Conflict);
            }
            let p = decrypt(&db.provider_key, &r)?;
            let verified = claims(
                &p.refresh_binding,
                owner,
                s.app_revision,
                s.service,
                "refresh",
                r.mailbox.as_deref().ok_or(StoreError::Corrupt)?,
            )?;
            let mut nonce = [0u8; 32];
            OsRng
                .try_fill_bytes(&mut nonce)
                .map_err(|_| StoreError::Unavailable)?;
            let request_id = Uuid::new_v4();
            let changed = tx.execute(
                "UPDATE member_google_grants SET refresh_nonce=?3,refresh_request_id=?4,refresh_phase='dispatched' WHERE tenant_id=?1 AND account_id=?2 AND epoch=?5 AND credential_version=?6 AND refresh_phase='idle' AND service=?7",
                params![owner.tenant.to_string(), owner.account.to_string(), nonce.as_slice(), request_id.to_string(), r.epoch, r.version, s.service.as_str()],
            )?;
            if changed != 1 {
                return Err(StoreError::Conflict);
            }
            tx.commit()?;
            Ok(MemberBackgroundRefresh {
                service: s.service,
                request_id,
                app_revision: s.app_revision,
                refresh_token: p.refresh_token,
                refresh_binding: p.refresh_binding,
                subject: verified.subject,
                owner,
                auth_epoch,
                app_version: r.app_version,
                source: s,
                version: r.version,
                nonce,
            })
        })
        .await
    }

    /// `None` (or any failure) withdraws the grant: a dispatched refresh may have rotated upstream.
    pub async fn settle_member_brief_refresh(
        &self,
        lease: MemberBackgroundRefresh,
        g: Option<MemberGoogleGrant>,
    ) -> Result<()> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (t, a) = (lease.owner.tenant.to_string(), lease.owner.account.to_string());
            lease.owner.check_identity(&tx, None)?;
            let r = row_ids(&tx, &t, &a, lease.service)?.ok_or(StoreError::Conflict)?;
            if !matches(&r, lease.owner, lease.app_revision)
                || r.app_version != lease.app_version
                || r.id != Some(lease.source.connection_id)
                || r.epoch != lease.source.epoch
                || r.version != lease.version
                || r.nonce.as_deref() != Some(lease.nonce.as_slice())
                || r.refresh_id != Some(lease.request_id)
                || r.phase != "dispatched"
            {
                return Err(StoreError::Conflict);
            }
            let valid = account_current(&tx, lease.owner, lease.auth_epoch);
            let payload = g.ok_or(StoreError::Conflict).and_then(|g| {
                grant(
                    g,
                    lease.request_id,
                    lease.owner,
                    lease.app_revision,
                    lease.service,
                    Some((
                        r.mailbox.as_deref().ok_or(StoreError::Corrupt)?,
                        lease.subject.as_deref(),
                        &lease.refresh_token,
                    )),
                )
            });
            match (valid, payload) {
                (Ok(()), Ok((payload, mailbox, expires))) => {
                    let mut next = r;
                    next.version = next.version.checked_add(1).ok_or(StoreError::Corrupt)?;
                    let cipher = encrypt(&db.provider_key, &next, &payload)?;
                    tx.execute(
                        "UPDATE member_google_grants SET credential_version=?3,mailbox=?4,cipher=?5,expires_at=?6,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='idle' WHERE tenant_id=?1 AND account_id=?2 AND epoch=?7 AND credential_version=?8 AND service=?9",
                        params![t, a, next.version, mailbox, cipher, expires, next.epoch, lease.version, lease.service.as_str()],
                    )?;
                    tx.commit()?;
                    Ok(())
                }
                (_, _) => {
                    tx.execute(
                        "UPDATE member_google_grants SET status='needs_reauth',cipher=NULL,expires_at=NULL,refresh_nonce=NULL,refresh_request_id=NULL,refresh_phase='unknown' WHERE tenant_id=?1 AND account_id=?2 AND epoch=?3 AND credential_version=?4 AND service=?5",
                        params![t, a, lease.source.epoch, lease.version, lease.service.as_str()],
                    )?;
                    source_lost(&tx, &t, &a, lease.service.as_str())?;
                    tx.commit()?;
                    Err(StoreError::Conflict)
                }
            }
        })
        .await
    }
}
