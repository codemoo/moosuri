//! Durable, installation-scoped *intent* checkpoint for creating the one shared Google
//! OAuth app (new unbilled project -> APIs -> branding -> audience -> web client).
//!
//! This registry is only an intent store. A row never proves that Google completed anything:
//! `intent=true` means "a provider write may have been sent"; a confirmed step means only that
//! the controller observed the result and said so. The controller must verify, before it
//! confirms `Project`, that the provider project has the stable id from `Setup::project_id`,
//! is ACTIVE and has billing disabled. Nothing here records or links a billing account.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
/// identity and keeps the same setup id. Claiming an already `Registered` checkpoint just
/// returns it.
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
        "SELECT EXISTS(SELECT 1 FROM installation_google_bootstrap_runs WHERE run_id=?1)",
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
    if step == Step::Registered {
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
        if read(tx)?.is_some_and(|c| c.step == next && !c.intent) {
            return renew(tx, now);
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
    leased(root, s, run, |tx, dir, _| {
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
        Ok(result)
    })
}

/// Only call after explicit guest teardown acknowledgement. Cancellation or a lost transport
/// deliberately leaves the 180-second fence for the old browser; an expired or superseded
/// run cannot release a successor.
pub fn release(root: &Path, s: &AdminSession, run: Uuid) -> Result<()> {
    leased(root, s, run, |tx, _, _| {
        tx.execute(
            "UPDATE installation_google_bootstrap SET lease_until=0 WHERE singleton=1",
            [],
        )?;
        Ok(())
    })
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
    fn advance_to_client(r: &Path, s: &AdminSession, run: Uuid) {
        for step in ORDER {
            intend(r, s, run, step).unwrap();
            confirmed(r, s, run, step).unwrap();
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
            intend(r, &s, run, *step).unwrap();
            confirmed(r, &s, run, *step).unwrap();
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
            intend(r, &s, run, step).unwrap();
            confirmed(r, &s, run, step).unwrap();
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
            intend(r, &s, run, step).unwrap();
            confirmed(r, &s, run, step).unwrap();
        }
        // At Client but with no recorded intent.
        assert!(matches!(
            register(r, &s, run, probe),
            Err(StoreError::Conflict)
        ));
        assert!(!called.load(Ordering::SeqCst));
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
        // Retry after completion reads the same state and offers no provider write.
        assert_eq!(current(r, &s).unwrap().unwrap().step, Step::Registered);
        expire(r);
        let again = begin(r, &s, Uuid::new_v4());
        assert_eq!(again.step, Step::Registered);
        assert!(intend(r, &s, Uuid::new_v4(), Step::Client).is_err());
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
            claim(r, &s, Uuid::new_v4(), "fixed@example.test".into(), CALLBACK.into()),
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
            (corrected.google_email.as_str(), corrected.redirect_uri.as_str()),
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
                c.execute(
                    "UPDATE installation_google_bootstrap SET setup_id='x'",
                    []
                )
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
