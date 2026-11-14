//! Fresh-world setup and real HTTP/Runtime execution. No gold imports.
use crate::{
    EvalResult,
    model::{Provider, Script},
    observe::{self, Observation},
    world::{self, Case, Kind},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use moosuri_agent::Runtime;
use moosuri_core::{IngestBatch, ModelSelection, ProviderConfig, ProviderKind, SourceInput};
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::{CredentialChange, LoginResult, ProviderChange, Store};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tower::ServiceExt;

const PASSWORD: &str = "Synthetic-evaluation-passphrase-2026";
const EMAIL: &str = "owner@evaluation.invalid";
const ORIGIN: &str = "https://moosuri-eval.invalid";
#[derive(Clone, Serialize)]
pub struct TurnTiming {
    pub admission_ms: u64,
    pub terminal_observed_ms: u64,
}
#[derive(Clone)]
pub struct Execution {
    pub observations: Vec<Observation>,
    pub timings: Vec<TurnTiming>,
    pub account: String,
    pub source: Option<String>,
    pub lifecycle: Option<Lifecycle>,
    pub replay_same_run: bool,
    pub mission_reopen_preserved: bool,
}
#[derive(Clone)]
pub struct Lifecycle {
    pub same_memory_id: bool,
    pub corrected_revision: i64,
    pub forgotten_status: String,
    pub remaining_versions: i64,
    pub remaining_index_rows: i64,
    pub remaining_derived_output: i64,
}
pub struct World {
    app: Option<Router>,
    runtime: Option<Runtime>,
    store: Option<Store>,
    login: Option<LoginResult>,
    pub provider: Provider,
    dir: tempfile::TempDir,
}
fn ms(t: Instant) -> u64 {
    t.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
fn value_id(v: &Value) -> EvalResult<String> {
    v["id"].as_str().map(str::to_owned).ok_or("response_id")
}
impl World {
    pub async fn setup(script: Script) -> EvalResult<Self> {
        let dir = tempfile::Builder::new()
            .prefix("moosuri-eval-")
            .tempdir()
            .map_err(|_| "world_directory")?;
        let provider = Provider::start(script).await?;
        let store = Store::open(dir.path()).map_err(|_| "world_store")?;
        store
            .bootstrap(
                "Synthetic evaluator owner".into(),
                EMAIL.into(),
                PASSWORD.into(),
                false,
            )
            .await
            .map_err(|_| "world_bootstrap")?;
        let login = store
            .login(EMAIL.into(), PASSWORD.into())
            .await
            .map_err(|_| "world_login")?;
        let provider_row = store
            .create_provider(
                &login.user,
                ProviderChange {
                    name: "Synthetic loopback fixture".into(),
                    enabled: true,
                    config: ProviderConfig {
                        kind: ProviderKind::Compatible,
                        base_url: Some(provider.base.clone()),
                        allowed_ips: vec!["127.0.0.1".into()],
                        router_providers: vec![],
                        zero_data_retention: false,
                    },
                    credential: CredentialChange::Clear,
                },
            )
            .await
            .map_err(|_| "world_provider")?;
        // Trusted fixture setup records the exact scripted protocol capability;
        // this is not a live provider capability/quality probe.
        store
            .record_model_probe(
                &login.user,
                provider_row.id.clone(),
                provider_row.revision,
                world::MODEL.into(),
                None,
            )
            .await
            .map_err(|_| "world_text_profile")?;
        store
            .record_tool_probe(
                &login.user,
                provider_row.id.clone(),
                provider_row.revision,
                world::MODEL.into(),
                None,
            )
            .await
            .map_err(|_| "world_tool_profile")?;
        store
            .set_default_model(
                &login.user,
                Some(ModelSelection {
                    source: Default::default(),
                    provider_id: provider_row.id,
                    provider_revision: provider_row.revision,
                    model: world::MODEL.into(),
                    effort: None,
                }),
            )
            .await
            .map_err(|_| "world_default")?;
        let runtime = Runtime::start(store.clone());
        let app = router_with_runtime(
            store.clone(),
            ServerConfig::new(ORIGIN, false).map_err(|_| "world_origin")?,
            runtime.clone(),
        );
        Ok(Self {
            app: Some(app),
            runtime: Some(runtime),
            store: Some(store),
            login: Some(login),
            provider,
            dir,
        })
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn login(&self) -> &LoginResult {
        self.login.as_ref().unwrap()
    }
    async fn request(&self, method: &str, path: &str, body: Value) -> EvalResult<(u16, Value)> {
        let response = self.response(method, path, body).await?;
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), 256 * 1024)
            .await
            .map_err(|_| "http_body_bound")?;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| "http_json")?
        };
        Ok((status, value))
    }
    async fn response(
        &self,
        method: &str,
        path: &str,
        body: Value,
    ) -> EvalResult<axum::response::Response> {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "moosuri-eval.invalid")
            .header("origin", ORIGIN)
            .header("content-type", "application/json")
            .header("cookie", format!("moosuri_session={}", self.login().token))
            .header("x-moosuri-account", &self.login().user.account().id)
            .body(Body::from(body.to_string()))
            .map_err(|_| "http_request")?;
        self.app
            .as_ref()
            .unwrap()
            .clone()
            .oneshot(request)
            .await
            .map_err(|_| "http_router")
    }
    async fn checked(
        &self,
        method: &str,
        path: &str,
        body: Value,
        status: u16,
    ) -> EvalResult<Value> {
        let (actual, value) = self.request(method, path, body).await?;
        if actual != status {
            return Err("http_unexpected_status");
        }
        Ok(value)
    }
    async fn new_thread(&self) -> EvalResult<String> {
        value_id(
            &self
                .checked(
                    "POST",
                    "/api/v1/threads",
                    json!({"title":"Synthetic outcome evaluation"}),
                    201,
                )
                .await?,
        )
    }
    async fn turn(
        &self,
        text: &str,
        disconnect: bool,
    ) -> EvalResult<(Observation, TurnTiming, bool)> {
        let thread = self.new_thread().await?;
        let path = format!("/api/v1/threads/{thread}/messages");
        let request_id = uuid::Uuid::new_v4().to_string();
        let body = json!({"request_id":request_id,"text":text});
        let ledger_offset = self.provider.ledger().len();
        let started = Instant::now();
        let accepted = self.checked("POST", &path, body.clone(), 201).await?;
        let admission_ms = ms(started);
        let run_id = accepted["run"]["id"].as_str().ok_or("admission_run_id")?;
        let mut same_run = false;
        if disconnect {
            // Drop the actual admitted response and an actual SSE response body;
            // neither lifetime owns the server's execution. Replay the same UUID.
            let stream = self
                .response(
                    "GET",
                    &format!("/api/v1/threads/{thread}/events"),
                    Value::Null,
                )
                .await?;
            if stream.status().as_u16() != 200 {
                return Err("stream_admission");
            }
            drop(stream);
            let replay = self.checked("POST", &path, body, 201).await?;
            same_run = replay["run"]["id"] == run_id;
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        loop {
            let page = self.checked("GET", &path, Value::Null, 200).await?;
            if let Some(turn) = page["turns"].as_array().and_then(|ts| ts.last())
                && !matches!(turn["run"]["state"].as_str(), Some("queued" | "running"))
            {
                let ledger = self
                    .provider
                    .ledger()
                    .into_iter()
                    .skip(ledger_offset)
                    .collect();
                return Ok((
                    observe::snapshot(self.dir.path(), run_id, ledger)?,
                    TurnTiming {
                        admission_ms,
                        terminal_observed_ms: ms(started),
                    },
                    same_run,
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err("terminal_timeout");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    async fn source(&self, login: &LoginResult, text: &str) -> EvalResult<String> {
        let collection = self
            .store()
            .create_collection(
                &login.user,
                "Synthetic public fixtures".into(),
                uuid::Uuid::new_v4().to_string(),
            )
            .await
            .map_err(|_| "world_collection")?;
        let receipt = self
            .store()
            .ingest_text(
                &login.user,
                collection.id,
                IngestBatch {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    expected_cursor: 0,
                    next_cursor: 1,
                    complete_snapshot: false,
                    entries: vec![SourceInput {
                        external_id: "quote".into(),
                        version: "v1".into(),
                        expected_revision: None,
                        title: "Synthetic quote".into(),
                        text: text.into(),
                        speaker: Some("Synthetic issuer".into()),
                        observed_at: world::FIXTURE_CLOCK,
                        valid_from: None,
                        valid_until: None,
                    }],
                },
            )
            .await
            .map_err(|_| "world_ingest")?;
        receipt
            .source_ids
            .first()
            .cloned()
            .ok_or("world_ingest_receipt")
    }
    async fn foreign_source(&self) -> EvalResult<()> {
        let invite = self
            .store()
            .create_user(
                &self.login().user,
                "Synthetic other owner".into(),
                "other@evaluation.invalid".into(),
            )
            .await
            .map_err(|_| "world_other_owner")?;
        let restricted = self
            .store()
            .login("other@evaluation.invalid".into(), invite.temporary_password)
            .await
            .map_err(|_| "world_other_login")?;
        let other = self
            .store()
            .change_password(&restricted.user, "Synthetic-other-owner-passphrase".into())
            .await
            .map_err(|_| "world_other_password")?;
        self.source(&other, world::FOREIGN_SOURCE).await?;
        Ok(())
    }
    async fn restart(&mut self) -> EvalResult<()> {
        self.shutdown().await;
        self.app.take();
        self.runtime.take();
        self.login.take();
        self.store.take();
        let store = Store::open(self.dir.path()).map_err(|_| "world_reopen")?;
        let login = store
            .login(EMAIL.into(), PASSWORD.into())
            .await
            .map_err(|_| "world_reopen_login")?;
        let runtime = Runtime::start(store.clone());
        self.app = Some(router_with_runtime(
            store.clone(),
            ServerConfig::new(ORIGIN, false).map_err(|_| "world_origin")?,
            runtime.clone(),
        ));
        self.runtime = Some(runtime);
        self.store = Some(store);
        self.login = Some(login);
        Ok(())
    }
    pub async fn shutdown(&self) {
        if let Some(runtime) = &self.runtime {
            runtime.shutdown().await;
        }
    }
    pub fn usage_totals(&self) -> Option<(Option<u64>, Option<u64>)> {
        let conn = observe::open(self.dir.path()).ok()?;
        let mut statement = conn.prepare("SELECT usage FROM model_call_receipts").ok()?;
        let rows = statement.query_map([], |r| r.get::<_, String>(0)).ok()?;
        let (mut input, mut output) = (Some(0_u64), Some(0_u64));
        let mut seen = false;
        for row in rows {
            let v: Value = serde_json::from_str(&row.ok()?).ok()?;
            seen = true;
            input = input.zip(v["input_tokens"].as_u64()).map(|(a, b)| a + b);
            output = output.zip(v["output_tokens"].as_u64()).map(|(a, b)| a + b);
        }
        seen.then_some((input, output))
    }
    pub async fn execute(&mut self, case: Case) -> EvalResult<Execution> {
        let mut out = Execution {
            observations: vec![],
            timings: vec![],
            account: self.login().user.account().id.clone(),
            source: None,
            lifecycle: None,
            replay_same_run: false,
            mission_reopen_preserved: false,
        };
        match case.kind {
            Kind::Memory => {
                let draft = |text: &str| json!({"text":text,"subject":"preference","speaker":null,"valid_from":null,"valid_until":null,"evidence":[]});
                let memory = self.checked("POST","/api/v1/knowledge/memories",json!({"request_id":uuid::Uuid::new_v4(),"draft":draft(world::PREFERENCE_A)}),201).await?;
                let id = value_id(&memory)?;
                let (observed, timing, _) = self.turn("What is my preference?", false).await?;
                out.observations.push(observed);
                out.timings.push(timing);
                let corrected = self.checked("PUT",&format!("/api/v1/knowledge/memories/{id}"),json!({"request_id":uuid::Uuid::new_v4(),"expected_revision":1,"draft":draft(world::PREFERENCE_B)}),200).await?;
                let (observed, timing, _) = self.turn("What is my preference?", false).await?;
                out.observations.push(observed);
                out.timings.push(timing);
                self.checked(
                    "DELETE",
                    &format!("/api/v1/knowledge/memories/{id}"),
                    json!({"expected_revision":2}),
                    204,
                )
                .await?;
                self.restart().await?;
                let (observed, timing, _) = self.turn("What is my preference?", false).await?;
                out.observations.push(observed);
                out.timings.push(timing);
                let conn = observe::open(self.dir.path())?;
                let (forgotten_status,remaining_versions,remaining_index_rows): (String,i64,i64) = conn.query_row("SELECT status,(SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1),(SELECT count(*) FROM knowledge_fts WHERE object_id=?1) FROM knowledge_memories WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_| "observer_memory_lifecycle")?;
                let remaining_derived_output = conn
                    .query_row(
                        "SELECT count(*) FROM chat_output WHERE run_id IN (?1,?2)",
                        rusqlite::params![out.observations[0].run_id, out.observations[1].run_id],
                        |r| r.get(0),
                    )
                    .map_err(|_| "observer_derived_output")?;
                out.lifecycle = Some(Lifecycle {
                    same_memory_id: corrected["id"] == id,
                    corrected_revision: corrected["revision"].as_i64().unwrap_or(0),
                    forgotten_status,
                    remaining_versions,
                    remaining_index_rows,
                    remaining_derived_output,
                });
            }
            Kind::Disconnect | Kind::Truncated => {
                let (observed, timing, replayed) = self
                    .turn(
                        "Give a brief greeting.",
                        matches!(case.kind, Kind::Disconnect),
                    )
                    .await?;
                out.observations.push(observed);
                out.timings.push(timing);
                out.replay_same_run = replayed;
            }
            Kind::Calculation | Kind::SkilledCalculation => {
                out.source = Some(self.source(self.login(), world::SOURCE).await?);
                self.foreign_source().await?;
                let (observed,timing,_) = self.turn("Calculate the saved quote amount plus shipping in USD. Do not purchase anything.",false).await?;
                out.observations.push(observed);
                out.timings.push(timing);
            }
            Kind::Mission => {
                let source = self.source(self.login(), world::SOURCE).await?;
                out.source = Some(source.clone());
                let started = Instant::now();
                // These are explicit user-authored product acceptance checks,
                // not evaluator gold or hidden grading fields.
                let admitted=self.checked("POST","/api/v1/missions",json!({"request_id":uuid::Uuid::new_v4(),"task":"Calculate the saved quote amount plus shipping in USD. Do not purchase anything.","constraints":"","checks":[{"kind":"decimal_equals","expected":"24.75"},{"kind":"source_quote","id":source,"revision":1,"quote":world::SOURCE}],"execution_mode":"durable"}),200).await?;
                let admission_ms = ms(started);
                let id = value_id(&admitted)?;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
                loop {
                    let missions = self
                        .checked("GET", "/api/v1/missions", Value::Null, 200)
                        .await?;
                    if let Some(m) = missions
                        .as_array()
                        .and_then(|ms| ms.iter().find(|m| m["id"] == id))
                        && !matches!(
                            m["state"].as_str(),
                            Some("queued" | "planning" | "executing" | "verifying")
                        )
                    {
                        break;
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Err("mission_terminal_timeout");
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                let run_id: String = observe::open(self.dir.path())?
                    .query_row("SELECT run_id FROM missions WHERE id=?1", [&id], |r| {
                        r.get(0)
                    })
                    .map_err(|_| "observer_mission_run")?;
                let observation =
                    observe::snapshot(self.dir.path(), &run_id, self.provider.ledger())?;
                let candidate = observation.mission.as_ref().map(|m| m.candidate.clone());
                out.observations.push(observation);
                out.timings.push(TurnTiming {
                    admission_ms,
                    terminal_observed_ms: ms(started),
                });
                let dispatched = self.provider.ledger().len();
                self.restart().await?;
                let reopened = observe::snapshot(self.dir.path(), &run_id, self.provider.ledger())?;
                out.mission_reopen_preserved = self.provider.ledger().len() == dispatched
                    && reopened.mission.as_ref().is_some_and(|m| {
                        m.state == "achieved" && Some(&m.candidate) == candidate.as_ref()
                    });
            }
        }
        Ok(out)
    }
}
