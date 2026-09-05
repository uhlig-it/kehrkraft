use std::net::SocketAddr;

use kehrkraft::app;
use kehrkraft::config;
use kehrkraft::db;
use tokio::process::Command;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = term.recv() => {},
    }
}

async fn typst_available() -> bool {
    match Command::new("typst").arg("--version").output().await {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let typst = typst_available().await;
    if !typst {
        tracing::warn!("typst CLI not found or not executable; PDF generation may fail");
    }

    // Initialize database pool and run migrations
    let pool = db::connect_pool().await?;
    db::migrate(&pool).await?;

    // Admin area authentication: required unless demo mode is on.
    let demo_mode = config::demo_mode_from_env();
    let admin_credentials = if demo_mode {
        tracing::warn!("KEHRKRAFT_DEMO_MODE is set: authentication disabled, demo banner shown");
        None
    } else {
        let (admin_user, admin_pass) = config::admin_credentials_from_env()
            .map_err(|_| "ADMIN_USER and ADMIN_PASS must be set")?;
        Some((admin_user, admin_pass))
    };
    let app = app::build_router(pool, admin_credentials, demo_mode);

    let port_opt = config::port_from_env();
    let bind_addr = SocketAddr::from(([0, 0, 0, 0], port_opt.unwrap_or(0)));

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let actual_addr = listener.local_addr()?;
    tracing::info!(
        typst_available = typst,
        "Kehrkraft server listening on http://{}",
        actual_addr
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}
