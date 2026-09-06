//! The E2E scenarios from Milestone 8/10, driven over HTTP.

use crate::harness::{self, Harness};
use kehrkraft::db::queries;
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

/// Create an apartment for the building; returns its id. The form collects
/// the first owner together with the apartment (a building's apartment must
/// always have an ownership record); the initial owner period is closed so
/// tests can add owners starting 2026 and tile the chain.
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
    .form(&[
        ("name", name),
        ("description", ""),
        ("owner_name", "Ursprünglicher Eigentümer"),
        ("owner_email", "urspruenglich@example.com"),
        ("owner_start_date", "2025-01-01"),
        ("owner_end_date", "2025-12-31"),
    ])
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
    let body = auth.text().await.expect("admin body");
    assert!(
        body.contains("Gebäude"),
        "expected buildings home page, got {body:?}"
    );

    // The site root is the same admin-only buildings home page.
    let root = basic_auth(client.get(format!("{}/", h.base_url)))
        .send()
        .await
        .expect("authenticated root request");
    assert_eq!(root.status(), StatusCode::OK);
    assert!(
        root.text().await.expect("root body").contains("Gebäude"),
        "expected buildings home page at /"
    );

    // Static stylesheet is served without authentication.
    let css = reqwest::Client::new()
        .get(format!("{}/static/app.css", h.base_url))
        .send()
        .await
        .expect("stylesheet request");
    assert_eq!(css.status(), StatusCode::OK);
    assert_eq!(
        css.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/css; charset=utf-8")
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
        body.contains("mailto:alice@example.com"),
        "admin listed as contact, got {body:?}"
    );

    // Add an apartment.
    let apartment_id = create_apartment(&h, &client, &id, "EG links").await;

    // The legacy apartments index now redirects to the building page.
    let legacy = basic_auth(client.get(format!("{}/admin/buildings/{id}/apartments", h.base_url)))
        .send()
        .await
        .expect("fetch legacy apartments index");
    assert_eq!(legacy.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        legacy
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(format!("/admin/buildings/{id}").as_str())
    );

    // Apartments are listed right on the building page.
    let building_page = basic_auth(client.get(format!("{}/admin/buildings/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page");
    assert_eq!(building_page.status(), StatusCode::OK);
    let body = building_page.text().await.expect("building body");
    assert!(
        body.contains("EG links"),
        "apartment listed on building page, got {body:?}"
    );

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
    let show = basic_auth(client.get(apt_url.clone()))
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

    // The edit form lives on its own page, linked from the apartment page.
    let edit_path = format!("/admin/buildings/{id}/apartments/{apartment_id}/edit");
    assert!(body.contains(&edit_path), "edit link on apartment page");
    let edit = basic_auth(client.get(format!("{}{edit_path}", h.base_url)))
        .send()
        .await
        .expect("fetch apartment edit page");
    assert_eq!(edit.status(), StatusCode::OK);
    let edit_body = edit.text().await.expect("edit body");
    assert!(
        edit_body.contains(&format!(
            "action=\"/admin/buildings/{id}/apartments/{apartment_id}/update\""
        )),
        "edit form posts to update, got {edit_body:?}"
    );

    // The edit form collects only name and description — owners are managed
    // on the apartment page — and submitting it must not require owner fields.
    let update = basic_auth(client.post(format!("{apt_url}/update")))
        .form(&[
            ("name", "EG links saniert"),
            ("description", "Frisch gestrichen"),
        ])
        .send()
        .await
        .expect("update apartment");
    assert_eq!(
        update.status(),
        StatusCode::SEE_OTHER,
        "apartment update redirects to the show page"
    );
    assert_eq!(
        update
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(format!("/admin/buildings/{id}/apartments/{apartment_id}").as_str())
    );

    // The show page reflects the saved changes.
    let updated = basic_auth(client.get(apt_url.clone()))
        .send()
        .await
        .expect("fetch updated apartment");
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body = updated.text().await.expect("updated apartment body");
    assert!(
        updated_body.contains("EG links saniert"),
        "renamed apartment on show page, got {updated_body:?}"
    );
    assert!(
        updated_body.contains("Frisch gestrichen"),
        "updated description on show page, got {updated_body:?}"
    );
}

/// Dragging the drag handle ("⋮⋮") reorders apartments: Sortable.js moves the
/// row and dispatches an `end` event (htmx pattern "drag-to-reorder"), which
/// htmx turns into a POST of the hidden `item` inputs as repeated form fields
/// in their new DOM order.
///
/// This drives that POST exactly like the browser does. It guards the raw
/// pair parsing in the handler: axum's `Form` extractor (serde_urlencoded)
/// rejects repeated fields for a `Vec` with "invalid type: string, expected a
/// sequence".
#[tokio::test]
async fn drag_reordering_persists_apartment_order() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "Haus Sonnenschein").await;
    let a = create_apartment(&h, &client, &building_id, "Apartment 1").await;
    let b = create_apartment(&h, &client, &building_id, "Apartment 2").await;
    let c = create_apartment(&h, &client, &building_id, "Apartment 3").await;

    // Sanity: apartments start out in creation order.
    let initial = list_apartment_ids(&h.pool, &building_id).await;
    assert_eq!(initial, vec![a.clone(), b.clone(), c.clone()]);

    // Drag the third row to the top: one `item` field per row, in the new
    // DOM order (this is what htmx sends when the drag ends).
    let resp = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/apartments/reorder",
        h.base_url
    )))
    .form(&[
        ("item", c.as_str()),
        ("item", a.as_str()),
        ("item", b.as_str()),
    ])
    .send()
    .await
    .expect("reorder apartments");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "reorder must accept repeated item fields"
    );

    // The new order is persisted.
    let reordered = list_apartment_ids(&h.pool, &building_id).await;
    assert_eq!(reordered, vec![c.clone(), a.clone(), b.clone()]);

    // The response re-renders the table body in the new order so htmx can
    // swap it in.
    let body = resp.text().await.expect("reorder response body");
    let row_pos = |id: &str| {
        body.find(format!("/apartments/{id}").as_str())
            .unwrap_or_else(|| panic!("reordered row {id:?} missing from response"))
    };
    assert!(row_pos(&c) < row_pos(&a), "tbody rows follow the new order");
    assert!(row_pos(&a) < row_pos(&b), "tbody rows follow the new order");
}

async fn list_apartment_ids(pool: &kehrkraft::db::Db, building_id: &str) -> Vec<String> {
    queries::list_apartments(pool, building_id)
        .await
        .expect("list apartments")
        .into_iter()
        .map(|a| a.id)
        .collect()
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
        body.contains("Jahresplan"),
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

#[tokio::test]
async fn public_ical_feed_lists_kehrwoche_weeks() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_building(&h, &client, "Kalenderhaus").await;
    let detail = basic_auth(client.get(format!("{}/admin/buildings/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch building detail");
    let html = detail.text().await.expect("building detail body");
    let ics_path = Regex::new(r#"href="(/p/[^"]+/kehrwoche\.ics)""#)
        .expect("regex")
        .captures(&html)
        .expect("iCal link on building page")[1]
        .to_string();

    // Public iCal feed: no authentication required.
    let feed = reqwest::Client::new()
        .get(format!("{}{}", h.base_url, ics_path))
        .send()
        .await
        .expect("fetch public ical feed");
    assert_eq!(feed.status(), StatusCode::OK);
    assert_eq!(
        feed.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/calendar; charset=utf-8")
    );
    let body = feed.text().await.expect("ical body");
    assert!(body.starts_with("BEGIN:VCALENDAR\r\n"), "{body:?}");
    assert!(body.ends_with("END:VCALENDAR\r\n"), "{body:?}");
    assert!(body.contains("BEGIN:VEVENT"), "has events, got {body:?}");
    assert!(
        body.contains("DTSTART;VALUE=DATE:") && body.contains("DTEND;VALUE=DATE:"),
        "all-day events, got {body:?}"
    );
    assert!(body.contains("SUMMARY:"), "has summaries, got {body:?}");
}

#[tokio::test]
async fn unknown_ical_slug_returns_404() {
    let h = harness::start().await;
    let feed = reqwest::Client::new()
        .get(format!("{}/p/does-not-exist/kehrwoche.ics", h.base_url))
        .send()
        .await
        .expect("fetch ical feed");
    assert_eq!(feed.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn overlapping_ownerships_are_rejected() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_building(&h, &client, "Eigentümerblock").await;
    let apartment_id = create_apartment(&h, &client, &id, "DG").await;
    let apt_url = format!(
        "{}/admin/buildings/{id}/apartments/{apartment_id}",
        h.base_url
    );

    // First ownership occupies January through June.
    let first = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Otto"),
            ("email", "otto@example.com"),
            ("start_date", "2026-01-01"),
            ("end_date", "2026-06-30"),
        ])
        .send()
        .await
        .expect("create ownership");
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    // Overlapping ownership must be rejected on create.
    let overlap = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Karla"),
            ("email", "karla@example.com"),
            ("start_date", "2026-06-01"),
        ])
        .send()
        .await
        .expect("create overlapping ownership");
    assert_eq!(
        overlap.status(),
        StatusCode::BAD_REQUEST,
        "overlapping ownership should be rejected"
    );

    // Adjacent (non-overlapping) ownership is accepted.
    let adjacent = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Karl"),
            ("email", "karl@example.com"),
            ("start_date", "2026-07-01"),
        ])
        .send()
        .await
        .expect("create adjacent ownership");
    assert_eq!(adjacent.status(), StatusCode::SEE_OTHER);

    // Updating an ownership into an overlapping period must be rejected.
    let owners = queries::list_ownerships(&h.pool, &apartment_id)
        .await
        .expect("list ownerships");
    let adjacent_id = owners
        .iter()
        .find(|o| o.name == "Karl")
        .expect("adjacent ownership")
        .id
        .clone();
    let update = basic_auth(client.post(format!("{apt_url}/ownerships/{adjacent_id}")))
        .form(&[
            ("name", "Karl"),
            ("email", "karl@example.com"),
            ("start_date", "2026-06-01"),
        ])
        .send()
        .await
        .expect("update ownership into overlap");
    assert_eq!(
        update.status(),
        StatusCode::BAD_REQUEST,
        "overlapping ownership update should be rejected"
    );
}

#[tokio::test]
async fn overlapping_tenancy_update_rejected() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_building(&h, &client, "Musterblock").await;
    let apartment_id = create_apartment(&h, &client, &id, "OG links").await;
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

    // A second, adjacent tenancy.
    let second = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Karl"),
            ("email", "karl@example.com"),
            ("start_date", "2026-07-01"),
        ])
        .send()
        .await
        .expect("create adjacent tenancy");
    assert_eq!(second.status(), StatusCode::SEE_OTHER);

    // Updating the second tenancy into an overlapping period is rejected.
    let tenancies = queries::list_tenancies(&h.pool, &apartment_id)
        .await
        .expect("list tenancies");
    let second_id = tenancies
        .iter()
        .find(|t| t.name == "Karl")
        .expect("second tenancy")
        .id
        .clone();
    let update = basic_auth(client.post(format!("{apt_url}/tenancies/{second_id}")))
        .form(&[
            ("name", "Karl"),
            ("email", "karl@example.com"),
            ("start_date", "2026-06-01"),
        ])
        .send()
        .await
        .expect("update tenancy into overlap");
    assert_eq!(
        update.status(),
        StatusCode::BAD_REQUEST,
        "overlapping tenancy update should be rejected"
    );
}

#[tokio::test]
async fn name_length_limits_are_enforced() {
    let h = harness::start().await;
    let client = admin_client();

    // Building names are capped at 30 characters.
    let too_long = "x".repeat(31);
    let rejected = basic_auth(client.post(format!("{}/admin/buildings", h.base_url)))
        .form(&[
            ("name", too_long.as_str()),
            ("description", ""),
            ("admin_name", "Alice"),
            ("admin_email", "alice@example.com"),
        ])
        .send()
        .await
        .expect("create building with long name");
    assert_eq!(
        rejected.status(),
        StatusCode::BAD_REQUEST,
        "building name over 30 chars should be rejected"
    );

    let ok_name = "x".repeat(30);
    let accepted = basic_auth(client.post(format!("{}/admin/buildings", h.base_url)))
        .form(&[
            ("name", ok_name.as_str()),
            ("description", "ok"),
            ("admin_name", "Alice"),
            ("admin_email", "alice@example.com"),
        ])
        .send()
        .await
        .expect("create building with 30-char name");
    assert_eq!(accepted.status(), StatusCode::SEE_OTHER);

    // Apartment names are capped at 30 characters as well.
    let id = create_building(&h, &client, "Musterblock").await;
    let apartment_rejected =
        basic_auth(client.post(format!("{}/admin/buildings/{id}/apartments", h.base_url)))
            .form(&[
                ("name", too_long.as_str()),
                ("description", ""),
                ("owner_name", "Alice"),
                ("owner_email", "alice@example.com"),
                ("owner_start_date", "2026-01-01"),
            ])
            .send()
            .await
            .expect("create apartment with long name");
    assert_eq!(
        apartment_rejected.status(),
        StatusCode::BAD_REQUEST,
        "apartment name over 30 chars should be rejected"
    );

    let apartment_accepted =
        basic_auth(client.post(format!("{}/admin/buildings/{id}/apartments", h.base_url)))
            .form(&[
                ("name", ok_name.as_str()),
                ("description", ""),
                ("owner_name", "Alice"),
                ("owner_email", "alice@example.com"),
                ("owner_start_date", "2026-01-01"),
            ])
            .send()
            .await
            .expect("create apartment with 30-char name");
    assert_eq!(apartment_accepted.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn deletes_redirect_htmx_requests_via_hx_redirect_header() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "Haus HX").await;

    // htmx-driven deletes (HX-Request) get a 200 + HX-Redirect header so
    // htmx performs a full-page navigation instead of swapping the empty
    // response body into the form.
    let hx = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/delete",
        h.base_url
    )))
    .header("HX-Request", "true")
    .send()
    .await
    .expect("htmx delete building");
    assert_eq!(hx.status(), StatusCode::OK);
    let expected = "/admin";
    assert_eq!(
        hx.headers()
            .get("HX-Redirect")
            .and_then(|v| v.to_str().ok()),
        Some(expected),
        "htmx deletes should respond with HX-Redirect"
    );

    // Deleting an apartment redirects back to the building page.
    let building2 = create_building(&h, &client, "Haus HX 2").await;
    let apartment_id = create_apartment(&h, &client, &building2, "Wohnung HX").await;
    let apt_url = format!(
        "{}/admin/buildings/{building2}/apartments/{apartment_id}",
        h.base_url
    );

    // Deleting an ownership or tenancy redirects back to the apartment page.
    basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Otto"),
            ("email", "otto@example.com"),
            ("start_date", "2026-01-01"),
        ])
        .send()
        .await
        .expect("create ownership");
    basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Bob"),
            ("email", "bob@example.com"),
            ("start_date", "2026-01-01"),
        ])
        .send()
        .await
        .expect("create tenancy");
    let ownership_id = queries::list_ownerships(&h.pool, &apartment_id)
        .await
        .expect("list ownerships")[0]
        .id
        .clone();
    let tenancy_id = queries::list_tenancies(&h.pool, &apartment_id)
        .await
        .expect("list tenancies")[0]
        .id
        .clone();
    let apartment_path = format!("/admin/buildings/{building2}/apartments/{apartment_id}");
    for (label, url) in [
        (
            "ownership",
            format!("{apt_url}/ownerships/{ownership_id}/delete"),
        ),
        (
            "tenancy",
            format!("{apt_url}/tenancies/{tenancy_id}/delete"),
        ),
    ] {
        let resp = basic_auth(client.post(url))
            .header("HX-Request", "true")
            .send()
            .await
            .unwrap_or_else(|_| panic!("htmx delete {label}"));
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("HX-Redirect")
                .and_then(|v| v.to_str().ok()),
            Some(apartment_path.as_str()),
            "htmx delete of {label} should respond with HX-Redirect"
        );
    }

    let hx_apartment = basic_auth(client.post(format!(
        "{}/admin/buildings/{building2}/apartments/{apartment_id}/delete",
        h.base_url
    )))
    .header("HX-Request", "true")
    .send()
    .await
    .expect("htmx delete apartment");
    assert_eq!(hx_apartment.status(), StatusCode::OK);
    let expected = format!("/admin/buildings/{building2}");
    assert_eq!(
        hx_apartment
            .headers()
            .get("HX-Redirect")
            .and_then(|v| v.to_str().ok()),
        Some(expected.as_str()),
        "htmx delete of apartment should respond with HX-Redirect"
    );

    // Plain form posts keep the classic 303 + Location redirect.
    let building3 = create_building(&h, &client, "Haus Plain").await;
    let plain =
        basic_auth(client.post(format!("{}/admin/buildings/{building3}/delete", h.base_url)))
            .send()
            .await
            .expect("plain delete building");
    assert_eq!(plain.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        plain
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/admin")
    );
}

type Regex = regex::Regex;

#[tokio::test]
async fn demo_mode_disables_auth_and_shows_banner() {
    let h = harness::start_demo().await;
    // No redirects: form posts return 303 + Location that we assert on.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client");

    // No credentials required in demo mode: the admin area is open.
    let resp = client
        .get(format!("{}/admin", h.base_url))
        .send()
        .await
        .expect("unauthenticated request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "/admin should be open in demo mode"
    );
    let body = resp.text().await.expect("admin body");
    assert!(
        body.contains("Gebäude"),
        "expected buildings home page, got {body:?}"
    );

    // The red demo banner is present on full pages...
    assert!(
        body.contains("Demo-Modus"),
        "expected demo banner, got {body:?}"
    );
    assert!(
        body.contains(r#"class="demo-banner""#),
        "banner should carry the demo-banner class"
    );
    assert!(
        body.contains(r#"<body><div class="demo-banner">Demo-Modus</div>"#),
        "banner should be the first element inside <body>"
    );

    // ...including on a building detail page.
    let created = client
        .post(format!("{}/admin/buildings", h.base_url))
        .form(&[
            ("name", "Demo Haus"),
            ("description", ""),
            ("admin_name", "Alice"),
            ("admin_email", "alice@example.com"),
        ])
        .send()
        .await
        .expect("create building");
    let location = created
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .to_string();
    let detail = client
        .get(format!("{}{}", h.base_url, location))
        .send()
        .await
        .expect("fetch building detail");
    assert_eq!(detail.status(), StatusCode::OK);
    assert!(
        detail
            .text()
            .await
            .expect("detail body")
            .contains("Demo-Modus"),
        "demo banner should appear on the building detail page"
    );
}
