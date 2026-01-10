mod config;
mod db;

use axum::{routing::get, Router};
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

async fn healthz() -> &'static str {
    "ok"
}

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    // Initialize database pool and run migrations
    let pool = db::connect_pool().await?;
    db::migrate(&pool).await?;

    // Build router and hold pool in state (so it lives for app lifetime)
    let app = Router::new()
        .route("/healthz", get(healthz))
        .with_state(pool.clone());

    let port_opt = config::port_from_env();
    let bind_addr = SocketAddr::from(([0, 0, 0, 0], port_opt.unwrap_or(0)));

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let actual_addr = listener.local_addr()?;
    tracing::info!("Kehrkraft server listening on http://{}", actual_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}
