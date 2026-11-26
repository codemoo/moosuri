//! Installation-wide scenarios, with distinct synthetic identities and tokens.
use super::*;
use std::sync::atomic::Ordering;

fn all(parent: &Path) -> Options {
    let mut o = enrolled(parent, None);
    o.vault_all_members = true;
    o
}
async fn distinct(o: &Options, email: &str) -> Member {
    let m = member(o, email).await;
    let mut b = m.behaviour.lock().unwrap();
    b.handoff = Some(format!(
        "{:x}{:x}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ));
    b.grant = Some(format!(
        "{:x}{:x}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    ));
    drop(b);
    m
}
async fn send(
    g: &Gateway,
    o: &Options,
    value: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    call(
        adapter_router(g.clone()),
        Method::POST,
        "/internal/v1/vault",
        adapter_headers(&key_of(o)),
        &value.to_string(),
    )
    .await
}
async fn exchange(g: &Gateway, o: &Options, m: &Member) -> (StatusCode, serde_json::Value) {
    let handoff = m.behaviour.lock().unwrap().handoff.clone().unwrap();
    send(
        g,
        o,
        serde_json::json!({"kind":"exchange","handoff":handoff,"vault_origin":ORIGIN}),
    )
    .await
}
#[tokio::test]
async fn invited_members_join_without_reconfiguring_and_tokens_route_only_to_their_owner() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let g = Gateway::new(&o).unwrap();
    // The second account is created after gateway startup.
    let b = distinct(&o, "b@example.test").await;
    for m in [&a, &b] {
        let h = browser_headers(&g, &m.allocation, &Method::GET);
        let (status, body) = call(router(g.clone()), Method::GET, STATUS_PATH, h, "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["state"], "ready");
        assert_eq!(handoff_token(&g, m).await, StatusCode::OK);
        let before = if std::ptr::eq(m, &a) {
            b.count.load(Ordering::SeqCst)
        } else {
            a.count.load(Ordering::SeqCst)
        };
        let (status, reply) = exchange(&g, &o, m).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(reply["owner"]["account"], m.allocation.account_id);
        let other = if std::ptr::eq(m, &a) { &b } else { &a };
        assert_eq!(other.count.load(Ordering::SeqCst), before);
        assert_eq!(
            send(
                &g,
                &o,
                serde_json::json!({"kind":"validate","grant":reply["grant"]})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(send(&g, &o, serde_json::json!({"kind":"validate","grant":reply["grant"],"account":other.allocation.account_id})).await.0, StatusCode::BAD_REQUEST);
        assert!(!g.vault.as_ref().unwrap().fixed_member(&m.allocation));
    }
    a.task.abort();
    b.task.abort();
}
#[tokio::test]
async fn token_collisions_never_replace_another_members_scope_or_kind() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let b = distinct(&o, "b@example.test").await;
    let g = Gateway::new(&o).unwrap();
    let token = a.behaviour.lock().unwrap().handoff.clone();
    b.behaviour.lock().unwrap().handoff = token;
    assert_eq!(handoff_token(&g, &a).await, StatusCode::OK);
    assert_eq!(handoff_token(&g, &b).await, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(exchange(&g, &o, &a).await.0, StatusCode::OK);
    // A grant cannot be re-registered as a handoff, including by the same owner.
    let grant = a.behaviour.lock().unwrap().grant.clone();
    a.behaviour.lock().unwrap().handoff = grant.clone();
    assert_eq!(handoff_token(&g, &a).await, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        send(&g, &o, serde_json::json!({"kind":"validate","grant":grant}))
            .await
            .0,
        StatusCode::OK
    );
    a.task.abort();
    b.task.abort();
}
#[tokio::test]
async fn fencing_and_restart_deny_old_grants_without_cross_member_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let b = distinct(&o, "b@example.test").await;
    let g = Gateway::new(&o).unwrap();
    for m in [&a, &b] {
        assert_eq!(handoff_token(&g, m).await, StatusCode::OK);
        assert_eq!(exchange(&g, &o, m).await.0, StatusCode::OK);
    }
    let grant = a.behaviour.lock().unwrap().grant.clone().unwrap();
    runtime_registry::set_routing(&g.registry, &a.allocation, false).unwrap();
    let before = (
        a.count.load(Ordering::SeqCst),
        b.count.load(Ordering::SeqCst),
    );
    assert_eq!(
        send(&g, &o, serde_json::json!({"kind":"validate","grant":grant}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        (
            a.count.load(Ordering::SeqCst),
            b.count.load(Ordering::SeqCst)
        ),
        before
    );
    let restarted = Gateway::new(&o).unwrap();
    let grant = b.behaviour.lock().unwrap().grant.clone().unwrap();
    assert_eq!(
        send(
            &restarted,
            &o,
            serde_json::json!({"kind":"validate","grant":grant})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    a.task.abort();
    b.task.abort();
}
#[tokio::test]
async fn lifecycle_redelivery_ack_identity_and_round_robin_preserve_both_members() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let b = distinct(&o, "b@example.test").await;
    let g = Gateway::new(&o).unwrap();
    for m in [&a, &b] {
        m.behaviour.lock().unwrap().lifecycle = Some(Uuid::new_v4());
    }
    let next = serde_json::json!({"kind":"lifecycle_next"});
    let (status, first) = send(&g, &o, next.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(send(&g, &o, next.clone()).await.1, first);
    let selected = if first["owner"]["account"] == a.allocation.account_id {
        &a
    } else {
        &b
    };
    let wrong = Uuid::new_v4();
    assert_eq!(
        send(
            &g,
            &o,
            serde_json::json!({"kind":"lifecycle_ack","event":wrong})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    selected.behaviour.lock().unwrap().ack_digest = Some("reset".into());
    let ack = serde_json::json!({"kind":"lifecycle_ack","event":first["operation"]});
    assert_eq!(
        send(&g, &o, ack.clone()).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(send(&g, &o, next.clone()).await.1, first);
    selected.behaviour.lock().unwrap().ack_digest = None;
    assert_eq!(send(&g, &o, ack).await.0, StatusCode::OK);
    let (status, second) = send(&g, &o, next.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(second["owner"]["account"], first["owner"]["account"]);
    assert_eq!(
        send(
            &g,
            &o,
            serde_json::json!({"kind":"lifecycle_ack","event":second["operation"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(send(&g, &o, next).await.0, StatusCode::NOT_FOUND);
    a.task.abort();
    b.task.abort();
}
#[tokio::test]
async fn lifecycle_fence_drops_pending_route_and_continues_with_the_next_member() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let b = distinct(&o, "b@example.test").await;
    let g = Gateway::new(&o).unwrap();
    for m in [&a, &b] {
        m.behaviour.lock().unwrap().lifecycle = Some(Uuid::new_v4());
    }
    let next = serde_json::json!({"kind":"lifecycle_next"});
    let (_, first) = send(&g, &o, next.clone()).await;
    let selected = if first["owner"]["account"] == a.allocation.account_id {
        &a
    } else {
        &b
    };
    runtime_registry::set_routing(&g.registry, &selected.allocation, false).unwrap();
    let before = selected.count.load(Ordering::SeqCst);
    assert_eq!(
        send(
            &g,
            &o,
            serde_json::json!({"kind":"lifecycle_ack","event":first["operation"]})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(selected.count.load(Ordering::SeqCst), before);
    let (status, second) = send(&g, &o, next).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(second["owner"]["account"], first["owner"]["account"]);
    a.task.abort();
    b.task.abort();
}
#[tokio::test]
async fn lifecycle_skips_empty_members_and_serializes_overlapping_requests() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let b = distinct(&o, "b@example.test").await;
    let g = Gateway::new(&o).unwrap();
    let selected = if a.allocation.account_id > b.allocation.account_id {
        &a
    } else {
        &b
    };
    selected.behaviour.lock().unwrap().lifecycle = Some(Uuid::new_v4());
    let next = serde_json::json!({"kind":"lifecycle_next"});
    let locked = g.vault.as_ref().unwrap().lifecycle.lock().await;
    assert_eq!(
        send(&g, &o, next.clone()).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(locked);
    let (status, event) = send(&g, &o, next).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(event["owner"]["account"], selected.allocation.account_id);
    a.task.abort();
    b.task.abort();
}

#[test]
fn installation_mode_requires_complete_configuration_and_excludes_fixed_mode() {
    use clap::Parser;
    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        options: Options,
    }
    let base = [
        "gateway",
        "--public-origin",
        "https://assistant.example.test",
        "--registry",
        "/srv/registry",
        "--ipc-root",
        "/srv/ipc",
        "--socket-dir",
        "/srv/socket",
    ];
    let all_args = [
        "--vault-all-members",
        "--vault-site-origin",
        ORIGIN,
        "--vault-adapter-listen",
        "127.0.0.1:41999",
        "--vault-adapter-key-file",
        "/srv/key",
    ];
    let args: Vec<_> = base.iter().chain(all_args.iter()).copied().collect();
    assert!(Cli::try_parse_from(&args).is_ok());
    let mut both = args.clone();
    both.extend(["--vault-member", "0b7f0d57-5c3b-4a3c-a9b0-3d5a9d4f7b11"]);
    assert!(Cli::try_parse_from(both).is_err());
    assert!(Cli::try_parse_from(base.iter().copied().chain(["--vault-all-members"])).is_err());
    assert!(
        Cli::try_parse_from(
            base.iter()
                .copied()
                .chain(["--vault-adapter-listen", "127.0.0.1:41999"])
        )
        .is_err()
    );
}

#[tokio::test]
async fn one_member_cannot_fill_the_installations_token_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let b = distinct(&o, "b@example.test").await;
    let g = Gateway::new(&o).unwrap();
    for i in 0..MAX_MEMBER_TOKENS {
        a.behaviour.lock().unwrap().handoff = Some(format!("{i:064x}"));
        assert_eq!(handoff_token(&g, &a).await, StatusCode::OK);
    }
    a.behaviour.lock().unwrap().handoff = Some(format!("{:064x}", MAX_MEMBER_TOKENS));
    assert_eq!(handoff_token(&g, &a).await, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(handoff_token(&g, &b).await, StatusCode::OK);
    assert_eq!(exchange(&g, &o, &b).await.0, StatusCode::OK);
    a.task.abort();
    b.task.abort();
}

#[tokio::test]
async fn a_failed_lifecycle_ack_does_not_starve_other_members() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let o = all(&root);
    let a = distinct(&o, "a@example.test").await;
    let b = distinct(&o, "b@example.test").await;
    let g = Gateway::new(&o).unwrap();
    for m in [&a, &b] {
        m.behaviour.lock().unwrap().lifecycle = Some(Uuid::new_v4());
    }
    let next = serde_json::json!({"kind":"lifecycle_next"});
    let (_, first) = send(&g, &o, next.clone()).await;
    let selected = if first["owner"]["account"] == a.allocation.account_id {
        &a
    } else {
        &b
    };
    selected.behaviour.lock().unwrap().ack_failure = true;
    assert_eq!(
        send(
            &g,
            &o,
            serde_json::json!({"kind":"lifecycle_ack","event":first["operation"]})
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let (status, second) = send(&g, &o, next.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(second["owner"]["account"], first["owner"]["account"]);
    assert_eq!(
        send(
            &g,
            &o,
            serde_json::json!({"kind":"lifecycle_ack","event":second["operation"]})
        )
        .await
        .0,
        StatusCode::OK
    );
    selected.behaviour.lock().unwrap().ack_failure = false;
    // The durable event was not acknowledged or lost; the next sweep retries it.
    assert_eq!(send(&g, &o, next).await.1, first);
    a.task.abort();
    b.task.abort();
}
