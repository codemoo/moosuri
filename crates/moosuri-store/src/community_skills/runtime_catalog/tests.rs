use super::*;
use crate::tests::{TEST_LOCK, admin, member};
fn source() -> CatalogSource {
    CatalogSource::new(SOURCE, "catalog", "release").unwrap()
}
fn fixture() -> (Validated, Vec<u8>) {
    let old = catalog().unwrap().remove(0);
    let e=external::decode_catalog_artifact(serde_json::json!({"format_version":2,"repository":SOURCE,"directory":"community/skills/source-briefing","commit":COMMIT,"slug":old.package.slug,"files":old.package.files}).to_string().as_bytes()).unwrap();
    let artifact = external::encode(&e).unwrap();
    let index=serde_json::json!({"format_version":1,"repository":SOURCE,"source_commit":COMMIT,"packages":[{"directory":e.external.as_ref().unwrap().directory,"id":e.id(),"name":e.package.slug,"version":e.metadata.version,"description":e.description,"license":e.metadata.license,"capabilities":e.metadata.capabilities,"envelope_version":2,"content_digest":e.digest,"artifact":format!("package-{}.json",e.digest),"artifact_sha256":format!("{:x}",Sha256::digest(artifact))}]}).to_string().into_bytes();
    (e, index)
}
async fn age(s: &Store, source: CatalogSource) -> Result<()> {
    s.call(move |db| {
        db.conn.execute(
            "UPDATE community_catalog_cache SET checked_at=0,lease_until=0 WHERE source_key=?1",
            [source.key()],
        )?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn previewing_an_update_preserves_installed_version_readiness_in_both_directions() {
    let _g = TEST_LOCK.lock().await;
    for installed_needs_web in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await;
        let (base, _) = fixture();
        let variant = |needs_web: bool, version: &str, commit: &str| {
            let mut value: serde_json::Value =
                serde_json::from_str(&external::encode(&base).unwrap()).unwrap();
            let capabilities = if needs_web {
                vec!["knowledge_search", "web_read"]
            } else {
                vec!["knowledge_search"]
            };
            value["commit"] = commit.into();
            value["files"]["moosuri.toml"] = format!(
                "format_version = 1\nversion = \"{version}\"\nmode = \"instructions\"\nlicense = \"Apache-2.0\"\ncapabilities = {}\n",
                serde_json::to_string(&capabilities).unwrap()
            ).into();
            external::decode_catalog_artifact(value.to_string().as_bytes()).unwrap()
        };
        let installed = variant(installed_needs_web, "1.0.0", &"1".repeat(40));
        let offered = variant(!installed_needs_web, "2.0.0", &"2".repeat(40));
        let id = installed.id();
        let installed_digest = installed.digest.clone();
        let offered_digest = offered.digest.clone();
        let artifact = external::encode(&offered).unwrap();
        let index = serde_json::json!({"format_version":1,"repository":SOURCE,"source_commit":offered.package.commit,"packages":[{"directory":offered.external.as_ref().unwrap().directory,"id":id,"name":offered.package.slug,"version":offered.metadata.version,"description":offered.description,"license":offered.metadata.license,"capabilities":offered.metadata.capabilities,"envelope_version":2,"content_digest":offered.digest,"artifact":format!("package-{}.json",offered.digest),"artifact_sha256":format!("{:x}",Sha256::digest(artifact))}]}).to_string().into_bytes();
        // Synthetic previously verified snapshots, as in the cache-reuse fixture.
        s.call(move |db| {
            stage(&db.conn, &installed)?;
            stage(&db.conn, &offered)
        })
        .await
        .unwrap();
        s.mutate_community_skill(
            &a.user,
            id.clone(),
            CommunitySkillMutation {
                action: "install".into(),
                expected_revision: 0,
                expected_digest: Some(installed_digest.clone()),
            },
        )
        .await
        .unwrap();
        let source = source();
        let lease = s
            .begin_community_catalog_refresh(&a.user, source.clone(), false)
            .await
            .unwrap()
            .unwrap();
        s.complete_community_catalog_refresh(
            &a.user,
            lease,
            Ok(IndexResponse::Modified {
                bytes: index,
                etag: None,
            }),
        )
        .await
        .unwrap();
        let before = s
            .community_catalog_cards(&a.user, source.clone())
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.id == id)
            .unwrap();
        let preview = s
            .community_catalog_preview(&a.user, source, id, offered_digest.clone())
            .await
            .unwrap();
        let after = s
            .reuse_community_catalog_preview(&a.user, &preview)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            before.readiness,
            if installed_needs_web {
                "connection_needed"
            } else {
                "ready"
            }
        );
        assert_eq!(
            after.readiness, before.readiness,
            "preview cannot change the installed version's readiness"
        );
        assert_eq!(after.missing, before.missing);
        assert_eq!(
            after.installed_digest.as_deref(),
            Some(installed_digest.as_str())
        );
        assert_eq!(after.digest, offered_digest);
        assert!(after.catalog.unwrap().verified);
    }
}
#[tokio::test]
async fn index_only_offers_cache_etag_offline_corruption_and_source_identity_are_fenced() {
    let _g = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let a = admin(&s).await;
    let (e, bytes) = fixture();
    let source = source();
    let lease = s
        .begin_community_catalog_refresh(&a.user, source.clone(), false)
        .await
        .unwrap()
        .unwrap();
    assert!(
        s.begin_community_catalog_refresh(&a.user, source.clone(), true)
            .await
            .unwrap()
            .is_none()
    );
    s.complete_community_catalog_refresh(
        &a.user,
        lease,
        Ok(IndexResponse::Modified {
            bytes: bytes.clone(),
            etag: Some("\"synthetic\"".into()),
        }),
    )
    .await
    .unwrap();
    assert!(
        s.begin_community_catalog_refresh(&a.user, source.clone(), false)
            .await
            .unwrap()
            .is_none()
    );
    let offer = s
        .community_catalog_cards(&a.user, source.clone())
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.id == e.id())
        .unwrap();
    assert_eq!(offer.readiness, "preview_required");
    assert!(!offer.catalog.unwrap().verified);
    assert!(
        s.mutate_community_skill(
            &a.user,
            e.id(),
            CommunitySkillMutation {
                action: "install".into(),
                expected_revision: 0,
                expected_digest: Some(e.digest.clone())
            }
        )
        .await
        .is_err()
    );
    age(&s, source.clone()).await.unwrap();
    let lease = s
        .begin_community_catalog_refresh(&a.user, source.clone(), true)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.etag.as_deref(), Some("\"synthetic\""));
    s.complete_community_catalog_refresh(
        &a.user,
        lease,
        Err(moosuri_connectors::github_catalog::Error::Unavailable),
    )
    .await
    .unwrap();
    assert_eq!(
        s.community_catalog_status(&a.user, Some(source.clone()))
            .await
            .unwrap()
            .state,
        "offline_cache"
    );
    assert!(
        s.community_catalog_cards(&a.user, source.clone())
            .await
            .unwrap()
            .iter()
            .any(|c| c.id == e.id())
    );
    let source2 = CatalogSource::new(SOURCE, "other-ref", "release").unwrap();
    assert!(
        !s.community_catalog_cards(&a.user, source2)
            .await
            .unwrap()
            .iter()
            .any(|c| c.id == e.id())
    );
    let key = source.key();
    s.call(move|db|{db.conn.execute("UPDATE community_catalog_cache SET index_json='corrupt',checked_at=0 WHERE source_key=?1",[key])?;Ok(())}).await.unwrap();
    assert_eq!(
        s.community_catalog_status(&a.user, Some(source.clone()))
            .await
            .unwrap()
            .state,
        "corrupt"
    );
    assert!(
        !s.community_catalog_cards(&a.user, source.clone())
            .await
            .unwrap()
            .iter()
            .any(|c| c.id == e.id())
    );
    let lease = s
        .begin_community_catalog_refresh(&a.user, source.clone(), true)
        .await
        .unwrap()
        .unwrap();
    assert!(lease.etag.is_none());
    s.complete_community_catalog_refresh(&a.user, lease, Ok(IndexResponse::NotModified))
        .await
        .unwrap();
    assert_eq!(
        s.community_catalog_status(&a.user, Some(source))
            .await
            .unwrap()
            .state,
        "corrupt"
    );
}
#[tokio::test]
async fn preferences_are_private_and_verified_cache_reuse_does_not_install_or_expand_authority() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let b = member(&s, &a.user, "other@example.test").await;
    let (e, bytes) = fixture();
    let source = source();
    let l = s
        .begin_community_catalog_refresh(&a.user, source.clone(), false)
        .await
        .unwrap()
        .unwrap();
    s.complete_community_catalog_refresh(
        &a.user,
        l,
        Ok(IndexResponse::Modified { bytes, etag: None }),
    )
    .await
    .unwrap();
    s.set_community_catalog_hidden(&a.user, source.clone(), e.id(), e.digest.clone(), 0, true)
        .await
        .unwrap();
    let cards = s
        .community_catalog_cards(&b.user, source.clone())
        .await
        .unwrap();
    assert!(!cards.iter().find(|c| c.id == e.id()).unwrap().hidden);
    let p = s
        .community_catalog_preview(&a.user, source.clone(), e.id(), e.digest.clone())
        .await
        .unwrap();
    assert!(
        s.reuse_community_catalog_preview(&a.user, &p)
            .await
            .unwrap()
            .is_none()
    );
    // Private test staging stands for the existing GitHub acquisition receipt.
    let digest = e.digest.clone();
    s.call(move |db| stage(&db.conn, &e)).await.unwrap();
    let reused = s
        .reuse_community_catalog_preview(&a.user, &p)
        .await
        .unwrap()
        .unwrap();
    assert!(reused.catalog.unwrap().verified);
    assert!(reused.installed_digest.is_none());
    s.mutate_community_skill(
        &a.user,
        p.item.id.clone(),
        CommunitySkillMutation {
            action: "install".into(),
            expected_revision: 1,
            expected_digest: Some(digest),
        },
    )
    .await
    .unwrap();
    assert!(
        s.community_catalog_cards(&b.user, source.clone())
            .await
            .unwrap()
            .iter()
            .find(|c| c.id == p.item.id)
            .unwrap()
            .installed_digest
            .is_none()
    );
    let l = {
        age(&s, source.clone()).await.unwrap();
        s.begin_community_catalog_refresh(&b.user, source.clone(), true)
            .await
            .unwrap()
            .unwrap()
    };
    s.logout(&b.user).await.unwrap();
    assert!(
        s.complete_community_catalog_refresh(&b.user, l, Ok(IndexResponse::NotModified))
            .await
            .is_err()
    );
    assert!(
        s.set_community_catalog_hidden(&a.user, source, p.item.id, p.item.content_digest, 1, false)
            .await
            .is_err(),
        "stale preference CAS"
    );
}
#[test]
fn index_and_artifact_self_claims_cannot_bypass_exact_validator() {
    let (e, bytes) = fixture();
    let source = source();
    let mut index = CatalogIndex::parse(&bytes, &source).unwrap();
    index.packages[0].content_digest = "f".repeat(64);
    assert!(item_matches(&e, &index, &index.packages[0]).is_err());
    let duplicate = b"{\"format_version\":2,\"format_version\":2}";
    assert!(external::decode_catalog_artifact(duplicate).is_err());
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["packages"][0]["artifact"] = "../../secret".into();
    assert!(CatalogIndex::parse(v.to_string().as_bytes(), &source).is_err());
    v = serde_json::from_slice(&bytes).unwrap();
    let item = v["packages"][0].clone();
    v["packages"].as_array_mut().unwrap().push(item);
    assert!(CatalogIndex::parse(v.to_string().as_bytes(), &source).is_err());
}

#[tokio::test]
async fn moving_index_and_lease_reclamation_reject_old_preview_and_late_publish_without_installation()
 {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let source = source();
    let (e, bytes) = fixture();
    let first = s
        .begin_community_catalog_refresh(&a.user, source.clone(), false)
        .await
        .unwrap()
        .unwrap();
    age(&s, source.clone()).await.unwrap();
    let newer = s
        .begin_community_catalog_refresh(&a.user, source.clone(), true)
        .await
        .unwrap()
        .unwrap();
    assert!(
        s.complete_community_catalog_refresh(
            &a.user,
            first,
            Ok(IndexResponse::Modified {
                bytes: bytes.clone(),
                etag: None
            })
        )
        .await
        .is_err()
    );
    s.complete_community_catalog_refresh(
        &a.user,
        newer,
        Ok(IndexResponse::Modified {
            bytes: bytes.clone(),
            etag: Some("\"etag\"".into()),
        }),
    )
    .await
    .unwrap();
    let p = s
        .community_catalog_preview(&a.user, source.clone(), e.id(), e.digest.clone())
        .await
        .unwrap();
    age(&s, source.clone()).await.unwrap();
    let next = s
        .begin_community_catalog_refresh(&a.user, source.clone(), true)
        .await
        .unwrap()
        .unwrap();
    s.complete_community_catalog_refresh(&a.user, next, Ok(IndexResponse::NotModified))
        .await
        .unwrap();
    assert!(
        s.reuse_community_catalog_preview(&a.user, &p)
            .await
            .is_err()
    );
    let helper = s
        .login_chatgpt_helper("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap();
    assert!(
        s.community_catalog_cards(&helper.user, source.clone())
            .await
            .is_err()
    );
    assert!(
        s.begin_community_catalog_refresh(&helper.user, source, true)
            .await
            .is_err()
    );
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM community_packages", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM community_installs", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn schema34_to35_preserves_helper_purpose_and_target_host_while_adding_empty_public_cache() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let helper = s
        .login_chatgpt_helper("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap();
    let ticket = s
        .prepare_chatgpt_handoff(&helper.user, None, "Preserved P2".into())
        .await
        .unwrap();
    let host = ticket.context.target_host.clone();
    let token = helper.token;
    s.call(|db| {
        db.conn
            .execute_batch("ALTER TABLE chatgpt_model_probes DROP COLUMN function_tools;DROP TABLE community_catalog_cache;PRAGMA user_version=34;")?;
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let helper = s.authenticate(token).await.unwrap();
    assert!(helper.is_chatgpt_helper());
    assert!(helper.helper_deadline().is_some());
    assert_eq!(
        s.chatgpt_handoff_status(&helper, ticket.context.ticket)
            .await
            .unwrap()
            .phase,
        "prepared"
    );
    s.call(move |db| {
        assert_eq!(
            db.conn
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
            crate::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            db.conn.query_row(
                "SELECT host_id FROM subscription_runtime WHERE singleton=1",
                [],
                |r| r.get::<_, String>(0)
            )?,
            host
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM community_catalog_cache", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.authenticate(a.token).await.is_ok());
}

#[tokio::test]
async fn last_verified_public_index_and_daily_etag_state_survive_restart_without_user_activation() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let source = source();
    let (e, bytes) = fixture();
    let l = s
        .begin_community_catalog_refresh(&a.user, source.clone(), false)
        .await
        .unwrap()
        .unwrap();
    s.complete_community_catalog_refresh(
        &a.user,
        l,
        Ok(IndexResponse::Modified {
            bytes,
            etag: Some("\"persisted\"".into()),
        }),
    )
    .await
    .unwrap();
    let token = a.token;
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let u = s.authenticate(token).await.unwrap();
    assert!(
        s.begin_community_catalog_refresh(&u, source.clone(), false)
            .await
            .unwrap()
            .is_none()
    );
    let cards = s.community_catalog_cards(&u, source.clone()).await.unwrap();
    let card = cards.iter().find(|c| c.id == e.id()).unwrap();
    assert!(card.installed_digest.is_none());
    assert_eq!(card.readiness, "preview_required");
    age(&s, source.clone()).await.unwrap();
    let l = s
        .begin_community_catalog_refresh(&u, source.clone(), true)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(l.etag.as_deref(), Some("\"persisted\""));
    s.complete_community_catalog_refresh(
        &u,
        l,
        Err(moosuri_connectors::github_catalog::Error::Unavailable),
    )
    .await
    .unwrap();
    assert_eq!(
        s.community_catalog_status(&u, Some(source))
            .await
            .unwrap()
            .state,
        "offline_cache"
    );
}
