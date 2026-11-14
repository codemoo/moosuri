//! Public static catalog bytes from one operator-selected GitHub artifact directory.
//! No private queries, credentials, cookies, redirect, proxy or arbitrary host.
use super::github_skills::{GitHubSkillSource, full_sha};
use futures_util::StreamExt;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, time::Duration};

pub const INDEX_LIMIT: usize = 64 * 1024;
pub const ARTIFACT_LIMIT: usize = 512 * 1024;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogSource {
    repository: String,
    reference: String,
    directory: String,
}
impl CatalogSource {
    pub fn new(repository: &str, reference: &str, directory: &str) -> Result<Self, Error> {
        let s = GitHubSkillSource {
            repository: repository.into(),
            reference: reference.into(),
            directory: directory.into(),
        }
        .normalize()
        .map_err(|_| Error::Invalid)?;
        if !reference
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            repository: s.repository().into(),
            reference: s.reference().into(),
            directory: s.directory().into(),
        })
    }
    pub fn repository(&self) -> &str {
        &self.repository
    }
    pub fn label(&self) -> String {
        format!(
            "{} · {} / {}",
            self.repository, self.reference, self.directory
        )
    }
    pub fn key(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&("moosuri/catalog-source/v1", self)).expect("string tuple")
            )
        )
    }
    fn url(&self, file: &str) -> Result<url::Url, Error> {
        let repo = self
            .repository
            .strip_prefix("https://github.com/")
            .ok_or(Error::Invalid)?;
        let mut u =
            url::Url::parse("https://raw.githubusercontent.com/").map_err(|_| Error::Invalid)?;
        u.path_segments_mut()
            .map_err(|_| Error::Invalid)?
            .extend(repo.split('/'))
            .push(&self.reference)
            .extend(self.directory.split('/'))
            .push(file);
        Ok(u)
    }
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid public catalog")]
    Invalid,
    #[error("public catalog unavailable")]
    Unavailable,
    #[error("public catalog request limit reached")]
    RateLimited,
    #[error("public catalog exceeds bounds")]
    Limit,
}
fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogIndex {
    pub format_version: u32,
    pub repository: String,
    pub source_commit: String,
    pub packages: Vec<CatalogItem>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogItem {
    pub directory: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub license: String,
    pub capabilities: Vec<String>,
    pub envelope_version: u32,
    pub content_digest: String,
    pub artifact: String,
    pub artifact_sha256: String,
}
impl CatalogIndex {
    pub fn parse(bytes: &[u8], source: &CatalogSource) -> Result<Self, Error> {
        if bytes.len() > INDEX_LIMIT {
            return Err(Error::Limit);
        }
        let v: Self = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        if v.format_version != 1
            || v.repository != source.repository
            || !full_sha(&v.source_commit)
            || v.packages.is_empty()
            || v.packages.len() > 16
        {
            return Err(Error::Invalid);
        }
        let mut ids = BTreeSet::new();
        let mut dirs = BTreeSet::new();
        for item in &v.packages {
            let normalized = GitHubSkillSource {
                repository: v.repository.clone(),
                directory: item.directory.clone(),
                reference: v.source_commit.clone(),
            }
            .normalize()
            .map_err(|_| Error::Invalid)?;
            let mut hash = Sha256::new();
            for s in [&v.repository, &item.directory] {
                hash.update((s.len() as u64).to_be_bytes());
                hash.update(s.as_bytes());
            }
            let identity = format!("github-{:x}", hash.finalize());
            if normalized.directory() != item.directory
                || item.id != identity
                || !ids.insert(&item.id)
                || !dirs.insert(&item.directory)
                || item.envelope_version != 2
                || !sha256(&item.content_digest)
                || !sha256(&item.artifact_sha256)
                || item.artifact != format!("package-{}.json", item.content_digest)
                || item.name.is_empty()
                || item.name.len() > 64
                || !item
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                || item.description.is_empty()
                || item.description.len() > 512
                || item.description.chars().any(char::is_control)
                || item.version.split('.').count() != 3
                || !item
                    .version
                    .split('.')
                    .all(|p| !p.is_empty() && p.len() <= 8 && p.bytes().all(|b| b.is_ascii_digit()))
                || item.license != "Apache-2.0"
                || item.capabilities.is_empty()
                || item.capabilities.len() > 9
                || item.capabilities.iter().collect::<BTreeSet<_>>().len()
                    != item.capabilities.len()
                || item.capabilities.iter().any(|c| {
                    ![
                        "knowledge_search",
                        "knowledge_read_source",
                        "knowledge_read_memory",
                        "decimal_calculate",
                        "mail_search",
                        "mail_read",
                        "mail_thread",
                        "mail_prepare_draft",
                        "web_read",
                    ]
                    .contains(&c.as_str())
                })
            {
                return Err(Error::Invalid);
            }
        }
        Ok(v)
    }
}
/// Only successful fixed-host transport can create response bytes; local cache is
/// rechecked structurally by consumers before it may supply discovery metadata.
pub enum IndexResponse {
    NotModified,
    Modified {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}
#[derive(Clone)]
pub struct CatalogClient {
    client: Client,
    #[cfg(any(test, feature = "test-fixtures"))]
    fixture: Option<url::Url>,
}
impl CatalogClient {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .referer(false)
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .no_zstd()
                .pool_max_idle_per_host(0)
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(20))
                .user_agent("moosuri-public-catalog/1")
                .build()
                .map_err(|_| Error::Unavailable)?,
            #[cfg(any(test, feature = "test-fixtures"))]
            fixture: None,
        })
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    #[doc(hidden)]
    pub fn synthetic(origin: &str) -> Result<Self, Error> {
        let u = url::Url::parse(origin).map_err(|_| Error::Invalid)?;
        if u.scheme() != "http"
            || u.host_str() != Some("127.0.0.1")
            || u.port().is_none_or(|p| p == 0)
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(Error::Invalid);
        }
        let mut c = Self::new()?;
        c.fixture = Some(u);
        Ok(c)
    }
    fn url(&self, source: &CatalogSource, file: &str) -> Result<url::Url, Error> {
        let u = source.url(file)?;
        #[cfg(any(test, feature = "test-fixtures"))]
        if let Some(f) = &self.fixture {
            return f.join(u.path()).map_err(|_| Error::Invalid);
        }
        Ok(u)
    }
    async fn get(
        &self,
        source: &CatalogSource,
        file: &str,
        etag: Option<&str>,
        limit: usize,
    ) -> Result<IndexResponse, Error> {
        let mut request = self.client.get(self.url(source, file)?);
        if let Some(etag) = etag {
            if !valid_etag(etag) {
                return Err(Error::Invalid);
            }
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(|_| Error::Unavailable)?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return if etag.is_some() {
                Ok(IndexResponse::NotModified)
            } else {
                Err(Error::Invalid)
            };
        }
        if matches!(response.status().as_u16(), 403 | 429) {
            return Err(Error::RateLimited);
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(Error::Unavailable);
        }
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|v| valid_etag(v))
            .map(str::to_owned);
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| Error::Unavailable)?;
            if bytes.len() + chunk.len() > limit {
                return Err(Error::Limit);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(IndexResponse::Modified { bytes, etag })
    }
    pub async fn index(
        &self,
        source: &CatalogSource,
        etag: Option<&str>,
    ) -> Result<IndexResponse, Error> {
        self.get(source, "catalog.json", etag, INDEX_LIMIT).await
    }
    pub async fn artifact(
        &self,
        source: &CatalogSource,
        item: &CatalogItem,
    ) -> Result<Vec<u8>, Error> {
        if item.artifact != format!("package-{}.json", item.content_digest)
            || !sha256(&item.content_digest)
            || !sha256(&item.artifact_sha256)
        {
            return Err(Error::Invalid);
        }
        match self
            .get(source, &item.artifact, None, ARTIFACT_LIMIT)
            .await?
        {
            IndexResponse::Modified { bytes, .. }
                if format!("{:x}", Sha256::digest(&bytes)) == item.artifact_sha256 =>
            {
                Ok(bytes)
            }
            _ => Err(Error::Invalid),
        }
    }
}
pub fn valid_etag(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_source_cannot_escape_fixed_public_host_or_add_private_query() {
        for repository in [
            "http://github.com/a/b",
            "https://attacker.test/a/b",
            "https://user@github.com/a/b",
            "https://github.com/a/b?private=query",
        ] {
            assert!(CatalogSource::new(repository, "catalog", "release").is_err());
        }
        assert!(CatalogSource::new("https://github.com/a/b", "branch/path", "release").is_err());
        let s = CatalogSource::new("https://github.com/A/B", "catalog", "release").unwrap();
        assert_eq!(
            s.url("catalog.json").unwrap().as_str(),
            "https://raw.githubusercontent.com/a/b/catalog/release/catalog.json"
        );
        assert!(!valid_etag("header\r\nsecret"));
        for origin in [
            "http://localhost:1234/",
            "http://127.0.0.1:0/",
            "http://user@127.0.0.1:1234/",
            "http://127.0.0.1:1234/path",
            "http://127.0.0.1:1234/?query=private",
            "http://127.0.0.1:1234/#fragment",
            "https://127.0.0.1:1234/",
        ] {
            assert!(CatalogClient::synthetic(origin).is_err());
        }
        assert_ne!(
            s.key(),
            CatalogSource::new("https://github.com/a/b", "other-ref", "release")
                .unwrap()
                .key()
        );
        assert_ne!(
            s.key(),
            CatalogSource::new("https://github.com/a/b", "catalog", "other-directory")
                .unwrap()
                .key()
        );
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, Response, StatusCode},
        routing::get,
    };
    use std::sync::{Arc, Mutex};
    #[tokio::test]
    async fn conditional_public_fetch_and_artifact_digest_are_bounded_and_never_send_private_context()
     {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let app = Router::new().fallback(get(move |r: Request<Body>| {
            let captured = captured.clone();
            async move {
                assert!(r.headers().get("authorization").is_none());
                assert!(r.headers().get("cookie").is_none());
                assert!(r.uri().query().is_none());
                captured.lock().unwrap().push((
                    r.uri().path().to_owned(),
                    r.headers()
                        .get("if-none-match")
                        .map(|v| v.to_str().unwrap().to_owned()),
                ));
                if r.headers().contains_key("if-none-match") {
                    return Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .body(Body::empty())
                        .unwrap();
                }
                if r.uri().path().ends_with("catalog.json") {
                    Response::builder()
                        .header("etag", "\"fixture\"")
                        .body(Body::from("{\"synthetic\":true}"))
                        .unwrap()
                } else {
                    Response::builder()
                        .body(Body::from("synthetic-artifact"))
                        .unwrap()
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = CatalogClient::synthetic(&origin).unwrap();
        let source =
            CatalogSource::new("https://github.com/example/recipes", "catalog", "release").unwrap();
        assert!(matches!(
            client.index(&source, None).await.unwrap(),
            IndexResponse::Modified { etag: Some(_), .. }
        ));
        assert!(matches!(
            client.index(&source, Some("\"fixture\"")).await.unwrap(),
            IndexResponse::NotModified
        ));
        let mut item = CatalogItem {
            directory: "recipe".into(),
            id: String::new(),
            name: "recipe".into(),
            version: "1.0.0".into(),
            description: "Synthetic".into(),
            license: "Apache-2.0".into(),
            capabilities: vec!["knowledge_search".into()],
            envelope_version: 2,
            content_digest: "a".repeat(64),
            artifact: format!("package-{}.json", "a".repeat(64)),
            artifact_sha256: format!("{:x}", Sha256::digest(b"synthetic-artifact")),
        };
        assert_eq!(
            client.artifact(&source, &item).await.unwrap(),
            b"synthetic-artifact"
        );
        item.artifact_sha256 = "b".repeat(64);
        assert!(client.artifact(&source, &item).await.is_err());
        let recorded = requests.lock().unwrap();
        assert!(
            recorded
                .iter()
                .all(|(path, _)| path.starts_with("/example/recipes/catalog/release/"))
        );
        assert_eq!(recorded[1].1.as_deref(), Some("\"fixture\""));
        task.abort();
    }
    #[tokio::test]
    async fn redirects_and_oversize_index_fail_without_following_or_accepting_bytes() {
        let app = Router::new().fallback(get(|| async {
            Response::builder()
                .status(302)
                .header("location", "http://127.0.0.1:1/private")
                .body(Body::empty())
                .unwrap()
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let source =
            CatalogSource::new("https://github.com/example/recipes", "catalog", "release").unwrap();
        assert!(
            CatalogClient::synthetic(&origin)
                .unwrap()
                .index(&source, None)
                .await
                .is_err()
        );
        task.abort();
        let app = Router::new().fallback(get(|| async { vec![b'x'; INDEX_LIMIT + 1] }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(matches!(
            CatalogClient::synthetic(&origin)
                .unwrap()
                .index(&source, None)
                .await,
            Err(Error::Limit)
        ));
        task.abort();
    }
}
