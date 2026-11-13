use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
fn entry(repository: &str, directory: &str, revision: u8) -> Validated {
    let mut files = catalog().unwrap().remove(0).package.files;
    if revision == 2 {
        let side = files.get_mut("moosuri.toml").unwrap();
        *side = side
            .replace("1.0.0", "1.1.0")
            .replace("\"knowledge_search\"", "\"knowledge_search\", \"web_read\"");
        files
            .get_mut("SKILL.md")
            .unwrap()
            .push_str("\nSynthetic external version two.\n");
    }
    validate(Envelope {
        format_version: 2,
        repository: repository.into(),
        directory: directory.into(),
        commit: format!("{revision:x}").repeat(40),
        slug: "source-briefing".into(),
        files,
    })
    .unwrap()
}
fn change(action: &str, revision: i64, digest: Option<String>) -> CommunitySkillMutation {
    CommunitySkillMutation {
        action: action.into(),
        expected_revision: revision,
        expected_digest: digest,
    }
}
#[test]
fn v2_identity_separates_repositories_directories_and_legacy_pins_without_rehashing_v1() {
    let legacy = catalog().unwrap().remove(0);
    let encoded = encode(&legacy).unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(&encoded)
            .unwrap()
            .get("format_version")
            .is_none()
    );
    assert!(decode(&encoded).unwrap().pin() == legacy.pin());
    let a = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        1,
    );
    let b = entry(
        "https://github.com/another/recipes",
        "skills/source-briefing",
        1,
    );
    let c = entry(
        "https://github.com/example/recipes",
        "other/source-briefing",
        1,
    );
    assert_ne!(a.id(), b.id());
    assert_ne!(a.id(), c.id());
    assert_ne!(a.digest, b.digest);
    assert_ne!(a.pin().id, legacy.pin().id);
    let next = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        2,
    );
    assert_eq!(a.id(), next.id());
    assert_ne!(a.digest, next.digest);
    let encoded = encode(&a).unwrap();
    assert!(encoded.contains("\"format_version\":2"));
    assert!(decode(&encoded).unwrap().pin() == a.pin());
    assert!(decode(&encoded.replace("\"format_version\":2", "\"format_version\":3")).is_err());
}
#[tokio::test]
async fn external_preview_activation_update_rollback_and_remove_are_local_and_do_not_replace_bundled_twins()
 {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "synthetic-other@example.test")
        .await
        .user;
    let first = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        1,
    );
    let id = first.card_id();
    let digest = first.digest.clone();
    let old_pin = first.pin();
    let owner = a.clone();
    let preview = store
        .call(move |db| super::preview(db, owner, first))
        .await
        .unwrap();
    assert!(preview.installed_digest.is_none());
    assert_eq!(preview.revision, 0);
    assert!(preview.github.is_some());
    assert_eq!(store.community_skills(&a).await.unwrap().len(), 6);
    assert!(store.claim_agent_run().await.unwrap().is_none());
    store
        .mutate_community_skill(&a, id.clone(), change("install", 0, Some(digest.clone())))
        .await
        .unwrap();
    let owner = a.clone();
    store
        .call(move |db| {
            let mut profile = agent_execution::skills::profile();
            admit(&db.conn, &owner, &mut profile)?;
            assert_eq!(profile.skills.len(), 7);
            assert!(profile.skills.iter().any(|p| p.id == "source-briefing"));
            assert!(profile.skills.contains(&old_pin));
            assert!(
                profile.community_metadata[0]
                    .description
                    .contains("https://github.com/example/recipes")
            );
            assert!(profile.web.is_none());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(store.community_skills(&b).await.unwrap().len(), 6);
    let second = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        2,
    );
    let next_digest = second.digest.clone();
    let owner = a.clone();
    let preview = store
        .call(move |db| super::preview(db, owner, second))
        .await
        .unwrap();
    assert!(preview.missing.contains(&"web_read".into()));
    store
        .mutate_community_skill(&a, id.clone(), change("update", 1, Some(next_digest)))
        .await
        .unwrap();
    let original = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        1,
    )
    .pin();
    store
        .call(move |db| {
            let doc = read(&db.conn, &original)?.unwrap();
            assert!(!doc.instructions.contains("Synthetic external version two"));
            Ok(())
        })
        .await
        .unwrap();
    store
        .mutate_community_skill(&a, id.clone(), change("rollback", 2, None))
        .await
        .unwrap();
    let own = store
        .community_skills(&a)
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    assert_eq!(own.installed_digest.as_deref(), Some(digest.as_str()));
    store
        .mutate_community_skill(&a, id.clone(), change("remove", 3, None))
        .await
        .unwrap();
    assert_eq!(store.community_skills(&a).await.unwrap().len(), 6);
    let owner = a.clone();
    store
        .call(move |db| {
            let mut profile = agent_execution::skills::profile();
            admit(&db.conn, &owner, &mut profile)?;
            assert_eq!(profile.skills.len(), 6);
            assert!(profile.skills.iter().any(|p| p.id == "source-briefing"));
            Ok(())
        })
        .await
        .unwrap();
    let other = entry(
        "https://github.com/another/recipes",
        "skills/source-briefing",
        1,
    );
    let owner = a.clone();
    let preview = store
        .call(move |db| super::preview(db, owner, other))
        .await
        .unwrap();
    assert_ne!(preview.id, id);
    assert!(matches!(
        store
            .mutate_community_skill(&a, id, change("install", 4, Some(preview.digest)))
            .await,
        Err(StoreError::InvalidInput)
    ));
}
#[tokio::test]
async fn held_acquisition_does_not_hold_actor_and_revoked_context_cannot_stage_or_activate() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let login = admin(&store).await;
    let user = login.user;
    let (release, wait) = tokio::sync::oneshot::channel();
    let worker = store.clone();
    let captured = user.clone();
    let package = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        1,
    );
    let digest = package.digest.clone();
    let held = tokio::spawn(async move {
        wait.await.unwrap();
        worker
            .call(move |db| super::preview(db, captured, package))
            .await
    });
    assert!(store.list_threads(&user).await.is_ok());
    store.logout(&user).await.unwrap();
    release.send(()).unwrap();
    assert!(matches!(held.await.unwrap(), Err(StoreError::Unauthorized)));
    store
        .call(move |db| {
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM community_packages WHERE digest=?1",
                    [digest],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    let package = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        1,
    );
    let id = package.card_id();
    let digest = package.digest;
    assert!(matches!(
        store
            .mutate_community_skill(&user, id, change("install", 0, Some(digest)))
            .await,
        Err(StoreError::Unauthorized)
    ));
}

pub(crate) fn activate_fixture(
    db: &mut Database,
    user: UserContext,
    version: u8,
) -> Result<String> {
    let fixture = entry(
        "https://github.com/example/recipes",
        "skills/source-briefing",
        version,
    );
    let id = fixture.card_id();
    let digest = fixture.digest.clone();
    apply(
        db,
        user,
        fixture,
        change(
            if version == 1 { "install" } else { "update" },
            if version == 1 { 0 } else { 1 },
            Some(digest),
        ),
    )?;
    Ok(id)
}
