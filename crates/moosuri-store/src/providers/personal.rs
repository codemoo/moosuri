//! Private compatible connections: immutable ownership and destination enrollment.
use super::*;
use moosuri_core::ModelSource;

pub(super) fn owner(c: &Connection, id: &str) -> Result<Option<(String, String)>> {
    Ok(c.query_row(
        "SELECT tenant_id,account_id FROM personal_compatible_providers WHERE provider_id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?)
}
pub(super) fn require_owner(c: &Connection, t: &str, a: &str, id: &str) -> Result<()> {
    if owner(c, id)?.as_ref() != Some(&(t.to_owned(), a.to_owned())) {
        return Err(StoreError::NotFound);
    }
    Ok(())
}
/// Public HTTPS requires server-resolved, pinned, exclusively public addresses.
/// Local destinations require exact current administrator enrollment. Only a
/// trusted administrator may enroll their own fixed documented daemon endpoint.
pub(super) fn personal_config(
    c: &Connection,
    t: &str,
    a: &str,
    config: &ProviderConfig,
) -> Result<()> {
    if config.kind != ProviderKind::Compatible || config.allowed_ips.is_empty() {
        return Err(StoreError::InvalidInput);
    }
    let u = url::Url::parse(config.base_url.as_deref().ok_or(StoreError::InvalidInput)?)
        .map_err(|_| StoreError::InvalidInput)?;
    let public = config.allowed_ips.iter().all(|v| {
        v.parse::<IpAddr>()
            .is_ok_and(moosuri_connectors::model::public_model_address)
    });
    if public && u.scheme() == "https" {
        return Ok(());
    }
    let enrolled:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM providers p JOIN provider_revisions r ON r.provider_id=p.id AND r.revision=p.current_revision WHERE r.enabled=1 AND r.config=?1 AND NOT EXISTS(SELECT 1 FROM personal_compatible_providers o WHERE o.provider_id=p.id))",[config_bytes(config)?],|r|r.get(0))?;
    if enrolled {
        return Ok(());
    }
    let admin:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE tenant_id=?1 AND id=?2 AND role='admin' AND status='active' AND deleted=0)",params![t,a],|r|r.get(0))?;
    let fixed = matches!(
        config.base_url.as_deref(),
        Some("http://127.0.0.1:11434/v1" | "http://127.0.0.1:1234/v1")
    ) && config.allowed_ips == ["127.0.0.1"];
    if admin && fixed {
        Ok(())
    } else {
        Err(StoreError::Forbidden)
    }
}

pub(super) fn authorized_revision(
    c: &Connection,
    u: &UserContext,
    id: &str,
    revision: i64,
    dispatch: bool,
) -> Result<Revision> {
    let current = validate(c, u, true, true)?;
    if owner(c, id)?.is_some() {
        require_owner(c, &current.tenant.0, &current.account.id, id)?;
        let row = require_binding_metadata(c, id, revision)?;
        personal_config(c, &current.tenant.0, &current.account.id, &row.config)?;
        if dispatch {
            crate::recovery_budget::require_dispatch_identity(c)?;
        }
        Ok(row)
    } else {
        if dispatch {
            require_binding(c, id, revision)
        } else {
            require_binding_metadata(c, id, revision)
        }
    }
}
pub(super) fn personal_choices_for(
    c: &Connection,
    t: &str,
    a: &str,
    id: Option<&str>,
) -> Result<Vec<ModelChoice>> {
    let rows=c.prepare("SELECT r.provider_id,r.revision,r.name,r.config,m.model,m.verified_at,m.function_tools FROM personal_compatible_providers o JOIN providers p ON p.id=o.provider_id JOIN provider_revisions r ON r.provider_id=p.id AND r.revision=p.current_revision JOIN provider_model_probes m ON m.provider_id=r.provider_id AND m.revision=r.revision WHERE o.tenant_id=?1 AND o.account_id=?2 AND r.enabled=1 AND (?3 IS NULL OR p.id=?3) ORDER BY r.name,m.model")?.query_map(params![t,a,id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,bool>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .filter_map(|(id, rev, name, config, model, at, tools)| {
            let config: ProviderConfig = match serde_json::from_str(&config) {
                Ok(c) => c,
                Err(_) => return Some(Err(StoreError::Corrupt)),
            };
            if personal_config(c, t, a, &config).is_err() {
                return None;
            }
            Some(Ok(ModelChoice {
                source: ModelSource::PersonalCompatible,
                provider_id: id,
                provider_revision: rev,
                provider_name: name,
                kind: ProviderKind::Compatible,
                model,
                effort: None,
                verified_at: at,
                function_tools: tools,
            }))
        })
        .collect()
}
pub(super) fn personal_choices(c: &Connection, u: &UserContext) -> Result<Vec<ModelChoice>> {
    personal_choices_for(c, &u.tenant.0, &u.account.id, None)
}
pub(super) fn personal_verified(
    c: &Connection,
    t: &str,
    a: &str,
    s: &ModelSelection,
) -> Result<ModelChoice> {
    if s.source != ModelSource::PersonalCompatible {
        return Err(StoreError::Conflict);
    }
    require_owner(c, t, a, &s.provider_id)?;
    model_valid(&s.model, &s.effort, ProviderKind::Compatible)?;
    personal_choices_for(c, t, a, Some(&s.provider_id))?
        .into_iter()
        .find(|v| {
            v.provider_revision == s.provider_revision && v.model == s.model && v.effort == s.effort
        })
        .ok_or(StoreError::Conflict)
}
pub(super) fn verified_for_user(
    c: &Connection,
    u: &UserContext,
    s: &ModelSelection,
) -> Result<ModelChoice> {
    if owner(c, &s.provider_id)?.is_some() {
        let mut s = s.clone();
        s.source = ModelSource::PersonalCompatible;
        personal_verified(c, &u.tenant.0, &u.account.id, &s)
    } else {
        verified(c, s)
    }
}
pub(super) fn personal_binding(
    c: &Connection,
    key: &[u8; 32],
    t: &str,
    a: &str,
    choice: &ModelChoice,
) -> Result<ProviderBinding> {
    let current = personal_verified(
        c,
        t,
        a,
        &ModelSelection {
            source: choice.source,
            provider_id: choice.provider_id.clone(),
            provider_revision: choice.provider_revision,
            model: choice.model.clone(),
            effort: choice.effort.clone(),
        },
    )?;
    if choice.function_tools && !current.function_tools {
        return Err(StoreError::Conflict);
    }
    crate::recovery_budget::require_dispatch_identity(c)?;
    let row = require_binding_metadata(c, &choice.provider_id, choice.provider_revision)?;
    binding(key, row, choice.model.clone(), choice.effort.clone())
}
impl Store {
    pub async fn disconnect_personal_compatible(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<ProviderSummary> {
        let u = user.clone();
        let copy = id.clone();
        let config = self
            .call(move |db| {
                validate(&db.conn, &u, true, true)?;
                require_owner(&db.conn, &u.tenant.0, &u.account.id, &copy)?;
                load_revision(&db.conn, &copy)
            })
            .await?;
        self.update_personal_compatible(
            user,
            id,
            revision,
            ProviderChange {
                name: config.name,
                config: config.config,
                enabled: false,
                credential: CredentialChange::Clear,
            },
        )
        .await
    }
    pub async fn personal_compatible_providers(
        &self,
        user: &UserContext,
    ) -> Result<ProvidersResponse> {
        let u = user.clone();
        self.call(move|db|{
            validate(&db.conn,&u,true,true)?;
            let ids=db.conn.prepare("SELECT provider_id FROM personal_compatible_providers WHERE tenant_id=?1 AND account_id=?2 ORDER BY provider_id")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(ProvidersResponse{providers:ids.into_iter().map(|id|summary(&db.conn,load_revision(&db.conn,&id)?)).collect::<Result<_>>()?,default_selection:None})
        }).await
    }
    /// Destination policy admission before discovery IO; no credential is returned.
    pub async fn admit_personal_compatible_config(
        &self,
        user: &UserContext,
        base_url: String,
        ips: Vec<String>,
    ) -> Result<ProviderConfig> {
        let u = user.clone();
        self.call(move|db|{
            validate(&db.conn,&u,true,true)?;
            let mut config=ProviderConfig{kind:ProviderKind::Compatible,base_url:Some(base_url),allowed_ips:ips,router_providers:vec![],zero_data_retention:false};
            // Explicit administrator enrollment supplies the exact local address
            // set; member input can never choose a private allowlist.
            let enrolled:Option<String>=db.conn.query_row("SELECT r.config FROM providers p JOIN provider_revisions r ON r.provider_id=p.id AND r.revision=p.current_revision WHERE r.enabled=1 AND json_extract(r.config,'$.kind')='compatible' AND json_extract(r.config,'$.base_url')=?1 AND json_array_length(json_extract(r.config,'$.allowed_ips'))>0 AND NOT EXISTS(SELECT 1 FROM personal_compatible_providers o WHERE o.provider_id=p.id) LIMIT 1",[&config.base_url],|r|r.get(0)).optional()?;
            if let Some(v)=enrolled {config=serde_json::from_str(&v).map_err(|_|StoreError::Corrupt)?;}
            validate_change(&ProviderChange{name:"validation".into(),config:config.clone(),enabled:true,credential:CredentialChange::Clear})?;
            personal_config(&db.conn,&u.tenant.0,&u.account.id,&config)?;
            Ok(config)
        }).await
    }
}
