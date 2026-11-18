//! Fixed official /usage command only. Free-form output and credentials never escape.
use super::*;
use serde_json::{Value, json};

#[derive(Debug, Serialize)]
pub struct UsageLimit {
    pub label: String,
    pub remaining_percent: Option<f64>,
    pub resets_at: Option<String>,
    pub scope: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct AccountUsage {
    pub status: &'static str,
    pub limits: Vec<UsageLimit>,
}
impl AccountUsage {
    pub fn unavailable() -> Self {
        Self {
            status: "unavailable",
            limits: vec![],
        }
    }
}
fn label(value: &Value, max: usize) -> Result<String, TransportError> {
    let s = value.as_str().ok_or_else(err)?;
    if s.is_empty()
        || s.len() > max
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" _-()./".contains(&b))
    {
        return Err(err());
    }
    Ok(s.to_owned())
}
fn report(value: &Value) -> Result<AccountUsage, TransportError> {
    let Some(rows) = value
        .get("rate_limits")
        .and_then(|v| v.get("limits"))
        .filter(|v| !v.is_null())
    else {
        return Ok(AccountUsage::unavailable());
    };
    let rows = rows.as_array().ok_or_else(err)?;
    if rows.len() > 32 {
        return Err(TransportError::LimitExceeded);
    }
    let mut limits = Vec::new();
    for row in rows {
        let kind = label(&row["kind"], 64)?;
        let group = label(&row["group"], 64)?;
        let used = row["percent"]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
            .ok_or_else(err)?;
        let resets_at = match row.get("resets_at") {
            Some(Value::Null) => None,
            Some(Value::String(s)) if s.len() <= 64 => Some(
                chrono::DateTime::parse_from_rfc3339(s)
                    .map_err(|_| err())?
                    .to_rfc3339(),
            ),
            _ => return Err(err()),
        };
        let mut scopes = Vec::new();
        if let Some(scope) = row.get("scope").filter(|v| !v.is_null()) {
            if !scope.is_object() {
                return Err(err());
            }
            for field in ["model", "surface"] {
                if let Some(v) = scope.get(field).filter(|v| !v.is_null()) {
                    scopes.push(label(&v["display_name"], 100)?);
                }
            }
        }
        limits.push(UsageLimit {
            label: if group == kind {
                kind
            } else {
                format!("{group} / {kind}")
            },
            remaining_percent: Some(100.0 - used),
            resets_at,
            scope: if scopes.is_empty() {
                None
            } else {
                Some(scopes.join(" / "))
            },
        });
    }
    Ok(AccountUsage {
        status: "available",
        limits,
    })
}
impl Profile {
    /// Initialize verifies subscription authority, then send exactly /usage.
    /// This official local command cannot receive arbitrary user input or fall
    /// back to an inference request, an API key, or parsing display text.
    pub async fn account_usage(
        &self,
        environment: &str,
        cancel: &CancellationToken,
    ) -> Result<AccountUsage, TransportError> {
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
        let user = uuid::Uuid::new_v4().to_string();
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
            let mut stdin = Some(child.stdin.take().ok_or_else(err)?);
            let initialize = json!({"type":"control_request","request_id":request_id,"request":{"subtype":"initialize"}}).to_string() + "\n";
            stdin
                .as_mut()
                .ok_or_else(err)?
                .write_all(initialize.as_bytes())
                .await
                .map_err(|_| TransportError::Transport)?;
            let mut stdout = BufReader::new(child.stdout.take().ok_or_else(err)?);
            let (mut initialized, mut found, mut completed, mut total) = (false, None, false, 0);
            let (mut session_initialized, mut lifecycle_done, mut lifecycle_events) =
                (false, false, 0);
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
                if v["type"] == "command_lifecycle" {
                    lifecycle_events += 1;
                    if !initialized || lifecycle_events > 3 {
                        return Err(err());
                    }
                    protocol::command_lifecycle(
                        &v,
                        &session,
                        &user,
                        session_initialized,
                        completed,
                        &mut lifecycle_done,
                    )?;
                    continue;
                }
                if completed {
                    return Err(err());
                }
                match v["type"].as_str() {
                    Some("control_response") if !initialized => {
                        let catalog = discovery::catalog(&v, &request_id)?;
                        initialized = true;
                        if catalog.login_required {
                            return Ok(AccountUsage::unavailable());
                        }
                        let command = json!({"type":"user","uuid":user,"session_id":session,"parent_tool_use_id":null,"message":{"role":"user","content":[{"type":"text","text":"/usage"}]}}).to_string() + "\n";
                        let mut input = stdin.take().ok_or_else(err)?;
                        input
                            .write_all(command.as_bytes())
                            .await
                            .map_err(|_| TransportError::Transport)?;
                        input
                            .shutdown()
                            .await
                            .map_err(|_| TransportError::Transport)?;
                    }
                    Some("system")
                        if matches!(v["subtype"].as_str(), Some("init" | "ui_invalidate")) =>
                    {
                        if v["subtype"] == "init" && v["session_id"] != session {
                            return Err(err());
                        }
                        if v["subtype"] == "init" {
                            if session_initialized {
                                return Err(err());
                            }
                            session_initialized = true;
                        }
                    }
                    Some("assistant") if initialized && found.is_none() && !completed => {
                        if v["session_id"] != session || !v["parent_tool_use_id"].is_null() {
                            return Err(err());
                        }
                        // Missing structured support is explicitly unavailable.
                        found = Some(match v.get("usage_report") {
                            Some(v) => report(v)?,
                            None => AccountUsage::unavailable(),
                        });
                    }
                    Some("result") if initialized && found.is_some() && !completed => {
                        if v["session_id"] != session
                            || v["subtype"] != "success"
                            || v["is_error"] != false
                        {
                            return Err(err());
                        }
                        completed = true;
                    }
                    _ => return Err(err()),
                }
            }
            if !child
                .wait()
                .await
                .map_err(|_| TransportError::Transport)?
                .success()
                || !completed
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
    fn usage_only_exposes_bounded_structured_rows() {
        let make = |rows| json!({"session":{"raw":"secret"},"rate_limits":{"limits":rows,"extra_usage":{"raw":"secret"}}});
        let row = json!({"kind":"weekly_scoped","group":"weekly","percent":12.5,"resets_at":"2026-11-18T20:00:00Z","scope":{"model":{"display_name":"Opus"},"surface":{"display_name":"Claude Code"}}});
        let parsed = report(&make(json!([row.clone()]))).unwrap();
        assert_eq!(parsed.limits[0].remaining_percent, Some(87.5));
        assert_eq!(
            parsed.limits[0].scope.as_deref(),
            Some("Opus / Claude Code")
        );
        assert!(!serde_json::to_string(&parsed).unwrap().contains("secret"));
        assert_eq!(report(&make(json!([]))).unwrap().status, "available");
        assert_eq!(report(&make(Value::Null)).unwrap().status, "unavailable");
        for (field, bad) in [
            ("percent", json!(101)),
            ("percent", json!(-1)),
            ("kind", json!("bad\nlabel")),
            ("group", json!("x".repeat(65))),
            ("resets_at", json!("not-a-timestamp")),
        ] {
            let mut invalid = row.clone();
            invalid[field] = bad;
            assert!(report(&make(json!([invalid]))).is_err(), "{field}");
        }
        assert!(report(&make(json!(vec![row; 33]))).is_err());
    }
    #[tokio::test]
    async fn fixed_usage_command_uses_sandbox_and_rejects_fallback_output() {
        let f = crate::claude_code::fixture::Fixture::new();
        let env = "00000000-0000-4000-8000-000000000001";
        let usage = f
            .profile
            .account_usage(env, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(usage.status, "available");
        assert_eq!(usage.limits.len(), 2);
        assert_eq!(usage.limits[0].remaining_percent, Some(75.0));
        let serialized = serde_json::to_string(&usage).unwrap();
        assert!(!serialized.contains("never-expose"));
        assert_eq!(
            std::fs::read_to_string(f.directory.path().join("usage-command")).unwrap(),
            "/usage"
        );
        for behavior in ["usage-null", "usage-unsupported", "logged-out"] {
            f.behavior(behavior);
            assert_eq!(
                f.profile
                    .account_usage(env, &CancellationToken::new())
                    .await
                    .unwrap()
                    .status,
                "unavailable"
            );
        }
        f.behavior("usage-empty");
        let empty = f
            .profile
            .account_usage(env, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(empty.status, "available");
        assert!(empty.limits.is_empty());
        for behavior in [
            "usage-malformed",
            "usage-nonzero",
            "usage-wrong-session",
            "usage-wrong-command",
            "usage-duplicate-ack",
            "usage-after-result",
        ] {
            f.behavior(behavior);
            assert!(
                f.profile
                    .account_usage(env, &CancellationToken::new())
                    .await
                    .is_err()
            );
        }
    }
}
