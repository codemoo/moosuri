use clap::Parser;
use moosuri_vault_service::{
    database::Database,
    http::{Adapter, Config, router},
};
use std::{
    io::Read,
    net::SocketAddr,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};
#[derive(Parser)]
#[command(about = "Separate-origin client-encrypted vault; never accepts unlock secrets")]
struct Args {
    #[arg(long)]
    data_dir: PathBuf,
    #[arg(long, default_value = "127.0.0.1:3220")]
    bind: SocketAddr,
    #[arg(long)]
    origin: String,
    #[arg(long)]
    assistant_origin: String,
    #[arg(long, default_value = "http://127.0.0.1:3211/internal/v1/vault")]
    adapter_url: String,
    #[arg(long)]
    adapter_key_file: PathBuf,
    #[arg(long)]
    dev: bool,
}
fn private_key(path: &std::path::Path) -> Result<String, &'static str> {
    let m = std::fs::symlink_metadata(path).map_err(|_| "vault_key_file_unavailable")?;
    // SAFETY: geteuid has no preconditions and returns only the current process identity.
    let uid = unsafe { libc::geteuid() };
    if m.uid() != uid
        || !m.is_file()
        || m.file_type().is_symlink()
        || m.permissions().mode() & 0o077 != 0
        || m.len() > 128
    {
        return Err("vault_private_key_file_required");
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "vault_key_file_unavailable")?;
    if f.metadata()
        .map_err(|_| "vault_key_file_unavailable")?
        .ino()
        != m.ino()
    {
        return Err("vault_key_file_changed");
    }
    let mut value = zeroize::Zeroizing::new(String::new());
    f.read_to_string(&mut value)
        .map_err(|_| "vault_key_file_unavailable")?;
    Ok(value.trim_end_matches(['\r', '\n']).into())
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("vault_service_failed");
        std::process::exit(1)
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if !args.bind.ip().is_loopback() {
        return Err("loopback_bind_required_use_tls_proxy".into());
    }
    if !moosuri_vault_service::http::has_assets() {
        return Err("build_vault_web_before_rust_binary".into());
    }
    let config = Config::new(&args.origin, &args.assistant_origin, args.dev)?;
    let adapter = Adapter::new(&args.adapter_url, private_key(&args.adapter_key_file)?)?;
    let db = Database::open(&args.data_dir)?;
    let app = router(db, adapter, config);
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
