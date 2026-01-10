mod config;
mod db;
mod web;

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{extract::State, middleware, middleware::Next};
use axum::{routing::{get, post}, Router};
use base64::Engine as _;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

async fn healthz() -> &'static str {
    "ok"
}

static KEHRKRAFT_SVG: &[u8] = include_bytes!("../kehrkraft.svg");

async fn logo_svg() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], KEHRKRAFT_SVG)
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

async fn require_basic_auth(
    State((expected_user, expected_pass)): State<(String, String)>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let unauthorized = || {
        let mut res = (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        res.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"Admin\""),
        );
        res
    };

    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if let Some(auth) = auth {
        if let Some(b64) = auth.strip_prefix("Basic ") {
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(b64) {
                if let Ok(decoded_str) = std::str::from_utf8(&decoded) {
                    if let Some((user, pass)) = decoded_str.split_once(':') {
                        if user == expected_user && pass == expected_pass {
                            return next.run(req).await;
                        }
                    }
                }
            }
        }
    }
    unauthorized()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    // Initialize database pool and run migrations
    let pool = db::connect_pool().await?;
    db::migrate(&pool).await?;

    // Admin: Basic Auth from env, required
    let (admin_user, admin_pass) = config::admin_credentials_from_env()
        .map_err(|_| "ADMIN_USER and ADMIN_PASS must be set")?;
    let admin_router = Router::new()
        .route("/admin", get(web::admin::dashboard))
        .route("/admin/plans", get(web::admin::plans_index).post(web::admin::plans_create))
        .route("/admin/plans/new", get(web::admin::plans_new))
        .route("/admin/plans/:id", get(web::admin::plans_show))
        .route("/admin/plans/:id/delete", post(web::admin::plans_delete))
        .route("/admin/plans/:id/tenants", get(web::admin::tenants_index).post(web::admin::tenants_create))
        .route("/admin/plans/:id/tenants/new", get(web::admin::tenants_new))
        .route("/admin/plans/:id/tenants/:tenant_id/edit", get(web::admin::tenants_edit))
        .route("/admin/plans/:id/tenants/:tenant_id", post(web::admin::tenants_update))
        .route("/admin/plans/:id/tenants/:tenant_id/delete", post(web::admin::tenants_delete))
        .route_layer(middleware::from_fn_with_state(
            (admin_user, admin_pass),
            require_basic_auth,
        ));

    // Build router and hold pool in state (so it lives for app lifetime)
    let app = Router::new()
        .route("/healthz", get(healthz))
        .merge(admin_router)
        .route("/kehrkraft.svg", get(logo_svg))
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
