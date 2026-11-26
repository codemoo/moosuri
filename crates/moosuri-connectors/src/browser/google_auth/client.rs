//! Bounded Google Auth Platform OAuth *client* reconcile and creation, driven in the
//! SAME authenticated Chrome as the SDK bootstrap (`Chrome::client`).
//!
//! Nothing here accepts a URL, script, selector, name, redirect or secret from a
//! command. Every value derives from the validated `bootstrap::Request` (setup id,
//! exact callback), the stable project id and the bound verified Google email.
//! The client's stable name is `Moosuri <full simple setup UUID>`.
//!
//! # Operations (`bootstrap::Response`)
//! * `InspectClient`: read-only. Opens `/auth/clients`, reconciles the *visible*
//!   list by exact stable name, then opens the one same-origin
//!   `/auth/clients/<segment>` link and validates the persisted client: name,
//!   application type Web, agentic flag on, exactly one redirect URI equal to the
//!   request callback and no other URI (so no JavaScript origin). Zero matches =>
//!   `Absent`; one valid match => `ClientPresent` (public identity, NEVER a
//!   secret); duplicates / a look-alike row => `NeedsHuman` / `Blocked`.
//! * `CreateClient`: re-reconciles first (anything present => `Blocked`), then fills
//!   the create form (strictly ordered steps, each at most once), clicks Create
//!   once, captures the secret only from the creation result dialog's visible
//!   `Client secret` field through the fixed expression, then re-reads the persisted
//!   client from the list/detail pages. Only if that exact client matches the
//!   captured id is a `ClientRegistration` returned; otherwise the secret is
//!   dropped (zeroized) and an error is returned. An uncertain click/response is an
//!   error that taints Chrome and spends the worker's single create (see
//!   `Sequence`): it is never replayed. If the secret is not visible the result is
//!   `NeedsHuman`; automatic rotation / secret recovery is NOT implemented.
//!
//! # Observed vs NOT PROVEN (live DOM verification gaps)
//! Observed live: console origin, exact `project` query, the open shadow-root account
//! toolbar (`Account: Name (email)`), the create form (application type
//! `role=combobox` -> "Web application", Name input, two "Add URI" buttons, the "Use
//! this client for an AI-powered agent" checkbox, Create/Cancel), the single-row list
//! (`TABLE role=grid`; header row = checkbox TD + TH Name/Creation date/Type/Client
//! ID/Actions; body rows = checkbox TD + `rowheader` name link + four cells; a second
//! empty-text icon link to the same detail path) and the detail page (title "Client
//! ID for Web application", labelled Name input, "URIs 1" input, "Additional
//! information" / `Client ID` label then the visible id). Still NOT proven and
//! fail-closed: the zero-row empty-state text, pagination/filter markup, multi-row
//! lists, the `/auth/clients/create` path and the creation-result dialog fields.
use super::{
    Error,
    bootstrap::{
        Action, ClientIdentity, ClientRegistration, Reply, Request, Response, valid_client_id,
        valid_client_secret,
    },
    sdk::{Email, ProjectId},
};
use crate::gmail::SecretString;
use serde::Serialize;
use std::{fmt, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

#[cfg(test)]
mod tests;

const PREFIX: &str = "https://console.cloud.google.com/auth/clients";
const POLLS: usize = 30;
const DRIVE_POLLS: usize = 120;
const DRIVE_IDLE: usize = 40;
const RESULT_POLLS: usize = 60;
const RELOADS: usize = 3;
const PAUSE: Duration = Duration::from_millis(700);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Inspect,
    Create,
}

impl Op {
    pub fn from_action(action: Action) -> Option<Self> {
        match action {
            Action::InspectClient => Some(Self::Inspect),
            Action::CreateClient => Some(Self::Create),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    List,
    Detail,
    Create,
    Result,
}

/// Fixed per-request values, built only from parsed, validated types.
#[derive(Clone)]
pub(super) struct Spec {
    project: String,
    email: String,
    setup_id: Uuid,
    redirect: String,
    name: String,
}

impl Spec {
    pub fn new(request: &Request, project: &ProjectId, email: &Email) -> Self {
        Self {
            project: project.as_str().to_owned(),
            email: email.as_str().to_owned(),
            setup_id: request.setup_id,
            redirect: request.redirect_uri.clone(),
            name: request.client_name(),
        }
    }
    fn url(&self, mode: Mode, segment: &str) -> String {
        let page = match mode {
            Mode::List | Mode::Result => String::new(),
            Mode::Create => "/create".to_owned(),
            Mode::Detail => format!("/{segment}"),
        };
        format!("{PREFIX}{page}?project={}&hl=en", self.project)
    }
    fn identity(&self, client_id: String) -> ClientIdentity {
        ClientIdentity {
            setup_id: self.setup_id,
            client_id,
            redirect_uri: self.redirect.clone(),
        }
    }
}

/// One evaluation of the fixed page script.
#[derive(Clone, Copy, Debug)]
pub(super) struct Call<'a> {
    pub mode: Mode,
    /// Last step acted on ("" = none).
    pub last: &'static str,
    /// Empty = read-only peek; otherwise act only if the page still shows it.
    pub expected: &'static str,
    /// Validated client path segment (`Mode::Detail` only).
    pub segment: &'a str,
}

/// Minimal browser surface; Chrome implements it, tests use a scripted fake. The
/// evaluation result is zeroizing because the creation result carries the secret.
#[allow(async_fn_in_trait)]
pub(super) trait Port {
    async fn navigate(&mut self, url: &str) -> Result<(), Error>;
    async fn eval(&mut self, spec: &Spec, call: &Call<'_>) -> Result<Zeroizing<String>, Error>;
    async fn pause(&mut self, duration: Duration);
}

/// Result of one client operation. `Created` holds the one-time secret.
#[derive(Debug)]
pub(super) enum Outcome {
    Absent,
    Present(ClientIdentity),
    Created(ClientRegistration),
}

impl Outcome {
    pub fn into_response(self) -> Response {
        match self {
            Self::Absent => Response::Status(Reply::Absent),
            Self::Present(identity) => Response::ClientPresent(identity),
            Self::Created(registration) => Response::Client(registration),
        }
    }
}

/// Create-form steps in strict progression order (derive order is the rank).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    AppTypeOpen,
    AppTypePick,
    NameFill,
    RedirectAdd,
    RedirectFill,
    AgentCheck,
    Create,
}

const STEPS: [(Step, &str); 7] = [
    (Step::AppTypeOpen, "app_type_open"),
    (Step::AppTypePick, "app_type_pick"),
    (Step::NameFill, "name_fill"),
    (Step::RedirectAdd, "redirect_add"),
    (Step::RedirectFill, "redirect_fill"),
    (Step::AgentCheck, "agent_check"),
    (Step::Create, "create"),
];

impl Step {
    fn wire(self) -> &'static str {
        STEPS
            .iter()
            .find(|(s, _)| *s == self)
            .map_or("", |(_, w)| w)
    }
}

/// Captured creation result. Secret zeroizes; Debug never shows it.
struct Capture {
    id: String,
    secret: Zeroizing<String>,
}
impl fmt::Debug for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Capture")
            .field("id", &self.id)
            .field("secret", &"<redacted>")
            .finish()
    }
}

#[derive(Debug)]
enum Seen {
    Waiting,
    Step(Step),
    ListNone,
    ListOne(String),
    Detail(String),
    Created(Capture),
    /// Foreign page, wrong account, look-alike/mismatching client: never touched.
    Blocked,
    /// Duplicates, hidden secret or unrecognised DOM: a human must look.
    Human,
    Acted,
    Changed,
}

fn segment_ok(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 256
        && !matches!(segment, "create" | "." | "..")
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

impl Seen {
    fn parse(raw: &str) -> Result<Self, Error> {
        if let Some(rest) = raw.strip_prefix("created\n") {
            let (id, secret) = rest.split_once('\n').ok_or(Error::Unavailable)?;
            if !valid_client_id(id) || !valid_client_secret(secret) {
                return Err(Error::Unavailable);
            }
            return Ok(Self::Created(Capture {
                id: id.to_owned(),
                secret: Zeroizing::new(secret.to_owned()),
            }));
        }
        if let Some(segment) = raw.strip_prefix("link:") {
            return segment_ok(segment)
                .then(|| Self::ListOne(segment.to_owned()))
                .ok_or(Error::Unavailable);
        }
        if let Some(id) = raw.strip_prefix("match:") {
            return valid_client_id(id)
                .then(|| Self::Detail(id.to_owned()))
                .ok_or(Error::Unavailable);
        }
        Ok(match raw {
            "waiting" => Self::Waiting,
            "list_none" => Self::ListNone,
            "foreign" | "wrong_account" | "client_mismatch" => Self::Blocked,
            "client_duplicate" | "ambiguous" | "unsupported" | "secret_hidden" => Self::Human,
            "acted" => Self::Acted,
            "changed" => Self::Changed,
            other => STEPS
                .iter()
                .find(|(_, w)| *w == other)
                .map(|(s, _)| Self::Step(*s))
                .ok_or(Error::Unavailable)?,
        })
    }
    fn refusal(&self) -> Error {
        match self {
            Self::Blocked => Error::Blocked,
            Self::Human | Self::Waiting => Error::NeedsHuman,
            _ => Error::Unavailable,
        }
    }
}

pub(super) fn expression(spec: &Spec, call: &Call<'_>) -> Result<String, Error> {
    let args = serde_json::to_string(&(
        call.mode,
        spec.project.as_str(),
        spec.email.as_str(),
        call.last,
        call.expected,
        spec.name.as_str(),
        spec.redirect.as_str(),
        call.segment,
    ))
    .map_err(|_| Error::Invalid)?;
    Ok(format!("({SCRIPT})(...{args})"))
}

async fn observe<P: Port>(
    port: &mut P,
    spec: &Spec,
    call: Call<'_>,
    cancel: &CancellationToken,
) -> Result<Seen, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Unavailable);
    }
    let raw = port.eval(spec, &call).await?;
    Seen::parse(&raw)
}

/// Polls a read-only peek until it is not `waiting`.
async fn first_seen<P: Port>(
    port: &mut P,
    spec: &Spec,
    mode: Mode,
    segment: &str,
    cancel: &CancellationToken,
) -> Result<Seen, Error> {
    for _ in 0..POLLS {
        let call = Call {
            mode,
            last: "",
            expected: "",
            segment,
        };
        match observe(port, spec, call, cancel).await? {
            Seen::Waiting => port.pause(PAUSE).await,
            seen => return Ok(seen),
        }
    }
    Err(Error::NeedsHuman)
}

enum Listing {
    None,
    One(String),
}

/// Fresh navigation to the clients list and a stable (two-read) classification.
async fn reconcile<P: Port>(
    port: &mut P,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<Listing, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Unavailable);
    }
    port.navigate(&spec.url(Mode::List, "")).await?;
    let first = first_seen(port, spec, Mode::List, "", cancel).await?;
    if !matches!(first, Seen::ListNone | Seen::ListOne(_)) {
        return Err(first.refusal());
    }
    // Neither absence nor the link may be a loading skeleton.
    port.pause(PAUSE).await;
    let second = first_seen(port, spec, Mode::List, "", cancel).await?;
    match (first, second) {
        (Seen::ListNone, Seen::ListNone) => Ok(Listing::None),
        (Seen::ListOne(a), Seen::ListOne(b)) if a == b => Ok(Listing::One(a)),
        (_, Seen::Blocked) => Err(Error::Blocked),
        _ => Err(Error::NeedsHuman),
    }
}

/// Opens the one client page and validates the persisted settings. Read-only.
async fn read_client<P: Port>(
    port: &mut P,
    spec: &Spec,
    segment: &str,
    cancel: &CancellationToken,
) -> Result<ClientIdentity, Error> {
    if !segment_ok(segment) {
        return Err(Error::Blocked);
    }
    port.navigate(&spec.url(Mode::Detail, segment)).await?;
    let id = match first_seen(port, spec, Mode::Detail, segment, cancel).await? {
        Seen::Detail(id) => id,
        other => return Err(other.refusal()),
    };
    port.pause(PAUSE).await;
    match first_seen(port, spec, Mode::Detail, segment, cancel).await? {
        Seen::Detail(again) if again == id => Ok(spec.identity(id)),
        Seen::Blocked => Err(Error::Blocked),
        _ => Err(Error::NeedsHuman),
    }
}

async fn find<P: Port>(
    port: &mut P,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<Option<ClientIdentity>, Error> {
    match reconcile(port, spec, cancel).await? {
        Listing::None => Ok(None),
        Listing::One(segment) => read_client(port, spec, &segment, cancel).await.map(Some),
    }
}

async fn inspect<P: Port>(
    port: &mut P,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<Outcome, Error> {
    Ok(match find(port, spec, cancel).await? {
        None => Outcome::Absent,
        Some(identity) => Outcome::Present(identity),
    })
}

/// Walks the create form. Each step is acted on at most once and only in strictly
/// increasing order; a page that does not advance ends in `NeedsHuman`. Returns
/// after the single `create` click was reported as performed.
async fn drive<P: Port>(
    port: &mut P,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    let mut last: Option<Step> = None;
    let mut idle = 0usize;
    for _ in 0..DRIVE_POLLS {
        let wire = last.map_or("", Step::wire);
        let peek = Call {
            mode: Mode::Create,
            last: wire,
            expected: "",
            segment: "",
        };
        match observe(port, spec, peek, cancel).await? {
            Seen::Waiting => idle += 1,
            Seen::Step(step) => {
                if !last.is_none_or(|done| step > done) {
                    idle += 1;
                } else {
                    // A freshly opened form must not already be ready to submit.
                    if last.is_none() && step == Step::Create {
                        return Err(Error::NeedsHuman);
                    }
                    let act = Call {
                        mode: Mode::Create,
                        last: wire,
                        expected: step.wire(),
                        segment: "",
                    };
                    match observe(port, spec, act, cancel).await? {
                        Seen::Acted => {
                            last = Some(step);
                            idle = 0;
                            if step == Step::Create {
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

/// Reads the visible creation result. Never clicks.
async fn capture<P: Port>(
    port: &mut P,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<Capture, Error> {
    for _ in 0..RESULT_POLLS {
        let call = Call {
            mode: Mode::Result,
            last: "create",
            expected: "",
            segment: "",
        };
        match observe(port, spec, call, cancel).await? {
            Seen::Waiting => port.pause(PAUSE).await,
            Seen::Created(capture) => return Ok(capture),
            other => return Err(other.refusal()),
        }
    }
    Err(Error::NeedsHuman)
}

async fn create<P: Port>(
    port: &mut P,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<Outcome, Error> {
    // Re-prove absence immediately before the single create.
    if find(port, spec, cancel).await?.is_some() {
        return Err(Error::Blocked);
    }
    port.navigate(&spec.url(Mode::Create, "")).await?;
    drive(port, spec, cancel).await?;
    let captured = capture(port, spec, cancel).await?;
    // The secret is released only if the persisted client reads back exactly.
    for _ in 0..RELOADS {
        match find(port, spec, cancel).await? {
            Some(identity) if identity.client_id == captured.id => {
                let secret = SecretString::new(captured.secret.as_str().to_owned())
                    .map_err(|_| Error::Unavailable)?;
                return Ok(Outcome::Created(ClientRegistration::new(identity, secret)));
            }
            Some(_) => return Err(Error::Blocked),
            None => port.pause(PAUSE * 3).await,
        }
    }
    Err(Error::NeedsHuman)
}

/// Entry point. Chrome taints itself on any error (see `Chrome::client`).
pub(super) async fn run<P: Port>(
    port: &mut P,
    op: Op,
    spec: &Spec,
    cancel: &CancellationToken,
) -> Result<Outcome, Error> {
    match op {
        Op::Inspect => inspect(port, spec, cancel).await,
        Op::Create => create(port, spec, cancel).await,
    }
}

/// Fixed page script, evaluated only in the top-level document. It returns an enum
/// string, a client path segment (`link:`), a client id (`match:`) or, only for the
/// creation result dialog's visible fields, `created\n<id>\n<secret>`. It never
/// returns other page text. It clicks only: the application type combobox and its
/// "Web application" option, the Name and redirect URI inputs (value set), the
/// redirect section's "Add URI" button, the AI-agent checkbox and Create.
const SCRIPT: &str = r##"function(mode, project, email, last, expected, name, redirect, segment) {
  "use strict";
  const visible = e => !!e && !!e.getClientRects().length && getComputedStyle(e).visibility !== "hidden";
  const rendered = e => { const s = getComputedStyle(e); return s.display !== "none" && s.visibility !== "hidden"; };
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
  const button = (label, root = document) => need(all("button, [role=button]", root)
    .filter(b => low(txt(b)) === label || low(b.getAttribute("aria-label")) === label));
  const nameOf = i => low(i.getAttribute("aria-label")
    || (i.getAttribute("aria-labelledby") || "").split(/\s+/).map(id => txt(document.getElementById(id))).filter(Boolean).join(" ") || (i.labels && i.labels[0] && i.labels[0].innerText)
    || i.getAttribute("placeholder") || "").replace(/\s+/g, " ").trim().replace(/\s*\*$/, "");
  const setValue = (field, value) => {
    const proto = field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value") && Object.getOwnPropertyDescriptor(proto, "value").set;
    if (!setter) throw "unsupported";
    setter.call(field, value);
    field.dispatchEvent(new Event("input", {bubbles: true}));
    field.dispatchEvent(new Event("change", {bubbles: true}));
  };
  // Rendered text and (non-password, non-choice) input values in document order.
  const linesOf = root => {
    const out = [];
    const walk = n => {
      for (const c of n.childNodes) {
        if (c.nodeType === 3) {
          const t = c.textContent.replace(/\s+/g, " ").trim();
          if (t) out.push(t);
        } else if (c.nodeType === 1) {
          if (!rendered(c) || c.matches("script,style,noscript")) continue;
          if (c.matches("input,textarea")) {
            if (!/^(checkbox|radio|button|submit|hidden|password)$/i.test(c.type || "") && c.value) out.push(c.value.trim());
            continue;
          }
          walk(c);
          if (c.shadowRoot) walk(c.shadowRoot);
        }
      }
    };
    walk(root);
    return out;
  };
  let stage = null, run = null;
  const go = (s, r) => { stage = s; run = r; };
  const press = (s, b) => enabled(b) ? go(s, () => b.click()) : go("waiting", null);
  try {
    const params = new URLSearchParams(location.search);
    const path = location.pathname.replace(/\/+$/, "");
    const pathOk = mode === "list" ? path === "/auth/clients"
      : mode === "create" ? path === "/auth/clients/create"
      : mode === "detail" ? (segment !== "" && segment !== "create" && path === "/auth/clients/" + segment)
      : (path === "/auth/clients" || path.startsWith("/auth/clients/"));
    if (location.origin !== "https://console.cloud.google.com" || !pathOk
        || params.getAll("project").length !== 1 || params.get("project") !== project) return "foreign";
    if (document.readyState === "loading") return "waiting";
    const accounts = new Set(all('[aria-label]').filter(e => /^(Google Account|Account:)/i.test(e.getAttribute('aria-label'))).flatMap(e =>
      (e.getAttribute("aria-label").match(/[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}/gi) || []).map(low)));
    if (!accounts.size) return "waiting";
    if (accounts.size !== 1 || !accounts.has(low(email))) return "wrong_account";
    const root = document.querySelector("main") || document.body;
    const uri = l => /^[a-z][a-z0-9+.-]*:\/\//i.test(l);

    if (mode === "list") {
      const tables = all("table,[role=table]", root).filter(t => all("th,[role=columnheader]", t).some(c => low(txt(c)) === "name"));
      if (tables.length > 1) return "ambiguous";
      // Console keeps an aria-hidden, zero-height progress-button placeholder
      // after loading completes. Only an exposed indicator fences list proof.
      const busy = all("[role=progressbar],mat-spinner,mat-progress-bar,mat-progress-spinner")
        .some(e => !e.closest('[aria-hidden="true"]'));
      if (busy) return "waiting";
      // Absence or uniqueness on only one page is not evidence for the entire
      // project. A filtered or paginated list must be reconciled by an operator
      // until an observed, complete traversal is supported.
      if (all("input", root).some(i => /filter|search/i.test(nameOf(i)) && i.value.trim())) return "unsupported";
      if (all("mat-chip-row", root).length) return "unsupported";
      const paging = all("button,[role=button]", root).filter(b => /^(next|previous|last|first) page$/i.test(b.getAttribute("aria-label") || txt(b)));
      if (paging.some(enabled)) return "unsupported";
      const ranges = all(".mat-mdc-paginator-range-label,.mat-paginator-range-label", root);
      for (const range of ranges) {
        const m = txt(range).match(/^(\d+)\s*[–-]\s*(\d+)\s+of\s+(\d+)$/);
        if (m && (+m[1] !== 1 || +m[2] !== +m[3])) return "unsupported";
      }
      const clientLink = a => {
        try {
          const u = new URL(a.href, location.href);
          return u.origin === location.origin && /^\/auth\/clients\/[^\/]+$/.test(u.pathname) && u.pathname !== "/auth/clients/create";
        } catch (e) { return false; }
      };
      const empty = all("td,[role=cell],p,[role=status]", root).some(e =>
        /^(No OAuth clients to display|No OAuth clients|No clients to display|No rows to display)\.?$/i.test(txt(e)));
      const create = all("a,button,[role=button]", root).filter(b => low(txt(b)) === "create client");
      if (!tables.length) {
        if (all("a[href]", root).some(clientLink) || !create.length || !empty) return "waiting";
        return "list_none";
      }
      // Observed: the header row starts with an empty select-all checkbox TD before
      // the TH columnheaders, and each body row starts with a checkbox TD. A header
      // row is any row holding a columnheader; only the other rows are data rows.
      const headRows = all("tr,[role=row]", tables[0]).filter(r => all("th,[role=columnheader]", r).length);
      const rows = all("tr,[role=row]", tables[0]).filter(r => !headRows.includes(r)
        && all("td,[role=cell],[role=gridcell],[role=rowheader]", r).length);
      if (!rows.length || (rows.length === 1 && empty && !all("a[href]", rows[0]).length))
        return empty && create.length ? "list_none" : "waiting";
      if (headRows.length !== 1) return "unsupported";
      // Positional: leading checkbox cells keep their slot, so Name is not shifted.
      const heads = all("th,td,[role=columnheader],[role=gridcell],[role=cell],[role=rowheader]", headRows[0]).map(txt);
      if (heads.filter(h => h === "Name").length !== 1) return "unsupported";
      const nameIndex = heads.indexOf("Name");
      for (const row of rows) {
        const cells = all("td,[role=cell],[role=gridcell],[role=rowheader]", row);
        if (cells.length !== heads.length || !txt(cells[nameIndex])) return "waiting";
      }
      const uuid = low(name.slice("Moosuri ".length));
      const exact = [];
      let mentions = 0;
      for (const row of rows) {
        const cells = all("td,[role=cell],[role=gridcell],[role=rowheader]", row);
        const hits = all("a[href]", cells[nameIndex]).filter(a => txt(a) === name);
        if (hits.length > 1) return "client_duplicate";
        if (hits.length === 1) exact.push({link: hits[0], row});
        const text = low(txt(row));
        if (text.includes(uuid) || text.includes(low(name))) mentions++;
      }
      if (exact.length > 1 || mentions > 1) return "client_duplicate";
      if (exact.length === 0) return mentions ? "client_mismatch" : "list_none";
      if (mentions !== 1 || !clientLink(exact[0].link)) return "client_mismatch";
      const u = new URL(exact[0].link.href, location.href);
      // The row's icon link (empty text) is the same client; any other client link
      // or labelled link in the row is not.
      for (const a of all("a[href]", exact[0].row).filter(clientLink)) {
        if (new URL(a.href, location.href).pathname !== u.pathname) return "client_mismatch";
        if (txt(a) !== "" && txt(a) !== name) return "client_mismatch";
      }
      return "link:" + decodeURIComponent(u.pathname.slice("/auth/clients/".length));
    }

    if (mode === "detail") {
      const lines = linesOf(root);
      const at = label => lines.map((l, i) => l === label ? i : -1).filter(i => i >= 0);
      const field = label => {
        const hit = at(label);
        if (!hit.length) throw "waiting";
        if (hit.length > 1) throw "ambiguous";
        return lines[hit[0] + 1] || "";
      };
      // Observed: title heading "Client ID for Web application" (no separate
      // "Application type" label) and an editable Name input with a real label.
      // The older plain "Application type" line stays supported only when no
      // such title exists. Helper text means "next line" is not trusted for Name.
      const titles = all("h1,h2,h3,h4,h5,h6,[role=heading]", root).map(txt).filter(t => /^client id for /i.test(t));
      if (titles.length > 1) return "ambiguous";
      const type = titles.length ? titles[0].replace(/^client id for /i, "") : field("Application type");
      const nameInputs = all("input", root).filter(i => nameOf(i) === "name");
      if (nameInputs.length > 1) return "ambiguous";
      const shown = nameInputs.length ? nameInputs[0].value : field("Name");
      const id = field("Client ID");
      if (shown !== name || type !== "Web application") return "client_mismatch";
      if (/\s/.test(id) || id.length > 256 || !/\.apps\.googleusercontent\.com$/i.test(id)) return "unsupported";
      if (/\.apps\.googleusercontent\.com$/i.test(segment) && low(decodeURIComponent(segment)) !== low(id)) return "client_mismatch";
      // Redirect inputs are labelled "URIs N". Exactly one may exist (origins have
      // none), so a blank extra row or an origin input cannot hide.
      const uriInputs = all("input", root).filter(i => /^uris? \d+$/.test(nameOf(i)));
      if (uriInputs.length > 1 || (uriInputs.length === 1 && uriInputs[0].value !== redirect)) return "client_mismatch";
      const boxes = all("input[type=checkbox],[role=checkbox],[role=switch]", root)
        .filter(c => /ai-powered agent/i.test(nameOf(c) + " " + txt(c.closest("label") || c.parentElement)));
      let agent;
      if (boxes.length > 1) return "ambiguous";
      if (boxes.length === 1) agent = checked(boxes[0]);
      else {
        const hit = lines.map((l, i) => /ai-powered agent/i.test(l) ? i : -1).filter(i => i >= 0);
        if (hit.length !== 1) return "unsupported";
        const v = low(lines[hit[0] + 1] || "");
        if (["yes", "enabled", "on", "true"].includes(v)) agent = true;
        else if (["no", "disabled", "off", "false"].includes(v)) agent = false;
        else return "unsupported";
      }
      if (!agent) return "client_mismatch";
      const start = at("Authorized redirect URIs");
      if (start.length !== 1) return "unsupported";
      const stop = /^(name|client id|client secrets?|application type|creation date|created|last used|status|additional information|authorized javascript origins)$/i;
      let end = lines.length;
      for (let i = start[0] + 1; i < lines.length; i++) if (stop.test(lines[i])) { end = i; break; }
      const inside = lines.slice(start[0] + 1, end).filter(uri);
      // Any URI outside the redirect section (e.g. a JavaScript origin) is a mismatch.
      const outside = lines.slice(0, start[0] + 1).concat(lines.slice(end)).filter(uri);
      if (outside.length || inside.length !== 1 || inside[0] !== redirect) return "client_mismatch";
      return "match:" + id;
    }

    if (mode === "result") {
      const dialog = need(all('[role="dialog"]').filter(e => {
        const lines = linesOf(e);
        return lines.includes("Client ID") && lines.includes("Client secret");
      }));
      if (!dialog) return "waiting";
      const lines = linesOf(dialog);
      const pick = label => {
        const hit = lines.map((l, i) => l === label ? i : -1).filter(i => i >= 0);
        if (hit.length > 1) throw "ambiguous";
        return hit.length ? (lines[hit[0] + 1] || "") : null;
      };
      const id = pick("Client ID"), secret = pick("Client secret");
      if (id === null || secret === null || !id || !secret) return "waiting";
      if (/[*•●·]/.test(secret)) return "secret_hidden";
      return "created\n" + id + "\n" + secret;
    }

    // mode === "create"
    const heading = e => [...e.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent).join(" ").replace(/\s+/g, " ").trim();
    let section = null;
    const inputs = {origins: [], redirect: []}, adds = {origins: [], redirect: []};
    for (const e of root.querySelectorAll("*")) {
      if (!visible(e)) continue;
      const h = heading(e);
      if (h.length < 60) {
        if (/^authorized javascript origins/i.test(h)) section = "origins";
        else if (/^authorized redirect uris/i.test(h)) section = "redirect";
      }
      if (!section) continue;
      if (e.matches("input[type=text],input[type=url],input:not([type])") && e.getAttribute("role") !== "combobox") inputs[section].push(e);
      else if (e.matches("button,[role=button]") && low(txt(e)) === "add uri") adds[section].push(e);
    }
    const boxes = all('[role="combobox"]', root);
    const typeBox = need(boxes.filter(c => nameOf(c).includes("application type"))) || (boxes.length === 1 ? boxes[0] : null);
    if (!typeBox) return "waiting";
    if (low(txt(typeBox)) !== "web application") {
      const options = all('[role="option"]').filter(o => low(txt(o)) === "web application");
      if (options.length > 1) return "ambiguous";
      if (options.length === 1) go("app_type_pick", () => options[0].click());
      else go("app_type_open", () => typeBox.click());
    } else {
      const nameInput = need(all("input", root).filter(i => nameOf(i) === "name"));
      if (!nameInput) return "waiting";
      if (nameInput.value !== name) go("name_fill", () => setValue(nameInput, name));
      else if (inputs.origins.length) return "ambiguous";
      else if (!inputs.redirect.length) {
        const add = need(adds.redirect);
        if (!add) return "unsupported";
        press("redirect_add", add);
      } else if (inputs.redirect.length > 1) return "ambiguous";
      else if (inputs.redirect[0].value !== redirect) {
        if (inputs.redirect[0].value !== "") return "ambiguous";
        go("redirect_fill", () => setValue(inputs.redirect[0], redirect));
      } else {
        const agent = need(all("input[type=checkbox],[role=checkbox]", root)
          .filter(c => /ai-powered agent/i.test(nameOf(c) + " " + txt(c.closest("label") || (c.parentElement && c.parentElement.parentElement)))));
        if (!agent) return "unsupported";
        if (!checked(agent)) go("agent_check", () => agent.click());
        else {
          const create = button("create");
          if (!create) return "unsupported";
          press("create", create);
        }
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
