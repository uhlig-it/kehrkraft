use std::net::SocketAddr;
use std::time::Duration;

use chrono::Utc;
use kehrkraft::app;
use kehrkraft::backup;
use kehrkraft::config;
use kehrkraft::db;
use tokio::net::TcpListener;
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

/// Bind the HTTP listener.
///
/// Binds to KEHRKRAFT_PORT when set. Otherwise reuses the dev port persisted
/// by a previous run (via [`config::save_dev_port`]) so that `cargo watch`
/// restarts keep the same port; if that port is unavailable, falls back to an
/// OS-assigned ephemeral port and persists it for the next restart.
async fn bind_http_listener() -> Result<TcpListener, Box<dyn std::error::Error>> {
    if let Some(port) = config::port_from_env() {
        return Ok(TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await?);
    }

    let port_file = config::port_file_path();
    if let Some(saved) = config::saved_dev_port(&port_file) {
        // The previous instance may still be shutting down; retry briefly.
        for attempt in 1..=5 {
            match TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], saved))).await {
                Ok(listener) => {
                    tracing::info!(
                        "Reusing dev port {} saved in {}",
                        saved,
                        port_file.display()
                    );
                    return Ok(listener);
                }
                Err(err) if attempt < 5 => {
                    tracing::debug!(
                        "Dev port {} busy (attempt {}): {}; retrying",
                        saved,
                        attempt,
                        err
                    );
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(err) => {
                    tracing::warn!(
                        "Dev port {} saved in {} is not available ({}); picking a new ephemeral port",
                        saved,
                        port_file.display(),
                        err
                    );
                }
            }
        }
    }

    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
    let port = listener.local_addr()?.port();
    config::save_dev_port(&port_file, port);
    tracing::info!(
        "Assigned dev port {}; saved to {} for future restarts",
        port,
        port_file.display()
    );
    Ok(listener)
}

/// `kehrkraft verify` downloads the latest hourly backup, decrypts it, and
/// checks its integrity and freshness. Exits non-zero on failure so it can be
/// used as a cron health check (from this host or any other).
async fn run_verify() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let config = backup::Config::from_env()?
        .ok_or("backups are disabled: set KEHRKRAFT_BACKUP_BUCKET to verify")?;
    let store = backup::build_store(&config)?;
    backup::verify(&config, store.as_ref(), Utc::now()).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("verify") => return run_verify().await,
        Some(other) => {
            eprintln!("unknown command: {other}\n\nusage: kehrkraft [verify]");
            std::process::exit(2);
        }
        None => {}
    }

    init_tracing();

    let typst = typst_available().await;
    if !typst {
        tracing::warn!("typst CLI not found or not executable; PDF generation may fail");
    }

    // Initialize database pool and run migrations
    let pool = db::connect_pool().await?;
    db::migrate(&pool).await?;

    // Hourly encrypted S3 backups: opt-in via KEHRKRAFT_BACKUP_BUCKET; runs in
    // a background task immediately and then once per hour.
    if let Some(config) = backup::Config::from_env()? {
        let runner = backup::Runner::new(config, pool.clone())?;
        tracing::info!("starting hourly encrypted backups");
        tokio::spawn(backup::run_forever(runner));
    } else {
        tracing::info!(
            "backups disabled: set KEHRKRAFT_BACKUP_BUCKET to enable hourly encrypted S3 backups"
        );
    }

    // Admin area authentication: required unless demo mode is on.
    let demo_mode = config::demo_mode_from_env();
    let admin_credentials = if demo_mode {
        tracing::warn!("KEHRKRAFT_DEMO_MODE is set: authentication disabled, demo banner shown");
        None
    } else {
        let (admin_user, admin_pass) = config::admin_credentials_from_env()
            .map_err(|_| "KEHRKRAFT_ADMIN_USER and KEHRKRAFT_ADMIN_PASS must be set")?;
        Some((admin_user, admin_pass))
    };
    let public_url = config::public_url_from_env();
    if public_url.is_none() {
        tracing::warn!(
            "KEHRKRAFT_PUBLIC_URL is unset; the Kehrwoche PDF will be generated without a QR code"
        );
    }
    let app = app::build_router(pool, admin_credentials, demo_mode, public_url);

    let listener = bind_http_listener().await?;
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
