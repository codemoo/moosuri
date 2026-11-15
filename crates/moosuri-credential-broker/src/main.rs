//! Trusted-local experimental companion. Passwords/tokens never use argv or stdout.
use clap::Parser;
use moosuri_credential_broker::{
    Engine, Error, auth::SigningKey, github::GitHubAdapter, transport::Transport,
};
use moosuri_vault_core::{SecretText, UnlockedArchive};
use moosuri_vault_protocol::broker::*;
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};
use tokio::io::AsyncBufReadExt;
use zeroize::Zeroizing;
#[derive(Parser)]
#[command(about = "Experimental trusted-device credential reads; no signed distribution")]
struct Args {
    #[arg(long)]
    state_dir: PathBuf,
    #[arg(long)]
    origin: String,
    #[arg(long)]
    pair: bool,
    /// Exact independent HTTPS vault origin for native live mode.
    #[arg(long)]
    vault_origin: Option<String>,
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|n| n.as_secs() as i64)
        .unwrap_or(i64::MAX)
}
fn password(label: &str) -> Result<SecretText, Error> {
    use std::os::fd::AsRawFd;
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| Error::PrivateState)?;
    let fd = tty.as_raw_fd();
    let mut old = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, old.as_mut_ptr()) } != 0 {
        return Err(Error::PrivateState);
    }
    let old = unsafe { old.assume_init() };
    let mut hidden = old;
    hidden.c_lflag &= !libc::ECHO;
    struct Restore(i32, libc::termios);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(self.0, libc::TCSAFLUSH, &self.1);
            }
        }
    }
    if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &hidden) } != 0 {
        return Err(Error::PrivateState);
    }
    let _restore = Restore(fd, old);
    write!(tty, "{label}: ").map_err(|_| Error::PrivateState)?;
    tty.flush().map_err(|_| Error::PrivateState)?;
    let mut secret = Zeroizing::new(String::new());
    let mut reader = io::BufReader::new(&tty);
    reader
        .read_line(&mut secret)
        .map_err(|_| Error::PrivateState)?;
    writeln!(tty).map_err(|_| Error::PrivateState)?;
    let trimmed = secret.trim_end_matches(['\r', '\n']).to_owned();
    SecretText::new(trimmed).map_err(|_| Error::Policy)
}
async fn run() -> Result<(), Error> {
    let args = Args::parse();
    let transport = Transport::new(&args.origin)?;
    let mut input = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut engine = if args.pair {
        let key = Zeroizing::new(SigningKey::generate_pkcs8()?);
        let signer = SigningKey::from_pkcs8(&key)?;
        println!(
            "Public key (paste into the assistant's trusted-device connection): {}",
            signer
                .public_key()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        println!(
            "Paste the owner challenge JSON from your assistant at {}:",
            args.origin
        );
        let line = input
            .next_line()
            .await
            .map_err(|_| Error::PrivateState)?
            .ok_or(Error::Authentication)?;
        if line.len() > 16384 {
            return Err(Error::Policy);
        }
        let challenge: PairChallenge =
            serde_json::from_str(&line).map_err(|_| Error::Authentication)?;
        if challenge.origin != args.origin
            || challenge.public_key != signer.public_key()
            || challenge.expires_at <= now()
        {
            return Err(Error::Authentication);
        }
        let proof = PairProof {
            challenge: challenge.nonce,
            signature: signer.sign(&pairing_bytes(&challenge).ok_or(Error::Authentication)?),
        };
        // Persist identity BEFORE network pairing, so a lost response never creates
        // a second key/device. An uncertain pairing is local recovery, not reset.
        let engine = Engine::enroll(
            &args.state_dir,
            challenge.device.clone(),
            args.origin.clone(),
            &key,
        )?;
        let device: Device = transport
            .post("/api/v1/credential-broker/device/pair", &proof)
            .await?;
        if device != challenge.device {
            return Err(Error::Authentication);
        }
        engine
    } else {
        Engine::open_local(&args.state_dir)?
    };
    if engine.origin() != args.origin {
        return Err(Error::Authentication);
    }
    let adapter = GitHubAdapter::new()?;
    enum Vault {
        Archive(UnlockedArchive),
        Live(moosuri_credential_broker::live_vault::LiveVault),
    }
    let mut vault: Option<Vault> = None;
    println!(
        "Commands: connect-live ACCOUNT_EMAIL | unlock ABSOLUTE_ARCHIVE_PATH (snapshot) | items | select ITEM_UUID OWNER REPO | select-google ITEM_UUID GOOGLE_SUB EMAIL | select-google-preview ITEM_UUID GOOGLE_SUB EMAIL | select-google-body ITEM_UUID GOOGLE_SUB EMAIL | select-google-send ITEM_UUID GOOGLE_SUB EMAIL | poll | lock | quit"
    );
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        if engine.is_revoked() {
            vault = None;
        }
        let line = tokio::select! {
            value=input.next_line()=>value.map_err(|_|Error::PrivateState)?,
            _=interval.tick(),if vault.is_some()=>{
                tokio::select! {
                    result=async {match vault.as_mut().unwrap(){Vault::Archive(v)=>transport.poll(&mut engine,v,&adapter).await,Vault::Live(v)=>transport.poll_live(&mut engine,v,&adapter).await}}=>{
                        if let Ok(n)=result && n>0 {println!("Processed {n} original requests.");}
                        if matches!(result,Err(Error::Authentication|Error::Reconnect)) {if matches!(vault,Some(Vault::Live(_))){vault=None;println!("Live connection retired; reconnect locally.");}}
                        else if matches!(result,Err(Error::Unavailable)){println!("Connection unavailable; dispatch suspended until fresh validation succeeds.");}
                        if matches!(vault.as_ref(),Some(Vault::Live(v)) if !v.is_unlocked()){vault=None;}
                        continue;
                    },
                    value=input.next_line()=>value.map_err(|_|Error::PrivateState)?,
                }
            }
        };
        let Some(line) = line else { break };
        let mut parts = line.split_whitespace();
        let command = parts.next().unwrap_or("");
        let result: Result<(), Error> = async {
            match command {
                "connect-live" => {
                    vault=None;
                    let email=parts.next().ok_or(Error::Policy)?;
                    let origin=args.vault_origin.as_deref().ok_or(Error::Policy)?;
                    let account_password=password("Assistant account password (native login)")?;
                    let mut live=moosuri_credential_broker::live_vault::LiveVault::login(&args.origin,origin,engine.device().owner.clone(),email,&account_password).await?;
                    drop(account_password);
                    let master=password("Vault master password (local decrypt only)")?;
                    live.unlock(&master).await?;drop(master);
                    vault=Some(Vault::Live(live));
                    println!("Live vault unlocked only on this device. Select the exact GitHub repository or Google account locally.");
                }
                "unlock" => {
                    vault = None;
                    let path = PathBuf::from(parts.next().ok_or(Error::Policy)?);
                    let bytes = moosuri_credential_broker::read_private_archive(&path)?;
                    let secret = password("Vault archive password (snapshot only)")?;
                    let unlocked =
                        UnlockedArchive::unlock(&bytes, &secret).map_err(|_| Error::Policy)?;
                    if unlocked.header().binding.owner_id != engine.device().owner.account
                        || unlocked.header().binding.tenant_id != engine.device().owner.tenant
                    {
                        return Err(Error::Policy);
                    }
                    vault = Some(Vault::Archive(unlocked));
                    println!("Unlocked on this device only.");
                }
                "items" => {
                    match vault.as_mut().ok_or(Error::Locked)? {
                        Vault::Archive(v)=>{let ids=v.items().iter().map(|i|i.id).collect::<Vec<_>>();for id in ids{let item=v.open(id).map_err(|_|Error::Policy)?;if item.kind==moosuri_vault_core::ItemKind::ApiKey{println!("{} {}",id,item.title.expose());}}}
                        Vault::Live(v)=>{for(id,title)in v.items().await?{println!("{id} {title}");}}
                    }
                }
                "select" => {
                    let id = parts
                        .next()
                        .ok_or(Error::Policy)?
                        .parse()
                        .map_err(|_| Error::Policy)?;
                    let repository = GitHubRepository {
                        owner: parts.next().ok_or(Error::Policy)?.into(),
                        repository: parts.next().ok_or(Error::Policy)?.into(),
                    };
                    match vault.as_mut().ok_or(Error::Locked)? {
                        Vault::Archive(v)=>{
                            let binding=engine.archive_grant(v,id,repository.clone(),now()+30*86400)?;
                            transport.command(&engine,Command::RegisterRepository{binding,repository}).await?;
                        }
                        Vault::Live(v)=>{
                            // Resolve all uncertain older preparations before changing
                            // a local version; their original identity is never replayed.
                            transport.sync_retained(&mut engine).await?;
                            let binding=engine.live_grant(v,id,repository.clone(),now()+30*86400).await?;
                            transport.register_live(&mut engine,&binding,&repository).await?;
                        }
                    }
                    println!(
                        "Exact repository connected; compatible requests reuse this selection."
                    );
                }
                operation @ ("select-google" | "select-google-preview" | "select-google-body" | "select-google-send")=>{
                    let preview=operation=="select-google-preview"; let body=operation=="select-google-body"; let send=operation=="select-google-send";
                    let id=parts.next().ok_or(Error::Policy)?.parse().map_err(|_|Error::Policy)?;
                    let identity=moosuri_credential_broker::google::GoogleIdentity{subject:parts.next().ok_or(Error::Policy)?.into(),email:parts.next().ok_or(Error::Policy)?.into()};
                    if parts.next().is_some(){return Err(Error::Policy)}
                    transport.sync_retained(&mut engine).await?;
                    let binding=match vault.as_mut().ok_or(Error::Locked)?{
                        Vault::Archive(v)=>if send {engine.archive_google_send_grant(v,id,identity,now()+30*86400)?} else if body { engine.archive_google_body_grant(v,id,identity,now()+30*86400)? } else if preview {engine.archive_google_preview_grant(v,id,identity,now()+30*86400)?} else {engine.archive_google_grant(v,id,identity,now()+30*86400)?},
                        Vault::Live(v)=>{if send {engine.live_google_send_grant(v,id,identity,now()+30*86400).await?} else if body { engine.live_google_body_grant(v,id,identity,now()+30*86400).await? } else if preview {engine.live_google_preview_grant(v,id,identity,now()+30*86400).await?} else {engine.live_google_grant(v,id,identity,now()+30*86400).await?}}
                    };
                    transport.register_google(&mut engine,&binding).await?;
                    println!("Google account selected for {} only; compatible requests reuse this local grant. Provider identity and exact actual selected scopes are checked each time.",if send {"final-approved plaintext mail send and original Sent reconciliation"} else if body {"selected INBOX plaintext body"} else if preview {"bounded INBOX preview"} else {"INBOX counts"});
                }
                "poll" => {
                    if vault.is_none() {
                        return Err(Error::Locked);
                    }
                    interval.reset_immediately();
                }
                "lock" => {
                    vault = None;
                    println!("Locked. Waiting requests remain on the assistant.");
                }
                "quit" => {
                    vault = None;
                }
                _ => return Err(Error::Policy),
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            if matches!(error, Error::Authentication | Error::Reconnect)
                && matches!(vault, Some(Vault::Live(_)))
            {
                vault = None;
                println!("Live connection retired; reconnect locally.");
            }
            eprintln!("{error}");
        }
        if command == "quit" {
            break;
        }
    }
    drop(vault);
    Ok(())
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1)
    }
}
