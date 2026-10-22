use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use puja_server::monitor::Level;
use puja_server::{config::Config, routes, state::AppState, tls};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "puja_server=info,tower_http=warn".into()),
        )
        .init();

    let cfg = Config::from_env();
    std::fs::create_dir_all(&cfg.data_dir)?;
    let app_dir = cfg.app_dir.clone();

    let state = AppState::new(cfg.clone())?;
    state.monitor.record(
        Level::Info,
        "server",
        format!("Server started · v{}", env!("CARGO_PKG_VERSION")),
        Some(serde_json::json!({
            "pid": std::process::id(),
            "ledger": cfg.ledger_path.display().to_string(),
            "tls": cfg.tls,
            "pin": cfg.pin.is_some(),
        })),
    );

    match state.monitor.ledger() {
        Some(l) if l.ok => {
            tracing::info!(
                "ledger ready: {} (sheet {:?}, last row {})",
                cfg.ledger_path.display(),
                cfg.ledger_sheet,
                l.rows
            );
            state.monitor.record(
                Level::Ok,
                "ledger",
                format!(
                    "Ledger loaded · {} rows · cash ₹{} · online ₹{}",
                    l.rows, l.cash, l.online
                ),
                Some(serde_json::json!({
                    "path": cfg.ledger_path.display().to_string(),
                    "sheet": cfg.ledger_sheet,
                })),
            );
        }
        _ => {
            tracing::warn!("ledger missing or unreadable: {}", cfg.ledger_path.display());
            state.monitor.record(
                Level::Error,
                "ledger",
                format!("Ledger missing or unreadable · {}", cfg.ledger_path.display()),
                None,
            );
        }
    }

    let index = app_dir.join("index.html");
    // Only the two media folders are published: data/ also holds puja.db and
    // certs/ca.key, which must never be reachable over http.
    let router = routes::router(state)
        .fallback_service(ServeDir::new(&app_dir).not_found_service(ServeFile::new(&index)))
        .layer(TraceLayer::new_for_http());

    // HTTP stays on for the laptop browser and curl.
    let http = tokio::net::TcpListener::bind(&cfg.addr).await?;
    tracing::info!("http  http://{}", cfg.addr);

    if cfg.tls {
        let tls_cfg = tls::ensure(&cfg.data_dir)?;
        let rustls = axum_server::tls_rustls::RustlsConfig::from_pem(
            tls_cfg.cert.clone().into_bytes(),
            tls_cfg.key.clone().into_bytes(),
        )
        .await?;
        let https_router = router.clone();
        let tls_addr = cfg.tls_addr.clone();
        tokio::spawn(async move {
            let Ok(addr) = tls_addr.parse::<std::net::SocketAddr>() else {
                tracing::error!("bad PUJA_TLS_ADDR: {tls_addr}");
                return;
            };
            tracing::info!("https https://{tls_addr}");
            for ip in tls::local_ips() {
                tracing::info!("  phone: https://{ip}:{}  (import /ca.crt once)", tls_addr.rsplit(':').next().unwrap_or("8443"));
            }
            if let Err(e) = axum_server::bind_rustls(addr, rustls)
                .serve(https_router.into_make_service())
                .await
            {
                tracing::error!("https listener stopped: {e}");
            }
        });
    } else {
        tracing::info!("https off (PUJA_TLS=off): the phone will have no microphone");
    }

    axum::serve(http, router).await?;
    Ok(())
}
