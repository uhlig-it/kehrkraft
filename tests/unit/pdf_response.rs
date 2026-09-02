//! Integration test for the public PDF endpoint (Milestone 6).
//!
//! Boots the real router in-process on an ephemeral port, creates a plan in an
//! in-memory database, and fetches the PDF over HTTP. Skipped when the `typst`
//! CLI is not installed.

use kehrkraft::app;
use kehrkraft::db::{migrate, queries};
use std::net::SocketAddr;
use tokio::net::TcpListener;

fn typst_available() -> bool {
    match std::process::Command::new("typst")
        .arg("--version")
        .output()
    {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

/// Start the app on 127.0.0.1 with a random port; returns the base URL.
/// The returned pool must be kept alive so the in-memory DB persists.
async fn start_app() -> (String, sqlx::SqlitePool) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory db");
    migrate(&pool).await.expect("migrate");

    let app = app::build_router(pool.clone(), "admin".into(), "secret".into());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve");
    });

    (format!("http://{}", addr), pool)
}

#[tokio::test]
async fn public_pdf_returns_valid_pdf() {
    if !typst_available() {
        eprintln!("typst not installed; skipping PDF response test");
        return;
    }

    let (base_url, _pool) = start_app().await;

    let plan = queries::create_plan(&_pool, "Test Plan", "Alice", "alice@example.com")
        .await
        .expect("create plan");

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/p/{}/kehrwoche.pdf", base_url, plan.secret_slug))
        .send()
        .await
        .expect("fetch pdf");

    assert_eq!(resp.status(), reqwest::StatusCode::OK, "expected 200");
    assert_eq!(
        resp.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/pdf"),
        "expected application/pdf content type"
    );
    let cd = resp
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .expect("content-disposition header");
    assert!(
        cd.contains("inline"),
        "expected inline disposition, got {cd:?}"
    );

    let body = resp.bytes().await.expect("read body");
    assert!(!body.is_empty(), "expected non-empty PDF body");
    assert!(
        body.starts_with(b"%PDF-"),
        "expected PDF magic header, got {:?}",
        &body[..body.len().min(8)]
    );
}

#[tokio::test]
async fn unknown_slug_returns_404() {
    let (base_url, _pool) = start_app().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/p/does-not-exist/kehrwoche.pdf", base_url))
        .send()
        .await
        .expect("fetch pdf");

    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn healthz_returns_ok() {
    let (base_url, _pool) = start_app().await;
    let resp = reqwest::get(format!("{base_url}/healthz"))
        .await
        .expect("fetch healthz");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert_eq!(resp.text().await.expect("body"), "ok");
}
