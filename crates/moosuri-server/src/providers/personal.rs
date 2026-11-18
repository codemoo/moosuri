//! Owner-facing compatible connections. The browser cannot supply an allowlist.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersonalInput {
    name: String,
    base_url: String,
    enabled: bool,
    credential: CredentialInput,
    expected_revision: Option<i64>,
}
async fn config(s: &AppState, u: &UserContext, base: String) -> Result<ProviderConfig, HttpError> {
    let url = url::Url::parse(&base).map_err(|_| HttpError(StoreError::InvalidInput))?;
    if base.len() > 2048
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(StoreError::InvalidInput.into());
    }
    let ips = if matches!(
        base.as_str(),
        "http://127.0.0.1:11434/v1" | "http://127.0.0.1:1234/v1"
    ) {
        vec!["127.0.0.1".into()]
    } else {
        vec![]
    };
    // Admission before any IO allows exact administrator-enrolled local origins.
    if let Ok(c) = s
        .store
        .admit_personal_compatible_config(u, base.clone(), ips)
        .await
    {
        validate_config(&c)?;
        return Ok(c);
    }
    if url.scheme() != "https" {
        return Err(StoreError::InvalidInput.into());
    }
    let port = url
        .port_or_known_default()
        .ok_or(StoreError::InvalidInput)?;
    let mut ips = match url.host().ok_or(StoreError::InvalidInput)? {
        url::Host::Ipv4(ip) => vec![std::net::IpAddr::V4(ip)],
        url::Host::Ipv6(ip) => vec![std::net::IpAddr::V6(ip)],
        url::Host::Domain(host) => tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| HttpError(StoreError::Unavailable))?
        .map_err(|_| HttpError(StoreError::Unavailable))?
        .map(|s| s.ip())
        .collect(),
    };
    ips.sort();
    ips.dedup();
    if ips.is_empty()
        || ips.len() > 32
        || ips
            .iter()
            .any(|ip| !moosuri_connectors::model::public_model_address(*ip))
    {
        return Err(StoreError::InvalidInput.into());
    }
    let c = s
        .store
        .admit_personal_compatible_config(u, base, ips.into_iter().map(|v| v.to_string()).collect())
        .await?;
    validate_config(&c)?;
    Ok(c)
}
pub(crate) async fn list(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current_admin(&s, &h).await?;
    Ok(Json(s.store.personal_compatible_providers(&u).await?).into_response())
}
pub(crate) async fn create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<PersonalInput>,
) -> HttpResult {
    let u = current_admin(&s, &h).await?;
    if i.expected_revision.is_some() {
        return Err(StoreError::InvalidInput.into());
    }
    let config = config(&s, &u, i.base_url).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            s.store
                .create_personal_compatible(
                    &u,
                    ProviderChange {
                        name: i.name,
                        config,
                        enabled: i.enabled,
                        credential: i.credential.into(),
                    },
                )
                .await?,
        ),
    )
        .into_response())
}
pub(crate) async fn update(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<PersonalInput>,
) -> HttpResult {
    let u = current_admin(&s, &h).await?;
    let revision = i.expected_revision.ok_or(StoreError::InvalidInput)?;
    // Disabling the unchanged destination must remain possible while DNS or
    // local enrollment is unavailable. Store repeats ownership/revision checks.
    let existing = s
        .store
        .personal_compatible_providers(&u)
        .await?
        .providers
        .into_iter()
        .find(|v| v.id == id)
        .ok_or(StoreError::NotFound)?;
    let config = if !i.enabled && existing.config.base_url.as_deref() == Some(i.base_url.as_str()) {
        existing.config
    } else {
        config(&s, &u, i.base_url).await?
    };
    Ok(Json(
        s.store
            .update_personal_compatible(
                &u,
                id,
                revision,
                ProviderChange {
                    name: i.name,
                    config,
                    enabled: i.enabled,
                    credential: i.credential.into(),
                },
            )
            .await?,
    )
    .into_response())
}
pub(crate) async fn discover(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current_admin(&s, &h).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| HttpError(StoreError::Busy))?;
    let mut candidates = vec![];
    for (name, base) in [
        ("Ollama", "http://127.0.0.1:11434/v1"),
        ("LM Studio", "http://127.0.0.1:1234/v1"),
    ] {
        let Ok(config) = s
            .store
            .admit_personal_compatible_config(&u, base.into(), vec!["127.0.0.1".into()])
            .await
        else {
            continue;
        };
        let Ok(client) = moosuri_agent::model_client(&config, None, TextCapabilities::default())
        else {
            continue;
        };
        let cancel = CancellationToken::new();
        if let Ok(Ok(catalog)) =
            tokio::time::timeout(Duration::from_secs(2), client.list_models(&cancel)).await
        {
            // Recheck current role, enrollment and session before disclosing existence.
            s.store
                .admit_personal_compatible_config(&u, base.into(), vec!["127.0.0.1".into()])
                .await?;
            candidates.push(serde_json::json!({"name":name,"base_url":base,"models":catalog.models.into_iter().map(|v|serde_json::json!({"id":v.id,"name":v.name})).collect::<Vec<_>>()}));
        }
        cancel.cancel();
    }
    Ok(Json(serde_json::json!({"candidates":candidates})).into_response())
}

pub(crate) async fn disconnect(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<CatalogInput>,
) -> HttpResult {
    let u = current_admin(&s, &h).await?;
    Ok(Json(
        s.store
            .disconnect_personal_compatible(&u, id, i.revision)
            .await?,
    )
    .into_response())
}
