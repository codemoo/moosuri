//! Temporary account/session-owned dictation. No audio bodies are persisted.
use super::*;
use moosuri_core::{AudioJob, AudioSelection, AudioSettings, ProviderKind};
const MODEL: &str = "gpt-4o-mini-transcribe";
pub struct AudioLease {
    user: UserContext,
    pub id: String,
    nonce: String,
    generation: i64,
    pub selection: AudioSelection,
}
pub enum AudioAdmission {
    Existing(AudioJob),
    Dispatch(Box<AudioLease>, Box<ProviderBinding>),
}
pub fn restart(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE audio_jobs SET transcript=NULL,state=CASE WHEN state='processing' THEN 'outcome_unknown' WHEN state='ready' THEN 'expired' ELSE state END,error=CASE WHEN state='processing' THEN 'audio_outcome_unknown' ELSE error END",[])?;
    Ok(())
}
pub(crate) fn quarantine_restore(conn: &Connection) -> Result<()> {
    advance_authority(conn, None)?;
    restart(conn)?;
    conn.execute("DELETE FROM audio_selection", [])?;
    Ok(())
}
fn authority(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT generation FROM audio_authority WHERE singleton=1",
        [],
        |r| r.get(0),
    )?)
}
/// Caller owns the writer transaction (or the encompassing restore transaction).
fn advance_authority(conn: &Connection, expected: Option<i64>) -> Result<i64> {
    let old = authority(conn)?;
    if expected.is_some_and(|v| v != old) {
        return Err(StoreError::Conflict);
    }
    let next = old.checked_add(1).ok_or(StoreError::Corrupt)?;
    conn.execute(
        "UPDATE audio_authority SET generation=?1 WHERE singleton=1 AND generation=?2",
        params![next, old],
    )?;
    conn.execute("UPDATE audio_jobs SET state='cancelled',transcript=NULL,error=NULL WHERE state IN('processing','ready')",[])?;
    Ok(next)
}
fn cleanup(conn: &Connection, time: i64) -> Result<()> {
    conn.execute("UPDATE audio_jobs SET transcript=NULL,state='cancelled',error=NULL WHERE state IN('processing','ready') AND authority_generation!=(SELECT generation FROM audio_authority WHERE singleton=1)",[])?;
    conn.execute("UPDATE audio_jobs SET transcript=NULL,state=CASE WHEN state='processing' THEN 'outcome_unknown' ELSE 'expired' END,error=CASE WHEN state='processing' THEN 'audio_outcome_unknown' ELSE error END WHERE state IN('processing','ready') AND (expires_at<=?1 OR NOT EXISTS(SELECT 1 FROM sessions s JOIN accounts a ON a.id=s.account_id WHERE s.digest=audio_jobs.session_digest AND s.account_id=audio_jobs.account_id AND a.auth_epoch=audio_jobs.auth_epoch AND s.auth_epoch=audio_jobs.auth_epoch AND s.expires_at>?1 AND a.status='active' AND a.recovery_required=0) OR NOT EXISTS(SELECT 1 FROM audio_selection sel JOIN providers p ON p.id=sel.provider_id JOIN provider_revisions r ON r.provider_id=p.id AND r.revision=p.current_revision WHERE sel.provider_id=audio_jobs.provider_id AND sel.provider_revision=audio_jobs.provider_revision AND sel.model=audio_jobs.model AND p.current_revision=audio_jobs.provider_revision AND r.enabled=1 AND r.credential IS NOT NULL))",[time])?;
    // Compact nonsecret receipts stay as bounded permanent tombstones. Removing an
    // old UUID would allow replay to create another provider bill.
    Ok(())
}
fn load(conn: &Connection) -> Result<Option<AudioSelection>> {
    Ok(conn
        .query_row(
            "SELECT provider_id,provider_revision,model FROM audio_selection WHERE singleton=1",
            [],
            |r| {
                Ok(AudioSelection {
                    provider_id: r.get(0)?,
                    provider_revision: r.get(1)?,
                    model: r.get(2)?,
                })
            },
        )
        .optional()?)
}
fn bound(conn: &Connection, key: &[u8; 32], selection: &AudioSelection) -> Result<ProviderBinding> {
    if selection.model != MODEL {
        return Err(StoreError::InvalidInput);
    }
    let b = providers::binding(
        key,
        providers::require_binding(conn, &selection.provider_id, selection.provider_revision)?,
        selection.model.clone(),
        None,
    )?;
    if b.config.kind != ProviderKind::OpenAi || b.credential.is_none() {
        return Err(StoreError::InvalidInput);
    }
    Ok(b)
}
fn view(conn: &Connection, user: &UserContext, id: &str) -> Result<AudioJob> {
    conn.query_row("SELECT id,state,transcript,error,expires_at FROM audio_jobs WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND session_digest=?4 AND auth_epoch=?5",params![id,user.tenant.0,user.account.id,user.digest.as_slice(),user.epoch],|r|Ok(AudioJob {
        id:r.get(0)?,state:r.get(1)?,transcript:r.get(2)?,error:r.get(3)?,expires_at:r.get(4)?
    }
    )).optional()?.ok_or(StoreError::NotFound)
}
impl Store {
    pub async fn audio_settings(&self, user: &UserContext) -> Result<AudioSettings> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            cleanup(&db.conn, now()?)?;
            let selection = load(&db.conn)?;
            let available = selection
                .as_ref()
                .is_some_and(|s| bound(&db.conn, &db.provider_key, s).is_ok());
            Ok(AudioSettings {
                selection,
                available,
                max_seconds: 120,
            })
        })
        .await
    }
    pub async fn audio_probe_admit(
        &self,
        user: &UserContext,
        selection: AudioSelection,
    ) -> Result<(ProviderBinding, i64)> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, true)?;
            Ok((
                bound(&db.conn, &db.provider_key, &selection)?,
                authority(&db.conn)?,
            ))
        })
        .await
    }
    pub async fn audio_probe_current(
        &self,
        user: &UserContext,
        selection: AudioSelection,
        expected: i64,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, true)?;
            if authority(&db.conn)? != expected {
                return Err(StoreError::Conflict);
            }
            bound(&db.conn, &db.provider_key, &selection)?;
            Ok(())
        })
        .await
    }
    /// A successful probe commits only against the authority present at admission.
    /// Disable has no provider prerequisite and always permanently advances authority.
    pub async fn audio_select_verified(
        &self,
        user: &UserContext,
        selection: Option<AudioSelection>,
        expected: Option<i64>,
    ) -> Result<AudioSettings> {
        let user = user.clone();
        self.call(move|db|{
            validate(&db.conn,&user,true,true)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(s)=&selection {
                if expected.is_none(){return Err(StoreError::InvalidInput)}
                bound(&tx,&db.provider_key,s)?;
            }
            advance_authority(&tx,expected)?;
            if let Some(s)=&selection {
                tx.execute("INSERT INTO audio_selection VALUES(1,?1,?2,?3) ON CONFLICT(singleton) DO UPDATE SET provider_id=excluded.provider_id,provider_revision=excluded.provider_revision,model=excluded.model",params![s.provider_id,s.provider_revision,s.model])?;
            } else {tx.execute("DELETE FROM audio_selection",[])?;}
            tx.commit()?;
            Ok(AudioSettings{available:selection.is_some(),selection,max_seconds:120})
        }).await
    }
    pub async fn audio_begin(
        &self,
        user: &UserContext,
        id: String,
        digest: [u8; 32],
        duration_ms: u32,
    ) -> Result<AudioAdmission> {
        let user = user.clone();
        self.call(move |db|  {
            let user=validate(&db.conn,&user,true,false)?;
            let time=now()?;
            let id=Uuid::parse_str(&id).map_err(|_|StoreError::InvalidInput)?.to_string();
            if duration_ms==0||duration_ms>120000 {
                return Err(StoreError::InvalidInput)
            }
            let duration_ms=duration_ms.max(1000);
            // Minimum one-second reservation also bounds tombstone count.
            cleanup(&db.conn,time)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(old)=tx.query_row("SELECT input_digest FROM audio_jobs WHERE id=?1",[&id],|r|r.get::<_,Vec<u8>>(0)).optional()?  {
                if old!=digest {
                    return Err(StoreError::Conflict)
                }
                return Ok(AudioAdmission::Existing(view(&tx,&user,&id)?))
            }
            let count:i64=tx.query_row("SELECT count(*) FROM audio_jobs WHERE account_id=?1",[&user.account.id],|r|r.get(0))?;
            if count>=4096 { return Err(StoreError::Busy) }
            let selection=load(&tx)?.ok_or(StoreError::Unavailable)?;
            let binding=bound(&tx,&db.provider_key,&selection)?;
            let active:i64=tx.query_row("SELECT count(*) FROM audio_jobs WHERE state='processing'",[],|r|r.get(0))?;
            let own:i64=tx.query_row("SELECT count(*) FROM audio_jobs WHERE account_id=?1 AND state='processing'",[&user.account.id],|r|r.get(0))?;
            let used:i64=tx.query_row("SELECT coalesce(sum(duration_ms),0) FROM audio_jobs WHERE account_id=?1 AND created_at>=?2 AND dispatched=1",params![user.account.id,time-time.rem_euclid(86400)],|r|r.get(0))?;
            let global:i64=tx.query_row("SELECT coalesce(sum(duration_ms),0) FROM audio_jobs WHERE created_at>=?1 AND dispatched=1",[time-time.rem_euclid(86400)],|r|r.get(0))?;
            if active>=2||own>=1 {
                return Err(StoreError::Busy)
            }
            if used+i64::from(duration_ms)>600000||global+i64::from(duration_ms)>3600000 {
                return Err(StoreError::BudgetExhausted)
            }
            let nonce=Uuid::new_v4().to_string();
            let generation=authority(&tx)?;
            // Durable commitment is conservative: process loss after this point is unknown.
            tx.execute("INSERT INTO audio_jobs(id,tenant_id,account_id,session_digest,auth_epoch,input_digest,provider_id,provider_revision,model,nonce,state,created_at,expires_at,duration_ms,dispatched,authority_generation) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'processing',?11,?12,?13,1,?14)",params![id,user.tenant.0,user.account.id,user.digest.as_slice(),user.epoch,digest.as_slice(),selection.provider_id,selection.provider_revision,selection.model,nonce,time,time+120,duration_ms,generation])?;
            tx.commit()?;
            Ok(AudioAdmission::Dispatch(Box::new(AudioLease {
                user,id,nonce,selection,generation
            })
            ,Box::new(binding)))
        }
        ).await
    }
    pub async fn audio_alive(&self, lease: &AudioLease) -> Result<()> {
        let user = lease.user.clone();
        let id = lease.id.clone();
        let nonce = lease.nonce.clone();
        let selection = lease.selection.clone();
        let generation = lease.generation;
        self.call(move |db|  {
            validate(&db.conn,&user,true,false)?;
            cleanup(&db.conn,now()?)?;
            bound(&db.conn,&db.provider_key,&selection)?;
            if authority(&db.conn)?!=generation || load(&db.conn)?.as_ref()!=Some(&selection) {
                return Err(StoreError::Conflict)
            }
            let live:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM audio_jobs WHERE id=?1 AND nonce=?2 AND state='processing' AND expires_at>?3)",params![id,nonce,now()?],|r|r.get(0))?;
            if live {
                Ok(())
            }
            else {
                Err(StoreError::Conflict)
            }
        }
        ).await
    }
    pub async fn audio_finish(
        &self,
        lease: AudioLease,
        text: Option<String>,
        error: Option<String>,
    ) -> Result<()> {
        let user = lease.user;
        self.call(move |db|  {
            let time=now()?;
            cleanup(&db.conn,time)?;
            let valid=authority(&db.conn)?==lease.generation && validate(&db.conn,&user,true,false).is_ok()&&bound(&db.conn,&db.provider_key,&lease.selection).is_ok()&&load(&db.conn)?.as_ref()==Some(&lease.selection);
            let safe=text.filter(|s|!s.trim().is_empty()&&s.len()<=24576&&!s.chars().any(|c|c.is_control()&&!matches!(c,'\n'|'\r'|'\t')));
            let state=if !valid  {
                "cancelled"
            }
            else if safe.is_some() {
                "ready"
            }
            else {
                "outcome_unknown"
            }
            ;
            let safe=if valid {
                safe
            }
            else {
                None
            }
            ;
            let error=error.filter(|v|matches!(v.as_str(),"audio_credentials_rejected"|"audio_rate_limited"|"audio_response_invalid"|"audio_outcome_unknown"|"audio_unavailable"));
            db.conn.execute("UPDATE audio_jobs SET state=?3,transcript=?4,error=?5 WHERE id=?1 AND nonce=?2 AND state='processing' AND expires_at>?6",params![lease.id,lease.nonce,state,safe,error,time])?;
            Ok(())
        }
        ).await
    }
    pub async fn audio_job(&self, user: &UserContext, id: String) -> Result<AudioJob> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            cleanup(&db.conn, now()?)?;
            let mut value = view(&db.conn, &user, &id)?;
            if value.state == "ready" {
                let old: AudioSelection = db.conn.query_row(
                    "SELECT provider_id,provider_revision,model FROM audio_jobs WHERE id=?1",
                    [&id],
                    |r| {
                        Ok(AudioSelection {
                            provider_id: r.get(0)?,
                            provider_revision: r.get(1)?,
                            model: r.get(2)?,
                        })
                    },
                )?;
                if bound(&db.conn, &db.provider_key, &old).is_err()
                    || load(&db.conn)?.as_ref() != Some(&old)
                {
                    db.conn.execute(
                        "UPDATE audio_jobs SET transcript=NULL,state='cancelled' WHERE id=?1",
                        [&id],
                    )?;
                    value.state = "cancelled".into();
                    value.transcript = None;
                }
            }
            Ok(value)
        })
        .await
    }
    pub async fn audio_delete(&self, user: &UserContext, id: String) -> Result<()> {
        let user = user.clone();
        self.call(move |db|  {
            validate(&db.conn,&user,true,false)?;
            let id=Uuid::parse_str(&id).map_err(|_|StoreError::InvalidInput)?.to_string();
            let time=now()?;
            cleanup(&db.conn,time)?;
            let exists:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM audio_jobs WHERE id=?1)",[&id],|r|r.get(0))?;
            if exists {
                view(&db.conn,&user,&id)?;
                db.conn.execute("UPDATE audio_jobs SET transcript=NULL,state='cancelled' WHERE id=?1",[id])?;
            }
            else {
                let count:i64=db.conn.query_row("SELECT count(*) FROM audio_jobs WHERE account_id=?1",[&user.account.id],|r|r.get(0))?;
                if count>=4096 {
                    return Err(StoreError::Busy)
                }
                // Cancellation can win before a delayed upload reaches durable admission.
                db.conn.execute("INSERT INTO audio_jobs(id,tenant_id,account_id,session_digest,auth_epoch,input_digest,provider_id,provider_revision,model,nonce,state,created_at,expires_at,duration_ms,dispatched,authority_generation) VALUES(?1,?2,?3,?4,?5,zeroblob(32),'',0,'','','cancelled',?6,?6,0,0,?7)",params![id,user.tenant.0,user.account.id,user.digest.as_slice(),user.epoch,time,authority(&db.conn)?])?;
            }
            Ok(())
        }
        ).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory, member};
    use moosuri_core::{ProviderConfig, ProviderKind};
    async fn configured(store: &Store, user: &UserContext) -> AudioSelection {
        let p = store
            .create_provider(
                user,
                ProviderChange {
                    name: "synthetic audio".into(),
                    config: ProviderConfig {
                        kind: ProviderKind::OpenAi,
                        base_url: None,
                        allowed_ips: vec![],
                        router_providers: vec![],
                        zero_data_retention: false,
                    },
                    enabled: true,
                    credential: CredentialChange::Replace(Zeroizing::new("synthetic-key".into())),
                },
            )
            .await
            .unwrap();
        let s = AudioSelection {
            provider_id: p.id,
            provider_revision: p.revision,
            model: MODEL.into(),
        };
        store
            .audio_select_verified(
                user,
                Some(s.clone()),
                Some(store.audio_probe_admit(user, s.clone()).await.unwrap().1),
            )
            .await
            .unwrap();
        s
    }
    async fn begin(store: &Store, user: &UserContext, id: &str) -> AudioLease {
        match store
            .audio_begin(user, id.into(), [4; 32], 1000)
            .await
            .unwrap()
        {
            AudioAdmission::Dispatch(l, _) => *l,
            _ => panic!("new dispatch"),
        }
    }
    #[tokio::test]
    async fn dedup_original_session_quota_and_text_independence() {
        let _lock = TEST_LOCK.lock().await;
        let d = directory();
        let store = Store::open(d.path()).unwrap();
        let a = admin(&store).await.user;
        configured(&store, &a).await;
        let b = member(&store, &a, "audio@example.test").await.user;
        let id = Uuid::new_v4().to_string();
        let lease = begin(&store, &a, &id).await;
        assert!(matches!(
            store
                .audio_begin(&a, id.clone(), [4; 32], 1000)
                .await
                .unwrap(),
            AudioAdmission::Existing(_)
        ));
        assert!(matches!(
            store.audio_begin(&a, id.clone(), [5; 32], 1000).await,
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            store.audio_job(&b, id.clone()).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            store
                .audio_begin(&a, Uuid::new_v4().to_string(), [6; 32], 1000)
                .await,
            Err(StoreError::Busy)
        ));
        // Audio does not consume the chat queue or stop unrelated normal thread creation.
        store.create_thread(&a, "plain text".into()).await.unwrap();
        store
            .audio_finish(lease, Some("synthetic transcript".into()), None)
            .await
            .unwrap();
        assert_eq!(
            store
                .audio_job(&a, id.clone())
                .await
                .unwrap()
                .transcript
                .as_deref(),
            Some("synthetic transcript")
        );
        let replacement = store
            .login("admin@example.test".into(), crate::tests::PASSWORD.into())
            .await
            .unwrap();
        assert!(matches!(
            store.audio_job(&replacement.user, id.clone()).await,
            Err(StoreError::NotFound)
        ));
        store.audio_delete(&a, id.clone()).await.unwrap();
        assert!(store.audio_job(&a, id).await.unwrap().transcript.is_none());
    }
    #[tokio::test]
    async fn delete_expiry_provider_and_session_fence_late_completion() {
        let _lock = TEST_LOCK.lock().await;
        let d = directory();
        let store = Store::open(d.path()).unwrap();
        let login = admin(&store).await;
        let a = login.user.clone();
        configured(&store, &a).await;
        for mode in 0..4 {
            let id = Uuid::new_v4().to_string();
            let lease = begin(&store, &a, &id).await;
            match mode {
                0 => store.audio_delete(&a, id.clone()).await.unwrap(),
                1 => {
                    let id = id.clone();
                    store
                        .call(move |db| {
                            db.conn
                                .execute("UPDATE audio_jobs SET expires_at=0 WHERE id=?1", [id])?;
                            Ok(())
                        })
                        .await
                        .unwrap();
                }
                2 => {
                    store
                        .update_provider(
                            &a,
                            lease.selection.provider_id.clone(),
                            lease.selection.provider_revision,
                            ProviderChange {
                                name: "rotated synthetic".into(),
                                config: ProviderConfig {
                                    kind: ProviderKind::OpenAi,
                                    base_url: None,
                                    allowed_ips: vec![],
                                    router_providers: vec![],
                                    zero_data_retention: false,
                                },
                                enabled: true,
                                credential: CredentialChange::Keep,
                            },
                        )
                        .await
                        .unwrap();
                }
                _ => {
                    store.logout(&a).await.unwrap();
                }
            }
            assert!(store.audio_alive(&lease).await.is_err());
            store
                .audio_finish(lease, Some("LATE-CANARY".into()), None)
                .await
                .unwrap();
            let check = id.clone();
            let text: Option<String> = store
                .call(move |db| {
                    Ok(db.conn.query_row(
                        "SELECT transcript FROM audio_jobs WHERE id=?1",
                        [check],
                        |r| r.get(0),
                    )?)
                })
                .await
                .unwrap();
            assert!(text.is_none());
            if mode == 2 {
                configured(&store, &a).await;
            }
        }
    }
    #[tokio::test]
    async fn cancellation_before_upload_admission_is_durable() {
        let _lock = TEST_LOCK.lock().await;
        let d = directory();
        let store = Store::open(d.path()).unwrap();
        let a = admin(&store).await.user;
        configured(&store, &a).await;
        let id = Uuid::new_v4().to_string();
        store.audio_delete(&a, id.clone()).await.unwrap();
        assert!(matches!(
            store.audio_begin(&a, id.clone(), [7; 32], 1000).await,
            Err(StoreError::Conflict)
        ));
        assert_eq!(store.audio_job(&a, id).await.unwrap().state, "cancelled");
    }
    #[tokio::test]
    async fn processing_restart_is_unknown_never_new_dispatch() {
        let _lock = TEST_LOCK.lock().await;
        let d = directory();
        let store = Store::open(d.path()).unwrap();
        let login = admin(&store).await;
        configured(&store, &login.user).await;
        let id = Uuid::new_v4().to_string();
        let _lease = begin(&store, &login.user, &id).await;
        let digest = login.user.digest;
        drop(store);
        let store = Store::open(d.path()).unwrap();
        let user = store
            .call(move |db| load_context(&db.conn, digest, false))
            .await
            .unwrap();
        let receipt = store.audio_job(&user, id.clone()).await.unwrap();
        assert_eq!(receipt.state, "outcome_unknown");
        assert!(matches!(
            store.audio_begin(&user, id, [4; 32], 1000).await.unwrap(),
            AudioAdmission::Existing(_)
        ));
    }
    #[tokio::test]
    async fn restore_quarantine_erases_ready_text_and_selection_but_keeps_receipt() {
        let _lock = TEST_LOCK.lock().await;
        let d = directory();
        let store = Store::open(d.path()).unwrap();
        let a = admin(&store).await.user;
        configured(&store, &a).await;
        let id = Uuid::new_v4().to_string();
        let lease = begin(&store, &a, &id).await;
        store
            .audio_finish(lease, Some("TEMPORARY-TRANSCRIPT-CANARY".into()), None)
            .await
            .unwrap();
        assert!(
            store
                .audio_job(&a, id.clone())
                .await
                .unwrap()
                .transcript
                .is_some()
        );
        store.call(|db| quarantine_restore(&db.conn)).await.unwrap();
        let value = store.audio_job(&a, id.clone()).await.unwrap();
        assert_eq!(value.state, "cancelled");
        assert!(value.transcript.is_none());
        assert!(!store.audio_settings(&a).await.unwrap().available);
        assert!(matches!(
            store.audio_begin(&a, id, [4; 32], 1000).await.unwrap(),
            AudioAdmission::Existing(_)
        ));
    }
    #[tokio::test]
    async fn authority_survives_disable_restart_and_blocks_stale_probe_cas() {
        let _lock = TEST_LOCK.lock().await;
        let d = directory();
        let store = Store::open(d.path()).unwrap();
        let login = admin(&store).await;
        let user = login.user.clone();
        let selection = configured(&store, &user).await;
        let before = store
            .audio_probe_admit(&user, selection.clone())
            .await
            .unwrap()
            .1;
        store
            .audio_select_verified(&user, None, None)
            .await
            .unwrap();
        assert!(matches!(
            store
                .audio_select_verified(&user, Some(selection.clone()), Some(before))
                .await,
            Err(StoreError::Conflict)
        ));
        let after = store
            .audio_probe_admit(&user, selection.clone())
            .await
            .unwrap()
            .1;
        assert!(after > before);
        let digest = user.digest;
        drop(store);
        let store = Store::open(d.path()).unwrap();
        let user = store
            .call(move |db| load_context(&db.conn, digest, false))
            .await
            .unwrap();
        let reopened = store
            .audio_probe_admit(&user, selection.clone())
            .await
            .unwrap()
            .1;
        assert_eq!(reopened, after);
        assert!(!store.audio_settings(&user).await.unwrap().available);
        let delete = store
            .call(|db| {
                db.conn.execute("DELETE FROM audio_authority", [])?;
                Ok(())
            })
            .await;
        assert!(delete.is_err());
        let rollback = store
            .call(|db| {
                db.conn
                    .execute("UPDATE audio_authority SET generation=1", [])?;
                Ok(())
            })
            .await;
        assert!(rollback.is_err());
        store
            .audio_select_verified(&user, Some(selection), Some(reopened))
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn daily_reservations_remain_for_unknown_outcomes() {
        let _lock = TEST_LOCK.lock().await;
        let d = directory();
        let store = Store::open(d.path()).unwrap();
        let a = admin(&store).await.user;
        configured(&store, &a).await;
        for i in 0..5 {
            let id = Uuid::new_v4().to_string();
            let AudioAdmission::Dispatch(lease, _) =
                store.audio_begin(&a, id, [i; 32], 120000).await.unwrap()
            else {
                panic!("new")
            };
            store
                .audio_finish(*lease, None, Some("audio_outcome_unknown".into()))
                .await
                .unwrap();
        }
        assert!(matches!(
            store
                .audio_begin(&a, Uuid::new_v4().to_string(), [9; 32], 1)
                .await,
            Err(StoreError::BudgetExhausted)
        ));
    }
}
