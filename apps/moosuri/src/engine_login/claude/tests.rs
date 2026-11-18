use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

#[test]
fn email_is_a_private_single_line() {
    let root = tempfile::Builder::new()
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let file = root.path().join("email");
    fs::write(&file, "owner@example.test\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(&*read_expected_email(&file).unwrap(), "owner@example.test");

    let linked = root.path().join("link");
    symlink(&file, &linked).unwrap();
    assert!(read_expected_email(&linked).is_err());

    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(read_expected_email(&file).is_err());
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&file, "owner@example.test\nother@example.test\n").unwrap();
    assert!(read_expected_email(&file).is_err());
}

#[cfg(feature = "test-fixtures")]
#[tokio::test]
async fn stream_rechecks_account_and_model_before_user_input() {
    use moosuri_connectors::{
        claude_code::fixture::Fixture,
        model::protocol::{TextMessage, TextRole},
    };

    let fixture = Fixture::new();
    let environment = Uuid::new_v4().to_string();
    let expected_email = Zeroizing::new("owner@example.test".to_owned());
    let wrong_model = Adapter::bind(
        fixture.profile.clone(),
        environment.clone(),
        expected_email.clone(),
        "claude-unverified",
    )
    .await;
    assert_eq!(
        wrong_model.err().unwrap(),
        "claude_worker_model_unavailable"
    );
    let model = "claude-fixture-v1";
    let adapter = Adapter::bind(fixture.profile.clone(), environment, expected_email, model)
        .await
        .unwrap();
    let request = TextRequest {
        model: model.into(),
        messages: vec![TextMessage {
            role: TextRole::User,
            text: "Synthetic prompt allowed after initialization".into(),
            phase: None,
        }],
        max_output_tokens: 128,
        effort: None,
    };
    let user_input_marker = fixture.directory.path().join("bound-user");
    for behavior in [
        "different-account",
        "email-missing",
        "external-provider",
        "unsubscribed",
        "catalog-empty",
        "logged-out",
        "ok",
    ] {
        fixture.behavior(behavior);
        let (tx, mut rx) = mpsc::channel(16);
        let result = adapter
            .stream(&request, &tx, &CancellationToken::new())
            .await;
        assert_eq!(result.is_ok(), behavior == "ok", "{behavior}");
        assert_eq!(user_input_marker.exists(), behavior == "ok", "{behavior}");
        if behavior != "ok" {
            assert!(rx.try_recv().is_err(), "{behavior}");
        } else {
            assert_eq!(fs::read_to_string(&user_input_marker).unwrap(), "128");
            assert!(
                std::iter::from_fn(|| rx.try_recv().ok())
                    .any(|event| matches!(event, ModelEvent::Finished { .. }))
            );
        }
    }
    assert_eq!(fixture.launches(), 9);
}
