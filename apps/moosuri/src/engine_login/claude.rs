//! Official Claude Code execution under the worker's non-root Linux identity.

use clap::Args;
use moosuri_connectors::{
    claude_code::{AccountModels, Profile},
    model::{
        CancellationToken,
        protocol::{ModelEvent, TextRequest},
    },
};
use moosuri_store::runtime_registry::engine::ModelChoice;
use serde::Deserialize;
#[cfg(test)]
use std::fs;
use std::{
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    profile_file: PathBuf,
    profile_digest: String,
    environment_id: Uuid,
    expected_email_file: PathBuf,
}

#[derive(Args)]
pub(crate) struct LoginOptions {
    /// Private operator profile for the dedicated non-root Linux worker.
    #[arg(long)]
    profile_file: PathBuf,
    /// Unique UUID for this owner's Claude Code environment.
    #[arg(long)]
    environment_id: Uuid,
    /// Private one-line file with this owner's exact Claude account email.
    #[arg(long)]
    expected_email_file: PathBuf,
}

#[derive(Args)]
pub(crate) struct IsolationOptions {
    /// Credential-free prepared profile, with an empty acceptance field.
    #[arg(long)]
    profile_file: PathBuf,
}

#[derive(Args)]
pub(crate) struct RetireOptions {
    #[arg(long)]
    config: PathBuf,
}
pub(crate) async fn retire(options: &RetireOptions) -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("claude_worker_retire_identity_invalid".into());
    }
    let config = super::read_config(&options.config)?;
    super::validate_config(&config)?;
    let super::Settings::ClaudeCode(settings) = config.adapter else {
        return Err("claude_worker_retire_identity_invalid".into());
    };
    let (profile, environment, _) = prepare(settings).await?;
    profile
        .retire_environment(&environment)
        .await
        .map_err(|_| "claude_worker_retire_unconfirmed".into())
}

pub(crate) async fn admit_isolation(options: &IsolationOptions) -> Result<(), String> {
    let bytes = private_file(&options.profile_file, 16_384)
        .map_err(|_| "claude_isolation_input_invalid")?;
    let raw = std::str::from_utf8(&bytes).map_err(|_| "claude_isolation_input_invalid")?;
    let cancel = CancellationToken::new();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "claude_isolation_signal_unavailable")?;
    let operation = Profile::admit_json(raw, &cancel);
    tokio::pin!(operation);
    let result = tokio::select! {
        result = &mut operation => result,
        _ = term.recv() => { cancel.cancel(); operation.await },
        _ = tokio::signal::ctrl_c() => { cancel.cancel(); operation.await },
    }
    .map_err(|_| "claude_isolation_not_accepted")?;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|_| "claude_isolation_output_failed")?
    );
    Ok(())
}

pub(super) struct Adapter {
    profile: Profile,
    environment: String,
    expected_email: Zeroizing<String>,
    choices: Vec<ModelChoice>,
}

impl Adapter {
    pub(super) async fn open(config: Config, choices: &[ModelChoice]) -> Result<Self, String> {
        let (profile, environment, expected_email) = prepare(config).await?;
        Self::bind(profile, environment, expected_email, choices).await
    }

    async fn bind(
        profile: Profile,
        environment: String,
        expected_email: Zeroizing<String>,
        choices: &[ModelChoice],
    ) -> Result<Self, String> {
        profile
            .cleanup_stale_environment(&environment)
            .await
            .map_err(|_| "claude_worker_profile_unavailable")?;
        let models = check_account(&profile, &environment, &expected_email).await?;
        if models.login_required {
            return Err("claude_worker_account_unavailable".into());
        }
        if !choices_available(&models, choices) {
            return Err("claude_worker_model_unavailable".into());
        }
        Ok(Self {
            profile,
            environment,
            expected_email,
            choices: choices.to_vec(),
        })
    }

    pub(super) fn validate(&self, request: &TextRequest) -> Result<(), String> {
        if !(1..=2048).contains(&request.max_output_tokens)
            || !self
                .choices
                .iter()
                .any(|choice| choice.model == request.model && choice.effort == request.effort)
        {
            return Err("claude_worker_request_invalid".into());
        }
        self.profile
            .validate_text_with_images(request, &[])
            .map_err(|_| "claude_worker_request_invalid".into())
    }

    pub(super) async fn stream(
        &self,
        request: &TextRequest,
        tx: &mpsc::Sender<ModelEvent>,
        cancel: &CancellationToken,
    ) -> Result<(), String> {
        self.validate(request)?;
        self.profile
            .stream_text_for_email(&self.environment, &self.expected_email, request, tx, cancel)
            .await
            .map_err(|_| "claude_worker_execution_failed".into())
    }
}

/// Shared private-config binding checks for the worker and the login bridge.
async fn prepare(config: Config) -> Result<(Profile, String, Zeroizing<String>), String> {
    if config.environment_id.is_nil() || config.environment_id.get_version_num() == 8 {
        return Err("claude_worker_environment_invalid".into());
    }
    let expected_email = read_expected_email(&config.expected_email_file)?;
    let profile = load_profile(config.profile_file).await?;
    if profile.digest() != config.profile_digest {
        return Err("claude_worker_profile_changed".into());
    }
    Ok((profile, config.environment_id.to_string(), expected_email))
}

/// Every selected model and effort must be present in the verified catalog.
fn choices_available(models: &AccountModels, choices: &[ModelChoice]) -> bool {
    choices.iter().all(|choice| {
        models.models.contains(&choice.model)
            && !choice.effort.as_ref().is_some_and(|effort| {
                !models.effort_metadata.iter().any(|metadata| {
                    metadata.model == choice.model
                        && metadata
                            .efforts
                            .as_ref()
                            .is_some_and(|levels| levels.contains(effort))
                })
            })
    })
}

pub(crate) async fn login(options: &LoginOptions) -> Result<(), String> {
    if options.environment_id.is_nil() || options.environment_id.get_version_num() == 8 {
        return Err("claude_worker_environment_invalid".into());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() || !io::stderr().is_terminal() {
        return Err("claude_worker_login_terminal_required".into());
    }
    let profile = load_profile(options.profile_file.clone()).await?;
    let expected_email = read_expected_email(&options.expected_email_file)?;
    let environment = options.environment_id.to_string();
    profile
        .cleanup_stale_environment(&environment)
        .await
        .map_err(|_| "claude_worker_profile_unavailable")?;
    let models = check_account(&profile, &environment, &expected_email).await;
    let models = if models.as_ref().is_ok_and(|models| !models.login_required) {
        models?
    } else {
        profile
            .login(&environment)
            .await
            .map_err(|_| "claude_worker_login_failed")?;
        check_account(&profile, &environment, &expected_email).await?
    };
    if models.login_required || models.models.is_empty() {
        return Err("claude_worker_account_unavailable".into());
    }
    println!(
        "{}",
        serde_json::json!({
            "profile_digest": profile.digest(),
            "environment_id": options.environment_id,
            "models": models.models,
            "effort_metadata": models.effort_metadata.iter().map(|metadata| serde_json::json!({
                "model": metadata.model,
                "efforts": metadata.efforts,
            })).collect::<Vec<_>>(),
        })
    );
    Ok(())
}

async fn load_profile(path: PathBuf) -> Result<Profile, String> {
    tokio::task::spawn_blocking(move || {
        let bytes = private_file(&path, 16_384).map_err(|_| "claude_worker_profile_invalid")?;
        let profile = std::str::from_utf8(&bytes).map_err(|_| "claude_worker_profile_invalid")?;
        Profile::from_json(profile).map_err(|_| "claude_worker_profile_invalid".to_owned())
    })
    .await
    .map_err(|_| "claude_worker_profile_unavailable")?
}

async fn check_account(
    profile: &Profile,
    environment: &str,
    email: &str,
) -> Result<AccountModels, String> {
    profile
        .account_models_for_email(environment, email, &CancellationToken::new())
        .await
        .map_err(|_| "claude_worker_catalog_unavailable".into())
}

fn read_expected_email(path: &Path) -> Result<Zeroizing<String>, String> {
    let bytes = private_file(path, 256).map_err(|_| "claude_worker_email_file_invalid")?;
    let email = std::str::from_utf8(&bytes).map_err(|_| "claude_worker_email_file_invalid")?;
    let email = email
        .strip_suffix("\r\n")
        .or_else(|| email.strip_suffix('\n'))
        .unwrap_or(email);
    if email.len() > 254
        || !email.bytes().all(|b| b.is_ascii_graphic())
        || !email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
    {
        return Err("claude_worker_email_file_invalid".into());
    }
    Ok(Zeroizing::new(email.to_owned()))
}

fn private_file(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, ()> {
    let file = super::protected_input(path, limit as u64).map_err(|_| ())?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > limit {
        return Err(());
    }
    Ok(bytes)
}

mod login_bridge;
pub(crate) use login_bridge::{BridgeOptions as LoginBridgeOptions, run as login_bridge};

#[cfg(test)]
mod tests;
