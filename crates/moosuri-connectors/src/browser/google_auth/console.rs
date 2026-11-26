//! Bounded Google Auth Platform (Console) branding and Data Access scope setup,
//! driven in the SAME authenticated Chrome as the SDK bootstrap.
//!
//! Nothing here accepts a URL, script, selector, scope or label from a command.
//! Every value is derived from the bound, verified Google email, the stable
//! project id (itself derived from the request's `setup_id`) and fixed constants.
//! No OAuth client is created and no secret is read or emitted. Billing is never
//! touched: the only controls the fixed script may click are listed below.
//!
//! # Replies (via `bootstrap::Reply`, which stays `Ready | Absent`)
//! * `InspectBranding`: `Ready` = persisted branding equals the expected values
//!   (app "Moosuri", support and contact email = bound account); `Absent` = the
//!   unconfigured "Get started" page seen on two consecutive polls. Anything else is
//!   an error (a different app, wrong account, foreign page => `Blocked`;
//!   ambiguous/unsupported DOM or timeout => `NeedsHuman`).
//! * `ConfigureBranding`: inspects first (response loss => inspection, never a
//!   second Create); `Ready` if already correct, otherwise walks the wizard and
//!   returns `Ready` only after a fresh page load reads the expected values back.
//! * `InspectScopes`: `Ready` = persisted table is exactly the seven fixed scopes;
//!   `Absent` = a strict subset (including empty). Any other scope => `Blocked`.
//! * `ConfigureScopes`: same inspect-first rule, then Add or remove scopes ->
//!   manual input -> Add to table -> Update -> Save, then persisted readback.
//!
//! * `InspectDomains` / `ConfigureDomains` (Branding page, after the client is saved):
//!   `Ready` = the Application home page, privacy policy and terms of service inputs (found
//!   by their real label association) hold exactly `<callback origin>/about`, `/privacy`,
//!   `/terms` AND a listed authorized domain covers the origin hostname on a DNS label
//!   boundary (exact hostname or a dotted parent). `Absent` = at least one URL blank and no
//!   conflict. A different non-empty URL, other branding or another account => `Blocked`; no
//!   covering authorized domain => `NeedsHuman` and NOTHING is written (no public suffix
//!   list exists here, so no "registrable root" is ever guessed or added). Configure fills
//!   only blank URLs, clicks Save once, then a fresh load must read all three back.
//! * `InspectPublication` / `ConfigurePublication` (Audience page): `Ready` = the exact
//!   "Publishing status" field reads `In production` for an External app (banner text is
//!   never read); `Absent` = `Testing` with an ENABLED "Publish app" action; a disabled
//!   action (prerequisites missing) or any other status => `NeedsHuman`. Configure clicks
//!   Publish app, then only the exact "Push to production?" dialog's own Confirm. LIVE-UNPROVEN:
//!   that dialog is the documented conventional one and has not been observed; any other
//!   dialog or extra control is refused untouched. This is publication only: Google
//!   verification, the unverified-app warning and the user cap are untouched and not claimed.
//!   The old `InspectAudience`/`ConfigureAudience` treat an External `In production` app as
//!   ready without adding a test user.
//!
//! # Finite states and safety rules
//! The page script returns exactly one of: `waiting`, `foreign` (origin, path or
//! `project` differs), `wrong_account`, `ambiguous` (duplicate matching controls,
//! unexpected chips/options), `unsupported` (no recognised state),
//! `branding_match`, `branding_mismatch`, `scopes_match`, `scopes_extra`, a step
//! name (see `Step`), and for act calls `acted` or `changed`. The driver acts on a
//! step only if it is strictly later than every step already acted on, so a stale
//! or slow page can never cause a repeated mutating click. A step is re-validated
//! inside the page at click time (`changed` = no click). The wizard may only start
//! from `get_started`; a half-open wizard is never adopted. Existing different
//! branding is never overwritten. Any error taints Chrome (see `Chrome::console`).
//!
//! # NOT PROVEN
//! The DOM heuristics (label text, roles, `hl=en` English strings, chip classes,
//! manual-scope `textarea`, table cells holding exact scope strings, the Google
//! Account avatar label as identity proof) come from observed behaviour, not from a
//! provider contract, and synthetic tests only exercise the Rust state machine.
//! Unrecognised DOM yields `unsupported` => `NeedsHuman`; it must be live-verified.
use super::{
    Error,
    bootstrap::{Action, Reply},
    sdk::{Email, ProjectId},
};
use serde::Serialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
mod tests;

/// The only scopes ever configured (and the only ones tolerated as present).
pub(super) const SCOPES: [&str; 7] = [
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/drive.metadata.readonly",
    "https://www.googleapis.com/auth/documents.readonly",
    "https://www.googleapis.com/auth/spreadsheets.readonly",
    "https://www.googleapis.com/auth/presentations.readonly",
    "openid",
    "https://www.googleapis.com/auth/userinfo.email",
];
pub(super) const APP_NAME: &str = "Moosuri";
const PREFIX: &str = "https://console.cloud.google.com/auth/";
const POLLS: usize = 30;
const DRIVE_POLLS: usize = 200;
const DRIVE_IDLE: usize = 40;
const RELOADS: usize = 3;
const PAUSE: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    InspectBranding,
    ConfigureBranding,
    InspectScopes,
    ConfigureScopes,
    InspectAudience,
    ConfigureAudience,
    InspectDomains,
    ConfigureDomains,
    InspectPublication,
    ConfigurePublication,
}

impl Op {
    pub fn from_action(action: Action) -> Option<Self> {
        match action {
            Action::InspectDomains => Some(Self::InspectDomains),
            Action::ConfigureDomains => Some(Self::ConfigureDomains),
            Action::InspectPublication => Some(Self::InspectPublication),
            Action::ConfigurePublication => Some(Self::ConfigurePublication),
            Action::InspectBranding => Some(Self::InspectBranding),
            Action::ConfigureBranding => Some(Self::ConfigureBranding),
            Action::InspectAudience => Some(Self::InspectAudience),
            Action::ConfigureAudience => Some(Self::ConfigureAudience),
            Action::InspectScopes => Some(Self::InspectScopes),
            Action::ConfigureScopes => Some(Self::ConfigureScopes),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Branding,
    Scopes,
    Audience,
    /// Public information URLs on the Branding page.
    Domains,
    /// Publishing status on the Audience page.
    Publication,
}

/// Fixed per-request values. Constructed only from parsed, validated types.
#[derive(Clone)]
pub(super) struct Spec {
    project: String,
    email: String,
    /// Validated `https://<dns-hostname>[:port]` callback origin and its lowercase hostname.
    site: Option<(String, String)>,
}

/// The hostname of a plain https origin that can be an authorized domain: a DNS name with at
/// least two labels (never an IP literal), no userinfo/path/query/fragment.
fn site_host(origin: &str) -> Option<String> {
    let url = url::Url::parse(origin).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || origin != url.origin().ascii_serialization()
    {
        return None;
    }
    let url::Host::Domain(host) = url.host()? else {
        return None;
    };
    let labels: Vec<&str> = host.split('.').collect();
    let sane = host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|l| {
            (1..=63).contains(&l.len())
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
        && !labels
            .last()
            .is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()));
    sane.then(|| host.to_owned())
}

impl Spec {
    pub fn new(project: &ProjectId, email: &Email) -> Self {
        Self {
            project: project.as_str().to_owned(),
            email: email.as_str().to_owned(),
            site: None,
        }
    }
    /// For the public-information operations: the installation's validated callback origin.
    pub fn with_site(project: &ProjectId, email: &Email, origin: &str) -> Result<Self, Error> {
        let host = site_host(origin).ok_or(Error::NeedsHuman)?;
        Ok(Self {
            site: Some((origin.to_owned(), host)),
            ..Self::new(project, email)
        })
    }
    pub fn url(&self, mode: Mode) -> String {
        let page = match mode {
            Mode::Audience | Mode::Publication => "audience",
            Mode::Branding | Mode::Domains => "branding",
            Mode::Scopes => "scopes",
        };
        format!("{PREFIX}{page}?project={}&hl=en", self.project)
    }
}

/// One evaluation of the fixed page script.
#[derive(Clone, Copy, Debug)]
pub(super) struct Call {
    pub mode: Mode,
    /// Last step acted on ("" = none); the script is context-dependent.
    pub last: &'static str,
    /// Empty = read-only peek; otherwise act only if the page still shows it.
    pub expected: &'static str,
}

/// Minimal browser surface; Chrome implements it, tests use a scripted fake.
#[allow(async_fn_in_trait)]
pub(super) trait Port {
    async fn navigate(&mut self, url: &str) -> Result<(), Error>;
    async fn eval(&mut self, spec: &Spec, call: &Call) -> Result<String, Error>;
    async fn pause(&mut self, duration: Duration);
}

/// Wizard / dialog steps in strict progression order (derive order is the rank).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    GetStarted,
    AppName,
    SupportOpen,
    SupportPick,
    AppNext,
    AudiencePick,
    AudienceNext,
    ContactFill,
    ContactNext,
    AgreeCheck,
    AgreeContinue,
    Create,
    ScopesOpen,
    ScopesInput,
    ScopesAddToTable,
    ScopesUpdate,
    ScopesSave,
    AudienceOpen,
    AudienceInput,
    AudienceSave,
    DomainsFill,
    DomainsSave,
    PublishOpen,
    PublishConfirm,
}

const STEPS: [(Step, &str); 24] = [
    (Step::GetStarted, "get_started"),
    (Step::AppName, "app_name"),
    (Step::SupportOpen, "support_open"),
    (Step::SupportPick, "support_pick"),
    (Step::AppNext, "app_next"),
    (Step::AudiencePick, "audience_pick"),
    (Step::AudienceNext, "audience_next"),
    (Step::ContactFill, "contact_fill"),
    (Step::ContactNext, "contact_next"),
    (Step::AgreeCheck, "agree_check"),
    (Step::AgreeContinue, "agree_continue"),
    (Step::Create, "create"),
    (Step::ScopesOpen, "scopes_partial"),
    (Step::ScopesInput, "scopes_input"),
    (Step::ScopesAddToTable, "scopes_add_table"),
    (Step::ScopesUpdate, "scopes_update"),
    (Step::ScopesSave, "scopes_save"),
    (Step::AudienceOpen, "audience_partial"),
    (Step::AudienceInput, "audience_input"),
    (Step::AudienceSave, "audience_save"),
    (Step::DomainsFill, "domains_fill"),
    (Step::DomainsSave, "domains_save"),
    (Step::PublishOpen, "publish_open"),
    (Step::PublishConfirm, "publish_confirm"),
];

impl Step {
    fn wire(self) -> &'static str {
        STEPS
            .iter()
            .find(|(s, _)| *s == self)
            .map_or("", |(_, w)| w)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    Waiting,
    Step(Step),
    BrandingMatch,
    ScopesMatch,
    AudienceMatch,
    DomainsMatch,
    PublicationMatch,
    /// Foreign page, wrong account, different persisted state: never overwritten.
    Blocked,
    /// Duplicate controls or unrecognised DOM: a human must look.
    Human,
    Acted,
    Changed,
}

impl Seen {
    fn parse(raw: &str) -> Result<Self, Error> {
        Ok(match raw {
            "waiting" => Self::Waiting,
            "branding_match" => Self::BrandingMatch,
            "scopes_match" => Self::ScopesMatch,
            "audience_match" => Self::AudienceMatch,
            "domains_match" => Self::DomainsMatch,
            "publication_match" => Self::PublicationMatch,
            "foreign" | "wrong_account" | "branding_mismatch" | "scopes_extra"
            | "audience_mismatch" | "domains_mismatch" | "publication_mismatch" => Self::Blocked,
            // `domain_unproven`: no listed authorized domain covers the hostname.
            // `publish_blocked`: the Publish app action is disabled.
            "ambiguous" | "unsupported" | "domain_unproven" | "publish_blocked" => Self::Human,
            "acted" => Self::Acted,
            "changed" => Self::Changed,
            other => STEPS
                .iter()
                .find(|(_, w)| *w == other)
                .map(|(s, _)| Self::Step(*s))
                .ok_or(Error::Unavailable)?,
        })
    }
    fn refusal(self) -> Error {
        if self == Self::Blocked {
            Error::Blocked
        } else {
            Error::NeedsHuman
        }
    }
}

pub(super) fn expression(spec: &Spec, call: &Call) -> Result<String, Error> {
    let args = serde_json::to_string(&(
        call.mode,
        spec.project.as_str(),
        spec.email.as_str(),
        call.last,
        SCOPES,
        call.expected,
        APP_NAME,
        spec.site.as_ref().map_or("", |(origin, _)| origin.as_str()),
        spec.site.as_ref().map_or("", |(_, host)| host.as_str()),
    ))
    .map_err(|_| Error::Invalid)?;
    Ok(format!("({SCRIPT})(...{args})"))
}

async fn observe<P: Port>(
    port: &mut P,
    spec: &Spec,
    call: Call,
    cancel: &CancellationToken,
) -> Result<Seen, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Unavailable);
    }
    Seen::parse(&port.eval(spec, &call).await?)
}

/// Polls a read-only peek until it is not `waiting`.
async fn first_seen<P: Port>(
    port: &mut P,
    spec: &Spec,
    mode: Mode,
    cancel: &CancellationToken,
) -> Result<Seen, Error> {
    for _ in 0..POLLS {
        let call = Call {
            mode,
            last: "",
            expected: "",
        };
        match observe(port, spec, call, cancel).await? {
            Seen::Waiting => port.pause(PAUSE).await,
            seen => return Ok(seen),
        }
    }
    Err(Error::NeedsHuman)
}

/// Fresh navigation then read-only classification. Never clicks.
async fn inspect<P: Port>(
    port: &mut P,
    spec: &Spec,
    mode: Mode,
    cancel: &CancellationToken,
) -> Result<Reply, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Unavailable);
    }
    port.navigate(&spec.url(mode)).await?;
    let first = first_seen(port, spec, mode, cancel).await?;
    match (mode, first) {
        (Mode::Branding, Seen::BrandingMatch)
        | (Mode::Scopes, Seen::ScopesMatch)
        | (Mode::Audience, Seen::AudienceMatch)
        | (Mode::Domains, Seen::DomainsMatch)
        | (Mode::Publication, Seen::PublicationMatch) => Ok(Reply::Ready),
        (Mode::Branding, Seen::Step(Step::GetStarted))
        | (Mode::Scopes, Seen::Step(Step::ScopesOpen))
        | (Mode::Audience, Seen::Step(Step::AudienceOpen))
        | (Mode::Domains, Seen::Step(Step::DomainsFill))
        | (Mode::Publication, Seen::Step(Step::PublishOpen)) => {
            // Absence must be stable, not a loading skeleton.
            port.pause(PAUSE).await;
            if first_seen(port, spec, mode, cancel).await? == first {
                Ok(Reply::Absent)
            } else {
                Err(Error::NeedsHuman)
            }
        }
        (_, other) => Err(other.refusal()),
    }
}

/// Walks the wizard/dialog. Each step is acted on at most once and only in
/// strictly increasing order; a page that does not advance ends in `NeedsHuman`.
async fn drive<P: Port>(
    port: &mut P,
    spec: &Spec,
    mode: Mode,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    let (first, terminal) = match mode {
        Mode::Branding => (Step::GetStarted, Step::Create),
        Mode::Scopes => (Step::ScopesOpen, Step::ScopesSave),
        Mode::Audience => (Step::AudienceOpen, Step::AudienceSave),
        Mode::Domains => (Step::DomainsFill, Step::DomainsSave),
        Mode::Publication => (Step::PublishOpen, Step::PublishConfirm),
    };
    let mut last: Option<Step> = None;
    let mut idle = 0usize;
    for _ in 0..DRIVE_POLLS {
        let wire = last.map_or("", Step::wire);
        let peek = Call {
            mode,
            last: wire,
            expected: "",
        };
        match observe(port, spec, peek, cancel).await? {
            Seen::Waiting => idle += 1,
            Seen::BrandingMatch if mode == Mode::Branding => return Ok(()),
            Seen::Step(step) => {
                let fresh = match last {
                    None => true,
                    Some(done) => step > done,
                };
                if !fresh {
                    idle += 1;
                } else {
                    if last.is_none() && step != first {
                        return Err(Error::NeedsHuman);
                    }
                    let act = Call {
                        mode,
                        last: wire,
                        expected: step.wire(),
                    };
                    match observe(port, spec, act, cancel).await? {
                        Seen::Acted => {
                            last = Some(step);
                            idle = 0;
                            if step == terminal {
                                return Ok(());
                            }
                        }
                        Seen::Changed | Seen::Waiting | Seen::Step(_) => idle += 1,
                        other => return Err(other.refusal()),
                    }
                }
            }
            other => return Err(other.refusal()),
        }
        if idle > DRIVE_IDLE {
            return Err(Error::NeedsHuman);
        }
        port.pause(PAUSE).await;
    }
    Err(Error::NeedsHuman)
}

/// After the mutation: only a fresh load that reads the expected state back counts.
/// "Still unconfigured" is waited for, never re-clicked.
async fn settle<P: Port>(
    port: &mut P,
    spec: &Spec,
    mode: Mode,
    cancel: &CancellationToken,
) -> Result<Reply, Error> {
    for _ in 0..RELOADS {
        match inspect(port, spec, mode, cancel).await? {
            Reply::Ready => return Ok(Reply::Ready),
            Reply::Absent => port.pause(PAUSE * 3).await,
        }
    }
    Err(Error::NeedsHuman)
}

async fn configure<P: Port>(
    port: &mut P,
    spec: &Spec,
    mode: Mode,
    cancel: &CancellationToken,
) -> Result<Reply, Error> {
    // Inspect before any click: a lost reply or repeated request finds the result.
    if inspect(port, spec, mode, cancel).await? == Reply::Ready {
        return Ok(Reply::Ready);
    }
    drive(port, spec, mode, cancel).await?;
    settle(port, spec, mode, cancel).await
}

/// Entry point. `Ready` for configure ops means persisted readback matched.
pub(super) async fn run<P: Port>(
    port: &mut P,
    op: Op,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<Reply, Error> {
    if matches!(op, Op::InspectDomains | Op::ConfigureDomains) && spec.site.is_none() {
        return Err(Error::NeedsHuman);
    }
    match op {
        Op::InspectDomains => inspect(port, spec, Mode::Domains, cancel).await,
        Op::ConfigureDomains => configure(port, spec, Mode::Domains, cancel).await,
        Op::InspectPublication => inspect(port, spec, Mode::Publication, cancel).await,
        Op::ConfigurePublication => configure(port, spec, Mode::Publication, cancel).await,
        Op::InspectAudience => inspect(port, spec, Mode::Audience, cancel).await,
        Op::ConfigureAudience => configure(port, spec, Mode::Audience, cancel).await,
        Op::InspectBranding => inspect(port, spec, Mode::Branding, cancel).await,
        Op::InspectScopes => inspect(port, spec, Mode::Scopes, cancel).await,
        Op::ConfigureBranding => configure(port, spec, Mode::Branding, cancel).await,
        Op::ConfigureScopes => configure(port, spec, Mode::Scopes, cancel).await,
    }
}

/// Fixed page script, evaluated only in the top-level document. It returns an enum
/// string, never page text or field values. It reads/clicks only the controls named
/// here: Get started, Next, Continue, Create, the App name / contact inputs, the
/// support-email combobox and option, External radio, the user-data-policy checkbox,
/// Add or remove scopes, the dialog textarea, Add to table, Update and Save.
const SCRIPT: &str = r##"function(mode, project, email, last, scopes, expected, appName, origin, host) {
  "use strict";
  const visible = e => !!e && !!e.getClientRects().length && getComputedStyle(e).visibility !== "hidden";
  const txt = e => ((e && (e.innerText || e.textContent)) || "").replace(/\s+/g, " ").trim();
  const low = s => (s || "").toLowerCase();
  // The Console account toolbar uses an open shadow root. Never cross a frame
  // or read hidden page state; inspect only rendered controls in this document.
  const all = (sel, root = document) => {
    const roots = [root];
    for (let i = 0; i < roots.length; i++)
      for (const e of roots[i].querySelectorAll("*")) if (e.shadowRoot) roots.push(e.shadowRoot);
    return roots.flatMap(r => [...r.querySelectorAll(sel)]).filter(visible);
  };
  const need = list => { if (list.length > 1) throw "ambiguous"; return list[0] || null; };
  const enabled = e => !!e && !e.disabled && e.getAttribute("aria-disabled") !== "true";
  const checked = e => e.checked === true || e.getAttribute("aria-checked") === "true";
  const has = (text, value) => low(text).split(/[\s,;()<>]+/).includes(low(value));
  const button = (name, root = document) => need(all("button, [role=button]", root)
    .filter(b => low(txt(b)) === name || low(b.getAttribute("aria-label")) === name));
  const nameOf = i => low(i.getAttribute("aria-label")
    || (i.getAttribute("aria-labelledby") || "").split(/\s+/).map(id => txt(document.getElementById(id))).filter(Boolean).join(" ") || (i.labels && i.labels[0] && i.labels[0].innerText)
    || i.getAttribute("placeholder") || "").replace(/\s*\*$/, "");
  const setValue = (field, value) => {
    const proto = field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value") && Object.getOwnPropertyDescriptor(proto, "value").set;
    if (!setter) throw "unsupported";
    setter.call(field, value);
    field.dispatchEvent(new Event("input", {bubbles: true}));
    field.dispatchEvent(new Event("change", {bubbles: true}));
  };
  let stage = null, run = null;
  const go = (s, r) => { stage = s; run = r; };
  const press = (s, b) => enabled(b) ? go(s, () => b.click()) : go("waiting", null);
  try {
    const params = new URLSearchParams(location.search);
    const path = location.pathname.replace(/\/+$/, "");
    const pathOk = (mode === "audience" || mode === "publication") ? path === "/auth/audience"
      : mode === "scopes" ? path === "/auth/scopes"
      : mode === "domains" ? path === "/auth/branding"
      : (path === "/auth/branding" || path.startsWith("/auth/overview"));
    if (location.origin !== "https://console.cloud.google.com" || !pathOk
        || params.getAll("project").length !== 1 || params.get("project") !== project) return "foreign";
    if (document.readyState === "loading") return "waiting";
    const accounts = new Set(all('[aria-label]').filter(e => /^(Google Account|Account:)/i.test(e.getAttribute('aria-label'))).flatMap(e =>
      (e.getAttribute("aria-label").match(/[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}/gi) || []).map(low)));
    if (!accounts.size) return "waiting";
    if (accounts.size !== 1 || !accounts.has(low(email))) return "wrong_account";
    if (mode === "branding") {
      const wizardPath = path.startsWith("/auth/overview");
      const nameInput = need(all("input").filter(i => nameOf(i).startsWith("app name")));
      if (path === "/auth/branding" && nameInput) {
        const support = need(all('[role="combobox"]').filter(c => nameOf(c) === "user support email"));
        const contact = need(all('input[aria-label="Text field for emails"]'));
        const group = contact && contact.closest("mat-form-field");
        if (!support || !group) return "unsupported";
        const emails = all("mat-chip-row", group).map(txt).map(low);
        return nameInput.value === appName && low(txt(support)) === low(email)
          && emails.length === 1 && emails[0] === low(email) ? "branding_match" : "branding_mismatch";
      }
      const radios = all("input[type=radio], [role=radio]");
      const emailInput = need(all("input").filter(i => nameOf(i).startsWith("email address")));
      const agree = need(all("input[type=checkbox], [role=checkbox]")
        .filter(c => low(txt(c.closest("label") || (c.parentElement && c.parentElement.parentElement))).includes("user data policy")));
      const create = button("create"), next = button("next"), cont = button("continue");
      const afterAgree = !!create && last === "agree_continue";
      if (nameInput || radios.length || emailInput || agree || afterAgree) {
        if (!wizardPath) return "unsupported";
        if (agree) {
          if (cont && create) return "ambiguous";
          if (!checked(agree)) go("agree_check", () => agree.click());
          else if (cont) press("agree_continue", cont);
          else if (create) press("create", create);
          else return "waiting";
        } else if (afterAgree) {
          press("create", create);
        } else if (emailInput) {
          const chips = all('[class*="chip"]').map(txt).filter(t => t.includes("@"));
          if (chips.some(t => !has(t, email))) return "ambiguous";
          if (!chips.length) go("contact_fill", () => {
            setValue(emailInput, email);
            for (const type of ["keydown", "keypress", "keyup"])
              emailInput.dispatchEvent(new KeyboardEvent(type, {key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true}));
          });
          else press("contact_next", next);
        } else if (radios.length) {
          const external = need(radios.filter(r => low(txt(r.closest("label") || r.parentElement)).startsWith("external")
            || low(r.getAttribute("aria-label")).startsWith("external")));
          if (!external) return "unsupported";
          if (!checked(external)) go("audience_pick", () => external.click());
          else press("audience_next", next);
        } else {
          if (nameInput.value !== appName) go("app_name", () => setValue(nameInput, appName));
          else {
            const options = all('[role="option"]');
            const support = need(all('[role="combobox"]').filter(c =>
              nameOf(c).includes("support email")));
            if (options.length) {
              const hit = options.filter(o => has(txt(o), email));
              if (hit.length !== 1) return "ambiguous";
              go("support_pick", () => hit[0].click());
            } else if (!support) return "unsupported";
            else if (!has(txt(support), email)) go("support_open", () => support.click());
            else press("app_next", next);
          }
        }
      } else {
        const lines = (document.querySelector("main") || document.body).innerText.split("\n").map(s => s.trim()).filter(Boolean);
        const field = name => {
          const at = lines.map((l, i) => low(l) === name ? i : -1).filter(i => i >= 0);
          if (at.length > 1) throw "ambiguous";
          return at.length ? (lines[at[0] + 1] || "") : null;
        };
        const app = field("app name");
        if (app !== null) {
          const support = field("user support email"), contact = field("developer contact information");
          return app === appName && support !== null && low(support) === low(email)
            && contact !== null && low(contact) === low(email) ? "branding_match" : "branding_mismatch";
        }
        const start = button("get started");
        if (!start) return "unsupported";
        press("get_started", start);
      }
    } else if (mode === "audience") {
      const lines = (document.querySelector("main") || document.body).innerText.split("\n").map(s => s.trim()).filter(Boolean);
      const field = name => { const at = lines.map((v,i) => v === name ? i : -1).filter(i => i >= 0);
        if (at.length !== 1) throw "unsupported"; return lines[at[0]+1]; };
      const type = field("User type"), status = field("Publishing status");
      if (type !== "External" || (status !== "Testing" && status !== "In production")) return "audience_mismatch";
      // A published app needs no test user; never try to add one.
      if (status === "In production") return "audience_match";
      const dialog = need(all('[role="dialog"]').filter(e => txt(e)));
      if (dialog) {
        const input = need(all('input[aria-label="Text field for emails"]', dialog));
        if (!input) return "unsupported";
        const emails = all("mat-chip-row", dialog).map(txt).map(low);
        if (emails.some(e => e !== low(email)) || emails.length > 1) return "ambiguous";
        if (!emails.length && last === "audience_partial") go("audience_input", () => {
          setValue(input, email);
          for (const type of ["keydown", "keypress", "keyup"])
            input.dispatchEvent(new KeyboardEvent(type, {key:"Enter",code:"Enter",keyCode:13,which:13,bubbles:true}));
        });
        else if (emails.length === 1 && last === "audience_input") press("audience_save", button("save", dialog));
        else go("waiting", null);
      } else {
        const tables = all("table,[role=table]").filter(t => all("th,[role=columnheader]",t).some(c => txt(c)==="User information"));
        if (tables.length !== 1) return "unsupported";
        const users = all("td,[role=cell]",tables[0]).map(txt).filter(v=>v.includes("@"));
        if (users.some(u => low(u) === low(email))) return "audience_match";
        press("audience_partial", button("add users"));
      }
    } else if (mode === "domains") {
      if (!origin || !host) return "unsupported";
      const nameInput = need(all("input").filter(i => nameOf(i).startsWith("app name")));
      const support = need(all('[role="combobox"]').filter(c => nameOf(c) === "user support email"));
      const contact = need(all('input[aria-label="Text field for emails"]'));
      const group = contact && contact.closest("mat-form-field");
      // Still rendering: wait (bounded by the caller's polls, then NeedsHuman).
      if (!nameInput || !support || !group) return "waiting";
      const emails = all("mat-chip-row", group).map(txt).map(low);
      if (!(nameInput.value === appName && low(txt(support)) === low(email)
          && emails.length === 1 && emails[0] === low(email))) return "branding_mismatch";
      // Real HTML label association only (exactly one label), never placeholder or position.
      const labelText = i => i.labels && i.labels.length === 1
        ? low(txt(i.labels[0])).replace(/\s*\*$/, "").trim() : "";
      const urls = [["application home page", origin + "/about"],
        ["application privacy policy link", origin + "/privacy"],
        ["application terms of service link", origin + "/terms"]];
      const fields = urls.map(([name]) => need(all("input").filter(i => labelText(i) === name)));
      if (fields.some(f => !f)) return "waiting";
      const blank = [];
      for (let k = 0; k < fields.length; k++) {
        if (fields[k].value.trim() === "") blank.push(k);
        else if (fields[k].value !== urls[k][1]) return "domains_mismatch";
      }
      // An existing authorized domain must cover the hostname on a DNS label boundary. No
      // public suffix list exists here: never derive or add a "registrable" parent.
      const label = "[a-z0-9](?:[a-z0-9-]*[a-z0-9])?";
      const dns = new RegExp("^" + label + "(?:\\." + label + ")+$");
      const authorized = all("input").filter(i => /^authorized domain \d+$/.test(labelText(i)))
        .map(i => low(i.value).trim().replace(/\.$/, "")).filter(Boolean);
      if (!authorized.some(d => dns.test(d) && (host === d || host.endsWith("." + d)))) return "domain_unproven";
      if (blank.length) {
        if (last === "") go("domains_fill", () => { for (const k of blank) setValue(fields[k], urls[k][1]); });
        else go("waiting", null);
      } else if (last === "") return "domains_match";
      else if (last === "domains_fill") press("domains_save", button("save"));
      else go("waiting", null);
    } else if (mode === "publication") {
      const lines = (document.querySelector("main") || document.body).innerText.split("\n").map(s => s.trim()).filter(Boolean);
      const field = name => { const at = lines.map((v, i) => v === name ? i : -1).filter(i => i >= 0);
        if (!at.length) throw "waiting";
        if (at.length !== 1) throw "unsupported"; return lines[at[0] + 1]; };
      if (field("User type") !== "External") return "publication_mismatch";
      // The exact field only; banner or explanation text is never evidence.
      const status = field("Publishing status");
      if (status === "In production") return "publication_match";
      if (status !== "Testing") return "unsupported";
      const dialog = need(all('[role="dialog"]').filter(e => txt(e)));
      if (dialog) {
        // Only the conventional documented confirmation, opened by our own Publish app click.
        const buttons = all("button, [role=button]", dialog);
        const names = buttons.map(b => low(txt(b)) || low(b.getAttribute("aria-label")));
        const confirm = need(buttons.filter(b => low(txt(b)) === "confirm"));
        if (last !== "publish_open" || !low(txt(dialog)).includes("push to production?") || !confirm
            || names.some(n => n !== "confirm" && n !== "cancel")
            || all("input, textarea, select, a[href], [role=checkbox], [role=radio], [role=combobox]", dialog).length)
          return "unsupported";
        press("publish_confirm", confirm);
      } else if (last !== "") go("waiting", null);
      else {
        const publish = button("publish app");
        if (!publish) return "unsupported";
        if (!enabled(publish)) return "publish_blocked";
        go("publish_open", () => publish.click());
      }
    } else {
      const allowed = new Set(scopes);
      const dialog = need(all('[role="dialog"]').filter(e => txt(e)));
      if (dialog) {
        const area = need(all("textarea", dialog));
        if (!area) return "unsupported";
        const joined = scopes.join("\n");
        if (last === "scopes_partial") {
          if (area.value === "") go("scopes_input", () => setValue(area, joined)); else return "ambiguous";
        } else if (last === "scopes_input") {
          if (area.value !== joined) return "ambiguous";
          press("scopes_add_table", button("add to table", dialog));
        } else if (last === "scopes_add_table") press("scopes_update", button("update", dialog));
        else if (last === "scopes_update") go("waiting", null);
        else return "ambiguous";
      } else if (last === "scopes_update") press("scopes_save", button("save"));
      else if (last !== "") go("waiting", null);
      else {
        const open = button("add or remove scopes");
        if (!open) return "waiting";
        const found = new Set();
        const tables = all("table,[role=table]").filter(t => all("th,[role=columnheader]",t).some(c => txt(c)==="Scope"));
        if (!tables.length) return "unsupported";
        for (const table of tables) {
          const heads = all("th,[role=columnheader]",table).map(txt);
          if (heads.filter(h=>h==="Scope").length !== 1) return "ambiguous";
          const index = heads.indexOf("Scope");
          for (const row of all("tr,[role=row]",table)) {
            const cells = all("td,[role=cell],[role=gridcell]",row);
            if (!cells.length) continue;
            // Empty tables use a single presentation cell, not a scope record.
            if (cells.length===1 && /^(No rows|No scopes|No data)/i.test(txt(cells[0]))) continue;
            if (cells.length !== heads.length) return "unsupported";
            const value = txt(cells[index]);
            const normalized = value.startsWith(".../auth/") ? "https://www.googleapis.com/auth/"+value.slice(9) : value;
            if (!allowed.has(normalized)) return "scopes_extra";
            if (found.has(normalized)) return "ambiguous";
            found.add(normalized);
          }
        }
        if ([...found].some(s => !allowed.has(s))) return "scopes_extra";
        if (found.size === allowed.size) return "scopes_match";
        go("scopes_partial", () => open.click());
      }
    }
    if (!stage) return "unsupported";
    if (expected === "") return stage;
    if (stage === expected && run) { run(); return "acted"; }
    return "changed";
  } catch (e) {
    return typeof e === "string" ? e : "unsupported";
  }
}"##;
