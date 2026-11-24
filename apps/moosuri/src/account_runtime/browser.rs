//! Authenticated member-side browser control. Persist the dispatch before contacting Chromium.
use super::{browser_model, browser_wire as wire};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use moosuri_connectors::browser::capsule::{WorkspaceFrame, WorkspaceInput, WorkspaceObservation};
use moosuri_core::personal_browser::{Control, Frame, Link, Status, Tab, Workspace};
use moosuri_store::{
    Store, StoreError,
    personal_browser::{
        Admission, Job, Output, Scope, Version,
        progress::{self, Progress},
    },
    personal_volume::VolumeOwner,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::Mutex as DispatchLock;
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct Service {
    pub(super) store: Store,
    pub(super) owner: VolumeOwner,
    pub(super) socket: Option<PathBuf>,
    pub(super) uid: u32,
    pub(super) model: Option<moosuri_server::personal_text::Options>,
    frames: Arc<tokio::sync::Semaphore>,
    pub(super) broker_slot: Arc<DispatchLock<()>>,
    dispatch: Arc<Mutex<HashMap<String, Weak<DispatchLock<()>>>>>,
}
pub(super) fn routes(
    store: Store,
    owner: VolumeOwner,
    socket: Option<PathBuf>,
    uid: u32,
    model: Option<moosuri_server::personal_text::Options>,
) -> Router {
    let state = Arc::new(Service {
        store,
        owner,
        socket,
        uid,
        model,
        frames: Arc::new(tokio::sync::Semaphore::new(1)),
        broker_slot: Default::default(),
        dispatch: Default::default(),
    });
    if state.socket.is_some() {
        let weak = Arc::downgrade(&state);
        tokio::spawn(async move {
            loop {
                {
                    let Some(state) = weak.upgrade() else {
                        break;
                    };
                    cleanup(&state).await;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    }
    Router::new()
        .route("/api/v1/threads/{id}/browser", get(status).post(start))
        .route("/api/v1/threads/{id}/browser/frame", get(frame))
        .route("/api/v1/threads/{id}/browser/{operation}", post(mutate))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(state)
}
async fn cleanup(s: &Service) {
    let Ok(ids) = s.store.personal_browser_retired(s.owner).await else {
        return;
    };
    for id in ids {
        let Ok(lock) = workspace_lock(s, &id) else {
            return;
        };
        let Ok(_guard) = lock.try_lock() else {
            continue;
        };
        let result = wire::call::<Value>(s, &id, "stop", json!({})).await;
        if matches!(&result, Err(Failure::Missing)) || result.is_ok_and(|r| r["stopped"] == true) {
            let _ = s.store.personal_browser_retired_ack(s.owner, id).await;
        }
    }
}
/// Serialize broker effects for a single workspace. A stop first fences the job in SQLite,
/// then waits only for its previous broker dispatch. An opening worker cannot outlive the stop.
async fn broker_action<T: serde::de::DeserializeOwned>(
    s: &Service,
    scope: &Scope,
    v: &Workspace,
    op: &str,
    args: Value,
) -> Result<T, Failure> {
    let lock = workspace_lock(s, &v.id)?;
    let _guard = lock.lock().await;
    let current = s
        .store
        .personal_browser_current(scope.clone())
        .await?
        .ok_or(Failure::Stale)?;
    if current.view.id != v.id || current.view.revision != v.revision {
        return Err(Failure::Stale);
    }
    wire::call(s, &v.id, op, args).await
}
pub(super) fn workspace_lock(s: &Service, id: &str) -> Result<Arc<DispatchLock<()>>, Failure> {
    let lock = {
        let mut locks = s.dispatch.lock().map_err(|_| Failure::Unavailable)?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        match locks.get(id).and_then(Weak::upgrade) {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(DispatchLock::new(()));
                locks.insert(id.to_owned(), Arc::downgrade(&lock));
                lock
            }
        }
    };
    Ok(lock)
}
pub(super) enum Failure {
    Invalid,
    Stale,
    Busy,
    Missing,
    Forbidden,
    Unavailable,
    Unknown,
    Store(StoreError),
}
impl From<StoreError> for Failure {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Store(StoreError::Unauthorized) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden | Self::Store(StoreError::Forbidden) => {
                (StatusCode::FORBIDDEN, "forbidden")
            }
            Self::Store(StoreError::NotFound) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Store(StoreError::BudgetExhausted) => {
                (StatusCode::CONFLICT, "browser_budget_exhausted")
            }
            Self::Stale | Self::Store(StoreError::Conflict) => {
                (StatusCode::CONFLICT, "browser_stale")
            }
            Self::Invalid | Self::Store(StoreError::InvalidInput) => {
                (StatusCode::BAD_REQUEST, "invalid_request")
            }
            Self::Busy => (StatusCode::TOO_MANY_REQUESTS, "browser_busy"),
            Self::Missing => (StatusCode::SERVICE_UNAVAILABLE, "browser_worker_missing"),
            Self::Unknown => (StatusCode::SERVICE_UNAVAILABLE, "browser_outcome_unknown"),
            _ => (StatusCode::SERVICE_UNAVAILABLE, "browser_unavailable"),
        };
        (
            status,
            [("cache-control", "no-store")],
            Json(json!({"code":code})),
        )
            .into_response()
    }
}
type Reply = Result<Response, Failure>;
fn response<T: Serialize>(v: T) -> Response {
    ([("cache-control", "no-store")], Json(v)).into_response()
}
fn status_view(s: &Service, v: Option<Workspace>) -> Response {
    response(Status {
        configured: wire::available(s),
        workspace: v,
    })
}
async fn scope(s: &Service, h: &HeaderMap, thread: String) -> Result<Scope, Failure> {
    let user = moosuri_server::personal_request_user(&s.store, h).await?;
    if user.account().id != s.owner.account.to_string() {
        return Err(Failure::Forbidden);
    }
    Ok(Scope {
        owner: s.owner,
        user,
        thread,
    })
}
fn epoch(h: &HeaderMap) -> Option<i64> {
    let mut values = h
        .get_all(moosuri_server::personal_text::SESSION_EPOCH_HEADER)
        .iter();
    values
        .next()?
        .to_str()
        .ok()?
        .parse()
        .ok()
        .filter(|v: &i64| *v >= 0 && values.next().is_none())
}
fn valid_png(encoded: &str, width: u32, height: u32) -> bool {
    use base64::Engine;
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    bytes.len() >= 33
        && bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        && bytes[16..20] == width.to_be_bytes()
        && bytes[20..24] == height.to_be_bytes()
}
fn display_url(raw: &str) -> String {
    if raw == "about:blank" {
        return raw.into();
    }
    let Ok(mut url) = url::Url::parse(raw) else {
        return String::new();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return String::new();
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.into()
}
fn scrub_urls(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::new();
    let mut offset = 0;
    while let Some(start) = lower[offset..].find("http").map(|v| v + offset) {
        if !lower[start..].starts_with("http://") && !lower[start..].starts_with("https://") {
            out.push_str(&text[offset..start + 4]);
            offset = start + 4;
            continue;
        }
        let end = text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`'))
            .map_or(text.len(), |e| start + e);
        out.push_str(&text[offset..start]);
        out.push_str(&display_url(&text[start..end]));
        offset = end;
    }
    out.push_str(&text[offset..]);
    if !out.contains(['?', '#']) {
        return out;
    }
    let mut safe = String::with_capacity(out.len());
    for token in out.split_inclusive(char::is_whitespace) {
        let word = token.trim_end();
        let address = word.trim_start_matches(['(', '[', '<', '"', '\'', '`']);
        if let Some((prefix, _)) = address.split_once(['?', '#'])
            && !prefix.contains("://")
            && url::Url::parse(&format!("https://{prefix}"))
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .is_some_and(|host| host.contains('.'))
        {
            safe.push_str(&word[..word.len() - address.len()]);
            let url = display_url(&format!("https://{prefix}"));
            safe.push_str(url.trim_start_matches("https://"));
            safe.push_str(&token[word.len()..]);
        } else {
            safe.push_str(token);
        }
    }
    safe
}
#[derive(Deserialize)]
struct Tabs {
    active_tab: Option<Uuid>,
    tabs: Vec<BrokerTab>,
}
#[derive(Deserialize)]
struct BrokerTab {
    tab_id: Uuid,
    url: String,
}
fn tabs(view: &mut Workspace, t: Tabs) -> Result<(), Failure> {
    if t.tabs.len() > 16
        || t.active_tab
            .is_some_and(|id| !t.tabs.iter().any(|tab| tab.tab_id == id))
    {
        return Err(Failure::Unknown);
    }
    view.active_tab_id = t.active_tab.map(|id| id.to_string());
    let mut mapped = Vec::new();
    for mut tab in t.tabs {
        tab.url = display_url(&tab.url);
        let id = tab.tab_id.to_string();
        let title = view
            .tabs
            .iter()
            .find(|b| b.id == id && b.url == tab.url)
            .map(|b| b.title.clone())
            .unwrap_or_default();
        if Some(&id) == view.active_tab_id.as_ref() {
            view.url = tab.url.clone();
            view.title = title.clone();
        }
        mapped.push(Tab {
            id,
            url: tab.url,
            title,
        });
    }
    view.tabs = mapped;
    Ok(())
}
async fn status(State(s): State<Arc<Service>>, h: HeaderMap, Path(thread): Path<String>) -> Reply {
    let scope = scope(&s, &h, thread).await?;
    let Some(j) = s.store.personal_browser_current(scope.clone()).await? else {
        return Ok(status_view(&s, None));
    };
    let mut v = j.view;
    if matches!(v.state.as_str(), "ready" | "user_control" | "completed") {
        match wire::call(&s, &v.id, "tabs", json!({})).await {
            Ok(t) => tabs(&mut v, t)?,
            Err(Failure::Missing) => {
                v.state = "outcome_unknown".into();
                v.reason = Some("browser_worker_missing".into());
            }
            Err(Failure::Busy) => {}
            Err(error) => return Err(error),
        }
        v = s
            .store
            .personal_browser_refresh(scope, v.revision, v)
            .await?;
    }
    Ok(status_view(&s, Some(v)))
}
async fn frame(State(s): State<Arc<Service>>, h: HeaderMap, Path(thread): Path<String>) -> Reply {
    let scope = scope(&s, &h, thread).await?;
    let Some(job) = s.store.personal_browser_current(scope.clone()).await? else {
        return Err(StoreError::NotFound.into());
    };
    let _permit = s.frames.try_acquire().map_err(|_| Failure::Stale)?;
    let v = job.view;
    if !matches!(
        v.state.as_str(),
        "reading" | "ready" | "user_control" | "completed"
    ) {
        return Err(Failure::Stale);
    }
    let tab = v.active_tab_id.ok_or(Failure::Stale)?;
    let f: WorkspaceFrame = wire::call(&s, &v.id, "frame", json!({"tab_id":tab})).await?;
    if f.width != 1280
        || f.height != 900
        || f.png_base64.len() > 3 * 1024 * 1024
        || f.frame_token.len() != 64
        || !f.frame_token.bytes().all(|c| c.is_ascii_hexdigit())
        || !valid_png(&f.png_base64, f.width, f.height)
    {
        return Err(Failure::Unknown);
    }
    let current = s
        .store
        .personal_browser_current(scope)
        .await?
        .ok_or(Failure::Stale)?;
    if current.view.id != v.id
        || current.view.revision != v.revision
        || current.view.active_tab_id.as_ref() != Some(&tab)
    {
        return Err(Failure::Stale);
    }
    Ok(response(Frame {
        workspace_id: v.id,
        revision: v.revision,
        tab_id: tab,
        frame_token: f.frame_token,
        png_base64: f.png_base64,
        width: f.width,
        height: f.height,
        url: display_url(&f.url),
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Start {
    request_id: Uuid,
    url: String,
    text: String,
    #[serde(default)]
    wait_for_user: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Guide {
    request_id: Uuid,
    workspace_id: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum StartOrGuide {
    Start(Start),
    Guide(Guide),
}
#[derive(Deserialize)]
struct Opened {
    tab_id: Uuid,
    observation: WorkspaceObservation,
}
async fn start(
    State(s): State<Arc<Service>>,
    h: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<StartOrGuide>,
) -> Reply {
    let scope = scope(&s, &h, thread).await?;
    let epoch = epoch(&h);
    match input {
        StartOrGuide::Guide(g) => {
            let job = s
                .store
                .personal_browser_guide(scope, g.workspace_id, g.request_id, scrub_urls(&g.text))
                .await?;
            Ok(status_view(&s, Some(job.view)))
        }
        StartOrGuide::Start(input) => {
            // Detached only for this accepted HTTP call: a lost response cannot strand or replay an action.

            let url = url::Url::parse(&input.url).map_err(|_| Failure::Invalid)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(Failure::Invalid);
            }
            if !wire::available(&s) {
                s.store.personal_browser_current(scope.clone()).await?;
                return Err(Failure::Unavailable);
            }
            let admission = s
                .store
                .personal_browser_open(
                    scope.clone(),
                    input.request_id,
                    display_url(&input.url),
                    scrub_urls(&input.text),
                    input.wait_for_user,
                )
                .await?;
            let j = match admission {
                Admission::Existing(j) => return Ok(status_view(&s, Some(j.view))),
                Admission::Fresh(j) => j,
            };
            let accepted = status_view(&s, Some(j.view.clone()));
            tokio::spawn(async move {
                let result =
                    broker_action::<Opened>(&s, &scope, &j.view, "open", json!({"url":input.url}))
                        .await;
                let opened = match result {
                    Ok(x) => x,
                    Err(err) => {
                        return failed(&s, scope, j.view, input.request_id, err, None).await;
                    }
                };
                observed(
                    &s,
                    scope,
                    j,
                    input.request_id,
                    opened,
                    input.wait_for_user,
                    epoch,
                )
                .await
            });
            Ok((StatusCode::ACCEPTED, accepted).into_response())
        }
    }
}
async fn observed(
    s: &Service,
    scope: Scope,
    mut j: Job,
    request: Uuid,
    opened: Opened,
    wait: bool,
    epoch: Option<i64>,
) -> Reply {
    let mut tab = opened.tab_id;
    let mut p = opened.observation.page;
    let mut previous = Vec::new();
    // Content already read by this execution; the store remembers the last read across executions.
    let mut seen = Vec::new();
    let mut stalled = false;
    let generated = loop {
        p.url = display_url(&p.url);
        p.title = scrub_urls(&p.title);
        p.text = scrub_urls(&page_reading(&p.text));
        // Query-bearing links are not durable action targets; retain only exact safe URLs.
        p.links.retain(|l| display_url(&l.url) == l.url);
        for link in &mut p.links {
            link.text = scrub_urls(&link.text);
        }
        publish_tab(&mut j, tab, &p);
        let workspace = workspace_lock(s, &j.view.id)?;
        let guard = workspace.lock().await;
        j = s
            .store
            .personal_browser_publish(scope.clone(), request, j.view)
            .await?;
        drop(guard);
        // Progress is judged on what was read and the instructions in force, never on UI wording.
        let choices = p
            .links
            .iter()
            .take(4)
            .filter(|link| public_read_link(&p.url, &link.url))
            .map(|link| (link.url.as_str(), link.text.as_str()))
            .collect::<Vec<_>>();
        let content = progress::fingerprint(&p.title, &p.text, &choices);
        let advanced = s
            .store
            .personal_browser_progress(scope.clone(), request, j.view.id.clone(), content.clone())
            .await?
            == Progress::Advanced
            && !seen.contains(&content);
        seen.push(content);
        if wait {
            break None;
        }
        if !advanced {
            // Another model call or navigation cannot add evidence: pause before any further effect.
            stalled = true;
            break None;
        }
        let candidates = if previous.len() < 2 && !j.human_touched && j.remaining_reads > 0 {
            p.links
                .iter()
                .filter(|link| {
                    public_read_link(&p.url, &link.url)
                        && !j.view.tabs.iter().any(|tab| tab.url == link.url)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let titles = candidates
            .iter()
            .map(|link| link.text.clone())
            .collect::<Vec<_>>();
        let index = match browser_model::answer(s, &scope, &j, &p, epoch, &titles, &previous).await
        {
            Some(browser_model::Decision::Follow(index)) => index,
            Some(browser_model::Decision::Answer(answer, model)) => break Some((answer, model)),
            None => break None,
        };
        let Some(link) = candidates.get(index) else {
            break None;
        };
        let url = link.url.clone();
        match s
            .store
            .personal_browser_follow_observed(
                scope.clone(),
                request,
                j.view.revision,
                url.clone(),
                j.guidance.len(),
            )
            .await
        {
            Ok(job) => j = job,
            // New human direction supersedes the proposed navigation.
            Err(StoreError::Conflict) => {
                break None;
            }
            Err(e) => return Err(e.into()),
        }
        let next: Opened = match broker_action(
            s,
            &scope,
            &j.view,
            "follow",
            json!({"tab_id":tab,"url":url}),
        )
        .await
        {
            Ok(next) => next,
            Err(e) => return failed(s, scope, j.view, request, e, Some("ready")).await,
        };
        previous.push(json!({"url":p.url,"title":p.title,"visible_text":p.text.chars().take(1600).collect::<String>()}));
        tab = next.tab_id;
        p = next.observation.page;
    };
    let (output, needs_user, reason) = match generated {
        Some((answer, model)) => {
            let text = if previous.is_empty() {
                format!("{}\n\n{}", scrub_urls(&answer.answer), p.url)
            } else {
                let mut text = scrub_urls(&answer.answer);
                for reading in &previous {
                    if let (Some(url), Some(excerpt)) =
                        (reading["url"].as_str(), reading["visible_text"].as_str())
                    {
                        append_browser_source(&mut text, url, excerpt);
                    }
                }
                append_browser_source(&mut text, &p.url, &p.text);
                text
            };
            (
                Output::Generated {
                    text,
                    model,
                    applied: j.guidance.len(),
                },
                answer.needs_user,
                "waiting_for_user",
            )
        }
        None => {
            let text = p.text.chars().take(2000).collect::<String>();
            let note = if stalled {
                "같은 화면과 같은 지시로는 새로운 내용이 없어 작업을 일시 중지했습니다. 추가 지시를 보내거나, 직접 조작으로 조건을 바꾸거나, 확인된 다른 링크를 열어 이어가세요."
            } else if wait {
                "현재 화면입니다. 조건을 변경한 뒤 완료를 눌러 주세요."
            } else {
                "페이지에서 확인한 내용입니다. 모델 답변을 받지 못해 작업을 열어 두었습니다."
            };
            (
                Output::Observation(format!("{note}\n\n{}\n{}\n\n{text}", p.title, p.url)),
                true,
                if stalled {
                    progress::NO_PROGRESS
                } else if wait {
                    "waiting_for_user"
                } else {
                    "browser_model_unavailable"
                },
            )
        }
    };
    // Model latency can span account fencing, a crashed worker, or a device takeover.
    // Revalidate the host namespace and selected tab immediately before local settlement.
    let active: Tabs = match broker_action(s, &scope, &j.view, "tabs", json!({})).await {
        Ok(t) => t,
        Err(Failure::Busy) => {
            return failed(s, scope, j.view, request, Failure::Busy, Some("ready")).await;
        }
        Err(e) => return failed(s, scope, j.view, request, e, None).await,
    };
    if active.active_tab != Some(tab)
        || !active
            .tabs
            .iter()
            .any(|t| t.tab_id == tab && display_url(&t.url) == p.url)
    {
        return failed(s, scope, j.view, request, Failure::Stale, Some("ready")).await;
    }
    j.view.state = if needs_user { "ready" } else { "completed" }.into();
    j.view.reason = needs_user.then(|| reason.into());
    let workspace = workspace_lock(s, &j.view.id)?;
    let _guard = workspace.lock().await;
    let view = s
        .store
        .personal_browser_finish(scope, j.view, request, Some(output))
        .await?;
    Ok(status_view(s, Some(view)))
}
// The Chromium worker projects both the accessibility tree and visible layout.
// Keep machine node metadata out of evidence budgets, model prompts and chat excerpts.
fn page_reading(projection: &str) -> String {
    let Some((tree, visible)) = projection
        .strip_prefix("Accessibility:\n")
        .and_then(|s| s.split_once("\nVisible layout text:\n"))
    else {
        return projection.into();
    };
    if !visible.trim().is_empty() {
        return visible.trim().to_owned();
    }
    tree.lines()
        .filter_map(|row| serde_json::from_str::<serde_json::Value>(row).ok())
        .filter_map(|node| node["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>()
        .join(" ")
}
// A comparison keeps verifiable excerpts from both pages even if a small model omits a detail.
fn append_browser_source(text: &mut String, url: &str, excerpt: &str) {
    let excerpt = excerpt.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = excerpt.chars();
    let mut snippet: String = chars.by_ref().take(200).collect();
    if chars.next().is_some() {
        snippet.push('…');
    }
    text.push_str(&format!("\n\n{url}\n{snippet}"));
}
fn publish_tab(job: &mut Job, id: Uuid, page: &moosuri_connectors::browser::capsule::RenderedPage) {
    let v = &mut job.view;
    let tab = Tab {
        id: id.to_string(),
        url: page.url.clone(),
        title: page.title.clone(),
    };
    v.url = tab.url.clone();
    v.title = tab.title.clone();
    v.active_tab_id = Some(tab.id.clone());
    if let Some(previous) = v.tabs.iter_mut().find(|other| other.id == tab.id) {
        *previous = tab;
    } else {
        v.tabs.push(tab);
    }
    v.links = if job.human_touched {
        vec![]
    } else {
        page.links
            .iter()
            .take(4)
            .map(|l| Link {
                url: l.url.clone(),
                text: l.text.clone(),
            })
            .collect()
    };
}
/// Model navigation uses uncredentialed, on-site document links from this exact observation.
/// Form submissions, action links and parameterized URLs require a human controller.
fn public_read_link(source: &str, target: &str) -> bool {
    let (Ok(base), Ok(url)) = (url::Url::parse(source), url::Url::parse(target)) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    url.origin() == base.origin()
        && matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && !path.contains('%')
        && ![
            "login",
            "logout",
            "sign-in",
            "signin",
            "signout",
            "delete",
            "remove",
            "unsubscribe",
            "checkout",
            "purchase",
            "payment",
            "confirm",
            "submit",
            "authorize",
            "oauth",
            "token",
            "booking",
            "reserve",
        ]
        .iter()
        .any(|term| path.contains(term))
}
async fn failed(
    s: &Service,
    scope: Scope,
    mut v: Workspace,
    request: Uuid,
    error: Failure,
    restore: Option<&str>,
) -> Reply {
    if matches!(error, Failure::Busy) && restore.is_none() {
        v.state = "closed".into();
        v.reason = Some("browser_busy".into());
    } else if matches!(error, Failure::Stale | Failure::Invalid | Failure::Busy)
        && let Some(state) = restore
    {
        v.state = state.into();
        v.reason = Some("browser_stale".into());
    } else {
        v.state = "outcome_unknown".into();
        v.reason = Some("browser_unavailable".into());
    }
    s.store
        .personal_browser_finish(scope, v, request, None)
        .await?;
    Err(error)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    workspace_id: String,
    revision: i64,
    request_id: Uuid,
    control_token: Option<String>,
    frame_token: Option<String>,
    input: Option<WorkspaceInput>,
    action: Option<String>,
    tab_id: Option<Uuid>,
    url: Option<String>,
}
async fn mutate(
    State(s): State<Arc<Service>>,
    h: HeaderMap,
    Path((thread, op)): Path<(String, String)>,
    Json(a): Json<Action>,
) -> Reply {
    let scope = scope(&s, &h, thread).await?;
    let epoch = epoch(&h);
    tokio::spawn(async move { dispatch(s, scope, op, a, epoch).await })
        .await
        .map_err(|_| Failure::Unknown)?
}
async fn dispatch(
    s: Arc<Service>,
    scope: Scope,
    op: String,
    a: Action,
    epoch: Option<i64>,
) -> Reply {
    let op = match op.as_str() {
        "takeover" => "takeover",
        "input" => "input",
        "resume" => "resume",
        "close" => "close",
        "tabs" => match a.action.as_deref() {
            Some("switch") => "switch",
            Some("close") => "close_tab",
            Some("follow") => "follow",
            _ => return Err(Failure::Invalid),
        },
        _ => return Err(StoreError::NotFound.into()),
    };
    let mut args = json!({});
    match op {
        "input" | "resume" => {
            let proof = a
                .frame_token
                .as_ref()
                .filter(|t| !t.is_empty() && t.len() <= 256)
                .ok_or(Failure::Invalid)?;
            args["frame_token"] = json!(proof);
            if op == "input" {
                let input = a.input.as_ref().ok_or(Failure::Invalid)?;
                input.validate().map_err(|_| Failure::Invalid)?;
                args["input"] = json!(input);
            }
        }
        "switch" => args["to_tab_id"] = json!(a.tab_id.ok_or(Failure::Invalid)?),
        "close_tab" => {
            if a.tab_id.is_none() {
                return Err(Failure::Invalid);
            }
        }
        "follow" => {
            args["url"] = json!(
                a.url
                    .as_ref()
                    .filter(|u| u.len() <= 2048)
                    .ok_or(Failure::Invalid)?
            )
        }
        _ => {}
    }
    // Keep a known non-dispatch rollback atomic relative to older background results.
    let workspace = workspace_lock(&s, &a.workspace_id)?;
    let dispatch_guard = workspace.lock().await;
    let prior = s
        .store
        .personal_browser_current(scope.clone())
        .await?
        .ok_or(StoreError::NotFound)?;
    if op == "close_tab" && a.tab_id.map(|t| t.to_string()) != prior.view.active_tab_id {
        return Err(Failure::Stale);
    }
    let control = if op == "takeover" {
        Some(Uuid::new_v4().to_string())
    } else {
        a.control_token
    };
    let version = Version {
        workspace_id: a.workspace_id,
        revision: a.revision,
        request_id: a.request_id,
    };
    let mut j = match s
        .store
        .personal_browser_reserve(scope.clone(), version, op, control.clone())
        .await?
    {
        Admission::Existing(j) => return Ok(status_view(&s, Some(j.view))),
        Admission::Fresh(j) => j,
    };
    if op == "takeover" {
        return Ok(response(Control {
            workspace: j.view,
            control_token: control.ok_or(Failure::Invalid)?,
        }));
    }
    if op != "close" {
        args["tab_id"] = json!(j.view.active_tab_id);
    }
    let wire_op = match op {
        "close" => "stop",
        "resume" => "selected",
        s => s,
    };
    let result = wire::call::<Value>(&s, &j.view.id, wire_op, args).await;
    let result = match result {
        Ok(r) => r,
        Err(Failure::Busy) => {
            s.store
                .personal_browser_busy(scope, prior, j.view.revision, a.request_id)
                .await?;
            return Err(Failure::Busy);
        }
        Err(Failure::Missing) if op == "close" => json!({"stopped":true}),
        Err(e) => return failed(&s, scope, j.view, a.request_id, e, Some(&prior.view.state)).await,
    };
    drop(dispatch_guard);
    match op {
        "resume" | "follow" => {
            let opened = if op == "follow" {
                serde_json::from_value::<Opened>(result).ok()
            } else {
                j.view
                    .active_tab_id
                    .as_ref()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .zip(serde_json::from_value::<WorkspaceObservation>(result).ok())
                    .map(|(tab_id, observation)| Opened {
                        tab_id,
                        observation,
                    })
            };
            let Some(opened) = opened else {
                return failed(&s, scope, j.view, a.request_id, Failure::Unknown, None).await;
            };
            return observed(&s, scope, j, a.request_id, opened, false, epoch).await;
        }
        "switch" | "close_tab" => {
            if let Ok(t) = serde_json::from_value(result)
                && tabs(&mut j.view, t).is_ok()
            {
                if j.view.tabs.is_empty() {
                    j.view.state = "closed".into();
                }
            } else {
                return failed(&s, scope, j.view, a.request_id, Failure::Unknown, None).await;
            }
        }
        "close" if result["stopped"] == true => {
            j.view.state = "closed".into();
            j.view.active_tab_id = None;
            j.view.tabs.clear();
            j.view.links.clear();
        }
        "input" if result["accepted"] == true => {}
        _ => return failed(&s, scope, j.view, a.request_id, Failure::Unknown, None).await,
    }
    let view = s
        .store
        .personal_browser_finish(scope, j.view, a.request_id, None)
        .await?;
    Ok(status_view(&s, Some(view)))
}

#[cfg(test)]
#[path = "browser_progress_tests.rs"]
mod progress_tests;
#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
