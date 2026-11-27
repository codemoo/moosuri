//! Public engine capabilities and administrator decisions. Provider secrets,
//! worker paths, prompts and receipts belong to other stores.
pub mod quota;
use super::{admin::AdminSession, *};
use moosuri_core::ProviderKind;
use serde::{Deserialize, de::DeserializeOwned};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(deny_unknown_fields)]
pub struct ModelChoice {
    pub model: String,
    pub effort: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct MemberModelPolicy {
    pub connection_revision: Uuid,
    pub model: String,
    pub effort: Option<String>,
}
#[derive(Serialize, Deserialize, PartialEq)]
struct Catalog {
    kind: ProviderKind,
    host_default: ModelChoice,
    choices: Vec<ModelChoice>,
}
#[derive(Clone, Copy)]
pub enum ConnectionMode {
    Api,
    ChatgptPlan,
    ClaudeCode,
}
impl ConnectionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::ChatgptPlan => "chatgpt_plan",
            Self::ClaudeCode => "claude_code",
        }
    }
}

#[derive(Default, Serialize)]
pub struct Settings {
    configured: bool,
    enabled: bool,
    revision: i64,
    connection_revision: Option<Uuid>,
    kind: Option<ProviderKind>,
    connection_mode: Option<String>,
    host_default: Option<ModelChoice>,
    choices: Vec<ModelChoice>,
    default_choice: Option<ModelChoice>,
    default_stale: bool,
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| StoreError::InvalidInput)
}
fn parse<T: DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|_| StoreError::Corrupt)
}
fn valid(c: &ModelChoice) -> bool {
    !c.model.is_empty()
        && c.model.len() <= 200
        && c.model.bytes().all(|b| b.is_ascii_graphic())
        && c.effort.as_ref().is_none_or(|e| {
            !e.is_empty()
                && e.len() <= 32
                && e.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}
/// Publish public choices only after the installation-owned broker validates
/// its configuration. Replaying the same publication preserves admin edits.
pub fn publish_models(
    root: &Path,
    connection_revision: Uuid,
    kind: ProviderKind,
    mode: ConnectionMode,
    default: &ModelChoice,
    choices: &[ModelChoice],
    members: &[Uuid],
) -> Result<()> {
    if connection_revision.is_nil()
        || choices.is_empty()
        || choices.len() > 256
        || members.is_empty()
        || members.len() > 128
        || !choices.contains(default)
        || !choices.iter().all(valid)
    {
        return Err(StoreError::InvalidInput);
    }
    let mut choices = choices.to_vec();
    choices.sort();
    choices.dedup();
    let catalog = encode(&Catalog {
        kind,
        host_default: default.clone(),
        choices,
    })?;
    let (_lock, mut db) = open(root, false)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = connection_revision.to_string();
    let old: Option<(String, String, Option<String>)> = tx
        .query_row(
            "SELECT connection_revision,catalog,connection_mode FROM engine_settings WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if old.as_ref().is_some_and(|(prior, rows, prior_mode)| {
        prior == &id
            && (rows != &catalog || prior_mode.as_deref().is_some_and(|m| m != mode.as_str()))
    }) {
        return Err(StoreError::Conflict);
    }
    if old.as_ref().is_none_or(|(prior, _, _)| prior != &id)
        && tx.execute(
            "INSERT INTO engine_connections(id) VALUES(?1) ON CONFLICT DO NOTHING",
            [&id],
        )? == 0
    {
        return Err(StoreError::Conflict);
    }
    let mut changed = false;
    let mut keep = std::collections::BTreeSet::new();
    for member in members {
        let member = member.to_string();
        if !keep.insert(member.clone()) {
            return Err(StoreError::InvalidInput);
        }
        let n = tx.execute(
            "INSERT INTO engine_bindings(account_id,tenant_id,volume_id,generation)
            SELECT account_id,tenant_id,volume_id,generation FROM allocations WHERE account_id=?1
            ON CONFLICT(account_id) DO UPDATE SET tenant_id=excluded.tenant_id,volume_id=excluded.volume_id,generation=excluded.generation,choice=NULL,choice_connection=NULL
            WHERE (tenant_id,volume_id,generation)!=(excluded.tenant_id,excluded.volume_id,excluded.generation)",
            [&member],
        )?;
        if n == 0
            && !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM allocations WHERE account_id=?1)",
                [&member],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Err(StoreError::NotFound);
        }
        changed |= n > 0;
    }
    let previous = tx
        .prepare("SELECT account_id FROM engine_bindings")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for m in previous {
        if !keep.contains(&m) {
            tx.execute("DELETE FROM engine_bindings WHERE account_id=?1", [m])?;
            changed = true;
        }
    }
    tx.execute("INSERT INTO engine_settings(singleton,revision,connection_revision,catalog,enabled,connection_mode) VALUES(1,1,?1,?2,1,?4)
        ON CONFLICT(singleton) DO UPDATE SET revision=revision+1,connection_revision=excluded.connection_revision,catalog=excluded.catalog,connection_mode=excluded.connection_mode WHERE connection_revision!=excluded.connection_revision OR ?3 OR connection_mode IS NULL",params![id,catalog,changed,mode.as_str()])?;
    tx.commit()?;
    Ok(())
}
/// Withdraw the exact public catalog after the caller durably unpublished its
/// owned connection config. The historical ID also fences a broker that read
/// the old config before removal and tries to publish it late. A newer catalog
/// and its member choices are preserved. This does not retire credentials.
pub fn retire_published_models(
    root: &Path,
    actor: &AdminSession,
    connection_revision: Uuid,
) -> Result<()> {
    if connection_revision.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    let (_lock, mut db) = open(root, false)?;
    admin::check(&db, actor)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = connection_revision.to_string();
    tx.execute(
        "INSERT INTO engine_connections(id) VALUES(?1) ON CONFLICT DO NOTHING",
        [&id],
    )?;
    if tx.execute(
        "DELETE FROM engine_settings WHERE singleton=1 AND connection_revision=?1",
        [&id],
    )? > 0
    {
        tx.execute("DELETE FROM engine_bindings", [])?;
    }
    tx.commit()?;
    Ok(())
}
struct Current {
    revision: i64,
    connection: String,
    catalog: Catalog,
    mode: Option<String>,
    enabled: bool,
    selected: Option<ModelChoice>,
    stale: bool,
}
fn current(c: &Connection) -> Result<Option<Current>> {
    let row = c
        .query_row(
            "SELECT revision,connection_revision,catalog,enabled,default_choice,
                coalesce(default_connection!=connection_revision,0),connection_mode
         FROM engine_settings WHERE singleton=1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, bool>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(revision, connection, raw, enabled, choice, stale, mode)| {
            Ok(Current {
                revision,
                connection,
                catalog: parse(&raw)?,
                mode,
                enabled,
                selected: choice.as_deref().map(parse).transpose()?,
                stale,
            })
        },
    )
    .transpose()
}
fn settings(c: &Connection) -> Result<Settings> {
    let Some(s) = current(c)? else {
        return Ok(Settings::default());
    };
    Ok(Settings {
        configured: true,
        enabled: s.enabled,
        revision: s.revision,
        connection_revision: Some(s.connection.parse().map_err(|_| StoreError::Corrupt)?),
        kind: Some(s.catalog.kind),
        connection_mode: s.mode,
        host_default: Some(s.catalog.host_default),
        choices: s.catalog.choices,
        default_choice: s.selected,
        default_stale: s.stale,
    })
}
pub fn read(root: &Path, actor: &AdminSession) -> Result<Settings> {
    let (_lock, c) = open(root, false)?;
    admin::check(&c, actor)?;
    settings(&c)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationChange {
    pub revision: i64,
    pub connection_revision: Uuid,
    pub enabled: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub default_choice: Option<ModelChoice>,
}
pub fn change(root: &Path, actor: &AdminSession, change: InstallationChange) -> Result<Settings> {
    let (_lock, mut db) = open(root, false)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    admin::check(&tx, actor)?;
    let s = current(&tx)?.ok_or(StoreError::NotFound)?;
    if change.revision != s.revision || change.connection_revision.to_string() != s.connection {
        return Err(StoreError::Conflict);
    }
    if change
        .default_choice
        .as_ref()
        .is_some_and(|c| !s.catalog.choices.contains(c))
    {
        return Err(StoreError::InvalidInput);
    }
    let raw = change.default_choice.as_ref().map(encode).transpose()?;
    let pin = raw.as_ref().map(|_| &s.connection);
    tx.execute(
        "UPDATE engine_settings SET revision=revision+1, enabled=?1, default_choice=?2,
         default_connection=?3 WHERE singleton=1",
        params![change.enabled, raw, pin],
    )?;
    let out = settings(&tx)?;
    tx.commit()?;
    Ok(out)
}
fn effective(c: &Connection, a: &Allocation) -> Result<Option<MemberModelPolicy>> {
    let Some(s) = current(c)? else {
        return Ok(None);
    };
    if !s.enabled {
        return Err(StoreError::Forbidden);
    }
    // Existing registries may still store old member choices in these rows.
    // The binding now authorizes the caller to use the installation choice.
    let bound = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM engine_bindings
         WHERE account_id=?1 AND tenant_id=?2 AND volume_id=?3 AND generation=?4)",
        params![a.account_id, a.tenant_id, a.volume_id, a.generation],
        |r| r.get::<_, bool>(0),
    )?;
    if !bound {
        return Err(StoreError::Forbidden);
    }
    if s.stale {
        return Err(StoreError::Conflict);
    }
    let chosen = s.selected.unwrap_or(s.catalog.host_default);
    Ok(Some(MemberModelPolicy {
        connection_revision: s.connection.parse().map_err(|_| StoreError::Corrupt)?,
        model: chosen.model,
        effort: chosen.effort,
    }))
}
pub fn member_model_policy(root: &Path, account_id: Uuid) -> Result<Option<MemberModelPolicy>> {
    let (_lock, c) = open(root, false)?;
    let (a, _) = ready_session(&c, SessionDestination::Account(account_id))?;
    effective(&c, &a)
}
/// Must only run short local operations inside the registry lock.
pub fn with_ready_epoch_and_model_policy<T>(
    root: &Path,
    account_id: Uuid,
    expected_epoch: i64,
    action: impl FnOnce(Allocation, Option<MemberModelPolicy>) -> Result<T>,
) -> Result<T> {
    let (_lock, c) = open(root, false)?;
    let (a, e) = ready_session(&c, SessionDestination::Account(account_id))?;
    if e != expected_epoch {
        return Err(StoreError::Forbidden);
    }
    let policy = effective(&c, &a)?;
    action(a, policy)
}
