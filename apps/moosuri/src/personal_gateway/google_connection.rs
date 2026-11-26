//! One live Kiri attempt per enrolled member. A restart discards all pending work.
//! The gateway transports only vault-sealed credentials. The private key stays in Kiri.
use super::*;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::oneshot,
    time::{MissedTickBehavior, interval, timeout},
};

const SOCKET: &str = "/run/moosuri-google-connection/owner.sock";
const UNAVAILABLE: StatusCode = StatusCode::SERVICE_UNAVAILABLE;
const GONE: StatusCode = StatusCode::GONE;
const VAULT_JOB_LIFETIME: Duration = Duration::from_secs(730);
const KIRI_RUN_LIMIT: Duration = Duration::from_secs(420);
const KIRI_JOB_LIFETIME: Duration = Duration::from_secs(425);
const TERMINAL_RECEIPT_LIFETIME: Duration = Duration::from_secs(120);
const AUTH_RECHECK_INTERVAL: Duration = Duration::from_secs(2);
const GUEST_TEARDOWN_GRACE: Duration = Duration::from_secs(180);
const MAX_MEMBER_JOBS: usize = 16;
const MAX_MEMBER_VAULT_JOBS: usize = 8;
const PROTOCOL: &str = "moosuri-google-connection-v3";
pub(super) type Jobs = Arc<Mutex<Vec<Job>>>;
pub(super) struct Job {
    id: Uuid,
    challenge: Uuid,
    stop: Option<oneshot::Sender<()>>,
    allocation: Allocation,
    epoch: i64,
    session: String,
    email: String,
    google_email: Option<String>,
    expires: Instant,
    state: &'static str,
    read: Option<moosuri_store::MemberGoogleReadIntent>,
    flow: Option<String>,
    service: Option<&'static str>,
    handle: Option<String>,
    delivery: Option<String>,
    guest_active: bool,
    guest_deadline: Option<Instant>,
}

fn run_window(now: Instant) -> (Instant, Duration) {
    (now + KIRI_JOB_LIFETIME, KIRI_RUN_LIMIT)
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
        google_email: String,
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
async fn reauthorize(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    expected_email: &str,
) -> Result<(), StatusCode> {
    if authorize(g, a, session).await? != expected_email {
        return Err(GONE);
    }
    Ok(())
}
fn deliverable(state: &str) -> bool {
    state == "vault"
}
fn valid_google_email(email: &str) -> bool {
    email.len() <= 254
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
}
fn terminal(state: &str) -> bool {
    matches!(
        state,
        "complete"
            | "denied"
            | "blocked"
            | "needs_human"
            | "unavailable"
            | "setup_required"
            | "cancelled"
    )
}
fn member_has_live_job(
    jobs: &[Job],
    a: &SessionAllocation,
    email: &str,
    except: Option<usize>,
) -> bool {
    jobs.iter().enumerate().any(|(index, j)| {
        Some(index) != except
            && j.same_identity(a, email)
            && (j.guest_active || (!terminal(j.state) && j.state != "vault"))
    })
}
fn retire_other_vault_jobs(
    jobs: &mut Vec<Job>,
    winner: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) {
    jobs.retain(|j| j.id == winner || j.state != "vault" || !j.owned_by(a, session, email));
}
fn reserve_member_vault(
    jobs: &mut Vec<Job>,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) -> Result<(), StatusCode> {
    // A valid guest from any session/epoch wins until its run task has observed
    // teardown. A second session cannot infer that the first session is invalid.
    if member_has_live_job(jobs, a, email, None) {
        return Err(StatusCode::CONFLICT);
    }
    let current: Vec<_> = jobs.iter().filter(|j| j.same_member(a, email)).collect();
    if current
        .iter()
        .any(|j| !terminal(j.state) && j.state != "vault")
    {
        return Err(StatusCode::CONFLICT);
    }
    let replace = current.iter().filter(|j| j.state == "vault").count() >= MAX_MEMBER_VAULT_JOBS;
    if current.len() + 1 - usize::from(replace) > MAX_MEMBER_JOBS {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    // Valid sessions cannot delete one another's reservation or completed result.
    // At the member-wide vault limit, a session may replace only its own oldest
    // still-vault reservation.
    if !make_room_for_member_vault(jobs, a, session, email) {
        return Err(UNAVAILABLE);
    }
    Ok(())
}
fn make_room_for_member_vault(
    jobs: &mut Vec<Job>,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) -> bool {
    let member_jobs = jobs.iter().filter(|j| j.same_member(a, email)).count();
    let replace = jobs
        .iter()
        .filter(|j| j.state == "vault" && j.same_member(a, email))
        .count()
        >= MAX_MEMBER_VAULT_JOBS;
    if member_jobs + 1 - usize::from(replace) > MAX_MEMBER_JOBS {
        return false;
    }
    if replace {
        let Some(index) = jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| j.state == "vault" && j.owned_by(a, session, email))
            .min_by_key(|(_, j)| j.expires)
            .map(|(index, _)| index)
        else {
            return false;
        };
        jobs.remove(index);
    }
    true
}
fn same_read_intent(
    left: &Option<moosuri_store::MemberGoogleReadIntent>,
    right: &Option<moosuri_store::MemberGoogleReadIntent>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.thread_id == right.thread_id && left.request_id == right.request_id
        }
        _ => false,
    }
}
fn same_start(
    job: &Job,
    service: Option<&'static str>,
    read: &Option<moosuri_store::MemberGoogleReadIntent>,
) -> bool {
    job.service == service && same_read_intent(&job.read, read)
}
fn guest_teardown_confirmed(state: &str) -> bool {
    matches!(
        state,
        "blocked" | "needs_human" | "unavailable" | "returned"
    )
}
fn delivery_fingerprint(envelope: &Envelope) -> String {
    hash(&format!(
        "{}\n{}\n{}",
        envelope.key, envelope.iv, envelope.body
    ))
}
fn job_id_conflicts(jobs: &[Job], id: Uuid, a: &SessionAllocation, email: &str) -> bool {
    jobs.iter().any(|j| j.id == id && j.same_identity(a, email))
}
fn finish_receipt(j: &mut Job, handle: Option<&str>, now: Instant) {
    j.handle = handle.map(str::to_owned);
    j.state = if handle.is_some() {
        "complete"
    } else {
        "denied"
    };
    // Preserve a provider result even when it arrives at the end of the Kiri
    // window, so a throttled/background member tab can still collect it.
    j.expires = j.expires.max(now + TERMINAL_RECEIPT_LIFETIME);
}
fn record_run_state(j: &mut Job, state: &'static str, now: Instant) {
    if !terminal(j.state) {
        j.state = state;
    }
    if terminal(j.state) {
        j.expires = j.expires.max(now + TERMINAL_RECEIPT_LIFETIME);
    }
}
fn finish_run(j: &mut Job, state: &'static str, teardown_confirmed: bool, now: Instant) {
    record_run_state(j, state, now);
    j.stop = None;
    j.guest_active = !teardown_confirmed;
    j.guest_deadline = (!teardown_confirmed).then_some(now + GUEST_TEARDOWN_GRACE);
    if let Some(deadline) = j.guest_deadline {
        // The receipt owns the live-guest fence; it must survive its full grace.
        j.expires = j.expires.max(deadline);
    }
}
fn expire_active_guests(jobs: &mut [Job], now: Instant) {
    for j in jobs
        .iter_mut()
        .filter(|j| j.guest_active && j.guest_deadline.is_some_and(|deadline| deadline <= now))
    {
        j.guest_active = false;
        j.guest_deadline = None;
        j.stop.take();
        record_run_state(j, "unavailable", now);
    }
}
fn cancel_job(jobs: &mut Vec<Job>, position: usize) -> Value {
    if terminal(jobs[position].state) {
        return jobs[position].receipt();
    }
    if jobs[position].guest_active {
        let j = &mut jobs[position];
        j.state = "cancelled";
        j.expires = j.expires.max(Instant::now() + TERMINAL_RECEIPT_LIFETIME);
        j.stop.take();
    } else {
        jobs.remove(position);
    }
    json!({"state":"cancelled"})
}
fn confirm_opening(
    jobs: &mut Vec<Job>,
    id: Uuid,
    challenge: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) -> Result<(), StatusCode> {
    let winner = jobs
        .iter()
        .position(|j| {
            j.id == id
                && j.challenge == challenge
                && j.state == "validating"
                && j.owned_by(a, session, email)
        })
        .ok_or(GONE)?;
    jobs[winner].state = "opening";
    Ok(())
}
fn confirm_verification(
    jobs: &mut [Job],
    id: Uuid,
    challenge: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) -> Result<(), StatusCode> {
    let job = jobs
        .iter_mut()
        .find(|j| {
            j.id == id
                && j.challenge == challenge
                && j.state == "opening"
                && j.owned_by(a, session, email)
        })
        .ok_or(GONE)?;
    job.state = "verifying";
    Ok(())
}
fn confirm_signed_in(
    jobs: &mut Vec<Job>,
    id: Uuid,
    challenge: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) -> Result<(), StatusCode> {
    let winner = jobs
        .iter()
        .position(|j| {
            j.id == id
                && j.challenge == challenge
                && matches!(j.state, "opening" | "verifying")
                && j.owned_by(a, session, email)
        })
        .ok_or(GONE)?;
    jobs[winner].state = "signed_in";
    retire_other_vault_jobs(jobs, id, a, session, email);
    Ok(())
}
fn confirm_consent(
    jobs: &mut [Job],
    id: Uuid,
    challenge: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
    flow: &str,
) -> Result<(), StatusCode> {
    let j = jobs
        .iter_mut()
        .find(|j| {
            j.id == id
                && j.challenge == challenge
                && j.state == "signed_in"
                && j.owned_by(a, session, email)
        })
        .ok_or(GONE)?;
    j.flow = Some(flow.into());
    j.state = "consent";
    Ok(())
}
impl Job {
    fn same_identity(&self, a: &SessionAllocation, email: &str) -> bool {
        self.allocation == a.allocation && self.email == email
    }
    fn same_member(&self, a: &SessionAllocation, email: &str) -> bool {
        self.same_identity(a, email) && self.epoch == a.epoch && self.expires > Instant::now()
    }
    fn owned_by(&self, a: &SessionAllocation, session: &str, email: &str) -> bool {
        self.same_member(a, email) && self.session == hash(session)
    }
    fn owns(&self, id: Uuid, a: &SessionAllocation, session: &str, email: &str) -> bool {
        self.id == id && self.owned_by(a, session, email)
    }
    fn receipt(&self) -> Value {
        json!({"job": self.id, "challenge":self.challenge, "state": self.state, "flow": self.flow, "handle": self.handle, "email":self.google_email})
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
        let now = Instant::now();
        expire_active_guests(&mut jobs, now);
        jobs.retain(|j| j.expires > now);
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
            if let Some(j) = jobs.iter().find(|j| j.owns(id, &a, &session, &email)) {
                if !same_start(j, service, &read) {
                    return Err(StatusCode::CONFLICT);
                }
                return Ok(j.receipt());
            }
            if job_id_conflicts(&jobs, id, &a, &email) {
                return Err(StatusCode::CONFLICT);
            }
            // A rejected Start must not destroy another tab's pending reservation or
            // an unpolled terminal receipt. A successful Start supersedes stale
            // sessions/epochs and only then makes room for this reservation.
            reserve_member_vault(&mut jobs, &a, &session, &email)?;
            let challenge = Uuid::new_v4();
            let j = Job {
                id,
                challenge,
                stop: None,
                allocation: a.allocation.clone(),
                epoch: a.epoch,
                session: hash(&session),
                email: email.clone(),
                google_email: None,
                expires: Instant::now() + VAULT_JOB_LIFETIME,
                // Reserving a member-bound job does not open Kiri. The vault
                // must select or create a matching login before Deliver starts it.
                state: "vault",
                read,
                flow: None,
                service,
                handle: None,
                delivery: None,
                guest_active: false,
                guest_deadline: None,
            };
            let receipt = j.receipt();
            jobs.push(j);
            return Ok(receipt);
        }
        let position = jobs
            .iter()
            .position(|j| j.owns(request_id, &a, &session, &email))
            .ok_or(GONE)?;
        match input {
            Input::Deliver {
                envelope,
                google_email,
                ..
            } => {
                use base64::engine::general_purpose::STANDARD;
                if !valid_google_email(&google_email) {
                    return Err(StatusCode::BAD_REQUEST);
                }
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
                let fingerprint = delivery_fingerprint(&envelope);
                if !deliverable(jobs[position].state) {
                    // A lost HTTP response may replay this exact owned handoff.
                    // Return the canonical receipt without starting Kiri twice.
                    if jobs[position].delivery.as_deref() != Some(&fingerprint)
                        || jobs[position].google_email.as_deref() != Some(&google_email)
                    {
                        return Err(StatusCode::CONFLICT);
                    }
                    return Ok(jobs[position].receipt());
                }
                if member_has_live_job(&jobs, &a, &email, Some(position)) {
                    return Err(StatusCode::CONFLICT);
                }
                let (challenge, service, read, email, cancelled, budget) = {
                    let j = &mut jobs[position];
                    let (expires, budget) = run_window(Instant::now());
                    j.expires = expires;
                    let challenge = j.challenge;
                    let service = j.service;
                    let read = j.read.clone();
                    let email = j.email.clone();
                    let (stop, cancelled) = oneshot::channel();
                    j.stop = Some(stop);
                    j.state = "validating";
                    j.delivery = Some(fingerprint);
                    j.google_email = Some(google_email.clone());
                    j.guest_active = true;
                    j.guest_deadline = Some(expires);
                    (challenge, service, read, email, cancelled, budget)
                };
                let gateway = g.clone();
                let envelope = json!(envelope);
                let run_allocation = SessionAllocation {
                    allocation: a.allocation.clone(),
                    epoch: a.epoch,
                };
                let run_session = session.clone();
                let run_email = email.clone();
                tokio::spawn(async move {
                    let (state, teardown_confirmed) = tokio::select! {
                        _ = cancelled => ("cancelled", false),
                        outcome = timeout(
                            budget,
                            run(
                                &gateway,
                                &run_allocation,
                                &run_session,
                                request_id,
                                challenge,
                                &run_email,
                                &google_email,
                                service,
                                read,
                                envelope,
                            ),
                        ) => match outcome {
                            Ok(Ok(run_state)) => (
                                if run_state == "returned" { "unavailable" } else { run_state },
                                guest_teardown_confirmed(run_state),
                            ),
                            _ => ("unavailable", false),
                        }
                    };
                    let _ = change(&gateway, request_id, challenge, |j| {
                        finish_run(j, state, teardown_confirmed, Instant::now());
                    });
                });
            }
            Input::Cancel { .. } => {
                return Ok(cancel_job(&mut jobs, position));
            }
            _ => {}
        }
        Ok(jobs
            .iter()
            .find(|j| j.owns(request_id, &a, &session, &email))
            .ok_or(GONE)?
            .receipt())
    }
    .await;
    match result {
        Ok(value) => response(StatusCode::OK, value.to_string().into_bytes()),
        Err(status) => failure(status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(epoch: i64) -> SessionAllocation {
        SessionAllocation {
            allocation: Allocation {
                account_id: Uuid::new_v4().to_string(),
                tenant_id: Uuid::new_v4().to_string(),
                volume_id: Uuid::new_v4().to_string(),
                generation: 4,
            },
            epoch,
        }
    }
    fn pending(a: &SessionAllocation, state: &'static str) -> Job {
        Job {
            id: Uuid::new_v4(),
            challenge: Uuid::new_v4(),
            stop: None,
            allocation: a.allocation.clone(),
            epoch: a.epoch,
            session: hash("session"),
            email: "member@example.test".into(),
            google_email: None,
            expires: Instant::now() + Duration::from_secs(60),
            state,
            read: None,
            flow: None,
            service: None,
            handle: None,
            delivery: None,
            guest_active: false,
            guest_deadline: None,
        }
    }

    #[test]
    fn delivery_starts_a_separate_bounded_kiri_window() {
        let now = Instant::now();
        let (expires, budget) = run_window(now);
        assert_eq!(expires.duration_since(now), KIRI_JOB_LIFETIME);
        assert_eq!(budget, KIRI_RUN_LIMIT);
        assert!(budget < KIRI_JOB_LIFETIME);
    }

    #[test]
    fn pending_vault_job_is_owned_by_exact_job_session_and_email() {
        let a = owner(7);
        let job = pending(&a, "vault");
        assert!(job.owns(job.id, &a, "session", "member@example.test"));
        assert!(!job.owns(Uuid::new_v4(), &a, "session", "member@example.test"));
        assert!(!job.owns(job.id, &a, "other", "member@example.test"));
        assert!(!job.owns(job.id, &a, "session", "other@example.test"));
    }

    #[test]
    fn only_the_vault_state_accepts_delivery() {
        assert!(deliverable("vault"));
        for state in [
            "validating",
            "opening",
            "verifying",
            "signed_in",
            "consent",
            "complete",
        ] {
            assert!(!deliverable(state));
        }
    }

    #[test]
    fn verification_wait_preserves_ownership_and_can_finish_or_cancel() {
        let a = owner(7);
        let job = pending(&a, "opening");
        let id = job.id;
        let challenge = job.challenge;
        let mut jobs = vec![job];
        assert_eq!(
            confirm_verification(&mut jobs, id, challenge, &a, "other", "member@example.test"),
            Err(GONE)
        );
        confirm_verification(
            &mut jobs,
            id,
            challenge,
            &a,
            "session",
            "member@example.test",
        )
        .unwrap();
        assert!(!terminal(jobs[0].state));
        assert!(member_has_live_job(&jobs, &a, "member@example.test", None));
        confirm_signed_in(
            &mut jobs,
            id,
            challenge,
            &a,
            "session",
            "member@example.test",
        )
        .unwrap();
        assert_eq!(jobs[0].state, "signed_in");
        jobs[0].state = "verifying";
        jobs[0].guest_active = true;
        cancel_job(&mut jobs, 0);
        assert_eq!(
            confirm_signed_in(
                &mut jobs,
                id,
                challenge,
                &a,
                "session",
                "member@example.test"
            ),
            Err(GONE)
        );
        assert_eq!(
            confirm_verification(
                &mut jobs,
                id,
                challenge,
                &a,
                "session",
                "member@example.test"
            ),
            Err(GONE)
        );
    }

    #[test]
    fn idempotent_start_must_match_service_and_read_intent() {
        let a = owner(7);
        let mut job = pending(&a, "vault");
        let read = moosuri_store::MemberGoogleReadIntent {
            thread_id: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
        };
        job.service = Some("drive");
        job.read = Some(read.clone());
        assert!(same_start(&job, Some("drive"), &Some(read.clone())));
        assert!(!same_start(&job, Some("docs"), &Some(read.clone())));
        assert!(!same_start(
            &job,
            Some("drive"),
            &Some(moosuri_store::MemberGoogleReadIntent {
                thread_id: read.thread_id,
                request_id: Uuid::new_v4(),
            })
        ));
    }

    #[test]
    fn only_explicit_guest_terminal_states_confirm_teardown() {
        for state in ["blocked", "needs_human", "unavailable", "returned"] {
            assert!(guest_teardown_confirmed(state));
        }
        for state in ["setup_required", "cancelled", "complete", "unexpected"] {
            assert!(!guest_teardown_confirmed(state));
        }
        assert_eq!(stopped("unexpected"), Err(UNAVAILABLE));
    }

    #[test]
    fn another_members_live_job_does_not_block_delivery() {
        let member = owner(7);
        let foreign = owner(7);
        let member_job = pending(&member, "vault");
        let foreign_job = pending(&foreign, "opening");
        let jobs = vec![member_job, foreign_job];
        assert!(!member_has_live_job(
            &jobs,
            &member,
            "member@example.test",
            Some(0)
        ));
        assert!(member_has_live_job(
            &jobs,
            &foreign,
            "member@example.test",
            None
        ));
    }

    #[test]
    fn a_new_session_preserves_other_sessions_reservations_and_receipts() {
        let member = owner(7);
        let old_epoch = SessionAllocation {
            allocation: member.allocation.clone(),
            epoch: 6,
        };
        let foreign = owner(7);
        let current = pending(&member, "vault");
        let current_id = current.id;
        let mut old_session = pending(&member, "vault");
        old_session.session = hash("old-session");
        let old_session_id = old_session.id;
        let old_epoch_job = pending(&old_epoch, "complete");
        let old_epoch_id = old_epoch_job.id;
        let foreign_job = pending(&foreign, "opening");
        let foreign_id = foreign_job.id;
        let mut jobs = vec![current, old_session, old_epoch_job, foreign_job];
        reserve_member_vault(&mut jobs, &member, "session", "member@example.test").unwrap();
        assert!(jobs.iter().any(|j| j.id == current_id));
        assert!(jobs.iter().any(|j| j.id == foreign_id));
        assert!(jobs.iter().any(|j| j.id == old_session_id));
        assert!(jobs.iter().any(|j| j.id == old_epoch_id));
    }

    #[test]
    fn job_id_collision_is_scoped_to_the_member_identity() {
        let member = owner(7);
        let foreign = owner(7);
        let member_job = pending(&member, "vault");
        let id = member_job.id;
        let mut foreign_job = pending(&foreign, "vault");
        foreign_job.id = id;
        assert!(job_id_conflicts(
            &[member_job],
            id,
            &member,
            "member@example.test"
        ));
        assert!(!job_id_conflicts(
            &[foreign_job],
            id,
            &member,
            "member@example.test"
        ));
    }

    #[test]
    fn successful_sign_in_retires_other_pending_tabs_but_keeps_the_winner() {
        let a = owner(7);
        let winner = pending(&a, "validating");
        let winner_id = winner.id;
        let winner_challenge = winner.challenge;
        let other = pending(&a, "vault");
        let other_id = other.id;
        let mut jobs = vec![winner, other];
        confirm_opening(
            &mut jobs,
            winner_id,
            winner_challenge,
            &a,
            "session",
            "member@example.test",
        )
        .unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().any(|j| j.id == other_id));
        confirm_signed_in(
            &mut jobs,
            winner_id,
            winner_challenge,
            &a,
            "session",
            "member@example.test",
        )
        .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, winner_id);
        assert_eq!(jobs[0].state, "signed_in");
    }

    #[test]
    fn failed_validation_keeps_other_pending_tabs() {
        let a = owner(7);
        let winner = pending(&a, "validating");
        let winner_id = winner.id;
        let other = pending(&a, "vault");
        let other_id = other.id;
        let mut jobs = vec![winner, other];
        assert_eq!(
            confirm_opening(
                &mut jobs,
                winner_id,
                Uuid::new_v4(),
                &a,
                "session",
                "member@example.test",
            ),
            Err(GONE)
        );
        assert!(jobs.iter().any(|j| j.id == other_id));
        assert_eq!(jobs.iter().filter(|j| j.state == "vault").count(), 1);
    }

    #[test]
    fn ninth_member_reservation_evicts_only_that_members_oldest_vault_job() {
        let a = owner(7);
        let foreign = owner(7);
        let foreign_job = pending(&foreign, "vault");
        let foreign_id = foreign_job.id;
        let mut jobs = vec![foreign_job];
        for _ in 0..MAX_MEMBER_VAULT_JOBS {
            jobs.push(pending(&a, "vault"));
        }
        let oldest = jobs[1].id;
        jobs[1].expires = Instant::now() + Duration::from_secs(1);
        assert!(make_room_for_member_vault(
            &mut jobs,
            &a,
            "session",
            "member@example.test"
        ));
        assert_eq!(jobs.len(), MAX_MEMBER_VAULT_JOBS);
        assert!(jobs.iter().any(|j| j.id == foreign_id));
        assert!(!jobs.iter().any(|j| j.id == oldest));
    }

    #[test]
    fn member_vault_limit_does_not_evict_another_sessions_reservation() {
        let a = owner(7);
        let mut jobs: Vec<_> = (0..MAX_MEMBER_VAULT_JOBS)
            .map(|_| {
                let mut job = pending(&a, "vault");
                job.session = hash("other-session");
                job
            })
            .collect();
        let ids: Vec<_> = jobs.iter().map(|j| j.id).collect();
        assert!(!make_room_for_member_vault(
            &mut jobs,
            &a,
            "session",
            "member@example.test"
        ));
        assert_eq!(jobs.iter().map(|j| j.id).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn capacity_refusal_preserves_unpolled_terminal_receipts() {
        let a = owner(7);
        let mut jobs: Vec<_> = (0..MAX_MEMBER_JOBS)
            .map(|_| pending(&a, "complete"))
            .collect();
        let ids: Vec<_> = jobs.iter().map(|j| j.id).collect();
        assert!(!make_room_for_member_vault(
            &mut jobs,
            &a,
            "session",
            "member@example.test"
        ));
        assert_eq!(jobs.iter().map(|j| j.id).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn rejected_start_does_not_retire_another_session() {
        let a = owner(7);
        let current = pending(&a, "opening");
        let current_id = current.id;
        let mut stale = pending(&a, "opening");
        stale.session = hash("old-session");
        let stale_id = stale.id;
        let mut jobs = vec![current, stale];
        assert_eq!(
            reserve_member_vault(&mut jobs, &a, "session", "member@example.test"),
            Err(StatusCode::CONFLICT)
        );
        assert!(jobs.iter().any(|j| j.id == current_id));
        assert!(jobs.iter().any(|j| j.id == stale_id));
    }

    #[test]
    fn another_sessions_active_guest_blocks_start_and_is_not_retired() {
        let a = owner(7);
        let mut active = pending(&a, "consent");
        active.session = hash("other-session");
        active.guest_active = true;
        let active_id = active.id;
        let mut jobs = vec![active];
        assert_eq!(
            reserve_member_vault(&mut jobs, &a, "session", "member@example.test"),
            Err(StatusCode::CONFLICT)
        );
        assert!(jobs.iter().any(|j| j.id == active_id && j.guest_active));
    }

    #[test]
    fn cancellation_keeps_active_marker_until_run_task_acknowledges() {
        let a = owner(7);
        let mut active = pending(&a, "opening");
        let (stop, _cancelled) = oneshot::channel();
        active.stop = Some(stop);
        active.guest_active = true;
        let mut jobs = vec![active];
        assert_eq!(cancel_job(&mut jobs, 0), json!({"state":"cancelled"}));
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].state, "cancelled");
        assert!(jobs[0].guest_active);
        assert!(jobs[0].stop.is_none());
        assert!(member_has_live_job(&jobs, &a, "member@example.test", None));
        jobs[0].guest_active = false;
        assert!(!member_has_live_job(&jobs, &a, "member@example.test", None));
    }

    #[test]
    fn cancelled_job_cannot_advance_to_sign_in_or_consent() {
        let a = owner(7);
        let mut opening = pending(&a, "opening");
        opening.guest_active = true;
        let opening_id = opening.id;
        let opening_challenge = opening.challenge;
        let mut jobs = vec![opening];
        cancel_job(&mut jobs, 0);
        assert_eq!(
            confirm_signed_in(
                &mut jobs,
                opening_id,
                opening_challenge,
                &a,
                "session",
                "member@example.test",
            ),
            Err(GONE)
        );

        let mut signed_in = pending(&a, "signed_in");
        signed_in.guest_active = true;
        let signed_in_id = signed_in.id;
        let signed_in_challenge = signed_in.challenge;
        let mut jobs = vec![signed_in];
        cancel_job(&mut jobs, 0);
        assert_eq!(
            confirm_consent(
                &mut jobs,
                signed_in_id,
                signed_in_challenge,
                &a,
                "session",
                "member@example.test",
                "flow",
            ),
            Err(GONE)
        );
    }

    #[test]
    fn cancel_does_not_overwrite_a_completed_receipt() {
        let a = owner(7);
        let mut complete = pending(&a, "complete");
        complete.handle = Some("provider-handle".into());
        complete.guest_active = true;
        let mut jobs = vec![complete];
        let receipt = cancel_job(&mut jobs, 0);
        assert_eq!(receipt["state"], "complete");
        assert_eq!(receipt["handle"], "provider-handle");
        assert_eq!(jobs[0].state, "complete");
        assert!(jobs[0].guest_active);
    }

    #[test]
    fn active_guest_has_a_hard_fail_safe_deadline() {
        let a = owner(7);
        let now = Instant::now();
        let mut active = pending(&a, "opening");
        let (stop, _cancelled) = oneshot::channel();
        active.stop = Some(stop);
        active.guest_active = true;
        active.guest_deadline = Some(now);
        let mut jobs = vec![active];
        expire_active_guests(&mut jobs, now);
        assert!(!jobs[0].guest_active);
        assert!(jobs[0].guest_deadline.is_none());
        assert!(jobs[0].stop.is_none());
        assert_eq!(jobs[0].state, "unavailable");
    }

    #[test]
    fn terminal_provider_result_gets_a_fresh_collection_window() {
        let a = owner(7);
        let mut job = pending(&a, "consent");
        let now = Instant::now();
        job.expires = now + Duration::from_secs(1);
        finish_receipt(&mut job, Some("provider-handle"), now);
        assert_eq!(job.state, "complete");
        assert_eq!(job.handle.as_deref(), Some("provider-handle"));
        assert_eq!(job.expires.duration_since(now), TERMINAL_RECEIPT_LIFETIME);
    }

    #[test]
    fn run_terminal_result_gets_a_fresh_collection_window() {
        let a = owner(7);
        let mut job = pending(&a, "opening");
        let now = Instant::now();
        job.expires = now + Duration::from_secs(1);
        record_run_state(&mut job, "needs_human", now);
        assert_eq!(job.state, "needs_human");
        assert_eq!(job.expires.duration_since(now), TERMINAL_RECEIPT_LIFETIME);
    }

    #[test]
    fn receipt_retains_the_guest_fence_until_full_teardown_grace() {
        let a = owner(7);
        let now = Instant::now();
        let mut job = pending(&a, "verifying");
        job.expires = now + Duration::from_secs(1);
        finish_run(&mut job, "unavailable", false, now);
        let mut jobs = vec![job];
        let after_receipt = now + TERMINAL_RECEIPT_LIFETIME + Duration::from_secs(1);
        expire_active_guests(&mut jobs, after_receipt);
        jobs.retain(|j| j.expires > after_receipt);
        assert_eq!(jobs.len(), 1);
        assert!(member_has_live_job(&jobs, &a, "member@example.test", None));
        expire_active_guests(&mut jobs, now + GUEST_TEARDOWN_GRACE);
        assert!(!member_has_live_job(&jobs, &a, "member@example.test", None));
    }

    #[test]
    fn early_provider_result_does_not_shorten_the_run_window() {
        let a = owner(7);
        let mut job = pending(&a, "consent");
        let now = Instant::now();
        job.expires = now + Duration::from_secs(300);
        finish_receipt(&mut job, Some("provider-handle"), now);
        assert_eq!(job.expires.duration_since(now), Duration::from_secs(300));
    }

    #[test]
    fn initial_kiri_reply_requires_the_current_protocol() {
        assert!(parse_phase(&json!({"status":"vault"}), true).is_err());
        assert!(
            parse_phase(
                &json!({"status":"vault","protocol":"moosuri-google-connection-v1"}),
                true
            )
            .is_err()
        );
        assert_eq!(
            parse_phase(&json!({"status":"vault","protocol":PROTOCOL}), true).unwrap(),
            "vault"
        );
    }

    #[test]
    fn delivery_replay_must_match_the_first_envelope() {
        let first = Envelope {
            key: "key".into(),
            iv: "iv".into(),
            body: "body".into(),
        };
        let first_fingerprint = delivery_fingerprint(&first);
        let second = Envelope {
            body: "other".into(),
            ..first
        };
        assert_ne!(first_fingerprint, delivery_fingerprint(&second));
    }
}
fn change<T>(
    g: &Gateway,
    id: Uuid,
    challenge: Uuid,
    edit: impl FnOnce(&mut Job) -> T,
) -> Result<T, StatusCode> {
    let mut guard = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
    Ok(edit(
        guard
            .iter_mut()
            .find(|j| j.id == id && j.challenge == challenge && j.expires > Instant::now())
            .ok_or(GONE)?,
    ))
}
fn require_state(
    g: &Gateway,
    id: Uuid,
    challenge: Uuid,
    expected: &'static str,
) -> Result<(), StatusCode> {
    change(g, id, challenge, |j| {
        if !terminal(j.state) && j.state == expected {
            Ok(())
        } else {
            Err(GONE)
        }
    })?
}
fn begin_opening(
    g: &Gateway,
    id: Uuid,
    challenge: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) -> Result<(), StatusCode> {
    let mut jobs = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
    confirm_opening(&mut jobs, id, challenge, a, session, email)
}
fn finish_sign_in(
    g: &Gateway,
    id: Uuid,
    challenge: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
) -> Result<(), StatusCode> {
    let mut jobs = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
    confirm_signed_in(&mut jobs, id, challenge, a, session, email)
}
fn begin_consent(
    g: &Gateway,
    id: Uuid,
    challenge: Uuid,
    a: &SessionAllocation,
    session: &str,
    email: &str,
    flow: &str,
) -> Result<(), StatusCode> {
    let mut jobs = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
    confirm_consent(&mut jobs, id, challenge, a, session, email, flow)
}
async fn send(peer: &mut BufReader<UnixStream>, v: Value) -> Result<(), StatusCode> {
    let mut line = v.to_string().into_bytes();
    line.push(b'\n');
    peer.get_mut()
        .write_all(&line)
        .await
        .map_err(|_| UNAVAILABLE)
}
fn parse_phase(reply: &Value, require_protocol: bool) -> Result<String, StatusCode> {
    let fields = reply.as_object().ok_or(UNAVAILABLE)?;
    let expected_fields = if require_protocol { 2 } else { 1 };
    if fields.len() != expected_fields
        || (require_protocol && reply["protocol"].as_str() != Some(PROTOCOL))
    {
        return Err(UNAVAILABLE);
    }
    reply["status"]
        .as_str()
        .map(str::to_owned)
        .ok_or(UNAVAILABLE)
}
async fn phase(
    peer: &mut BufReader<UnixStream>,
    require_protocol: bool,
) -> Result<String, StatusCode> {
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
    parse_phase(&reply, require_protocol)
}
async fn authorized_phase(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    email: &str,
    peer: &mut BufReader<UnixStream>,
    require_protocol: bool,
) -> Result<String, StatusCode> {
    let mut checks = interval(AUTH_RECHECK_INTERVAL);
    checks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    checks.tick().await;
    let reply = phase(peer, require_protocol);
    tokio::pin!(reply);
    loop {
        tokio::select! {
            result = &mut reply => return result,
            _ = checks.tick() => reauthorize(g, a, session, email).await?,
        }
    }
}
fn stopped(s: &str) -> Result<&'static str, StatusCode> {
    match s {
        "blocked" => Ok("blocked"),
        "needs_human" => Ok("needs_human"),
        "unavailable" => Ok("unavailable"),
        _ => Err(UNAVAILABLE),
    }
}
async fn run(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
    challenge: Uuid,
    email: &str,
    google_email: &str,
    service: Option<&'static str>,
    read: Option<moosuri_store::MemberGoogleReadIntent>,
    envelope: Value,
) -> Result<&'static str, StatusCode> {
    let stream = UnixStream::connect(SOCKET).await.map_err(|_| UNAVAILABLE)?;
    if stream.peer_cred().map_err(|_| UNAVAILABLE)?.uid() != 0 {
        return Err(UNAVAILABLE);
    }
    let mut peer = BufReader::new(stream);
    send(
        &mut peer,
        json!({"protocol":PROTOCOL,"job":challenge,"account":a.account_id,"email":email,"google_email":google_email,"service":service.unwrap_or("gmail")}),
    )
    .await?;
    let state = authorized_phase(g, a, session, email, &mut peer, true).await?;
    if state != "vault" {
        return stopped(&state);
    }
    reauthorize(g, a, session, email).await?;
    require_state(g, id, challenge, "validating")?;
    send(&mut peer, envelope).await?;
    let state = authorized_phase(g, a, session, email, &mut peer, false).await?;
    if state != "verified" {
        return stopped(&state);
    }
    reauthorize(g, a, session, email).await?;
    // Kiri must not create Chrome until this post-decryption reauthorization and
    // exact job/challenge ownership transition succeeds. Sibling vault
    // reservations remain available until this attempt signs in successfully.
    begin_opening(g, id, challenge, a, session, email)?;
    send(&mut peer, json!({"status":"proceed"})).await?;
    let mut awaiting_verification = false;
    let state = loop {
        let state = authorized_phase(g, a, session, email, &mut peer, false).await?;
        if state != "verifying" {
            break state;
        }
        if awaiting_verification {
            return Err(UNAVAILABLE);
        }
        reauthorize(g, a, session, email).await?;
        {
            let mut jobs = g.google_jobs.lock().map_err(|_| UNAVAILABLE)?;
            confirm_verification(&mut jobs, id, challenge, a, session, email)?;
        }
        awaiting_verification = true;
    };
    if state != "signed_in" {
        return stopped(&state);
    }
    reauthorize(g, a, session, email).await?;
    finish_sign_in(g, id, challenge, a, session, email)?;
    let path = match service {
        Some(name) => format!("/api/v1/personal/google/{name}/start"),
        None => "/api/v1/personal/google/start".into(),
    };
    let mut payload = json!({"selected_email":google_email});
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
        .append_pair("login_hint", google_email);
    let flow = start["flow_id"].as_str().ok_or(UNAVAILABLE)?;
    reauthorize(g, a, session, email).await?;
    begin_consent(g, id, challenge, a, session, email, flow)?;
    send(&mut peer, json!({"url":url.as_str()})).await?;
    let state = authorized_phase(g, a, session, email, &mut peer, false).await?;
    if state == "returned" {
        // Only the matching provider callback can finish this job. Preserve the
        // distinct internal value so the caller can treat it as teardown proof.
        Ok("returned")
    } else {
        stopped(&state)
    }
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
    let Some(j) = jobs.iter_mut().find(|j| {
        j.state == "consent"
            && j.expires > Instant::now()
            && j.flow.as_deref() == Some(flow)
            && j.service == service
            && j.epoch == a.epoch
            && j.allocation == a.allocation
    }) else {
        return false;
    };
    finish_receipt(j, handle, Instant::now());
    true
}
