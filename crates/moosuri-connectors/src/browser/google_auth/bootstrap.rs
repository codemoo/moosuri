//! Fixed typed protocol for SDK preparation inside the same-Chrome authentication
//! worker. Nothing here carries a credential, CLI flag, path or script: the only
//! inputs are a setup id, the exact OAuth redirect and one of six fixed actions.
//! The project id and ownership label are derived from the setup id.
//!
//! Wire: after the worker's `{"status":"signed_in"}` the controller sends either
//! the existing consent command or `{"bootstrap":Request}`. Each bootstrap action
//! is answered by exactly one `{"bootstrap":Reply}` and the worker then waits for
//! the next command. Any failure is a terminal worker status; nothing is retried.
//!
//! This transport does not make commands public and does not establish admin
//! authority. The gateway must enforce durable intent and a fresh admin heartbeat
//! before it sends any command. There is deliberately no Console step yet.
//!
//! Shutdown budget. Controller EOF, SIGTERM and the worker deadline cancel the
//! running action's token and the action is awaited to its end (the SDK's
//! `finish_login` may spend up to 65 s on best-effort credential cleanup); then the
//! worker runs its own awaited `cleanup_login_credentials` (up to 60 s), then stops
//! Chrome. The launcher therefore allows `BOOTSTRAP_SHUTDOWN_GRACE` (150 s) after
//! closing the worker's stdin before it may kill it. A bootstrap worker exits zero
//! only after SDK cleanup is proven, including when its operation reports failure.
//! Forced termination never grants this proof; container removal alone is insufficient. That must stay under the
//! 180 s durable lease (2 s heartbeat). Remaining deployment work (not done here):
//! the relay future that owns `GoogleAttempt::shutdown`, and the gateway/systemd
//! `TimeoutStopSec`, must not abandon it earlier than this grace, and must not
//! use a generic 220 s outer limit.
use super::{Binding, Error};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Worker-exit grace the outer relay future must allow once any bootstrap request
/// has been sent. Old non-bootstrap attempts keep the short default.
pub const BOOTSTRAP_SHUTDOWN_GRACE: Duration = Duration::from_secs(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Authorize,
    InspectProject,
    CreateProject,
    EnableApis,
    VerifyApis,
    Finish,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub setup_id: Uuid,
    pub redirect_uri: String,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Ready,
    Absent,
}

/// Worker-side incoming shape (`{"bootstrap":Request}`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    pub bootstrap: Request,
}

impl Request {
    pub fn callback(binding: &Binding) -> String {
        format!("{}/api/v1/mail/oauth/callback", binding.origin)
    }
    /// Rejects a nil setup id or any redirect other than this installation's
    /// exact callback.
    pub fn validate(&self, binding: &Binding) -> Result<(), Error> {
        if self.setup_id.is_nil()
            || !binding.valid()
            || self.redirect_uri != Self::callback(binding)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    /// `moosuri-` plus the first 20 lowercase hex digits of the setup id.
    pub fn project_id(&self) -> String {
        format!("moosuri-{}", &self.setup_id.simple().to_string()[..20])
    }
    /// `setup-` plus the full simple setup id.
    pub fn setup_label(&self) -> String {
        format!("setup-{}", self.setup_id.simple())
    }
    pub fn to_wire(&self) -> serde_json::Value {
        serde_json::json!({ "bootstrap": self })
    }
}

impl Reply {
    pub fn to_wire(self) -> serde_json::Value {
        serde_json::json!({ "bootstrap": self })
    }
    /// Exact shape only: one object with one `bootstrap` member.
    pub fn from_wire(value: &serde_json::Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            bootstrap: Reply,
        }
        serde_json::from_value::<Wire>(value.clone())
            .map(|w| w.bootstrap)
            .map_err(|_| Error::Invalid)
    }
}

/// Command-order state for one worker session. Identity (setup id) is fixed by the
/// first `Authorize`, which must come first and only once.
#[derive(Default, Debug)]
pub(super) struct Sequence {
    setup: Option<Uuid>,
    owned: bool,
    absent: bool,
    created: bool,
    finished: bool,
}

impl Sequence {
    pub fn active(&self) -> bool {
        self.setup.is_some() && !self.finished
    }
    /// Consent is allowed only when no bootstrap occurred or after `Finish`.
    pub fn consent_allowed(&self) -> bool {
        self.setup.is_none() || self.finished
    }
    pub fn admit(&mut self, request: &Request, binding: &Binding) -> Result<(), Error> {
        request.validate(binding)?;
        if self.finished {
            return Err(Error::Invalid);
        }
        let Some(setup) = self.setup else {
            if request.action != Action::Authorize {
                return Err(Error::Invalid);
            }
            self.setup = Some(request.setup_id);
            return Ok(());
        };
        if setup != request.setup_id || request.action == Action::Authorize {
            return Err(Error::Invalid);
        }
        // Absence is only good for the very next command.
        let absent = std::mem::take(&mut self.absent);
        match request.action {
            Action::CreateProject if !absent || self.created => Err(Error::Invalid),
            Action::CreateProject => {
                self.created = true;
                Ok(())
            }
            Action::EnableApis | Action::VerifyApis if !self.owned => Err(Error::Invalid),
            _ => Ok(()),
        }
    }
    /// Records a successful action's reply.
    pub fn record(&mut self, action: Action, reply: Reply) {
        match (action, reply) {
            (Action::InspectProject, Reply::Absent) => self.absent = true,
            (Action::InspectProject | Action::CreateProject, Reply::Ready) => self.owned = true,
            (Action::Finish, Reply::Ready) => self.finished = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> Binding {
        super::super::tests::binding()
    }
    fn request(binding: &Binding, setup: Uuid, action: Action) -> Request {
        Request {
            setup_id: setup,
            redirect_uri: Request::callback(binding),
            action,
        }
    }

    #[test]
    fn wire_is_strict_and_snake_case() {
        let b = binding();
        let id = Uuid::new_v4();
        let wire = request(&b, id, Action::InspectProject).to_wire();
        assert_eq!(wire["bootstrap"]["action"], "inspect_project");
        let envelope: Envelope = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(envelope.bootstrap.action, Action::InspectProject);
        let mut extra = wire.clone();
        extra["bootstrap"]["flags"] = "--x".into();
        assert!(serde_json::from_value::<Envelope>(extra).is_err());
        let mut sibling = wire;
        sibling["other"] = 1.into();
        assert!(serde_json::from_value::<Envelope>(sibling).is_err());
        assert_eq!(
            Reply::from_wire(&Reply::Absent.to_wire()),
            Ok(Reply::Absent)
        );
        for bad in [
            serde_json::json!({"bootstrap":"other"}),
            serde_json::json!({"bootstrap":"ready","x":1}),
            serde_json::json!({"status":"returned"}),
            serde_json::json!("ready"),
        ] {
            assert!(Reply::from_wire(&bad).is_err());
        }
    }

    #[test]
    fn validation_and_derived_names() {
        let b = binding();
        let id = Uuid::new_v4();
        let ok = request(&b, id, Action::Authorize);
        assert_eq!(ok.validate(&b), Ok(()));
        let simple = id.simple().to_string();
        assert_eq!(ok.project_id(), format!("moosuri-{}", &simple[..20]));
        assert_eq!(ok.setup_label(), format!("setup-{simple}"));
        assert!(super::super::sdk::ProjectId::parse(&ok.project_id()).is_ok());
        assert!(super::super::sdk::SetupLabel::parse(&ok.setup_label()).is_ok());
        assert!(
            request(&b, Uuid::nil(), Action::Authorize)
                .validate(&b)
                .is_err()
        );
        for uri in [
            format!("{}/api/v1/mail/oauth/callback/", b.origin),
            format!("{}/api/v1/mail/oauth/callback?x=1", b.origin),
            "https://evil.example/api/v1/mail/oauth/callback".into(),
            String::new(),
        ] {
            let mut bad = ok.clone();
            bad.redirect_uri = uri;
            assert!(bad.validate(&b).is_err());
        }
    }

    #[test]
    fn sequence_requires_authorize_first_once_and_fixed_identity() {
        let b = binding();
        let id = Uuid::new_v4();
        let mut s = Sequence::default();
        assert!(s.consent_allowed());
        assert!(
            s.admit(&request(&b, id, Action::InspectProject), &b)
                .is_err()
        );
        s.admit(&request(&b, id, Action::Authorize), &b).unwrap();
        assert!(!s.consent_allowed());
        assert!(s.admit(&request(&b, id, Action::Authorize), &b).is_err());
        assert!(
            s.admit(&request(&b, Uuid::new_v4(), Action::InspectProject), &b)
                .is_err()
        );
        s.admit(&request(&b, id, Action::InspectProject), &b)
            .unwrap();
    }

    #[test]
    fn create_only_right_after_proven_absent_and_once() {
        let b = binding();
        let id = Uuid::new_v4();
        let mut s = Sequence::default();
        s.admit(&request(&b, id, Action::Authorize), &b).unwrap();
        s.record(Action::Authorize, Reply::Ready);
        assert!(
            s.admit(&request(&b, id, Action::CreateProject), &b)
                .is_err()
        );
        s.admit(&request(&b, id, Action::InspectProject), &b)
            .unwrap();
        s.record(Action::InspectProject, Reply::Absent);
        // Another command in between spends the proof.
        s.admit(&request(&b, id, Action::InspectProject), &b)
            .unwrap();
        s.record(Action::InspectProject, Reply::Absent);
        s.admit(&request(&b, id, Action::CreateProject), &b)
            .unwrap();
        s.record(Action::CreateProject, Reply::Ready);
        s.admit(&request(&b, id, Action::InspectProject), &b)
            .unwrap();
        s.record(Action::InspectProject, Reply::Absent);
        assert!(
            s.admit(&request(&b, id, Action::CreateProject), &b)
                .is_err()
        );
    }

    #[test]
    fn enable_and_verify_need_owned_and_consent_only_after_finish() {
        let b = binding();
        let id = Uuid::new_v4();
        let mut s = Sequence::default();
        s.admit(&request(&b, id, Action::Authorize), &b).unwrap();
        assert!(s.admit(&request(&b, id, Action::EnableApis), &b).is_err());
        assert!(s.admit(&request(&b, id, Action::VerifyApis), &b).is_err());
        s.admit(&request(&b, id, Action::InspectProject), &b)
            .unwrap();
        s.record(Action::InspectProject, Reply::Ready);
        s.admit(&request(&b, id, Action::EnableApis), &b).unwrap();
        s.admit(&request(&b, id, Action::VerifyApis), &b).unwrap();
        assert!(!s.consent_allowed());
        s.admit(&request(&b, id, Action::Finish), &b).unwrap();
        s.record(Action::Finish, Reply::Ready);
        assert!(s.consent_allowed());
        assert!(!s.active());
        assert!(
            s.admit(&request(&b, id, Action::InspectProject), &b)
                .is_err()
        );
    }
}
