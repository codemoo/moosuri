use super::*;
use crate::tests::{TEST_LOCK, admin, member};
async fn seed(s: &Store, u: &UserContext, source: ModelSource) -> ModelSelection {
    match source {
        ModelSource::PersonalClaudeCode => {
            let b = s
                .create_claude_binding(u, "Synthetic vision".into(), "a".repeat(64), "b".repeat(64))
                .await
                .unwrap();
            s.record_claude_probe(u, b.clone(), "claude-synthetic".into())
                .await
                .unwrap();
            s.record_claude_model_efforts(
                u,
                b.clone(),
                "claude-synthetic".into(),
                vec!["high".into()],
            )
            .await
            .unwrap();
            ModelSelection {
                source,
                provider_id: b.id,
                provider_revision: b.revision,
                model: "claude-synthetic".into(),
                effort: None,
            }
        }
        _ => {
            let b = s
                .begin_subscription_login(u, None, "Synthetic vision".into())
                .await
                .unwrap()
                .binding;
            let a = b.clone();
            s.call(move|db|{
                db.conn.execute("UPDATE chatgpt_bindings SET status='active' WHERE id=?1",[&a.id])?;
                db.conn.execute("INSERT INTO chatgpt_sessions(tenant_id,account_id,binding_id,revision,generation,cipher,expires_at,plan_enabled) VALUES(?1,?2,?3,?4,1,x'00',9999999999,1)",params![a.tenant,a.account,a.id,a.revision])?;
                Ok(())
            }).await.unwrap();
            for effort in [None, Some("high".into())] {
                s.record_subscription_probe(
                    u,
                    SubscriptionRef {
                        model: "gpt-synthetic".into(),
                        effort,
                        ..b.clone()
                    },
                )
                .await
                .unwrap();
            }
            ModelSelection {
                source,
                provider_id: b.id,
                provider_revision: b.revision,
                model: "gpt-synthetic".into(),
                effort: None,
            }
        }
    }
}
#[tokio::test]
async fn exact_owner_tuple_requires_one_launch_and_retains_no_pixels_or_answer() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let b = member(&s, &a.user, "vision-owner@example.test").await;
    for source in [
        ModelSource::PersonalClaudeCode,
        ModelSource::PersonalChatGpt,
    ] {
        let selection = seed(&s, &a.user, source).await;
        assert!(!s.personal_vision_proven(&a.user, &selection).await.unwrap());
        assert!(
            s.personal_vision_probe_binding(&b.user, selection.clone())
                .await
                .is_err()
        );
        let unreserved = s
            .personal_vision_probe_binding(&a.user, selection.clone())
            .await
            .unwrap();
        assert!(
            s.record_personal_vision_probe(&a.user, unreserved)
                .await
                .is_err()
        );
        let p = s
            .personal_vision_probe_binding(&a.user, selection.clone())
            .await
            .unwrap();
        s.reserve_personal_vision_probe(&a.user, &p).await.unwrap();
        assert!(s.reserve_personal_vision_probe(&a.user, &p).await.is_err());
        assert!(s.validate_personal_vision_probe(&b.user, &p).await.is_err());
        s.record_personal_vision_probe(&a.user, p).await.unwrap();
        assert!(s.personal_vision_proven(&a.user, &selection).await.unwrap());
        let high = ModelSelection {
            effort: Some("high".into()),
            ..selection.clone()
        };
        assert!(!s.personal_vision_proven(&a.user, &high).await.unwrap());
        let invalid = ModelSelection {
            provider_revision: selection.provider_revision + 1,
            ..selection.clone()
        };
        assert!(s.personal_vision_proven(&a.user, &invalid).await.is_err());
        // A persisted opaque receipt cannot be replayed to mint a second proof.
        let p = s
            .personal_vision_probe_binding(&a.user, high)
            .await
            .unwrap();
        s.reserve_personal_vision_probe(&a.user, &p).await.unwrap();
        let replay = p.frozen();
        s.record_personal_vision_probe(&a.user, p).await.unwrap();
        assert!(
            s.record_personal_vision_probe(&a.user, replay)
                .await
                .is_err()
        );
    }
    s.call(|db| {
        let columns = db
            .conn
            .prepare("PRAGMA table_info(personal_subscription_vision_probes)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert_eq!(
            columns,
            [
                "source",
                "tenant_id",
                "account_id",
                "binding_id",
                "revision",
                "model",
                "effort",
                "harness",
                "verified_at"
            ]
        );
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    s.call(|db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM personal_subscription_vision_probes",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            4
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn revoked_session_or_changed_environment_cannot_record_late_proof() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let selection = seed(&s, &a.user, ModelSource::PersonalClaudeCode).await;
    let p = s
        .personal_vision_probe_binding(&a.user, selection.clone())
        .await
        .unwrap();
    s.reserve_personal_vision_probe(&a.user, &p).await.unwrap();
    let id = selection.provider_id.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE claude_code_bindings SET profile_digest=?2 WHERE id=?1",
            params![id, "c".repeat(64)],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.record_personal_vision_probe(&a.user, p).await.is_err());
    assert!(!s.personal_vision_proven(&a.user, &selection).await.unwrap());
    let p = s
        .personal_vision_probe_binding(&a.user, selection.clone())
        .await
        .unwrap();
    s.reserve_personal_vision_probe(&a.user, &p).await.unwrap();
    s.logout(&a.user).await.unwrap();
    assert!(s.record_personal_vision_probe(&a.user, p).await.is_err());
}
