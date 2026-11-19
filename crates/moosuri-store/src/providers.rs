//! Shared provider settings. Server-managed encryption is separate from the user vault.
use super::*;
mod personal;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use moosuri_core::{
    ModelChoice, ModelSelection, ModelSettings, ModelSource, ProviderConfig, ProviderKind,
    ProviderSummary, ProvidersResponse,
};
use personal::*;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::IpAddr,
};

pub enum CredentialChange {
    Keep,
    Replace(Zeroizing<String>),
    Clear,
}
pub struct ProviderChange {
    pub name: String,
    pub config: ProviderConfig,
    pub enabled: bool,
    pub credential: CredentialChange,
}
/// Private server credential carrier: intentionally no Debug or serialization.
pub struct ProviderBinding {
    pub claude_code: Option<crate::ClaudeExecutionRef>,
    pub subscription: Option<crate::SubscriptionRef>,
    pub id: String,
    pub revision: i64,
    pub config: ProviderConfig,
    pub model: String,
    pub effort: Option<String>,
    pub credential: Option<Zeroizing<String>>,
}
const KEY_AAD: &[u8] = b"moosuri/provider-key-check/v1";
const KEY_PLAIN: &[u8] = b"moosuri provider encryption key";
pub(super) fn encrypt(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0; 24];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| StoreError::Unavailable)?;
    let mut bytes = nonce.to_vec();
    bytes.extend(
        XChaCha20Poly1305::new(key.into())
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
            .map_err(|_| StoreError::Unavailable)?,
    );
    Ok(bytes)
}
pub(super) fn decrypt(key: &[u8; 32], cipher: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if cipher.len() < 40 {
        return Err(StoreError::Corrupt);
    }
    Ok(Zeroizing::new(
        XChaCha20Poly1305::new(key.into())
            .decrypt(
                XNonce::from_slice(&cipher[..24]),
                Payload {
                    msg: &cipher[24..],
                    aad,
                },
            )
            .map_err(|_| StoreError::Corrupt)?,
    ))
}
pub(super) fn open_key(conn: &Connection, dir: &Path) -> Result<Zeroizing<[u8; 32]>> {
    let check: Option<Vec<u8>> = conn
        .query_row(
            "SELECT key_check FROM provider_key_meta WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let path = dir.join("provider-secrets.key");
    let mut key = Zeroizing::new([0; 32]);
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            if !meta.is_file() || meta.len() != 32 {
                return Err(StoreError::Corrupt);
            }
            #[cfg(unix)]
            if meta.uid() != unsafe { libc::geteuid() }
                || meta.nlink() != 1
                || meta.permissions().mode() & 0o777 != 0o600
            {
                return Err(StoreError::InvalidInput);
            }
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
            let mut file = options.open(&path).map_err(|_| StoreError::Unavailable)?;
            let opened = file.metadata().map_err(|_| StoreError::Unavailable)?;
            #[cfg(unix)]
            if opened.ino() != meta.ino()
                || opened.dev() != meta.dev()
                || opened.nlink() != 1
                || opened.permissions().mode() & 0o777 != 0o600
            {
                return Err(StoreError::InvalidInput);
            }
            file.read_exact(key.as_mut())
                .map_err(|_| StoreError::Corrupt)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && check.is_none() => {
            // A DB marker is written only after durable key creation. Never recreate a marked key.
            OsRng
                .try_fill_bytes(key.as_mut())
                .map_err(|_| StoreError::Unavailable)?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            let mut file = options.open(&path).map_err(|_| StoreError::Unavailable)?;
            file.write_all(key.as_ref())
                .and_then(|_| file.sync_all())
                .map_err(|_| StoreError::Unavailable)?;
            File::open(dir)
                .and_then(|f| f.sync_all())
                .map_err(|_| StoreError::Unavailable)?;
        }
        Err(_) => return Err(StoreError::Unavailable),
    }
    if let Some(check) = check {
        if decrypt(&key, &check, KEY_AAD)?.as_slice() != KEY_PLAIN {
            return Err(StoreError::Corrupt);
        }
    } else {
        // An unmarked DB must not already contain ciphertext.
        if conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM provider_revisions WHERE credential IS NOT NULL) OR EXISTS(SELECT 1 FROM chatgpt_sessions WHERE cipher IS NOT NULL) OR EXISTS(SELECT 1 FROM chatgpt_binding_revisions WHERE identity IS NOT NULL)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(StoreError::Corrupt);
        }
        let check = encrypt(&key, KEY_PLAIN, KEY_AAD)?;
        conn.execute(
            "INSERT INTO provider_key_meta(singleton,key_check) VALUES (1,?1)",
            [check],
        )?;
    }
    Ok(key)
}
fn model_valid(model: &str, effort: &Option<String>, kind: ProviderKind) -> Result<()> {
    if model.is_empty()
        || model.len() > 256
        || model.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(StoreError::InvalidInput);
    }
    if let Some(e) = effort
        && (kind == ProviderKind::Compatible
            || e.is_empty()
            || e.len() > 32
            || !e
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'))
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn validate_change(change: &ProviderChange) -> Result<()> {
    name_valid(change.name.clone(), 200)?;
    let c = &change.config;
    if c.allowed_ips.len() > 32
        || c.allowed_ips.iter().any(|ip| ip.parse::<IpAddr>().is_err())
        || c.router_providers.len() > 16
        || c.router_providers.iter().any(|s| {
            s.is_empty()
                || s.len() > 256
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_./:".contains(&b))
        })
    {
        return Err(StoreError::InvalidInput);
    }
    if c.kind == ProviderKind::Compatible {
        let base = c.base_url.as_ref().ok_or(StoreError::InvalidInput)?;
        if base.len() > 2048 {
            return Err(StoreError::InvalidInput);
        }
        let u = url::Url::parse(base).map_err(|_| StoreError::InvalidInput)?;
        if !matches!(u.scheme(), "http" | "https")
            || u.host_str().is_none()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || !u
                .path()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_-".contains(&b))
            || !c.router_providers.is_empty()
            || c.zero_data_retention
        {
            return Err(StoreError::InvalidInput);
        }
    } else if c.base_url.is_some()
        || !c.allowed_ips.is_empty()
        || (c.kind == ProviderKind::OpenRouter && c.router_providers.is_empty())
        || (c.kind != ProviderKind::OpenRouter
            && (!c.router_providers.is_empty() || c.zero_data_retention))
    {
        return Err(StoreError::InvalidInput);
    }
    if let CredentialChange::Replace(secret) = &change.credential {
        credential_valid(secret)?;
    }
    Ok(())
}
fn credential_valid(secret: &str) -> Result<()> {
    if secret.is_empty() || secret.len() > 8192 || !secret.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
pub(super) struct Revision {
    id: String,
    revision: i64,
    name: String,
    config: ProviderConfig,
    enabled: bool,
    cipher: Option<Vec<u8>>,
}
fn load_revision(conn: &Connection, id: &str) -> Result<Revision> {
    if Uuid::parse_str(id).is_err() {
        return Err(StoreError::InvalidInput);
    }
    let row=conn.query_row("SELECT r.provider_id,r.revision,r.name,r.config,r.enabled,r.credential FROM providers p JOIN provider_revisions r ON r.provider_id=p.id AND r.revision=p.current_revision WHERE p.id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?,r.get::<_,Option<Vec<u8>>>(5)?))).optional()?.ok_or(StoreError::NotFound)?;
    Ok(Revision {
        id: row.0,
        revision: row.1,
        name: row.2,
        config: serde_json::from_str(&row.3).map_err(|_| StoreError::Corrupt)?,
        enabled: row.4,
        cipher: row.5,
    })
}
fn config_bytes(config: &ProviderConfig) -> Result<String> {
    serde_json::to_string(config).map_err(|_| StoreError::InvalidInput)
}
fn aad(id: &str, revision: i64, config: &ProviderConfig) -> Result<Vec<u8>> {
    serde_json::to_vec(&("moosuri/provider-credential/v1", id, revision, config))
        .map_err(|_| StoreError::InvalidInput)
}
fn credential(key: &[u8; 32], row: &Revision) -> Result<Option<Zeroizing<String>>> {
    row.cipher
        .as_ref()
        .map(|c| {
            let plain = decrypt(key, c, &aad(&row.id, row.revision, &row.config)?)?;
            let secret = Zeroizing::new(
                std::str::from_utf8(&plain)
                    .map_err(|_| StoreError::Corrupt)?
                    .to_owned(),
            );
            credential_valid(&secret).map_err(|_| StoreError::Corrupt)?;
            Ok(secret)
        })
        .transpose()
}
pub(super) fn require_binding(conn: &Connection, id: &str, revision: i64) -> Result<Revision> {
    crate::recovery_budget::require_dispatch_identity(conn)?;
    if owner(conn, id)?.is_some() {
        return Err(StoreError::Conflict);
    }
    require_binding_metadata(conn, id, revision)
}
fn require_binding_metadata(conn: &Connection, id: &str, revision: i64) -> Result<Revision> {
    let row = load_revision(conn, id)?;
    if row.revision != revision {
        return Err(StoreError::Conflict);
    }
    if !row.enabled {
        return Err(StoreError::Conflict);
    }
    Ok(row)
}
pub(super) fn binding(
    key: &[u8; 32],
    row: Revision,
    model: String,
    effort: Option<String>,
) -> Result<ProviderBinding> {
    model_valid(&model, &effort, row.config.kind)?;
    let credential = credential(key, &row)?;
    if row.config.kind != ProviderKind::Compatible && credential.is_none() {
        return Err(StoreError::Unavailable);
    }
    Ok(ProviderBinding {
        claude_code: None,
        subscription: None,
        id: row.id,
        revision: row.revision,
        config: row.config,
        model,
        effort,
        credential,
    })
}
fn choices(conn: &Connection, id: Option<&str>) -> Result<Vec<ModelChoice>> {
    let mut stmt=conn.prepare("SELECT r.provider_id,r.revision,r.name,r.config,m.model,m.effort,m.verified_at,m.function_tools FROM providers p JOIN provider_revisions r ON r.provider_id=p.id AND r.revision=p.current_revision JOIN provider_model_probes m ON m.provider_id=r.provider_id AND m.revision=r.revision WHERE r.enabled=1 AND NOT EXISTS(SELECT 1 FROM personal_compatible_providers o WHERE o.provider_id=p.id) AND (?1 IS NULL OR p.id=?1) ORDER BY r.name,m.model,m.effort")?;
    let rows = stmt.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, bool>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (
            provider_id,
            provider_revision,
            provider_name,
            config,
            model,
            effort,
            verified_at,
            function_tools,
        ) = row?;
        let config: ProviderConfig =
            serde_json::from_str(&config).map_err(|_| StoreError::Corrupt)?;
        Ok(ModelChoice {
            source: Default::default(),
            provider_id,
            provider_revision,
            provider_name,
            kind: config.kind,
            model,
            effort: if effort.is_empty() {
                None
            } else {
                Some(effort)
            },
            verified_at,
            function_tools,
        })
    })
    .collect()
}
fn selected(conn: &Connection, user: Option<&UserContext>) -> Result<Option<ModelSelection>> {
    let mut row = if let Some(user) = user {
        conn.query_row("SELECT provider_id,revision,model,effort FROM personal_model_selections WHERE tenant_id=?1 AND account_id=?2",params![user.tenant.0,user.account.id],selection_row).optional()?
    } else {
        conn.query_row("SELECT provider_id,revision,model,effort FROM installation_model_selection WHERE singleton=1",[],selection_row).optional()?
    };
    if let Some(selection) = &mut row {
        if owner(conn, &selection.provider_id)?.is_some() {
            selection.source = ModelSource::PersonalCompatible;
        }
    }
    Ok(row)
}
fn selection_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ModelSelection> {
    let effort: String = r.get(3)?;
    Ok(ModelSelection {
        source: ModelSource::Shared,
        provider_id: r.get(0)?,
        provider_revision: r.get(1)?,
        model: r.get(2)?,
        effort: if effort.is_empty() {
            None
        } else {
            Some(effort)
        },
    })
}
fn matches_selection(choice: &ModelChoice, selection: &ModelSelection) -> bool {
    choice.source == selection.source
        && choice.provider_id == selection.provider_id
        && choice.provider_revision == selection.provider_revision
        && choice.model == selection.model
        && choice.effort == selection.effort
}
fn verified(conn: &Connection, selection: &ModelSelection) -> Result<ModelChoice> {
    if selection.source != moosuri_core::ModelSource::Shared {
        return Err(StoreError::Conflict);
    }
    if owner(conn, &selection.provider_id)?.is_some() {
        return Err(StoreError::Conflict);
    }
    let row = load_revision(conn, &selection.provider_id).map_err(|error| {
        if error == StoreError::NotFound {
            StoreError::Conflict
        } else {
            error
        }
    })?;
    if row.revision != selection.provider_revision {
        return Err(StoreError::Conflict);
    }
    model_valid(&selection.model, &selection.effort, row.config.kind)?;
    choices(conn, Some(&selection.provider_id))?
        .into_iter()
        .find(|c| c.model == selection.model && c.effort == selection.effort)
        .ok_or(StoreError::Conflict)
}
fn admin_selection(conn: &Connection, user: &UserContext) -> Result<Option<ModelSelection>> {
    Ok(crate::claude_execution::selected(conn, user)?
        .or(subscriptions::selected(conn, user)?)
        .or(selected(conn, Some(user))?))
}
/// A stale configured selection returns an error. Absence permits the
/// installation's owner-only selection path.
pub(super) fn installation_choice(conn: &Connection) -> Result<Option<ModelChoice>> {
    selected(conn, None)?
        .map(|selection| verified(conn, &selection))
        .transpose()
}
fn selection_choice(
    conn: &Connection,
    user: &UserContext,
    selection: &ModelSelection,
) -> Result<ModelChoice> {
    match selection.source {
        ModelSource::Shared => verified(conn, selection),
        ModelSource::PersonalCompatible => {
            personal_verified(conn, &user.tenant.0, &user.account.id, selection)
        }
        ModelSource::PersonalClaudeCode => crate::claude_execution::verified(conn, user, selection),
        ModelSource::PersonalChatGpt => subscriptions::verified(conn, user, selection),
    }
}
fn effective_choice(conn: &Connection, user: &UserContext) -> Result<Option<ModelChoice>> {
    let installation = installation_choice(conn)?;
    if installation.is_some() || user.account.role == Role::Member {
        return Ok(installation);
    }
    admin_selection(conn, user)?
        .map(|selection| selection_choice(conn, user, &selection))
        .transpose()
}
fn available_choice(conn: &Connection, user: &UserContext) -> Result<Option<ModelChoice>> {
    match effective_choice(conn, user) {
        Err(StoreError::Conflict | StoreError::NotFound) => Ok(None),
        outcome => outcome,
    }
}
fn settings(conn: &Connection, user: &UserContext) -> Result<ModelSettings> {
    if user.account.role == Role::Member {
        return Ok(ModelSettings {
            selection: None,
            effective: available_choice(conn, user)?,
            choices: Vec::new(),
            installation_default: None,
        });
    }
    let selection = admin_selection(conn, user)?;
    let installation_default = selected(conn, None)?;
    let effective = installation_default.as_ref().or(selection.as_ref());
    let mut all = choices(conn, None)?;
    all.extend(personal_choices(conn, user)?);
    all.extend(subscriptions::choices(conn, user)?);
    all.extend(crate::claude_execution::choices(conn, user)?);
    let effective = effective.and_then(|s| all.iter().find(|c| matches_selection(c, s)).cloned());
    Ok(ModelSettings {
        selection,
        effective,
        choices: all,
        installation_default,
    })
}
fn summary(conn: &Connection, row: Revision) -> Result<ProviderSummary> {
    let default = selected(conn, None)?;
    Ok(ProviderSummary {
        id: row.id.clone(),
        name: row.name,
        revision: row.revision,
        config: row.config,
        enabled: row.enabled,
        has_credential: row.cipher.is_some(),
        choices: if let Some((tenant, account)) = owner(conn, &row.id)? {
            personal_choices_for(conn, &tenant, &account, Some(&row.id))?
        } else {
            choices(conn, Some(&row.id))?
        },
        is_default: default.is_some_and(|s| s.provider_id == row.id),
    })
}
fn list(conn: &Connection) -> Result<ProvidersResponse> {
    let ids = conn
        .prepare("SELECT id FROM providers WHERE NOT EXISTS(SELECT 1 FROM personal_compatible_providers o WHERE o.provider_id=providers.id) ORDER BY id")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ProvidersResponse {
        providers: ids
            .into_iter()
            .map(|id| summary(conn, load_revision(conn, &id)?))
            .collect::<Result<_>>()?,
        default_selection: selected(conn, None)?,
    })
}
impl Store {
    pub async fn create_provider(
        &self,
        user: &UserContext,
        change: ProviderChange,
    ) -> Result<ProviderSummary> {
        self.create_provider_scoped(user, change, false).await
    }
    pub async fn create_personal_compatible(
        &self,
        user: &UserContext,
        change: ProviderChange,
    ) -> Result<ProviderSummary> {
        self.create_provider_scoped(user, change, true).await
    }
    async fn create_provider_scoped(
        &self,
        user: &UserContext,
        change: ProviderChange,
        personal: bool,
    ) -> Result<ProviderSummary> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn,&user,true,true)?; validate_change(&change)?;
            if personal { personal_config(&db.conn,&user.tenant.0,&user.account.id,&change.config)?; if db.conn.query_row("SELECT count(*) FROM personal_compatible_providers WHERE tenant_id=?1 AND account_id=?2",params![user.tenant.0,user.account.id],|r|r.get::<_,i64>(0))?>=16{return Err(StoreError::Conflict)} }
            if matches!(change.credential,CredentialChange::Keep) { return Err(StoreError::InvalidInput); }
            if db.conn.query_row("SELECT count(*) FROM providers WHERE NOT EXISTS(SELECT 1 FROM personal_compatible_providers o WHERE o.provider_id=providers.id)",[],|r|r.get::<_,i64>(0))? >= 64 && !personal { return Err(StoreError::Conflict); }
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let id=Uuid::new_v4().to_string(); let revision=1;
            let cipher=match change.credential {CredentialChange::Replace(secret)=>Some(encrypt(&db.provider_key,secret.as_bytes(),&aad(&id,revision,&change.config)?)?),_=>None};
            if change.enabled && change.config.kind!=ProviderKind::Compatible && cipher.is_none() {return Err(StoreError::InvalidInput);}
            tx.execute("INSERT INTO providers(id,current_revision) VALUES (?1,1)",[&id])?;
            tx.execute("INSERT INTO provider_revisions(provider_id,revision,name,config,enabled,credential) VALUES (?1,1,?2,?3,?4,?5)",params![id,name_valid(change.name,200)?,config_bytes(&change.config)?,change.enabled,cipher])?;
            if personal { tx.execute("INSERT INTO personal_compatible_providers VALUES(?1,?2,?3)",params![id,user.tenant.0,user.account.id])?; }
            let result=summary(&tx,load_revision(&tx,&id)?)?; tx.commit()?; Ok(result)
        }).await
    }
    pub async fn update_provider(
        &self,
        user: &UserContext,
        id: String,
        expected_revision: i64,
        change: ProviderChange,
    ) -> Result<ProviderSummary> {
        self.update_provider_scoped(user, id, expected_revision, change, false)
            .await
    }
    pub async fn update_personal_compatible(
        &self,
        user: &UserContext,
        id: String,
        expected_revision: i64,
        change: ProviderChange,
    ) -> Result<ProviderSummary> {
        self.update_provider_scoped(user, id, expected_revision, change, true)
            .await
    }
    async fn update_provider_scoped(
        &self,
        user: &UserContext,
        id: String,
        expected_revision: i64,
        change: ProviderChange,
        personal: bool,
    ) -> Result<ProviderSummary> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn,&user,true,true)?; validate_change(&change)?;
            if personal { require_owner(&db.conn,&user.tenant.0,&user.account.id,&id)?; if change.enabled || load_revision(&db.conn,&id)?.config!=change.config {personal_config(&db.conn,&user.tenant.0,&user.account.id,&change.config)?;} }
            else if owner(&db.conn,&id)?.is_some() {return Err(StoreError::NotFound)}
            let old=load_revision(&db.conn,&id)?;
            if old.revision!=expected_revision { return Err(StoreError::Conflict); }
            if matches!(change.credential,CredentialChange::Keep) && old.cipher.is_some() && old.config!=change.config { return Err(StoreError::InvalidInput); }
            let secret=match change.credential {CredentialChange::Keep=>credential(&db.provider_key,&old)?,CredentialChange::Replace(secret)=>Some(secret),CredentialChange::Clear=>None};
            if change.enabled && change.config.kind!=ProviderKind::Compatible && secret.is_none() {return Err(StoreError::InvalidInput);}
            let revision=old.revision.checked_add(1).ok_or(StoreError::Corrupt)?;
            let cipher=secret.map(|s|encrypt(&db.provider_key,s.as_bytes(),&aad(&id,revision,&change.config)?)).transpose()?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("INSERT INTO provider_revisions(provider_id,revision,name,config,enabled,credential) VALUES (?1,?2,?3,?4,?5,?6)",params![id,revision,name_valid(change.name,200)?,config_bytes(&change.config)?,change.enabled,cipher])?;
            tx.execute("UPDATE providers SET current_revision=?1 WHERE id=?2 AND current_revision=?3",params![revision,id,expected_revision])?;
            tx.execute("UPDATE provider_revisions SET credential=NULL WHERE provider_id=?1 AND revision<>?2 AND credential IS NOT NULL",params![id,revision])?;
            if personal {
                crate::calendar::agent::invalidate_personal_model(
                    &tx, &user.tenant.0, &user.account.id, &id, expected_revision, moosuri_core::ModelSource::PersonalCompatible,
                )?;
            }
            crate::missions::connected::withdraw_provider(&tx,&id)?;
            tx.execute("UPDATE missions SET grant_enabled=0,candidate=NULL,round_text=NULL,plan='[]',state='quarantined' WHERE state!='deleted' AND EXISTS(SELECT 1 FROM chat_runs r WHERE r.tenant_id=missions.tenant_id AND r.id=missions.run_id AND json_extract(r.model,'$.provider_id')=?1)",[&id])?;
            let result=summary(&tx,load_revision(&tx,&id)?)?; tx.commit()?; Ok(result)
        }).await
    }
    pub async fn list_providers(&self, user: &UserContext) -> Result<ProvidersResponse> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, true)?;
            list(&db.conn)
        })
        .await
    }
    pub async fn model_settings(&self, user: &UserContext) -> Result<ModelSettings> {
        let user = user.clone();
        self.call(move |db| {
            let current = validate(&db.conn, &user, true, false)?;
            settings(&db.conn, &current)
        })
        .await
    }
    pub async fn set_model_preference(
        &self,
        user: &UserContext,
        selection: Option<ModelSelection>,
    ) -> Result<ModelSettings> {
        let user = user.clone();
        self.call(move|db|{
            let current=validate(&db.conn,&user,true,true)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(s)=selection {
                if s.source==moosuri_core::ModelSource::PersonalClaudeCode {
                    let choice=crate::claude_execution::verified(&tx,&current,&s)?;
                    tx.execute("INSERT INTO personal_claude_selections(tenant_id,account_id,binding_id,revision,model,effort) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(tenant_id,account_id) DO UPDATE SET binding_id=excluded.binding_id,revision=excluded.revision,model=excluded.model,effort=excluded.effort",params![current.tenant.0,current.account.id,s.provider_id,choice.provider_revision,s.model,s.effort])?;
                    tx.execute("DELETE FROM personal_model_selections WHERE tenant_id=?1 AND account_id=?2",params![current.tenant.0,current.account.id])?;
                    tx.execute("DELETE FROM personal_subscription_selections WHERE tenant_id=?1 AND account_id=?2",params![current.tenant.0,current.account.id])?;
                    let result=settings(&tx,&current)?;tx.commit()?;return Ok(result);
                }
                if s.source==moosuri_core::ModelSource::PersonalChatGpt {
                    let choice=subscriptions::verified(&tx,&current,&s)?;
                    tx.execute("DELETE FROM personal_model_selections WHERE tenant_id=?1 AND account_id=?2",params![current.tenant.0,current.account.id])?;
                    tx.execute("INSERT INTO personal_subscription_selections VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(tenant_id,account_id) DO UPDATE SET binding_id=excluded.binding_id,revision=excluded.revision,model=excluded.model,effort=excluded.effort",params![current.tenant.0,current.account.id,s.provider_id,choice.provider_revision,s.model,s.effort.unwrap_or_default()])?;
                    let result=settings(&tx,&current)?;tx.commit()?;return Ok(result);
                }
                let choice=if s.source==moosuri_core::ModelSource::PersonalCompatible {personal_verified(&tx,&current.tenant.0,&current.account.id,&s)?}else{verified(&tx,&s)?};
                tx.execute("INSERT INTO personal_model_selections(tenant_id,account_id,provider_id,revision,model,effort) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(tenant_id,account_id) DO UPDATE SET provider_id=excluded.provider_id,revision=excluded.revision,model=excluded.model,effort=excluded.effort",params![current.tenant.0,current.account.id,s.provider_id,choice.provider_revision,s.model,s.effort.unwrap_or_default()])?;
            } else {tx.execute("DELETE FROM personal_claude_selections WHERE tenant_id=?1 AND account_id=?2",params![current.tenant.0,current.account.id])?;tx.execute("DELETE FROM personal_model_selections WHERE tenant_id=?1 AND account_id=?2",params![current.tenant.0,current.account.id])?;tx.execute("DELETE FROM personal_subscription_selections WHERE tenant_id=?1 AND account_id=?2",params![current.tenant.0,current.account.id])?;}
            let result=settings(&tx,&current)?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn set_default_model(
        &self,
        user: &UserContext,
        selection: Option<ModelSelection>,
    ) -> Result<ProvidersResponse> {
        let user = user.clone();
        self.call(move|db|{
            validate(&db.conn,&user,true,true)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(s)=selection {
                let choice=verified(&tx,&s)?;
                tx.execute("INSERT INTO installation_model_selection(singleton,provider_id,revision,model,effort) VALUES (1,?1,?2,?3,?4) ON CONFLICT(singleton) DO UPDATE SET provider_id=excluded.provider_id,revision=excluded.revision,model=excluded.model,effort=excluded.effort",params![s.provider_id,choice.provider_revision,s.model,s.effort.unwrap_or_default()])?;
            } else {tx.execute("DELETE FROM installation_model_selection",[])?;}
            let result=list(&tx)?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn probe_binding(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<ProviderBinding> {
        let user = user.clone();
        self.call(move |db| {
            let row = authorized_revision(&db.conn, &user, &id, revision, model != "catalog")?;
            binding(&db.provider_key, row, model, effort)
        })
        .await
    }
    /// Private synthetic probe material. Catalog access or a text-only binding
    /// cannot assert tool support. The challenge must not enter the first prompt.
    pub async fn tool_probe_binding(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<(ProviderBinding, String)> {
        let user = user.clone();
        self.call(move |db| {
            let row = authorized_revision(&db.conn, &user, &id, revision, true)?;
            verified_for_user(
                &db.conn,
                &user,
                &ModelSelection {
                    source: Default::default(),
                    provider_id: id,
                    provider_revision: revision,
                    model: model.clone(),
                    effort: effort.clone(),
                },
            )?;
            let binding = binding(&db.provider_key, row, model, effort)?;
            // This is a public protocol challenge, not an authentication secret.
            // A fresh 64-bit value proves the result was observed without asking
            // the model to reproduce a long opaque token. Exact matching remains.
            let nonce = random_token()?;
            Ok((binding, format!("proof-{}", &nonce[..16])))
        })
        .await
    }
    /// Only the successful server round-trip verifier may call this. HTTP must
    /// never accept a client-supplied capability assertion as proof.
    pub async fn record_tool_probe(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<ModelChoice> {
        let user = user.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorized_revision(&tx,&user,&id,revision,true)?;
            let selection=ModelSelection{ source: Default::default(), provider_id:id.clone(),provider_revision:revision,model:model.clone(),effort:effort.clone()};
            verified_for_user(&tx,&user,&selection)?;
            let changed=tx.execute("UPDATE provider_model_probes SET function_tools=1 WHERE provider_id=?1 AND revision=?2 AND model=?3 AND effort=?4",params![id,revision,model,effort.unwrap_or_default()])?;
            if changed!=1{return Err(StoreError::Conflict);}
            let result=verified_for_user(&tx,&user,&selection)?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn validate_provider_binding(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            authorized_revision(&db.conn, &user, &id, revision, true)?;
            Ok(())
        })
        .await
    }
    /// Only the server's successful bounded probe path calls this method. A request
    /// cannot attest its own verification: HTTP must never expose a direct mark API.
    pub async fn record_model_probe(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<ModelChoice> {
        let user = user.clone();
        self.call(move|db|{
            let row=authorized_revision(&db.conn,&user,&id,revision,true)?;model_valid(&model,&effort,row.config.kind)?;
            let exists=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM provider_model_probes WHERE provider_id=?1 AND revision=?2 AND model=?3 AND effort=?4)",params![id,revision,model,effort.clone().unwrap_or_default()],|r|r.get::<_,bool>(0))?;
            if !exists && db.conn.query_row("SELECT count(*) FROM provider_model_probes WHERE provider_id=?1 AND revision=?2",params![id,revision],|r|r.get::<_,i64>(0))? >= 128 { return Err(StoreError::Conflict); }
            let time=now()?;
            db.conn.execute("INSERT INTO provider_model_probes(provider_id,revision,model,effort,verified_at) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(provider_id,revision,model,effort) DO UPDATE SET verified_at=excluded.verified_at",params![id,revision,model,effort.clone().unwrap_or_default(),time])?;
            verified_for_user(&db.conn,&user,&ModelSelection{ source: Default::default(), provider_id:id,provider_revision:revision,model,effort})
        }).await
    }
    pub async fn resolve_model(&self, user: &UserContext) -> Result<ProviderBinding> {
        let user = user.clone();
        self.call(move |db| {
            let current = validate(&db.conn, &user, true, false)?;
            let choice = effective_choice(&db.conn, &current)?.ok_or(StoreError::Unavailable)?;
            binding_for_owner_choice(
                &db.conn,
                &db.provider_key,
                &current.tenant.0,
                &current.account.id,
                &choice,
            )
        })
        .await
    }
}
#[cfg(test)]
mod tests;

/// Exact current verified binding for durable dispatch; never resolves a fallback.
pub(super) fn binding_for_choice(
    conn: &Connection,
    key: &[u8; 32],
    choice: &ModelChoice,
) -> Result<ProviderBinding> {
    if choice.source != moosuri_core::ModelSource::Shared {
        return Err(StoreError::Conflict);
    }
    let selection = ModelSelection {
        source: Default::default(),
        provider_id: choice.provider_id.clone(),
        provider_revision: choice.provider_revision,
        model: choice.model.clone(),
        effort: choice.effort.clone(),
    };
    let current = verified(conn, &selection)?;
    if choice.function_tools && !current.function_tools {
        return Err(StoreError::Conflict);
    }
    binding(
        key,
        require_binding(conn, &choice.provider_id, choice.provider_revision)?,
        choice.model.clone(),
        choice.effort.clone(),
    )
}
pub(super) fn selected_mission_model(conn: &Connection, user: &UserContext) -> Result<ModelChoice> {
    available_choice(conn, user)?.ok_or(StoreError::ModelRequired)
}
pub(super) fn selected_choice(conn: &Connection, user: &UserContext) -> Result<ModelChoice> {
    available_choice(conn, user)?.ok_or(StoreError::ModelRequired)
}

// Durable work keeps its exact pin. An installation selection must authorize
// the pin at dispatch, including work admitted under the retired member policy.
pub(super) fn require_model_policy(
    conn: &Connection,
    tenant: &str,
    account: &str,
    choice: &ModelChoice,
) -> Result<()> {
    let role: String = conn
        .query_row(
            "SELECT role FROM accounts WHERE tenant_id=?1 AND id=?2 AND status='active' AND deleted=0",
            params![tenant, account],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict)?;
    match selected(conn, None)? {
        Some(installation) if matches_selection(choice, &installation) => Ok(()),
        None if role == "admin" => Ok(()),
        _ => Err(StoreError::Conflict),
    }
}

pub(super) fn binding_for_owner_choice(
    conn: &Connection,
    key: &[u8; 32],
    tenant: &str,
    account: &str,
    choice: &ModelChoice,
) -> Result<ProviderBinding> {
    require_model_policy(conn, tenant, account, choice)?;
    revalidate_owner_binding(conn, key, tenant, account, choice)
}

/// Check a dispatched call's exact provider, capability proof and credential.
/// The writer already enforced installation selection when it admitted the call.
pub(super) fn revalidate_owner_binding(
    conn: &Connection,
    key: &[u8; 32],
    tenant: &str,
    account: &str,
    choice: &ModelChoice,
) -> Result<ProviderBinding> {
    match choice.source {
        ModelSource::Shared => binding_for_choice(conn, key, choice),
        ModelSource::PersonalChatGpt => subscriptions::binding(conn, tenant, account, choice),
        ModelSource::PersonalClaudeCode => {
            crate::claude_execution::binding(conn, tenant, account, choice)
        }
        ModelSource::PersonalCompatible => personal_binding(conn, key, tenant, account, choice),
    }
}

// Writer-side recheck immediately before reserving a probe call.
pub(super) fn authorize_probe(
    c: &Connection,
    u: &UserContext,
    id: &str,
    revision: i64,
) -> Result<()> {
    authorized_revision(c, u, id, revision, true).map(|_| ())
}

/// Account deletion retains only opaque connection ownership tombstones. Never
/// turn an orphaned private connection into a shared provider.
pub(crate) fn retire_deleted_owner(c: &Connection, account: &str) -> Result<()> {
    c.execute("DELETE FROM personal_model_selections WHERE provider_id IN(SELECT provider_id FROM personal_compatible_providers WHERE account_id=?1)",[account])?;
    c.execute("DELETE FROM provider_model_probes WHERE provider_id IN(SELECT provider_id FROM personal_compatible_providers WHERE account_id=?1)",[account])?;
    c.execute("DELETE FROM provider_vision_probes WHERE provider_id IN(SELECT provider_id FROM personal_compatible_providers WHERE account_id=?1)",[account])?;
    c.execute("UPDATE provider_revisions SET name='Deleted personal connection',config='{\"kind\":\"compatible\",\"base_url\":null,\"allowed_ips\":[],\"router_providers\":[],\"zero_data_retention\":false}',enabled=0,credential=NULL WHERE provider_id IN(SELECT o.provider_id FROM personal_compatible_providers o JOIN accounts a ON a.id=o.account_id WHERE o.account_id=?1 AND a.deleted=1)",[account])?;
    Ok(())
}
