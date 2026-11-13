//! Trusted, repository-owned instruction packages. No network, shell or executable loading.
use super::*;
use moosuri_core::{CommunitySkillCard, CommunitySkillMutation, SkillPin};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const SOURCE: &str = "https://github.com/codemoo/moosuri";
const COMMIT: &str = "7f91c0ad8ef38049c5e0e601658da9b99b5f824e";
const FILES: [&str; 4] = ["LICENSE", "SKILL.md", "examples.json", "moosuri.toml"];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    source: String,
    commit: String,
    slug: String,
    files: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sidecar {
    format_version: u32,
    version: String,
    mode: String,
    license: String,
    capabilities: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Examples {
    provenance: String,
    examples: Vec<Example>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Example {
    kind: String,
    request: String,
    expected: String,
}
struct Validated {
    package: Package,
    digest: String,
    metadata: Sidecar,
    description: String,
}
impl Validated {
    fn id(&self) -> String {
        format!("{SOURCE}#community/skills/{}", self.package.slug)
    }
    fn pin(&self) -> SkillPin {
        SkillPin {
            id: format!("community:{}", self.package.slug),
            revision: format!("{}:{}", self.package.commit, self.metadata.version),
            digest: self.digest.clone(),
        }
    }
}
// The accepted sidecar syntax is deliberately a strict, small TOML subset. It
// supports only scalar JSON strings/numbers and a single-line string array.
fn sidecar(text: &str) -> Result<Sidecar> {
    let mut fields = serde_json::Map::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (key, value) = line.split_once('=').ok_or(StoreError::InvalidInput)?;
        if fields
            .insert(
                key.trim().into(),
                serde_json::from_str(value.trim()).map_err(|_| StoreError::InvalidInput)?,
            )
            .is_some()
        {
            return Err(StoreError::InvalidInput);
        }
    }
    serde_json::from_value(serde_json::Value::Object(fields)).map_err(|_| StoreError::InvalidInput)
}
fn validate_package(package: Package) -> Result<Validated> {
    if package.source != SOURCE
        || package.commit.len() != 40
        || !package
            .commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || package.slug.is_empty()
        || package.slug.len() > 64
        || !package
            .slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || package.files.len() != 4
        || !FILES.iter().all(|f| package.files.contains_key(*f))
        || package
            .files
            .values()
            .any(|v| v.len() > 32 * 1024 || v.contains('\0'))
        || package.files.values().map(String::len).sum::<usize>() > 64 * 1024
    {
        return Err(StoreError::InvalidInput);
    }
    let metadata = sidecar(&package.files["moosuri.toml"])?;
    let known = [
        "knowledge_search",
        "knowledge_read_source",
        "knowledge_read_memory",
        "decimal_calculate",
        "mail_search",
        "mail_read",
        "mail_thread",
        "mail_prepare_draft",
        "web_read",
    ];
    if metadata.format_version != 1
        || metadata.mode != "instructions"
        || metadata.license != "Apache-2.0"
        || metadata.version.split('.').count() != 3
        || !metadata
            .version
            .split('.')
            .all(|p| !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_digit()))
        || metadata.capabilities.is_empty()
        || metadata.capabilities.len() > 9
        || metadata
            .capabilities
            .iter()
            .any(|c| !known.contains(&c.as_str()))
        || !package.files["LICENSE"].contains("Apache License, Version 2.0")
    {
        return Err(StoreError::InvalidInput);
    }
    let document = &package.files["SKILL.md"];
    let front = document
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---\n"))
        .ok_or(StoreError::InvalidInput)?;
    let mut name = None;
    let mut description = None;
    for line in front.0.lines() {
        if let Some(v) = line.strip_prefix("name: ") {
            if name.replace(v).is_some() {
                return Err(StoreError::InvalidInput);
            }
        } else if let Some(v) = line.strip_prefix("description: ") {
            if description.replace(v).is_some() {
                return Err(StoreError::InvalidInput);
            }
        } else {
            return Err(StoreError::InvalidInput);
        }
    }
    let description = description
        .filter(|v| !v.is_empty() && v.len() <= 512)
        .ok_or(StoreError::InvalidInput)?
        .to_owned();
    if name != Some(package.slug.as_str()) || front.1.trim().is_empty() {
        return Err(StoreError::InvalidInput);
    }
    let examples: Examples = serde_json::from_str(&package.files["examples.json"])
        .map_err(|_| StoreError::InvalidInput)?;
    if !examples.provenance.contains("synthetic")
        || examples.examples.len() < 2
        || examples.examples.len() > 8
        || !examples.examples.iter().any(|e| e.kind == "normal")
        || !examples.examples.iter().any(|e| e.kind != "normal")
        || examples.examples.iter().any(|e| {
            e.request.is_empty()
                || e.expected.is_empty()
                || e.request.len() > 2048
                || e.expected.len() > 2048
        })
    {
        return Err(StoreError::InvalidInput);
    }
    // Length framing prevents filename/content concatenation ambiguity. Source,
    // subdirectory and commit are part of identity as well as all package bytes.
    let mut hash = Sha256::new();
    for value in [&package.source, &package.commit, &package.slug]
        .into_iter()
        .chain(package.files.iter().flat_map(|(k, v)| [k, v]))
    {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    Ok(Validated {
        package,
        metadata,
        description,
        digest: format!("{:x}", hash.finalize()),
    })
}
fn catalog() -> Result<Vec<Validated>> {
    macro_rules! entry {
        ($slug:literal) => {{
            let files = BTreeMap::from([
                (
                    "SKILL.md".into(),
                    include_str!(concat!("../../../community/skills/", $slug, "/SKILL.md")).into(),
                ),
                (
                    "moosuri.toml".into(),
                    include_str!(concat!(
                        "../../../community/skills/",
                        $slug,
                        "/moosuri.toml"
                    ))
                    .into(),
                ),
                (
                    "examples.json".into(),
                    include_str!(concat!(
                        "../../../community/skills/",
                        $slug,
                        "/examples.json"
                    ))
                    .into(),
                ),
                (
                    "LICENSE".into(),
                    include_str!(concat!("../../../community/skills/", $slug, "/LICENSE")).into(),
                ),
            ]);
            validate_package(Package {
                source: SOURCE.into(),
                commit: COMMIT.into(),
                slug: $slug.into(),
                files,
            })?
        }};
    }
    let entries = vec![
        entry!("source-briefing"),
        entry!("meeting-preparation"),
        entry!("evidence-comparison"),
        entry!("inbox-briefing"),
        entry!("mail-reply"),
        entry!("web-research"),
    ];
    let manifest: BTreeMap<String, String> =
        serde_json::from_str(include_str!("../../../community/catalog-pins.json"))
            .map_err(|_| StoreError::Corrupt)?;
    if entries.len() != manifest.len()
        || entries
            .iter()
            .any(|e| manifest.get(&e.package.slug) != Some(&e.digest))
    {
        return Err(StoreError::Corrupt);
    }
    Ok(entries)
}
fn stored(conn: &Connection, digest: &str) -> Result<Validated> {
    let json: String = conn.query_row(
        "SELECT package FROM community_packages WHERE digest=?1",
        [digest],
        |r| r.get(0),
    )?;
    let package: Package = serde_json::from_str(&json).map_err(|_| StoreError::Corrupt)?;
    let verified = validate_package(package).map_err(|_| StoreError::Corrupt)?;
    if verified.digest != digest {
        return Err(StoreError::Corrupt);
    }
    Ok(verified)
}
fn stage(conn: &Connection, entry: &Validated) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO community_packages(digest,source,source_commit,package) VALUES (?1,?2,?3,?4)",params![entry.digest,entry.package.source,entry.package.commit,serde_json::to_string(&entry.package).map_err(|_|StoreError::Corrupt)?])?;
    let _ = stored(conn, &entry.digest)?;
    Ok(())
}
pub(crate) fn admit(
    conn: &Connection,
    user: &UserContext,
    profile: &mut moosuri_core::RunToolProfile,
) -> Result<()> {
    let installs=conn.prepare("SELECT package_id,active_digest,enabled FROM community_installs WHERE tenant_id=?1 AND managed=1 ORDER BY package_id")?.query_map([&user.tenant.0],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,bool>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (identity, digest, enabled) in installs {
        let slug = identity
            .strip_prefix(&format!("{SOURCE}#community/skills/"))
            .ok_or(StoreError::Corrupt)?;
        // Opting into managed distribution replaces the bundled twin. A durable
        // tombstone suppresses procedure discovery after disable/removal; it
        // never suppresses or grants the underlying native tool capabilities.
        profile.skills.retain(|pin| pin.id != slug);
        if enabled && let Some(digest) = digest {
            let entry = stored(conn, &digest)?;
            if entry.id() != identity {
                return Err(StoreError::Corrupt);
            }
            let pin = entry.pin();
            profile
                .community_metadata
                .push(moosuri_core::SkillMetadata {
                    pin: pin.clone(),
                    description: entry.description.clone(),
                });
            profile.skills.push(pin);
        }
    }
    Ok(())
}
pub(crate) fn read(
    conn: &Connection,
    pin: &SkillPin,
) -> Result<Option<moosuri_core::SkillDocument>> {
    if !pin.id.starts_with("community:") {
        return Ok(None);
    }
    let entry = stored(conn, &pin.digest)?;
    if entry.pin() != *pin {
        return Ok(None);
    }
    Ok(Some(moosuri_core::SkillDocument {
        pin: pin.clone(),
        instructions: entry.package.files["SKILL.md"].clone(),
    }))
}
fn curated_title(slug: &str) -> &str {
    match slug {
        "source-briefing" => "모아 둔 자료 정리",
        "meeting-preparation" => "회의 준비",
        "evidence-comparison" => "자료를 근거로 비교",
        "web-research" => "웹에서 조사하고 비교",
        "inbox-briefing" => "메일함 핵심 정리",
        "mail-reply" => "메일 답장 준비",
        _ => slug,
    }
}

fn cards(
    conn: &Connection,
    user: &UserContext,
    web: &moosuri_core::WebToolProfile,
) -> Result<Vec<CommunitySkillCard>> {
    let mail = !crate::mail::pins(conn, user)?.is_empty();
    catalog()?.into_iter().map(|e| {
        let row=conn.query_row("SELECT active_digest,previous_digest,enabled,hidden,revision FROM community_installs WHERE tenant_id=?1 AND package_id=?2",params![user.tenant.0,e.id()],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,bool>(2)?,r.get::<_,bool>(3)?,r.get::<_,i64>(4)?))).optional()?;
        let (active,previous,enabled,hidden,revision)=row.unwrap_or((None,None,false,false,0));
        let current=active.as_deref().map(|d|stored(conn,d)).transpose()?;
        let requirements=current.as_ref().unwrap_or(&e);
        let missing:Vec<_>=requirements.metadata.capabilities.iter().filter(|c| (c.starts_with("mail_")&&!mail) || (c.as_str()=="web_read"&&!web.read)).cloned().collect();
        Ok(CommunitySkillCard { id:e.package.slug.clone(),name:curated_title(&e.package.slug).to_owned(),description:e.description.clone(),source:format!("{SOURCE}/tree/{COMMIT}/community/skills/{}",e.package.slug),source_commit:e.package.commit.clone(),version:e.metadata.version.clone(),digest:e.digest.clone(),license:e.metadata.license.clone(),capabilities:e.metadata.capabilities.clone(),installed_version:current.as_ref().map(|v|v.metadata.version.clone()),installed_digest:active.clone(),enabled,hidden,revision,can_rollback:previous.is_some(),readiness:if !missing.is_empty(){"connection_needed"}else if enabled{"ready"}else if active.is_some(){"disabled"}else{"installable"}.into(),missing })
    }).collect()
}
impl Store {
    pub async fn community_skills(&self, user: &UserContext) -> Result<Vec<CommunitySkillCard>> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            cards(&db.conn, &user, &db.web_profile)
        })
        .await
    }
    pub async fn mutate_community_skill(
        &self,
        user: &UserContext,
        id: String,
        input: CommunitySkillMutation,
    ) -> Result<Vec<CommunitySkillCard>> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let entry = catalog()?
                .into_iter()
                .find(|e| e.package.slug == id)
                .ok_or(StoreError::NotFound)?;
            apply(db, user, entry, input)
        })
        .await
    }
}
// Only trusted catalog resolution constructs the entry passed here. Keeping
// activation separate lets failure tests exercise the same transactional path.
fn apply(
    db: &mut Database,
    user: UserContext,
    entry: Validated,
    input: CommunitySkillMutation,
) -> Result<Vec<CommunitySkillCard>> {
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let row=tx.query_row("SELECT active_digest,previous_digest,enabled,hidden,revision,managed FROM community_installs WHERE tenant_id=?1 AND package_id=?2",params![user.tenant.0,entry.id()],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,bool>(2)?,r.get::<_,bool>(3)?,r.get::<_,i64>(4)?,r.get::<_,bool>(5)?))).optional()?;
    let (mut active, mut previous, mut enabled, mut hidden, revision, mut managed) =
        row.unwrap_or((None, None, false, false, 0, false));
    if revision != input.expected_revision {
        return Err(StoreError::Conflict);
    }
    match input.action.as_str() {
        "install" | "update" => {
            if input.expected_digest.as_deref() != Some(&entry.digest) {
                return Err(StoreError::Conflict);
            }
            if input.action == "update" && active.is_none() {
                return Err(StoreError::Conflict);
            }
            stage(&tx, &entry)?;
            managed = true;
            if active.as_ref() != Some(&entry.digest) {
                previous = active.take();
                active = Some(entry.digest.clone())
            }
            if input.action == "install" {
                enabled = true;
                hidden = false;
            }
        }
        "rollback" => {
            if previous.is_none() {
                return Err(StoreError::Conflict);
            }
            std::mem::swap(&mut active, &mut previous);
        }
        "disable" => {
            if active.is_none() {
                return Err(StoreError::Conflict);
            }
            enabled = false;
        }
        "enable" => {
            if active.is_none() {
                return Err(StoreError::Conflict);
            }
            enabled = true;
        }
        "hide" => hidden = true,
        "show" => hidden = false,
        "remove" => {
            managed = true;
            active = None;
            previous = None;
            enabled = false;
            revoke(&tx, &user, &entry.pin().id)?;
        }
        _ => return Err(StoreError::InvalidInput),
    }
    tx.execute("INSERT INTO community_installs(tenant_id,package_id,active_digest,previous_digest,enabled,hidden,revision,managed) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(tenant_id,package_id) DO UPDATE SET active_digest=excluded.active_digest,previous_digest=excluded.previous_digest,enabled=excluded.enabled,hidden=excluded.hidden,revision=excluded.revision,managed=excluded.managed",params![user.tenant.0,entry.id(),active,previous,enabled,hidden,revision+1,managed])?;
    tx.commit()?;
    cards(&db.conn, &user, &db.web_profile)
}
// Removal revokes every admitted run using this package, including queued runs
// and retries. Exact run snapshots and accounting/effect receipts are retained.
fn revoke(conn: &Connection, user: &UserContext, id: &str) -> Result<()> {
    let runs=conn.prepare("SELECT id FROM chat_runs WHERE tenant_id=?1 AND EXISTS (SELECT 1 FROM json_each(chat_runs.model,'$.agent_profile.skills') WHERE json_extract(value,'$.id')=?2)")?.query_map(params![user.tenant.0,id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for id in runs {
        conn.execute(
            "INSERT OR IGNORE INTO community_run_revocations VALUES (?1,?2)",
            params![user.tenant.0, id],
        )?;
        conn.execute(
            "UPDATE agent_execution SET invalidated=1 WHERE tenant_id=?1 AND run_id=?2",
            params![user.tenant.0, id],
        )?;
        let run = chat::load_run(conn, &user.tenant.0, &id)?;
        if run.state.active() {
            chat::transition(conn, &run, moosuri_core::RunState::Cancelled, None)?;
        }
    }
    Ok(())
}
#[cfg(test)]
pub(crate) mod tests;
