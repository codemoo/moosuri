//! Disposable public-page reader for member public watches.
//!
//! Runs only from the member timer, through `moosuri_server::personal_tasks::PublicReads`. It reuses the
//! verified member browser broker socket and its single command-admission slot (`browser_wire`), and
//! nothing else: no generic network client, no member cookies or session, no model, and no broker
//! operation other than `open` (a *fresh* workspace on the pinned page) and `stop` (that same
//! workspace). The entry point it uses ([`wire::disposable`]) cannot even name another operation.
//!
//! A human's live workspace is never addressed: the store mints a fresh workspace id per slot, the broker
//! refuses to `open` an id that already exists, and this reader only ever sends the id from its lease.
//! Whatever happens during the read, it ends with a `stop` of that workspace; if the stop cannot be
//! confirmed the result says so (`closed: false`) and the timer stops that id later.
use super::{
    browser::Failure,
    browser_wire::{self as wire, Disposable, Endpoint},
};
use moosuri_connectors::browser::capsule::WorkspaceObservation;
use moosuri_server::personal_tasks::{PublicReadFuture, PublicReads, PublicStopFuture};
use moosuri_store::personal_browser::{
    PublicPage, PublicRead, PublicReadOutcome, PublicWatchLease,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex as DispatchLock;
use uuid::Uuid;

pub(super) struct Reader {
    socket: Option<PathBuf>,
    uid: u32,
    generation: u64,
    slot: Arc<DispatchLock<()>>,
}

/// `socket`/`uid` are exactly what the human browser routes use for the same member runtime (the fixed
/// root-owned broker socket in a deployment). `None` yields an honestly unavailable reader.
pub(super) fn reader(socket: Option<PathBuf>, uid: u32, generation: u64) -> Arc<dyn PublicReads> {
    let slot = wire::slot_for(socket.as_deref());
    Arc::new(Reader {
        socket,
        uid,
        generation,
        slot,
    })
}

#[derive(Deserialize)]
struct Opened {
    observation: WorkspaceObservation,
}

fn reason(failure: &Failure) -> &'static str {
    match failure {
        Failure::Busy => "browser_busy",
        Failure::Unavailable => "browser_unavailable",
        Failure::Forbidden => "destination_refused",
        Failure::Stale => "workspace_conflict",
        Failure::Invalid => "invalid_request",
        Failure::Missing => "worker_missing",
        Failure::Unknown | Failure::Store(_) => "outcome_unknown",
    }
}

/// The lease is already validated by the store; this refuses anything that is not `https` on the pinned
/// host before a single byte reaches the broker.
fn target_ok(url: &str, host: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && matches!(u.host(), Some(url::Host::Domain(h)) if h.eq_ignore_ascii_case(host))
    })
}

/// The worker projects both the accessibility tree and the visible layout; only the visible text is
/// page content. Node metadata never enters the fingerprint.
fn visible_text(projection: &str) -> String {
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
        .filter_map(|row| serde_json::from_str::<Value>(row).ok())
        .filter_map(|node| node["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Reader {
    fn endpoint(&self) -> Endpoint<'_> {
        Endpoint {
            socket: self.socket.as_deref(),
            uid: self.uid,
            generation: self.generation,
            slot: &self.slot,
        }
    }

    /// True once the broker confirms the workspace is gone (a missing one is gone too).
    async fn stop_workspace(&self, id: &str) -> bool {
        match wire::disposable::<Value>(&self.endpoint(), id, Disposable::Stop, json!({})).await {
            Ok(reply) => reply["stopped"] == true,
            Err(Failure::Missing) => true,
            Err(_) => false,
        }
    }

    async fn read_target(&self, url: &str, host: &str, workspace: Uuid) -> PublicRead {
        let unread = |code| PublicRead {
            outcome: PublicReadOutcome::Failed(code),
            // Nothing was opened, so there is nothing to stop.
            closed: true,
        };
        if !target_ok(url, host) {
            return unread("invalid_target");
        }
        let endpoint = self.endpoint();
        if !wire::disposable_available(&endpoint) {
            return unread("browser_unavailable");
        }
        let id = workspace.to_string();
        let opened =
            wire::disposable::<Opened>(&endpoint, &id, Disposable::Open, json!({"url": url})).await;
        let outcome = match opened {
            Ok(opened) => {
                let page = opened.observation.page;
                PublicReadOutcome::Page(PublicPage {
                    url: page.url,
                    title: page.title,
                    text: visible_text(&page.text),
                    observed_at: i64::try_from(page.retrieved_at_unix_ms / 1000).unwrap_or(0),
                })
            }
            Err(failure) => PublicReadOutcome::Failed(reason(&failure)),
        };
        // Always close the disposable workspace, whatever the read did (an open that timed out may
        // still have started one).
        let closed = self.stop_workspace(&id).await;
        PublicRead { outcome, closed }
    }
}

impl PublicReads for Reader {
    fn read<'a>(&'a self, lease: &'a PublicWatchLease) -> PublicReadFuture<'a> {
        Box::pin(async move {
            self.read_target(&lease.url, &lease.host, lease.workspace_id)
                .await
        })
    }
    fn stop<'a>(&'a self, workspace_id: &'a str) -> PublicStopFuture<'a> {
        Box::pin(async move {
            match Uuid::parse_str(workspace_id) {
                Ok(_) => self.stop_workspace(workspace_id).await,
                // Not an id this reader could ever have opened: nothing to stop.
                Err(_) => true,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, path::Path, sync::Mutex};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };

    const URL: &str = "https://status.example.test/service";
    type Log = Arc<Mutex<Vec<Value>>>;

    /// A private same-UID socket standing in for the broker; every request line is recorded.
    fn broker(path: &Path, reply: impl Fn(&Value) -> Value + Send + 'static) -> Log {
        let listener = UnixListener::bind(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let log: Log = Default::default();
        let seen = log.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await.unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                seen.lock().unwrap().push(request.clone());
                let body = reply(&request);
                stream
                    .write_all(format!("{body}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        log
    }
    fn reader_on(socket: Option<PathBuf>) -> Reader {
        Reader {
            socket,
            uid: unsafe { libc::geteuid() },
            generation: 1,
            slot: Default::default(),
        }
    }
    fn opened() -> Value {
        json!({"ok":true,"result":{"tab_id":Uuid::new_v4(),"observation":{"page":{
            "title":"Status","url":URL,"warning":"","links":[],"truncated":false,"retrieved_at_unix_ms":5000,
            "text":"Accessibility:\n{\"name\":\"node-metadata\"}\n\nVisible layout text:\nAll systems operational"}}}})
    }
    fn ops(log: &Log) -> Vec<String> {
        log.lock()
            .unwrap()
            .iter()
            .map(|r| r["op"].as_str().unwrap().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn public_read_opens_then_stops_only_its_own_fresh_workspace_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("b.sock");
        let log = broker(&socket, |r| match r["op"].as_str().unwrap() {
            "open" => opened(),
            "stop" => json!({"ok":true,"result":{"stopped":true}}),
            other => panic!("a public read must not send {other}"),
        });
        let workspace = Uuid::new_v4();
        let read = reader_on(Some(socket))
            .read_target(URL, "status.example.test", workspace)
            .await;
        assert_eq!(ops(&log), ["open", "stop"]);
        let sent = log.lock().unwrap().clone();
        for request in &sent {
            // Exactly the closed wire shape: no cookies, headers, selectors, tabs or input.
            let mut keys: Vec<_> = request.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            let expected: &[&str] = if request["op"] == "open" {
                &["generation", "op", "url", "v", "workspace_id"]
            } else {
                &["generation", "op", "v", "workspace_id"]
            };
            assert_eq!(keys, expected);
            assert_eq!(request["workspace_id"], workspace.to_string());
            assert_eq!(request["generation"], 1);
        }
        assert_eq!(sent[0]["url"], URL);
        assert!(read.closed);
        let PublicReadOutcome::Page(page) = read.outcome else {
            panic!("expected a page")
        };
        // Visible text only; node metadata never reaches the fingerprint input.
        assert_eq!(
            (page.text.as_str(), page.url.as_str(), page.observed_at),
            ("All systems operational", URL, 5)
        );
    }

    #[tokio::test]
    async fn every_failure_stops_the_workspace_and_none_becomes_a_page() {
        let dir = tempfile::tempdir().unwrap();
        // The broker refuses the destination, and its stop is busy: the failure is honest, the
        // workspace is reported unconfirmed (the timer stops it later), and no page is produced.
        let socket = dir.path().join("refuse.sock");
        let log = broker(&socket, |r| match r["op"].as_str().unwrap() {
            "open" => json!({"ok":false,"code":"forbidden"}),
            _ => json!({"ok":false,"code":"capacity"}),
        });
        let read = reader_on(Some(socket))
            .read_target(URL, "status.example.test", Uuid::new_v4())
            .await;
        assert_eq!(ops(&log), ["open", "stop"]);
        assert!(matches!(
            read.outcome,
            PublicReadOutcome::Failed("destination_refused")
        ));
        assert!(!read.closed);
        // A worker that is already gone counts as stopped.
        let socket = dir.path().join("gone.sock");
        let log = broker(&socket, |r| match r["op"].as_str().unwrap() {
            "open" => json!({"ok":false,"code":"unavailable"}),
            _ => json!({"ok":false,"code":"missing"}),
        });
        let read = reader_on(Some(socket))
            .read_target(URL, "status.example.test", Uuid::new_v4())
            .await;
        assert_eq!(ops(&log), ["open", "stop"]);
        assert!(matches!(
            read.outcome,
            PublicReadOutcome::Failed("outcome_unknown")
        ));
        assert!(read.closed);
        // Targets off the pinned host or not https never reach the broker at all.
        let socket = dir.path().join("untouched.sock");
        let log = broker(&socket, |_| panic!("the broker must not be contacted"));
        let reader = reader_on(Some(socket));
        for (url, host) in [
            ("http://status.example.test/service", "status.example.test"),
            ("https://other.example.test/service", "status.example.test"),
            (
                "https://user:pw@status.example.test/service",
                "status.example.test",
            ),
            (
                "https://status.example.test:8443/service",
                "status.example.test",
            ),
            ("https://203.0.113.9/service", "203.0.113.9"),
        ] {
            let read = reader.read_target(url, host, Uuid::new_v4()).await;
            assert!(
                matches!(read.outcome, PublicReadOutcome::Failed("invalid_target")),
                "{url}"
            );
            assert!(read.closed);
        }
        assert!(ops(&log).is_empty());
        // No broker socket: unavailable, nothing opened, nothing to stop.
        let read = reader_on(None)
            .read_target(URL, "status.example.test", Uuid::new_v4())
            .await;
        assert!(matches!(
            read.outcome,
            PublicReadOutcome::Failed("browser_unavailable")
        ));
        assert!(read.closed);
    }
}
