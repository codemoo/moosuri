//! Native same-host helper. Authentication stays in memory; no token CLI arguments.
#[cfg(unix)]
mod private_state;
#[cfg(unix)]
mod vm;
use clap::Subcommand;
use reqwest::{Client, header};
use serde_json::{Value, json};
use std::time::Duration;
#[derive(Subcommand)]
pub enum Command {
    /// Continue with ChatGPT on this computer and transfer to your HTTPS server over SSH.
    #[command(name = "chatgpt-vm")]
    Vm {
        #[arg(long)]
        server: Option<String>,
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        ssh: Option<String>,
        #[arg(long, default_value = "My ChatGPT")]
        label: String,
        #[arg(long)]
        binding: Option<String>,
        #[arg(long)]
        revision: Option<i64>,
        /// Cancel an incomplete attempt and perform fresh OAuth using its saved registration.
        #[arg(long)]
        restart: bool,
    },
    /// Receive a protected handoff over SSH stdin using the running server's owner API.
    #[command(name = "chatgpt-receive")]
    Receive {
        #[arg(long)]
        stdin: bool,
    },
    /// Sign in on the computer running Moosuri and open the system browser.
    #[command(name = "chatgpt-login")]
    Login {
        #[arg(long)]
        server: String,
        #[arg(long)]
        email: String,
        #[arg(long, default_value = "My ChatGPT")]
        label: String,
        #[arg(long)]
        binding: Option<String>,
        #[arg(long)]
        revision: Option<i64>,
    },
}
pub async fn run(command: &Command) -> Result<(), String> {
    match command {
        #[cfg(unix)]
        Command::Vm {
            server,
            email,
            ssh,
            label,
            binding,
            revision,
            restart,
        } => return vm::run(server, email, ssh, label, binding, revision, *restart).await,
        #[cfg(unix)]
        Command::Receive { stdin } => return vm::receive(*stdin).await,
        Command::Login { .. } => (),
        #[cfg(not(unix))]
        _ => return Err("native_vm_helper_requires_unix".into()),
    }
    let Command::Login {
        server,
        email,
        label,
        binding,
        revision,
    } = command
    else {
        return Err("unsupported_subscription_command".into());
    };
    let url = url::Url::parse(server).map_err(|_| "invalid_local_server")?;
    if !["http", "https"].contains(&url.scheme())
        || url
            .host_str()
            .and_then(|h| h.parse::<std::net::IpAddr>().ok())
            .is_none_or(|a| !a.is_loopback())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("same_host_server_required".into());
    }
    let origin = url.origin().ascii_serialization();
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|_| "local_client_unavailable")?;
    let password =
        rpassword::prompt_password("Moosuri password: ").map_err(|_| "password_input_failed")?;
    let response = client
        .post(format!("{origin}/api/v1/auth/login"))
        .header(header::ORIGIN, &origin)
        .json(&json!({"email":email,"password":password}))
        .send()
        .await
        .map_err(|_| "local_login_failed")?;
    if !response.status().is_success() {
        return Err("local_login_failed".into());
    }
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(';').next())
        .ok_or("local_login_failed")?
        .to_owned();
    let body: Value = response.json().await.map_err(|_| "local_login_failed")?;
    let account = body
        .pointer("/user/id")
        .and_then(Value::as_str)
        .ok_or("local_login_failed")?;
    let response = client
        .post(format!("{origin}/api/v1/settings/chatgpt/local-login"))
        .header(header::ORIGIN, &origin)
        .header(header::COOKIE, &cookie)
        .header("x-moosuri-account", account)
        .json(&json!({"label":label,"binding_id":binding,"expected_revision":revision}))
        .send()
        .await
        .map_err(|_| "subscription_login_unavailable")?;
    if !response.status().is_success() {
        return Err("subscription_login_unavailable".into());
    }
    let launch: Value = response
        .json()
        .await
        .map_err(|_| "subscription_login_unavailable")?;
    let auth = launch["authorization_url"]
        .as_str()
        .ok_or("subscription_login_unavailable")?;
    let auth_url = url::Url::parse(auth).map_err(|_| "subscription_login_unavailable")?;
    if auth_url.scheme() != "https"
        || auth_url.host_str() != Some("auth.openai.com")
        || auth_url.path() != "/api/accounts/authorize"
    {
        return Err("subscription_login_unavailable".into());
    }
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    let status = tokio::process::Command::new(opener)
        .arg(auth)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| "system_browser_unavailable")?;
    if !status.success() {
        return Err("system_browser_unavailable".into());
    }
    let attempt = launch["attempt_id"]
        .as_str()
        .ok_or("subscription_login_unavailable")?;
    println!("Complete ChatGPT sign-in in your browser.");
    for _ in 0..300 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let response = client
            .get(format!(
                "{origin}/api/v1/settings/chatgpt/attempts/{attempt}"
            ))
            .header(header::COOKIE, &cookie)
            .header("x-moosuri-account", account)
            .send()
            .await
            .map_err(|_| "subscription_login_unavailable")?;
        if !response.status().is_success() {
            return Err("subscription_login_unavailable".into());
        }
        let phase: Value = response
            .json()
            .await
            .map_err(|_| "subscription_login_unavailable")?;
        match phase["phase"].as_str() {
            Some("completed") => {
                println!(
                    "ChatGPT connected. Verify and select an exact model in personal model settings."
                );
                return Ok(());
            }
            Some("pending" | "issued") => (),
            _ => return Err("subscription_login_incomplete".into()),
        }
    }
    Err("subscription_login_expired".into())
}
