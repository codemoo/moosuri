use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
fn mutation(action: &str, revision: i64, digest: Option<String>) -> CommunitySkillMutation {
    CommunitySkillMutation {
        action: action.into(),
        expected_revision: revision,
        expected_digest: digest,
    }
}
fn entry() -> Validated {
    catalog().unwrap().remove(0)
}
fn next() -> Validated {
    let mut p = entry().package;
    p.commit = "a".repeat(40);
    p.files.get_mut("moosuri.toml").unwrap().push('\n');
    let side = p.files.get_mut("moosuri.toml").unwrap();
    *side = side
        .replace("1.0.0", "1.1.0")
        .replace("\"knowledge_search\"", "\"knowledge_search\", \"web_read\"");
    let document = p.files.get_mut("SKILL.md").unwrap();
    *document = document.replace(
        "description: Prepare a concise briefing from the user's saved sources and memories.",
        "description: Synthetic updated briefing with optional public web context.",
    );
    p.files
        .get_mut("SKILL.md")
        .unwrap()
        .push_str("\nSynthetic v2 guidance.\n");
    validate_package(p).unwrap()
}
#[test]
fn bounded_identity_rejects_hooks_paths_unknown_capabilities_and_ambiguous_metadata() {
    let original = entry();
    assert_eq!(original.digest.len(), 64);
    let mut other = original.package.clone();
    other.commit = "a".repeat(40);
    assert_ne!(original.digest, validate_package(other).unwrap().digest);
    for (name, value) in [
        ("scripts/run.sh", "echo no"),
        ("../SKILL.md", "no"),
        ("assets/symlink", "no"),
    ] {
        let mut p = original.package.clone();
        p.files.insert(name.into(), value.into());
        assert!(validate_package(p).is_err());
    }
    for side in ["\nhooks = []", "\nformat_version = 1"] {
        let mut p = original.package.clone();
        p.files.get_mut("moosuri.toml").unwrap().push_str(side);
        assert!(validate_package(p).is_err());
    }
    let mut p = original.package.clone();
    p.files
        .get_mut("moosuri.toml")
        .unwrap()
        .push_str("\nallowed_tools = [\"shell\"]");
    assert!(validate_package(p).is_err());
    let mut p = original.package.clone();
    let side = p.files.get_mut("moosuri.toml").unwrap();
    *side = side.replace("knowledge_search", "credential_read");
    assert!(validate_package(p).is_err());
    let mut p = original.package.clone();
    p.files.insert("LICENSE".into(), "x".repeat(32769));
    assert!(validate_package(p).is_err());
    let mut p = original.package;
    p.files.insert("examples.json".into(), "{}".into());
    assert!(validate_package(p).is_err());
}
#[tokio::test]
async fn atomic_lifecycle_is_scoped_and_failed_staging_keeps_old_revision() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "other@example.test").await.user;
    let e = entry();
    let id = e.package.slug.clone();
    let digest = e.digest.clone();
    let first = store
        .mutate_community_skill(&a, id.clone(), mutation("install", 0, Some(digest.clone())))
        .await
        .unwrap();
    assert!(first[0].enabled);
    assert!(
        store
            .community_skills(&b)
            .await
            .unwrap()
            .iter()
            .all(|c| c.installed_digest.is_none())
    );
    store
        .mutate_community_skill(&b, id.clone(), mutation("install", 0, Some(digest.clone())))
        .await
        .unwrap();
    let user = a.clone();
    store.call(move|db|{db.conn.execute_batch("CREATE TEMP TRIGGER fail_community_stage BEFORE UPDATE ON community_installs BEGIN SELECT RAISE(ABORT,'synthetic activation failure'); END;")?;let err=apply(db,user,next(),mutation("update",1,Some(next().digest)));assert!(err.is_err());assert_eq!(db.conn.query_row("SELECT count(*) FROM community_packages WHERE digest=?1",[next().digest],|r|r.get::<_,i64>(0))?,0);db.conn.execute_batch("DROP TRIGGER fail_community_stage")?;Ok(())}).await.unwrap();
    assert_eq!(
        store.community_skills(&a).await.unwrap()[0]
            .installed_digest
            .as_deref(),
        Some(digest.as_str())
    );
    assert_eq!(store.community_skills(&a).await.unwrap()[0].revision, 1);
    let user = a.clone();
    let v2 = next().digest;
    store
        .call(move |db| apply(db, user, next(), mutation("update", 1, Some(v2))))
        .await
        .unwrap();
    assert_eq!(
        store.community_skills(&a).await.unwrap()[0]
            .installed_version
            .as_deref(),
        Some("1.1.0")
    );
    let other = store.community_skills(&b).await.unwrap().remove(0);
    assert_eq!(other.installed_version.as_deref(), Some("1.0.0"));
    assert!(other.enabled);
    store
        .mutate_community_skill(&a, id.clone(), mutation("rollback", 2, None))
        .await
        .unwrap();
    assert_eq!(
        store.community_skills(&a).await.unwrap()[0]
            .installed_digest
            .as_deref(),
        Some(digest.as_str())
    );
    assert!(matches!(
        store
            .mutate_community_skill(&a, id.clone(), mutation("disable", 2, None))
            .await,
        Err(StoreError::Conflict)
    ));
    store
        .mutate_community_skill(&a, id.clone(), mutation("hide", 3, None))
        .await
        .unwrap();
    store
        .mutate_community_skill(&a, id.clone(), mutation("disable", 4, None))
        .await
        .unwrap();
    let user = a.clone();
    store
        .call(move |db| {
            let mut profile = agent_execution::skills::profile();
            admit(&db.conn, &user, &mut profile)?;
            assert!(
                profile
                    .skills
                    .iter()
                    .all(|p| !p.id.starts_with("community:"))
            );
            assert!(read(&db.conn, &entry().pin())?.is_some());
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.community_skills(&a).await.unwrap()[0].hidden);
    store
        .mutate_community_skill(&a, id.clone(), mutation("remove", 5, None))
        .await
        .unwrap();
    let c = store.community_skills(&a).await.unwrap().remove(0);
    assert!(c.installed_digest.is_none());
    assert!(c.hidden);
    assert!(
        store
            .community_skills(&b)
            .await
            .unwrap()
            .iter()
            .all(|c| !c.hidden)
    );
    let other = store.community_skills(&b).await.unwrap().remove(0);
    assert!(other.enabled);
    assert_eq!(other.installed_digest.as_deref(), Some(digest.as_str()));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let owner = a.clone();
    store
        .call(move |db| {
            let mut profile = agent_execution::skills::profile();
            admit(&db.conn, &owner, &mut profile)?;
            assert!(
                profile
                    .skills
                    .iter()
                    .all(|p| p.id != "source-briefing" && p.id != "community:source-briefing")
            );
            Ok(())
        })
        .await
        .unwrap();
    // Explicit restore may re-enable this procedure; an update itself cannot
    // unhide a suggestion or re-enable a disabled package.
    store
        .mutate_community_skill(&a, id.clone(), mutation("install", 6, Some(digest.clone())))
        .await
        .unwrap();
    store
        .mutate_community_skill(&a, id.clone(), mutation("hide", 7, None))
        .await
        .unwrap();
    store
        .mutate_community_skill(&a, id.clone(), mutation("disable", 8, None))
        .await
        .unwrap();
    let owner = a.clone();
    store
        .call(move |db| {
            apply(
                db,
                owner,
                next(),
                mutation("update", 9, Some(next().digest)),
            )
        })
        .await
        .unwrap();
    let updated = store.community_skills(&a).await.unwrap().remove(0);
    assert!(updated.hidden);
    assert!(!updated.enabled);
}

pub(crate) fn activate_v2(db: &mut Database, user: UserContext) -> Result<()> {
    apply(db, user, next(), mutation("update", 1, Some(next().digest)))?;
    Ok(())
}
