//! Local installer bootstrap: authenticate the owner through the installed Unix
//! service; never change a private role, accept a browser proof, or retain tokens.
use axum::{
    body::Body,
    http::{HeaderMap, Method, StatusCode, header},
};
use clap::{Args, Subcommand};
use moosuri_core::SessionView;
use moosuri_store::runtime_registry::{self, Allocation, admin};
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroizing;
#[derive(Args)]
pub struct Options {
    #[arg(long)]
    registry: PathBuf,
    #[command(subcommand)]
    command: Operation,
}
#[derive(Subcommand)]
enum Operation {
    /// Grant the one-time first installation admin to a ready, password-changed member.
    Bootstrap {
        #[arg(long)]
        request_id: Uuid,
        #[arg(long)]
        email: String,
        #[arg(long)]
        ipc_root: PathBuf,
        /// Expected dedicated runtime UID, from the installer allocation.
        #[arg(long)]
        member_uid: u32,
        /// Read one private password line instead of prompting interactively.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Show safe installation role metadata for the local operator.
    Status {
        #[arg(long)]
        email: String,
    },
}
pub async fn run(o: &Options) -> Result<(), String> {
    let member = match &o.command {
        Operation::Status { email } => {
            let a = runtime_registry::lookup(&o.registry, email.clone())
                .map_err(|_| "allocation_unavailable")?;
            admin::status(
                &o.registry,
                Uuid::parse_str(&a.account_id).map_err(|_| "allocation_invalid")?,
            )
            .map_err(|_| "role_unavailable")?
        }
        Operation::Bootstrap {
            request_id,
            email,
            ipc_root,
            member_uid,
            password_stdin,
        } => {
            let password = if *password_stdin {
                use std::io::Read;
                let mut input = Zeroizing::new(String::new());
                std::io::stdin()
                    .lock()
                    .take(259)
                    .read_to_string(&mut input)
                    .map_err(|_| "password_unavailable")?;
                let line = input
                    .strip_suffix('\n')
                    .filter(|s| !s.contains(['\r', '\n']) && s.len() <= 256)
                    .ok_or("expected_one_password_line")?;
                Zeroizing::new(line.to_owned())
            } else {
                Zeroizing::new(
                    rpassword::prompt_password("Existing member password / 기존 회원 비밀번호: ")
                        .map_err(|_| "password_unavailable")?,
                )
            };
            bootstrap(
                &o.registry,
                *request_id,
                email,
                ipc_root,
                *member_uid,
                password,
            )
            .await?
        }
    };
    println!(
        "{}",
        serde_json::to_string(&member).map_err(|_| "role_invalid")?
    );
    Ok(())
}
fn socket(root: &Path, a: &Allocation, uid: u32) -> Result<PathBuf, String> {
    if !root.is_absolute() || std::fs::canonicalize(root).ok().as_deref() != Some(root) {
        return Err("invalid_ipc_root".into());
    }
    let dir = root.join(&a.account_id);
    let path = dir.join("personal.sock");
    for p in [root, &dir] {
        let m = std::fs::symlink_metadata(p).map_err(|_| "invalid_ipc_directory")?;
        if !m.is_dir()
            || m.mode() & 0o022 != 0
            || (m.uid() != 0 && m.uid() != unsafe { libc::geteuid() } && m.uid() != uid)
            || std::fs::canonicalize(p).ok().as_deref() != Some(p)
        {
            return Err("invalid_ipc_directory".into());
        }
    }
    let m = std::fs::symlink_metadata(&path).map_err(|_| "invalid_member_socket")?;
    if !m.file_type().is_socket() || m.uid() != uid || m.mode() & 0o077 != 0 || m.nlink() != 1 {
        return Err("invalid_member_socket".into());
    }
    Ok(path)
}
async fn ipc(
    path: &Path,
    uid: u32,
    a: &Allocation,
    method: Method,
    route: &str,
    body: Vec<u8>,
    session: Option<&str>,
) -> Result<(StatusCode, HeaderMap, Vec<u8>), String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|_| "member_unavailable")?;
        if stream
            .peer_cred()
            .map_err(|_| "member_identity_mismatch")?
            .uid()
            != uid
        {
            return Err("member_identity_mismatch".into());
        }
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
                .await
                .map_err(|_| "member_unavailable")?;
        tokio::spawn(connection);
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(route)
            .header(header::HOST, "personal.internal")
            .header(header::ORIGIN, "https://personal.internal")
            .header("x-moosuri-account", &a.account_id)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = session {
            builder = builder.header(header::COOKIE, format!("moosuri_session={token}"));
        }
        let request = builder
            .body(Body::from(body))
            .map_err(|_| "invalid_request")?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| "member_unavailable")?;
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(Body::new(body), 16384)
            .await
            .map_err(|_| "invalid_member_response")?;
        Ok((parts.status, parts.headers, bytes.to_vec()))
    })
    .await
    .map_err(|_| "member_unavailable".to_owned())?
}
async fn bootstrap(
    registry: &Path,
    request: Uuid,
    email: &str,
    ipc_root: &Path,
    uid: u32,
    password: Zeroizing<String>,
) -> Result<admin::Member, String> {
    bootstrap_inner(registry, request, email, ipc_root, uid, password, false).await
}
/// Clean-install completion: the operator's own selected secret was the initial
/// secret, so it is adopted through the ordinary member password route when the
/// member is still restricted. Safe to retry; an already changed member only logs in.
pub(crate) async fn bootstrap_adopting_password(
    registry: &Path,
    request: Uuid,
    email: &str,
    ipc_root: &Path,
    uid: u32,
    password: Zeroizing<String>,
) -> Result<admin::Member, String> {
    bootstrap_inner(registry, request, email, ipc_root, uid, password, true).await
}
fn session_cookie(headers: &HeaderMap) -> Result<Zeroizing<String>, String> {
    let cookies: Vec<_> = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .filter_map(|s| s.split(';').next())
        .filter_map(|s| s.strip_prefix("moosuri_session="))
        .collect();
    if cookies.len() != 1
        || cookies[0].len() != 64
        || !cookies[0].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid_member_session".into());
    }
    Ok(Zeroizing::new(cookies[0].to_owned()))
}
async fn bootstrap_inner(
    registry: &Path,
    request: Uuid,
    email: &str,
    ipc_root: &Path,
    uid: u32,
    password: Zeroizing<String>,
    adopt: bool,
) -> Result<admin::Member, String> {
    let a = runtime_registry::lookup_ready(registry, email.to_owned())
        .map_err(|_| "allocation_not_ready")?;
    let path = socket(ipc_root, &a, uid)?;
    let (status, headers, _) = ipc(
        &path,
        uid,
        &a,
        Method::GET,
        "/health/ready",
        Vec::new(),
        None,
    )
    .await?;
    for (key, expected) in [
        ("x-moosuri-account", a.account_id.clone()),
        ("x-moosuri-tenant", a.tenant_id.clone()),
        ("x-moosuri-volume", a.volume_id.clone()),
        ("x-moosuri-generation", a.generation.to_string()),
    ] {
        if status != StatusCode::NO_CONTENT
            || headers.get(key).and_then(|h| h.to_str().ok()) != Some(expected.as_str())
        {
            return Err("member_identity_mismatch".into());
        }
    }
    #[derive(serde::Serialize)]
    struct Login<'a> {
        email: &'a str,
        password: &'a str,
    }
    let body = Zeroizing::new(
        serde_json::to_vec(&Login {
            email,
            password: password.as_str(),
        })
        .map_err(|_| "invalid_request")?,
    );
    let (status, headers, raw) = ipc(
        &path,
        uid,
        &a,
        Method::POST,
        "/api/v1/auth/login",
        body.to_vec(),
        None,
    )
    .await?;
    if status != StatusCode::OK {
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err("member_login_rate_limited".into());
        }
        if adopt && !matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err("member_login_unavailable".into());
        }
        return Err("member_login_required".into());
    }
    let mut token = session_cookie(&headers)?;
    let login_view: Result<SessionView, String> =
        serde_json::from_slice(&raw).map_err(|_| "invalid_member_response".to_owned());
    let mut adopted = Ok(());
    if adopt
        && login_view
            .as_ref()
            .is_ok_and(|v| v.password_change_required || v.user.must_change_password)
    {
        let body = Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({"password": password.as_str()}))
                .map_err(|_| "invalid_request")?,
        );
        adopted = async {
            let (status, headers, _) = ipc(
                &path,
                uid,
                &a,
                Method::POST,
                "/api/v1/auth/password",
                body.to_vec(),
                Some(&token),
            )
            .await?;
            if status != StatusCode::OK {
                return Err("member_password_adoption_rejected".to_owned());
            }
            // The ordinary route revokes every session and issues the replacement.
            token = session_cookie(&headers)?;
            Ok(())
        }
        .await;
    }
    let result = async {
        adopted?;
        login_view?;
        let (status, _, raw) = ipc(
            &path,
            uid,
            &a,
            Method::GET,
            "/api/v1/auth/session",
            Vec::new(),
            Some(&token),
        )
        .await?;
        if status != StatusCode::OK {
            return Err("member_session_required".into());
        }
        let view: SessionView =
            serde_json::from_slice(&raw).map_err(|_| "invalid_member_response")?;
        admin::bootstrap(registry, request, &a, &view)
            .map_err(|_| "admin_bootstrap_rejected".into())
    }
    .await;
    // Retire this operator-created session even when eligibility/bootstrap fails.
    let _ = ipc(
        &path,
        uid,
        &a,
        Method::POST,
        "/api/v1/auth/logout",
        b"{}".to_vec(),
        Some(&token),
    )
    .await;
    result
}
#[cfg(test)]
mod tests;
