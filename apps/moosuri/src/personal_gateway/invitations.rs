//! Public one-use invitation admission into the existing pinned host worker.
//! No password or capability is written to a job, argument, trace or log.
use super::*;
use crate::kiri_invite::WebInput;
use crate::kiri_lifecycle::worker::WebEnrollment;
use moosuri_store::runtime_registry::admin::{self as control, invitation_links};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn configuration(o: &Options) -> Result<Option<WebEnrollment>, String> {
    match (
        &o.onboard_config,
        &o.onboard_policy,
        &o.onboard_operations_root,
    ) {
        (None, None, None) => Ok(None),
        (Some(config), Some(policy), Some(operations_root)) => {
            let c = WebEnrollment {
                config: config.clone(),
                policy: policy.clone(),
                operations_root: operations_root.clone(),
            };
            c.validate(&o.registry, &o.public_origin)?;
            Ok(Some(c))
        }
        _ => Err("onboard_web_profile_incomplete".into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Issue {
    request_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    token: String,
    #[serde(default)]
    password: Option<String>,
}

pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route("/api/v1/personal/admin/invitations/link", post(issue))
        .route("/api/v1/personal/invitations/status", post(redeem))
        .route("/api/v1/personal/invitations/redeem", post(redeem))
        .layer(DefaultBodyLimit::max(4096))
}
fn admit(g: &Gateway, headers: &HeaderMap, uri: &Uri) -> Result<(), StatusCode> {
    let ip = g.headers(headers, &Method::POST)?;
    if uri.query().is_some()
        || one(headers, "content-type").and_then(|s| s.split(';').next())
            != Some("application/json")
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if g.onboarding.is_none() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    if !g.admit(ip) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    Ok(())
}
async fn issue(
    State(g): State<Gateway>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        admit(&g, &headers, &uri)?;
        let input: Issue = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
        let (a, view) = admin::authenticated_session(&g, &headers).await?;
        let root = g.registry.clone();
        let origin = g.origin.clone();
        tokio::task::spawn_blocking(move || {
            let pin = control::checked_session(&root, &a, &view)
                .and_then(|s| s.at_epoch(a.epoch))
                .map_err(admin::error)?;
            let mut random = Zeroizing::new([0u8; 32]);
            SystemRandom::new()
                .fill(random.as_mut())
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            let token = Zeroizing::new(URL_SAFE_NO_PAD.encode(random.as_ref()));
            let link = invitation_links::issue(&root, &pin, input.request_id, &token)
                .map_err(admin::error)?;
            Ok::<_, StatusCode>(response(StatusCode::OK, json!({
                "url": format!("{origin}/join#{}", token.as_str()), "expires_at": link.expires_at
            }).to_string().into_bytes()))
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    }
    .await;
    result.unwrap_or_else(failure)
}
async fn redeem(
    State(g): State<Gateway>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        admit(&g, &headers, &uri)?;
        let input: Submission =
            serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
        let token = Zeroizing::new(input.token);
        let password = input.password.map(Zeroizing::new);
        let submit = uri.path().ends_with("/redeem");
        if submit
            && !password.as_ref().is_some_and(|p| {
                moosuri_core::account_password_valid(p) && !p.contains(['\r', '\n'])
            })
            || !submit && password.is_some()
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        // Registry I/O may wait for a bounded password initialization. Keep it
        // off async runtime workers; status never takes the initializer permit.
        let root = g.registry.clone();
        let (link, token) = tokio::task::spawn_blocking(move || {
            invitation_links::inspect(&root, &token)
                .map(|link| (link, token))
                .map_err(admin::error)
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)??;
        if !submit || link.ready {
            return Ok(response(
                StatusCode::OK,
                json!({"name":link.invitation.name,
                "email":link.invitation.email,"ready":link.ready,"password_set":false})
                .to_string()
                .into_bytes(),
            ));
        }
        // One validated initializer; existing persisted operation locks also
        // exclude the timer/CLI and another gateway process.
        let permit = g
            .invitation_inflight
            .clone()
            .try_acquire_owned()
            .map_err(|_| StatusCode::CONFLICT)?;
        let runtime = tokio::runtime::Handle::current();
        let task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let input = WebInput {
                token,
                password: password.ok_or(StatusCode::BAD_REQUEST)?,
                password_set: AtomicBool::new(false),
            };
            let c = g
                .onboarding
                .as_ref()
                .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
            runtime
                .block_on(c.redeem(&g.registry, &g.origin, &input))
                .map_err(|reason| match reason.as_str() {
                    "invite_link_unavailable" | "invite_link_identity_changed" => {
                        StatusCode::FORBIDDEN
                    }
                    "onboard_web_preparing" | "onboard_operation_in_progress" => {
                        StatusCode::CONFLICT
                    }
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                })?;
            Ok::<_, StatusCode>(response(
                StatusCode::OK,
                json!({"ready":true,
                "password_set":input.password_set.load(Ordering::Relaxed)})
                .to_string()
                .into_bytes(),
            ))
        });
        // A lost HTTP response does not drop an initializer after it wrote the
        // password. Retry checks private initialization and never replaces it.
        task.await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    }
    .await;
    result.unwrap_or_else(failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;
    fn fixture() -> (tempfile::TempDir, Gateway, String) {
        let d = tempfile::tempdir().unwrap();
        let o = super::super::tests::options(&d.path().canonicalize().unwrap());
        let mut g = Gateway::new(&o).unwrap();
        // This fixture never calls a real initializer; the installed browser
        // suite covers that. It enables only the capability/status boundary.
        g.onboarding = Some(WebEnrollment {
            config: "/unused".into(),
            policy: "/unused".into(),
            operations_root: "/unused".into(),
        });
        let a = runtime_registry::reserve(&g.registry, Uuid::new_v4(), "admin@example.test".into())
            .unwrap();
        runtime_registry::set_routing(&g.registry, &a, true).unwrap();
        let v = moosuri_core::SessionView {
            user: moosuri_core::AccountSummary {
                id: a.account_id.clone(),
                name: "Admin".into(),
                email: "admin@example.test".into(),
                role: moosuri_core::Role::Member,
                must_change_password: false,
            },
            password_change_required: false,
            chat_draft_scope: None,
        };
        control::bootstrap(&g.registry, Uuid::new_v4(), &a, &v).unwrap();
        let pin = control::checked_session(&g.registry, &a, &v).unwrap();
        let i = control::reserve_invitation(
            &g.registry,
            &pin,
            Uuid::new_v4(),
            "Guest".into(),
            "guest@example.test".into(),
        )
        .unwrap();
        let token = URL_SAFE_NO_PAD.encode([37u8; 32]);
        invitation_links::issue(&g.registry, &pin, i.request_id, &token).unwrap();
        (d, g, token)
    }
    fn req(path: &str, origin: &str, value: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "assistant.example.test")
            .header("origin", origin)
            .header("x-real-ip", "192.0.2.1")
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap()
    }
    #[tokio::test]
    async fn public_capability_status_is_private_and_completed_replay_never_initializes() {
        let (_d, g, t) = fixture();
        let app = router(g.clone());
        let response = app
            .clone()
            .oneshot(req(
                "/api/v1/personal/invitations/status",
                &g.origin,
                json!({"token":t}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
        let link = invitation_links::inspect(&g.registry, &t).unwrap();
        let a = runtime_registry::lookup(&g.registry, link.invitation.email).unwrap();
        invitation_links::activate(&g.registry, &t, &a).unwrap();
        let response = app
            .oneshot(req(
                "/api/v1/personal/invitations/redeem",
                &g.origin,
                json!({"token":t,"password":"replacement42"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["ready"], true);
        assert_eq!(value["password_set"], false);
    }
    #[tokio::test]
    async fn foreign_origin_query_malformed_input_and_invalid_capability_are_refused() {
        let (_d, g, t) = fixture();
        let app = router(g.clone());
        for (path, origin, body, expected) in [
            (
                "/api/v1/personal/invitations/status",
                "https://foreign.test",
                json!({"token":t}),
                StatusCode::FORBIDDEN,
            ),
            (
                "/api/v1/personal/invitations/status?token=bad",
                g.origin.as_str(),
                json!({"token":t}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "/api/v1/personal/invitations/status",
                g.origin.as_str(),
                json!({"token":"invalid"}),
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/api/v1/personal/invitations/redeem",
                g.origin.as_str(),
                json!({"token":t,"password":"short"}),
                StatusCode::BAD_REQUEST,
            ),
            (
                "/api/v1/personal/invitations/redeem",
                g.origin.as_str(),
                json!({"token":t,"password":"chosen42","config":"/etc/other"}),
                StatusCode::BAD_REQUEST,
            ),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(req(path, origin, body))
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
        let response = app
            .oneshot(req(
                "/api/v1/personal/admin/invitations/link",
                &g.origin,
                json!({"request_id":Uuid::new_v4()}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn unconfigured_installation_and_rate_limit_do_not_enter_the_worker() {
        let (_d, mut g, t) = fixture();
        g.onboarding = None;
        assert_eq!(
            router(g.clone())
                .oneshot(req(
                    "/api/v1/personal/invitations/status",
                    &g.origin,
                    json!({"token":t})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let (_d, g, t) = fixture();
        let app = router(g.clone());
        for _ in 0..20 {
            assert!(g.admit("192.0.2.1".parse().unwrap()));
        }
        assert_eq!(
            app.oneshot(req(
                "/api/v1/personal/invitations/redeem",
                &g.origin,
                json!({"token":t,"password":"chosen42"})
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    #[tokio::test]
    async fn status_and_invalid_capabilities_do_not_contend_for_the_initializer_permit() {
        let (_d, g, t) = fixture();
        let permit = g.invitation_inflight.clone().try_acquire_owned().unwrap();
        let app = router(g.clone());
        assert_eq!(
            app.clone()
                .oneshot(req(
                    "/api/v1/personal/invitations/status",
                    &g.origin,
                    json!({"token":t})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            app.clone()
                .oneshot(req(
                    "/api/v1/personal/invitations/status",
                    &g.origin,
                    json!({"token":"invalid"})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            app.oneshot(req(
                "/api/v1/personal/invitations/redeem",
                &g.origin,
                json!({"token":t,"password":"chosen42"})
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::CONFLICT
        );
        drop(permit);
    }
}
