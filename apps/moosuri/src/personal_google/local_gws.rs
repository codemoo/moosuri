//! Member-local readonly Gmail. The OAuth connector owns code exchange and
//! refresh. Only an access token goes into a short-lived gws process.
use super::{Failure, Inbox, Operation, Secret, StatusCode, decode, hex64};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::try_join_all;
use moosuri_connectors::{
    gmail::{
        GmailClient, InboxMetadata, MailboxProfile, OAuthClient, READ_SCOPE, SecretString, TokenSet,
    },
    model::CancellationToken,
};
use moosuri_store::{MemberGoogleInboxMessage, personal_volume::VolumeOwner};
use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::RawValue};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{io::AsyncReadExt, process::Command};
use uuid::Uuid;
use zeroize::Zeroizing;

const CONFIG_LIMIT: usize = 32 * 1024;
const OUTPUT_LIMIT: usize = 128 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const REFRESH_LIFETIME: u64 = 400 * 24 * 3600;
const PAGE_SIZE: usize = 5;
const DISCOVERY_PATH: &str = "/usr/local/share/moosuri/gmail-v1.json";
const DISCOVERY_LIMIT: usize = 2 * 1024 * 1024;
const DISCOVERY_DIGEST: [u8; 32] = [
    0x83, 0x9b, 0x79, 0x63, 0xdf, 0x95, 0x48, 0x1f, 0x3c, 0xd2, 0xba, 0x7a, 0xa1, 0xe9, 0x06, 0xf1,
    0xfc, 0x67, 0xc4, 0xae, 0x89, 0x71, 0x9d, 0x09, 0xbb, 0x73, 0x05, 0x04, 0x0e, 0x99, 0xf0, 0xfb,
];

pub(super) struct LocalGws {
    revision: Uuid,
    app: OAuthClient,
    oauth: GmailClient,
    key: hmac::Key,
    executable: PathBuf,
    proxy: String,
    directory: tempfile::TempDir,
    discovery_cache: File,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config<'a> {
    app_revision: Uuid,
    redirect_uri: &'a str,
    web_client: &'a RawValue,
    capability_key: SecretString,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    account_id: Uuid,
    tenant_id: Uuid,
    volume_id: Uuid,
    generation: i64,
    peer_uid: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    v: u32,
    owner: Owner,
    app_revision: Uuid,
    purpose: String,
    mailbox: String,
    token_tag: String,
    expires_at: u64,
}

#[derive(Serialize)]
struct Tokens<'a> {
    access_token: Secret<'a>,
    refresh_token: Option<Secret<'a>>,
    expires_in: u64,
    scopes: [&'static str; 1],
    mailbox: String,
    access_binding: String,
    refresh_binding: String,
}

#[derive(Serialize)]
struct Reply<T> {
    v: u32,
    request_id: Uuid,
    owner: Owner,
    app_revision: Uuid,
    state: &'static str,
    result: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Page {
    #[serde(default)]
    messages: Vec<MessageRef>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageRef {
    id: String,
}

enum GmailRead<'a> {
    Profile,
    Inbox,
    Metadata(&'a str),
}

fn unavailable() -> Failure {
    Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable")
}

fn config_bytes(path: &Path) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let invalid = "member_google_config_invalid";
    if !path.is_absolute() {
        return Err(invalid);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => "google_setup_required",
            _ => invalid,
        })?;
    let metadata = file.metadata().map_err(|_| invalid)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
        || metadata.len() > CONFIG_LIMIT as u64
    {
        return Err(invalid);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take((CONFIG_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid)?;
    if bytes.len() > CONFIG_LIMIT {
        return Err(invalid);
    }
    Ok(bytes)
}

fn executable_path(path: &Path) -> Result<PathBuf, &'static str> {
    let invalid = "member_gws_executable_invalid";
    if !path.is_absolute() {
        return Err(invalid);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid)?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o6022 != 0
        || metadata.mode() & 0o111 == 0
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Err(invalid);
    }
    let mut magic = [0; 4];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .map_err(|_| invalid)?;
    if magic != [0x7f, b'E', b'L', b'F'] {
        return Err(invalid);
    }
    Ok(path.to_path_buf())
}

fn private_directory() -> Result<tempfile::TempDir, &'static str> {
    let invalid = "member_gws_private_directory_unavailable";
    let directory = tempfile::Builder::new()
        .prefix("member-gws-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .map_err(|_| invalid)?;
    // Upstream dotenvy searches parent directories for .env. Stop the search
    // inside our empty private cwd, before any ancestor can supply credentials.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.path().join(".env"))
        .map_err(|_| invalid)?;
    Ok(directory)
}

fn discovery_bytes() -> Result<Vec<u8>, &'static str> {
    let invalid = "member_gws_discovery_invalid";
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(DISCOVERY_PATH)
        .map_err(|_| invalid)?;
    let metadata = file.metadata().map_err(|_| invalid)?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o7777 != 0o444
        || metadata.len() > DISCOVERY_LIMIT as u64
    {
        return Err(invalid);
    }
    let mut bytes = Vec::new();
    file.take((DISCOVERY_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid)?;
    if bytes.len() > DISCOVERY_LIMIT || Sha256::digest(&bytes).as_slice() != DISCOVERY_DIGEST {
        return Err(invalid);
    }
    Ok(bytes)
}

fn seed_discovery(directory: &Path, bytes: &[u8]) -> Result<File, &'static str> {
    let invalid = "member_gws_discovery_cache_unavailable";
    let directory = directory.join("cache");
    fs::create_dir(&directory).map_err(|_| invalid)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join("gmail_v1.json"))
        .map_err(|_| invalid)?;
    file.write_all(bytes).map_err(|_| invalid)?;
    Ok(file)
}

fn now() -> Result<u64, Failure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| unavailable())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}

pub(crate) fn registered_client(
    config_path: &Path,
) -> Result<(Uuid, OAuthClient, hmac::Key), &'static str> {
    let invalid = "member_google_config_invalid";
    let bytes = config_bytes(config_path)?;
    let config: Config<'_> = serde_json::from_slice(&bytes).map_err(|_| invalid)?;
    let redirect = url::Url::parse(config.redirect_uri).map_err(|_| invalid)?;
    if config.app_revision.is_nil()
        || redirect.scheme() != "https"
        || redirect.path() != "/api/v1/mail/oauth/callback"
    {
        return Err(invalid);
    }
    let app = OAuthClient::from_web_client_json(config.web_client.get(), config.redirect_uri)
        .map_err(|_| invalid)?;
    let key = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(config.capability_key.expose())
            .map_err(|_| invalid)?,
    );
    if key.len() != 32 {
        return Err(invalid);
    }
    Ok((
        config.app_revision,
        app,
        hmac::Key::new(hmac::HMAC_SHA256, &key),
    ))
}

impl LocalGws {
    pub(super) fn load(
        config_path: &Path,
        executable: &Path,
        proxy: SocketAddr,
    ) -> Result<Self, &'static str> {
        let (revision, app, key) = registered_client(config_path)?;
        let directory = private_directory()?;
        let discovery_cache = seed_discovery(directory.path(), &discovery_bytes()?)?;
        Ok(Self {
            revision,
            app,
            oauth: GmailClient::google_oauth_proxy(proxy)
                .map_err(|_| "member_google_proxy_invalid")?,
            key,
            executable: executable_path(executable)?,
            proxy: format!("http://{proxy}"),
            directory,
            discovery_cache,
        })
    }

    pub(super) async fn raw(
        &self,
        owner: VolumeOwner,
        id: Uuid,
        revision: Option<Uuid>,
        operation: Operation<'_>,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        if id.is_nil()
            || owner.account.is_nil()
            || owner.tenant.is_nil()
            || owner.volume.is_nil()
            || !(1..=i64::MAX as u64).contains(&owner.generation)
            || revision != (!matches!(&operation, Operation::Describe)).then_some(self.revision)
        {
            return Err(unavailable());
        }
        let owner = Owner {
            account_id: owner.account,
            tenant_id: owner.tenant,
            volume_id: owner.volume,
            generation: owner.generation as i64,
            peer_uid: unsafe { libc::geteuid() },
        };
        match operation {
            Operation::Describe => self.reply(
                owner,
                id,
                json!({
                    "client_id": self.app.client_id,
                    "redirect_uri": self.app.redirect_uri,
                    "scope": READ_SCOPE,
                    "operations": ["authorization_url", "exchange", "refresh", "inbox_page"],
                }),
            ),
            Operation::AuthorizationUrl { state, verifier } => {
                if !hex64(state) {
                    return Err(unavailable());
                }
                let authorization_url = GmailClient::authorization_url_scoped(
                    &self.app.client_id,
                    &self.app.redirect_uri,
                    state,
                    verifier.0.expose(),
                    &[READ_SCOPE],
                )
                .map_err(|_| unavailable())?;
                self.reply(owner, id, json!({"authorization_url": authorization_url}))
            }
            Operation::Exchange { code, verifier } => {
                let tokens = self
                    .oauth
                    .exchange_code(&self.app, code.0, verifier.0, &CancellationToken::new())
                    .await
                    .map_err(|_| unavailable())?;
                self.tokens(owner, id, tokens, None).await
            }
            Operation::Refresh {
                refresh_token,
                refresh_binding,
            } => {
                let previous = refresh_token.0;
                let expected_mailbox = self.verify(owner, "refresh", refresh_binding, previous)?;
                let tokens = self
                    .oauth
                    .refresh(&self.app, previous, &CancellationToken::new())
                    .await
                    .map_err(|_| unavailable())?;
                self.tokens(owner, id, tokens, Some((previous, &expected_mailbox)))
                    .await
            }
            Operation::InboxPage {
                access_token,
                access_binding,
            } => {
                let mailbox = self.verify(owner, "access", access_binding, access_token.0)?;
                let page = self.inbox(mailbox, access_token.0).await?;
                self.reply(owner, id, page)
            }
        }
    }

    fn reply<T: Serialize>(
        &self,
        owner: Owner,
        id: Uuid,
        result: T,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let reply = Reply {
            v: 1,
            request_id: id,
            owner,
            app_revision: self.revision,
            state: "ok",
            result,
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&reply).map_err(|_| unavailable())?);
        if bytes.len() > OUTPUT_LIMIT {
            return Err(unavailable());
        }
        Ok(bytes)
    }

    fn token_tag(&self, token: &SecretString) -> String {
        URL_SAFE_NO_PAD.encode(hmac::sign(&self.key, token.expose().as_bytes()).as_ref())
    }

    fn bind(
        &self,
        owner: Owner,
        purpose: &str,
        mailbox: &str,
        token: &SecretString,
        expires_at: u64,
    ) -> Result<String, Failure> {
        let claim = Binding {
            v: 1,
            owner,
            app_revision: self.revision,
            purpose: purpose.into(),
            mailbox: mailbox.into(),
            token_tag: self.token_tag(token),
            expires_at,
        };
        let payload =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claim).map_err(|_| unavailable())?);
        let signature = URL_SAFE_NO_PAD.encode(hmac::sign(&self.key, payload.as_bytes()).as_ref());
        Ok(format!("{payload}.{signature}"))
    }

    fn verify(
        &self,
        owner: Owner,
        purpose: &str,
        receipt: &str,
        token: &SecretString,
    ) -> Result<String, Failure> {
        if receipt.len() > 4096 {
            return Err(unavailable());
        }
        let (payload, signature) = receipt.split_once('.').ok_or_else(unavailable)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| unavailable())?;
        hmac::verify(&self.key, payload.as_bytes(), &signature).map_err(|_| unavailable())?;
        let claim: Binding =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).map_err(|_| unavailable())?)
                .map_err(|_| unavailable())?;
        let tag = URL_SAFE_NO_PAD
            .decode(claim.token_tag)
            .map_err(|_| unavailable())?;
        if claim.v != 1
            || claim.owner != owner
            || claim.app_revision != self.revision
            || claim.purpose != purpose
            || claim.expires_at <= now()?
            || hmac::verify(&self.key, token.expose().as_bytes(), &tag).is_err()
        {
            return Err(unavailable());
        }
        Ok(claim.mailbox)
    }

    async fn tokens(
        &self,
        owner: Owner,
        id: Uuid,
        tokens: TokenSet,
        prior: Option<(&SecretString, &str)>,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        // Google may omit the scope on refresh. The authenticated prior grant
        // proves in that case that the unchanged scope is Gmail read-only.
        let readonly = match tokens.scopes.as_deref() {
            Some([scope]) => scope == READ_SCOPE,
            None => prior.is_some(),
            _ => false,
        };
        if !readonly || !(1..=86400).contains(&tokens.expires_in) {
            return Err(unavailable());
        }
        let refresh = tokens
            .refresh_token
            .as_ref()
            .or(prior.map(|(token, _)| token))
            .ok_or_else(unavailable)?;
        let created_at = now()?;
        let bytes = self.read(GmailRead::Profile, &tokens.access_token).await?;
        let profile = MailboxProfile::from_gmail_json(&bytes).map_err(|_| unavailable())?;
        let mailbox = profile.email_address;
        if prior.is_some_and(|(_, expected)| !expected.eq_ignore_ascii_case(&mailbox)) {
            return Err(unavailable());
        }
        let access_binding = self.bind(
            owner,
            "access",
            &mailbox,
            &tokens.access_token,
            created_at + tokens.expires_in,
        )?;
        let refresh_binding = self.bind(
            owner,
            "refresh",
            &mailbox,
            refresh,
            created_at + REFRESH_LIFETIME,
        )?;
        self.reply(
            owner,
            id,
            Tokens {
                access_token: Secret(&tokens.access_token),
                refresh_token: tokens.refresh_token.as_ref().map(Secret),
                expires_in: tokens.expires_in,
                scopes: [READ_SCOPE],
                mailbox,
                access_binding,
                refresh_binding,
            },
        )
    }

    async fn inbox(&self, mailbox: String, token: &SecretString) -> Result<Inbox, Failure> {
        let bytes = self.read(GmailRead::Inbox, token).await?;
        let page: Page = decode(&bytes).map_err(|_| unavailable())?;
        if page.messages.len() > PAGE_SIZE
            || page
                .next_page_token
                .as_ref()
                .is_some_and(|p| p.is_empty() || p.len() > 2048 || p.chars().any(char::is_control))
        {
            return Err(unavailable());
        }
        let mut seen = BTreeSet::new();
        if page
            .messages
            .iter()
            .any(|m| !valid_id(&m.id) || !seen.insert(&m.id))
        {
            return Err(unavailable());
        }
        let mut messages = try_join_all(page.messages.iter().map(|item| async {
            let bytes = self.read(GmailRead::Metadata(&item.id), token).await?;
            let message =
                InboxMetadata::from_gmail_json(&item.id, &bytes).map_err(|_| unavailable())?;
            Ok::<_, Failure>(MemberGoogleInboxMessage {
                id: message.id,
                received_at_ms: message.received_at_ms,
                subject: message.subject,
                from: message.from,
                date: message.date,
            })
        }))
        .await?;
        messages.sort_by(|a, b| {
            b.received_at_ms
                .cmp(&a.received_at_ms)
                .then_with(|| b.id.cmp(&a.id))
        });
        let page = Inbox {
            mailbox,
            messages,
            has_more: page.next_page_token.is_some(),
            scope: "one_inbox_page_metadata_only".into(),
        };
        if !page.valid() {
            return Err(unavailable());
        }
        Ok(page)
    }

    async fn read(
        &self,
        read: GmailRead<'_>,
        token: &SecretString,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        if matches!(&read, GmailRead::Profile | GmailRead::Inbox) {
            // gws 0.22.0 considers the cached schema stale after 24 hours.
            // Keep the image-verified file fresh before the first command of
            // a read so follow-up metadata calls use the same schema.
            self.discovery_cache
                .set_modified(SystemTime::now())
                .map_err(|_| unavailable())?;
        }
        let (args, params): (&[&str], Value) = match read {
            GmailRead::Profile => (
                &["gmail", "users", "getProfile"],
                json!({"userId": "me", "fields": "emailAddress"}),
            ),
            GmailRead::Inbox => (
                &["gmail", "users", "messages", "list"],
                json!({
                    "userId": "me", "labelIds": ["INBOX"], "includeSpamTrash": false,
                    "maxResults": PAGE_SIZE, "fields": "messages(id),nextPageToken",
                }),
            ),
            GmailRead::Metadata(id) => (
                &["gmail", "users", "messages", "get"],
                json!({
                    "userId": "me", "id": id, "format": "metadata", "metadataHeaders": ["Subject", "From", "Date"],
                    "fields": "id,labelIds,internalDate,payload(headers)",
                }),
            ),
        };
        let private = self.directory.path();
        let mut cmd = Command::new(&self.executable);
        cmd.env_clear()
            .env("HOME", private)
            .env("XDG_CONFIG_HOME", private)
            .env("TMPDIR", private)
            .env("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", private)
            .env("GOOGLE_WORKSPACE_CLI_TOKEN", token.expose())
            .env("HTTPS_PROXY", &self.proxy)
            .current_dir(private)
            .args(args)
            .arg("--params")
            .arg(params.to_string())
            .args(["--format", "json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = cmd.spawn().map_err(|_| unavailable())?;
        let mut group = ProcessGroup(child.id());
        tokio::time::timeout(COMMAND_TIMEOUT, async {
            let mut output = Zeroizing::new(Vec::new());
            child
                .stdout
                .take()
                .ok_or_else(unavailable)?
                .take((OUTPUT_LIMIT + 1) as u64)
                .read_to_end(&mut output)
                .await
                .map_err(|_| unavailable())?;
            if output.len() > OUTPUT_LIMIT {
                return Err(unavailable());
            }
            let status = child.wait().await.map_err(|_| unavailable())?;
            group.0 = None;
            if !status.success() {
                return Err(unavailable());
            }
            Ok(output)
        })
        .await
        .map_err(|_| unavailable())?
    }
}

struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            let _ = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        }
    }
}

#[cfg(test)]
mod tests;
