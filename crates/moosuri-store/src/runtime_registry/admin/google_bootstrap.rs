//! Durable, installation-scoped *intent* checkpoint for creating the one shared Google
//! OAuth app (new unbilled project -> APIs -> branding -> audience -> web client).
//!
//! This registry is only an intent store. A row never proves that Google completed anything:
//! `intent=true` means "a provider write may have been sent"; a confirmed step means only that
//! the controller observed the result and said so. The controller must verify, before it
//! confirms `Project`, that the provider project has the stable id from `Setup::project_id`,
//! is ACTIVE and has billing disabled. Nothing here records or links a billing account.
//!
//! The Branding and Audience steps each cover several provider mutations, so migration 9
//! journals them per `Command` (branding, audience, scopes: intent -> confirmed, same setup,
//! lease and admin checks). The step bit alone is never read as proof of any one of them.
//!
//! The registry cannot read credentials. If the process dies between the private client-file
//! commit and the checkpoint commit, the checkpoint stays at `Client` with intent set; the
//! stable `Setup::client_name` lets the gateway reconcile the provider client and adopt the
//! already-written private file. There is deliberately no reset: discarding the identity
//! would duplicate or orphan provider resources.
//!
//! Authority: a lease is never an authorization. Every operation re-checks the administrator
//! pin (`check`) *and* the per-run lease inside the same `Immediate` transaction as its
//! mutation. `open` also holds the exclusive registry-directory lock for the whole call, so
//! a revocation or successor claim cannot interleave between check and write.
use super::*;
use rusqlite::Transaction;

/// A lost transport leaves this fence for the old browser; it is released early only by
/// `release`, which the caller invokes after exact capsule teardown is acknowledged.
///
/// Parent contract: while the SDK or Console worker runs, the parent MUST call `heartbeat`
/// with a fresh (<=5s) `AdminSession` proof well inside this window and MUST cancel the
/// worker as soon as a heartbeat fails. Successful `intend`/`confirmed` also renew the lease,
/// but a renewable lease is only a fence against a *successor claim*; it is never proof that
/// an old external write has ended. The window is intentionally not lengthened to mask a
/// failed heartbeat.
const LEASE_SECONDS: i64 = 180;

/// Renew the caller's own lease inside the same transaction as its mutation.
fn renew(tx: &Transaction<'_>, now: i64) -> Result<()> {
    tx.execute(
        "UPDATE installation_google_bootstrap SET lease_until=?1 WHERE singleton=1",
        [now + LEASE_SECONDS],
    )?;
    Ok(())
}

/// Order matters: Project -> Apis -> Branding -> Audience -> Client -> Registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Project,
    Apis,
    Branding,
    Audience,
    Client,
    Registered,
}
impl Step {
    fn name(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Apis => "apis",
            Self::Branding => "branding",
            Self::Audience => "audience",
            Self::Client => "client",
            Self::Registered => "registered",
        }
    }
    fn next(self) -> Option<Self> {
        match self {
            Self::Project => Some(Self::Apis),
            Self::Apis => Some(Self::Branding),
            Self::Branding => Some(Self::Audience),
            Self::Audience => Some(Self::Client),
            // Registered is reached only through `register`, after the private file commit.
            Self::Client | Self::Registered => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub setup_id: Uuid,
    pub google_email: String,
    pub redirect_uri: String,
    pub step: Step,
    /// May have reached Google even if no response was received. Reconcile first.
    pub intent: bool,
}
impl Setup {
    /// Stable for the life of the installation; never regenerated on restart.
    pub fn project_id(&self) -> String {
        format!("moosuri-{}", &self.setup_id.simple().to_string()[..20])
    }
    pub fn client_name(&self) -> String {
        format!("Moosuri {}", self.setup_id.simple())
    }
}

fn read(c: &Connection) -> Result<Option<Setup>> {
    let raw: Option<(String, String, String, String, bool)> = c
        .query_row(
            "SELECT setup_id,google_email,redirect_uri,step,intent FROM installation_google_bootstrap WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    raw.map(|(id, email, redirect, step, intent)| {
        Ok(Setup {
            setup_id: Uuid::parse_str(&id).map_err(|_| StoreError::Corrupt)?,
            google_email: email,
            redirect_uri: redirect,
            step: serde_json::from_value(serde_json::Value::String(step))
                .map_err(|_| StoreError::Corrupt)?,
            intent,
        })
    })
    .transpose()
}

fn valid_callback(uri: &str) -> bool {
    uri.len() <= 2048
        && url::Url::parse(uri).is_ok_and(|u| {
            u.as_str() == uri
                && u.scheme() == "https"
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
                && u.query().is_none()
                && u.fragment().is_none()
                && u.path() == "/api/v1/mail/oauth/callback"
        })
}

/// Run `act` in one Immediate transaction after re-checking the administrator pin and the
/// caller's own unexpired lease. `dir` is the locked registry directory handle.
fn leased<T>(
    root: &Path,
    s: &AdminSession,
    run: Uuid,
    act: impl FnOnce(&Transaction<'_>, &fs::File, i64) -> Result<T>,
) -> Result<T> {
    let (dir, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check(&tx, s)?;
    let now = crate::now()?;
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM installation_google_bootstrap WHERE singleton=1 AND lease_id=?1 AND lease_account=?2 AND lease_generation=?3 AND lease_revision=?4 AND lease_epoch=?5 AND lease_until>?6)",
        params![run.to_string(), s.account, s.generation, s.revision, s.epoch, now],
        |r| r.get(0),
    )?;
    if !owned {
        return Err(StoreError::Conflict);
    }
    let out = act(&tx, &dir, now)?;
    tx.commit()?;
    Ok(out)
}

/// Read the checkpoint for an authorized administrator without taking a lease. A completed
/// (`Registered`) installation can be observed this way without any provider call.
pub fn current(root: &Path, s: &AdminSession) -> Result<Option<Setup>> {
    let (_dir, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check(&tx, s)?;
    read(&tx)
}

/// `run` is caller-generated, identifies one worker, and is persisted so it can never be
/// reused. The project identity is committed before the first Console request and survives
/// restarts; a second claim while another lease is live is refused. A changed Google account
/// or callback is refused too, except for a claim after the old lease ended while the
/// checkpoint is still `Project` with no intent (nothing was written): that corrects the
/// identity and keeps the same setup id. Use `current` for a completed installation;
/// `claim` always reserves a new run, including for a Registered checkpoint.
pub fn claim(
    root: &Path,
    s: &AdminSession,
    run: Uuid,
    email: String,
    callback: String,
) -> Result<Setup> {
    if run.is_nil() || !valid_callback(&callback) {
        return Err(StoreError::InvalidInput);
    }
    let email = email_valid(email)?;
    let (_dir, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check(&tx, s)?;
    let now = crate::now()?;
    let reused: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM installation_google_bootstrap_runs WHERE run_id=?1) OR EXISTS(SELECT 1 FROM installation_google_bootstrap_sdk_closed WHERE run_id=?1)",
        [run.to_string()],
        |r| r.get(0),
    )?;
    if reused {
        return Err(StoreError::Conflict);
    }
    let run = run.to_string();
    let until = now + LEASE_SECONDS;
    if let Some(prior) = read(&tx)? {
        let busy: bool = tx.query_row(
            "SELECT lease_until>?1 FROM installation_google_bootstrap WHERE singleton=1",
            [now],
            |r| r.get(0),
        )?;
        if busy {
            return Err(StoreError::Conflict);
        }
        // An unresolved temporary SDK grant outlives the ordinary lease.
        let pending: bool = tx.query_row(
            "SELECT sdk_run IS NOT NULL FROM installation_google_bootstrap WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if pending {
            return Err(StoreError::Conflict);
        }
        // Identity may be corrected only before any provider write was even attempted
        // (Project, no intent), and only by a new claim after the old lease ended. The
        // setup id, hence project id and client name, never changes.
        let changed = prior.google_email != email || prior.redirect_uri != callback;
        if changed && (prior.step != Step::Project || prior.intent) {
            return Err(StoreError::Conflict);
        }
        tx.execute(
            "UPDATE installation_google_bootstrap SET google_email=?7,redirect_uri=?8,lease_id=?1,lease_account=?2,lease_generation=?3,lease_revision=?4,lease_epoch=?5,lease_until=?6 WHERE singleton=1",
            params![run, s.account, s.generation, s.revision, s.epoch, until, email, callback],
        )?;
    } else {
        tx.execute(
            "INSERT INTO installation_google_bootstrap(singleton,setup_id,google_email,redirect_uri,step,intent,lease_id,lease_account,lease_generation,lease_revision,lease_epoch,lease_until) VALUES(1,?1,?2,?3,'project',0,?4,?5,?6,?7,?8,?9)",
            params![Uuid::new_v4().to_string(), email, callback, run, s.account, s.generation, s.revision, s.epoch, until],
        )?;
    }
    tx.execute(
        "INSERT INTO installation_google_bootstrap_runs(run_id) VALUES(?1)",
        [&run],
    )?;
    let result = read(&tx)?.ok_or(StoreError::Unavailable)?;
    tx.commit()?;
    Ok(result)
}

/// Renew while the private session is re-checked, including human waits.
pub fn heartbeat(root: &Path, s: &AdminSession, run: Uuid) -> Result<()> {
    leased(root, s, run, |tx, _, now| renew(tx, now))
}

/// Compare-and-set the begin-step marker before any provider write for `step`. A prior intent
/// is never read as permission to repeat creation; the caller reconciles the stable identity.
/// For `Project`, the controller must already have verified that the provider project is
/// identified by `project_id`, ACTIVE and unbilled before it later confirms the step.
pub fn intend(root: &Path, s: &AdminSession, run: Uuid, step: Step) -> Result<Setup> {
    if matches!(step, Step::Branding | Step::Audience | Step::Registered) {
        return Err(StoreError::InvalidInput);
    }
    leased(root, s, run, |tx, _, now| {
        if tx.execute(
            "UPDATE installation_google_bootstrap SET intent=1 WHERE singleton=1 AND step=?1 AND intent=0",
            [step.name()],
        )? != 1
        {
            return Err(StoreError::Conflict);
        }
        renew(tx, now)?;
        read(tx)?.ok_or(StoreError::Unavailable)
    })
}

/// Record that the controller observed `step` complete at the provider. Idempotent for a step
/// already confirmed; cannot skip a step or confirm one that was never begun.
pub fn confirmed(root: &Path, s: &AdminSession, run: Uuid, step: Step) -> Result<()> {
    let next = step.next().ok_or(StoreError::InvalidInput)?;
    leased(root, s, run, |tx, _, now| {
        let current = read(tx)?;
        if current
            .as_ref()
            .is_some_and(|c| c.step == next && !c.intent)
        {
            return renew(tx, now);
        }
        // The single historical bit cannot prove several distinct Console mutations: every
        // command of the stage needs its own confirmed readback before the stage advances.
        if let Some(c) = current.as_ref().filter(|c| c.step == step) {
            for cmd in Command::of(step) {
                if effective(tx, c, *cmd)? != CommandState::Confirmed {
                    return Err(StoreError::Conflict);
                }
            }
        }
        if tx.execute(
            "UPDATE installation_google_bootstrap SET step=?1,intent=0 WHERE singleton=1 AND step=?2 AND intent=1",
            params![next.name(), step.name()],
        )? != 1
        {
            return Err(StoreError::Conflict);
        }
        renew(tx, now)
    })
}

/// One provider-mutating Console command. Closed set: no caller-supplied names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Branding,
    /// Initiating account as Google test user (External/Testing).
    Audience,
    /// Exact Data Access scope set.
    Scopes,
}
impl Command {
    fn name(self) -> &'static str {
        match self {
            Self::Branding => "branding",
            Self::Audience => "audience",
            Self::Scopes => "scopes",
        }
    }
    fn home(self) -> Step {
        match self {
            Self::Branding => Step::Branding,
            Self::Audience | Self::Scopes => Step::Audience,
        }
    }
    /// The commands whose confirmed readbacks together complete a top-level step, in order.
    fn of(step: Step) -> &'static [Command] {
        match step {
            Step::Branding => &[Self::Branding],
            Step::Audience => &[Self::Audience, Self::Scopes],
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandState {
    /// Never attempted: the first submission is permitted.
    Untried,
    /// A provider write may have reached Google. Only a positive readback resolves it.
    Intent,
    Confirmed,
}

/// Effective state. A row is authoritative. Without one: a past stage was confirmed by step
/// progression; a migration-8 checkpoint (stage intent set, no command rows at all) is
/// conservatively uncertain for EVERY command of that stage (the old bit never said which
/// mutation was sent, so it is never aliased to one command); otherwise never attempted.
fn effective(c: &Connection, setup: &Setup, cmd: Command) -> Result<CommandState> {
    let mut q = c.prepare(
        "SELECT command,state FROM installation_google_console_commands WHERE setup_id=?1",
    )?;
    let rows = q
        .query_map([setup.setup_id.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if let Some((_, state)) = rows.iter().find(|(n, _)| n == cmd.name()) {
        return match state.as_str() {
            "intent" => Ok(CommandState::Intent),
            "confirmed" => Ok(CommandState::Confirmed),
            _ => Err(StoreError::Corrupt),
        };
    }
    Ok(if setup.step > cmd.home() {
        CommandState::Confirmed
    } else if !rows
        .iter()
        .any(|(name, _)| Command::of(cmd.home()).iter().any(|c| c.name() == name))
        && setup.step == cmd.home()
        && setup.intent
    {
        CommandState::Intent
    } else {
        CommandState::Untried
    })
}

/// Current state of `cmd` for the claimed run (fresh admin pin + lease, renews the lease).
pub fn command_state(
    root: &Path,
    s: &AdminSession,
    run: Uuid,
    cmd: Command,
) -> Result<CommandState> {
    leased(root, s, run, |tx, _, now| {
        let setup = read(tx)?.ok_or(StoreError::Unavailable)?;
        let state = effective(tx, &setup, cmd)?;
        renew(tx, now)?;
        Ok(state)
    })
}

/// Durable intent BEFORE the worker is told to mutate. Only a never-attempted command at its
/// own stage may begin; Scopes additionally needs Audience confirmed. Also sets the stage
/// intent bit atomically, so a lone bit with no rows can only be a migration-8 checkpoint.
pub fn intend_command(root: &Path, s: &AdminSession, run: Uuid, cmd: Command) -> Result<()> {
    leased(root, s, run, |tx, _, now| {
        let setup = read(tx)?.ok_or(StoreError::Unavailable)?;
        if setup.step != cmd.home()
            || effective(tx, &setup, cmd)? != CommandState::Untried
            || (cmd == Command::Scopes
                && effective(tx, &setup, Command::Audience)? != CommandState::Confirmed)
        {
            return Err(StoreError::Conflict);
        }
        tx.execute(
            "UPDATE installation_google_bootstrap SET intent=1 WHERE singleton=1 AND step=?1",
            [cmd.home().name()],
        )?;
        tx.execute(
            "INSERT INTO installation_google_console_commands(setup_id,command,state) VALUES(?1,?2,'intent')",
            params![setup.setup_id.to_string(), cmd.name()],
        )?;
        renew(tx, now)
    })
}

/// Record a positive exact readback. Resolves an uncertain intent (including a migration-8
/// one); idempotent when already confirmed; refused for a command never begun.
pub fn confirm_command(root: &Path, s: &AdminSession, run: Uuid, cmd: Command) -> Result<()> {
    leased(root, s, run, |tx, _, now| {
        let setup = read(tx)?.ok_or(StoreError::Unavailable)?;
        match effective(tx, &setup, cmd)? {
            CommandState::Confirmed => return renew(tx, now),
            CommandState::Untried => return Err(StoreError::Conflict),
            CommandState::Intent => {}
        }
        // Only an implicit legacy intent covers every sibling. A new explicit
        // audience intent says nothing about scopes, which remain untried.
        let explicit: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM installation_google_console_commands WHERE setup_id=?1 AND command=?2)",
            params![setup.setup_id.to_string(), cmd.name()], |r| r.get(0))?;
        for sibling in Command::of(cmd.home()).iter().filter(|_| !explicit) {
            tx.execute(
                "INSERT INTO installation_google_console_commands(setup_id,command,state) VALUES(?1,?2,'intent')",
                params![setup.setup_id.to_string(), sibling.name()],
            )?;
        }
        if tx.execute(
            "UPDATE installation_google_console_commands SET state='confirmed' WHERE setup_id=?1 AND command=?2 AND state='intent'",
            params![setup.setup_id.to_string(), cmd.name()],
        )? != 1
        {
            return Err(StoreError::Conflict);
        }
        renew(tx, now)
    })
}

/// Register through the existing private, atomic Google client writer. `save` receives the
/// locked registry directory handle and runs inside the same Immediate transaction as the
/// authority/lease checks and the checkpoint update; it must not call the registry again
/// (that would deadlock on the directory lock) or perform network I/O. The file commit
/// precedes the nonsecret checkpoint; if `save` fails nothing changes and the checkpoint stays
/// at `Client` with intent. If the final commit fails after `save` succeeded, the gateway
/// adopts the already-written file when reconciling the stable `client_name`.
pub fn register<T>(
    root: &Path,
    s: &AdminSession,
    run: Uuid,
    save: impl FnOnce(&fs::File) -> Result<T>,
) -> Result<T> {
    leased(root, s, run, |tx, dir, now| {
        let current = read(tx)?.ok_or(StoreError::Unavailable)?;
        if current.step != Step::Client || !current.intent {
            return Err(StoreError::Conflict);
        }
        let result = save(dir)?;
        if tx.execute(
            "UPDATE installation_google_bootstrap SET step='registered',intent=0 WHERE singleton=1 AND step='client' AND intent=1",
            [],
        )? != 1
        {
            return Err(StoreError::Conflict);
        }
        renew(tx, now)?;
        Ok(result)
    })
}

/// Reconcile a lost registration acknowledgement under fresh authority and the
/// same live lease. `verify` must only read/verify the exact private registration;
/// this does not authorize replacing a saved client or advancing any checkpoint.
pub fn reconcile_registration<T>(
    root: &Path,
    s: &AdminSession,
    run: Uuid,
    verify: impl FnOnce(&fs::File) -> Result<T>,
) -> Result<T> {
    leased(root, s, run, |tx, dir, now| {
        let current = read(tx)?.ok_or(StoreError::Unavailable)?;
        if current.step != Step::Registered || current.intent {
            return Err(StoreError::Conflict);
        }
        let result = verify(dir)?;
        renew(tx, now)?;
        Ok(result)
    })
}

/// Only call after explicit guest teardown acknowledgement. Cancellation or a lost transport
/// deliberately leaves the 180-second fence for the old browser; an expired or superseded
/// run cannot release a successor.
pub fn release(root: &Path, s: &AdminSession, run: Uuid) -> Result<()> {
    leased(root, s, run, |tx, _, _| {
        if sdk_pending(tx)?.is_some() {
            return Err(StoreError::Conflict);
        }
        tx.execute(
            "UPDATE installation_google_bootstrap SET lease_until=0 WHERE singleton=1",
            [],
        )?;
        Ok(())
    })
}

fn sdk_pending(c: &Connection) -> Result<Option<String>> {
    let row: Option<Option<String>> = c
        .query_row(
            "SELECT sdk_run FROM installation_google_bootstrap WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row.flatten())
}

/// Private root-broker observation, never an HTTP/operator API. Lookup by the
/// authenticated browser challenge lets teardown acknowledge even if the peer
/// disappears after begin_sdk commits but before Authorize arrives. This does
/// not clear anything and does not grant authority to dispatch SDK operations.
pub fn sdk_cleanup_for_run(root: &Path, run: Uuid) -> Result<Option<Uuid>> {
    if run.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    let (_dir, c) = open(root, false)?;
    let setup: Option<String> = c
        .query_row(
            "SELECT setup_id FROM installation_google_bootstrap WHERE singleton=1 AND sdk_run=?1",
            [run.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    setup
        .map(|s| Uuid::parse_str(&s).map_err(|_| StoreError::Corrupt))
        .transpose()
}

/// The run with an unresolved temporary SDK login, if any. Observation only.
pub fn pending_sdk_run(root: &Path, s: &AdminSession) -> Result<Option<Uuid>> {
    let (_dir, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check(&tx, s)?;
    sdk_pending(&tx)?
        .map(|v| Uuid::parse_str(&v).map_err(|_| StoreError::Corrupt))
        .transpose()
}

/// Persist the single dispatch intent BEFORE the worker is sent Authorize. Call once per
/// claimed run, with the leased administrator. A repeat (even of the same run), a run that
/// was already cleaned, or a foreign/expired run is refused. Renews the lease (180s).
pub fn begin_sdk(root: &Path, s: &AdminSession, run: Uuid) -> Result<()> {
    if run.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    leased(root, s, run, |tx, _, now| {
        if tx.execute(
            "UPDATE installation_google_bootstrap SET sdk_run=?1 WHERE singleton=1 AND sdk_run IS NULL AND sdk_cleaned_run IS NOT ?1 AND NOT EXISTS(SELECT 1 FROM installation_google_bootstrap_sdk_closed WHERE run_id=?1)",
            [run.to_string()],
        )? != 1
        {
            return Err(StoreError::Conflict);
        }
        renew(tx, now)
    })
}

/// Private root-broker receipt after proven shutdown (or proven no worker was
/// started). This API trusts the root registry owner, not member HTTP input.
/// Atomically close every authenticated run even if claim or begin_sdk has not
/// committed yet. A late claim must not revive a browser that already ended.
/// An exact pending run is cleared; a successor is not.
pub fn close_sdk_run(root: &Path, run: Uuid) -> Result<()> {
    if run.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    let (_dir, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT OR IGNORE INTO installation_google_bootstrap_sdk_closed(run_id) VALUES(?1)",
        [run.to_string()],
    )?;
    tx.execute(
            "UPDATE installation_google_bootstrap SET sdk_run=NULL,sdk_cleaned_run=?1 WHERE singleton=1 AND sdk_run=?1",
            [run.to_string()],
        )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::directory;
    use std::{
        sync::{
            Barrier,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };
    const CALLBACK: &str = "https://assistant.example.test/api/v1/mail/oauth/callback";
    const EMAIL: &str = "google@example.test";
    const ORDER: [Step; 4] = [Step::Project, Step::Apis, Step::Branding, Step::Audience];

    fn fixture() -> (tempfile::TempDir, AdminSession) {
        // directory() creates a private 0700 canonical temp root, as the registry requires.
        let d = directory();
        let a = reserve(d.path(), Uuid::new_v4(), "admin@example.test".into()).unwrap();
        set_routing(d.path(), &a, true).unwrap();
        let v = SessionView {
            user: moosuri_core::AccountSummary {
                id: a.account_id.clone(),
                name: "Fixture".into(),
                email: "admin@example.test".into(),
                role: Role::Member,
                must_change_password: false,
            },
            password_change_required: false,
            chat_draft_scope: None,
        };
        bootstrap(d.path(), Uuid::new_v4(), &a, &v).unwrap();
        let s = checked_session(d.path(), &a, &v).unwrap();
        (d, s)
    }
    fn begin(r: &Path, s: &AdminSession, id: Uuid) -> Setup {
        claim(r, s, id, EMAIL.into(), CALLBACK.into()).unwrap()
    }
    fn expire(r: &Path) {
        let (_d, c) = open(r, false).unwrap();
        c.execute("UPDATE installation_google_bootstrap SET lease_until=0", [])
            .unwrap();
    }
    fn shorten(r: &Path) {
        let (_d, c) = open(r, false).unwrap();
        let soon = crate::now().unwrap() + 5;
        c.execute(
            "UPDATE installation_google_bootstrap SET lease_until=?1",
            [soon],
        )
        .unwrap();
    }
    fn lease_renewed(r: &Path) -> bool {
        let (_d, c) = open(r, false).unwrap();
        let until: i64 = c
            .query_row(
                "SELECT lease_until FROM installation_google_bootstrap",
                [],
                |row| row.get(0),
            )
            .unwrap();
        until >= crate::now().unwrap() + LEASE_SECONDS - 5
    }
    fn stored(r: &Path) -> Setup {
        let (_d, c) = open(r, false).unwrap();
        read(&c).unwrap().unwrap()
    }
    /// Complete one stage the way the controller does: per-command journal for the Console
    /// stages (each with its own readback), plain intent for the others.
    fn advance(r: &Path, s: &AdminSession, run: Uuid, step: Step) {
        match step {
            Step::Branding => {
                intend_command(r, s, run, Command::Branding).unwrap();
                confirm_command(r, s, run, Command::Branding).unwrap();
            }
            Step::Audience => {
                for cmd in [Command::Audience, Command::Scopes] {
                    intend_command(r, s, run, cmd).unwrap();
                    confirm_command(r, s, run, cmd).unwrap();
                }
            }
            _ => {
                intend(r, s, run, step).unwrap();
            }
        }
        confirmed(r, s, run, step).unwrap();
    }
    fn advance_to_client(r: &Path, s: &AdminSession, run: Uuid) {
        for step in ORDER {
            advance(r, s, run, step);
        }
        intend(r, s, run, Step::Client).unwrap();
    }

    #[test]
    fn restart_keeps_the_same_project_client_name_and_uncertain_intent() {
        let (d, s) = fixture();
        let r = d.path();
        let first = Uuid::new_v4();
        let original = begin(r, &s, first);
        intend(r, &s, first, Step::Project).unwrap();
        expire(r);
        let run = Uuid::new_v4();
        let after_project = begin(r, &s, run);
        assert_eq!(after_project.project_id(), original.project_id());
        assert_eq!(
            (after_project.step, after_project.intent),
            (Step::Project, true)
        );
        assert!(intend(r, &s, first, Step::Project).is_err());
        confirmed(r, &s, run, Step::Project).unwrap();
        for step in &ORDER[1..] {
            advance(r, &s, run, *step);
        }
        intend(r, &s, run, Step::Client).unwrap();
        expire(r);
        let resumed = begin(r, &s, Uuid::new_v4());
        assert_eq!(resumed.setup_id, original.setup_id);
        assert_eq!(resumed.client_name(), original.client_name());
        assert_eq!((resumed.step, resumed.intent), (Step::Client, true));
    }

    #[test]
    fn concurrent_claims_have_one_winner_and_used_runs_never_return() {
        let (d, s) = fixture();
        let r = d.path();
        let start = Barrier::new(4);
        let (start, s) = (&start, &s);
        let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let outcomes: Vec<_> = thread::scope(|scope| {
            ids.iter()
                .copied()
                .map(|id| {
                    scope.spawn(move || {
                        start.wait();
                        claim(r, s, id, EMAIL.into(), CALLBACK.into()).map(|x| x.setup_id)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|j| j.join().unwrap())
                .collect()
        });
        assert_eq!(outcomes.iter().filter(|o| o.is_ok()).count(), 1);
        assert!(
            outcomes
                .iter()
                .filter(|o| o.is_err())
                .all(|o| matches!(o, Err(StoreError::Conflict)))
        );
        let winner = ids
            .iter()
            .copied()
            .find(|id| heartbeat(r, s, *id).is_ok())
            .unwrap();
        // Replay of the live run id and of a run that lost the race are both refused.
        assert!(matches!(
            claim(r, &s, winner, EMAIL.into(), CALLBACK.into()),
            Err(StoreError::Conflict)
        ));
        expire(r);
        assert!(claim(r, &s, winner, EMAIL.into(), CALLBACK.into()).is_err());
        // A loser id was never persisted, so it is a fresh run after expiry; then it is used.
        let loser = ids.iter().copied().find(|id| *id != winner).unwrap();
        begin(r, &s, loser);
        expire(r);
        assert!(claim(r, &s, loser, EMAIL.into(), CALLBACK.into()).is_err());
    }

    #[test]
    fn stale_lease_cannot_write_or_release_the_successor() {
        let (d, s) = fixture();
        let r = d.path();
        let old = Uuid::new_v4();
        begin(r, &s, old);
        expire(r);
        assert!(heartbeat(r, &s, old).is_err());
        let new = Uuid::new_v4();
        begin(r, &s, new);
        assert!(intend(r, &s, old, Step::Project).is_err());
        assert!(release(r, &s, old).is_err());
        // The successor's fence is intact: nobody else can claim until teardown/expiry.
        assert!(matches!(
            claim(r, &s, Uuid::new_v4(), EMAIL.into(), CALLBACK.into()),
            Err(StoreError::Conflict)
        ));
        heartbeat(r, &s, new).unwrap();
        release(r, &s, new).unwrap();
        assert!(release(r, &s, new).is_err());
        begin(r, &s, Uuid::new_v4());
    }

    #[test]
    fn changed_account_callback_epoch_or_demotion_cannot_resume_or_register() {
        let (d, mut s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        // Once a project write may have been sent, the identity is exact.
        intend(r, &s, run, Step::Project).unwrap();
        expire(r);
        assert!(matches!(
            claim(
                r,
                &s,
                Uuid::new_v4(),
                "another@example.test".into(),
                CALLBACK.into()
            ),
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            claim(
                r,
                &s,
                Uuid::new_v4(),
                EMAIL.into(),
                "https://other.example.test/api/v1/mail/oauth/callback".into()
            ),
            Err(StoreError::Conflict)
        ));
        let run = Uuid::new_v4();
        begin(r, &s, run);
        // Authority pin older than its five-second bound.
        s.issued = Instant::now() - Duration::from_secs(6);
        assert!(heartbeat(r, &s, run).is_err());
        s.issued = Instant::now();
        // Role revoked after the pin was issued: no operation, and no file save.
        let saved = AtomicBool::new(false);
        {
            let (_dir, c) = open(r, false).unwrap();
            c.execute(
                "UPDATE installation_members SET role_revision=role_revision+1",
                [],
            )
            .unwrap();
        }
        assert!(heartbeat(r, &s, run).is_err());
        assert!(intend(r, &s, run, Step::Project).is_err());
        assert!(release(r, &s, run).is_err());
        assert!(
            register(r, &s, run, |_| {
                saved.store(true, Ordering::SeqCst);
                Ok(())
            })
            .is_err()
        );
        assert!(!saved.load(Ordering::SeqCst));
        assert!(current(r, &s).is_err());
        // A rotated web-session epoch likewise invalidates the old pin.
        let (d2, s2) = fixture();
        let run2 = Uuid::new_v4();
        begin(d2.path(), &s2, run2);
        {
            let (_dir, c) = open(d2.path(), false).unwrap();
            c.execute("UPDATE allocations SET session_epoch=session_epoch+1", [])
                .unwrap();
        }
        assert!(heartbeat(d2.path(), &s2, run2).is_err());
        assert!(
            claim(
                d2.path(),
                &s2,
                Uuid::new_v4(),
                EMAIL.into(),
                CALLBACK.into()
            )
            .is_err()
        );
    }

    #[test]
    fn begin_step_is_compare_and_set_and_never_repeats_after_uncertainty() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        assert!(intend(r, &s, run, Step::Apis).is_err());
        assert!(intend(r, &s, run, Step::Registered).is_err());
        assert!(intend(r, &s, run, Step::Project).unwrap().intent);
        assert!(matches!(
            intend(r, &s, run, Step::Project),
            Err(StoreError::Conflict)
        ));
        expire(r);
        let next = Uuid::new_v4();
        assert!(begin(r, &s, next).intent);
        assert!(matches!(
            intend(r, &s, next, Step::Project),
            Err(StoreError::Conflict)
        ));
    }

    #[test]
    fn confirmation_is_idempotent_but_cannot_skip_or_complete_registration() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        assert!(confirmed(r, &s, run, Step::Project).is_err());
        shorten(r);
        intend(r, &s, run, Step::Project).unwrap();
        assert!(lease_renewed(r));
        assert!(confirmed(r, &s, run, Step::Apis).is_err());
        shorten(r);
        confirmed(r, &s, run, Step::Project).unwrap();
        assert!(lease_renewed(r));
        shorten(r);
        confirmed(r, &s, run, Step::Project).unwrap();
        assert!(lease_renewed(r));
        assert_eq!(stored(r).step, Step::Apis);
        database_rejects_skips_reversals_and_resets(r);
        assert!(intend(r, &s, run, Step::Branding).is_err());
        assert!(intend(r, &s, run, Step::Client).is_err());
        let mut seen = vec![Step::Apis];
        for step in [Step::Apis, Step::Branding, Step::Audience] {
            advance(r, &s, run, step);
            seen.push(stored(r).step);
        }
        assert_eq!(
            seen,
            [Step::Apis, Step::Branding, Step::Audience, Step::Client]
        );
        assert!(confirmed(r, &s, run, Step::Client).is_err());
        intend(r, &s, run, Step::Client).unwrap();
        assert!(register::<()>(r, &s, run, |_| Err(StoreError::Unavailable)).is_err());
        assert_eq!(stored(r).step, Step::Client);
    }
    // The schema itself refuses a skipped or reversed step, a changed identity, and removal
    // of the checkpoint or used run ids, independent of the Rust guards.
    fn database_rejects_skips_reversals_and_resets(r: &Path) {
        let (_dir, c) = open(r, false).unwrap();
        for sql in [
            "UPDATE installation_google_bootstrap SET step='client',intent=0",
            "UPDATE installation_google_bootstrap SET step='branding',intent=0",
            "UPDATE installation_google_bootstrap SET step='project',intent=0",
        ] {
            assert!(c.execute(sql, []).is_err(), "{sql}");
        }
        assert!(
            c.execute(
                "UPDATE installation_google_bootstrap SET google_email='x@example.test'",
                []
            )
            .is_err()
        );
        assert!(
            c.execute("DELETE FROM installation_google_bootstrap", [])
                .is_err()
        );
        assert!(
            c.execute("DELETE FROM installation_google_bootstrap_runs", [])
                .is_err()
        );
    }

    #[test]
    fn registration_only_at_client_intent_and_failed_save_keeps_the_checkpoint() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        let called = AtomicBool::new(false);
        let probe = |_: &fs::File| {
            called.store(true, Ordering::SeqCst);
            Ok(())
        };
        assert!(matches!(
            register(r, &s, run, probe),
            Err(StoreError::Conflict)
        ));
        for step in ORDER {
            advance(r, &s, run, step);
        }
        // At Client but with no recorded intent.
        assert!(matches!(
            register(r, &s, run, probe),
            Err(StoreError::Conflict)
        ));
        assert!(!called.load(Ordering::SeqCst));
        assert!(matches!(
            reconcile_registration(r, &s, run, probe),
            Err(StoreError::Conflict)
        ));
        intend(r, &s, run, Step::Client).unwrap();
        assert!(register::<()>(r, &s, run, |_| Err(StoreError::Unavailable)).is_err());
        let kept = stored(r);
        assert_eq!((kept.step, kept.intent), (Step::Client, true));
        register(r, &s, run, |dir| {
            // The callback receives the locked registry directory, the private-file contract.
            assert!(dir.metadata().unwrap().is_dir());
            Ok(())
        })
        .unwrap();
        let done = stored(r);
        assert_eq!((done.step, done.intent), (Step::Registered, false));
        assert!(matches!(
            register(r, &s, run, probe),
            Err(StoreError::Conflict)
        ));
        assert!(!called.load(Ordering::SeqCst));
        assert!(matches!(
            reconcile_registration::<()>(r, &s, run, |_| Err(StoreError::Conflict)),
            Err(StoreError::Conflict)
        ));
        reconcile_registration(r, &s, run, probe).unwrap();
        assert!(called.swap(false, Ordering::SeqCst));
        assert!(reconcile_registration(r, &s, Uuid::new_v4(), probe).is_err());
        assert!(!called.load(Ordering::SeqCst));
        // Retry after completion reads the same state and offers no provider write.
        assert_eq!(current(r, &s).unwrap().unwrap().step, Step::Registered);
        expire(r);
        assert!(reconcile_registration(r, &s, run, probe).is_err());
        assert!(!called.load(Ordering::SeqCst));
        let again = begin(r, &s, Uuid::new_v4());
        assert_eq!(again.step, Step::Registered);
        assert!(intend(r, &s, Uuid::new_v4(), Step::Client).is_err());
    }

    fn at_audience(r: &Path, s: &AdminSession, run: Uuid) {
        for step in [Step::Project, Step::Apis, Step::Branding] {
            advance(r, s, run, step);
        }
        assert_eq!(stored(r).step, Step::Audience);
    }
    fn console_rows(r: &Path) -> Vec<(String, String)> {
        let (_d, c) = open(r, false).unwrap();
        let mut q = c
            .prepare(
                "SELECT command,state FROM installation_google_console_commands ORDER BY command",
            )
            .unwrap();
        q.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn console_journal_rejects_forged_legacy_intent_and_direct_confirmed_inserts() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        let setup = begin(r, &s, run);
        for step in [Step::Project, Step::Apis] {
            advance(r, &s, run, step);
        }
        assert!(matches!(
            intend(r, &s, run, Step::Branding),
            Err(StoreError::InvalidInput)
        ));
        assert!(!stored(r).intent);
        advance(r, &s, run, Step::Branding);
        assert!(matches!(
            intend(r, &s, run, Step::Audience),
            Err(StoreError::InvalidInput)
        ));
        assert!(!stored(r).intent);
        let (_dir, c) = open(r, false).unwrap();
        assert!(c.execute(
            "INSERT INTO installation_google_console_commands VALUES(?1,'audience','confirmed')",
            [setup.setup_id.to_string()],
        ).is_err());
        assert!(
            c.execute(
                "INSERT INTO installation_google_console_commands VALUES(?1,'audience','intent')",
                [Uuid::new_v4().to_string()],
            )
            .is_err()
        );
        for state in ["intent", "confirmed"] {
            assert!(c.execute(
                "INSERT OR REPLACE INTO installation_google_console_commands VALUES(?1,'branding',?2)",
                params![setup.setup_id.to_string(), state],
            ).is_err());
        }
        let state: String = c
            .query_row(
                "SELECT state FROM installation_google_console_commands WHERE command='branding'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "confirmed");
    }

    #[test]
    fn lost_audience_reply_is_reconciled_then_scopes_are_first_submitted() {
        use CommandState::*;
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        at_audience(r, &s, run);
        assert_eq!(
            command_state(r, &s, run, Command::Audience).unwrap(),
            Untried
        );
        // Scopes cannot begin before the audience readback is confirmed.
        assert!(matches!(
            intend_command(r, &s, run, Command::Scopes),
            Err(StoreError::Conflict)
        ));
        intend_command(r, &s, run, Command::Audience).unwrap();
        // The stage cannot advance on an unconfirmed command.
        assert!(matches!(
            confirmed(r, &s, run, Step::Audience),
            Err(StoreError::Conflict)
        ));
        // Reply lost: worker gone, lease expires, a new run resumes.
        expire(r);
        let next = Uuid::new_v4();
        assert!(begin(r, &s, next).intent);
        assert_eq!(
            command_state(r, &s, next, Command::Audience).unwrap(),
            Intent
        );
        assert_eq!(
            command_state(r, &s, next, Command::Scopes).unwrap(),
            Untried
        );
        assert!(matches!(
            intend_command(r, &s, next, Command::Audience),
            Err(StoreError::Conflict)
        ));
        // A command never begun cannot be confirmed.
        assert!(matches!(
            confirm_command(r, &s, next, Command::Scopes),
            Err(StoreError::Conflict)
        ));
        // Positive readback reconciles the uncertain audience intent (idempotently).
        confirm_command(r, &s, next, Command::Audience).unwrap();
        confirm_command(r, &s, next, Command::Audience).unwrap();
        // The distinct never-attempted next command may now be submitted for the first time.
        intend_command(r, &s, next, Command::Scopes).unwrap();
        assert!(matches!(
            confirmed(r, &s, next, Step::Audience),
            Err(StoreError::Conflict)
        ));
        confirm_command(r, &s, next, Command::Scopes).unwrap();
        confirmed(r, &s, next, Step::Audience).unwrap();
        let done = stored(r);
        assert_eq!((done.step, done.intent), (Step::Client, false));
        // Past stage: confirmed, nothing may be mutated again.
        assert_eq!(
            command_state(r, &s, next, Command::Scopes).unwrap(),
            Confirmed
        );
        assert!(intend_command(r, &s, next, Command::Scopes).is_err());
        assert_eq!(done.project_id(), stored(r).project_id());
    }

    #[test]
    fn uncertain_scopes_survive_reclaim_and_cannot_be_replayed_or_downgraded() {
        use CommandState::*;
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        at_audience(r, &s, run);
        for cmd in [Command::Audience] {
            intend_command(r, &s, run, cmd).unwrap();
            confirm_command(r, &s, run, cmd).unwrap();
        }
        intend_command(r, &s, run, Command::Scopes).unwrap();
        expire(r);
        let next = Uuid::new_v4();
        begin(r, &s, next);
        assert_eq!(command_state(r, &s, next, Command::Scopes).unwrap(), Intent);
        assert!(matches!(
            intend_command(r, &s, next, Command::Scopes),
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            confirmed(r, &s, next, Step::Audience),
            Err(StoreError::Conflict)
        ));
        // The schema keeps the journal monotone and permanent.
        let (_dir, c) = open(r, false).unwrap();
        assert!(
            c.execute("UPDATE installation_google_console_commands SET state='intent' WHERE command='audience'", [])
                .is_err()
        );
        assert!(
            c.execute("UPDATE installation_google_console_commands SET command='branding' WHERE command='scopes'", [])
                .is_err()
        );
        assert!(
            c.execute("DELETE FROM installation_google_console_commands", [])
                .is_err()
        );
        assert!(
            c.execute(
                "INSERT INTO installation_google_console_commands VALUES('x','other','intent')",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn console_commands_need_fresh_admin_current_lease_and_respect_the_sdk_fence() {
        let (d, mut s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        at_audience(r, &s, run);
        begin_sdk(r, &s, run).unwrap();
        intend_command(r, &s, run, Command::Audience).unwrap();
        // Expired lease: no reads or writes for the old run, journal untouched.
        expire(r);
        assert!(command_state(r, &s, run, Command::Audience).is_err());
        assert!(confirm_command(r, &s, run, Command::Audience).is_err());
        // A still-pending SDK grant blocks any successor even after expiry; the old run's
        // command journal stays as it was.
        assert!(matches!(
            claim(r, &s, Uuid::new_v4(), EMAIL.into(), CALLBACK.into()),
            Err(StoreError::Conflict)
        ));
        assert_eq!(
            console_rows(r),
            [
                ("audience".to_string(), "intent".to_string()),
                ("branding".to_string(), "confirmed".to_string())
            ]
        );
        close_sdk_run(r, run).unwrap();
        let next = Uuid::new_v4();
        begin(r, &s, next);
        // Foreign run id and a pin older than five seconds are refused.
        assert!(confirm_command(r, &s, run, Command::Audience).is_err());
        s.issued = Instant::now() - Duration::from_secs(6);
        assert!(confirm_command(r, &s, next, Command::Audience).is_err());
        s.issued = Instant::now();
        // Demotion after the pin was issued.
        {
            let (_dir, c) = open(r, false).unwrap();
            c.execute(
                "UPDATE installation_members SET role_revision=role_revision+1",
                [],
            )
            .unwrap();
        }
        assert!(confirm_command(r, &s, next, Command::Audience).is_err());
        assert!(command_state(r, &s, next, Command::Audience).is_err());
        assert_eq!(
            console_rows(r),
            [
                ("audience".to_string(), "intent".to_string()),
                ("branding".to_string(), "confirmed".to_string())
            ]
        );
    }

    #[test]
    fn migration_eight_audience_intent_is_uncertain_for_audience_and_scopes() {
        use CommandState::*;
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        for step in [Step::Project, Step::Apis, Step::Branding] {
            advance(r, &s, run, step);
        }
        // Exactly what migration 8 wrote: the stage bit alone, with no command rows.
        // Reopening at schema 8 must recreate the journal without disturbing the checkpoint.
        {
            let (_dir, c) = open(r, false).unwrap();
            c.execute_batch(
                "DROP TABLE installation_google_console_commands; UPDATE installation_google_bootstrap SET intent=1 WHERE step='audience'; PRAGMA user_version=8;",
            )
            .unwrap();
        }
        assert!(console_rows(r).is_empty());
        let version: i64 = {
            let (_dir, c) = open(r, false).unwrap();
            c.query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(version, 9);
        // Neither command may be sent blind, and the old bit is not aliased to one of them.
        assert_eq!(
            command_state(r, &s, run, Command::Audience).unwrap(),
            Intent
        );
        assert_eq!(command_state(r, &s, run, Command::Scopes).unwrap(), Intent);
        for cmd in [Command::Audience, Command::Scopes] {
            assert!(matches!(
                intend_command(r, &s, run, cmd),
                Err(StoreError::Conflict)
            ));
        }
        assert!(confirmed(r, &s, run, Step::Audience).is_err());
        // A positive audience readback does not launder the scopes uncertainty.
        confirm_command(r, &s, run, Command::Audience).unwrap();
        assert_eq!(command_state(r, &s, run, Command::Scopes).unwrap(), Intent);
        assert!(matches!(
            intend_command(r, &s, run, Command::Scopes),
            Err(StoreError::Conflict)
        ));
        confirm_command(r, &s, run, Command::Scopes).unwrap();
        confirmed(r, &s, run, Step::Audience).unwrap();
        assert_eq!(stored(r).step, Step::Client);
    }

    #[test]
    fn checkpoint_holds_no_secret_or_billing_state() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        advance_to_client(r, &s, run);
        const SENTINEL: &str = "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inel";
        register(r, &s, run, |_| {
            fs::write(r.join(GOOGLE_WEB_CLIENT_FILE), SENTINEL).map_err(|_| StoreError::Unavailable)
        })
        .unwrap();
        let columns: Vec<String> = {
            let (_dir, c) = open(r, false).unwrap();
            let mut q = c
                .prepare("SELECT t.name||'.'||i.name FROM sqlite_schema t, pragma_table_info(t.name) i WHERE t.type='table' AND t.name LIKE 'installation_google_bootstrap%'")
                .unwrap();
            q.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };
        assert!(!columns.is_empty());
        for column in &columns {
            for forbidden in ["billing", "secret", "token", "password", "credential"] {
                assert!(!column.contains(forbidden), "{column}");
            }
        }
        let bytes = fs::read(r.join(REGISTRY)).unwrap();
        assert!(
            !bytes
                .windows(SENTINEL.len())
                .any(|w| w == SENTINEL.as_bytes())
        );
        let text = serde_json::to_string(&stored(r)).unwrap();
        assert!(!text.contains("billing") && !text.contains("secret"));
    }

    #[test]
    fn identity_is_correctable_only_before_any_project_intent() {
        let (d, s) = fixture();
        let r = d.path();
        let other_cb = "https://other.example.test/api/v1/mail/oauth/callback";
        let old = Uuid::new_v4();
        let original = begin(r, &s, old);
        // A live lease refuses correction.
        assert!(matches!(
            claim(
                r,
                &s,
                Uuid::new_v4(),
                "fixed@example.test".into(),
                CALLBACK.into()
            ),
            Err(StoreError::Conflict)
        ));
        // After confirmed teardown, pre-intent correction keeps the project identity.
        release(r, &s, old).unwrap();
        let fixed = Uuid::new_v4();
        let corrected = claim(r, &s, fixed, "fixed@example.test".into(), other_cb.into()).unwrap();
        assert_eq!(corrected.setup_id, original.setup_id);
        assert_eq!(corrected.project_id(), original.project_id());
        assert_eq!(corrected.client_name(), original.client_name());
        assert_eq!(
            (
                corrected.google_email.as_str(),
                corrected.redirect_uri.as_str()
            ),
            ("fixed@example.test", other_cb)
        );
        assert_eq!((corrected.step, corrected.intent), (Step::Project, false));
        // The old run stays invalid, including as a new claim.
        assert!(heartbeat(r, &s, old).is_err());
        assert!(intend(r, &s, old, Step::Project).is_err());
        expire(r);
        assert!(matches!(
            claim(r, &s, old, "fixed@example.test".into(), other_cb.into()),
            Err(StoreError::Conflict)
        ));
        // Expiration also fences `fixed`; a new worker must claim before writing.
        assert!(intend(r, &s, fixed, Step::Project).is_err());
        let fixed = Uuid::new_v4();
        let resumed = begin_with(r, &s, fixed, "fixed@example.test", other_cb);
        assert_eq!(resumed.setup_id, original.setup_id);
        // The setup id itself is never rewritable.
        {
            let (_dir, c) = open(r, false).unwrap();
            assert!(
                c.execute("UPDATE installation_google_bootstrap SET setup_id='x'", [])
                    .is_err()
            );
        }
        // Once intent is recorded the identity is exact again.
        intend(r, &s, fixed, Step::Project).unwrap();
        release(r, &s, fixed).unwrap();
        for (email, callback) in [
            ("again@example.test", other_cb),
            ("fixed@example.test", CALLBACK),
        ] {
            assert!(matches!(
                claim(r, &s, Uuid::new_v4(), email.into(), callback.into()),
                Err(StoreError::Conflict)
            ));
        }
        {
            let (_dir, c) = open(r, false).unwrap();
            assert!(
                c.execute(
                    "UPDATE installation_google_bootstrap SET google_email='y@example.test'",
                    []
                )
                .is_err()
            );
            assert!(
                c.execute("DELETE FROM installation_google_bootstrap", [])
                    .is_err()
            );
        }
        let same = begin_with(r, &s, Uuid::new_v4(), "fixed@example.test", other_cb);
        assert_eq!(same.setup_id, original.setup_id);
        assert!(same.intent);
    }
    fn begin_with(r: &Path, s: &AdminSession, id: Uuid, email: &str, callback: &str) -> Setup {
        claim(r, s, id, email.into(), callback.into()).unwrap()
    }

    fn sdk_columns(r: &Path) -> (Option<String>, Option<String>) {
        let (_d, c) = open(r, false).unwrap();
        c.query_row(
            "SELECT sdk_run,sdk_cleaned_run FROM installation_google_bootstrap",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    #[test]
    fn pending_sdk_blocks_claim_and_release_even_after_expiry_or_reopen() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        shorten(r);
        begin_sdk(r, &s, run).unwrap();
        assert!(lease_renewed(r));
        assert_eq!(pending_sdk_run(r, &s).unwrap(), Some(run));
        assert!(matches!(release(r, &s, run), Err(StoreError::Conflict)));
        // Crash-style: nothing in memory survives; reopening shows the same marker.
        assert_eq!(sdk_columns(r).0, Some(run.to_string()));
        expire(r);
        assert_eq!(sdk_columns(r).0, Some(run.to_string()));
        assert!(matches!(
            claim(r, &s, Uuid::new_v4(), EMAIL.into(), CALLBACK.into()),
            Err(StoreError::Conflict)
        ));
        // The schema refuses replacing the marker with another run.
        let (_dir, c) = open(r, false).unwrap();
        assert!(
            c.execute(
                "UPDATE installation_google_bootstrap SET sdk_run='other'",
                []
            )
            .is_err()
        );
        assert!(
            c.execute("UPDATE installation_google_bootstrap SET sdk_run=NULL", [])
                .is_err()
        );
    }

    #[test]
    fn begin_sdk_is_single_dispatch_and_leased() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        assert!(begin_sdk(r, &s, run).is_err()); // no setup yet
        begin(r, &s, run);
        assert!(begin_sdk(r, &s, Uuid::new_v4()).is_err()); // foreign
        assert!(matches!(
            begin_sdk(r, &s, Uuid::nil()),
            Err(StoreError::InvalidInput)
        ));
        assert_eq!(sdk_columns(r), (None, None));
        begin_sdk(r, &s, run).unwrap();
        assert!(matches!(begin_sdk(r, &s, run), Err(StoreError::Conflict)));
        // Even after cleanup the same run cannot dispatch again.
        close_sdk_run(r, run).unwrap();
        assert!(matches!(begin_sdk(r, &s, run), Err(StoreError::Conflict)));
        // An expired lease cannot begin.
        let old = Uuid::new_v4();
        expire(r);
        begin(r, &s, old);
        expire(r);
        assert!(begin_sdk(r, &s, old).is_err());
        assert_eq!(sdk_columns(r).0, None);
    }

    #[test]
    fn exact_acknowledge_clears_and_enables_a_new_run() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        let setup = begin(r, &s, run);
        assert_eq!(sdk_cleanup_for_run(r, run).unwrap(), None);
        begin_sdk(r, &s, run).unwrap();
        assert_eq!(sdk_cleanup_for_run(r, run).unwrap(), Some(setup.setup_id));
        assert_eq!(sdk_cleanup_for_run(r, Uuid::new_v4()).unwrap(), None);
        assert!(close_sdk_run(r, Uuid::nil()).is_err());
        close_sdk_run(r, Uuid::new_v4()).unwrap();
        assert_eq!(sdk_columns(r).0, Some(run.to_string()));
        expire(r);
        close_sdk_run(r, run).unwrap();
        assert_eq!(sdk_columns(r), (None, Some(run.to_string())));
        assert_eq!(sdk_cleanup_for_run(r, run).unwrap(), None);
        // Idempotent.
        close_sdk_run(r, run).unwrap();
        let next = Uuid::new_v4();
        begin(r, &s, next);
        // Replaying the old acknowledgement cannot clear the successor.
        begin_sdk(r, &s, next).unwrap();
        close_sdk_run(r, run).unwrap();
        assert_eq!(sdk_columns(r).0, Some(next.to_string()));
        close_sdk_run(r, next).unwrap();
        release(r, &s, next).unwrap();
        begin(r, &s, Uuid::new_v4());
    }

    #[test]
    fn closed_before_begin_refuses_late_commit_and_stays_closed_after_successor() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        close_sdk_run(r, run).unwrap();
        assert!(matches!(begin_sdk(r, &s, run), Err(StoreError::Conflict)));
        assert_eq!(sdk_columns(r).0, None);
        expire(r);
        let next = Uuid::new_v4();
        begin(r, &s, next);
        begin_sdk(r, &s, next).unwrap();
        close_sdk_run(r, run).unwrap();
        assert_eq!(sdk_columns(r).0, Some(next.to_string()));
        close_sdk_run(r, next).unwrap();
        let (_dir, c) = open(r, false).unwrap();
        let closed: i64 = c
            .query_row(
                "SELECT count(*) FROM installation_google_bootstrap_sdk_closed",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(closed, 2);
        assert!(
            c.execute("DELETE FROM installation_google_bootstrap_sdk_closed", [])
                .is_err()
        );
        assert!(
            c.execute(
                "UPDATE installation_google_bootstrap_sdk_closed SET run_id='replacement'",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn teardown_before_claim_permanently_refuses_late_authorization() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        close_sdk_run(r, run).unwrap();
        assert!(matches!(
            claim(r, &s, run, EMAIL.into(), CALLBACK.into()),
            Err(StoreError::Conflict)
        ));
        assert!(matches!(begin_sdk(r, &s, run), Err(StoreError::Conflict)));
        let next = Uuid::new_v4();
        begin(r, &s, next);
        begin_sdk(r, &s, next).unwrap();
        close_sdk_run(r, run).unwrap();
        assert_eq!(sdk_columns(r).0, Some(next.to_string()));
    }

    #[test]
    fn teardown_survives_role_revocation_without_clearing_other_runs() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        begin(r, &s, run);
        begin_sdk(r, &s, run).unwrap();
        {
            let (_dir, c) = open(r, false).unwrap();
            c.execute(
                "UPDATE installation_members SET role_revision=role_revision+1",
                [],
            )
            .unwrap();
        }
        assert!(heartbeat(r, &s, run).is_err());
        close_sdk_run(r, Uuid::new_v4()).unwrap();
        assert_eq!(sdk_columns(r).0, Some(run.to_string()));
        close_sdk_run(r, run).unwrap();
        assert_eq!(sdk_columns(r).0, None);
    }

    #[test]
    fn invalid_callback_does_not_reserve_the_installation() {
        let (d, s) = fixture();
        let r = d.path();
        let run = Uuid::new_v4();
        for callback in [
            "http://assistant.example.test/api/v1/mail/oauth/callback",
            "https://user@assistant.example.test/api/v1/mail/oauth/callback",
            "https://assistant.example.test/other",
            "https://assistant.example.test/api/v1/mail/oauth/callback?x=1",
            "https://assistant.example.test/api/v1/mail/x/../oauth/callback",
            "https://assistant.example.test/api/v1/mail/oauth/callback\n",
            "https://ASSISTANT.example.test/api/v1/mail/oauth/callback",
            "https://assistant.example.test:443/api/v1/mail/oauth/callback",
        ] {
            assert!(matches!(
                claim(r, &s, run, EMAIL.into(), callback.into()),
                Err(StoreError::InvalidInput)
            ));
        }
        assert_eq!(begin(r, &s, run).step, Step::Project);
    }
}
