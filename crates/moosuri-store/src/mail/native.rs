//! Existing approved mail actions, transported by one signed native device.
//! No tokens, arbitrary MIME/URL admission, or second approval endpoint.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use moosuri_credential_broker::auth;
use moosuri_vault_protocol::broker::*;

fn owner(conn: &Connection, p: &PairChallenge) -> Result<UserContext> {
    let tenant = p.device.owner.tenant.to_string();
    let account = p.device.owner.account.to_string();
    let (summary,epoch):(AccountSummary,i64)=conn.query_row("SELECT id,name,email,role,must_change_password,auth_epoch FROM accounts WHERE tenant_id=?1 AND id=?2 AND status='active' AND recovery_required=0 AND must_change_password=0 AND (temporary_expires_at IS NULL OR temporary_expires_at>unixepoch())",params![tenant,account],|r|Ok((summary_row(r)?,r.get(5)?))).optional()?.ok_or(StoreError::Forbidden)?;
    let user = UserContext {
        account: summary,
        tenant: TenantId(tenant),
        epoch,
        restricted: false,
        helper: false,
        absolute_deadline: None,
        digest: [0; 32],
    };
    if vault_authority::owner(conn, &user)? != p.device.owner {
        return Err(StoreError::Forbidden);
    }
    Ok(user)
}
pub(crate) fn register(
    conn: &Connection,
    p: &PairChallenge,
    user: &UserContext,
    binding: &Binding,
    email: &str,
    expected: Option<&Binding>,
) -> Result<()> {
    if !email.is_ascii()
        || email.len() > 254
        || !email.contains('@')
        || email.chars().any(char::is_control)
        || binding.id.is_nil()
        || binding.expires_at <= now()?
        || binding.epoch == 0
        || binding.grant_version == 0
        || binding.item_epoch == 0
        || binding.key_epoch == 0
        || binding.grant_hash == [0; 32]
    {
        return Err(StoreError::InvalidInput);
    }
    let device = p.device.id.to_string();
    let id = binding.id.to_string();
    let old: Option<String> = conn
        .query_row(
            "SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2",
            params![device, id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        let current: Binding = parse(&old)?;
        if &current != binding {
            let expected = expected.ok_or(StoreError::Conflict)?;
            if &current != expected
                || binding.id != expected.id
                || binding.epoch != expected.epoch.checked_add(1).ok_or(StoreError::Conflict)?
                || binding.grant_version
                    != expected
                        .grant_version
                        .checked_add(1)
                        .ok_or(StoreError::Conflict)?
                || binding.item_epoch < expected.item_epoch
                || binding.key_epoch < expected.key_epoch
                || binding.expires_at != expected.expires_at
                || binding.grant_hash == expected.grant_hash
            {
                return Err(StoreError::Conflict);
            }
            conn.execute("UPDATE credential_broker_bindings SET binding=?3 WHERE device_id=?1 AND binding_id=?2",params![device,id,json(binding)?])?;
            // Same device/account/scope only; committed requests retain their exact version.
            let rows=conn.prepare("SELECT e.effect_id,e.request FROM native_mail_effects e WHERE e.device_id=?1 AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments c WHERE c.tenant_id=e.tenant_id AND c.run_id=e.effect_id)")?.query_map([&device],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            for (effect, text) in rows {
                let mut request: Request = parse(&text)?;
                if request.binding == *expected {
                    request.binding = binding.clone();
                    let encoded = json(&request)?;
                    conn.execute("UPDATE native_mail_effects SET request=?3 WHERE tenant_id=?1 AND effect_id=?2",params![user.tenant.0,effect,encoded])?;
                    conn.execute("UPDATE credential_broker_invocations SET request=?3 WHERE tenant_id=?1 AND run_id=?2 AND cancelled=0",params![user.tenant.0,effect,encoded])?;
                }
            }
        }
    } else {
        if expected.is_some() {
            return Err(StoreError::Conflict);
        }
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM credential_broker_bindings WHERE device_id=?1",
            [&device],
            |r| r.get(0),
        )?;
        if count >= 256 {
            return Err(StoreError::Busy);
        }
        conn.execute(
            "INSERT INTO credential_broker_bindings VALUES(?1,?2,?3,1)",
            params![device, id, json(binding)?],
        )?;
    }
    let old:Option<(String,bool)>=conn.query_row("SELECT email,native_send FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![user.tenant.0,user.account.id,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if old
        .as_ref()
        .is_some_and(|(saved, native)| saved != email || !*native)
    {
        return Err(StoreError::Conflict);
    }
    let scopes = json(&vec![
        "openid",
        "email",
        moosuri_connectors::gmail::READ_SCOPE,
        moosuri_connectors::gmail::SEND_SCOPE,
    ])?;
    conn.execute("INSERT INTO mail_connections(tenant_id,account_id,id,email,status,credential,expires_at,scopes,native_send) VALUES(?1,?2,?3,?4,'active',NULL,?5,?6,1) ON CONFLICT(tenant_id,id) DO UPDATE SET status='active',expires_at=excluded.expires_at",params![user.tenant.0,user.account.id,id,email,binding.expires_at,scopes])?;
    conn.execute("INSERT INTO native_mail_connections VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tenant_id,connection_id) DO UPDATE SET auth_epoch=excluded.auth_epoch",params![user.tenant.0,id,device,id,user.epoch])?;
    Ok(())
}
fn current_connection(
    conn: &Connection,
    p: &PairChallenge,
    user: &UserContext,
    id: &str,
) -> Result<Binding> {
    let (raw,epoch):(String,i64)=conn.query_row("SELECT b.binding,n.auth_epoch FROM native_mail_connections n JOIN mail_connections c ON c.tenant_id=n.tenant_id AND c.id=n.connection_id JOIN credential_broker_devices d ON d.id=n.device_id JOIN credential_broker_bindings b ON b.device_id=n.device_id AND b.binding_id=n.binding_id WHERE n.tenant_id=?1 AND n.connection_id=?2 AND n.device_id=?3 AND c.account_id=?4 AND c.native_send=1 AND c.status='active' AND b.active=1 AND d.status='active'",params![user.tenant.0,id,p.device.id.to_string(),user.account.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::Forbidden)?;
    let binding: Binding = parse(&raw)?;
    if epoch != user.epoch || binding.expires_at <= now()? {
        return Err(StoreError::Forbidden);
    }
    Ok(binding)
}
fn effect(
    conn: &Connection,
    request: &Request,
) -> Result<(String, GoogleMailPayload, Option<Vec<u8>>)> {
    let tenant = request.device.owner.tenant.to_string();
    let id = request.invocation.run.to_string();
    let(action,raw,stored,lease):(String,String,String,Option<Vec<u8>>)=conn.query_row("SELECT action_id,payload,request,lease_nonce FROM native_mail_effects WHERE tenant_id=?1 AND effect_id=?2 AND device_id=?3",params![tenant,id,request.device.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::Forbidden)?;
    if parse::<Request>(&stored)? != *request {
        return Err(StoreError::Forbidden);
    }
    let payload: GoogleMailPayload = parse(&raw)?;
    let expected = auth::hash(&payload.execution_bytes());
    if !matches!(request.operation,Operation::GoogleMailSend{effect,execution}|Operation::GoogleMailLookup{effect,execution} if effect==request.invocation.run&&execution==expected)
    {
        return Err(StoreError::Forbidden);
    }
    Ok((action, payload, lease))
}
fn permit(
    conn: &Connection,
    user: &UserContext,
    action: &str,
    nonce: Vec<u8>,
) -> Result<MailReconcilePermit> {
    let(connection_id,message_id,fingerprint,reply_thread):(String,String,Option<String>,Option<String>)=conn.query_row("SELECT connection_id,message_id,fingerprint,reply_thread FROM mail_send_evidence WHERE tenant_id=?1 AND action_id=?2 AND account_id=?3",params![user.tenant.0,action,user.account.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    Ok(MailReconcilePermit {
        tenant: user.tenant.0.clone(),
        account: user.account.id.clone(),
        action: action.into(),
        nonce,
        connection_id,
        message_id,
        fingerprint,
        reply_thread,
        app: None,
    })
}
pub(crate) fn guard(
    conn: &Connection,
    p: &PairChallenge,
    request: &Request,
    committed: bool,
) -> Result<UserContext> {
    let user = owner(conn, p)?;
    let (action, payload, lease) = effect(conn, request)?;
    if request.device != p.device
        || request.expires_at <= now()?
        || current_connection(conn, p, &user, &payload.connection.to_string())? != request.binding
    {
        return Err(StoreError::Forbidden);
    }
    let a = action_record(conn, &user, &action)?;
    if a.revision as u64 != payload.revision
        || digest_bytes(&a.digest)? != payload.mime_digest
        || a.connection_id != payload.connection.to_string()
    {
        return Err(StoreError::Conflict);
    }
    if matches!(request.operation, Operation::GoogleMailSend { .. }) {
        let state = if committed {
            "dispatch_committed"
        } else {
            "approved"
        };
        if a.state != state
            || a.expires_at != payload.expires_at
            || a.expires_at <= now()?
            || payload.not_before > now()?
            || !workflows::grant_valid(conn, &user, &action)?
        {
            return Err(StoreError::Conflict);
        }
        let pins: (i64, i64) = conn.query_row(
            "SELECT connection_epoch,auth_epoch FROM mail_actions WHERE tenant_id=?1 AND id=?2",
            params![user.tenant.0, action],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if pins != (payload.connection_epoch as i64, payload.auth_epoch)
            || pins.1 != user.epoch
            || connection(conn, &user, &a.connection_id)?.epoch != pins.0
        {
            return Err(StoreError::Conflict);
        }
        let schedule = workflows::schedule(conn, &user, &action)?;
        if schedule.as_ref().map(|s| s.due_at).unwrap_or(0) != payload.not_before
            || schedule.as_ref().map(|s| s.timezone.clone()) != payload.timezone
        {
            return Err(StoreError::Conflict);
        }
        let withdrawn:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_send_evidence WHERE tenant_id=?1 AND action_id=?2 AND withdrawn=1)",params![user.tenant.0,action],|r|r.get(0))?;
        if withdrawn {
            return Err(StoreError::Forbidden);
        }
    } else {
        reconcile::guard(
            conn,
            &permit(conn, &user, &action, lease.ok_or(StoreError::Corrupt)?)?,
        )?;
    }
    Ok(user)
}
// Alias avoids the action row's local binding name shadowing the parent helper.
fn action_record(conn: &Connection, user: &UserContext, id: &str) -> Result<MailAction> {
    super::action(conn, user, id)
}
fn enqueue(
    conn: &Connection,
    p: &PairChallenge,
    user: &UserContext,
    a: &MailAction,
    lookup: Option<MailReconcilePermit>,
) -> Result<()> {
    let binding = current_connection(conn, p, user, &a.connection_id)?;
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM native_mail_effects WHERE tenant_id=?1",
        [&user.tenant.0],
        |r| r.get(0),
    )?;
    if count >= 1024 {
        return Err(StoreError::Busy);
    }
    let payload = if lookup.is_some() {
        let raw:String=conn.query_row("SELECT payload FROM native_mail_effects WHERE tenant_id=?1 AND action_id=?2 AND kind='send' AND revision=?3",params![user.tenant.0,a.id,a.revision],|r|r.get(0))?;
        parse(&raw)?
    } else {
        let (raw, auth_epoch, connection_epoch): (Vec<u8>, i64, i64) = conn.query_row(
            "SELECT raw,auth_epoch,connection_epoch FROM mail_actions WHERE tenant_id=?1 AND id=?2",
            params![user.tenant.0, a.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let frozen = FrozenMail::restore(raw, digest_bytes(&a.digest)?, None)
            .map_err(|_| StoreError::Corrupt)?;
        moosuri_connectors::gmail::validate_native_send(
            frozen.raw_mime(),
            &a.message_id,
            &connection(conn, user, &a.connection_id)?.email,
        )
        .map_err(|_| StoreError::InvalidInput)?;
        GoogleMailPayload {
            owner: vault_authority::owner(conn, user)?,
            action: a.id.parse().map_err(|_| StoreError::Corrupt)?,
            revision: a.revision as u64,
            connection: a.connection_id.parse().map_err(|_| StoreError::Corrupt)?,
            connection_epoch: u64::try_from(connection_epoch).map_err(|_| StoreError::Corrupt)?,
            auth_epoch,
            mime_digest: frozen.digest(),
            message_id: a.message_id.clone(),
            not_before: a.schedule.as_ref().map(|s| s.due_at).unwrap_or(0),
            timezone: a.schedule.as_ref().map(|s| s.timezone.clone()),
            expires_at: a.expires_at,
            fingerprint: moosuri_connectors::gmail::reconciliation_fingerprint(
                frozen.raw_mime(),
                &a.message_id,
            )
            .map_err(|_| StoreError::Corrupt)?,
            raw: None,
        }
    };
    let id = Uuid::new_v4();
    let execution = auth::hash(&payload.execution_bytes());
    let expires = if lookup.is_some() {
        now()? + 90
    } else {
        a.expires_at
    };
    let request = Request {
        device: p.device.clone(),
        invocation: Invocation {
            run: id,
            step: 1,
            call: Uuid::new_v4(),
            nonce: Uuid::new_v4(),
        },
        binding: binding.clone(),
        operation: if lookup.is_some() {
            Operation::GoogleMailLookup {
                effect: id,
                execution,
            }
        } else {
            Operation::GoogleMailSend {
                effect: id,
                execution,
            }
        },
        repository: None,
        digest: moosuri_credential_broker::google::send_digest(),
        expires_at: expires.min(binding.expires_at),
    };
    let nonce = lookup.as_ref().map(|p| p.nonce.clone());
    let encoded = json(&request)?;
    conn.execute(
        "INSERT INTO native_mail_effects(tenant_id,action_id,effect_id,kind,device_id,request,payload,lease_nonce,revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            user.tenant.0,
            a.id,
            id.to_string(),
            if lookup.is_some() { "lookup" } else { "send" },
            p.device.id.to_string(),
            encoded,
            json(&payload)?,
            nonce, a.revision
        ],
    )?;
    conn.execute(
        "INSERT INTO credential_broker_invocations VALUES(?1,?2,1,?3,?4,?5,0)",
        params![
            user.tenant.0,
            id.to_string(),
            request.invocation.call.to_string(),
            request.invocation.nonce.to_string(),
            encoded
        ],
    )?;
    Ok(())
}
pub(crate) fn poll(conn: &Connection, p: &PairChallenge, status: &str) -> Result<Reply> {
    if status != "active" {
        return Ok(Reply::Waiting {
            requests: vec![],
            revocation_pending: true,
        });
    }
    let user = owner(conn, p)?;
    conn.execute("UPDATE mail_actions SET version=version+1,state='expired',raw=NULL,content=NULL WHERE tenant_id=?1 AND state='approved' AND expires_at<=?2",params![user.tenant.0,now()?])?;
    let ids=conn.prepare("SELECT a.id FROM mail_actions a JOIN native_mail_connections n ON n.tenant_id=a.tenant_id AND n.connection_id=a.connection_id LEFT JOIN mail_schedules s ON s.tenant_id=a.tenant_id AND s.action_id=a.id JOIN mail_connections current_mail ON current_mail.tenant_id=a.tenant_id AND current_mail.id=a.connection_id JOIN credential_broker_bindings current_binding ON current_binding.device_id=n.device_id AND current_binding.binding_id=n.binding_id WHERE current_mail.status='active' AND current_mail.native_send=1 AND current_mail.epoch=a.connection_epoch AND n.auth_epoch=a.auth_epoch AND current_binding.active=1 AND json_extract(current_binding.binding,'$.expires_at')>?4 AND a.tenant_id=?1 AND a.account_id=?2 AND n.device_id=?3 AND a.state='approved' AND a.expires_at>?4 AND (s.action_id IS NULL OR s.due_at<=?4) AND NOT EXISTS(SELECT 1 FROM native_mail_effects e WHERE e.tenant_id=a.tenant_id AND e.action_id=a.id AND e.revision=a.revision AND e.kind='send') ORDER BY a.rowid LIMIT 8")?.query_map(params![user.tenant.0,user.account.id,p.device.id.to_string(),now()?],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        let a = action_record(conn, &user, &id)?;
        if current_connection(conn, p, &user, &a.connection_id).is_ok() {
            enqueue(conn, p, &user, &a, None)?;
        }
    }
    // Reuse the persisted original automatic budgets; offline waiting does not create another send.
    let due=conn.prepare("SELECT a.id FROM mail_actions a JOIN native_mail_connections n ON n.tenant_id=a.tenant_id AND n.connection_id=a.connection_id JOIN mail_reconciliations r ON r.tenant_id=a.tenant_id AND r.action_id=a.id WHERE a.tenant_id=?1 AND a.account_id=?2 AND n.device_id=?3 AND a.state='outcome_unknown' AND r.quarantined=0 AND r.next_due<=?4 AND r.automatic_attempts<4 AND EXISTS(SELECT 1 FROM mail_connections current_mail JOIN credential_broker_bindings current_binding ON current_binding.device_id=n.device_id AND current_binding.binding_id=n.binding_id WHERE current_mail.tenant_id=a.tenant_id AND current_mail.id=a.connection_id AND current_mail.status='active' AND current_mail.epoch=a.connection_epoch AND current_binding.active=1 AND json_extract(current_binding.binding,'$.expires_at')>?4) AND (r.lease_until IS NULL OR r.lease_until<=?4) ORDER BY r.next_due LIMIT 4")?.query_map(params![user.tenant.0,user.account.id,p.device.id.to_string(),now()?],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for id in due {
        let a = action_record(conn, &user, &id)?;
        if current_connection(conn, p, &user, &a.connection_id).is_err() {
            continue;
        }
        conn.execute_batch("SAVEPOINT native_mail_lookup")?;
        match reconcile::claim(conn, user.tenant.0.clone(), id.clone(), false)
            .and_then(|lease| enqueue(conn, p, &user, &a, Some(lease)))
        {
            Ok(()) => conn.execute_batch("RELEASE native_mail_lookup")?,
            Err(StoreError::Conflict | StoreError::Forbidden) => {
                conn.execute_batch("ROLLBACK TO native_mail_lookup; RELEASE native_mail_lookup")?;
                conn.execute("UPDATE mail_reconciliations SET next_due=NULL WHERE tenant_id=?1 AND action_id=?2",params![user.tenant.0,id])?;
            }
            Err(StoreError::Busy) => {
                conn.execute_batch("ROLLBACK TO native_mail_lookup; RELEASE native_mail_lookup")?
            }
            Err(error) => return Err(error),
        }
    }
    let rows=conn.prepare("SELECT i.request FROM credential_broker_invocations i JOIN native_mail_effects e ON e.tenant_id=i.tenant_id AND e.effect_id=i.run_id WHERE e.tenant_id=?1 AND e.device_id=?2 AND i.cancelled=0 AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments c WHERE c.tenant_id=i.tenant_id AND c.run_id=i.run_id)")?.query_map(params![user.tenant.0,p.device.id.to_string()],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    // Admission caps each tenant at1024 effects. Validate/retire the bounded
    // set before applying the returned-request limit, so stale edits cannot
    // occupy the front32 forever. A writer-proven uncommitted row cannot enter.
    let mut requests = Vec::new();
    for raw in rows {
        let request: Request = parse(&raw)?;
        match guard(conn, p, &request, false) {
            Ok(_) => {
                if requests.len() < 32 {
                    requests.push(request)
                }
            }
            Err(StoreError::Conflict | StoreError::Forbidden) => {
                conn.execute("UPDATE credential_broker_invocations SET cancelled=1 WHERE tenant_id=?1 AND run_id=?2 AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments c WHERE c.tenant_id=?1 AND c.run_id=?2)",params![user.tenant.0,request.invocation.run.to_string()])?;
                let (action, payload, _) = effect(conn, &request)?;
                if matches!(request.operation, Operation::GoogleMailSend { .. }) {
                    let revision =
                        i64::try_from(payload.revision).map_err(|_| StoreError::Corrupt)?;
                    conn.execute("UPDATE mail_actions SET version=version+1,state=?4,raw=NULL,content=NULL WHERE tenant_id=?1 AND id=?2 AND revision=?3 AND state='approved'",params![user.tenant.0,action,revision,if request.expires_at<=now()? {"expired"}else{"cancelled"}])?;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Reply::Waiting {
        requests,
        revocation_pending: false,
    })
}
pub(crate) fn payload(conn: &Connection, p: &PairChallenge, request: &Request) -> Result<Reply> {
    let user = guard(conn, p, request, false)?;
    let (action, mut payload, _) = effect(conn, request)?;
    if matches!(request.operation, Operation::GoogleMailSend { .. }) {
        let raw: Vec<u8> = conn.query_row(
            "SELECT raw FROM mail_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
            params![user.tenant.0, user.account.id, action],
            |r| r.get(0),
        )?;
        if raw.len() > 512 * 1024 {
            return Err(StoreError::InvalidInput);
        }
        payload.raw = Some(URL_SAFE_NO_PAD.encode(raw));
    }
    Ok(Reply::MailPayload {
        payload: Box::new(payload),
    })
}
pub(crate) fn commit(conn: &Connection, p: &PairChallenge, c: &Commitment) -> Result<()> {
    let user = guard(conn, p, &c.request, false)?;
    let (action, payload, _) = effect(conn, &c.request)?;
    if matches!(c.request.operation, Operation::GoogleMailSend { .. }) {
        let raw: Vec<u8> = conn.query_row(
            "SELECT raw FROM mail_actions WHERE tenant_id=?1 AND id=?2",
            params![user.tenant.0, action],
            |r| r.get(0),
        )?;
        let frozen =
            FrozenMail::restore(raw, payload.mime_digest, None).map_err(|_| StoreError::Corrupt)?;
        reconcile::freeze(conn, &user, &action, c.nonce.as_bytes(), &frozen)?;
        if conn.execute("UPDATE mail_actions SET version=version+1,state='dispatch_committed',dispatch_nonce=?3,raw=NULL WHERE tenant_id=?1 AND id=?2 AND state='approved'",params![user.tenant.0,action,c.nonce.as_bytes().as_slice()])?!=1{return Err(StoreError::Conflict)}
    }
    Ok(())
}
pub(crate) fn abandon(conn: &Connection, request: &Request) -> Result<()> {
    if !matches!(
        request.operation,
        Operation::GoogleMailSend { .. } | Operation::GoogleMailLookup { .. }
    ) {
        return Ok(());
    }
    let (action, payload, _) = effect(conn, request)?;
    if matches!(request.operation, Operation::GoogleMailSend { .. }) {
        let revision = i64::try_from(payload.revision).map_err(|_| StoreError::Corrupt)?;
        conn.execute("UPDATE mail_actions SET version=version+1,state='cancelled',content=NULL,raw=NULL WHERE tenant_id=?1 AND id=?2 AND revision=?3 AND state='approved'",params![request.device.owner.tenant.to_string(),action,revision])?;
    }
    Ok(())
}
pub(crate) fn settle(conn: &Connection, receipt: &SignedReceipt) -> Result<()> {
    let r = &receipt.receipt;
    let request = &r.commitment.request;
    if !matches!(
        request.operation,
        Operation::GoogleMailSend { .. } | Operation::GoogleMailLookup { .. }
    ) {
        return Ok(());
    }
    let (action, _, lease) = effect(conn, request)?;
    let tenant = request.device.owner.tenant.to_string();
    if matches!(request.operation, Operation::GoogleMailSend { .. }) {
        let (state, message, thread) = match (&r.outcome, &r.projection) {
            (
                Outcome::ProjectedSuccess,
                Projection::GoogleMailSent {
                    message_id,
                    thread_id,
                },
            ) => ("sent", Some(message_id), Some(thread_id)),
            (Outcome::NotDispatched | Outcome::Rejected, _) => ("failed_definite", None, None),
            _ => ("outcome_unknown", None, None),
        };
        conn.execute("UPDATE mail_actions SET version=version+1,state=?4,provider_message=?5,provider_thread=?6,content=CASE WHEN ?4='outcome_unknown' THEN NULL ELSE content END,raw=NULL WHERE tenant_id=?1 AND id=?2 AND dispatch_nonce=?3 AND state IN('dispatch_committed','outcome_unknown')",params![tenant,action,r.commitment.nonce.as_bytes().as_slice(),state,message,thread])?;
    } else {
        // Late lookup evidence is admissible only under its still-current read lease.
        let p: PairChallenge = conn
            .query_row(
                "SELECT enrollment FROM credential_broker_devices WHERE id=?1",
                [request.device.id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .and_then(|s| serde_json::from_str(&s).map_err(|_| rusqlite::Error::InvalidQuery))?;
        if guard(conn, &p, request, true).is_err() {
            return Ok(());
        }
        let nonce = lease.ok_or(StoreError::Corrupt)?;
        let (status, count, message, thread) = match (&r.outcome, &r.projection) {
            (
                Outcome::ProjectedSuccess,
                Projection::GoogleMailObserved {
                    status,
                    candidates,
                    message_id,
                    thread_id,
                },
            ) => (
                match status {
                    GoogleSentStatus::Matching => "matching_sent_observed",
                    GoogleSentStatus::NoCandidates => "no_candidates",
                    GoogleSentStatus::MultipleCandidates => "multiple_candidates",
                    GoogleSentStatus::IncompleteSearch => "incomplete_search",
                    GoogleSentStatus::CandidateMismatch => "candidate_mismatch",
                    GoogleSentStatus::UnsupportedCandidate => "unsupported_candidate",
                    GoogleSentStatus::LegacyEvidenceUnavailable => "legacy_evidence_unavailable",
                    GoogleSentStatus::LookupUnavailable => "lookup_unavailable",
                },
                *candidates,
                message_id.as_deref(),
                thread_id.as_deref(),
            ),
            _ => ("lookup_unavailable", 0, None, None),
        };
        reconcile::finish_observation(
            conn,
            &tenant,
            &action,
            &nonce,
            status,
            count as u32,
            message,
            thread,
        )?;
    }
    Ok(())
}
pub(crate) fn revoke(conn: &Connection, device: &str) -> Result<()> {
    conn.execute("UPDATE mail_actions SET version=version+1,state='cancelled',content=NULL,raw=NULL WHERE state IN('pending_approval','approved') AND (tenant_id,connection_id) IN(SELECT tenant_id,connection_id FROM native_mail_connections WHERE device_id=?1)",[device])?;
    conn.execute("UPDATE mail_connections SET status='needs_reconnect',epoch=epoch+1 WHERE (tenant_id,id) IN(SELECT tenant_id,connection_id FROM native_mail_connections WHERE device_id=?1)",[device])?;
    Ok(())
}
impl Store {
    pub async fn queue_native_mail_reconciliation(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<bool> {
        let user = user.clone();
        self.call(move|db|{
  validate(&db.conn,&user,true,false)?;let a=action_record(&db.conn,&user,&id)?;if !connection(&db.conn,&user,&a.connection_id)?.native_send{return Ok(false)}
  let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
  let encoded:String=tx.query_row("SELECT d.enrollment FROM native_mail_connections n JOIN credential_broker_devices d ON d.id=n.device_id WHERE n.tenant_id=?1 AND n.connection_id=?2",params![user.tenant.0,a.connection_id],|r|r.get(0))?;let p:PairChallenge=parse(&encoded)?;current_connection(&tx,&p,&user,&a.connection_id)?;
  let lease=reconcile::claim(&tx,user.tenant.0.clone(),id,true)?;enqueue(&tx,&p,&user,&a,Some(lease))?;tx.commit()?;Ok(true)
 }).await
    }
}

#[cfg(test)]
mod tests;

// Restore authority is quarantined with broker bindings; no old approval is revived.
pub(crate) fn quarantine(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE credential_broker_invocations SET cancelled=1 WHERE (tenant_id,run_id) IN(SELECT e.tenant_id,e.effect_id FROM native_mail_effects e WHERE NOT EXISTS(SELECT 1 FROM credential_broker_commitments c WHERE c.tenant_id=e.tenant_id AND c.run_id=e.effect_id))",[])?;
    conn.execute("UPDATE mail_actions SET version=version+1,state='cancelled',content=NULL,raw=NULL WHERE state IN('pending_approval','approved') AND (tenant_id,connection_id) IN(SELECT tenant_id,id FROM mail_connections WHERE native_send=1)",[])?;
    conn.execute("UPDATE mail_connections SET status='needs_reconnect',epoch=epoch+1 WHERE native_send=1 AND status='active'",[])?;
    Ok(())
}
