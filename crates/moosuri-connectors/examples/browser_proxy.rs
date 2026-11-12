use moosuri_connectors::browser::proxy;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let bind =
        std::env::var("MOOSURI_BROWSER_PROXY_BIND").unwrap_or_else(|_| "0.0.0.0:3128".into());
    let result = async {
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|_| "proxy listener unavailable")?;
        let cancel = CancellationToken::new();
        let control = cancel.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            control.cancel();
        });
        proxy::serve(listener, cancel)
            .await
            .map_err(|_| "proxy service failed")
    }
    .await;
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
