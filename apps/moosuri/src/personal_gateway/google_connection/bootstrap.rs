//! Gateway-owned SDK preparation for the one installation Google bootstrap.
//!
//! The caller owns the already signed-in worker socket
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
use moosuri_connectors::browser::google_auth::bootstrap::{
    self as wire, Action, Reply, Request, Response as WorkerResponse,
};
use moosuri_store::{
    StoreError,
    runtime_registry::admin::{
        self as control, AdminSession,
        google_bootstrap::{self as registry, Command, CommandState, Publication, Setup, Step},
    },
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
const REGISTRY_LIMIT: Duration = Duration::from_secs(10);
/// Longest single worker action (Authorize 420 s, Enable 600 s) plus margin.
const ACTION_WAIT: Duration = Duration::from_secs(660);
const MAX_REPLY: usize = wire::PRIVATE_WIRE_LIMIT;

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
    /// Provider client exists, but its one-time secret was never durably saved.
    SecretUnavailable,
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
fn parse_response(line: &[u8]) -> Result<WorkerResponse, Failure> {
    let mut value: Value = serde_json::from_slice(line).map_err(|_| protocol())?;
    let result = (|| {
        if let Ok(reply) = WorkerResponse::from_wire(&value) {
            return Ok(reply);
        }
        let fields = value
            .as_object()
            .filter(|f| f.len() == 1)
            .ok_or_else(protocol)?;
        let status = fields
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(protocol)?;
        Err(match status {
            "blocked" => Failure::Provider(Terminal::Blocked),
            "needs_human" => Failure::Provider(Terminal::NeedsHuman),
            "unavailable" => Failure::Provider(Terminal::Unavailable),
            "cleanup_unconfirmed" => Failure::CleanupUnconfirmed,
            _ => protocol(),
        })
    })();
    wire::scrub(&mut value);
    result
}
#[cfg(test)]
fn parse_reply(line: &[u8]) -> Result<Reply, Failure> {
    match parse_response(line)? {
        WorkerResponse::Status(reply) => Ok(reply),
        _ => Err(protocol()),
    }
}

/// Only an inspection may answer `absent`.
fn expected(action: Action, reply: Reply) -> Result<Reply, Failure> {
    match (action, reply) {
        (
            Action::InspectProject
            | Action::InspectBranding
            | Action::InspectAudience
            | Action::InspectScopes
            | Action::InspectDomains
            | Action::InspectPublication
            | Action::InspectClient,
            _,
        )
        | (_, Reply::Ready) => Ok(reply),
        _ => Err(protocol()),
    }
}

/// What to do for one Console command given its durable state and the fresh readback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    /// Already confirmed and still present: inspect only.
    Done,
    /// Record intent first if never attempted, send Configure only if the readback is absent,
    /// then confirm (a present readback, or the worker's reply to Configure, is the evidence).
    Settle { intend: bool, configure: bool },
}

/// An uncertain intent (or a confirmed one) whose provider state is absent is never replayed.
fn plan(state: CommandState, seen: Reply) -> Result<Plan, Failure> {
    match (state, seen) {
        (CommandState::Confirmed, Reply::Ready) => Ok(Plan::Done),
        (CommandState::Confirmed | CommandState::Intent, Reply::Absent) => Err(Failure::Unresolved),
        (CommandState::Intent, Reply::Ready) => Ok(Plan::Settle {
            intend: false,
            configure: false,
        }),
        (CommandState::Untried, seen) => Ok(Plan::Settle {
            intend: true,
            configure: seen == Reply::Absent,
        }),
    }
}

async fn read_reply(peer: &mut BufReader<UnixStream>) -> Result<WorkerResponse, Failure> {
    let mut line = Zeroizing::new(Vec::with_capacity(MAX_REPLY + 1));
    (&mut *peer)
        .take(MAX_REPLY as u64 + 1)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|_| protocol())?;
    if line.len() > MAX_REPLY || line.pop() != Some(b'\n') {
        return Err(protocol());
    }
    parse_response(&line)
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
        // Observe Registered without taking a new lease or changing identity. A saved client
        // is NOT publication: only a confirmed publication journal ends the setup here. An
        // older Registered checkpoint (no journal rows) continues below and is published.
        if let Some((setup, true)) = c
            .with_admin(|root, s| {
                if registry::pending_sdk_run(root, s)?.is_some() {
                    return Err(StoreError::Conflict);
                }
                let published = registry::publication_confirmed(root, s)?;
                Ok(registry::current(root, s)?.map(|setup| (setup, published)))
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
        if c.setup.step == Step::Registered && c.with_admin(registry::publication_confirmed).await?
        {
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

    /// Prepare branding, the audience (External/Testing plus the initiating account as test
    /// user) and the exact Data Access scopes in the same browser. Each provider mutation has
    /// its own durable command intent/confirmation under the same setup, lease and fresh admin
    /// check; the top-level step only advances to `Client` once every command has a positive
    /// exact readback. A lost mutation reply is never permission to send Configure again:
    /// only a positive readback reconciles it. A stage already confirmed in the registry is
    /// only inspected, never mutated.
    ///
    pub(super) async fn console(
        &mut self,
        peer: &mut BufReader<UnixStream>,
    ) -> Result<(), Failure> {
        if self.setup.step < Step::Branding {
            return Err(Failure::Gateway(StatusCode::CONFLICT));
        }
        for (cmd, inspect, configure) in [
            (
                Command::Branding,
                Action::InspectBranding,
                Action::ConfigureBranding,
            ),
            (
                Command::Audience,
                Action::InspectAudience,
                Action::ConfigureAudience,
            ),
            (
                Command::Scopes,
                Action::InspectScopes,
                Action::ConfigureScopes,
            ),
        ] {
            let seen = self.command(peer, inspect).await?;
            let state = self.command_state(cmd).await?;
            match plan(state, seen)? {
                Plan::Done => {}
                Plan::Settle {
                    intend,
                    configure: send,
                } => {
                    if intend {
                        self.intend_command(cmd).await?;
                    }
                    if send {
                        self.command(peer, configure).await?;
                    }
                    self.confirm_command(cmd).await?;
                }
            }
            // The stage completes with its last command; the registry refuses otherwise.
            let stage = match cmd {
                Command::Branding => Some((Step::Branding, Step::Audience)),
                Command::Scopes => Some((Step::Audience, Step::Client)),
                Command::Audience => None,
            };
            if let Some((step, next)) = stage
                && self.setup.step == step
            {
                self.confirm(step).await?;
                self.setup.step = next;
                self.setup.intent = false;
            }
        }
        Ok(())
    }
    /// Issue once, or adopt only the exact provider client whose private file
    /// already survived a lost journal commit. No secret reaches the web layer.
    pub(super) async fn client(&mut self, peer: &mut BufReader<UnixStream>) -> Result<(), Failure> {
        // A client already saved by an earlier run: nothing to create. (Publication is a
        // separate stage, see `publication`.)
        if self.setup.step == Step::Registered {
            return Ok(());
        }
        if self.setup.step != Step::Client {
            return Err(Failure::Gateway(StatusCode::CONFLICT));
        }
        let inspected = self.private_command(peer, Action::InspectClient).await?;
        let registration = match inspected {
            WorkerResponse::Status(Reply::Absent) => {
                if self.setup.intent {
                    return Err(Failure::Unresolved);
                }
                self.intend(Step::Client).await?;
                match self.private_command(peer, Action::CreateClient).await? {
                    WorkerResponse::Client(client) => Ok(client),
                    _ => return Err(protocol()),
                }
            }
            WorkerResponse::ClientPresent(identity) => {
                if !self.setup.intent {
                    self.intend(Step::Client).await?;
                }
                Err(identity)
            }
            _ => return Err(protocol()),
        };
        // Keep the one-time secret alive across bounded LOCAL save retries.
        // Never send CreateClient again. A timed-out blocking save may still
        // commit, so reconcile the exact file and checkpoint under the lease.
        let registration =
            Arc::new(
                registration.map(|client| moosuri_connectors::gmail::OAuthClient {
                    client_id: client.client_id,
                    client_secret: client.secret,
                    redirect_uri: client.redirect_uri,
                }),
            );
        let result = self.save_registration(registration.clone()).await;
        if result == Err(Failure::SecretUnavailable) {
            let identity = registration.as_ref().as_ref().err().ok_or_else(protocol)?;
            let expected_id = identity.client_id.clone();
            let client_id = expected_id.clone();
            let redirect = self.redirect_uri.clone();
            let run = self.run;
            let reserved = self
                .with_admin(move |root, session| {
                    registry::begin_secret_recovery(root, session, run, &client_id, |directory| {
                        super::super::google_setup::require_bootstrap_absent(directory, &redirect)
                    })
                })
                .await?;
            if !reserved {
                return Err(Failure::SecretUnavailable);
            }
            // The worker still holds the immediately preceding ClientPresent proof.
            // It independently requires exactly one unchanged secret before one Add.
            let recovered = match self
                .private_command(peer, Action::RecoverClientSecret)
                .await?
            {
                WorkerResponse::Client(client) if client.client_id == expected_id => client,
                _ => {
                    self.broken = true;
                    return Err(protocol());
                }
            };
            self.save_registration(Arc::new(Ok(moosuri_connectors::gmail::OAuthClient {
                client_id: recovered.client_id,
                client_secret: recovered.secret,
                redirect_uri: recovered.redirect_uri,
            })))
            .await?;
        } else {
            result?;
        }
        self.setup.step = Step::Registered;
        self.setup.intent = false;
        Ok(())
    }

    /// Bounded local-only save/reconcile. It never sends a provider command.
    async fn save_registration(
        &self,
        registration: Arc<Result<moosuri_connectors::gmail::OAuthClient, wire::ClientIdentity>>,
    ) -> Result<(), Failure> {
        for attempt in 0..3 {
            let run = self.run;
            let redirect = self.redirect_uri.clone();
            let registration = registration.clone();
            let saved = self
                .with_admin(move |root, session| {
                    use super::super::google_setup::{
                        adopt_bootstrap, save_bootstrap, verify_bootstrap,
                    };
                    let missing_file = std::cell::Cell::new(false);
                    let file_result = |result: Result<(), StoreError>| {
                        if matches!(result, Err(StoreError::NotFound)) {
                            missing_file.set(true);
                        }
                        result
                    };
                    let result = registry::register(root, session, run, |directory| {
                        file_result(match registration.as_ref() {
                            Ok(client) => save_bootstrap(directory, &redirect, client),
                            Err(identity) => {
                                adopt_bootstrap(directory, &redirect, &identity.client_id)
                            }
                        })
                    });
                    let result = if matches!(result, Err(StoreError::Conflict)) {
                        registry::reconcile_registration(root, session, run, |directory| {
                            file_result(match registration.as_ref() {
                                Ok(client) => verify_bootstrap(
                                    directory,
                                    &redirect,
                                    &client.client_id,
                                    Some(client.client_secret.expose()),
                                ),
                                Err(identity) => {
                                    adopt_bootstrap(directory, &redirect, &identity.client_id)
                                }
                            })
                        })
                    } else {
                        result
                    };
                    // Only the private-file callback may name a lost secret;
                    // missing registry authority or session HTTP 404 cannot.
                    Ok(registration_result(result, missing_file.get()))
                })
                .await;
            match saved.and_then(|result| result) {
                Ok(()) => break,
                Err(Failure::Gateway(status))
                    if attempt < 2
                        && matches!(
                            status,
                            StatusCode::SERVICE_UNAVAILABLE
                                | StatusCode::BAD_GATEWAY
                                | StatusCode::GATEWAY_TIMEOUT
                                | StatusCode::REQUEST_TIMEOUT
                                | StatusCode::TOO_MANY_REQUESTS
                        ) =>
                {
                    tokio::time::sleep(HEARTBEAT_INTERVAL).await;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Publication prerequisite AFTER the client is saved (`Registered`), including an older
    /// Registered checkpoint that predates the journal. The installation's own public
    /// information URLs, then External / In production, each with its own durable command:
    /// intent before the mutation, a positive exact readback as the only confirmation, and a
    /// lost reply is reconciled by inspection. Only these two idempotent settings may be
    /// resubmitted after a positive Absent readback, with three durable attempts total.
    /// Resource creation remains non-replayable. Needs a fresh admin proof
    /// for every registry step and every worker command, like the stages before it. The step
    /// intent bit (client-creation uncertainty) is neither set nor consumed. This is
    /// publication only: no Google verification is claimed and nothing about the
    /// unverified-app warning, the user cap or billing is touched. The caller must not offer
    /// consent to any member until this returns `Ok`.
    pub(super) async fn publication(
        &mut self,
        peer: &mut BufReader<UnixStream>,
    ) -> Result<(), Failure> {
        if self.setup.step != Step::Registered || self.setup.intent {
            return Err(Failure::Gateway(StatusCode::CONFLICT));
        }
        for (cmd, inspect, configure) in [
            (
                Publication::Domains,
                Action::InspectDomains,
                Action::ConfigureDomains,
            ),
            (
                Publication::Publish,
                Action::InspectPublication,
                Action::ConfigurePublication,
            ),
        ] {
            let seen = self.command(peer, inspect).await?;
            let state = self.publication_state(cmd).await?;
            if state == CommandState::Intent && seen == Reply::Absent {
                let run = self.run;
                if !self
                    .with_admin(move |root, s| registry::retry_publication(root, s, run, cmd))
                    .await?
                {
                    return Err(Failure::Unresolved);
                }
                self.command(peer, configure).await?;
                self.confirm_publication(cmd).await?;
                continue;
            }
            match plan(state, seen)? {
                Plan::Done => {}
                Plan::Settle {
                    intend,
                    configure: send,
                } => {
                    if intend {
                        self.intend_publication(cmd).await?;
                    }
                    if send {
                        self.command(peer, configure).await?;
                    }
                    self.confirm_publication(cmd).await?;
                }
            }
        }
        Ok(())
    }

    async fn publication_state(&self, cmd: Publication) -> Result<CommandState, Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::publication_state(root, s, run, cmd))
            .await
    }
    async fn intend_publication(&self, cmd: Publication) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::intend_publication(root, s, run, cmd))
            .await
    }
    async fn confirm_publication(&self, cmd: Publication) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::confirm_publication(root, s, run, cmd))
            .await
    }

    async fn command_state(&self, cmd: Command) -> Result<CommandState, Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::command_state(root, s, run, cmd))
            .await
    }
    async fn intend_command(&mut self, cmd: Command) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::intend_command(root, s, run, cmd))
            .await?;
        self.setup.intent = true;
        Ok(())
    }
    async fn confirm_command(&self, cmd: Command) -> Result<(), Failure> {
        let run = self.run;
        self.with_admin(move |root, s| registry::confirm_command(root, s, run, cmd))
            .await
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
        match self.private_command(peer, action).await? {
            WorkerResponse::Status(reply) => expected(action, reply),
            _ => {
                self.broken = true;
                Err(protocol())
            }
        }
    }

    async fn private_command(
        &mut self,
        peer: &mut BufReader<UnixStream>,
        action: Action,
    ) -> Result<WorkerResponse, Failure> {
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
    ) -> Result<WorkerResponse, Failure> {
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
        if !reply.answers(&request) {
            return Err(protocol());
        }
        if let WorkerResponse::Status(status) = &reply {
            expected(action, *status)?;
        }
        Ok(reply)
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

fn registration_result(result: Result<(), StoreError>, missing_file: bool) -> Result<(), Failure> {
    match result {
        Ok(()) => Ok(()),
        Err(StoreError::NotFound) if missing_file => Err(Failure::SecretUnavailable),
        Err(error) => Err(Failure::Gateway(super::super::admin::error(error))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_private_file_absence_means_secret_unavailable() {
        assert_eq!(
            registration_result(Err(StoreError::NotFound), true),
            Err(Failure::SecretUnavailable)
        );
        assert_eq!(
            registration_result(Err(StoreError::NotFound), false),
            Err(Failure::Gateway(StatusCode::NOT_FOUND))
        );
        // A marker from an earlier callback cannot mask later authority loss.
        assert_eq!(
            registration_result(Err(StoreError::Forbidden), true),
            Err(Failure::Gateway(StatusCode::FORBIDDEN))
        );
    }

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

    #[test]
    fn console_commands_never_replay_after_uncertainty_or_mutate_a_confirmed_stage() {
        use CommandState::*;
        // Audience inspection may answer absent, like the other inspections.
        assert_eq!(
            expected(Action::InspectAudience, Reply::Absent),
            Ok(Reply::Absent)
        );
        assert_eq!(
            expected(Action::ConfigureAudience, Reply::Absent),
            Err(protocol())
        );
        // Lost reply then positive readback: reconcile without sending Configure.
        assert_eq!(
            plan(Intent, Reply::Ready),
            Ok(Plan::Settle {
                intend: false,
                configure: false
            })
        );
        // Uncertain intent with provider state still absent: refuse replay.
        assert_eq!(plan(Intent, Reply::Absent), Err(Failure::Unresolved));
        // A never-attempted next command is submitted exactly once, intent first.
        assert_eq!(
            plan(Untried, Reply::Absent),
            Ok(Plan::Settle {
                intend: true,
                configure: true
            })
        );
        // A confirmed command is inspect-only; its absence is never repaired.
        assert_eq!(plan(Confirmed, Reply::Ready), Ok(Plan::Done));
        assert_eq!(plan(Confirmed, Reply::Absent), Err(Failure::Unresolved));
    }

    #[tokio::test]
    async fn console_refuses_before_apis_and_sends_nothing() {
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
            step: Step::Apis,
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
        assert_eq!(
            c.console(&mut peer).await,
            Err(Failure::Gateway(StatusCode::CONFLICT))
        );
        let mut byte = [0u8; 1];
        assert!(
            timeout(Duration::from_millis(50), worker.read(&mut byte))
                .await
                .is_err()
        );
        assert!(!c.broken);
    }

    #[tokio::test]
    async fn private_client_creation_recovery_and_missing_secret_keep_one_identity() {
        let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
        use moosuri_connectors::{
            browser::google_auth::bootstrap::{ClientIdentity, ClientRegistration},
            gmail::SecretString,
        };
        use moosuri_store::runtime_registry;
        // These drive real gateway session reauthorization and the registry +
        // private atomic writer; only the Console worker's answers are synthetic.
        for scenario in [
            "create",
            "transient_save",
            "lost_save_ack",
            "saved_before_crash",
            "secret_lost",
            "recovery_competing_save",
            "recovery_refused",
            "recovery_wrong_client",
            "recovery_lost_reply",
            "uncertain_absent",
            "logout",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let options = super::super::super::tests::options(&root);
            let email = "admin@example.test";
            let (allocation, store, member_task) =
                super::super::super::tests::real_member(&options, email).await;
            let login = store
                .login(email.into(), "Synthetic-password-42".into())
                .await
                .unwrap();
            let g = Gateway::new(&options).unwrap();
            let a = SessionAllocation {
                allocation,
                epoch: 0,
            };
            let (_, bytes, _) = g
                .proxy(
                    &a,
                    Method::GET,
                    "/api/v1/auth/session",
                    None,
                    Some(&login.token),
                )
                .await
                .unwrap();
            let view = member_session_view(&bytes, &a).unwrap();
            control::bootstrap(&options.registry, Uuid::new_v4(), &a, &view).unwrap();
            let pin = control::checked_session(&options.registry, &a, &view)
                .unwrap()
                .at_epoch(0)
                .unwrap();
            let run = Uuid::new_v4();
            let redirect = format!("{}{}", g.origin, google::CALLBACK);
            let original = registry::claim(
                &options.registry,
                &pin,
                run,
                "google@example.test".into(),
                redirect.clone(),
            )
            .unwrap();
            for step in [Step::Project, Step::Apis] {
                registry::intend(&options.registry, &pin, run, step).unwrap();
                registry::confirmed(&options.registry, &pin, run, step).unwrap();
            }
            for (cmd, done) in [
                (Command::Branding, Some(Step::Branding)),
                (Command::Audience, None),
                (Command::Scopes, Some(Step::Audience)),
            ] {
                registry::intend_command(&options.registry, &pin, run, cmd).unwrap();
                registry::confirm_command(&options.registry, &pin, run, cmd).unwrap();
                if let Some(step) = done {
                    registry::confirmed(&options.registry, &pin, run, step).unwrap();
                }
            }
            if matches!(
                scenario,
                "saved_before_crash"
                    | "secret_lost"
                    | "uncertain_absent"
                    | "recovery_competing_save"
                    | "recovery_refused"
                    | "recovery_wrong_client"
                    | "recovery_lost_reply"
            ) {
                registry::intend(&options.registry, &pin, run, Step::Client).unwrap();
            }
            let identity = ClientIdentity {
                setup_id: original.setup_id,
                client_id: "1234567890-synthetic.apps.googleusercontent.com".into(),
                redirect_uri: redirect.clone(),
            };
            if scenario == "saved_before_crash" {
                control::with_google_setup(&options.registry, &pin, |dir| {
                    super::super::super::google_setup::save_bootstrap(
                        dir,
                        &redirect,
                        &moosuri_connectors::gmail::OAuthClient {
                            client_id: identity.client_id.clone(),
                            client_secret: SecretString::new("GOCSPX-synthetic-only".into())
                                .unwrap(),
                            redirect_uri: redirect.clone(),
                        },
                    )
                })
                .unwrap()
                .unwrap();
            }
            let setup = registry::current(&options.registry, &pin).unwrap().unwrap();
            let mut controller =
                Controller::new(&g, &a, &login.token, email, run, redirect.clone(), setup);
            let (worker, gateway) = UnixStream::pair().unwrap();
            let worker_store = store.clone();
            let worker_user = login.user;
            let worker_root = options.registry.clone();
            let worker_pin = control::checked_session(&worker_root, &a, &view)
                .unwrap()
                .at_epoch(0)
                .unwrap();
            let worker_task = tokio::spawn(async move {
                let mut peer = BufReader::new(worker);
                let mut competing_file = None;
                let mut line = String::new();
                peer.read_line(&mut line).await.unwrap();
                let action: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(action["bootstrap"]["action"], "inspect_client");
                let seen = if matches!(
                    scenario,
                    "create" | "transient_save" | "lost_save_ack" | "uncertain_absent" | "logout"
                ) {
                    WorkerResponse::Status(Reply::Absent)
                } else {
                    WorkerResponse::ClientPresent(identity.clone())
                };
                if scenario == "logout" {
                    worker_store.logout(&worker_user).await.unwrap();
                }
                peer.get_mut()
                    .write_all(&seen.to_line().unwrap())
                    .await
                    .unwrap();
                if matches!(scenario, "create" | "transient_save" | "lost_save_ack") {
                    line.clear();
                    peer.read_line(&mut line).await.unwrap();
                    let action: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(action["bootstrap"]["action"], "create_client");
                    let pending =
                        worker_root.join(runtime_registry::GOOGLE_WEB_CLIENT_PENDING_FILE);
                    if scenario == "transient_save" {
                        // A transient filesystem failure after provider creation.
                        // Repair it before the bounded LOCAL retry; no second
                        // provider action is accepted by this worker fixture.
                        std::fs::write(&pending, b"synthetic-invalid-staging-file").unwrap();
                        std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o644))
                            .unwrap();
                    }
                    if scenario == "lost_save_ack" {
                        registry::register(&worker_root, &worker_pin, run, |directory| {
                            super::super::super::google_setup::save_bootstrap(
                                directory,
                                &identity.redirect_uri,
                                &moosuri_connectors::gmail::OAuthClient {
                                    client_id: identity.client_id.clone(),
                                    client_secret: SecretString::new(
                                        "GOCSPX-synthetic-only".into(),
                                    )
                                    .unwrap(),
                                    redirect_uri: identity.redirect_uri.clone(),
                                },
                            )
                        })
                        .unwrap();
                    }
                    let reply = WorkerResponse::Client(ClientRegistration::new(
                        identity.clone(),
                        SecretString::new("GOCSPX-synthetic-only".into()).unwrap(),
                    ));
                    peer.get_mut()
                        .write_all(&reply.to_line().unwrap())
                        .await
                        .unwrap();
                    if scenario == "transient_save" {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        std::fs::remove_file(pending).unwrap();
                    }
                }
                if matches!(
                    scenario,
                    "secret_lost"
                        | "recovery_competing_save"
                        | "recovery_refused"
                        | "recovery_wrong_client"
                        | "recovery_lost_reply"
                ) {
                    line.clear();
                    peer.read_line(&mut line).await.unwrap();
                    let action: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(action["bootstrap"]["action"], "recover_client_secret");
                    assert!(
                        !worker_root
                            .join(runtime_registry::GOOGLE_WEB_CLIENT_FILE)
                            .exists()
                    );
                    if scenario == "recovery_competing_save" {
                        // Another authorized save lands after the absent-file reservation,
                        // before the recovered response arrives. Its bytes must win unchanged.
                        control::with_google_setup(&worker_root, &worker_pin, |directory| {
                            super::super::super::google_setup::save_bootstrap(
                                directory,
                                &identity.redirect_uri,
                                &moosuri_connectors::gmail::OAuthClient {
                                    client_id: identity.client_id.clone(),
                                    client_secret: SecretString::new(
                                        "GOCSPX-synthetic-other-admin".into(),
                                    )
                                    .unwrap(),
                                    redirect_uri: identity.redirect_uri.clone(),
                                },
                            )
                        })
                        .unwrap()
                        .unwrap();
                        competing_file = Some(
                            std::fs::read(
                                worker_root.join(runtime_registry::GOOGLE_WEB_CLIENT_FILE),
                            )
                            .unwrap(),
                        );
                    }
                    if scenario == "recovery_lost_reply" {
                        return;
                    }
                    if scenario == "recovery_refused" {
                        peer.get_mut()
                            .write_all(b"{\"status\":\"needs_human\"}\n")
                            .await
                            .unwrap();
                    } else {
                        let mut recovered_identity = identity;
                        if scenario == "recovery_wrong_client" {
                            recovered_identity.client_id =
                                "987-unrelated.apps.googleusercontent.com".into();
                        }
                        let reply = WorkerResponse::Client(ClientRegistration::new(
                            recovered_identity,
                            SecretString::new("GOCSPX-synthetic-recovered".into()).unwrap(),
                        ));
                        peer.get_mut()
                            .write_all(&reply.to_line().unwrap())
                            .await
                            .unwrap();
                    }
                }
                // A failed/adopted inspection or recovery must not emit another write.
                line.clear();
                assert_eq!(peer.read_line(&mut line).await.unwrap(), 0);
                if let Some(bytes) = competing_file {
                    assert_eq!(
                        std::fs::read(worker_root.join(runtime_registry::GOOGLE_WEB_CLIENT_FILE))
                            .unwrap(),
                        bytes
                    );
                }
            });
            let mut peer = BufReader::new(gateway);
            let result = controller.client(&mut peer).await;
            match scenario {
                "create" | "transient_save" | "lost_save_ack" | "saved_before_crash"
                | "secret_lost" => {
                    assert_eq!(result, Ok(()))
                }
                "recovery_competing_save" => {
                    assert_eq!(result, Err(Failure::Gateway(StatusCode::CONFLICT)))
                }
                "recovery_refused" => {
                    assert_eq!(result, Err(Failure::Provider(Terminal::NeedsHuman)))
                }
                "recovery_wrong_client" | "recovery_lost_reply" => {
                    assert_eq!(result, Err(protocol()))
                }
                "uncertain_absent" => assert_eq!(result, Err(Failure::Unresolved)),
                "logout" => assert_eq!(result, Err(Failure::Gateway(StatusCode::UNAUTHORIZED))),
                _ => unreachable!(),
            }
            drop(peer);
            worker_task.await.unwrap();
            if scenario != "logout" {
                let saved = registry::current(&options.registry, &pin).unwrap().unwrap();
                assert_eq!(saved.setup_id, original.setup_id);
                assert_eq!(
                    saved.step,
                    if matches!(
                        scenario,
                        "create"
                            | "transient_save"
                            | "lost_save_ack"
                            | "saved_before_crash"
                            | "secret_lost"
                    ) {
                        Step::Registered
                    } else {
                        Step::Client
                    }
                );
                let client = options
                    .registry
                    .join(runtime_registry::GOOGLE_WEB_CLIENT_FILE);
                assert_eq!(
                    client.exists(),
                    matches!(
                        scenario,
                        "recovery_competing_save"
                            | "create"
                            | "transient_save"
                            | "lost_save_ack"
                            | "saved_before_crash"
                            | "secret_lost"
                    )
                );
                if client.exists() {
                    assert_eq!(std::fs::metadata(client).unwrap().mode() & 0o7777, 0o600);
                }
            }
            member_task.abort();
        }
    }

    #[tokio::test]
    async fn publication_refuses_before_a_saved_client_and_sends_nothing() {
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
        // Client step: a lost-client intent must stay untouched and nothing may be sent.
        let setup = Setup {
            setup_id: Uuid::new_v4(),
            google_email: "google@example.test".into(),
            redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
            step: Step::Client,
            intent: true,
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
        assert_eq!(
            c.publication(&mut peer).await,
            Err(Failure::Gateway(StatusCode::CONFLICT))
        );
        assert!(c.setup.intent && !c.broken);
        let mut byte = [0u8; 1];
        assert!(
            timeout(Duration::from_millis(50), worker.read(&mut byte))
                .await
                .is_err()
        );
        // Domains and publication absence may be answered by an inspection only.
        for action in [Action::InspectDomains, Action::InspectPublication] {
            assert_eq!(expected(action, Reply::Absent), Ok(Reply::Absent));
        }
        for action in [Action::ConfigureDomains, Action::ConfigurePublication] {
            assert_eq!(expected(action, Reply::Absent), Err(protocol()));
        }
    }

    #[tokio::test]
    async fn publication_runs_after_saved_client_and_retries_only_absent_idempotent_settings() {
        let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
        // Real gateway session reauthorization and the real registry journal; only the
        // Console worker's answers are synthetic (NOT a live Google result).
        #[derive(Clone, Copy)]
        enum W {
            Ready,
            Absent,
            NeedsHuman,
        }
        use W::*;
        // (scenario, journal prepared before the run, worker script, expected result)
        type Scenario = (
            &'static str,
            &'static [(Publication, bool)],
            &'static [(&'static str, W)],
            Result<(), Failure>,
        );
        let scenarios: [Scenario; 9] = [
            // Old Registered checkpoint with no journal at all: both are performed, in order.
            (
                "old_registered",
                &[],
                &[
                    ("inspect_domains", Absent),
                    ("configure_domains", Ready),
                    ("inspect_publication", Absent),
                    ("configure_publication", Ready),
                ],
                Ok(()),
            ),
            // Already In production with a lost confirmation: positive readback, no click.
            (
                "production_resume",
                &[(Publication::Domains, true), (Publication::Publish, false)],
                &[("inspect_domains", Ready), ("inspect_publication", Ready)],
                Ok(()),
            ),
            // Lost save reply, page definitively blank: reserve one idempotent retry.
            (
                "lost_domains_reply",
                &[(Publication::Domains, false)],
                &[
                    ("inspect_domains", Absent),
                    ("configure_domains", Ready),
                    ("inspect_publication", Absent),
                    ("configure_publication", Ready),
                ],
                Ok(()),
            ),
            // Lost publish reply, definitively Testing: bounded idempotent retry.
            (
                "lost_publish_reply",
                &[(Publication::Domains, true), (Publication::Publish, false)],
                &[
                    ("inspect_domains", Ready),
                    ("inspect_publication", Absent),
                    ("configure_publication", Ready),
                ],
                Ok(()),
            ),
            (
                "exhausted",
                &[(Publication::Domains, true), (Publication::Publish, false)],
                &[("inspect_domains", Ready), ("inspect_publication", Absent)],
                Err(Failure::Unresolved),
            ),
            (
                "confirmed_drift",
                &[(Publication::Domains, true), (Publication::Publish, true)],
                &[("inspect_domains", Ready), ("inspect_publication", Absent)],
                Err(Failure::Unresolved),
            ),
            // Publish action disabled (prerequisite missing): truthful human result.
            (
                "disabled_prerequisite",
                &[(Publication::Domains, true)],
                &[
                    ("inspect_domains", Ready),
                    ("inspect_publication", NeedsHuman),
                ],
                Err(Failure::Provider(Terminal::NeedsHuman)),
            ),
            // Fully confirmed journal and production: inspect only.
            (
                "idempotent",
                &[(Publication::Domains, true), (Publication::Publish, true)],
                &[("inspect_domains", Ready), ("inspect_publication", Ready)],
                Ok(()),
            ),
            // Authority lost between the worker answer and the journal write.
            (
                "logout",
                &[],
                &[("inspect_domains", Absent)],
                Err(Failure::Gateway(StatusCode::UNAUTHORIZED)),
            ),
        ];
        for (scenario, journal, script, wanted) in scenarios {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let options = super::super::super::tests::options(&root);
            let email = "admin@example.test";
            let (allocation, store, member_task) =
                super::super::super::tests::real_member(&options, email).await;
            let login = store
                .login(email.into(), "Synthetic-password-42".into())
                .await
                .unwrap();
            let g = Gateway::new(&options).unwrap();
            let a = SessionAllocation {
                allocation,
                epoch: 0,
            };
            let (_, bytes, _) = g
                .proxy(
                    &a,
                    Method::GET,
                    "/api/v1/auth/session",
                    None,
                    Some(&login.token),
                )
                .await
                .unwrap();
            let view = member_session_view(&bytes, &a).unwrap();
            control::bootstrap(&options.registry, Uuid::new_v4(), &a, &view).unwrap();
            let pin = control::checked_session(&options.registry, &a, &view)
                .unwrap()
                .at_epoch(0)
                .unwrap();
            let run = Uuid::new_v4();
            let redirect = format!("{}{}", g.origin, google::CALLBACK);
            let original = registry::claim(
                &options.registry,
                &pin,
                run,
                "google@example.test".into(),
                redirect.clone(),
            )
            .unwrap();
            for step in [Step::Project, Step::Apis] {
                registry::intend(&options.registry, &pin, run, step).unwrap();
                registry::confirmed(&options.registry, &pin, run, step).unwrap();
            }
            for (cmd, done) in [
                (Command::Branding, Some(Step::Branding)),
                (Command::Audience, None),
                (Command::Scopes, Some(Step::Audience)),
            ] {
                registry::intend_command(&options.registry, &pin, run, cmd).unwrap();
                registry::confirm_command(&options.registry, &pin, run, cmd).unwrap();
                if let Some(step) = done {
                    registry::confirmed(&options.registry, &pin, run, step).unwrap();
                }
            }
            registry::intend(&options.registry, &pin, run, Step::Client).unwrap();
            registry::register(&options.registry, &pin, run, |_| Ok(())).unwrap();
            // The saved client is not publication, for the registry or the start gate.
            assert!(registry::publication_pending(&options.registry).unwrap());
            for (cmd, confirmed) in journal {
                registry::intend_publication(&options.registry, &pin, run, *cmd).unwrap();
                if *confirmed {
                    registry::confirm_publication(&options.registry, &pin, run, *cmd).unwrap();
                }
            }
            if scenario == "exhausted" {
                assert!(
                    registry::retry_publication(&options.registry, &pin, run, Publication::Publish)
                        .unwrap()
                );
                assert!(
                    registry::retry_publication(&options.registry, &pin, run, Publication::Publish)
                        .unwrap()
                );
            }
            let setup = registry::current(&options.registry, &pin).unwrap().unwrap();
            assert_eq!((setup.step, setup.intent), (Step::Registered, false));
            let mut controller =
                Controller::new(&g, &a, &login.token, email, run, redirect.clone(), setup);
            let (worker, gateway) = UnixStream::pair().unwrap();
            let worker_store = store.clone();
            let worker_user = login.user;
            let worker_task = tokio::spawn(async move {
                let mut peer = BufReader::new(worker);
                let mut line = String::new();
                for (action, reply) in script {
                    line.clear();
                    peer.read_line(&mut line).await.unwrap();
                    let sent: Value = serde_json::from_str(&line).unwrap();
                    assert_eq!(sent["bootstrap"]["action"], *action);
                    if scenario == "logout" {
                        worker_store.logout(&worker_user).await.unwrap();
                    }
                    let bytes = match reply {
                        Ready => WorkerResponse::Status(Reply::Ready)
                            .to_line()
                            .unwrap()
                            .to_vec(),
                        Absent => WorkerResponse::Status(Reply::Absent)
                            .to_line()
                            .unwrap()
                            .to_vec(),
                        NeedsHuman => b"{\"status\":\"needs_human\"}\n".to_vec(),
                    };
                    peer.get_mut().write_all(&bytes).await.unwrap();
                }
                // Nothing beyond the exact script: no unbounded Configure retries.
                line.clear();
                assert_eq!(peer.read_line(&mut line).await.unwrap(), 0, "{scenario}");
            });
            let mut peer = BufReader::new(gateway);
            let result = controller.publication(&mut peer).await;
            assert_eq!(result, wanted, "{scenario}");
            drop(peer);
            worker_task.await.unwrap();
            // Observation without a session (the member start gate): pending until the
            // journal confirms publication, whatever happened above.
            let published = matches!(
                scenario,
                "old_registered"
                    | "production_resume"
                    | "idempotent"
                    | "lost_domains_reply"
                    | "lost_publish_reply"
                    | "confirmed_drift"
            );
            assert_eq!(
                registry::publication_pending(&options.registry).unwrap(),
                !published,
                "{scenario}"
            );
            if scenario != "logout" {
                let after = registry::current(&options.registry, &pin).unwrap().unwrap();
                assert_eq!(after.setup_id, original.setup_id);
                assert_eq!((after.step, after.intent), (Step::Registered, false));
                assert_eq!(
                    registry::publication_confirmed(&options.registry, &pin).unwrap(),
                    published,
                    "{scenario}"
                );
                let state =
                    |cmd| registry::publication_state(&options.registry, &pin, run, cmd).unwrap();
                match scenario {
                    "lost_domains_reply" => {
                        assert_eq!(state(Publication::Domains), CommandState::Confirmed);
                        assert_eq!(state(Publication::Publish), CommandState::Confirmed);
                    }
                    "lost_publish_reply" => {
                        assert_eq!(state(Publication::Publish), CommandState::Confirmed);
                    }
                    "disabled_prerequisite" => {
                        // Inspection failed before any intent: still never attempted.
                        assert_eq!(state(Publication::Publish), CommandState::Untried);
                    }
                    _ => {}
                }
            }
            member_task.abort();
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
        assert!(matches!(
            read_reply(&mut peer).await,
            Ok(WorkerResponse::Status(Reply::Absent))
        ));
        worker
            .write_all(b"{\"bootstrap\":\"ready\"}")
            .await
            .unwrap();
        drop(worker);
        assert!(matches!(read_reply(&mut peer).await, Err(error) if error == protocol()));

        let (mut worker, controller) = UnixStream::pair().unwrap();
        let mut peer = BufReader::new(controller);
        let mut long = vec![b' '; MAX_REPLY];
        long.push(b'\n');
        worker.write_all(&long).await.unwrap();
        assert!(matches!(read_reply(&mut peer).await, Err(error) if error == protocol()));
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
