//! Official SDK initialize catalog only. No user turn and no credential reads.
use super::*;
use serde_json::{Value, json};

pub struct AccountModels {
    pub login_required: bool,
    pub models: Vec<String>,
}
fn valid_model(model: &str) -> bool {
    model.starts_with("claude-")
        && model.len() <= 200
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}
fn catalog(v: &Value, request_id: &str) -> Result<AccountModels, TransportError> {
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
    Ok(AccountModels {
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
                {
                    return Err(err());
                } else if v["subtype"] == "init" && v["session_id"] != session {
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
