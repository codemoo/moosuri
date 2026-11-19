//! The installation administrator registers one client. Member consent stays
//! in each member's private runtime.
use super::*;
use axum::extract::rejection::BytesRejection;
use moosuri_connectors::gmail::{GoogleClientFileError, OAuthClient};
use moosuri_store::runtime_registry::{
    GOOGLE_WEB_CLIENT_FILE, GOOGLE_WEB_CLIENT_PENDING_FILE, admin as control,
};
use std::{
    ffi::CString,
    io,
    os::fd::{AsRawFd, FromRawFd},
};

const MAX_CLIENT_FILE: usize = 16 * 1024;
// Each raw byte may use up to six bytes when escaped in the JSON envelope.
const MAX_UPLOAD: usize = 6 * MAX_CLIENT_FILE + 256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    web_client_json: Zeroizing<String>,
}

#[derive(Serialize)]
struct SetupStatus {
    configured: bool,
    redirect_uri: String,
}

enum SetupError {
    InvalidRequest,
    InvalidClient,
    RedirectMismatch,
    AlreadyConfigured,
    Storage,
}

impl SetupError {
    fn response(self) -> Response {
        let (status, code) = match self {
            Self::InvalidRequest => return failure(StatusCode::BAD_REQUEST),
            Self::InvalidClient => (StatusCode::BAD_REQUEST, "invalid_google_client_file"),
            Self::RedirectMismatch => (StatusCode::BAD_REQUEST, "google_redirect_uri_mismatch"),
            Self::AlreadyConfigured => (StatusCode::CONFLICT, "google_setup_already_configured"),
            Self::Storage => (
                StatusCode::SERVICE_UNAVAILABLE,
                "google_setup_storage_unavailable",
            ),
        };
        response(
            status,
            serde_json::json!({"code": code}).to_string().into_bytes(),
        )
    }
}

pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(
            "/api/v1/personal/admin/google-setup",
            get(endpoint).post(endpoint),
        )
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
}

async fn endpoint(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<axum::body::Bytes, BytesRejection>,
) -> Response {
    let result = async {
        g.headers(&headers, &method)?;
        if uri.query().is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let (a, view) = admin::authenticated_session(&g, &headers).await?;
        let redirect_uri = format!("{}{}", g.origin, google::CALLBACK);
        let root = g.registry.clone();
        tokio::task::spawn_blocking(move || {
            let pin = control::checked_session(&root, &a, &view)
                .and_then(|proof| proof.at_epoch(a.epoch))
                .map_err(admin::error)?;
            control::with_google_setup(&root, &pin, |directory| {
                let Ok(body) = body else {
                    return SetupError::InvalidClient.response();
                };
                let result = if method == Method::POST {
                    if one(&headers, "content-type").and_then(|s| s.split(';').next())
                        != Some("application/json")
                    {
                        return SetupError::InvalidRequest.response();
                    }
                    save(directory, &redirect_uri, &body).map(|()| true)
                } else if method == Method::GET && body.is_empty() {
                    stored_client(directory, &redirect_uri).map(|client| client.is_some())
                } else {
                    return SetupError::InvalidRequest.response();
                };
                let configured = match result {
                    Ok(configured) => configured,
                    Err(error) => return error.response(),
                };
                let body = serde_json::to_vec(&SetupStatus {
                    configured,
                    redirect_uri,
                })
                .expect("status strings serialize to JSON");
                response(StatusCode::OK, body)
            })
            .map_err(admin::error)
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    }
    .await;
    result.unwrap_or_else(failure)
}

fn open_private(directory: &File, name: &str, flags: libc::c_int) -> io::Result<File> {
    let name = CString::new(name).expect("installation file name");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let stat = file.metadata()?;
    if !stat.is_file()
        || stat.uid() != unsafe { libc::geteuid() }
        || stat.mode() & 0o7777 != 0o600
        || stat.nlink() != 1
        || stat.len() > MAX_CLIENT_FILE as u64
    {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(file)
}

fn stored_client(directory: &File, redirect_uri: &str) -> Result<Option<OAuthClient>, SetupError> {
    let file = match open_private(directory, GOOGLE_WEB_CLIENT_FILE, libc::O_RDONLY) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SetupError::Storage),
    };
    let mut bytes = Zeroizing::new(String::new());
    file.take((MAX_CLIENT_FILE + 1) as u64)
        .read_to_string(&mut bytes)
        .map_err(|_| SetupError::Storage)?;
    OAuthClient::from_web_client_json(&bytes, redirect_uri)
        .map(Some)
        .map_err(|_| SetupError::Storage)
}

fn save(directory: &File, redirect_uri: &str, body: &[u8]) -> Result<(), SetupError> {
    let input: Upload = serde_json::from_slice(body).map_err(|_| SetupError::InvalidRequest)?;
    let client = OAuthClient::from_web_client_json(&input.web_client_json, redirect_uri).map_err(
        |error| match error {
            GoogleClientFileError::Invalid => SetupError::InvalidClient,
            GoogleClientFileError::RedirectMismatch => SetupError::RedirectMismatch,
        },
    )?;
    if let Some(existing) = stored_client(directory, redirect_uri)? {
        if existing.client_id == client.client_id
            && existing.client_secret.expose() == client.client_secret.expose()
        {
            return directory.sync_all().map_err(|_| SetupError::Storage);
        }
        return Err(SetupError::AlreadyConfigured);
    }

    #[derive(Serialize)]
    struct Client<'a> {
        client_id: &'a str,
        client_secret: &'a str,
        redirect_uris: [&'a str; 1],
    }
    #[derive(Serialize)]
    struct ClientFile<'a> {
        web: Client<'a>,
    }
    let bytes = Zeroizing::new(
        serde_json::to_vec(&ClientFile {
            web: Client {
                client_id: &client.client_id,
                client_secret: client.client_secret.expose(),
                redirect_uris: [redirect_uri],
            },
        })
        .map_err(|_| SetupError::Storage)?,
    );
    write_client(directory, &bytes).map_err(|_| SetupError::Storage)
}

fn write_client(directory: &File, bytes: &[u8]) -> io::Result<()> {
    let pending = CString::new(GOOGLE_WEB_CLIENT_PENDING_FILE).expect("installation file name");
    let destination = CString::new(GOOGLE_WEB_CLIENT_FILE).expect("installation file name");
    let fd = directory.as_raw_fd();
    // The registry lock excludes other writers. A previous interrupted upload
    // may leave a private staging file, which has no authority until renamed.
    match open_private(directory, GOOGLE_WEB_CLIENT_PENDING_FILE, libc::O_RDONLY) {
        Ok(_) => {
            if unsafe { libc::unlinkat(fd, pending.as_ptr(), 0) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let mut file = open_private(
        directory,
        GOOGLE_WEB_CLIENT_PENDING_FILE,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if unsafe { libc::renameat(fd, pending.as_ptr(), fd, destination.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    directory.sync_all()
}
