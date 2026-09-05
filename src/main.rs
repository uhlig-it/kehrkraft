use std::net::SocketAddr;
use std::time::Duration;

use kehrkraft::app;
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
            .map_err(|_| "KEHRKRAFT_ADMIN_USER and KEHRKRAFT_ADMIN_PASS must be set")?;
        Some((admin_user, admin_pass))
    };
    let app = app::build_router(pool, admin_credentials, demo_mode);

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
