//! Official SDK initialize catalog only. No user turn and no credential reads.
use super::*;
use serde_json::{Value, json};

pub struct AccountModels {
    pub login_required: bool,
    pub models: Vec<String>,
    pub effort_metadata: Vec<ModelEffortMetadata>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelEffortMetadata {
    pub model: String,
    /// None means the official catalog omitted or contradicted capability data.
    /// Some(empty) explicitly reports no supported effort on this transport.
    pub efforts: Option<Vec<String>>,
}
fn row_efforts(row: &Value) -> Option<Vec<String>> {
    match row.get("supportsEffort").and_then(Value::as_bool) {
        Some(false) => Some(vec![]),
        Some(true) => {
            let levels = row.get("supportedEffortLevels")?.as_array()?;
            if levels.len() > 16
                || levels.iter().any(|v| {
                    !matches!(
                        v.as_str(),
                        Some("low" | "medium" | "high" | "xhigh" | "max")
                    )
                })
            {
                return None;
            }
            // The pinned helper consumes settings.json. max is session-only and
            // cannot be encoded by effortLevel, so it is never offered here.
            Some(
                ["low", "medium", "high", "xhigh"]
                    .into_iter()
                    .filter(|level| levels.iter().any(|v| v == level))
                    .map(str::to_owned)
                    .collect(),
            )
        }
        None => None,
    }
}
fn valid_model(model: &str) -> bool {
    model.starts_with("claude-")
        && model.len() <= 200
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}
pub(super) fn catalog(v: &Value, request_id: &str) -> Result<AccountModels, TransportError> {
    if v["type"] != "control_response"
        || v["response"]["request_id"] != request_id
        || v["response"]["subtype"] != "success"
    {
        return Err(err());
    }
    let response = &v["response"]["response"];
    let account = response
        .get("account")
        .and_then(Value::as_object)
        .ok_or_else(err)?;
    // Only a personal Claude subscription is eligible. Keep all identity fields
    // in this local value; the service receives neither email nor OAuth material.
    let subscription = account
        .get("subscriptionType")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty() && s != "none");
    let first_party = account
        .get("apiProvider")
        .is_some_and(|v| v == "firstParty");
    let rows = response
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(err)?;
    if rows.len() > 64 {
        return Err(TransportError::LimitExceeded);
    }
    let mut models = Vec::new();
    // Resolve eligibility from the official standard aliases only. A separately
    // billed tier is excluded even when it is the account's current default.
    let standard = ["opus", "sonnet", "haiku"]
        .into_iter()
        .filter_map(|alias| {
            rows.iter()
                .find(|row| row["value"] == alias)
                .and_then(|row| row["resolvedModel"].as_str())
                .filter(|value| valid_model(value) && !value.contains("fable"))
        })
        .collect::<Vec<_>>();
    for alias in ["default", "opus", "sonnet", "haiku"] {
        let Some(row) = rows.iter().find(|row| row["value"] == alias) else {
            continue;
        };
        let Some(value) = row["resolvedModel"].as_str() else {
            continue;
        };
        if standard.contains(&value) && !models.iter().any(|m| m == value) && models.len() < 3 {
            models.push(value.to_owned());
        }
    }
    let effort_metadata = models
        .iter()
        .map(|model| {
            let matching = rows.iter().filter(|row| {
                matches!(row["value"].as_str(), Some("opus" | "sonnet" | "haiku"))
                    && row["resolvedModel"] == model.as_str()
            });
            let mut efforts: Option<Vec<String>> = None;
            for row in matching {
                let Some(reported) = row_efforts(row) else {
                    return ModelEffortMetadata {
                        model: model.clone(),
                        efforts: None,
                    };
                };
                match &mut efforts {
                    None => efforts = Some(reported),
                    Some(current) => current.retain(|level| reported.contains(level)),
                }
            }
            ModelEffortMetadata {
                model: model.clone(),
                efforts,
            }
        })
        .collect();
    Ok(AccountModels {
        effort_metadata,
        login_required: !subscription || !first_party,
        models,
    })
}
impl Profile {
    /// Reuses the pinned text helper unchanged: stream-json control messages are
    /// official CLI input. There is no user message, so discovery cannot infer.
    pub async fn account_models(
        &self,
        environment: &str,
        cancel: &CancellationToken,
    ) -> Result<AccountModels, TransportError> {
        let _os = tokio::select! {_=cancel.cancelled()=>return Err(TransportError::Cancelled),r=self.os_lock(environment)=>r?};
        let (env, temp, listener) = tokio::select! {_=cancel.cancelled()=>return Err(TransportError::Cancelled),r=self.prepare(environment)=>r?};
        for (file, bytes) in [
            ("system.txt", ""),
            ("mcp.json", "{\"mcpServers\":{}}"),
            (
                "settings.json",
                "{\"disableAllHooks\":true,\"autoMemoryEnabled\":false}",
            ),
        ] {
            std::fs::write(temp.path().join(file), bytes).map_err(|_| err())?;
        }
        let (session, request_id) = protocol::ids();
        let name = format!("moosuri-claude-{session}");
        let local = CancellationToken::new();
        let proxy = ProxyTask(tokio::spawn(proxy_server(listener, local.clone())));
        let mut cmd = self.launch(&env, temp.path(), &name, false, false);
        cmd.arg("default")
            .arg(&session)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let operation = async {
            self.allowed_environment(environment)?;
            let mut child = cmd.spawn().map_err(|_| TransportError::Transport)?;
            let mut stdin = child.stdin.take().ok_or_else(err)?;
            let input = json!({"type":"control_request","request_id":request_id,"request":{"subtype":"initialize"}}).to_string() + "\n";
            stdin
                .write_all(input.as_bytes())
                .await
                .map_err(|_| TransportError::Transport)?;
            // Leave input open until initialize settles (the official SDK does so).
            let mut stdin = Some(stdin);
            let mut stdout = BufReader::new(child.stdout.take().ok_or_else(err)?);
            let mut found = None;
            let mut total = 0;
            loop {
                let mut line = Vec::new();
                let n = (&mut stdout)
                    .take(256 * 1024 + 1)
                    .read_until(b'\n', &mut line)
                    .await
                    .map_err(|_| TransportError::Transport)?;
                if n == 0 {
                    break;
                }
                total += n;
                if n > 256 * 1024 || total > 1024 * 1024 || line.last() != Some(&b'\n') {
                    return Err(TransportError::LimitExceeded);
                }
                let v: Value = serde_json::from_slice(&line).map_err(|_| err())?;
                if v["type"] == "control_response" {
                    if found.is_some() {
                        return Err(err());
                    }
                    found = Some(catalog(&v, &request_id)?);
                    let mut input = stdin.take().ok_or_else(err)?;
                    input
                        .shutdown()
                        .await
                        .map_err(|_| TransportError::Transport)?;
                    drop(input);
                } else if v["type"] != "system"
                    || !matches!(v["subtype"].as_str(), Some("init" | "ui_invalidate"))
                    || (v["subtype"] == "init" && v["session_id"] != session)
                {
                    return Err(err());
                }
            }
            if !child
                .wait()
                .await
                .map_err(|_| TransportError::Transport)?
                .success()
            {
                return Err(TransportError::Transport);
            }
            found.ok_or_else(err)
        };
        let result = tokio::select! {biased;_=cancel.cancelled()=>Err(TransportError::Cancelled),r=tokio::time::timeout(Duration::from_secs(30),operation)=>r.unwrap_or(Err(TransportError::Timeout))};
        local.cancel();
        proxy.0.abort();
        if self.remove(&name).await.is_err() {
            let _ = self.block_environment(environment);
            return Err(TransportError::Transport);
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_catalog_metadata_is_required_and_transport_levels_are_bounded() {
        let response = |rows| json!({"type":"control_response","response":{"subtype":"success","request_id":"r","response":{"account":{"subscriptionType":"pro","apiProvider":"firstParty"},"models":rows}}});
        let parsed = catalog(&response(json!([
            {"value":"default","resolvedModel":"claude-first","supportsEffort":true,"supportedEffortLevels":["low"]},
            {"value":"opus","resolvedModel":"claude-first","supportsEffort":true,"supportedEffortLevels":["high","xhigh","max"]},
            {"value":"sonnet","resolvedModel":"claude-second","supportsEffort":false},
            {"value":"haiku","resolvedModel":"claude-third"}
        ])), "r").unwrap();
        assert_eq!(
            parsed.effort_metadata,
            vec![
                ModelEffortMetadata {
                    model: "claude-first".into(),
                    efforts: Some(vec!["high".into(), "xhigh".into()])
                },
                ModelEffortMetadata {
                    model: "claude-second".into(),
                    efforts: Some(vec![])
                },
                ModelEffortMetadata {
                    model: "claude-third".into(),
                    efforts: None
                },
            ]
        );
        for malformed in [
            json!({"supportsEffort":true}),
            json!({"supportsEffort":"true","supportedEffortLevels":["high"]}),
            json!({"supportsEffort":true,"supportedEffortLevels":["ultra"]}),
        ] {
            let mut row = malformed;
            row["value"] = json!("opus");
            row["resolvedModel"] = json!("claude-first");
            assert_eq!(
                catalog(&response(json!([row])), "r")
                    .unwrap()
                    .effort_metadata[0]
                    .efforts,
                None
            );
        }
    }
    #[tokio::test]
    async fn successful_text_prevents_stale_native_and_web_login_but_failed_text_does_not() {
        use crate::model::protocol::{TextMessage, TextRole};
        let f = crate::claude_code::fixture::Fixture::new();
        let environment = "00000000-0000-4000-8000-000000000001";
        let request = TextRequest {
            model: "claude-fixture-v1".into(),
            effort: None,
            max_output_tokens: 2048,
            messages: vec![TextMessage {
                role: TextRole::User,
                text: "OK".into(),
                phase: None,
            }],
        };
        let cancel = CancellationToken::new();
        let (tx, _rx) = mpsc::channel(16);
        f.behavior("nonzero");
        assert!(
            f.profile
                .stream_text(environment, &request, &tx, &cancel)
                .await
                .is_err()
        );
        assert!(f.profile.login_allowed(environment).is_ok());
        f.behavior("ok");
        let (tx, _rx) = mpsc::channel(16);
        f.profile
            .stream_text(environment, &request, &tx, &cancel)
            .await
            .unwrap();
        assert!(f.profile.login_allowed(environment).is_err());
        let before = f.launches();
        assert!(
            f.profile
                .login_with_cancel(environment, &cancel)
                .await
                .is_err()
        );
        let (events, _output) = mpsc::channel(1);
        let (_codes, mut input) = mpsc::channel(1);
        assert!(
            f.profile
                .login_web(environment, &events, &mut input, &cancel)
                .await
                .is_err()
        );
        assert_eq!(
            f.launches(),
            before,
            "stale login metadata cannot launch after verified inference"
        );
    }
    #[test]
    fn credit_default_cannot_enter_automatic_probes_and_standard_aliases_are_unique() {
        let response = json!({"type":"control_response","response":{"subtype":"success","request_id":"r","response":{"account":{"subscriptionType":"pro"},"models":[
            {"value":"default","resolvedModel":"claude-fable-synthetic"},
            {"value":"opus","resolvedModel":"claude-opus-synthetic"},
            {"value":"sonnet","resolvedModel":"claude-sonnet-synthetic"},
            {"value":"haiku","resolvedModel":"claude-haiku-synthetic"},
            {"value":"fable","resolvedModel":"claude-fable-synthetic"}]}}});
        assert_eq!(
            catalog(&response, "r").unwrap().models,
            [
                "claude-opus-synthetic",
                "claude-sonnet-synthetic",
                "claude-haiku-synthetic"
            ]
        );
        let mut response = response;
        response["response"]["response"]["models"][0]["resolvedModel"] =
            json!("claude-opus-synthetic");
        assert_eq!(catalog(&response, "r").unwrap().models.len(), 3);
    }
    #[test]
    fn catalog_never_promotes_aliases_or_account_identity_to_eligibility() {
        let response = |account| json!({"type":"control_response","response":{"subtype":"success","request_id":"r","response":{"account":account,"models":[{"value":"sonnet","resolvedModel":"claude-sonnet-synthetic"},{"value":"default","resolvedModel":"claude-opus-synthetic"},{"value":"opus","resolvedModel":"claude-opus-synthetic"}]}}});
        let parsed = catalog(
            &response(json!({"subscriptionType":"pro","apiProvider":"firstParty","email":"synthetic@example.test"})),
            "r",
        )
        .unwrap();
        assert!(!parsed.login_required);
        assert_eq!(
            parsed.models,
            ["claude-opus-synthetic", "claude-sonnet-synthetic"]
        );
        assert!(catalog(&response(json!({})), "r").unwrap().login_required);
        assert!(
            catalog(
                &response(json!({"subscriptionType":"pro","apiProvider":"bedrock"})),
                "r"
            )
            .unwrap()
            .login_required
        );
        assert!(catalog(&response(json!({})), "other").is_err());
    }
}
