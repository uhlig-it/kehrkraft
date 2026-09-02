//! The E2E scenarios from Milestone 8/10, driven over HTTP.

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

async fn create_building(h: &Harness, client: &reqwest::Client, name: &str) -> String {
    let resp = basic_auth(client.post(format!("{}/admin/buildings", h.base_url)))
        .form(&[
            ("name", name),
            ("description", "A test building"),
            ("admin_name", "Alice"),
            ("admin_email", "alice@example.com"),
        ])
        .send()
        .await
        .expect("create building");
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "create building redirects"
    );
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .to_string();
    assert!(
        location.starts_with("/admin/buildings/"),
        "got {location:?}"
    );
    location.trim_start_matches("/admin/buildings/").to_string()
}

/// Create an apartment for the building; returns its id.
async fn create_apartment(
    h: &Harness,
    client: &reqwest::Client,
    building_id: &str,
    name: &str,
) -> String {
    let resp = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/apartments",
        h.base_url
    )))
    .form(&[("name", name), ("description", "")])
    .send()
    .await
    .expect("create apartment");
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "create apartment redirects"
    );
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .to_string();
    let prefix = format!("/admin/buildings/{building_id}/apartments/");
    assert!(location.starts_with(&prefix), "got {location:?}");
    location.trim_start_matches(&prefix).to_string()
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
async fn create_building_apartment_owner_tenancy_flow() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_building(&h, &client, "Haus Sonnenschein").await;

    // Building detail page shows the name, description, the secret PDF link,
    // and the admin.
    let detail = basic_auth(client.get(format!("{}/admin/buildings/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch building detail");
    assert_eq!(detail.status(), StatusCode::OK);
    let body = detail.text().await.expect("building detail body");
    assert!(
        body.contains("Haus Sonnenschein"),
        "building name on detail page"
    );
    assert!(
        body.contains("A test building"),
        "building description on detail page"
    );
    assert!(
        body.contains("/kehrwoche.pdf"),
        "public PDF link on detail page"
    );
    assert!(
        body.contains("Alice &lt;alice@example.com&gt;"),
        "admin listed"
    );

    // Add an apartment.
    let apartment_id = create_apartment(&h, &client, &id, "EG links").await;

    // Apartment page lists it under the building.
    let apartments =
        basic_auth(client.get(format!("{}/admin/buildings/{id}/apartments", h.base_url)))
            .send()
            .await
            .expect("fetch apartments");
    assert_eq!(apartments.status(), StatusCode::OK);
    let body = apartments.text().await.expect("apartments body");
    assert!(body.contains("EG links"), "apartment listed, got {body:?}");

    // Record an owner and a tenant for the apartment.
    let apt_url = format!(
        "{}/admin/buildings/{id}/apartments/{apartment_id}",
        h.base_url
    );
    let owner = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Otto Eigentümer"),
            ("email", "otto@example.com"),
            ("start_date", "2026-01-01"),
        ])
        .send()
        .await
        .expect("create ownership");
    assert_eq!(owner.status(), StatusCode::SEE_OTHER, "owner redirects");

    let tenant = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Bob Mieter"),
            ("email", "bob@example.com"),
            ("start_date", "2026-01-01"),
        ])
        .send()
        .await
        .expect("create tenancy");
    assert_eq!(tenant.status(), StatusCode::SEE_OTHER, "tenancy redirects");

    // The apartment page shows owner and tenant.
    let show = basic_auth(client.get(apt_url))
        .send()
        .await
        .expect("fetch apartment");
    assert_eq!(show.status(), StatusCode::OK);
    let body = show.text().await.expect("apartment body");
    assert!(
        body.contains("Otto Eigentümer"),
        "owner listed, got {body:?}"
    );
    assert!(body.contains("Bob Mieter"), "tenant listed");
}

#[tokio::test]
async fn overlapping_tenancies_are_rejected() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_building(&h, &client, "Musterblock").await;
    let apartment_id = create_apartment(&h, &client, &id, "OG rechts").await;
    let apt_url = format!(
        "{}/admin/buildings/{id}/apartments/{apartment_id}",
        h.base_url
    );

    // First tenancy occupies January through June.
    let first = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Nina"),
            ("email", "nina@example.com"),
            ("start_date", "2026-01-01"),
            ("end_date", "2026-06-30"),
        ])
        .send()
        .await
        .expect("create tenancy");
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    // Overlapping tenancy must be rejected.
    let second = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Karl"),
            ("email", "karl@example.com"),
            ("start_date", "2026-06-01"),
        ])
        .send()
        .await
        .expect("create overlapping tenancy");
    assert_eq!(
        second.status(),
        StatusCode::BAD_REQUEST,
        "overlapping tenancy should be rejected"
    );

    // Adjacent (non-overlapping) tenancy is accepted.
    let adjacent = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Karl"),
            ("email", "karl@example.com"),
            ("start_date", "2026-07-01"),
        ])
        .send()
        .await
        .expect("create adjacent tenancy");
    assert_eq!(adjacent.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn schedule_preview_shows_assignments_and_pdf_link() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_building(&h, &client, "Musterblock").await;
    let apartment_id = create_apartment(&h, &client, &id, "EG").await;
    let apt_url = format!(
        "{}/admin/buildings/{id}/apartments/{apartment_id}",
        h.base_url
    );

    // Owner sells, rents out, and is represented by the tenant from February.
    basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Otto Eigentümer"),
            ("email", "otto@example.com"),
            ("start_date", "2026-01-01"),
        ])
        .send()
        .await
        .expect("create ownership");
    basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Anna Bewohnerin"),
            ("email", "anna@example.com"),
            ("start_date", "2026-02-01"),
        ])
        .send()
        .await
        .expect("create tenancy");

    let schedule = basic_auth(client.get(format!("{}/admin/buildings/{id}/schedule", h.base_url)))
        .send()
        .await
        .expect("fetch schedule");
    assert_eq!(schedule.status(), StatusCode::OK);
    let body = schedule.text().await.expect("schedule body");
    assert!(
        body.contains("Schedule Preview"),
        "schedule heading, got {body:?}"
    );
    // Tenant is delegated from February on; owner before that.
    assert!(body.contains("Anna Bewohnerin"), "assigned tenant in table");
    assert!(body.contains("Otto Eigentümer"), "assigned owner in table");
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

    let id = create_building(&h, &client, "E2E Plan").await;
    let detail = basic_auth(client.get(format!("{}/admin/buildings/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch building detail");
    let html = detail.text().await.expect("building detail body");
    let pdf_path = Regex::new(r#"href="(/p/[^"]+/kehrwoche\.pdf)""#)
        .expect("regex")
        .captures(&html)
        .expect("secret slug on building page")[1]
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
