//! Fixed typed protocol for SDK preparation inside the same-Chrome authentication
//! worker. Requests carry no credential, CLI flag, path or script: the only
//! inputs are a setup id, the exact OAuth redirect and one of the fixed actions.
//! The project id and ownership label are derived from the setup id. Console
//! branding/scope/client actions additionally use only the bound verified Google
//! email and fixed constants (see `console` and `client`).
//!
//! Wire: after the worker's `{"status":"signed_in"}` the controller sends either
//! the existing consent command or `{"bootstrap":Request}`. Each bootstrap action
//! is answered by exactly one `{"bootstrap":Response}` and the worker then waits for
//! the next command. Any failure is a terminal worker status; nothing is retried.
//!
//! `Response` is `Status(Reply)` (`"ready"`/`"absent"`, the original wire), or for
//! the two client actions `ClientPresent` (public identity, never a secret) or
//! `Client` (`CreateClient` or `RecoverClientSecret` only: includes the client secret
//! shown once). The latter
//! is PRIVATE: it travels worker -> capsule -> root relay -> authorized gateway
//! only (bounded to `PRIVATE_WIRE_LIMIT`), must never reach a web response, a log
//! or stderr, and is serialized with `Response::to_line` into a zeroizing buffer.
//!
//! This transport does not make commands public and does not establish admin
//! authority. The gateway must enforce durable intent and a fresh admin heartbeat
//! before it sends any command.
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
use crate::gmail::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{fmt, time::Duration};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// Upper bound of one private worker/relay line (including the client secret).
pub const PRIVATE_WIRE_LIMIT: usize = 16 * 1024;

/// Worker-exit grace the outer relay future must allow once any bootstrap request
/// has been sent. Old non-bootstrap attempts keep the short default.
/// Includes the 730 s sealed-Vault window, 20 min login/setup work and cleanup margin.
pub const ATTEMPT_LIFETIME: Duration = Duration::from_secs(35 * 60);

pub const BOOTSTRAP_SHUTDOWN_GRACE: Duration = Duration::from_secs(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Authorize,
    InspectProject,
    CreateProject,
    EnableApis,
    VerifyApis,
    /// Console branding (app/support/contact). Needs an owned project and verified
    /// APIs. `Ready` = persisted values match; `Absent` = unconfigured.
    InspectBranding,
    /// Only right after `InspectBranding` answered `Absent`, once. Re-inspects
    /// itself, never overwrites different branding. `Ready` = readback matched.
    ConfigureBranding,
    /// Data Access scopes; needs verified branding. `Ready` = exactly the seven
    /// fixed scopes persisted; `Absent` = missing some (never extra ones).
    InspectScopes,
    /// External Testing audience with the verified initiating account eligible.
    InspectAudience,
    ConfigureAudience,
    /// Only right after `InspectScopes` answered `Absent`, once.
    ConfigureScopes,
    /// Read-only reconcile of the installation's OAuth client by its stable name
    /// (`Request::client_name`). Needs project, APIs, branding, scopes and audience
    /// ready. Answers `Status(Absent)` or `ClientPresent`; never a secret.
    InspectClient,
    /// Only right after `InspectClient` answered `Absent`, once per worker. Answers
    /// `Client` only after the persisted settings were read back and verified.
    CreateClient,
    /// Same-client secret recovery by adding ONE new client secret. Only right after
    /// `InspectClient` answered `ClientPresent`, once per worker, and never in a worker
    /// that spent `CreateClient` (they share one write budget). Answers `Client` only
    /// after the new secret was captured from the verified page and the exact same
    /// client was re-read. Grants no authority: the gateway must hold a separate
    /// durable once-only recovery intent and have proven local registration absent.
    RecoverClientSecret,
    /// Branding public information URLs (home page, privacy policy, terms of service) at
    /// this installation's own `/about`, `/privacy`, `/terms` on the validated callback
    /// origin. Needs verified branding, scopes and audience. `Ready` = all three persisted
    /// exactly AND an existing authorized domain covers the origin's hostname; `Absent` =
    /// some URL blank (never a differing non-empty one: that is `Blocked`). An authorized
    /// domain that does not cover the hostname is `NeedsHuman`: no domain is ever written.
    InspectDomains,
    /// Only right after `InspectDomains` answered `Absent`, once. Fills blank URLs only.
    ConfigureDomains,
    /// External audience publishing status. `Ready` = In production (exact field);
    /// `Absent` = Testing with an enabled Publish app action. Needs domains ready.
    /// A disabled publish action or unknown status is `NeedsHuman`.
    InspectPublication,
    /// Only right after `InspectPublication` answered `Absent`, once. LIVE-UNPROVEN: the
    /// publish confirmation dialog is the documented conventional one, not yet observed.
    ConfigurePublication,
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
    /// Stable OAuth client name: `Moosuri ` plus the full simple setup id.
    pub fn client_name(&self) -> String {
        format!("Moosuri {}", self.setup_id.simple())
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

/// `<digits>-<token>.apps.googleusercontent.com`-style id; fail closed on anything else.
pub fn valid_client_id(value: &str) -> bool {
    value.len() <= 256
        && value
            .strip_suffix(".apps.googleusercontent.com")
            .and_then(|s| s.split_once('-'))
            .is_some_and(|(number, token)| {
                !number.is_empty()
                    && number.bytes().all(|b| b.is_ascii_digit())
                    && !token.is_empty()
                    && token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            })
}

/// Only the observed fully visible Google web-client secret shape; labels and masks fail closed.
pub fn valid_client_secret(value: &str) -> bool {
    value.len() <= 256
        && value.strip_prefix("GOCSPX-").is_some_and(|s| {
            s.len() >= 8
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
}

/// Public identity of the persisted OAuth client. Never carries a secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClientIdentity {
    pub setup_id: Uuid,
    pub client_id: String,
    pub redirect_uri: String,
}

impl ClientIdentity {
    pub fn valid(&self) -> bool {
        !self.setup_id.is_nil()
            && valid_client_id(&self.client_id)
            && !self.redirect_uri.is_empty()
            && self.redirect_uri.len() <= 2048
    }
    /// Same setup and the request's exact callback.
    pub fn matches(&self, request: &Request) -> bool {
        self.valid()
            && self.setup_id == request.setup_id
            && self.redirect_uri == request.redirect_uri
    }
}

/// A freshly created, read-back verified client including its one-time secret.
/// The secret zeroizes on drop and is never printed by `Debug`.
pub struct ClientRegistration {
    pub setup_id: Uuid,
    pub client_id: String,
    pub redirect_uri: String,
    pub secret: SecretString,
}

impl ClientRegistration {
    pub fn new(identity: ClientIdentity, secret: SecretString) -> Self {
        Self {
            setup_id: identity.setup_id,
            client_id: identity.client_id,
            redirect_uri: identity.redirect_uri,
            secret,
        }
    }
    pub fn identity(&self) -> ClientIdentity {
        ClientIdentity {
            setup_id: self.setup_id,
            client_id: self.client_id.clone(),
            redirect_uri: self.redirect_uri.clone(),
        }
    }
    pub fn matches(&self, request: &Request) -> bool {
        self.identity().matches(request) && valid_client_secret(self.secret.expose())
    }
}

impl fmt::Debug for ClientRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientRegistration")
            .field("setup_id", &self.setup_id)
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// One worker answer to a bootstrap request.
#[derive(Debug)]
pub enum Response {
    Status(Reply),
    /// `InspectClient`: exactly one matching client exists (no secret).
    ClientPresent(ClientIdentity),
    /// `CreateClient` / `RecoverClientSecret`: PRIVATE, includes the secret.
    Client(ClientRegistration),
}

#[derive(Serialize)]
struct WireOut<'a> {
    bootstrap: BodyOut<'a>,
}
#[derive(Serialize)]
#[serde(untagged)]
enum BodyOut<'a> {
    Status(Reply),
    Present { client_present: &'a ClientIdentity },
    Created { client_created: CreatedOut<'a> },
}
#[derive(Serialize)]
struct CreatedOut<'a> {
    setup_id: Uuid,
    client_id: &'a str,
    redirect_uri: &'a str,
    client_secret: &'a str,
}

fn exact<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, Error> {
    let map = value.as_object().ok_or(Error::Invalid)?;
    if map.len() != keys.len() || keys.iter().any(|k| !map.contains_key(*k)) {
        return Err(Error::Invalid);
    }
    Ok(map)
}
fn text<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, Error> {
    map.get(key).and_then(Value::as_str).ok_or(Error::Invalid)
}
fn identity_of(map: &Map<String, Value>) -> Result<ClientIdentity, Error> {
    let identity = ClientIdentity {
        setup_id: Uuid::parse_str(text(map, "setup_id")?).map_err(|_| Error::Invalid)?,
        client_id: text(map, "client_id")?.to_owned(),
        redirect_uri: text(map, "redirect_uri")?.to_owned(),
    };
    if identity.valid() {
        Ok(identity)
    } else {
        Err(Error::Invalid)
    }
}

impl Response {
    /// The only shape `request.action` may be answered with, bound to that request.
    pub fn answers(&self, request: &Request) -> bool {
        match (request.action, self) {
            (Action::InspectClient, Self::Status(Reply::Absent)) => true,
            (Action::InspectClient, Self::ClientPresent(identity)) => identity.matches(request),
            (Action::CreateClient | Action::RecoverClientSecret, Self::Client(registration)) => {
                registration.matches(request)
            }
            (Action::InspectClient | Action::CreateClient | Action::RecoverClientSecret, _) => {
                false
            }
            (_, Self::Status(_)) => true,
            (_, _) => false,
        }
    }
    fn wire(&self) -> WireOut<'_> {
        WireOut {
            bootstrap: match self {
                Self::Status(reply) => BodyOut::Status(*reply),
                Self::ClientPresent(identity) => BodyOut::Present {
                    client_present: identity,
                },
                Self::Client(registration) => BodyOut::Created {
                    client_created: CreatedOut {
                        setup_id: registration.setup_id,
                        client_id: &registration.client_id,
                        redirect_uri: &registration.redirect_uri,
                        client_secret: registration.secret.expose(),
                    },
                },
            },
        }
    }
    /// A `Value` copy is NOT zeroized (secret included for `Client`). Production
    /// relays use `to_line`.
    pub fn to_wire(&self) -> Value {
        serde_json::to_value(self.wire()).unwrap_or(Value::Null)
    }
    /// One newline-terminated private line in a zeroizing, pre-sized buffer.
    pub fn to_line(&self) -> Result<Zeroizing<Vec<u8>>, Error> {
        let mut out = Zeroizing::new(Vec::with_capacity(PRIVATE_WIRE_LIMIT + 1));
        serde_json::to_writer(&mut *out, &self.wire()).map_err(|_| Error::Invalid)?;
        if out.len() >= PRIVATE_WIRE_LIMIT {
            return Err(Error::Invalid);
        }
        out.push(b'\n');
        Ok(out)
    }
    /// Exact shapes only: one object with one `bootstrap` member holding `"ready"`,
    /// `"absent"`, `{"client_present":{setup_id,client_id,redirect_uri}}` or
    /// `{"client_created":{setup_id,client_id,redirect_uri,client_secret}}`.
    pub fn from_wire(value: &Value) -> Result<Self, Error> {
        let body = exact(value, &["bootstrap"])?
            .get("bootstrap")
            .ok_or(Error::Invalid)?;
        if let Some(status) = body.as_str() {
            return match status {
                "ready" => Ok(Self::Status(Reply::Ready)),
                "absent" => Ok(Self::Status(Reply::Absent)),
                _ => Err(Error::Invalid),
            };
        }
        let map = body
            .as_object()
            .filter(|m| m.len() == 1)
            .ok_or(Error::Invalid)?;
        if let Some(present) = map.get("client_present") {
            let keys = ["setup_id", "client_id", "redirect_uri"];
            return identity_of(exact(present, &keys)?).map(Self::ClientPresent);
        }
        let created = map.get("client_created").ok_or(Error::Invalid)?;
        let keys = ["setup_id", "client_id", "redirect_uri", "client_secret"];
        let fields = exact(created, &keys)?;
        let identity = identity_of(fields)?;
        let secret = text(fields, "client_secret")?;
        if !valid_client_secret(secret) {
            return Err(Error::Invalid);
        }
        let secret = SecretString::new(secret.to_owned()).map_err(|_| Error::Invalid)?;
        Ok(Self::Client(ClientRegistration::new(identity, secret)))
    }
}

/// Best-effort overwrite of every string inside a parsed private line.
pub fn scrub(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(scrub),
        Value::Object(map) => map.iter_mut().for_each(|(_, v)| scrub(v)),
        _ => {}
    }
}

/// Command-order state for one worker session. Identity (setup id) is fixed by the
/// first `Authorize`, which must come first and only once.
#[derive(Default, Debug)]
pub(super) struct Sequence {
    setup: Option<Uuid>,
    owned: bool,
    /// APIs verified (by `VerifyApis` or the verifying `EnableApis`).
    apis: bool,
    /// Persisted branding verified `Ready`.
    branded: bool,
    /// Persisted scopes / audience verified `Ready`.
    scopes_ready: bool,
    audience_ready: bool,
    /// Public-information URLs verified `Ready` in this session (gates publication).
    domains_ready: bool,
    domains_configured: bool,
    publication_configured: bool,
    /// The one client WRITE (`CreateClient` or `RecoverClientSecret`) this worker may
    /// ever attempt (spent when admitted; shared, so neither can follow the other).
    client_created: bool,
    /// The inspect action whose `Absent` reply is valid for the very next command.
    absent: Option<Action>,
    /// `InspectClient` answered `ClientPresent`: valid for the very next command only.
    present: bool,
    created: bool,
    branding_configured: bool,
    scopes_configured: bool,
    audience_configured: bool,
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
        let absent = self.absent.take();
        let present = std::mem::take(&mut self.present);
        match request.action {
            Action::CreateProject if absent != Some(Action::InspectProject) || self.created => {
                Err(Error::Invalid)
            }
            Action::CreateProject => {
                self.created = true;
                Ok(())
            }
            Action::EnableApis | Action::VerifyApis if !self.owned => Err(Error::Invalid),
            Action::InspectBranding | Action::ConfigureBranding if !(self.owned && self.apis) => {
                Err(Error::Invalid)
            }
            Action::ConfigureBranding
                if absent != Some(Action::InspectBranding) || self.branding_configured =>
            {
                Err(Error::Invalid)
            }
            Action::ConfigureBranding => {
                self.branding_configured = true;
                Ok(())
            }
            Action::InspectScopes
            | Action::ConfigureScopes
            | Action::InspectAudience
            | Action::ConfigureAudience
                if !(self.owned && self.apis && self.branded) =>
            {
                Err(Error::Invalid)
            }
            Action::ConfigureAudience
                if absent != Some(Action::InspectAudience) || self.audience_configured =>
            {
                Err(Error::Invalid)
            }
            Action::ConfigureAudience => {
                self.audience_configured = true;
                Ok(())
            }
            Action::ConfigureScopes
                if absent != Some(Action::InspectScopes) || self.scopes_configured =>
            {
                Err(Error::Invalid)
            }
            Action::ConfigureScopes => {
                self.scopes_configured = true;
                Ok(())
            }
            // The saved-client precondition is a durable gateway/registry rule; this worker
            // only needs the verified Console prerequisites of its own session.
            Action::InspectDomains
            | Action::ConfigureDomains
            | Action::InspectPublication
            | Action::ConfigurePublication
                if !(self.owned
                    && self.apis
                    && self.branded
                    && self.scopes_ready
                    && self.audience_ready) =>
            {
                Err(Error::Invalid)
            }
            Action::ConfigureDomains
                if absent != Some(Action::InspectDomains) || self.domains_configured =>
            {
                Err(Error::Invalid)
            }
            Action::ConfigureDomains => {
                self.domains_configured = true;
                Ok(())
            }
            Action::InspectPublication | Action::ConfigurePublication if !self.domains_ready => {
                Err(Error::Invalid)
            }
            Action::ConfigurePublication
                if absent != Some(Action::InspectPublication) || self.publication_configured =>
            {
                Err(Error::Invalid)
            }
            Action::ConfigurePublication => {
                self.publication_configured = true;
                Ok(())
            }
            Action::InspectClient | Action::CreateClient | Action::RecoverClientSecret
                if !(self.owned
                    && self.apis
                    && self.branded
                    && self.scopes_ready
                    && self.audience_ready) =>
            {
                Err(Error::Invalid)
            }
            Action::CreateClient
                if absent != Some(Action::InspectClient) || self.client_created =>
            {
                Err(Error::Invalid)
            }
            Action::CreateClient => {
                // Spent on admission: an uncertain or failed create is never replayed.
                self.client_created = true;
                Ok(())
            }
            Action::RecoverClientSecret if !present || self.client_created => Err(Error::Invalid),
            Action::RecoverClientSecret => {
                // Same shared budget: an uncertain Add is never repeated, and a worker
                // that created the client can never also add a secret.
                self.client_created = true;
                Ok(())
            }
            _ => Ok(()),
        }
    }
    /// Records a successful action's response.
    pub fn record_response(&mut self, action: Action, response: &Response) {
        match response {
            Response::Status(reply) => self.record(action, *reply),
            Response::ClientPresent(_) if action == Action::InspectClient => self.present = true,
            _ => {}
        }
    }
    /// Records a successful action's reply.
    pub fn record(&mut self, action: Action, reply: Reply) {
        match (action, reply) {
            (
                Action::InspectProject
                | Action::InspectBranding
                | Action::InspectScopes
                | Action::InspectAudience
                | Action::InspectDomains
                | Action::InspectPublication
                | Action::InspectClient,
                Reply::Absent,
            ) => self.absent = Some(action),
            (Action::InspectDomains | Action::ConfigureDomains, Reply::Ready) => {
                self.domains_ready = true
            }
            (Action::InspectScopes | Action::ConfigureScopes, Reply::Ready) => {
                self.scopes_ready = true
            }
            (Action::InspectAudience | Action::ConfigureAudience, Reply::Ready) => {
                self.audience_ready = true
            }
            (Action::InspectProject | Action::CreateProject, Reply::Ready) => self.owned = true,
            (Action::EnableApis | Action::VerifyApis, Reply::Ready) => self.apis = true,
            (Action::InspectBranding | Action::ConfigureBranding, Reply::Ready) => {
                self.branded = true
            }
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

    fn ready_session(b: &Binding, id: Uuid) -> Sequence {
        let mut s = Sequence::default();
        s.admit(&request(b, id, Action::Authorize), b).unwrap();
        s.admit(&request(b, id, Action::InspectProject), b).unwrap();
        s.record(Action::InspectProject, Reply::Ready);
        s.admit(&request(b, id, Action::VerifyApis), b).unwrap();
        s.record(Action::VerifyApis, Reply::Ready);
        s
    }

    #[test]
    fn console_actions_wire_names_and_prerequisites() {
        let b = binding();
        let id = Uuid::new_v4();
        let wire = request(&b, id, Action::ConfigureScopes).to_wire();
        assert_eq!(wire["bootstrap"]["action"], "configure_scopes");
        let mut s = Sequence::default();
        s.admit(&request(&b, id, Action::Authorize), &b).unwrap();
        for a in [
            Action::InspectBranding,
            Action::ConfigureBranding,
            Action::InspectScopes,
            Action::ConfigureScopes,
        ] {
            assert!(s.admit(&request(&b, id, a), &b).is_err(), "{a:?}");
        }
        // Owned but APIs not verified yet.
        s.admit(&request(&b, id, Action::InspectProject), &b)
            .unwrap();
        s.record(Action::InspectProject, Reply::Ready);
        assert!(
            s.admit(&request(&b, id, Action::InspectBranding), &b)
                .is_err()
        );
        s.admit(&request(&b, id, Action::EnableApis), &b).unwrap();
        s.record(Action::EnableApis, Reply::Ready);
        s.admit(&request(&b, id, Action::InspectBranding), &b)
            .unwrap();
        // Scopes need verified branding.
        assert!(
            s.admit(&request(&b, id, Action::InspectScopes), &b)
                .is_err()
        );
    }

    #[test]
    fn configure_only_right_after_its_own_absent_and_once() {
        let b = binding();
        let id = Uuid::new_v4();
        let mut s = ready_session(&b, id);
        assert!(
            s.admit(&request(&b, id, Action::ConfigureBranding), &b)
                .is_err()
        );
        s.admit(&request(&b, id, Action::InspectBranding), &b)
            .unwrap();
        s.record(Action::InspectBranding, Reply::Absent);
        // A different inspect's absence (project) does not authorise it either.
        s.admit(&request(&b, id, Action::ConfigureBranding), &b)
            .unwrap();
        s.record(Action::ConfigureBranding, Reply::Ready);
        s.admit(&request(&b, id, Action::InspectBranding), &b)
            .unwrap();
        s.record(Action::InspectBranding, Reply::Absent);
        assert!(
            s.admit(&request(&b, id, Action::ConfigureBranding), &b)
                .is_err()
        );
        // Scopes: absence proof is per-inspect and spent by any command.
        s.admit(&request(&b, id, Action::InspectScopes), &b)
            .unwrap();
        s.record(Action::InspectScopes, Reply::Absent);
        s.admit(&request(&b, id, Action::VerifyApis), &b).unwrap();
        assert!(
            s.admit(&request(&b, id, Action::ConfigureScopes), &b)
                .is_err()
        );
        s.admit(&request(&b, id, Action::InspectScopes), &b)
            .unwrap();
        s.record(Action::InspectScopes, Reply::Absent);
        s.admit(&request(&b, id, Action::ConfigureScopes), &b)
            .unwrap();
        s.record(Action::ConfigureScopes, Reply::Ready);
        s.admit(&request(&b, id, Action::InspectScopes), &b)
            .unwrap();
        s.record(Action::InspectScopes, Reply::Absent);
        assert!(
            s.admit(&request(&b, id, Action::ConfigureScopes), &b)
                .is_err()
        );
        // Wrong inspect: branding absence cannot unlock scopes configure.
        let mut t = ready_session(&b, id);
        t.admit(&request(&b, id, Action::InspectBranding), &b)
            .unwrap();
        t.record(Action::InspectBranding, Reply::Ready);
        t.admit(&request(&b, id, Action::InspectBranding), &b)
            .unwrap();
        t.record(Action::InspectBranding, Reply::Absent);
        assert!(
            t.admit(&request(&b, id, Action::ConfigureScopes), &b)
                .is_err()
        );
        t.admit(&request(&b, id, Action::Finish), &b).unwrap();
    }

    fn identity(b: &Binding, id: Uuid) -> ClientIdentity {
        ClientIdentity {
            setup_id: id,
            client_id: "123456-abc.apps.googleusercontent.com".into(),
            redirect_uri: Request::callback(b),
        }
    }
    fn created(b: &Binding, id: Uuid) -> ClientRegistration {
        ClientRegistration::new(
            identity(b, id),
            SecretString::new("GOCSPX-canary-secret-value".into()).unwrap(),
        )
    }

    #[test]
    fn private_response_wire_is_strict_bounded_and_redacted() {
        let b = binding();
        let id = Uuid::new_v4();
        // Status replies keep the original wire.
        assert!(matches!(
            Response::from_wire(&serde_json::json!({"bootstrap":"ready"})),
            Ok(Response::Status(Reply::Ready))
        ));
        let present = Response::ClientPresent(identity(&b, id));
        let wire = present.to_wire();
        assert!(wire.to_string().find("secret").is_none());
        assert!(matches!(
            Response::from_wire(&wire),
            Ok(Response::ClientPresent(i)) if i == identity(&b, id)
        ));
        let registration = Response::Client(created(&b, id));
        let wire = registration.to_wire();
        let Ok(Response::Client(parsed)) = Response::from_wire(&wire) else {
            panic!("created response must round trip");
        };
        assert_eq!(parsed.secret.expose(), "GOCSPX-canary-secret-value");
        let debug = format!("{parsed:?} {registration:?}");
        assert!(!debug.contains("canary"), "{debug}");
        let line = registration.to_line().unwrap();
        assert!(line.len() < PRIVATE_WIRE_LIMIT && line.ends_with(b"\n"));
        // Extra members, wrong nesting, missing fields, masked or hostile values.
        let mut extra = wire.clone();
        extra["bootstrap"]["client_created"]["note"] = "x".into();
        let mut sibling = wire.clone();
        sibling["other"] = 1.into();
        let mut masked = wire.clone();
        masked["bootstrap"]["client_created"]["client_secret"] = "GOCSPX-****".into();
        let mut bad_id = wire.clone();
        bad_id["bootstrap"]["client_created"]["client_id"] = "evil.example".into();
        let mut nil = wire.clone();
        nil["bootstrap"]["client_created"]["setup_id"] = Uuid::nil().to_string().into();
        let mut missing = wire.clone();
        missing["bootstrap"]["client_created"]
            .as_object_mut()
            .unwrap()
            .remove("client_secret");
        let mut secret_on_present = Response::ClientPresent(identity(&b, id)).to_wire();
        secret_on_present["bootstrap"]["client_present"]["client_secret"] =
            "GOCSPX-abcdefgh".into();
        for bad in [
            extra,
            sibling,
            masked,
            bad_id,
            nil,
            missing,
            secret_on_present,
            serde_json::json!({"bootstrap":{"client_present":{},"client_created":{}}}),
            serde_json::json!({"bootstrap":{"client":{}}}),
            serde_json::json!({"bootstrap":null}),
        ] {
            assert!(Response::from_wire(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn responses_must_answer_their_own_request() {
        let b = binding();
        let id = Uuid::new_v4();
        let inspect = request(&b, id, Action::InspectClient);
        let create = request(&b, id, Action::CreateClient);
        let other = request(&b, id, Action::InspectScopes);
        assert!(Response::Status(Reply::Absent).answers(&inspect));
        assert!(!Response::Status(Reply::Ready).answers(&inspect));
        assert!(Response::ClientPresent(identity(&b, id)).answers(&inspect));
        assert!(!Response::ClientPresent(identity(&b, id)).answers(&create));
        assert!(!Response::ClientPresent(identity(&b, id)).answers(&other));
        assert!(Response::Client(created(&b, id)).answers(&create));
        assert!(!Response::Client(created(&b, id)).answers(&inspect));
        assert!(!Response::Status(Reply::Ready).answers(&create));
        assert!(Response::Status(Reply::Ready).answers(&other));
        // Another setup or a different callback is a wrong binding.
        assert!(!Response::ClientPresent(identity(&b, Uuid::new_v4())).answers(&inspect));
        let mut foreign = identity(&b, id);
        foreign.redirect_uri = "https://evil.example/api/v1/mail/oauth/callback".into();
        assert!(!Response::ClientPresent(foreign).answers(&inspect));
        assert_eq!(create.client_name(), format!("Moosuri {}", id.simple()));
    }

    #[test]
    fn private_client_answer_is_only_for_create_or_recover() {
        let b = binding();
        let id = Uuid::new_v4();
        let recover = request(&b, id, Action::RecoverClientSecret);
        assert_eq!(
            recover.to_wire()["bootstrap"]["action"],
            "recover_client_secret"
        );
        assert!(Response::Client(created(&b, id)).answers(&recover));
        // A foreign setup, a foreign callback or a non-private reply never answers it.
        assert!(!Response::Client(created(&b, Uuid::new_v4())).answers(&recover));
        let mut foreign = created(&b, id);
        foreign.redirect_uri = "https://evil.example/api/v1/mail/oauth/callback".into();
        assert!(!Response::Client(foreign).answers(&recover));
        assert!(!Response::Status(Reply::Ready).answers(&recover));
        assert!(!Response::Status(Reply::Absent).answers(&recover));
        assert!(!Response::ClientPresent(identity(&b, id)).answers(&recover));
        // The private shape cannot answer any other action.
        for action in [
            Action::InspectClient,
            Action::InspectProject,
            Action::VerifyApis,
            Action::InspectScopes,
            Action::Finish,
        ] {
            assert!(
                !Response::Client(created(&b, id)).answers(&request(&b, id, action)),
                "{action:?}"
            );
        }
    }

    #[test]
    fn recover_needs_readiness_present_immediately_before_and_shares_one_write() {
        let b = binding();
        let id = Uuid::new_v4();
        let mut s = ready_session(&b, id);
        // Prerequisites missing.
        assert!(
            s.admit(&request(&b, id, Action::RecoverClientSecret), &b)
                .is_err()
        );
        for (inspect, configure) in [
            (Action::InspectBranding, Action::ConfigureBranding),
            (Action::InspectScopes, Action::ConfigureScopes),
            (Action::InspectAudience, Action::ConfigureAudience),
        ] {
            s.admit(&request(&b, id, inspect), &b).unwrap();
            s.record(inspect, Reply::Absent);
            s.admit(&request(&b, id, configure), &b).unwrap();
            s.record(configure, Reply::Ready);
        }
        let recover = request(&b, id, Action::RecoverClientSecret);
        // No Inspect proof at all, then an Absent proof (never enough).
        assert!(s.admit(&recover, &b).is_err());
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(Action::InspectClient, &Response::Status(Reply::Absent));
        assert!(s.admit(&recover, &b).is_err());
        // Present is spent by any intervening command.
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(
            Action::InspectClient,
            &Response::ClientPresent(identity(&b, id)),
        );
        s.admit(&request(&b, id, Action::VerifyApis), &b).unwrap();
        assert!(s.admit(&recover, &b).is_err());
        // A present proof from a different action never counts.
        s.record_response(
            Action::VerifyApis,
            &Response::ClientPresent(identity(&b, id)),
        );
        assert!(s.admit(&recover, &b).is_err());
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(
            Action::InspectClient,
            &Response::ClientPresent(identity(&b, id)),
        );
        s.admit(&recover, &b).unwrap();
        // Once: a second recovery (even with a fresh present proof) or a create is refused.
        s.record_response(
            Action::RecoverClientSecret,
            &Response::Client(created(&b, id)),
        );
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(
            Action::InspectClient,
            &Response::ClientPresent(identity(&b, id)),
        );
        assert!(s.admit(&recover, &b).is_err());
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(Action::InspectClient, &Response::Status(Reply::Absent));
        assert!(s.admit(&request(&b, id, Action::CreateClient), &b).is_err());

        // Never after a spent create in the same worker.
        let mut t = ready_session(&b, id);
        for (inspect, configure) in [
            (Action::InspectBranding, Action::ConfigureBranding),
            (Action::InspectScopes, Action::ConfigureScopes),
            (Action::InspectAudience, Action::ConfigureAudience),
        ] {
            t.admit(&request(&b, id, inspect), &b).unwrap();
            t.record(inspect, Reply::Absent);
            t.admit(&request(&b, id, configure), &b).unwrap();
            t.record(configure, Reply::Ready);
        }
        t.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        t.record_response(Action::InspectClient, &Response::Status(Reply::Absent));
        t.admit(&request(&b, id, Action::CreateClient), &b).unwrap();
        t.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        t.record_response(
            Action::InspectClient,
            &Response::ClientPresent(identity(&b, id)),
        );
        assert!(t.admit(&recover, &b).is_err());
    }

    #[test]
    fn client_actions_need_full_readiness_and_create_is_once_after_absent() {
        let b = binding();
        let id = Uuid::new_v4();
        let mut s = ready_session(&b, id);
        // APIs verified but branding, scopes and audience are not.
        for a in [Action::InspectClient, Action::CreateClient] {
            assert!(s.admit(&request(&b, id, a), &b).is_err(), "{a:?}");
        }
        for (inspect, configure) in [
            (Action::InspectBranding, Action::ConfigureBranding),
            (Action::InspectScopes, Action::ConfigureScopes),
            (Action::InspectAudience, Action::ConfigureAudience),
        ] {
            s.admit(&request(&b, id, inspect), &b).unwrap();
            s.record(inspect, Reply::Absent);
            s.admit(&request(&b, id, configure), &b).unwrap();
            s.record(configure, Reply::Ready);
            // Still not ready for create until every prerequisite is ready.
            assert!(s.admit(&request(&b, id, Action::CreateClient), &b).is_err());
        }
        // Create without its immediate Absent proof, then with a spent proof.
        assert!(s.admit(&request(&b, id, Action::CreateClient), &b).is_err());
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(Action::InspectClient, &Response::Status(Reply::Absent));
        s.admit(&request(&b, id, Action::VerifyApis), &b).unwrap();
        assert!(s.admit(&request(&b, id, Action::CreateClient), &b).is_err());
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(Action::InspectClient, &Response::Status(Reply::Absent));
        s.admit(&request(&b, id, Action::CreateClient), &b).unwrap();
        // A present client does not unlock creation, and no replay exists.
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(
            Action::InspectClient,
            &Response::ClientPresent(identity(&b, id)),
        );
        assert!(s.admit(&request(&b, id, Action::CreateClient), &b).is_err());
        s.admit(&request(&b, id, Action::InspectClient), &b)
            .unwrap();
        s.record_response(Action::InspectClient, &Response::Status(Reply::Absent));
        assert!(s.admit(&request(&b, id, Action::CreateClient), &b).is_err());
        // A wrong callback never reaches the sequence.
        let mut wrong = request(&b, id, Action::InspectClient);
        wrong.redirect_uri = "https://evil.example/api/v1/mail/oauth/callback".into();
        assert!(s.admit(&wrong, &b).is_err());
    }

    #[test]
    fn publication_actions_need_console_readiness_domains_first_and_are_once_after_absent() {
        use Action::*;
        let b = binding();
        let id = Uuid::new_v4();
        let mut s = ready_session(&b, id);
        assert_eq!(
            request(&b, id, ConfigurePublication).to_wire()["bootstrap"]["action"],
            "configure_publication"
        );
        for a in [
            InspectDomains,
            ConfigureDomains,
            InspectPublication,
            ConfigurePublication,
        ] {
            assert!(s.admit(&request(&b, id, a), &b).is_err(), "{a:?}");
        }
        for inspect in [InspectBranding, InspectScopes, InspectAudience] {
            s.admit(&request(&b, id, inspect), &b).unwrap();
            s.record(inspect, Reply::Ready);
        }
        // Publication needs this session's verified public information first.
        assert!(s.admit(&request(&b, id, InspectPublication), &b).is_err());
        assert!(s.admit(&request(&b, id, ConfigureDomains), &b).is_err());
        s.admit(&request(&b, id, InspectDomains), &b).unwrap();
        s.record(InspectDomains, Reply::Absent);
        s.admit(&request(&b, id, ConfigureDomains), &b).unwrap();
        s.record(ConfigureDomains, Reply::Ready);
        s.admit(&request(&b, id, InspectDomains), &b).unwrap();
        s.record(InspectDomains, Reply::Absent);
        assert!(s.admit(&request(&b, id, ConfigureDomains), &b).is_err());
        // Another inspection's absence never unlocks the publish click.
        s.admit(&request(&b, id, InspectDomains), &b).unwrap();
        s.record(InspectDomains, Reply::Absent);
        assert!(s.admit(&request(&b, id, ConfigurePublication), &b).is_err());
        s.admit(&request(&b, id, InspectPublication), &b).unwrap();
        s.record(InspectPublication, Reply::Absent);
        s.admit(&request(&b, id, ConfigurePublication), &b).unwrap();
        s.record(ConfigurePublication, Reply::Ready);
        s.admit(&request(&b, id, InspectPublication), &b).unwrap();
        s.record(InspectPublication, Reply::Absent);
        assert!(s.admit(&request(&b, id, ConfigurePublication), &b).is_err());
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
