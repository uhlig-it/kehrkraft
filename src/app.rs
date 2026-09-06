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
use axum::extract::{FromRef, Path};
use axum::http::header;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::Engine as _;

use crate::db::Db;
use crate::web::{admin, pdf};

/// Shared application state handed to handlers via [`axum::extract::State`].
/// Handlers only request the slices they need via `FromRef`.
#[derive(Clone)]
pub struct AppState {
    pub pool: Db,
    /// External base URL of this instance; used for the QR code on the PDF.
    pub public_url: Option<String>,
}

impl FromRef<AppState> for Db {
    fn from_ref(state: &AppState) -> Db {
        state.pool.clone()
    }
}

impl FromRef<AppState> for Option<String> {
    fn from_ref(state: &AppState) -> Option<String> {
        state.public_url.clone()
    }
}

const KEHRKRAFT_SVG: &[u8] = include_bytes!("../kehrkraft.svg");
const APP_CSS: &[u8] = include_bytes!("web/static/app.css");
const HTMX_JS: &[u8] = include_bytes!("web/static/htmx.min.js");
const SORTABLE_JS: &[u8] = include_bytes!("web/static/sortable.min.js");
const BARLOW_400: &[u8] = include_bytes!("web/static/barlow-400.woff2");
const BARLOW_500: &[u8] = include_bytes!("web/static/barlow-500.woff2");
const BARLOW_600: &[u8] = include_bytes!("web/static/barlow-600.woff2");
const BARLOW_700: &[u8] = include_bytes!("web/static/barlow-700.woff2");
const BARLOW_CONDENSED_600: &[u8] = include_bytes!("web/static/barlow-condensed-600.woff2");

async fn healthz() -> &'static str {
    "ok"
}

async fn logo_svg() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], KEHRKRAFT_SVG)
}

async fn app_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        APP_CSS,
    )
}

async fn htmx_js() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        HTMX_JS,
    )
}

async fn sortable_js() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        SORTABLE_JS,
    )
}

/// Bundled Barlow webfonts (OFL), embedded like the other static assets so the
/// app stays self-contained and works offline.
async fn font(Path(name): Path<String>) -> Response {
    let (bytes, content_type): (&[u8], &'static str) = match name.as_str() {
        "barlow-400.woff2" => (BARLOW_400, "font/woff2"),
        "barlow-500.woff2" => (BARLOW_500, "font/woff2"),
        "barlow-600.woff2" => (BARLOW_600, "font/woff2"),
        "barlow-700.woff2" => (BARLOW_700, "font/woff2"),
        "barlow-condensed-600.woff2" => (BARLOW_CONDENSED_600, "font/woff2"),
        _ => return (StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        bytes,
    )
        .into_response()
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

/// Markup injected at the top of every full HTML page in demo mode.
const DEMO_BANNER_HTML: &[u8] = b"<div class=\"demo-banner\">Demo-Modus</div>";

/// Inserts [DEMO_BANNER_HTML] right after the opening `<body>` tag, or returns
/// the body unchanged if no `<body>` tag is present.
fn insert_demo_banner(body: &[u8]) -> Vec<u8> {
    let body_tag = b"<body";
    let mut i = 0;
    while i + body_tag.len() <= body.len() {
        if &body[i..i + body_tag.len()] == body_tag {
            if let Some(rel) = body[i..].iter().position(|&b| b == b'>') {
                let insert_at = i + rel + 1;
                let mut out = Vec::with_capacity(body.len() + DEMO_BANNER_HTML.len());
                out.extend_from_slice(&body[..insert_at]);
                out.extend_from_slice(DEMO_BANNER_HTML);
                out.extend_from_slice(&body[insert_at..]);
                return out;
            }
            break;
        }
        i += 1;
    }
    body.to_vec()
}

/// Demo-mode middleware: adds the red "Demo Mode" banner to full HTML pages.
///
/// All admin pages extend `base.html`, so a full page always contains a
/// `<body>` tag, while htmx partials (table bodies, form fragments) do not and
/// pass through untouched. Runs at route level, i.e. inside the compression
/// layer, so the body can be rewritten before compression.
async fn demo_banner(req: Request<Body>, next: Next) -> Response {
    let res = next.run(req).await;
    let is_html = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    if !is_html {
        return res;
    }

    let (mut parts, body) = res.into_parts();
    // Bodies are small HTML pages; a failure to buffer within 1 MiB means the
    // response cannot be rewritten, so surface a server error rather than
    // sending an unmodified (banner-less) page.
    let Ok(bytes) = axum::body::to_bytes(body, 1_000_000).await else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to buffer response",
        )
            .into_response();
    };
    let out = insert_demo_banner(&bytes);
    if out.len() == bytes.len() {
        return Response::from_parts(parts, Body::from(out));
    }
    // The length changed, so a stale Content-Length would be wrong.
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(out))
}

/// The full application router.
///
/// `admin_credentials` enables Basic Auth on the admin area; `None` disables
/// it (demo mode). `demo_mode` additionally injects a "Demo Mode" banner into
/// every full HTML page.
///
/// Requires serving with `into_make_service_with_connect_info::<SocketAddr>()`
/// so the rate limiter can see client addresses.
pub fn build_router(
    pool: Db,
    admin_credentials: Option<(String, String)>,
    demo_mode: bool,
    public_url: Option<String>,
) -> Router {
    let state = AppState { pool, public_url };
    let admin_router = Router::new()
        .route("/", get(admin::buildings_index))
        .route("/admin", get(admin::buildings_index))
        .route(
            "/admin/buildings",
            get(admin::buildings_index).post(admin::buildings_create),
        )
        .route("/admin/buildings/new", get(admin::buildings_new))
        .route("/admin/buildings/{id}", get(admin::buildings_show))
        .route(
            "/admin/buildings/{id}/schedule",
            get(admin::buildings_schedule),
        )
        .route(
            "/admin/buildings/{id}/delete",
            axum::routing::post(admin::buildings_delete),
        )
        .route(
            "/admin/buildings/{id}/apartments",
            get(admin::apartments_index_redirect).post(admin::apartments_create),
        )
        .route(
            "/admin/buildings/{id}/apartments/new",
            get(admin::apartments_new),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}",
            get(admin::apartments_show),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/edit",
            get(admin::apartments_edit),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/update",
            axum::routing::post(admin::apartments_update),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/delete",
            axum::routing::post(admin::apartments_delete),
        )
        .route(
            "/admin/buildings/{id}/apartments/reorder",
            axum::routing::post(admin::apartments_reorder),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/ownerships",
            axum::routing::post(admin::ownerships_create),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/ownerships/{ownership_id}/edit",
            get(admin::ownerships_edit),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/ownerships/{ownership_id}",
            axum::routing::post(admin::ownerships_update),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/ownerships/{ownership_id}/delete",
            axum::routing::post(admin::ownerships_delete),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/tenancies",
            axum::routing::post(admin::tenancies_create),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/tenancies/{tenancy_id}/edit",
            get(admin::tenancies_edit),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/tenancies/{tenancy_id}",
            axum::routing::post(admin::tenancies_update),
        )
        .route(
            "/admin/buildings/{id}/apartments/{apartment_id}/tenancies/{tenancy_id}/delete",
            axum::routing::post(admin::tenancies_delete),
        );

    let public_router = Router::new()
        .route("/p/{secret_slug}/kehrwoche.pdf", get(pdf::public_pdf))
        .route_layer(middleware::from_fn(rate_limit));

    let admin_router = if demo_mode {
        admin_router.route_layer(middleware::from_fn(demo_banner))
    } else {
        let (admin_user, admin_pass) =
            admin_credentials.expect("admin credentials required when demo mode is disabled");
        admin_router.route_layer(middleware::from_fn_with_state(
            (admin_user, admin_pass),
            require_basic_auth,
        ))
    };

    Router::new()
        .route("/healthz", get(healthz))
        .merge(admin_router)
        .merge(public_router)
        .route("/kehrkraft.svg", get(logo_svg))
        .route("/static/app.css", get(app_css))
        .route("/static/htmx.min.js", get(htmx_js))
        .route("/static/sortable.min.js", get(sortable_js))
        .route("/static/{font}", get(font))
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
        .with_state(state)
}
