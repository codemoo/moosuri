use super::*;
use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
const COMMIT: &str = "1111111111111111111111111111111111111111";
const ROOT: &str = "2222222222222222222222222222222222222222";
const DIRECTORY: &str = "3333333333333333333333333333333333333333";
fn source() -> GitHubSkillSource {
    GitHubSkillSource {
        repository: "https://github.com/Example/Recipes.git".into(),
        directory: "recipe".into(),
        reference: "feature/synthetic".into(),
    }
}
fn blob_sha(text: &str) -> String {
    let mut hash = Sha1::new();
    hash.update(format!("blob {}\0", text.len()));
    hash.update(text);
    format!("{:x}", hash.finalize())
}
fn payload(path: &str) -> Value {
    if path.contains("/git/commits/") {
        return json!({"sha":COMMIT,"tree":{"sha":ROOT}});
    }
    if path.contains("/commits/") {
        return json!({"sha":COMMIT,"commit":{"tree":{"sha":ROOT}}});
    }
    if path.ends_with(ROOT) {
        return json!({"sha":ROOT,"truncated":false,"tree":[{"path":"recipe","mode":"040000","type":"tree","sha":DIRECTORY}]});
    }
    if path.ends_with(DIRECTORY) {
        return json!({"sha":DIRECTORY,"truncated":false,"tree":FILES.iter().map(|name|json!({"path":name,"mode":"100644","type":"blob","sha":blob_sha(name),"size":name.len()})).collect::<Vec<_>>()});
    }
    let sha = path.rsplit('/').next().unwrap();
    let text = FILES.iter().find(|name| blob_sha(name) == sha).unwrap();
    json!({"sha":sha,"size":text.len(),"encoding":"base64","content":STANDARD.encode(text)})
}
struct Fixture {
    base: Url,
    task: tokio::task::JoinHandle<()>,
    paths: Arc<Mutex<Vec<String>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(
        change: impl Fn(&str, &mut Value) -> Option<StatusCode> + Send + Sync + 'static,
    ) -> Self {
        let paths = Arc::new(Mutex::new(Vec::new()));
        let recorded = paths.clone();
        let change = Arc::new(change);
        let app = Router::new().fallback(get(move |request: Request<Body>| {
            let paths = recorded.clone();
            let change = change.clone();
            async move {
                assert!(request.headers().get("authorization").is_none());
                assert!(request.headers().get("cookie").is_none());
                assert!(request.headers().get("referer").is_none());
                let path = request.uri().path().to_owned();
                assert!(request.uri().query().is_none());
                paths.lock().unwrap().push(path.clone());
                let mut data = payload(&path);
                let status = change(&path, &mut data).unwrap_or(StatusCode::OK);
                Response::builder()
                    .status(status)
                    .header("Content-Type", "application/json")
                    .header("Location", "http://127.0.0.1/private")
                    .body(Body::from(data.to_string()))
                    .unwrap()
            }
        }));
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = Url::parse(&format!("http://{}/", socket.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
        Self { base, task, paths }
    }
    async fn acquire(&self) -> Result<DownloadedGitHubSkill> {
        Acquisition {
            client: builder().build().unwrap(),
            base: self.base.clone(),
            remaining: 2 * 1024 * 1024,
            requests: 0,
        }
        .acquire(source().normalize().unwrap())
        .await
    }
}
#[test]
fn repository_directory_reference_validation_has_one_canonical_identity() {
    let s = source().normalize().unwrap();
    assert_eq!(s.repository(), "https://github.com/example/recipes");
    assert_eq!(s.reference(), "feature/synthetic");
    for repository in [
        "http://github.com/a/b",
        "https://github.com.evil.test/a/b",
        "https://user@github.com/a/b",
        "https://github.com/a/b?token=synthetic",
        "https://github.com/a/b/tree/main/recipe",
        "https://github.com/a%2fb/c",
        "https://api.github.com/repos/a/b",
        "https://127.0.0.1/a/b",
    ] {
        let mut input = source();
        input.repository = repository.into();
        assert!(input.normalize().is_err(), "{repository}");
    }
    for directory in [
        "../recipe",
        "/recipe",
        "recipe/../other",
        "recipe//nested",
        "recipe/scripts\\run",
        "a/b/c/d/e/f/g",
    ] {
        let mut input = source();
        input.directory = directory.into();
        assert!(input.normalize().is_err());
    }
    for reference in [
        "",
        "../main",
        "main?private=1",
        "main#fragment",
        "main//next",
    ] {
        let mut input = source();
        input.reference = reference.into();
        assert!(input.normalize().is_err());
    }
}
#[tokio::test]
async fn resolves_branch_once_then_fetches_only_exact_tree_and_blob_objects_without_credentials() {
    let f = Fixture::new(|_, _| None).await;
    let package = f.acquire().await.unwrap();
    let (origin, commit, files) = package.into_parts();
    assert_eq!(origin.repository(), "https://github.com/example/recipes");
    assert_eq!(commit, COMMIT);
    assert_eq!(files.len(), 4);
    let paths = f.paths.lock().unwrap();
    assert_eq!(paths.iter().filter(|p| p.contains("/commits/")).count(), 1);
    assert_eq!(paths.len(), 7);
    assert!(paths[0].ends_with("feature%2Fsynthetic"));
    for path in paths.iter().skip(1) {
        assert!(path.contains("/git/trees/") || path.contains("/git/blobs/"));
        assert!(!path.contains("feature"));
    }
}
#[tokio::test]
async fn redirected_truncated_linked_executable_extra_and_oversized_packages_never_finish() {
    let f = Fixture::new(|_, _| Some(StatusCode::FOUND)).await;
    assert!(matches!(f.acquire().await, Err(GitHubSkillError::Redirect)));
    assert_eq!(f.paths.lock().unwrap().len(), 1);
    for mode in ["120000", "160000", "100755"] {
        let f = Fixture::new(move |path, data| {
            if path.ends_with(DIRECTORY) {
                data["tree"][0]["mode"] = json!(mode)
            }
            None
        })
        .await;
        assert!(matches!(
            f.acquire().await,
            Err(GitHubSkillError::Unsupported)
        ));
        assert_eq!(f.paths.lock().unwrap().len(), 3);
    }
    for kind in [
        "truncated",
        "extra",
        "size",
        "wrong_blob",
        "decoded_size",
        "content_mismatch",
    ] {
        let f = Fixture::new(move |path, data| {
            if path.ends_with(DIRECTORY) {
                match kind {
                    "truncated" => data["truncated"] = json!(true),
                    "extra" => data["tree"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"path":"scripts","mode":"040000","type":"tree","sha":ROOT})),
                    "size" => data["tree"][0]["size"] = json!(32769),
                    _ => {}
                }
            }
            if path.contains("/git/blobs/") {
                match kind {
                    "wrong_blob" => data["sha"] = json!(ROOT),
                    "decoded_size" => {
                        data["content"] = json!(STANDARD.encode("wrong decoded length"))
                    }
                    "content_mismatch" => {
                        let size = data["size"].as_u64().unwrap() as usize;
                        data["content"] = json!(STANDARD.encode("x".repeat(size)));
                    }
                    _ => {}
                }
            }
            None
        })
        .await;
        assert!(f.acquire().await.is_err(), "{kind}");
    }
}
#[tokio::test]
async fn transport_response_body_and_aggregate_limits_are_enforced_before_decode() {
    let f = Fixture::new(|_, data| {
        data["padding"] = json!("x".repeat(300 * 1024));
        None
    })
    .await;
    assert!(matches!(f.acquire().await, Err(GitHubSkillError::TooLarge)));
    let f = Fixture::new(|_, _| None).await;
    let mut acquisition = Acquisition {
        client: builder().build().unwrap(),
        base: f.base.clone(),
        remaining: 10,
        requests: 0,
    };
    assert!(matches!(
        acquisition
            .json::<Value>(&["repos", "a", "b", "commits", "main"], 1024)
            .await,
        Err(GitHubSkillError::TooLarge)
    ));
}
