//! A versioned external envelope; v1 curated bytes and pins are never rehashed.
use super::*;
use moosuri_connectors::github_skills::{DownloadedGitHubSkill, GitHubSkillSource};
use moosuri_core::GitHubSkillLocation;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format_version: u32,
    repository: String,
    directory: String,
    commit: String,
    slug: String,
    files: BTreeMap<String, String>,
}
pub(super) fn identity(location: &GitHubSkillLocation) -> String {
    let mut hash = Sha256::new();
    for value in [&location.repository, &location.directory] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    format!("github-{:x}", hash.finalize())
}
pub(super) fn valid_id(id: &str) -> bool {
    id.strip_prefix("github-").is_some_and(|v| {
        v.len() == 64
            && v.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
fn validate(envelope: Envelope) -> Result<Validated> {
    if envelope.format_version != 2 {
        return Err(StoreError::InvalidInput);
    }
    let source = GitHubSkillSource {
        repository: envelope.repository.clone(),
        directory: envelope.directory.clone(),
        reference: envelope.commit.clone(),
    }
    .normalize()
    .map_err(|_| StoreError::InvalidInput)?;
    if source.repository() != envelope.repository || source.directory() != envelope.directory {
        return Err(StoreError::InvalidInput);
    }
    let location = GitHubSkillLocation {
        repository: envelope.repository.clone(),
        directory: envelope.directory.clone(),
    };
    let mut entry = validate_contents(Package {
        source: envelope.repository,
        commit: envelope.commit,
        slug: envelope.slug,
        files: envelope.files,
    })?;
    let mut hash = Sha256::new();
    for value in [
        "moosuri-public-github-envelope-v2",
        &entry.package.source,
        &location.directory,
        &entry.package.commit,
        &entry.package.slug,
    ]
    .into_iter()
    .chain(
        entry
            .package
            .files
            .iter()
            .flat_map(|(k, v)| [k.as_str(), v.as_str()]),
    ) {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    entry.digest = format!("{:x}", hash.finalize());
    entry.external = Some(location);
    Ok(entry)
}
pub(super) fn decode(json: &str) -> Result<Validated> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|_| StoreError::Corrupt)?;
    if value.get("format_version").is_some() {
        validate(serde_json::from_value(value).map_err(|_| StoreError::Corrupt)?)
    } else {
        validate_package(serde_json::from_value(value).map_err(|_| StoreError::Corrupt)?)
    }
}
pub(super) fn encode(entry: &Validated) -> Result<String> {
    if let Some(location) = &entry.external {
        serde_json::to_string(&Envelope {
            format_version: 2,
            repository: location.repository.clone(),
            directory: location.directory.clone(),
            commit: entry.package.commit.clone(),
            slug: entry.package.slug.clone(),
            files: entry.package.files.clone(),
        })
        .map_err(|_| StoreError::Corrupt)
    } else {
        serde_json::to_string(&entry.package).map_err(|_| StoreError::Corrupt)
    }
}
impl Store {
    /// Network acquisition has already completed. Revalidation here fences an
    /// expired/replaced account before staging even public bytes or returning a
    /// private activation revision. No installation or run is created by preview.
    pub async fn preview_github_skill(
        &self,
        user: &UserContext,
        downloaded: DownloadedGitHubSkill,
    ) -> Result<CommunitySkillCard> {
        let (source, commit, files) = downloaded.into_parts();
        let slug = files
            .get("SKILL.md")
            .and_then(|s| s.lines().find_map(|line| line.strip_prefix("name: ")))
            .ok_or(StoreError::InvalidInput)?
            .to_owned();
        let entry = validate(Envelope {
            format_version: 2,
            repository: source.repository().into(),
            directory: source.directory().into(),
            commit,
            slug,
            files,
        })?;
        let user = user.clone();
        self.call(move |db| preview(db, user, entry)).await
    }
}
fn preview(db: &mut Database, user: UserContext, entry: Validated) -> Result<CommunitySkillCard> {
    let user = super::super::validate(&db.conn, &user, true, false)?;
    stage(&db.conn, &entry)?;
    card(&db.conn, &user, &entry, &db.web_profile, true)
}
#[cfg(test)]
pub(crate) mod tests;
