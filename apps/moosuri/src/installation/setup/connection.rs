//! Initial model setup calls the same authenticated API used by React.
use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request},
};
use serde_json::{Value, json};
use tower::ServiceExt;
struct Api {
    router: Router,
    cookie: zeroize::Zeroizing<String>,
    account: String,
}
impl Api {
    async fn request(&self, method: Method, path: &str, body: Value) -> Result<Value> {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:3210")
            .header("origin", "http://127.0.0.1:3210")
            .header("cookie", self.cookie.as_str())
            .header("x-moosuri-account", &self.account)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).map_err(error)?))
            .map_err(error)?;
        let r = self.router.clone().oneshot(request).await.map_err(error)?;
        if !r.status().is_success() {
            return Err("setup_model_connection_failed_use_web_settings".into());
        }
        let b = to_bytes(r.into_body(), 2 * 1024 * 1024)
            .await
            .map_err(error)?;
        serde_json::from_slice(&b).map_err(|_| "setup_model_response_invalid".into())
    }
}
pub(super) async fn configure(
    store: &Store,
    tty: &mut Terminal,
    email: &str,
    password: &str,
    temporary: bool,
) -> Result<String> {
    let pick = tty.select(
        "Choose a model connection",
        "모델 연결 방식을 선택하세요",
        &[
            (
                "Connect later in the dashboard",
                "나중에 웹 대시보드에서 연결",
            ),
            ("OpenAI API", "OpenAI API"),
            ("Anthropic API", "Anthropic API"),
            ("OpenRouter API", "OpenRouter API"),
            (
                "Local model / OpenAI-compatible server",
                "로컬 모델 / OpenAI 호환 서버",
            ),
            (
                "ChatGPT subscription · personal sign-in",
                "ChatGPT 구독 · 개인 로그인",
            ),
            (
                "Claude subscription · separate Linux setup",
                "Claude 구독 · 별도 Linux 설정",
            ),
        ],
        0,
    )?;
    if pick == 0 {
        return Ok("/app".into());
    }
    if pick >= 5 {
        tty.say(if pick==5{"After sign-in, open Model settings → ChatGPT. Use your own subscription; it is not shared with other accounts."}else{"Claude subscription requires a separately configured isolated official Claude Code runner on Linux. The installer does not install that optional runtime. Anthropic API works on all supported platforms."},if pick==5{"로그인 후 모델 설정 → ChatGPT에서 개인 구독을 연결하세요. 다른 사용자와 구독을 공유하지 않습니다."}else{"Claude 구독은 Linux의 격리된 공식 Claude Code 실행 환경을 별도로 설정해야 합니다. 선택 구성요소를 자동 설치하지 않습니다. Anthropic API는 모든 지원 플랫폼에서 쓸 수 있습니다."})?;
        return Ok("/settings/model".into());
    }
    if temporary {
        tty.say("First sign in with your temporary password and change it. Then connect this provider in Admin → Models.","임시 비밀번호로 로그인하고 비밀번호를 바꾼 뒤, 관리자 → 모델에서 연결해주세요.")?;
        return Ok("/admin/providers".into());
    }
    let login = store
        .login(email.to_owned(), password.to_owned())
        .await
        .map_err(|e| e.to_string())?;
    let user = store
        .authenticate(login.token.clone())
        .await
        .map_err(|e| e.to_string())?;
    let config =
        moosuri_server::ServerConfig::new("http://127.0.0.1:3210", true).map_err(str::to_owned)?;
    let runtime = config.start_runtime(store.clone());
    let api = Api {
        router: moosuri_server::router_with_runtime(store.clone(), config, runtime.clone()),
        cookie: zeroize::Zeroizing::new(format!("moosuri_session={}", login.token)),
        account: user.account().id.clone(),
    };
    let result = shared(&api, tty, pick).await;
    // Session and worker lifetime are bounded to this explicit setup operation.
    let logout = store.logout(&user).await;
    drop(api);
    runtime.shutdown().await;
    logout.map_err(|e| e.to_string())?;
    match result {
        Ok(()) => Ok("/app".into()),
        Err(e) if e == "setup_cancelled" => Err(e),
        Err(_) => {
            tty.say("Installation can continue. Model verification did not finish; open Admin → Models to retry. A saved connection is not yet a verified default.","설치는 계속할 수 있습니다. 모델 확인을 끝내지 못했습니다. 관리자 → 모델에서 다시 확인해주세요. 저장된 연결과 검증된 기본 모델은 구분됩니다.")?;
            Ok("/admin/providers".into())
        }
    }
}
async fn shared(api: &Api, tty: &mut Terminal, pick: usize) -> Result<()> {
    let (kind, name) = match pick {
        1 => ("open_ai", "OpenAI"),
        2 => ("anthropic", "Anthropic"),
        3 => ("open_router", "OpenRouter"),
        _ => ("compatible", "Local model"),
    };
    let mut allowed = Vec::<String>::new();
    let base = if pick == 4 {
        let preset = tty.select(
            "Local server type",
            "로컬 서버 종류",
            &[
                (
                    "Ollama · http://127.0.0.1:11434/v1",
                    "Ollama · http://127.0.0.1:11434/v1",
                ),
                (
                    "LM Studio · http://127.0.0.1:1234/v1",
                    "LM Studio · http://127.0.0.1:1234/v1",
                ),
                ("Another compatible endpoint", "다른 호환 서버 주소"),
            ],
            0,
        )?;
        let default = match preset {
            0 => "http://127.0.0.1:11434/v1",
            1 => "http://127.0.0.1:1234/v1",
            _ => "",
        };
        let endpoint = tty.input("API base URL", "API 기본 주소", default)?;
        let parsed = url::Url::parse(&endpoint).map_err(error)?;
        if parsed.username() != ""
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("setup_endpoint_invalid".into());
        }
        if let Some(host) = parsed.host_str() {
            if let Ok(ip) = host.parse::<std::net::IpAddr>() {
                allowed.push(ip.to_string())
            } else if host == "localhost" {
                allowed.extend(["127.0.0.1".into(), "::1".into()]);
            }
        }
        Some(endpoint)
    } else {
        None
    };
    let secret = tty.secret(
        if pick == 4 {
            "API key (Enter if this local server needs none)"
        } else {
            "API key"
        },
        if pick == 4 {
            "API 키 (인증 없는 로컬 서버면 Enter)"
        } else {
            "API 키"
        },
    )?;
    let credential = if secret.is_empty() {
        json!({"action":"clear"})
    } else {
        json!({"action":"replace","value":secret.as_str()})
    };
    let p=api.request(Method::POST,"/api/v1/admin/providers",json!({"name":name,"config":{"kind":kind,"base_url":base,"allowed_ips":allowed,"router_providers":[],"zero_data_retention":false},"enabled":true,"credential":credential})).await?;
    let id = p["id"]
        .as_str()
        .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        .ok_or("setup_model_response_invalid")?;
    let rev = p["revision"]
        .as_i64()
        .ok_or("setup_model_response_invalid")?;
    tty.say(
        "Reading available models…",
        "사용 가능한 모델을 확인하고 있습니다…",
    )?;
    let catalog = api
        .request(
            Method::POST,
            &format!("/api/v1/admin/providers/{id}/models"),
            json!({"revision":rev}),
        )
        .await;
    let mut ids = catalog
        .ok()
        .and_then(|v| v["models"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v["id"].as_str().map(str::to_owned))
        .filter(|s| !s.chars().any(char::is_control))
        .collect::<Vec<_>>();
    // A preferred model is only recommended if this actual connection lists it.
    ids.sort_by_key(|s| (s != "gpt-6.1-terra", s.clone()));
    let model = if !ids.is_empty() {
        let filter = tty.input(
            "Filter model names (Enter shows the first choices)",
            "모델 이름 검색 (Enter로 추천 목록)",
            "",
        )?;
        let choices = ids
            .iter()
            .filter(|s| s.to_lowercase().contains(&filter.to_lowercase()))
            .take(8)
            .collect::<Vec<_>>();
        let mut labels = choices
            .iter()
            .map(|s| (s.as_str(), s.as_str()))
            .collect::<Vec<_>>();
        labels.push(("Enter a model ID manually", "모델 ID 직접 입력"));
        let n = tty.select("Default model", "기본 모델", &labels, 0)?;
        if n < choices.len() {
            choices[n].to_string()
        } else {
            tty.input("Exact model ID", "정확한 모델 ID", "")?
        }
    } else {
        tty.say("This server did not return a model list. Enter the exact model ID from its configuration.","서버에서 모델 목록을 받지 못했습니다. 해당 서버 설정의 정확한 모델 ID를 입력해주세요.")?;
        tty.input("Exact model ID", "정확한 모델 ID", "")?
    };
    let effort = tty.select(
        "Reasoning effort (only supported values will pass verification)",
        "추론 강도 (이 모델에서 지원하는 값만 확인을 통과합니다)",
        &[
            ("Provider default", "제공자 기본값"),
            ("low", "low"),
            ("medium", "medium"),
            ("high", "high"),
        ],
        if model == "gpt-6.1-terra" { 3 } else { 0 },
    )?;
    let effort = match effort {
        1 => Some("low"),
        2 => Some("medium"),
        3 => Some("high"),
        _ => None,
    };
    tty.say("Checking this model with a small request; API usage may be billed. No private data is sent.","짧은 요청으로 모델을 확인합니다. API 사용료가 발생할 수 있으며 개인 자료는 보내지 않습니다.")?;
    api.request(
        Method::POST,
        &format!("/api/v1/admin/providers/{id}/probe"),
        json!({"revision":rev,"model":model,"effort":effort}),
    )
    .await?;
    api.request(Method::PUT,"/api/v1/admin/providers/default",json!({"selection":{"provider_id":id,"provider_revision":rev,"model":model,"effort":effort}})).await?;
    tty.say("Default chat model verified and saved. You can verify tool support and change the model in Admin → Models.","기본 대화 모델을 확인하고 저장했습니다. 도구 지원 확인과 모델 변경은 관리자 → 모델에서 할 수 있습니다.")?;
    Ok(())
}
