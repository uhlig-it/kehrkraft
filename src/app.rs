//! Application router assembly: routes, auth, and hardening layers.
//!
//! Kept in the library crate so integration and E2E tests can build and
//! serve the same app as the binary.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::Engine as _;

use crate::db::Db;
use crate::web::{admin, pdf};

const KEHRKRAFT_SVG: &[u8] = include_bytes!("../kehrkraft.svg");

async fn healthz() -> &'static str {
    "ok"
}

async fn logo_svg() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], KEHRKRAFT_SVG)
}

/// Basic Auth gate for the admin area; returns 401 + WWW-Authenticate on failure.
async fn require_basic_auth(
    State((expected_user, expected_pass)): State<(String, String)>,
    req: Request<Body>,
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

/// Basic security headers on every response.
async fn security_headers(req: Request<Body>, next: Next) -> Response {
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    res.headers_mut()
        .insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    res.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    res
}

/// Simple fixed-window per-IP rate limiter for the public PDF endpoint.
/// Deliberately process-local and coarse; good enough to slow down abuse.
const RATE_LIMIT_PER_MINUTE: u32 = 30;

#[derive(Clone, Copy)]
struct WindowEntry {
    window_start: Instant,
    count: u32,
}

static RATE_LIMIT_HITS: OnceLock<Arc<Mutex<HashMap<IpAddr, WindowEntry>>>> = OnceLock::new();

fn rate_limit_hits() -> Arc<Mutex<HashMap<IpAddr, WindowEntry>>> {
    RATE_LIMIT_HITS
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

async fn rate_limit(req: Request<Body>, next: Next) -> Response {
    // axum::serve with into_make_service_with_connect_info puts the client
    // address into the request extensions; the ConnectInfo extractor is just
    // a reader for it. Read it directly to keep the middleware state-free.
    let Some(ip) = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
    else {
        return next.run(req).await;
    };

    let window = Duration::from_secs(60);
    let hits = rate_limit_hits();
    let mut map = hits.lock().await;
    let now = Instant::now();
    let entry = map.entry(ip).or_insert(WindowEntry {
        window_start: now,
        count: 0,
    });
    if entry.window_start.elapsed() >= window {
        *entry = WindowEntry {
            window_start: now,
            count: 0,
        };
    }
    entry.count += 1;
    if entry.count > RATE_LIMIT_PER_MINUTE {
        return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
    }
    drop(map);
    next.run(req).await
}

/// The full application router.
///
/// Requires serving with `into_make_service_with_connect_info::<SocketAddr>()`
/// so the rate limiter can see client addresses.
pub fn build_router(pool: Db, admin_user: String, admin_pass: String) -> Router {
    let admin_router = Router::new()
        .route("/admin", get(admin::dashboard))
        .route(
            "/admin/plans",
            get(admin::plans_index).post(admin::plans_create),
        )
        .route("/admin/plans/new", get(admin::plans_new))
        .route("/admin/plans/{id}", get(admin::plans_show))
        .route("/admin/plans/{id}/schedule", get(admin::plans_schedule))
        .route(
            "/admin/plans/{id}/delete",
            axum::routing::post(admin::plans_delete),
        )
        .route(
            "/admin/plans/{id}/tenants",
            get(admin::tenants_index).post(admin::tenants_create),
        )
        .route("/admin/plans/{id}/tenants/new", get(admin::tenants_new))
        .route(
            "/admin/plans/{id}/tenants/{tenant_id}/edit",
            get(admin::tenants_edit),
        )
        .route(
            "/admin/plans/{id}/tenants/{tenant_id}",
            axum::routing::post(admin::tenants_update),
        )
        .route(
            "/admin/plans/{id}/tenants/{tenant_id}/delete",
            axum::routing::post(admin::tenants_delete),
        )
        .route_layer(middleware::from_fn_with_state(
            (admin_user, admin_pass),
            require_basic_auth,
        ));

    let public_router = Router::new()
        .route("/p/{secret_slug}/kehrwoche.pdf", get(pdf::public_pdf))
        .route_layer(middleware::from_fn(rate_limit));

    Router::new()
        .route("/healthz", get(healthz))
        .merge(admin_router)
        .merge(public_router)
        .route("/kehrkraft.svg", get(logo_svg))
        .layer(middleware::from_fn(security_headers))
        .layer(tower_http::limit::RequestBodyLimitLayer::new(1_000_000))
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(tower_http::trace::TraceLayer::new_for_http().on_response(
            |res: &Response, latency: Duration, _: &tracing::Span| {
                let status = res.status();
                if status.is_server_error() {
                    tracing::error!(
                        status = status.as_u16(),
                        latency_ms = latency.as_millis() as u64,
                        "request failed"
                    );
                } else {
                    tracing::info!(
                        status = status.as_u16(),
                        latency_ms = latency.as_millis() as u64,
                        "request completed"
                    );
                }
            },
        ))
        .with_state(pool)
}
