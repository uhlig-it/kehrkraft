//! Test harness: boots the app in-process on an ephemeral port with a fresh
//! in-memory SQLite database.

use kehrkraft::app;
use kehrkraft::db;
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub const ADMIN_USER: &str = "admin";
pub const ADMIN_PASS: &str = "secret";

pub struct Harness {
    pub base_url: String,
    /// Stays alive for the lifetime of the harness; keeps the in-memory DB alive.
    #[allow(dead_code)]
    pub pool: sqlx::SqlitePool,
    #[allow(dead_code)]
    server: tokio::task::JoinHandle<()>,
}

/// Start the app bound to port 0 with a fresh in-memory SQLite database,
/// with admin Basic Auth enabled (same as production).
pub async fn start() -> Harness {
    start_with(
        Some((ADMIN_USER.to_string(), ADMIN_PASS.to_string())),
        false,
    )
    .await
}

/// Start the app with admin Basic Auth disabled and the demo banner enabled.
pub async fn start_demo() -> Harness {
    start_with(None, true).await
}

async fn start_with(admin_credentials: Option<(String, String)>, demo_mode: bool) -> Harness {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory db");
    db::migrate(&pool).await.expect("migrate");

    let app = app::build_router(pool.clone(), admin_credentials, demo_mode);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve");
    });

    Harness {
        base_url: format!("http://{}", addr),
        pool,
        server,
    }
}
