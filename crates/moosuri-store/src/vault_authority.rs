//! Narrow online sidecar authority; no vault ciphertext or unlock secret is accepted.
use super::*;
use moosuri_vault_protocol::{AuthorityReply, AuthorityRequest, Handoff, Owner};

pub(crate) fn owner(conn: &Connection, user: &UserContext) -> Result<Owner> {
    let installation: String = conn.query_row(
        "SELECT installation_id FROM backup_installation WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    Ok(Owner {
        account: Uuid::parse_str(&user.account.id).map_err(|_| StoreError::Corrupt)?,
        tenant: Uuid::parse_str(&user.tenant.0).map_err(|_| StoreError::Corrupt)?,
        installation: Uuid::parse_str(&installation).map_err(|_| StoreError::Corrupt)?,
        epoch: user.epoch,
    })
}
fn token_digest(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(StoreError::Unauthorized);
    }
    Ok(digest(value))
}
type GrantRow = (Vec<u8>, String, String, i64, String);
fn grant_owner(conn: &Connection, token: &str) -> Result<Owner> {
    let row: GrantRow = conn.query_row("SELECT session_digest,account_id,tenant_id,auth_epoch,installation_id FROM vault_grants WHERE digest=?1 AND closed=0 AND expires_at>?2", params![token_digest(token)?.as_slice(), now()?], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or(StoreError::Unauthorized)?;
    let session: [u8; 32] = row.0.try_into().map_err(|_| StoreError::Corrupt)?;
    let user = load_context(conn, session, false)?;
    let user = validate(conn, &user, true, false)?;
    let current = owner(conn, &user)?;
    if current.account.to_string() != row.1
        || current.tenant.to_string() != row.2
        || current.epoch != row.3
        || current.installation.to_string() != row.4
    {
        return Err(StoreError::Unauthorized);
    }
    Ok(current)
}
impl Store {
    pub async fn vault_handoff(&self, user: &UserContext, vault_origin: String) -> Result<Handoff> {
        let user = user.clone();
        self.call(move |db| {
            // vault_origin comes only from trusted ServerConfig, never request input.
            if vault_origin.len() > 256 {
                return Err(StoreError::InvalidInput);
            }
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let user = validate(&tx, &user, true, false)?;
            let identity = owner(&tx, &user)?;
            tx.execute("DELETE FROM vault_handoffs WHERE expires_at<?1", [now()?])?;
            let pending: i64 = tx.query_row(
                "SELECT count(*) FROM vault_handoffs WHERE account_id=?1 AND consumed=0",
                [&user.account.id],
                |r| r.get(0),
            )?;
            if pending >= 16 {
                return Err(StoreError::Busy);
            }
            let token = random_token()?;
            let expires = now()?.checked_add(30).ok_or(StoreError::Unavailable)?;
            tx.execute(
                "INSERT INTO vault_handoffs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0)",
                params![
                    digest(&token).as_slice(),
                    user.digest.as_slice(),
                    user.account.id,
                    user.tenant.0,
                    user.epoch,
                    identity.installation.to_string(),
                    vault_origin,
                    expires
                ],
            )?;
            tx.commit()?;
            Ok(Handoff {
                token,
                vault_origin,
                expires_at: expires,
            })
        })
        .await
    }
    /// Called only by the separately authenticated internal adapter router.
    pub async fn vault_authority(&self, request: AuthorityRequest) -> Result<AuthorityReply> {
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let result=match request {
                AuthorityRequest::Exchange{handoff,vault_origin} => {
                    let token=Zeroizing::new(handoff);
                    let hash=token_digest(&token)?;
                    let row:GrantRow=tx.query_row("SELECT session_digest,account_id,tenant_id,auth_epoch,installation_id FROM vault_handoffs WHERE digest=?1 AND vault_origin=?2 AND expires_at>?3 AND consumed=0",params![hash.as_slice(),vault_origin,now()?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or(StoreError::Unauthorized)?;
                    let session:[u8;32]=row.0.try_into().map_err(|_|StoreError::Corrupt)?;
                    let u=load_context(&tx,session,false)?;let u=validate(&tx,&u,true,false)?;let who=owner(&tx,&u)?;
                    if who.account.to_string()!=row.1 || who.tenant.to_string()!=row.2 || who.epoch!=row.3 || who.installation.to_string()!=row.4 { return Err(StoreError::Unauthorized); }
                    tx.execute("DELETE FROM vault_grants WHERE expires_at<?1 OR closed=1",[now()?])?;
                    let count:i64=tx.query_row("SELECT count(*) FROM vault_grants WHERE account_id=?1",[&u.account.id],|r|r.get(0))?;
                    if count>=64 { return Err(StoreError::Busy); }
                    let grant=random_token()?;
                    tx.execute("UPDATE vault_handoffs SET consumed=1 WHERE digest=?1",[hash.as_slice()])?;
                    tx.execute("INSERT INTO vault_grants VALUES(?1,?2,?3,?4,?5,?6,?7,0)",params![digest(&grant).as_slice(),session.as_slice(),u.account.id,u.tenant.0,u.epoch,who.installation.to_string(),now()?.checked_add(86400).ok_or(StoreError::Unavailable)?])?;
                    AuthorityReply{owner:who,grant:Some(grant),operation:None,digest:None}
                }
                AuthorityRequest::Validate{grant} => {
                    let token=Zeroizing::new(grant);let who=grant_owner(&tx,&token)?;
                    AuthorityReply{owner:who,grant:None,operation:None,digest:None}
                }
                AuthorityRequest::Commit{grant,operation,digest:payload} => {
                    let token=Zeroizing::new(grant);let who=grant_owner(&tx,&token)?;
                    if operation.is_nil() || payload.len()!=64 || !payload.bytes().all(|c| c.is_ascii_hexdigit()) { return Err(StoreError::InvalidInput); }
                    let previous:Option<(String,String,i64,String)>=tx.query_row("SELECT payload_digest,account_id,auth_epoch,installation_id FROM vault_operation_commitments WHERE tenant_id=?1 AND operation_id=?2",params![who.tenant.to_string(),operation.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
                    if let Some((p,a,e,i))=previous {
                        if p!=payload || a!=who.account.to_string() || e!=who.epoch || i!=who.installation.to_string() { return Err(StoreError::Conflict); }
                    } else {
                        let count:i64=tx.query_row("SELECT count(*) FROM vault_operation_commitments WHERE account_id=?1",[who.account.to_string()],|r|r.get(0))?;
                        if count>=100000 { return Err(StoreError::Busy); }
                        tx.execute("INSERT INTO vault_operation_commitments VALUES(?1,?2,?3,?4,?5,?6,?7)",params![who.tenant.to_string(),who.account.to_string(),who.installation.to_string(),operation.to_string(),payload,who.epoch,now()?])?;
                    }
                    // This transaction is the authority linearization point. Later
                    // revocation cannot recall already admitted ciphertext settlement.
                    AuthorityReply{owner:who,grant:None,operation:Some(operation),digest:Some(payload)}
                }
                AuthorityRequest::Close{grant} => {
                    let token=Zeroizing::new(grant);let who=grant_owner(&tx,&token)?;
                    tx.execute("UPDATE vault_grants SET closed=1 WHERE digest=?1",[token_digest(&token)?.as_slice()])?;
                    AuthorityReply{owner:who,grant:None,operation:None,digest:None}
                }
            };
            tx.commit()?;Ok(result)
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory, member};
    async fn grant(s: &Store, u: &UserContext) -> String {
        let h = s
            .vault_handoff(u, "https://vault.example.test".into())
            .await
            .unwrap();
        s.vault_authority(AuthorityRequest::Exchange {
            handoff: h.token,
            vault_origin: h.vault_origin,
        })
        .await
        .unwrap()
        .grant
        .unwrap()
    }
    #[tokio::test]
    async fn handoff_is_one_time_origin_bound_and_each_grant_tracks_its_real_session() {
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await.user;
        let b = member(&s, &a, "vault-b@example.test").await.user;
        let h = s
            .vault_handoff(&b, "https://vault.example.test".into())
            .await
            .unwrap();
        assert!(
            s.vault_authority(AuthorityRequest::Exchange {
                handoff: h.token.clone(),
                vault_origin: "https://wrong.example.test".into()
            })
            .await
            .is_err()
        );
        let reply = s
            .vault_authority(AuthorityRequest::Exchange {
                handoff: h.token.clone(),
                vault_origin: h.vault_origin.clone(),
            })
            .await
            .unwrap();
        assert_eq!(reply.owner.account.to_string(), b.account.id);
        assert!(
            s.vault_authority(AuthorityRequest::Exchange {
                handoff: h.token,
                vault_origin: h.vault_origin
            })
            .await
            .is_err()
        );
        let token = reply.grant.unwrap();
        s.logout(&b).await.unwrap();
        assert!(
            s.vault_authority(AuthorityRequest::Validate { grant: token })
                .await
                .is_err()
        );
        let g = grant(&s, &a).await;
        assert!(
            s.vault_authority(AuthorityRequest::Validate { grant: g })
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn commitment_retries_exact_identity_and_revocation_fences_uncommitted_requests() {
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let u = admin(&s).await.user;
        let g = grant(&s, &u).await;
        let id = Uuid::new_v4();
        let p = "a".repeat(64);
        for _ in 0..2 {
            let r = s
                .vault_authority(AuthorityRequest::Commit {
                    grant: g.clone(),
                    operation: id,
                    digest: p.clone(),
                })
                .await
                .unwrap();
            assert_eq!(r.operation, Some(id));
            assert_eq!(r.digest.as_deref(), Some(p.as_str()));
        }
        assert!(
            s.vault_authority(AuthorityRequest::Commit {
                grant: g.clone(),
                operation: id,
                digest: "b".repeat(64)
            })
            .await
            .is_err()
        );
        s.change_password(&u, "Synthetic-vault-new-password-2026".into())
            .await
            .unwrap();
        assert!(
            s.vault_authority(AuthorityRequest::Commit {
                grant: g,
                operation: Uuid::new_v4(),
                digest: p
            })
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn suspension_and_restore_installation_change_revoke_sidecar_grants() {
        use moosuri_core::{AccountManagementAction, AccountManagementInput};
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await.user;
        let b = member(&s, &a, "vault-suspended@example.test").await.user;
        let bg = grant(&s, &b).await;
        let ag = grant(&s, &a).await;
        let target = s
            .managed_accounts(&a)
            .await
            .unwrap()
            .into_iter()
            .find(|m| m.user.id == b.account.id)
            .unwrap();
        let p = s
            .prepare_account_management(
                &a,
                b.account.id.clone(),
                AccountManagementInput {
                    request_id: Uuid::new_v4().to_string(),
                    expected_revision: target.revision,
                    action: AccountManagementAction::Suspend,
                },
            )
            .await
            .unwrap();
        s.apply_account_management(p).await.unwrap();
        assert!(
            s.vault_authority(AuthorityRequest::Validate { grant: bg })
                .await
                .is_err()
        );
        // Simulate the restore authority identity change without touching real data.
        s.call(|db| {
            db.conn.execute(
                "UPDATE backup_installation SET installation_id=?1",
                [Uuid::new_v4().to_string()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(
            s.vault_authority(AuthorityRequest::Validate { grant: ag })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn schema24_late_failure_rolls_back_and_retries_without_replacing_push_schema() {
        let _guard = TEST_LOCK.lock().await;
        let dir = directory();
        drop(Store::open(dir.path()).unwrap());
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        conn.execute_batch("DROP INDEX local_notifications_source;DROP TRIGGER notification_suppression_monotonic;DROP TABLE notification_source_preferences;ALTER TABLE local_notifications DROP COLUMN source_suppressed;ALTER TABLE local_notifications DROP COLUMN push_suppressed;DROP TABLE learned_skill_receipts;DROP TABLE learned_skill_observations;DROP TABLE learned_skill_uses;DROP TABLE learned_skill_proofs;DROP TABLE learned_skills;DROP TABLE credential_broker_auth;DROP TABLE credential_broker_bindings;DROP TABLE credential_broker_devices;DROP TABLE credential_broker_pairs;DROP TABLE credential_broker_invocations;DROP TABLE credential_broker_commitments;DROP TABLE community_catalog_cache;DROP TRIGGER chatgpt_handoff_session_logout;DROP TRIGGER chatgpt_handoff_account_epoch;DROP TRIGGER chatgpt_handoff_binding_change;DROP TRIGGER session_purpose_valid_insert;DROP TRIGGER session_purpose_immutable;ALTER TABLE sessions DROP COLUMN absolute_deadline;ALTER TABLE sessions DROP COLUMN purpose;DROP TABLE chatgpt_handoffs;DROP TRIGGER audio_authority_no_delete;DROP TRIGGER audio_authority_monotonic;DROP TABLE audio_jobs;DROP TABLE audio_selection;DROP TABLE audio_authority;DROP TRIGGER personal_subscription_exclusive;DROP TRIGGER personal_shared_exclusive;DROP TRIGGER personal_shared_exclusive_update;DROP TABLE personal_subscription_selections;DROP TABLE chatgpt_model_probes;DROP TABLE chatgpt_auth_attempts;DROP TABLE chatgpt_sessions;DROP TABLE chatgpt_binding_revisions;DROP TABLE chatgpt_bindings;DROP TABLE subscription_runtime;DROP TABLE mail_reconciliations;DROP TABLE mail_send_evidence;DROP TABLE calendar_agent_action_origins;DROP TABLE calendar_agent_action_links;DROP TABLE calendar_command_plans;DROP TABLE calendar_agent_observations;DROP TABLE calendar_run_dependencies;DROP TABLE calendar_agent_profiles;DROP TABLE mission_chat_commands;DROP TABLE mission_chat_plans;DROP TABLE recurring_mission_occurrences;DROP TABLE recurring_missions;ALTER TABLE media_objects DROP COLUMN filename;ALTER TABLE media_objects DROP COLUMN media_type;ALTER TABLE media_objects DROP COLUMN extracted_text;DROP TABLE calendar_actions;DROP TABLE calendar_oauth_sessions;DROP TABLE calendar_connections;DROP TABLE mission_controls;DROP TABLE mission_revisions;DROP TABLE missions;ALTER TABLE threads DROP COLUMN mission_internal;DROP TABLE vault_handoffs; DROP TABLE vault_grants; DROP TABLE vault_operation_commitments; CREATE TABLE vault_operation_commitments(conflict INTEGER); PRAGMA user_version=23;").unwrap();
        drop(conn);
        assert!(Store::open(dir.path()).is_err());
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            23
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'vault_%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'push_%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            6
        );
        conn.execute_batch("DROP TABLE vault_operation_commitments;")
            .unwrap();
        drop(conn);
        drop(Store::open(dir.path()).unwrap());
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            crate::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'vault_%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            3
        );
    }
}
