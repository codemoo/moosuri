//! Public GitHub instruction acquisition. No Git process, credentials or archive extraction.
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use url::Url;
const API: &str = "https://api.github.com/";
const FILES: [&str; 4] = ["LICENSE", "SKILL.md", "examples.json", "moosuri.toml"];
const DEADLINE: Duration = Duration::from_secs(30);
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubSkillSource {
    pub repository: String,
    pub directory: String,
    pub reference: String,
}
#[derive(Clone)]
pub struct NormalizedSource {
    repository: String,
    directory: String,
    reference: String,
    owner: String,
    repo: String,
}
impl NormalizedSource {
    pub fn repository(&self) -> &str {
        &self.repository
    }
    pub fn directory(&self) -> &str {
        &self.directory
    }
    pub fn reference(&self) -> &str {
        &self.reference
    }
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GitHubSkillError {
    #[error("invalid public GitHub source")]
    InvalidSource,
    #[error("GitHub package is unavailable")]
    NotFound,
    #[error("GitHub request limit reached")]
    RateLimited,
    #[error("GitHub acquisition failed")]
    Network,
    #[error("GitHub acquisition timed out")]
    Timeout,
    #[error("GitHub redirects are unsupported")]
    Redirect,
    #[error("GitHub package exceeds acquisition bounds")]
    TooLarge,
    #[error("unsupported GitHub package layout")]
    Unsupported,
    #[error("invalid GitHub package response")]
    InvalidResponse,
}
type Result<T> = std::result::Result<T, GitHubSkillError>;
fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
pub fn full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
impl GitHubSkillSource {
    pub fn normalize(&self) -> Result<NormalizedSource> {
        let url = Url::parse(&self.repository).map_err(|_| GitHubSkillError::InvalidSource)?;
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.repository.contains('%')
            || self.repository.contains('\\')
            || self.repository.split('/').any(|p| p == "." || p == "..")
        {
            return Err(GitHubSkillError::InvalidSource);
        }
        let path = url.path().trim_end_matches('/');
        let fields: Vec<_> = path.split('/').collect();
        if fields.len() != 3 || !segment(fields[1]) || !segment(fields[2]) {
            return Err(GitHubSkillError::InvalidSource);
        }
        let owner = fields[1].to_ascii_lowercase();
        let repo = fields[2]
            .strip_suffix(".git")
            .unwrap_or(fields[2])
            .to_ascii_lowercase();
        if !segment(&repo) {
            return Err(GitHubSkillError::InvalidSource);
        }
        let directory = self.directory.as_str();
        let parts: Vec<_> = directory.split('/').collect();
        if directory.len() > 256 || parts.len() > 6 || !parts.iter().all(|p| segment(p)) {
            return Err(GitHubSkillError::InvalidSource);
        }
        let reference = &self.reference;
        if reference.is_empty()
            || reference.len() > 128
            || reference.starts_with('/')
            || reference.ends_with('/')
            || reference
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || !reference
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b))
        {
            return Err(GitHubSkillError::InvalidSource);
        }
        Ok(NormalizedSource {
            repository: format!("https://github.com/{owner}/{repo}"),
            directory: directory.into(),
            reference: reference.clone(),
            owner,
            repo,
        })
    }
}
/// Only this adapter can construct downloaded packages; HTTP clients cannot
/// submit arbitrary package bytes while claiming GitHub provenance.
pub struct DownloadedGitHubSkill {
    source: NormalizedSource,
    commit: String,
    files: BTreeMap<String, String>,
}
impl DownloadedGitHubSkill {
    pub fn into_parts(self) -> (NormalizedSource, String, BTreeMap<String, String>) {
        (self.source, self.commit, self.files)
    }
}
#[derive(Deserialize)]
struct Commit {
    sha: String,
    commit: CommitBody,
}
#[derive(Deserialize)]
struct CommitBody {
    tree: TreeReference,
}
#[derive(Deserialize)]
struct GitCommit {
    sha: String,
    tree: TreeReference,
}
#[derive(Deserialize)]
struct TreeReference {
    sha: String,
}
#[derive(Deserialize)]
struct Tree {
    sha: String,
    truncated: bool,
    tree: Vec<TreeEntry>,
}
#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    size: Option<usize>,
}
#[derive(Deserialize)]
struct Blob {
    sha: String,
    size: usize,
    encoding: String,
    content: String,
}
struct Acquisition {
    client: Client,
    base: Url,
    remaining: usize,
    requests: u8,
}
impl Acquisition {
    async fn json<T: serde::de::DeserializeOwned>(
        &mut self,
        parts: &[&str],
        limit: usize,
    ) -> Result<T> {
        self.requests += 1;
        if self.requests > 12 {
            return Err(GitHubSkillError::TooLarge);
        }
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| GitHubSkillError::InvalidSource)?
            .extend(parts);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| GitHubSkillError::Network)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(GitHubSkillError::Redirect);
        }
        if status.as_u16() == 404 {
            return Err(GitHubSkillError::NotFound);
        }
        if status.as_u16() == 429
            || (status.as_u16() == 403
                && response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .is_some_and(|v| v == "0"))
        {
            return Err(GitHubSkillError::RateLimited);
        }
        if !status.is_success() {
            return Err(GitHubSkillError::Network);
        }
        if response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|v| v != "identity")
        {
            return Err(GitHubSkillError::Unsupported);
        }
        let kind = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("");
        if kind != "application/json" && kind != "application/vnd.github+json" {
            return Err(GitHubSkillError::Unsupported);
        }
        if response
            .content_length()
            .is_some_and(|n| n > limit as u64 || n > self.remaining as u64)
        {
            return Err(GitHubSkillError::TooLarge);
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| GitHubSkillError::Network)?;
            if chunk.len() > self.remaining || bytes.len() + chunk.len() > limit {
                return Err(GitHubSkillError::TooLarge);
            }
            self.remaining -= chunk.len();
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| GitHubSkillError::InvalidResponse)
    }
    async fn tree(&mut self, source: &NormalizedSource, sha: &str) -> Result<Tree> {
        if !full_sha(sha) {
            return Err(GitHubSkillError::InvalidResponse);
        }
        let tree: Tree = self
            .json(
                &["repos", &source.owner, &source.repo, "git", "trees", sha],
                512 * 1024,
            )
            .await?;
        if tree.sha != sha || tree.truncated || tree.tree.len() > 2048 {
            return Err(GitHubSkillError::TooLarge);
        }
        let mut names = BTreeSet::new();
        if tree
            .tree
            .iter()
            .any(|e| !segment(&e.path) || !names.insert(e.path.clone()))
        {
            return Err(GitHubSkillError::InvalidResponse);
        }
        Ok(tree)
    }
    async fn acquire(mut self, source: NormalizedSource) -> Result<DownloadedGitHubSkill> {
        let commit: Commit = if full_sha(&source.reference) {
            let object: GitCommit = self
                .json(
                    &[
                        "repos",
                        &source.owner,
                        &source.repo,
                        "git",
                        "commits",
                        &source.reference,
                    ],
                    256 * 1024,
                )
                .await?;
            Commit {
                sha: object.sha,
                commit: CommitBody { tree: object.tree },
            }
        } else {
            self.json(
                &[
                    "repos",
                    &source.owner,
                    &source.repo,
                    "commits",
                    &source.reference,
                ],
                256 * 1024,
            )
            .await?
        };
        if !full_sha(&commit.sha)
            || !full_sha(&commit.commit.tree.sha)
            || (full_sha(&source.reference) && commit.sha != source.reference)
        {
            return Err(GitHubSkillError::InvalidResponse);
        }
        let mut tree_sha = commit.commit.tree.sha;
        for part in source.directory.split('/') {
            let tree = self.tree(&source, &tree_sha).await?;
            let entry = tree
                .tree
                .into_iter()
                .find(|e| e.path == part)
                .ok_or(GitHubSkillError::NotFound)?;
            if entry.kind != "tree" || entry.mode != "040000" || !full_sha(&entry.sha) {
                return Err(GitHubSkillError::Unsupported);
            }
            tree_sha = entry.sha;
        }
        let tree = self.tree(&source, &tree_sha).await?;
        if tree.tree.len() != 4
            || tree.tree.iter().any(|e| {
                !FILES.contains(&e.path.as_str())
                    || e.kind != "blob"
                    || e.mode != "100644"
                    || !full_sha(&e.sha)
                    || e.size.is_none_or(|n| n > 32 * 1024)
            })
        {
            return Err(GitHubSkillError::Unsupported);
        }
        let mut files = BTreeMap::new();
        let mut total = 0;
        for entry in tree.tree {
            let blob: Blob = self
                .json(
                    &[
                        "repos",
                        &source.owner,
                        &source.repo,
                        "git",
                        "blobs",
                        &entry.sha,
                    ],
                    64 * 1024,
                )
                .await?;
            if blob.sha != entry.sha
                || blob.encoding != "base64"
                || blob.size > 32 * 1024
                || Some(blob.size) != entry.size
            {
                return Err(GitHubSkillError::InvalidResponse);
            }
            let encoded: String = blob
                .content
                .chars()
                .filter(|c| *c != '\n' && *c != '\r')
                .collect();
            let decoded = STANDARD
                .decode(encoded)
                .map_err(|_| GitHubSkillError::InvalidResponse)?;
            if decoded.len() != blob.size {
                return Err(GitHubSkillError::InvalidResponse);
            }
            let mut object = Sha1::new();
            object.update(format!("blob {}\0", decoded.len()).as_bytes());
            object.update(&decoded);
            if format!("{:x}", object.finalize()) != entry.sha {
                return Err(GitHubSkillError::InvalidResponse);
            }
            total += decoded.len();
            if total > 64 * 1024 {
                return Err(GitHubSkillError::TooLarge);
            }
            let text = String::from_utf8(decoded).map_err(|_| GitHubSkillError::Unsupported)?;
            if text.contains('\0') {
                return Err(GitHubSkillError::Unsupported);
            }
            files.insert(entry.path, text);
        }
        Ok(DownloadedGitHubSkill {
            source,
            commit: commit.sha,
            files,
        })
    }
}
async fn client() -> Result<Client> {
    let addresses: Vec<_> = tokio::net::lookup_host(("api.github.com", 443))
        .await
        .map_err(|_| GitHubSkillError::Network)?
        .collect();
    if addresses.is_empty()
        || addresses.len() > 64
        || addresses
            .iter()
            .any(|a| !crate::web::public_address(a.ip()))
    {
        return Err(GitHubSkillError::Network);
    }
    builder()
        .resolve_to_addrs("api.github.com", &addresses)
        .build()
        .map_err(|_| GitHubSkillError::Network)
}
fn builder() -> reqwest::ClientBuilder {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(0)
        .user_agent("moosuri-public-skills/0.1")
        .default_headers({
            let mut h = header::HeaderMap::new();
            h.insert(
                header::ACCEPT,
                header::HeaderValue::from_static("application/vnd.github+json"),
            );
            h.insert(
                header::ACCEPT_ENCODING,
                header::HeaderValue::from_static("identity"),
            );
            h.insert(
                "x-github-api-version",
                header::HeaderValue::from_static("2022-11-28"),
            );
            h
        })
}
pub async fn acquire_github_skill(input: GitHubSkillSource) -> Result<DownloadedGitHubSkill> {
    let source = input.normalize()?;
    tokio::time::timeout(DEADLINE, async {
        let client = client().await?;
        Acquisition {
            client,
            base: Url::parse(API).map_err(|_| GitHubSkillError::InvalidSource)?,
            remaining: 2 * 1024 * 1024,
            requests: 0,
        }
        .acquire(source)
        .await
    })
    .await
    .map_err(|_| GitHubSkillError::Timeout)?
}
#[cfg(test)]
mod tests;
