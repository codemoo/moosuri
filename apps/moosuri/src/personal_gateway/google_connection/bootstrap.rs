//! Gateway-owned SDK preparation for the one installation Google bootstrap.
//!
//! This is an unwired integration foundation: nothing here is reachable from a route and no
//! Console client preparation exists yet. A caller owns the already signed-in worker socket
//! (`BufReader<UnixStream>`) of the SAME browser and keeps it alive between phases. On any
//! failure the caller closes/drops that peer; this module never releases the durable lease
//! early on failure or on a successful `Finish`. The registry's 180 s fence stays in force
//! until `release_after_teardown_ack` is called after an explicit teardown acknowledgement.
//!
//! Authority is never cached across awaits. Every registry operation performs a new private
//! `GET /api/v1/auth/session`, then in `spawn_blocking` immediately builds a fresh
//! `checked_session(..).at_epoch(..)` pin and runs exactly one registry call. A heartbeat is
//! sent before every worker command and every 2 s while waiting for a reply; heartbeat bytes
//! never reach the worker. A lost heartbeat/authorization returns an error, and the caller
//! drops the peer, which cancels the worker.
#![allow(dead_code)]
use super::*;
use moosuri_connectors::browser::google_auth::bootstrap::{Action, Reply, Request};
use moosuri_store::{
    StoreError,
    runtime_registry::admin::{
        self as control, AdminSession,
        google_bootstrap::{self as registry, Setup, Step},
    },
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const REGISTRY_LIMIT: Duration = Duration::from_secs(10);
/// Longest single worker action (Authorize 420 s, Enable 600 s) plus margin.
const ACTION_WAIT: Duration = Duration::from_secs(660);
const MAX_REPLY: usize = 256;

/// Terminal provider/worker outcome. The worker has stopped after sending one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Terminal {
    Blocked,
    NeedsHuman,
    Unavailable,
}

/// Why preparation or a command stopped. Provider results are distinct from gateway/registry
/// authority failures (`Gateway(FORBIDDEN)`, `Gateway(CONFLICT)`, ...). Carries no secrets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Provider(Terminal),
    Gateway(StatusCode),
    /// A provider write may have happened but its result cannot be proven (Project intent
    /// recorded, project absent). Never repeated and never given a new id.
    Unresolved,
    /// The temporary SDK grant may remain live; durable pending state blocks retry.
    CleanupUnconfirmed,
}

fn protocol() -> Failure {
    Failure::Gateway(UNAVAILABLE)
}

/// Unique handle of this claimed run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RunHandle(Uuid);
impl RunHandle {
    pub(super) fn id(self) -> Uuid {
        self.0
    }
}

pub(super) enum Prepared {
    /// Registered: already configured, no SDK command was sent.
    Configured(Setup),
    /// SDK authorized, Project reconciled and APIs verified; worker and Chrome stay alive.
    Ready(Controller),
}

pub(super) struct Controller {
    g: Gateway,
    a: SessionAllocation,
    session: Zeroizing<String>,
    member_email: String,
    run: Uuid,
    redirect_uri: String,
    setup: Setup,
    /// Set before a command and cleared only after its exact reply; a dropped future or any
    /// failure leaves it set, so an unknown outcome is never retried through this controller.
    broken: bool,
    finished: bool,
}

impl std::fmt::Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controller")
            .field("run", &self.run)
            .field("step", &self.setup.step)
            .field("broken", &self.broken)
            .finish_non_exhaustive()
    }
}

/// Exact worker reply: one `{"bootstrap":"ready|absent"}` or one terminal
/// `{"status":"blocked|needs_human|unavailable"}`; anything else is a protocol failure.
fn parse_reply(line: &[u8]) -> Result<Reply, Failure> {
    let value: Value = serde_json::from_slice(line).map_err(|_| protocol())?;
    if let Ok(reply) = Reply::from_wire(&value) {
        return Ok(reply);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Status {
        status: String,
    }
    let status: Status = serde_json::from_value(value).map_err(|_| protocol())?;
    Err(match status.status.as_str() {
        "blocked" => Failure::Provider(Terminal::Blocked),
        "needs_human" => Failure::Provider(Terminal::NeedsHuman),
        "unavailable" => Failure::Provider(Terminal::Unavailable),
        "cleanup_unconfirmed" => Failure::CleanupUnconfirmed,
        _ => protocol(),
    })
}

/// Only an inspection may answer `absent`.
fn expected(action: Action, reply: Reply) -> Result<Reply, Failure> {
    match (action, reply) {
        (Action::InspectProject, _) | (_, Reply::Ready) => Ok(reply),
        _ => Err(protocol()),
    }
}

async fn read_reply(peer: &mut BufReader<UnixStream>) -> Result<Reply, Failure> {
    let mut line = Vec::new();
    (&mut *peer)
        .take(MAX_REPLY as u64 + 1)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|_| protocol())?;
    if line.len() > MAX_REPLY || line.pop() != Some(b'\n') {
        return Err(protocol());
    }
    parse_reply(&line)
}

impl Controller {
    fn new(
        g: &Gateway,
        a: &SessionAllocation,
        session: &str,
        member_email: &str,
        run: Uuid,
        redirect_uri: String,
        setup: Setup,
    ) -> Self {
        Self {
            g: g.clone(),
            a: SessionAllocation {
                allocation: a.allocation.clone(),
                epoch: a.epoch,
            },
            session: Zeroizing::new(session.to_owned()),
            member_email: member_email.to_owned(),
            run,
            redirect_uri,
            setup,
            broken: false,
            finished: false,
        }
    }

    pub(super) fn setup(&self) -> &Setup {
        &self.setup
    }
    pub(super) fn run(&self) -> RunHandle {
        RunHandle(self.run)
    }

    /// Prepare the SDK for the installation setup. Claims the stable setup with a fresh admin
    /// proof (the claim enforces the Google email and callback binding), then performs
    /// Authorize, Project read/reconcile and APIs. The callback is the gateway origin plus
    /// `google::CALLBACK`. On `Err` the caller must close `peer`; the lease is not released.
    pub(super) async fn prepare(
        g: &Gateway,
        a: &SessionAllocation,
        session: &str,
        member_email: &str,
        google_email: &str,
        prepared: &mut portable::Prepared,
    ) -> Result<Prepared, Failure> {
        let binding = prepared.binding();
        if !valid_google_email(google_email)
            || !binding.valid()
            || binding.origin != g.origin
            || binding.email != member_email
            || binding.account.to_string() != a.account_id
            || binding.tenant.to_string() != a.tenant_id
            || binding.volume.to_string() != a.volume_id
            || binding.generation != a.generation
            || binding.epoch != a.epoch
        {
            return Err(Failure::Gateway(StatusCode::BAD_REQUEST));
        }
        // Prepared is constructed only after checking the root peer's exact
        // recipient binding. A caller cannot supply an unrelated run id.
        let run = binding.challenge;
        let peer = &mut prepared.peer;
        let redirect_uri = format!("{}{}", g.origin, google::CALLBACK);
        let mut c = Self::new(
            g,
            a,
            session,
            member_email,
            run,
            redirect_uri.clone(),
            // Replaced by the claimed setup below.
            Setup {
                setup_id: Uuid::nil(),
                google_email: String::new(),
                redirect_uri: String::new(),
                step: Step::Project,
                intent: false,
            },
        );
        // Observe Registered without taking a new lease or changing identity.
        if let Some(setup) = c
            .with_admin(|root, s| {
                if registry::pending_sdk_run(root, s)?.is_some() {
                    return Err(StoreError::Conflict);
                }
                registry::current(root, s)
            })
            .await?
            && setup.step == Step::Registered
        {
            return Ok(Prepared::Configured(setup));
        }
        let email = google_email.to_owned();
        c.setup = c
            .with_admin(move |root, s| registry::claim(root, s, run, email, redirect_uri))
            .await?;
        if c.setup.step == Step::Registered {
            return Ok(Prepared::Configured(c.setup));
        }
        c.with_admin(move |root, s| registry::begin_sdk(root, s, run))
            .await?;
        c.command(peer, Action::Authorize).await?;
        let absent = c.command(peer, Action::InspectProject).await? == Reply::Absent;
        if c.setup.step == Step::Project {
            if absent {
                if c.setup.intent {
                    // A prior write may exist but cannot be proven: stop, never recreate.
                    return Err(Failure::Unresolved);
                }
                c.intend(Step::Project).await?;
                c.command(peer, Action::CreateProject).await?;
            } else if !c.setup.intent {
                c.intend(Step::Project).await?;
            }
            c.confirm(Step::Project).await?;
            c.setup.step = Step::Apis;
            c.setup.intent = false;
        } else if absent {
            return Err(Failure::Unresolved);
        }
        match c.setup.step {
            Step::Apis => {
                if !c.setup.intent {
                    c.intend(Step::Apis).await?;
                }
                // EnableApis is the fixed idempotent list; it re-proves owned/unbilled first.
                c.command(peer, Action::EnableApis).await?;
                c.command(peer, Action::VerifyApis).await?;
                c.confirm(Step::Apis).await?;
                c.setup.step = Step::Branding;
                c.setup.intent = false;
            }
            // Beyond APIs: establish the worker's owned session guard only.
            _ => {
                c.command(peer, Action::VerifyApis).await?;
            }
        }
        Ok(Prepared::Ready(c))
    }

    async fn intend(&mut self, step: Step) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::intend(root, s, run, step).map(|_| ()))
            .await?;
        self.setup.intent = true;
        Ok(())
    }
    async fn confirm(&self, step: Step) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::confirmed(root, s, run, step))
            .await
    }

    /// One authorized registry operation; no admin state outlives this call.
    async fn with_admin<T: Send + 'static>(
        &self,
        op: impl FnOnce(&std::path::Path, &AdminSession) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, Failure> {
        timeout(REGISTRY_LIMIT, async {
            let (status, bytes, _) = self
                .g
                .proxy(
                    &self.a,
                    Method::GET,
                    "/api/v1/auth/session",
                    None,
                    Some(self.session.as_str()),
                )
                .await
                .map_err(Failure::Gateway)?;
            if status != StatusCode::OK {
                return Err(Failure::Gateway(status));
            }
            let view = member_session_view(&bytes, &self.a).map_err(Failure::Gateway)?;
            if view.user.email != self.member_email {
                return Err(Failure::Gateway(GONE));
            }
            if view.password_change_required || view.user.must_change_password {
                return Err(Failure::Gateway(StatusCode::FORBIDDEN));
            }
            let root = self.g.registry.clone();
            let allocation = self.a.allocation.clone();
            let epoch = self.a.epoch;
            tokio::task::spawn_blocking(move || {
                let pin = control::checked_session(&root, &allocation, &view)
                    .and_then(|proof| proof.at_epoch(epoch))
                    .map_err(super::super::admin::error)?;
                op(&root, &pin).map_err(super::super::admin::error)
            })
            .await
            .map_err(|_| protocol())?
            .map_err(Failure::Gateway)
        })
        .await
        .map_err(|_| protocol())?
    }

    /// Renews the durable lease after fresh reauthorization. Never touches the worker.
    pub(super) async fn heartbeat(&self) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::heartbeat(root, s, run))
            .await
    }

    /// Send one fixed typed action and wait (bounded to 660 s, heartbeating every 2 s) for its
    /// exact reply. Any failure, timeout or drop of this future poisons the controller: the
    /// outcome is unknown and is never retried. The caller must then drop the peer.
    pub(super) async fn command(
        &mut self,
        peer: &mut BufReader<UnixStream>,
        action: Action,
    ) -> Result<Reply, Failure> {
        if self.broken || self.finished {
            return Err(Failure::Gateway(GONE));
        }
        self.broken = true;
        let reply = self.exchange(peer, action).await?;
        self.broken = false;
        Ok(reply)
    }

    async fn exchange(
        &self,
        peer: &mut BufReader<UnixStream>,
        action: Action,
    ) -> Result<Reply, Failure> {
        // Fresh authority and lease immediately before the bytes that may cause a write.
        self.heartbeat().await?;
        let request = Request {
            setup_id: self.setup.setup_id,
            redirect_uri: self.redirect_uri.clone(),
            action,
        };
        send(peer, request.to_wire())
            .await
            .map_err(Failure::Gateway)?;
        let mut checks = interval(HEARTBEAT_INTERVAL);
        checks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        checks.tick().await;
        let reply = read_reply(peer);
        tokio::pin!(reply);
        let deadline = tokio::time::sleep(ACTION_WAIT);
        tokio::pin!(deadline);
        let reply = loop {
            tokio::select! { biased;
                result = &mut reply => break result?,
                _ = &mut deadline => return Err(protocol()),
                _ = checks.tick() => self.heartbeat().await?,
            }
        };
        expected(action, reply)
    }

    /// SDK credential cleanup only. This is NOT teardown acknowledgement and does not
    /// release the lease or end the worker session.
    pub(super) async fn finish(&mut self, peer: &mut BufReader<UnixStream>) -> Result<(), Failure> {
        self.command(peer, Action::Finish).await?;
        self.finished = true;
        Ok(())
    }

    /// Release the fence early. Call ONLY after explicit guest teardown acknowledgement
    /// (never after a successful `finish`, a failure, or a cancel).
    pub(super) async fn release_after_teardown_ack(&self) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::release(root, s, run))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(value: Value) -> Vec<u8> {
        value.to_string().into_bytes()
    }

    #[test]
    fn replies_are_exact_ready_absent_or_terminal() {
        assert_eq!(
            parse_reply(&line(json!({"status":"cleanup_unconfirmed"}))),
            Err(Failure::CleanupUnconfirmed)
        );
        assert_eq!(
            parse_reply(&line(json!({"bootstrap":"ready"}))),
            Ok(Reply::Ready)
        );
        assert_eq!(
            parse_reply(&line(json!({"bootstrap":"absent"}))),
            Ok(Reply::Absent)
        );
        for (status, terminal) in [
            ("blocked", Terminal::Blocked),
            ("needs_human", Terminal::NeedsHuman),
            ("unavailable", Terminal::Unavailable),
        ] {
            assert_eq!(
                parse_reply(&line(json!({"status":status}))),
                Err(Failure::Provider(terminal))
            );
        }
        for bad in [
            json!({"bootstrap":"ready","x":1}),
            json!({"bootstrap":"ready","status":"blocked"}),
            json!({"status":"blocked","detail":"page text"}),
            json!({"status":"returned"}),
            json!({"status":"signed_in"}),
            json!({"bootstrap":"other"}),
            json!("ready"),
            json!([]),
        ] {
            assert_eq!(parse_reply(&line(bad)), Err(protocol()));
        }
        assert_eq!(parse_reply(b"{"), Err(protocol()));
    }

    #[test]
    fn absence_answers_only_an_inspection() {
        assert_eq!(
            expected(Action::InspectProject, Reply::Absent),
            Ok(Reply::Absent)
        );
        for action in [
            Action::Authorize,
            Action::CreateProject,
            Action::EnableApis,
            Action::VerifyApis,
            Action::Finish,
        ] {
            assert_eq!(expected(action, Reply::Absent), Err(protocol()));
            assert_eq!(expected(action, Reply::Ready), Ok(Reply::Ready));
        }
    }

    #[tokio::test]
    async fn reply_line_is_bounded_and_newline_terminated() {
        let (mut worker, controller) = UnixStream::pair().unwrap();
        let mut peer = BufReader::new(controller);
        worker
            .write_all(b"{\"bootstrap\":\"absent\"}\n")
            .await
            .unwrap();
        assert_eq!(read_reply(&mut peer).await, Ok(Reply::Absent));
        worker
            .write_all(b"{\"bootstrap\":\"ready\"}")
            .await
            .unwrap();
        drop(worker);
        assert_eq!(read_reply(&mut peer).await, Err(protocol()));

        let (mut worker, controller) = UnixStream::pair().unwrap();
        let mut peer = BufReader::new(controller);
        let mut long = vec![b' '; MAX_REPLY];
        long.push(b'\n');
        worker.write_all(&long).await.unwrap();
        assert_eq!(read_reply(&mut peer).await, Err(protocol()));
    }

    #[tokio::test]
    async fn lost_authority_sends_nothing_and_poisons_the_controller() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let g = Gateway::new(&super::super::super::tests::options(&root)).unwrap();
        let a = SessionAllocation {
            allocation: Allocation {
                account_id: Uuid::new_v4().to_string(),
                tenant_id: Uuid::new_v4().to_string(),
                volume_id: Uuid::new_v4().to_string(),
                generation: 1,
            },
            epoch: 0,
        };
        let setup = Setup {
            setup_id: Uuid::new_v4(),
            google_email: "google@example.test".into(),
            redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
            step: Step::Project,
            intent: false,
        };
        let mut c = Controller::new(
            &g,
            &a,
            "session",
            "member@example.test",
            Uuid::new_v4(),
            setup.redirect_uri.clone(),
            setup,
        );
        let (mut worker, controller) = UnixStream::pair().unwrap();
        let mut peer = BufReader::new(controller);
        // No private service answers, so the pre-send heartbeat fails and no byte is sent.
        assert!(c.command(&mut peer, Action::InspectProject).await.is_err());
        let mut byte = [0u8; 1];
        assert!(
            timeout(Duration::from_millis(50), worker.read(&mut byte))
                .await
                .is_err()
        );
        // Poisoned: no retry, in particular no repeated write after an unknown outcome.
        assert_eq!(
            c.command(&mut peer, Action::InspectProject).await,
            Err(Failure::Gateway(GONE))
        );
        // A dropped (cancelled) command also leaves the controller refusing further commands.
        let mut c2 = Controller::new(
            &g,
            &a,
            "session",
            "member@example.test",
            Uuid::new_v4(),
            c.redirect_uri.clone(),
            c.setup.clone(),
        );
        let _ = timeout(Duration::ZERO, c2.command(&mut peer, Action::VerifyApis)).await;
        assert!(c2.broken);
    }
}
