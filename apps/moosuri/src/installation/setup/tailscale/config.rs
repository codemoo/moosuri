use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Route {
    Empty,
    Matching,
}
pub(super) fn host(origin: &str) -> Result<String> {
    let url = url::Url::parse(origin).map_err(|_| "tailscale_hostname_invalid")?;
    let name = url.host_str().ok_or("tailscale_hostname_invalid")?;
    if url.scheme() != "https"
        || url.origin().ascii_serialization() != origin
        || url.port().is_some()
        || !name.ends_with(".ts.net")
        || name.len() > 253
        || name.split('.').any(|p| {
            p.is_empty()
                || p.starts_with('-')
                || p.ends_with('-')
                || !p
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err("tailscale_hostname_invalid".into());
    }
    Ok(name.into())
}
pub(super) fn node_origin(status: &Value) -> Result<String> {
    if status["BackendState"] != "Running" {
        return Err("tailscale_not_connected".into());
    }
    let dns = status["Self"]["DNSName"]
        .as_str()
        .ok_or("tailscale_hostname_invalid")?
        .trim_end_matches('.');
    let origin = format!("https://{dns}");
    host(&origin)?;
    if !status["CertDomains"]
        .as_array()
        .is_some_and(|v| v.iter().any(|d| d.as_str() == Some(dns)))
    {
        return Err("tailscale_https_required".into());
    }
    Ok(origin)
}
fn map<'a>(value: &'a Value, key: &str) -> Result<Option<&'a serde_json::Map<String, Value>>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(m)) => Ok(Some(m)),
        _ => Err("tailscale_unsupported_configuration".into()),
    }
}
fn has_port(config: &Value) -> Result<bool> {
    Ok(map(config, "TCP")?.is_some_and(|m| m.contains_key("443"))
        || map(config, "Web")?.is_some_and(|m| m.keys().any(|h| h.ends_with(":443"))))
}
pub(super) fn route(value: &Value, origin: &str, port: u16) -> Result<Route> {
    if !value.is_object() {
        return Err("tailscale_unsupported_configuration".into());
    }
    if map(value, "AllowFunnel")?.is_some_and(|m| {
        m.iter()
            .any(|(h, v)| h.ends_with(":443") && v != &Value::Bool(false))
    }) {
        return Err("tailscale_funnel_conflict_preserved".into());
    }
    if let Some(foreground) = map(value, "Foreground")? {
        for f in foreground.values() {
            if !f.is_object() || has_port(f)? {
                return Err("tailscale_existing_route_preserved".into());
            }
        }
    }
    let tcp = map(value, "TCP")?.and_then(|m| m.get("443"));
    let web = map(value, "Web")?;
    let hp = format!("{}:443", host(origin)?);
    let relevant: Vec<_> = web
        .into_iter()
        .flat_map(|m| m.iter())
        .filter(|(h, _)| h.ends_with(":443"))
        .collect();
    if tcp.is_none() && relevant.is_empty() {
        return Ok(Route::Empty);
    }
    let expected = json!({"Handlers":{"/":{"Proxy":format!("http://127.0.0.1:{port}")}}});
    if tcp == Some(&json!({"HTTPS":true}))
        && relevant.len() == 1
        && relevant[0].0 == &hp
        && relevant[0].1 == &expected
    {
        return Ok(Route::Matching);
    }
    Err("tailscale_existing_route_preserved".into())
}
pub(super) fn changed(value: &Value, origin: &str, port: u16, connect: bool) -> Result<Value> {
    route(value, origin, port)?;
    let mut result = value.clone();
    let root = result
        .as_object_mut()
        .ok_or("tailscale_unsupported_configuration")?;
    for key in ["TCP", "Web"] {
        if root.get(key).is_none_or(Value::is_null) {
            root.insert(key.into(), json!({}));
        }
    }
    let hp = format!("{}:443", host(origin)?);
    if connect {
        root.get_mut("TCP")
            .and_then(Value::as_object_mut)
            .unwrap()
            .insert("443".into(), json!({"HTTPS":true}));
        root.get_mut("Web")
            .and_then(Value::as_object_mut)
            .unwrap()
            .insert(
                hp,
                json!({"Handlers":{"/":{"Proxy":format!("http://127.0.0.1:{port}")}}}),
            );
    } else {
        root.get_mut("TCP")
            .and_then(Value::as_object_mut)
            .unwrap()
            .remove("443");
        root.get_mut("Web")
            .and_then(Value::as_object_mut)
            .unwrap()
            .remove(&hp);
    }
    Ok(result)
}

pub(super) fn node_id(status: &Value) -> Result<String> {
    let id = status["Self"]["ID"]
        .as_str()
        .filter(|v| {
            !v.is_empty() && v.len() <= 128 && v.is_ascii() && !v.chars().any(char::is_control)
        })
        .ok_or("tailscale_not_connected")?;
    Ok(id.into())
}
