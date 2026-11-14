//! Typed GitHub metadata requests. No generic URL or model-selected item/secret.
use super::*;
#[derive(serde::Serialize)]
pub struct GitHubJob {
    pub request_id: String,
    pub repository: GitHubRepository,
    pub status: String,
    pub projection: Option<Projection>,
}
fn job(conn: &Connection, tenant: &str, id: &str) -> Result<GitHubJob> {
    let (repo,run,expires,request):(String,String,i64,Option<String>)=conn.query_row("SELECT repository,run_id,expires_at,request FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::NotFound)?;
    let row:Option<(String,Option<String>)>=conn.query_row("SELECT commitment,receipt FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let mut projection = None;
    let cancelled:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM credential_broker_invocations WHERE tenant_id=?1 AND run_id=?2 AND cancelled=1)",params![tenant,run],|r|r.get(0))?;
    let current:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM credential_broker_jobs j JOIN accounts a ON a.tenant_id=j.tenant_id AND a.id=j.account_id WHERE j.tenant_id=?1 AND j.request_id=?2 AND j.auth_epoch=a.auth_epoch AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0)",params![tenant,id],|r|r.get(0))?;
    let status = if let Some((_, receipt)) = row {
        if let Some(receipt) = receipt {
            let r: SignedReceipt = decode(&receipt)?;
            match r.receipt.outcome {
                Outcome::ProjectedSuccess => {
                    projection = Some(r.receipt.projection);
                    "complete"
                }
                Outcome::NotDispatched => "not_dispatched",
                Outcome::Rejected => "credential_rejected",
                Outcome::Unknown => "outcome_unknown",
            }
        } else {
            "in_flight_or_unknown"
        }
    } else if !current {
        "authority_changed"
    } else if cancelled {
        "not_dispatched"
    } else if expires <= now()? {
        "expired"
    } else if request.is_some() {
        "waiting_for_unlock"
    } else {
        "waiting_for_credential"
    };
    Ok(GitHubJob {
        request_id: id.into(),
        repository: decode(&repo)?,
        status: status.into(),
        projection,
    })
}
impl Store {
    pub async fn request_github_metadata(
        &self,
        user: &UserContext,
        id: String,
        repository: GitHubRepository,
    ) -> Result<GitHubJob> {
        if Uuid::parse_str(&id).is_err() || !repository.valid() {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;
            let old:Option<String>=db.conn.query_row("SELECT repository FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![u.tenant.0,id],|r|r.get(0)).optional()?;
            let repo=encode(&repository)?;
            if let Some(old)=old {if old!=repo{return Err(StoreError::Conflict)}}else{
                let n:i64=db.conn.query_row("SELECT count(*) FROM credential_broker_jobs WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?;if n>=1024{return Err(StoreError::Busy)}
                db.conn.execute("INSERT INTO credential_broker_jobs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",params![u.tenant.0,u.account.id,id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),repo,u.epoch,now()?+3600])?;
            }
            job(&db.conn,&u.tenant.0,&id)
        }).await
    }
    pub async fn github_metadata_jobs(&self, user: &UserContext) -> Result<Vec<GitHubJob>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT request_id FROM credential_broker_jobs WHERE tenant_id=?1 ORDER BY rowid DESC LIMIT 30")?.query_map([&u.tenant.0],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ids.iter().map(|i|job(&db.conn,&u.tenant.0,i)).collect()
        }).await
    }
    pub async fn broker_devices(&self, user: &UserContext) -> Result<Vec<(String, String)>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;
            Ok(db.conn.prepare("SELECT d.id,CASE WHEN d.status!='active' THEN d.status WHEN EXISTS(SELECT 1 FROM sessions s JOIN accounts a ON a.id=s.account_id AND a.tenant_id=s.tenant_id WHERE s.digest=d.session_digest AND s.expires_at>unixepoch() AND s.auth_epoch=a.auth_epoch AND a.status='active' AND a.recovery_required=0) THEN 'active' ELSE 'enrollment_expired' END FROM credential_broker_devices d WHERE d.account_id=?1 AND d.tenant_id=?2")?.query_map(params![u.account.id,u.tenant.0],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        }).await
    }
}
pub(super) fn poll(
    conn: &Connection,
    p: &PairChallenge,
    session: &[u8],
    status: &str,
) -> Result<Reply> {
    if status != "active" {
        return Ok(Reply::Waiting {
            requests: vec![],
            revocation_pending: true,
        });
    }
    let u = active(conn, p, session)?;
    let rows=conn.prepare("SELECT request_id,repository,run_id,call_id,invocation_nonce,expires_at FROM credential_broker_jobs WHERE tenant_id=?1 AND account_id=?2 AND auth_epoch=?3 AND expires_at>?4 AND request IS NULL LIMIT 32")?.query_map(params![u.tenant.0,u.account.id,u.epoch,now()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, repo, run, call, nonce, expiry) in rows {
        let bindings=conn.prepare("SELECT r.device_id,b.binding FROM credential_broker_repositories r JOIN credential_broker_bindings b USING(device_id,binding_id) JOIN credential_broker_devices d ON d.id=r.device_id JOIN sessions live ON live.digest=d.session_digest WHERE live.expires_at>unixepoch() AND live.auth_epoch=?4 AND d.account_id=?1 AND d.tenant_id=?2 AND d.status='active' AND b.active=1 AND json_extract(b.binding,'$.expires_at')>unixepoch() AND r.repository=?3")?.query_map(params![u.account.id,u.tenant.0,repo,u.epoch],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if bindings.len() != 1 || bindings[0].0 != p.device.id.to_string() {
            continue;
        }
        let b: Binding = decode(&bindings[0].1)?;
        if b.expires_at <= now()? {
            continue;
        }
        let repository: GitHubRepository = decode(&repo)?;
        let request = Request {
            device: p.device.clone(),
            invocation: Invocation {
                run: run.parse().map_err(|_| StoreError::Corrupt)?,
                step: 1,
                call: call.parse().map_err(|_| StoreError::Corrupt)?,
                nonce: nonce.parse().map_err(|_| StoreError::Corrupt)?,
            },
            binding: b.clone(),
            operation: Operation::GitHubRepositoryRead,
            repository: Some(repository.clone()),
            digest: Sha256::digest(repository.bytes()).into(),
            expires_at: expiry.min(b.expires_at),
        };
        let json = encode(&request)?;
        conn.execute(
            "INSERT INTO credential_broker_invocations VALUES(?1,?2,1,?3,?4,?5,0)",
            params![u.tenant.0, run, call, nonce, json],
        )?;
        conn.execute("UPDATE credential_broker_jobs SET request=?1 WHERE tenant_id=?2 AND request_id=?3 AND request IS NULL",params![json,u.tenant.0,id])?;
    }
    let requests=conn.prepare("SELECT i.request FROM credential_broker_invocations i WHERE i.tenant_id=?1 AND i.cancelled=0 AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments c WHERE c.tenant_id=i.tenant_id AND c.run_id=i.run_id) LIMIT 64")?.query_map([&u.tenant.0],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().map(|s|decode::<Request>(&s)).collect::<Result<Vec<_>>>()?.into_iter().filter(|r|r.device==p.device && r.expires_at>now().unwrap_or(i64::MAX)).collect();
    Ok(Reply::Waiting {
        requests,
        revocation_pending: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory};
    use moosuri_credential_broker::{Engine, auth::SigningKey, github::GitHubAdapter};
    use moosuri_vault_core::{
        Continue, ItemFields, OsEntropy, PendingVault, RecoveryKey, SecretText, StorageReceipt,
        UnlockedArchive, inventory_digest,
    };
    use moosuri_vault_protocol::broker::{
        Binding as BrokerBinding, Command, Request as BrokerRequest,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    async fn signed(s: &Store, e: &Engine, command: Command) -> Result<Reply> {
        let challenge = s.broker_auth_challenge(e.device().id).await?;
        let input = e
            .sign_command(&challenge, "POST", DEVICE_PATH, command, now()?)
            .map_err(|_| StoreError::Forbidden)?;
        s.broker_command(input).await
    }
    fn archive(device: &Device) -> (Vec<u8>, Uuid) {
        let mut entropy = OsEntropy;
        let password = SecretText::new("Synthetic archive password2026".into()).unwrap();
        let mut pending = PendingVault::prepare(
            moosuri_vault_core::Binding {
                owner_id: device.owner.account,
                tenant_id: device.owner.tenant,
                vault_id: Uuid::new_v4(),
            },
            &password,
            &mut entropy,
        )
        .unwrap();
        let recovery = RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap();
        pending.verify_recovery(&recovery).unwrap();
        let (id, digest) = pending.candidate().unwrap();
        pending
            .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
            .unwrap();
        let mut active = pending.activate().unwrap();
        let fields:ItemFields=serde_json::from_value(serde_json::json!({"kind":"api_key","title":"Synthetic GitHub token","username":"","url":"https://api.github.com","secret":"SYNTHETIC-TOKEN-CANARY-NEVER-OUTPUT","notes":"","favorite":false,"expires_at":"","totp":""})).unwrap();
        let item = active
            .seal_item(Uuid::new_v4(), 1, &fields, &mut entropy)
            .unwrap();
        let id = item.id;
        let inventory = inventory_digest(std::slice::from_ref(&item)).unwrap();
        (
            active
                .export_archive(&[item], inventory, &Continue)
                .unwrap(),
            id,
        )
    }
    async fn assigned(s: &Store, e: &Engine) -> BrokerRequest {
        let Reply::Waiting { mut requests, .. } = signed(s, e, Command::Poll).await.unwrap() else {
            panic!("not waiting")
        };
        assert_eq!(requests.len(), 1);
        requests.remove(0)
    }
    async fn enter(
        s: &Store,
        e: &mut Engine,
        v: &mut UnlockedArchive,
        r: BrokerRequest,
        adapter: &GitHubAdapter,
    ) -> SignedReceipt {
        let invocation = r.invocation.clone();
        e.prepare_github(v, r, now().unwrap()).unwrap();
        let Reply::Commitment { commitment } = signed(
            s,
            e,
            Command::Commit {
                invocation: invocation.clone(),
            },
        )
        .await
        .unwrap() else {
            panic!("not committed")
        };
        e.accept_commitment(*commitment).unwrap();
        e.execute_github(v, &invocation, adapter).await.unwrap()
    }
    #[tokio::test]
    async fn github_actual_adapter_wait_unlock_reuse_canary_and_lost_ack_preserve_original_identity()
     {
        let _serial = TEST_LOCK.lock().await;
        let dir = directory();
        let s = Store::open(dir.path()).unwrap();
        let u = admin(&s).await.user;
        let repo = GitHubRepository {
            owner: "synthetic-owner".into(),
            repository: "synthetic-repo".into(),
        };
        let first = Uuid::new_v4().to_string();
        assert_eq!(
            s.request_github_metadata(&u, first.clone(), repo.clone())
                .await
                .unwrap()
                .status,
            "waiting_for_credential"
        );
        let key = zeroize::Zeroizing::new(SigningKey::generate_pkcs8().unwrap());
        let signer = SigningKey::from_pkcs8(&key).unwrap();
        let challenge = s
            .broker_pair_challenge(
                &u,
                "https://assistant.example.test".into(),
                signer.public_key(),
            )
            .await
            .unwrap();
        s.broker_pair(PairProof {
            challenge: challenge.nonce,
            signature: signer.sign(&pairing_bytes(&challenge).unwrap()),
        })
        .await
        .unwrap();
        let local = directory();
        let root = local.path().join("broker");
        let mut engine =
            Engine::enroll(&root, challenge.device.clone(), challenge.origin, &key).unwrap();
        let (bytes, item) = archive(engine.device());
        let password = SecretText::new("Synthetic archive password2026".into()).unwrap();
        let mut vault = UnlockedArchive::unlock(&bytes, &password).unwrap();
        let binding: BrokerBinding = engine
            .archive_grant(&mut vault, item, repo.clone(), now().unwrap() + 600)
            .unwrap();
        signed(
            &s,
            &engine,
            Command::RegisterRepository {
                binding: binding.clone(),
                repository: repo.clone(),
            },
        )
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let shared = count.clone();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = [0; 16384];
                let n = socket.read(&mut data).await.unwrap();
                let headers = String::from_utf8_lossy(&data[..n]);
                assert!(headers.starts_with("GET /repos/synthetic-owner/synthetic-repo HTTP/1.1"));
                assert!(
                    headers
                        .to_lowercase()
                        .contains("authorization: bearer synthetic-token-canary-never-output")
                );
                shared.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"id":42,"private":true,"archived":false,"stargazers_count":7,"open_issues_count":2,"token":"SYNTHETIC-TOKEN-CANARY-NEVER-OUTPUT","description":"SYNTHETIC-TOKEN-CANARY-NEVER-OUTPUT"}"#;
                let wire = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(wire.as_bytes()).await.unwrap();
            }
        });
        let adapter = GitHubAdapter::synthetic_loopback(address).unwrap();
        let request = assigned(&s, &engine).await;
        let invocation = request.invocation.clone();
        let receipt = enter(&s, &mut engine, &mut vault, request, &adapter).await;
        assert!(!serde_json::to_string(&receipt).unwrap().contains("CANARY"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Simulate successful provider execution + lost assistant settlement ACK and restart.
        drop(engine);
        let mut engine = Engine::open_local(&root).unwrap();
        let retained = engine.receipt(&invocation).unwrap().unwrap();
        assert!(
            engine
                .execute_github(&mut vault, &invocation, &adapter)
                .await
                .is_err()
        );
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(retained.clone()),
            },
        )
        .await
        .unwrap();
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(retained),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            s.github_metadata_jobs(&u).await.unwrap()[0].status,
            "complete"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Same exact scope after local lock/unlock reuses binding, without another selection.
        drop(vault);
        let second = Uuid::new_v4().to_string();
        s.request_github_metadata(&u, second, repo).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut vault = UnlockedArchive::unlock(&bytes, &password).unwrap();
        let request = assigned(&s, &engine).await;
        assert_eq!(request.binding.id, binding.id);
        let receipt = enter(&s, &mut engine, &mut vault, request, &adapter).await;
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(receipt),
            },
        )
        .await
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        // A valid writer commitment response held across the local deadline must
        // not reuse pre-await poll time to authorize provider entry.
        let expiry_id = Uuid::new_v4().to_string();
        let repo = GitHubRepository {
            owner: "synthetic-owner".into(),
            repository: "synthetic-repo".into(),
        };
        s.request_github_metadata(&u, expiry_id, repo)
            .await
            .unwrap();
        let mut expiring = assigned(&s, &engine).await;
        expiring.expires_at = now().unwrap() + 2;
        let stored = expiring.clone();
        s.call(move|db|{db.conn.execute("UPDATE credential_broker_invocations SET request=?1 WHERE tenant_id=?2 AND run_id=?3",params![encode(&stored)?,stored.device.owner.tenant.to_string(),stored.invocation.run.to_string()])?;Ok(())}).await.unwrap();
        engine
            .prepare_github(&vault, expiring.clone(), now().unwrap())
            .unwrap();
        let held = async {
            let committed = signed(
                &s,
                &engine,
                Command::Commit {
                    invocation: expiring.invocation.clone(),
                },
            )
            .await
            .unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            committed
        };
        let Reply::Commitment { commitment } = held.await else {
            panic!("not committed")
        };
        engine.accept_commitment(*commitment).unwrap();
        assert!(
            engine
                .execute_github(&mut vault, &expiring.invocation, &adapter)
                .await
                .is_err()
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "expired held commitment must perform zero new HTTP entries"
        );
        let receipt = engine.settle_without_retry(&expiring.invocation).unwrap();
        assert!(receipt.receipt.outcome == Outcome::NotDispatched);
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(receipt),
            },
        )
        .await
        .unwrap();
        let third = Uuid::new_v4().to_string();
        let repo = GitHubRepository {
            owner: "synthetic-owner".into(),
            repository: "synthetic-repo".into(),
        };
        s.request_github_metadata(&u, third, repo).await.unwrap();
        let request = assigned(&s, &engine).await;
        let mut forged = request.clone();
        forged.repository.as_mut().unwrap().repository = "different-repo".into();
        assert!(
            engine
                .prepare_github(&vault, forged, now().unwrap())
                .is_err()
        );
        engine
            .prepare_github(&vault, request.clone(), now().unwrap())
            .unwrap();
        s.logout(&u).await.unwrap();
        assert!(
            signed(
                &s,
                &engine,
                Command::Commit {
                    invocation: request.invocation
                }
            )
            .await
            .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let tenant = u.tenant.0.clone();
        s.call(move |db| {
            for table in [
                "credential_broker_jobs",
                "credential_broker_invocations",
                "credential_broker_commitments",
            ] {
                let fields = db
                    .conn
                    .prepare(&format!("SELECT * FROM {table} WHERE tenant_id=?1"))?
                    .query_map([&tenant], |r| {
                        let mut text = String::new();
                        for i in 0..r.as_ref().column_count() {
                            if let Ok(v) = r.get::<_, String>(i) {
                                text.push_str(&v);
                            }
                        }
                        Ok(text)
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert!(!fields.join("").contains("CANARY"));
            }
            Ok(())
        })
        .await
        .unwrap();
        server.abort();
    }
}
