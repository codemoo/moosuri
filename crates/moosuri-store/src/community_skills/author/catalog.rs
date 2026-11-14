//! Static release artifacts from verified public GitHub objects, without a Store.
//! No local bytes, branch resolution, private export, activation or publication.
use super::super::*;
use moosuri_connectors::github_skills::{GitHubSkillSource, acquire_github_skill, full_sha};
use std::{collections::BTreeSet, time::Duration};

pub const PACKAGE_LIMIT: usize = 16;
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(120);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("skills_catalog: {0}")]
pub struct Error(&'static str);
type CatalogResult<T> = std::result::Result<T, Error>;

/// Explicit public source selection; only full lowercase immutable commits.
/// Construction validates every selection before the first network request.
pub struct Selection {
    repository: String,
    commit: String,
    directories: BTreeSet<String>,
}
impl Selection {
    pub fn new(repository: &str, commit: &str, directories: Vec<String>) -> CatalogResult<Self> {
        if !full_sha(commit) || directories.is_empty() || directories.len() > PACKAGE_LIMIT {
            return Err(Error(
                "require exact lowercase 40-digit commit and 1-16 package directories",
            ));
        }
        let mut selected = BTreeSet::new();
        let mut canonical = None;
        for directory in directories {
            let source = GitHubSkillSource {
                repository: repository.into(),
                reference: commit.into(),
                directory,
            }
            .normalize()
            .map_err(|_| Error("invalid public repository or package directory"))?;
            canonical = Some(source.repository().to_owned());
            if !selected.insert(source.directory().to_owned()) {
                return Err(Error("duplicate package directory"));
            }
        }
        Ok(Self {
            repository: canonical.ok_or(Error("missing repository"))?,
            commit: commit.into(),
            directories: selected,
        })
    }
}

#[derive(Serialize)]
struct Index<'a> {
    format_version: u32,
    repository: &'a str,
    source_commit: &'a str,
    packages: Vec<Item>,
}
#[derive(Serialize)]
struct Item {
    directory: String,
    id: String,
    name: String,
    version: String,
    description: String,
    license: String,
    capabilities: Vec<String>,
    envelope_version: u32,
    content_digest: String,
    artifact: String,
    artifact_sha256: String,
}

/// Complete, deterministic artifact set. No index exists until all inputs pass.
/// The caller can atomically publish these UTF-8 bytes to a new directory.
pub struct Artifacts {
    files: BTreeMap<String, String>,
    package_count: usize,
}
impl Artifacts {
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }
    pub fn package_count(&self) -> usize {
        self.package_count
    }
}
struct Builder<'a> {
    selection: &'a Selection,
    entries: BTreeMap<String, Validated>,
}
impl<'a> Builder<'a> {
    fn new(selection: &'a Selection) -> Self {
        Self {
            selection,
            entries: BTreeMap::new(),
        }
    }
    fn add(&mut self, entry: Validated) -> CatalogResult<()> {
        let location = entry
            .external
            .as_ref()
            .ok_or(Error("verified GitHub envelope required"))?;
        if entry.package.source != self.selection.repository
            || entry.package.commit != self.selection.commit
            || !self.selection.directories.contains(&location.directory)
            || self.entries.contains_key(&location.directory)
        {
            return Err(Error("acquired source identity does not match selection"));
        }
        self.entries.insert(location.directory.clone(), entry);
        Ok(())
    }
    fn finish(self) -> CatalogResult<Artifacts> {
        if self.entries.len() != self.selection.directories.len() {
            return Err(Error("incomplete source selection"));
        }
        let mut files = BTreeMap::new();
        let mut packages = Vec::new();
        let mut total = 0;
        for (directory, entry) in self.entries {
            let encoded = external::encode(&entry).map_err(|_| Error("package encoding failed"))?;
            let artifact = format!("package-{}.json", entry.digest);
            let artifact_sha256 = format!("{:x}", Sha256::digest(encoded.as_bytes()));
            total += encoded.len();
            if total > OUTPUT_LIMIT || files.insert(artifact.clone(), encoded).is_some() {
                return Err(Error("artifact bounds or identity collision"));
            }
            packages.push(Item {
                directory,
                id: entry.id(),
                name: entry.package.slug,
                version: entry.metadata.version,
                description: entry.description,
                license: entry.metadata.license,
                capabilities: entry.metadata.capabilities,
                envelope_version: 2,
                content_digest: entry.digest,
                artifact,
                artifact_sha256,
            });
        }
        let package_count = packages.len();
        let index = serde_json::to_string(&Index {
            format_version: 1,
            repository: &self.selection.repository,
            source_commit: &self.selection.commit,
            packages,
        })
        .map_err(|_| Error("catalog encoding failed"))?;
        if total + index.len() > OUTPUT_LIMIT {
            return Err(Error("artifact output limit"));
        }
        files.insert("catalog.json".into(), index);
        Ok(Artifacts {
            files,
            package_count,
        })
    }
}

/// Read-only, fixed-host adapter acquisition. Each package is bounded by the
/// existing 12-request/2MiB/30s limits; the entire selection has a 120s deadline.
/// No authentication, Git subprocess or local package/installation read occurs.
pub async fn generate(selection: &Selection) -> CatalogResult<Artifacts> {
    tokio::time::timeout(DEADLINE, async {
        let mut builder = Builder::new(selection);
        for directory in &selection.directories {
            let downloaded = acquire_github_skill(GitHubSkillSource {
                repository: selection.repository.clone(),
                directory: directory.clone(),
                reference: selection.commit.clone(),
            })
            .await
            .map_err(|error| {
                use moosuri_connectors::github_skills::GitHubSkillError::*;
                Error(match error {
                    RateLimited => "public GitHub rate limit reached; retry later",
                    NotFound => "selected public commit or package is unavailable",
                    Timeout => "public source acquisition timed out",
                    _ => "public source acquisition failed validation or is unavailable",
                })
            })?;
            let entry = external::downloaded(downloaded)
                .map_err(|_| Error("package content compatibility failed"))?;
            builder.add(entry)?;
        }
        builder.finish()
    })
    .await
    .map_err(|_| Error("catalog acquisition exceeded total 120-second deadline"))?
}

#[cfg(test)]
mod tests;
