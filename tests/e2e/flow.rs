//! The E2E scenarios from Milestone 8, driven over HTTP.

use crate::harness::{self, Harness};
use reqwest::StatusCode;

/// Authenticated client that does not follow redirects (we assert on the
/// 303 + Location of form posts).
fn admin_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client")
}

async fn create_plan(h: &Harness, client: &reqwest::Client, name: &str) -> String {
    let resp = basic_auth(client.post(format!("{}/admin/plans", h.base_url)))
        .form(&[
            ("name", name),
            ("admin_name", "Alice"),
            ("admin_email", "alice@example.com"),
        ])
        .send()
        .await
        .expect("create plan");
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "create plan redirects"
    );
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .to_string();
    assert!(location.starts_with("/admin/plans/"), "got {location:?}");
    location.trim_start_matches("/admin/plans/").to_string()
}

fn basic_auth(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.basic_auth(harness::ADMIN_USER, Some(harness::ADMIN_PASS))
}

fn typst_available() -> bool {
    match std::process::Command::new("typst")
        .arg("--version")
        .output()
    {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

#[tokio::test]
async fn admin_requires_basic_auth() {
    let h = harness::start().await;

    let unauth = reqwest::Client::new()
        .get(format!("{}/admin", h.base_url))
        .send()
        .await
        .expect("unauthenticated request");
    assert_eq!(
        unauth.status(),
        StatusCode::UNAUTHORIZED,
        "unauthenticated /admin should reject"
    );

    let client = admin_client();
    let auth = basic_auth(client.get(format!("{}/admin", h.base_url)))
        .send()
        .await
        .expect("authenticated request");
    assert_eq!(auth.status(), StatusCode::OK);
    let body = auth.text().await.expect("dashboard body");
    assert!(
        body.contains("Admin Dashboard"),
        "expected dashboard, got {body:?}"
    );
}

#[tokio::test]
async fn create_plan_and_tenant_flow() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_plan(&h, &client, "Haus Sonnenschein").await;

    // Plan detail page shows the name, the secret PDF link, and the admin.
    let detail = basic_auth(client.get(format!("{}/admin/plans/{}", h.base_url, id)))
        .send()
        .await
        .expect("fetch plan detail");
    assert_eq!(detail.status(), StatusCode::OK);
    let body = detail.text().await.expect("plan detail body");
    assert!(
        body.contains("Haus Sonnenschein"),
        "plan name on detail page"
    );
    assert!(
        body.contains("/kehrwoche.pdf"),
        "public PDF link on detail page"
    );
    assert!(
        body.contains("Alice &lt;alice@example.com&gt;"),
        "admin listed"
    );

    // Add a tenant.
    let tenant = basic_auth(client.post(format!("{}/admin/plans/{id}/tenants", h.base_url)))
        .form(&[
            ("name", "Bob Mieter"),
            ("email", "bob@example.com"),
            ("start_date", "2026-01-01"),
        ])
        .send()
        .await
        .expect("create tenant");
    assert_eq!(
        tenant.status(),
        StatusCode::SEE_OTHER,
        "create tenant redirects"
    );

    // Tenant index lists Bob.
    let tenants = basic_auth(client.get(format!("{}/admin/plans/{id}/tenants", h.base_url)))
        .send()
        .await
        .expect("fetch tenants");
    assert_eq!(tenants.status(), StatusCode::OK);
    let body = tenants.text().await.expect("tenants body");
    assert!(body.contains("Bob Mieter"), "tenant listed, got {body:?}");
}

#[tokio::test]
async fn schedule_preview_shows_assignments_and_pdf_link() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_plan(&h, &client, "Musterblock").await;
    basic_auth(client.post(format!("{}/admin/plans/{id}/tenants", h.base_url)))
        .form(&[
            ("name", "Anna Bewohnerin"),
            ("email", "anna@example.com"),
            ("start_date", "2026-02-01"),
        ])
        .send()
        .await
        .expect("create tenant");

    let schedule = basic_auth(client.get(format!("{}/admin/plans/{id}/schedule", h.base_url)))
        .send()
        .await
        .expect("fetch schedule");
    assert_eq!(schedule.status(), StatusCode::OK);
    let body = schedule.text().await.expect("schedule body");
    assert!(
        body.contains("Schedule Preview"),
        "schedule heading, got {body:?}"
    );
    assert!(body.contains("Anna Bewohnerin"), "assigned tenant in table");
    assert!(
        body.contains("/p/") && body.contains("/kehrwoche.pdf"),
        "PDF link shown"
    );
}

#[tokio::test]
async fn public_pdf_endpoint_returns_pdf() {
    if !typst_available() {
        eprintln!("typst not installed; skipping public_pdf_endpoint_returns_pdf");
        return;
    }
    let h = harness::start().await;
    let client = admin_client();

    let id = create_plan(&h, &client, "E2E Plan").await;
    let detail = basic_auth(client.get(format!("{}/admin/plans/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch plan detail");
    let html = detail.text().await.expect("plan detail body");
    let pdf_path = Regex::new(r#"href="(/p/[^"]+/kehrwoche\.pdf)""#)
        .expect("regex")
        .captures(&html)
        .expect("secret slug on plan page")[1]
        .to_string();

    // Public PDF: no authentication required.
    let pdf = reqwest::Client::new()
        .get(format!("{}{}", h.base_url, pdf_path))
        .send()
        .await
        .expect("fetch public pdf");
    assert_eq!(pdf.status(), StatusCode::OK);
    assert_eq!(
        pdf.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/pdf")
    );
    let body = pdf.bytes().await.expect("read pdf body");
    assert!(
        body.starts_with(b"%PDF-"),
        "expected PDF magic, got {:?}",
        &body[..body.len().min(8)]
    );
}

type Regex = regex::Regex;
