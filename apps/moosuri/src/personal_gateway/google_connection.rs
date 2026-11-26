//! One live Kiri attempt per enrolled member. A restart discards all pending work.
//! The gateway transports only vault-sealed credentials. The private key stays in Kiri.
use super::*;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::oneshot,
    time::timeout,
};

const SOCKET: &str = "/run/moosuri-google-connection/owner.sock";
const UNAVAILABLE: StatusCode = StatusCode::SERVICE_UNAVAILABLE;
const GONE: StatusCode = StatusCode::GONE;
pub(super) type Jobs = Arc<Mutex<Option<Job>>>;
pub(super) struct Job {
    id: Uuid,
    challenge: Uuid,
    _stop: oneshot::Sender<()>,
    allocation: Allocation,
    epoch: i64,
    session: String,
    expires: Instant,
    state: &'static str,
    delivery: Option<oneshot::Sender<Value>>,
    flow: Option<String>,
    service: Option<&'static str>,
    handle: Option<String>,
}

pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route("/api/v1/personal/google/kiri", post(handle))
        .layer(DefaultBodyLimit::max(12 * 1024))
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Start {
        job: Uuid,
        service: String,
        read: Option<moosuri_store::MemberGoogleReadIntent>,
    },
    Status {
        job: Uuid,
    },
    Deliver {
        job: Uuid,
        envelope: Envelope,
    },
    Cancel {
        job: Uuid,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    key: String,
    iv: String,
    body: String,
}

async fn authorize(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
) -> Result<String, StatusCode> {
    if !g.vault.as_ref().is_some_and(|v| v.enrolled(a)) {
        return Err(StatusCode::FORBIDDEN);
    }
    let (status, bytes, _) = g
        .proxy(a, Method::GET, "/api/v1/auth/session", None, Some(session))
        .await?;
    if status != StatusCode::OK {
        return Err(status);
    }
    let view = member_session_view(&bytes, a)?;
    if view.password_change_required || view.user.must_change_password {
        return Err(StatusCode::FORBIDDEN);
    }
    recheck(g, a).await?;
    Ok(view.user.email)
}
fn terminal(state: &str) -> bool {
    matches!(
        state,
        "complete" | "denied" | "blocked" | "needs_human" | "unavailable" | "setup_required"
    )
}
impl Job {
    fn owns(&self, id: Uuid, a: &SessionAllocation, session: &str) -> bool {
        self.id == id
            && self.allocation == a.allocation
            && self.epoch == a.epoch
            && self.session == hash(session)
            && self.expires > Instant::now()
    }
    fn receipt(&self) -> Value {
        json!({"job": self.id, "challenge":self.challenge, "state": self.state, "flow": self.flow, "handle": self.handle})
    }
}

async fn handle(
    State(g): State<Gateway>,
    headers: HeaderMap,
    uri: Uri,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        g.headers(&headers, &Method::POST)?;
        if uri.query().is_some()
            || one(&headers, "content-type").and_then(|v| v.split(';').next())
                != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g.inflight.try_acquire().map_err(|_| UNAVAILABLE)?;
        let input: Input = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
        let (a, session) = authenticated_allocation(&g, &headers, true).await?;
        let email = authorize(&g, &a, &session).await?;
        let mut jobs = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
        let request_id = match &input {
            Input::Start { job, .. }
            | Input::Status { job }
            | Input::Deliver { job, .. }
            | Input::Cancel { job } => *job,
        };
        if request_id.is_nil() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if let Input::Start {
            job: id,
            service,
            read,
        } = input
        {
            let service = match service.as_str() {
                "gmail" => None,
                "docs" => Some("docs"),
                "drive" => Some("drive"),
                "sheets" => Some("sheets"),
                "slides" => Some("slides"),
                _ => return Err(StatusCode::BAD_REQUEST),
            };
            if service.is_some() && read.is_some() {
                return Err(StatusCode::BAD_REQUEST);
            }
            if let Some(j) = jobs.as_ref().filter(|j| j.expires > Instant::now()) {
                if j.owns(id, &a, &session) {
                    return Ok(j.receipt());
                }
                if !terminal(j.state) {
                    return Err(StatusCode::CONFLICT);
                }
            }
            let (sender, rx) = oneshot::channel();
            let (stop, cancelled) = oneshot::channel();
            let challenge = Uuid::new_v4();
            let j = Job {
                id,
                challenge,
                _stop: stop,
                allocation: a.allocation.clone(),
                epoch: a.epoch,
                session: hash(&session),
                expires: Instant::now() + Duration::from_secs(730),
                state: "opening",
                delivery: Some(sender),
                flow: None,
                service,
                handle: None,
            };
            let receipt = j.receipt();
            *jobs = Some(j);
            let gateway = g.clone();
            tokio::spawn(async move {
                let state = tokio::select! {
                    _ = cancelled => return,
                    outcome = timeout(
                    Duration::from_secs(715),
                    run(&gateway, &a, &session, id, challenge, &email, service, read, rx),
                )
                => match outcome { Ok(Ok(state))=>state, _=>"unavailable" }
                };
                let _ = change(&gateway, id, |j| {
                    if !terminal(j.state) {
                        j.state = state
                    }
                    j.delivery = None;
                });
            });
            return Ok(receipt);
        }
        let j = jobs
            .as_mut()
            .filter(|j| j.owns(request_id, &a, &session))
            .ok_or(GONE)?;
        match input {
            Input::Deliver { envelope, .. } => {
                use base64::engine::general_purpose::STANDARD;
                if STANDARD
                    .decode(&envelope.key)
                    .ok()
                    .is_none_or(|v| v.len() != 384)
                    || STANDARD
                        .decode(&envelope.iv)
                        .ok()
                        .is_none_or(|v| v.len() != 12)
                    || STANDARD
                        .decode(&envelope.body)
                        .ok()
                        .is_none_or(|v| !(17..=4112).contains(&v.len()))
                {
                    return Err(StatusCode::BAD_REQUEST);
                }
                if j.state != "vault" {
                    return Err(StatusCode::CONFLICT);
                }
                j.delivery
                    .take()
                    .ok_or(GONE)?
                    .send(json!(envelope))
                    .map_err(|_| GONE)?;
                j.state = "signing_in";
            }
            Input::Cancel { .. } => {
                *jobs = None;
                return Ok(json!({"state":"cancelled"}));
            }
            _ => {}
        }
        Ok(j.receipt())
    }
    .await;
    match result {
        Ok(value) => response(StatusCode::OK, value.to_string().into_bytes()),
        Err(status) => failure(status),
    }
}
fn change<T>(g: &Gateway, id: Uuid, edit: impl FnOnce(&mut Job) -> T) -> Result<T, StatusCode> {
    let mut guard = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
    Ok(edit(
        guard
            .as_mut()
            .filter(|j| j.id == id && j.expires > Instant::now())
            .ok_or(GONE)?,
    ))
}
async fn send(peer: &mut BufReader<UnixStream>, v: Value) -> Result<(), StatusCode> {
    let mut line = v.to_string().into_bytes();
    line.push(b'\n');
    peer.get_mut()
        .write_all(&line)
        .await
        .map_err(|_| UNAVAILABLE)
}
async fn phase(peer: &mut BufReader<UnixStream>) -> Result<String, StatusCode> {
    let mut line = Vec::new();
    (&mut *peer)
        .take(257)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|_| UNAVAILABLE)?;
    if line.len() > 256 || line.pop() != Some(b'\n') {
        return Err(UNAVAILABLE);
    }
    let reply: Value = serde_json::from_slice(&line).map_err(|_| UNAVAILABLE)?;
    reply["status"]
        .as_str()
        .map(str::to_owned)
        .ok_or(UNAVAILABLE)
}
fn stopped(s: &str) -> &'static str {
    match s {
        "blocked" => "blocked",
        "needs_human" => "needs_human",
        _ => "unavailable",
    }
}
async fn run(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
    challenge: Uuid,
    email: &str,
    service: Option<&'static str>,
    read: Option<moosuri_store::MemberGoogleReadIntent>,
    rx: oneshot::Receiver<Value>,
) -> Result<&'static str, StatusCode> {
    let stream = UnixStream::connect(SOCKET).await.map_err(|_| UNAVAILABLE)?;
    if stream.peer_cred().map_err(|_| UNAVAILABLE)?.uid() != 0 {
        return Err(UNAVAILABLE);
    }
    let mut peer = BufReader::new(stream);
    send(
        &mut peer,
        json!({"job":challenge,"account":a.account_id,"email":email,"service":service.unwrap_or("gmail")}),
    )
    .await?;
    let state = phase(&mut peer).await?;
    if state != "vault" {
        return Ok(stopped(&state));
    }
    change(g, id, |j| j.state = "vault")?;
    let packet = timeout(Duration::from_secs(560), rx)
        .await
        .map_err(|_| GONE)?
        .map_err(|_| GONE)?;
    authorize(g, a, session).await?;
    change(g, id, |_| ())?;
    send(&mut peer, packet).await?;
    let state = phase(&mut peer).await?;
    if state != "signed_in" {
        return Ok(stopped(&state));
    }
    authorize(g, a, session).await?;
    change(g, id, |j| j.state = "signed_in")?;
    let path = match service {
        Some(name) => format!("/api/v1/personal/google/{name}/start"),
        None => "/api/v1/personal/google/start".into(),
    };
    let mut payload = json!({"match_member_account":true});
    if let Some(intent) = read {
        payload["read"] = json!(intent)
    }
    let (status, bytes, _) = g
        .proxy(
            a,
            Method::POST,
            &path,
            Some(payload.to_string().as_bytes()),
            Some(session),
        )
        .await?;
    if status == StatusCode::NOT_FOUND {
        return Ok("setup_required");
    }
    if !status.is_success() {
        return Err(status);
    }
    let checked = crate::personal_google::public_response(&path, &Method::POST, status, &bytes)?;
    let started = google::consent_url(g, a, &checked, service)?;
    let bytes = axum::body::to_bytes(started.into_body(), 8192)
        .await
        .map_err(|_| UNAVAILABLE)?;
    let start: Value = serde_json::from_slice(&bytes).map_err(|_| UNAVAILABLE)?;
    let mut url = url::Url::parse(start["authorization_url"].as_str().ok_or(UNAVAILABLE)?)
        .map_err(|_| UNAVAILABLE)?;
    let fields: Vec<_> = url
        .query_pairs()
        .into_owned()
        .filter(|(k, _)| k != "login_hint")
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(fields)
        .append_pair("login_hint", email);
    let flow = start["flow_id"].as_str().ok_or(UNAVAILABLE)?;
    authorize(g, a, session).await?;
    change(g, id, |j| {
        j.flow = Some(flow.into());
        j.state = "consent"
    })?;
    send(&mut peer, json!({"url":url.as_str()})).await?;
    let state = phase(&mut peer).await?;
    Ok(if state == "returned" {
        "unavailable" // Only the matching provider callback can finish this job.
    } else {
        stopped(&state)
    })
}
pub(super) fn capture(
    g: &Gateway,
    a: &SessionAllocation,
    service: Option<&str>,
    flow: &str,
    handle: Option<&str>,
) -> bool {
    let Ok(mut jobs) = g.google_jobs.lock() else {
        return false;
    };
    let Some(j) = jobs.as_mut().filter(|j| {
        j.state == "consent"
            && j.expires > Instant::now()
            && j.flow.as_deref() == Some(flow)
            && j.service == service
            && j.epoch == a.epoch
            && j.allocation == a.allocation
    }) else {
        return false;
    };
    j.handle = handle.map(str::to_owned);
    j.state = if handle.is_some() {
        "complete"
    } else {
        "denied"
    };
    true
}
