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

/// Today's date minus `n` days, formatted YYYY-MM-DD for the forms. The
/// delete guards and the end-date warning hinge on the real current date, so
/// these tests build their periods relative to it instead of hardcoding dates.
fn days_ago(n: i64) -> String {
    use chrono::{Duration, Local};
    (Local::now().date_naive() - Duration::days(n))
        .format("%Y-%m-%d")
        .to_string()
}

/// Create a building owned as a whole by `owner_name`/`owner_email` starting
/// `owner_start` (the "ownership_style=building" variant); returns the id.
async fn create_wholly_owned_building(
    h: &Harness,
    client: &reqwest::Client,
    name: &str,
    owner_name: &str,
    owner_email: &str,
    owner_start: &str,
) -> String {
    let resp = basic_auth(client.post(format!("{}/admin/buildings", h.base_url)))
        .form(&[
            ("name", name),
            ("description", ""),
            ("admin_name", "Alice"),
            ("admin_email", "alice@example.com"),
            ("ownership_style", "building"),
            ("owner_name", owner_name),
            ("owner_email", owner_email),
            ("owner_start_date", owner_start),
        ])
        .send()
        .await
        .expect("create wholly-owned building");
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "wholly-owned building creation redirects"
    );
    resp.headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .trim_start_matches("/admin/buildings/")
        .to_string()
}

/// Create an apartment that relies on the building owner (no per-apartment
/// ownership); returns its id.
async fn create_building_owned_apartment(
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
        ("owner_name", ""),
        ("owner_email", ""),
        ("owner_start_date", ""),
        ("owner_end_date", ""),
    ])
    .send()
    .await
    .expect("create building-owned apartment");
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "building-owned apartment creation redirects"
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

/// A building can have a single owner for the whole building (one entity owns
/// all flats, e.g. a housing company). Its apartments then need no individual
/// ownership record, and the Kehrwoche falls to the building owner unless a
/// tenant covers the week.
#[tokio::test]
async fn building_owner_flow() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "Musterblock").await;

    // The building page offers to add a building owner.
    let detail = basic_auth(client.get(format!("{}/admin/buildings/{building_id}", h.base_url)))
        .send()
        .await
        .expect("fetch building detail");
    assert_eq!(detail.status(), StatusCode::OK);
    let body = detail.text().await.expect("building detail body");
    assert!(
        body.contains("Gebäudeeigentümer"),
        "building owner section on detail page, got {body:?}"
    );
    assert!(body.contains("building_owners/new"), "add-owner link");

    // Add the building owner via the dedicated form.
    let create = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners",
        h.base_url
    )))
    .form(&[
        ("name", "Deutsche Wohnbau SE"),
        ("email", "service@deutsche-wohnbau.example"),
        ("start_date", "1995-01-01"),
    ])
    .send()
    .await
    .expect("create building owner");
    assert_eq!(
        create.status(),
        StatusCode::SEE_OTHER,
        "create building owner redirects"
    );
    assert_eq!(
        create
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(format!("/admin/buildings/{building_id}").as_str())
    );

    // The building page lists the owner.
    let detail = basic_auth(client.get(format!("{}/admin/buildings/{building_id}", h.base_url)))
        .send()
        .await
        .expect("fetch building detail");
    let body = detail.text().await.expect("building detail body");
    assert!(
        body.contains("Deutsche Wohnbau SE"),
        "building owner listed, got {body:?}"
    );
    assert!(
        body.contains("service@deutsche-wohnbau.example"),
        "building owner e-mail listed"
    );

    // The apartment form is aware of the building owner: it no longer offers
    // the owner fields at all (the apartment belongs to the building as a
    // whole), and the apartment can be created without an owner.
    let new_page = basic_auth(client.get(format!(
        "{}/admin/buildings/{building_id}/apartments/new",
        h.base_url
    )))
    .send()
    .await
    .expect("fetch new apartment page");
    assert_eq!(new_page.status(), StatusCode::OK);
    let new_body = new_page.text().await.expect("new apartment body");
    assert!(
        new_body.contains("gehört als Ganzes dem Gebäudeeigentümer"),
        "explanation that the apartment has no own owner, got {new_body:?}"
    );
    assert!(
        !new_body.contains(r#"name="owner_name""#),
        "no owner fields for a wholly-owned building, got {new_body:?}"
    );

    let resp = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/apartments",
        h.base_url
    )))
    .form(&[
        ("name", "EG links"),
        ("description", ""),
        ("owner_name", ""),
        ("owner_email", ""),
        ("owner_start_date", ""),
    ])
    .send()
    .await
    .expect("create apartment without owner");
    assert_eq!(
        resp.status(),
        StatusCode::SEE_OTHER,
        "building-owned apartment creation redirects"
    );

    // The apartment's owner is the building owner; a tenant covers the week
    // and takes over the Kehrwoche.
    let apartment_id = queries::list_apartments(&h.pool, &building_id)
        .await
        .expect("list apartments")[0]
        .id
        .clone();
    assert_eq!(
        queries::list_ownerships(&h.pool, &apartment_id)
            .await
            .expect("list ownerships")
            .len(),
        0,
        "building-owned apartment has no individual ownership"
    );
    basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/apartments/{apartment_id}/tenancies",
        h.base_url
    )))
    .form(&[
        ("name", "Ronny Mieter"),
        ("email", "ronny@example.com"),
        ("start_date", "2026-01-01"),
    ])
    .send()
    .await
    .expect("create tenancy");

    // The schedule preview lists the tenant as assignee (delegated).
    let schedule = basic_auth(client.get(format!(
        "{}/admin/buildings/{building_id}/schedule",
        h.base_url
    )))
    .send()
    .await
    .expect("fetch schedule");
    assert_eq!(schedule.status(), StatusCode::OK);
    let schedule_body = schedule.text().await.expect("schedule body");
    assert!(
        schedule_body.contains("Ronny Mieter"),
        "tenant delegated in schedule, got {schedule_body:?}"
    );
}

/// Adding a new building owner while the previous one is still current asks
/// whether the previous building-owner period shall end on the day before the
/// new one begins. "Yes" closes it and adds the new owner; "No" is told that
/// the operation would fail. An adjacent (already tiled) start goes through
/// directly without asking.
#[tokio::test]
async fn adding_building_owner_asks_to_close_previous_building_owner() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "Musterblock").await;
    let owners_url = format!(
        "{}/admin/buildings/{building_id}/building_owners",
        h.base_url
    );

    // First building-owner period ends June 30.
    let first = basic_auth(client.post(owners_url.clone()))
        .form(&[
            ("name", "Deutsche Wohnbau SE"),
            ("email", "service@deutsche-wohnbau.example"),
            ("start_date", "2020-01-01"),
            ("end_date", "2026-06-30"),
        ])
        .send()
        .await
        .expect("create building owner");
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    // An adjacent (already tiled) start goes through directly without asking.
    let adjacent = basic_auth(client.post(owners_url.clone()))
        .form(&[
            ("name", "Berlin Wohnen GmbH"),
            ("email", "kontakt@berlin-wohnen.example"),
            ("start_date", "2026-07-01"),
        ])
        .send()
        .await
        .expect("create adjacent building owner");
    assert_eq!(adjacent.status(), StatusCode::SEE_OTHER);

    // The new building owner starts while "Berlin Wohnen" is still current:
    // the first submission asks whether it shall end on the day before.
    let ask = basic_auth(client.post(owners_url.clone()))
        .form(&[
            ("name", "Hansa Baugesellschaft"),
            ("email", "info@hansa-bau.example"),
            ("start_date", "2026-08-01"),
        ])
        .send()
        .await
        .expect("ask about previous building owner");
    assert_eq!(ask.status(), StatusCode::OK, "asking is not a rejection");
    let ask_body = ask.text().await.expect("confirmation body");
    assert!(
        ask_body.contains("Berlin Wohnen GmbH") && ask_body.contains("2026-07-31"),
        "confirmation names the previous owner and the day before, got {ask_body:?}"
    );
    assert!(
        ask_body.contains("schlägt das Anlegen des neuen Gebäudeeigentümers fehl"),
        "confirmation warns that declining fails, got {ask_body:?}"
    );
    assert!(
        ask_body.contains(r#"name="close_previous" value="yes""#)
            && ask_body.contains(r#"name="close_previous" value="no""#),
        "confirmation offers yes/no buttons, got {ask_body:?}"
    );

    // Declining ("no") tells the user the operation would fail.
    let declined = basic_auth(client.post(owners_url.clone()))
        .form(&[
            ("name", "Hansa Baugesellschaft"),
            ("email", "info@hansa-bau.example"),
            ("start_date", "2026-08-01"),
            ("close_previous", "no"),
        ])
        .send()
        .await
        .expect("decline closing the previous building owner");
    assert_eq!(declined.status(), StatusCode::BAD_REQUEST);
    assert!(
        declined
            .text()
            .await
            .expect("decline body")
            .contains("kann der neue Gebäudeeigentümer nicht angelegt werden"),
        "declining is reported as failing"
    );
    // Nothing was created.
    assert_eq!(
        queries::list_building_owners(&h.pool, &building_id)
            .await
            .expect("list building owners")
            .len(),
        2,
        "declining added no building owner"
    );

    // Confirming ("yes") closes "Berlin Wohnen" on 2026-07-31 and adds the
    // new owner.
    let confirmed = basic_auth(client.post(owners_url.clone()))
        .form(&[
            ("name", "Hansa Baugesellschaft"),
            ("email", "info@hansa-bau.example"),
            ("start_date", "2026-08-01"),
            ("close_previous", "yes"),
        ])
        .send()
        .await
        .expect("confirm closing the previous building owner");
    assert_eq!(confirmed.status(), StatusCode::SEE_OTHER);

    let owners = queries::list_building_owners(&h.pool, &building_id)
        .await
        .expect("list building owners");
    let berlin = owners
        .iter()
        .find(|o| o.name == "Berlin Wohnen GmbH")
        .expect("Berlin Wohnen");
    assert_eq!(
        berlin.end_date.as_deref(),
        Some("2026-07-31"),
        "previous building owner ends the day before"
    );
    assert!(
        owners.iter().any(|o| o.name == "Hansa Baugesellschaft"),
        "new building owner added"
    );
}

/// The current building owner cannot be deleted while apartments depend on it:
/// the delete guard of 0011 rejects it, so a building never loses its covering
/// owner through deletion. A closed predecessor (entirely in the past) is
/// still deletable.
#[tokio::test]
async fn current_building_owner_cannot_be_deleted() {
    let h = harness::start().await;
    let client = admin_client();

    // Wholly-owned building: owner A since ~400 days, one apartment relying on it.
    let a_start = days_ago(400);
    let building_id = create_wholly_owned_building(
        &h,
        &client,
        "Einzeleigentümerhaus",
        "Deutsche Wohnbau SE",
        "service@deutsche-wohnbau.example",
        &a_start,
    )
    .await;
    let _apartment_id =
        create_building_owned_apartment(&h, &client, &building_id, "EG links").await;

    // Deleting the current building owner is rejected: the building would lose
    // its covering owner while the apartment depends on it.
    let owners = queries::list_building_owners(&h.pool, &building_id)
        .await
        .expect("list building owners");
    assert_eq!(owners.len(), 1);
    let denied = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners/{}/delete",
        h.base_url, owners[0].id
    )))
    .send()
    .await
    .expect("delete current building owner");
    assert!(
        denied.status().is_client_error(),
        "deleting the current building owner must be rejected, got {}",
        denied.status()
    );
    assert!(
        denied
            .text()
            .await
            .expect("rejection body")
            .contains("derzeit abdeckt, kann nicht gelöscht werden"),
        "clear rejection message shown inline"
    );
    assert_eq!(
        queries::list_building_owners(&h.pool, &building_id)
            .await
            .expect("list building owners")
            .len(),
        1,
        "the current building owner survives the rejected delete"
    );

    // A successor taking over ~7 days ago (confirming the handover) closes the
    // predecessor; the closed, past period may then be deleted, the current
    // successor still not.
    let b_start = days_ago(7);
    let ask = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners",
        h.base_url
    )))
    .form(&[
        ("name", "Berlin Wohnen GmbH"),
        ("email", "kontakt@berlin-wohnen.example"),
        ("start_date", b_start.as_str()),
    ])
    .send()
    .await
    .expect("ask about previous building owner");
    assert_eq!(ask.status(), StatusCode::OK);
    let confirmed = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners",
        h.base_url
    )))
    .form(&[
        ("name", "Berlin Wohnen GmbH"),
        ("email", "kontakt@berlin-wohnen.example"),
        ("start_date", b_start.as_str()),
        ("close_previous", "yes"),
    ])
    .send()
    .await
    .expect("confirm closing the previous building owner");
    assert_eq!(confirmed.status(), StatusCode::SEE_OTHER);

    let owners = queries::list_building_owners(&h.pool, &building_id)
        .await
        .expect("list building owners");
    let a = owners
        .iter()
        .find(|o| o.name == "Deutsche Wohnbau SE")
        .expect("predecessor A");
    let b = owners
        .iter()
        .find(|o| o.name == "Berlin Wohnen GmbH")
        .expect("successor B");
    assert!(a.end_date.is_some(), "A is closed by the handover");

    // Deleting the closed predecessor (A, entirely in the past) is allowed.
    let past_delete = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners/{}/delete",
        h.base_url, a.id
    )))
    .send()
    .await
    .expect("delete closed predecessor");
    assert!(
        past_delete.status().is_redirection(),
        "a closed, past period stays deletable"
    );

    // Deleting the current successor (B, covers today) is still rejected even
    // though a predecessor existed before the handover.
    let still_denied = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners/{}/delete",
        h.base_url, b.id
    )))
    .send()
    .await
    .expect("delete current successor");
    assert!(
        still_denied.status().is_client_error(),
        "deleting the current building owner must be rejected even with a closed predecessor, got {}",
        still_denied.status()
    );
    assert_eq!(
        queries::list_building_owners(&h.pool, &building_id)
            .await
            .expect("list building owners")
            .len(),
        1,
        "only the current owner remains"
    );
}

/// The apartment analogue: an apartment's owner covering the current date
/// cannot be deleted, while the closed past period can.
#[tokio::test]
async fn current_apartment_owner_cannot_be_deleted() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "WEG-Haus").await;
    let past_start = days_ago(500);
    let past_end = days_ago(400);
    let current_start = days_ago(399);
    let created = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/apartments",
        h.base_url
    )))
    .form(&[
        ("name", "EG links"),
        ("description", ""),
        ("owner_name", "Ursprünglicher Eigentümer"),
        ("owner_email", "urspruenglich@example.com"),
        ("owner_start_date", past_start.as_str()),
        ("owner_end_date", past_end.as_str()),
    ])
    .send()
    .await
    .expect("create apartment with initial owner");
    assert_eq!(created.status(), StatusCode::SEE_OTHER);
    let apartment_id = created
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location")
        .trim_start_matches(&format!("/admin/buildings/{building_id}/apartments/"))
        .to_string();
    let apt_url = format!(
        "{}/admin/buildings/{building_id}/apartments/{apartment_id}",
        h.base_url
    );

    // The current owner takes over the day after the initial period ends and
    // is still open: covers today.
    let current = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Otto"),
            ("email", "otto@example.com"),
            ("start_date", current_start.as_str()),
        ])
        .send()
        .await
        .expect("add current owner");
    assert_eq!(current.status(), StatusCode::SEE_OTHER);

    let owners = queries::list_ownerships(&h.pool, &apartment_id)
        .await
        .expect("list ownerships");
    let otto = owners
        .iter()
        .find(|o| o.name == "Otto")
        .expect("current owner");
    let denied = basic_auth(client.post(format!("{apt_url}/ownerships/{}/delete", otto.id)))
        .send()
        .await
        .expect("delete current owner");
    assert!(
        denied.status().is_client_error(),
        "deleting the current apartment owner must be rejected, got {}",
        denied.status()
    );
    let denied_body = denied.text().await.expect("rejection body");
    assert!(
        denied_body.contains("derzeit abdeckt, kann nicht gelöscht werden"),
        "clear rejection message shown inline, got {denied_body:?}"
    );

    let initial = owners
        .iter()
        .find(|o| o.name == "Ursprünglicher Eigentümer")
        .expect("initial owner");
    let past_delete =
        basic_auth(client.post(format!("{apt_url}/ownerships/{}/delete", initial.id)))
            .send()
            .await
            .expect("delete past period");
    assert!(
        past_delete.status().is_redirection(),
        "a closed, past period stays deletable"
    );
    assert_eq!(
        queries::list_ownerships(&h.pool, &apartment_id)
            .await
            .expect("list ownerships")
            .len(),
        1,
        "only the current owner remains"
    );
}

/// Setting the current building owner's end date before today asks for
/// confirmation: the building would lose its covering owner. Declining
/// discards the change, confirming applies it, and an end date on/after today
/// needs no confirmation.
#[tokio::test]
async fn setting_current_building_owner_end_date_asks_confirmation() {
    let h = harness::start().await;
    let client = admin_client();

    let a_start = days_ago(400);
    let building_id = create_wholly_owned_building(
        &h,
        &client,
        "Einzeleigentümerhaus",
        "Deutsche Wohnbau SE",
        "service@deutsche-wohnbau.example",
        &a_start,
    )
    .await;
    let _apartment_id =
        create_building_owned_apartment(&h, &client, &building_id, "EG links").await;

    let owners = queries::list_building_owners(&h.pool, &building_id)
        .await
        .expect("list building owners");
    let owner_id = owners[0].id.clone();
    let owner_url = format!(
        "{}/admin/buildings/{building_id}/building_owners/{owner_id}",
        h.base_url
    );
    let update = |fields: &[(&str, &str)]| {
        let mut all = vec![
            ("name", "Deutsche Wohnbau SE"),
            ("email", "service@deutsche-wohnbau.example"),
            ("start_date", a_start.as_str()),
        ];
        all.extend_from_slice(fields);
        basic_auth(client.post(owner_url.clone())).form(&all).send()
    };

    // An end date on/after today keeps the owner covering today: no warning.
    let later = days_ago(-30);
    let future = update(&[("end_date", later.as_str())])
        .await
        .expect("set future end date");
    assert_eq!(future.status(), StatusCode::SEE_OTHER, "no warning needed");

    // An end date before today drops the coverage: warning + confirmation.
    let early = days_ago(30);
    let warn = update(&[("end_date", early.as_str())])
        .await
        .expect("set early end date");
    assert_eq!(warn.status(), StatusCode::OK, "warning is not a rejection");
    let warn_body = warn.text().await.expect("warning body");
    assert!(
        warn_body.contains("keinen Eigentümer mehr"),
        "warning about losing the owner, got {warn_body:?}"
    );
    assert!(
        warn_body.contains(r#"name="confirm_end" value="yes""#)
            && warn_body.contains(r#"name="confirm_end" value="no""#),
        "warning offers yes/no buttons, got {warn_body:?}"
    );
    assert_eq!(
        queries::get_building_owner(&h.pool, &owner_id)
            .await
            .expect("get owner")
            .expect("owner exists")
            .end_date
            .as_deref(),
        Some(later.as_str()),
        "the early end date is not applied yet"
    );

    // Declining discards the change.
    let declined = update(&[("end_date", early.as_str()), ("confirm_end", "no")])
        .await
        .expect("decline early end");
    assert_eq!(declined.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        queries::get_building_owner(&h.pool, &owner_id)
            .await
            .expect("get owner")
            .expect("owner exists")
            .end_date
            .as_deref(),
        Some(later.as_str()),
        "declined end date is not applied"
    );

    // Confirming applies the end date.
    let confirmed = update(&[("end_date", early.as_str()), ("confirm_end", "yes")])
        .await
        .expect("confirm early end");
    assert_eq!(confirmed.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        queries::get_building_owner(&h.pool, &owner_id)
            .await
            .expect("get owner")
            .expect("owner exists")
            .end_date
            .as_deref(),
        Some(early.as_str()),
        "confirmed end date is applied"
    );
}

/// The apartment analogue: setting the current owner's end date before today
/// asks for confirmation; declining discards, confirming applies.
#[tokio::test]
async fn setting_current_apartment_owner_end_date_asks_confirmation() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "WEG-Haus").await;
    let past_start = days_ago(500);
    let past_end = days_ago(400);
    let current_start = days_ago(399);
    let created = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/apartments",
        h.base_url
    )))
    .form(&[
        ("name", "EG links"),
        ("description", ""),
        ("owner_name", "Ursprünglicher Eigentümer"),
        ("owner_email", "urspruenglich@example.com"),
        ("owner_start_date", past_start.as_str()),
        ("owner_end_date", past_end.as_str()),
    ])
    .send()
    .await
    .expect("create apartment with initial owner");
    assert_eq!(created.status(), StatusCode::SEE_OTHER);
    let apartment_id = created
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location")
        .trim_start_matches(&format!("/admin/buildings/{building_id}/apartments/"))
        .to_string();
    let apt_url = format!(
        "{}/admin/buildings/{building_id}/apartments/{apartment_id}",
        h.base_url
    );
    let _current = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Otto"),
            ("email", "otto@example.com"),
            ("start_date", current_start.as_str()),
        ])
        .send()
        .await
        .expect("add current owner");

    let owners = queries::list_ownerships(&h.pool, &apartment_id)
        .await
        .expect("list ownerships");
    let otto = owners
        .iter()
        .find(|o| o.name == "Otto")
        .expect("current owner");
    let owner_url = format!("{apt_url}/ownerships/{}", otto.id);
    let update = |fields: &[(&str, &str)]| {
        let mut all = vec![
            ("name", "Otto"),
            ("email", "otto@example.com"),
            ("start_date", current_start.as_str()),
        ];
        all.extend_from_slice(fields);
        basic_auth(client.post(owner_url.clone())).form(&all).send()
    };

    // An end date on/after today: no warning.
    let later = days_ago(-30);
    let future = update(&[("end_date", later.as_str())])
        .await
        .expect("set future end date");
    assert_eq!(future.status(), StatusCode::SEE_OTHER, "no warning needed");

    // An end date before today: warning + confirmation.
    let early = days_ago(30);
    let warn = update(&[("end_date", early.as_str())])
        .await
        .expect("set early end date");
    assert_eq!(warn.status(), StatusCode::OK, "warning is not a rejection");
    let warn_body = warn.text().await.expect("warning body");
    assert!(
        warn_body.contains("keinen Eigentümer mehr"),
        "warning about losing the owner, got {warn_body:?}"
    );
    assert!(
        warn_body.contains(r#"name="confirm_end" value="yes""#)
            && warn_body.contains(r#"name="confirm_end" value="no""#),
        "warning offers yes/no buttons, got {warn_body:?}"
    );

    let declined = update(&[("end_date", early.as_str()), ("confirm_end", "no")])
        .await
        .expect("decline early end");
    assert_eq!(declined.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        queries::get_ownership(&h.pool, &otto.id)
            .await
            .expect("get ownership")
            .expect("ownership exists")
            .end_date
            .as_deref(),
        Some(later.as_str()),
        "declined end date is not applied"
    );

    let confirmed = update(&[("end_date", early.as_str()), ("confirm_end", "yes")])
        .await
        .expect("confirm early end");
    assert_eq!(confirmed.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        queries::get_ownership(&h.pool, &otto.id)
            .await
            .expect("get ownership")
            .expect("ownership exists")
            .end_date
            .as_deref(),
        Some(early.as_str()),
        "confirmed end date is applied"
    );
}

/// A chain-gap rejection names the previous period's end date and the
/// required next start, so the user sees why e.g. 1.9. is not the day after
/// 30.8. (end dates are inclusive; the successor must start on 31.8.).
#[tokio::test]
async fn gap_rejection_names_the_previous_end_date() {
    let h = harness::start().await;
    let client = admin_client();

    // Relative dates: previous period ends `prev` (two days before `too_late`),
    // so the successor would have to start on `next` — `too_late` skips a day.
    let prev = days_ago(30);
    let next = days_ago(29);
    let too_late = days_ago(28);

    // Building owners.
    let building_id = create_building(&h, &client, "Einzeleigentümerhaus").await;
    let first = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners",
        h.base_url
    )))
    .form(&[
        ("name", "Deutsche Wohnbau SE"),
        ("email", "service@deutsche-wohnbau.example"),
        ("start_date", days_ago(400).as_str()),
        ("end_date", prev.as_str()),
    ])
    .send()
    .await
    .expect("create first building owner");
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    let skip = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners",
        h.base_url
    )))
    .form(&[
        ("name", "Berlin Wohnen GmbH"),
        ("email", "kontakt@berlin-wohnen.example"),
        ("start_date", too_late.as_str()),
    ])
    .send()
    .await
    .expect("create building owner in a gap");
    assert_eq!(skip.status(), StatusCode::BAD_REQUEST);
    let body = skip.text().await.expect("rejection body");
    assert!(
        body.contains(&format!("endet am {prev}"))
            && body.contains(&format!("muss am {next} liegen")),
        "gap rejection names the previous end and the required start, got {body:?}"
    );

    // Apartment ownerships.
    let weg = create_building(&h, &client, "WEG-Haus").await;
    let apt = basic_auth(client.post(format!("{}/admin/buildings/{weg}/apartments", h.base_url)))
        .form(&[
            ("name", "EG links"),
            ("description", ""),
            ("owner_name", "Ursprünglicher Eigentümer"),
            ("owner_email", "urspruenglich@example.com"),
            ("owner_start_date", days_ago(500).as_str()),
            ("owner_end_date", prev.as_str()),
        ])
        .send()
        .await
        .expect("create apartment with initial owner");
    assert_eq!(apt.status(), StatusCode::SEE_OTHER);
    let apartment_id = apt
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location")
        .trim_start_matches(&format!("/admin/buildings/{weg}/apartments/"))
        .to_string();

    let skip = basic_auth(client.post(format!(
        "{}/admin/buildings/{weg}/apartments/{apartment_id}/ownerships",
        h.base_url
    )))
    .form(&[
        ("name", "Otto"),
        ("email", "otto@example.com"),
        ("start_date", too_late.as_str()),
    ])
    .send()
    .await
    .expect("create ownership in a gap");
    assert_eq!(skip.status(), StatusCode::BAD_REQUEST);
    let body = skip.text().await.expect("rejection body");
    assert!(
        body.contains(&format!("endet am {prev}"))
            && body.contains(&format!("muss am {next} liegen")),
        "ownership gap rejection names the previous end and the required start, got {body:?}"
    );

    // Starting on the day after the previous end is accepted.
    let tiled = basic_auth(client.post(format!(
        "{}/admin/buildings/{weg}/apartments/{apartment_id}/ownerships",
        h.base_url
    )))
    .form(&[
        ("name", "Otto"),
        ("email", "otto@example.com"),
        ("start_date", next.as_str()),
    ])
    .send()
    .await
    .expect("create ownership on the day after");
    assert_eq!(tiled.status(), StatusCode::SEE_OTHER);
}

/// One person can own a whole building AND an apartment in another building
/// (WEG): both roles share a single `people` row, resolved by e-mail. The
/// building form collects the initial building owner inline (no separate
/// owner-creation step), and renaming the person through one period updates
/// every period at once.
#[tokio::test]
async fn owner_person_is_shared_across_building_and_apartment() {
    let h = harness::start().await;
    let client = admin_client();

    // The new-building form offers the ownership structure as a visual
    // choice and known owners as suggestions.
    let form = basic_auth(client.get(format!("{}/admin/buildings/new", h.base_url)))
        .send()
        .await
        .expect("fetch new-building page");
    let form_body = form.text().await.expect("new-building body");
    assert!(
        form_body.contains("ownership_style"),
        "ownership-structure radio group, got {form_body:?}"
    );
    assert!(
        form_body.contains("Gebäudeeigentümer"),
        "owner option, got {form_body:?}"
    );
    assert!(
        form_body.contains(r#"<datalist id="people-names">"#),
        "suggestion list on the new-building form"
    );

    // Create the building together with its owner — one form, one step.
    let created = basic_auth(client.post(format!("{}/admin/buildings", h.base_url)))
        .form(&[
            ("name", "Wohnblock am Park"),
            ("description", ""),
            ("admin_name", "Alice"),
            ("admin_email", "alice@example.com"),
            ("ownership_style", "building"),
            ("owner_name", "Deutsche Wohnbau SE"),
            ("owner_email", "service@deutsche-wohnbau.example"),
            ("owner_start_date", "1995-01-01"),
        ])
        .send()
        .await
        .expect("create building with owner");
    assert_eq!(
        created.status(),
        StatusCode::SEE_OTHER,
        "building with owner redirects"
    );
    let location = created
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .to_string();
    let building_id = location.trim_start_matches("/admin/buildings/").to_string();

    let people = queries::list_people(&h.pool).await.expect("list people");
    // The Ansprechpartner (Alice) is a person too since 0008; the building
    // owner is a second person row.
    assert_eq!(people.len(), 2, "admin + building owner, got {people:?}");
    let owner_person = people
        .iter()
        .find(|p| p.email == "service@deutsche-wohnbau.example")
        .expect("building-owner person");
    assert_eq!(owner_person.name, "Deutsche Wohnbau SE");

    // The building page shows the owner; the apartment form is aware of it.
    let page = basic_auth(client.get(format!("{}/admin/buildings/{building_id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page");
    let body = page.text().await.expect("building body");
    assert!(
        body.contains("Deutsche Wohnbau SE"),
        "building owner listed, got {body:?}"
    );

    // The dedicated building-owner form renders with the suggestion lists
    // (it loads the person master data like the other owner forms).
    let owner_form = basic_auth(client.get(format!(
        "{}/admin/buildings/{building_id}/building_owners/new",
        h.base_url
    )))
    .send()
    .await
    .expect("fetch building-owner form")
    .text()
    .await
    .expect("building-owner form body");
    assert!(
        owner_form.contains(r#"<datalist id="people-names">"#),
        "suggestion list on the building-owner form, got {owner_form:?}"
    );

    // The same person owns an apartment in a second building (a WEG). The
    // person is reused via the e-mail address; the newer name becomes the
    // person's current contact name.
    let second_building = create_building(&h, &client, "Musterblock").await;
    let apartment = basic_auth(client.post(format!(
        "{}/admin/buildings/{second_building}/apartments",
        h.base_url
    )))
    .form(&[
        ("name", "EG links"),
        ("description", ""),
        ("owner_name", "Deutsche Wohnbau AG"),
        ("owner_email", "service@deutsche-wohnbau.example"),
        ("owner_start_date", "2026-01-01"),
    ])
    .send()
    .await
    .expect("create apartment with known owner");
    assert_eq!(apartment.status(), StatusCode::SEE_OTHER);

    let people = queries::list_people(&h.pool).await.expect("list people");
    assert_eq!(
        people.len(),
        2,
        "same e-mail must not create a second person, got {people:?}"
    );
    assert_eq!(
        people
            .iter()
            .find(|p| p.email == "service@deutsche-wohnbau.example")
            .expect("owner person")
            .name,
        "Deutsche Wohnbau AG"
    );

    // Both the building-owner period and the apartment ownership point at the
    // same person row.
    let building_owners = queries::list_building_owners(&h.pool, &building_id)
        .await
        .expect("list building owners");
    assert_eq!(building_owners.len(), 1);
    let apartment_id = queries::list_apartments(&h.pool, &second_building)
        .await
        .expect("list apartments")[0]
        .id
        .clone();
    let ownerships = queries::list_ownerships(&h.pool, &apartment_id)
        .await
        .expect("list ownerships");
    assert_eq!(ownerships.len(), 1);
    assert_eq!(building_owners[0].person_id, ownerships[0].person_id);
    assert_eq!(building_owners[0].name, "Deutsche Wohnbau AG");

    // The ownership edit page renders with the suggestion lists and the
    // person's current name pre-filled.
    let ownership_edit = basic_auth(client.get(format!(
        "{}/admin/buildings/{second_building}/apartments/{apartment_id}/ownerships/{}/edit",
        h.base_url, ownerships[0].id
    )))
    .send()
    .await
    .expect("fetch ownership edit page")
    .text()
    .await
    .expect("ownership edit body");
    assert!(
        ownership_edit.contains(r#"<datalist id="people-names">"#),
        "suggestion list on the ownership edit page, got {ownership_edit:?}"
    );
    assert!(
        ownership_edit.contains("Deutsche Wohnbau AG"),
        "person name pre-filled"
    );
    // The building page (which shows the building owner) reflects the shared
    // person's current name.
    let page = basic_auth(client.get(format!("{}/admin/buildings/{building_id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page");
    assert!(
        page.text()
            .await
            .expect("building body")
            .contains("Deutsche Wohnbau AG"),
        "building page shows the person's current name"
    );

    // Renaming the person through the building owner edit updates the
    // apartment's ownership listing as well.
    let owner_id = &building_owners[0].id;
    let rename = basic_auth(client.post(format!(
        "{}/admin/buildings/{building_id}/building_owners/{owner_id}",
        h.base_url
    )))
    .form(&[
        ("name", "Deutsche Wohnbau SE & Co. KG"),
        ("email", "service@deutsche-wohnbau.example"),
        ("start_date", "1995-01-01"),
        ("end_date", ""),
    ])
    .send()
    .await
    .expect("rename building owner");
    assert_eq!(rename.status(), StatusCode::SEE_OTHER);

    let ownerships = queries::list_ownerships(&h.pool, &apartment_id)
        .await
        .expect("list ownerships after rename");
    assert_eq!(ownerships[0].name, "Deutsche Wohnbau SE & Co. KG");
    let page = basic_auth(client.get(format!(
        "{}/admin/buildings/{second_building}/apartments/{apartment_id}",
        h.base_url
    )))
    .send()
    .await
    .expect("fetch apartment page");
    assert!(
        page.text()
            .await
            .expect("apartment body")
            .contains("Deutsche Wohnbau SE &#38; Co. KG"),
        "apartment page shows the renamed person (HTML-escaped)"
    );
    assert_eq!(
        queries::list_people(&h.pool)
            .await
            .expect("list people")
            .len(),
        2,
        "rename keeps the two person rows (admin + owner)"
    );
}

/// The people pages list every person with their roles and link to the
/// objects they refer to — the Ansprechpartner, building owner, apartment
/// owner and tenant views all point at the same person row.
#[tokio::test]
async fn people_page_lists_roles_and_links() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "Haus Sonnenschein").await;
    let apartment_id = create_apartment(&h, &client, &building_id, "EG links").await;
    let apt_url = format!(
        "{}/admin/buildings/{building_id}/apartments/{apartment_id}",
        h.base_url
    );

    // A tenant whose e-mail matches the apartment's first owner, so that one
    // person back two roles. The building owner lives in a second, wholly-
    // owned building: the two ownership forms are mutually exclusive.
    let owned_id = create_building(&h, &client, "Geschäftshaus").await;
    basic_auth(client.post(format!(
        "{}/admin/buildings/{owned_id}/building_owners",
        h.base_url
    )))
    .form(&[
        ("name", "Deutsche Wohnbau SE"),
        ("email", "service@deutsche-wohnbau.example"),
        ("start_date", "1995-01-01"),
    ])
    .send()
    .await
    .expect("create building owner");
    basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Ursprünglicher Eigentümer"),
            ("email", "urspruenglich@example.com"),
            ("start_date", "2026-02-01"),
        ])
        .send()
        .await
        .expect("create tenancy");

    // Alice (the Ansprechpartner), the owner/tenant person, and the building
    // owner company "Deutsche Wohnbau SE"".
    let people = queries::list_people(&h.pool).await.expect("list people");
    assert_eq!(
        people.len(),
        3,
        "roles of three distinct persons, got {people:?}"
    );

    // The index lists everyone, linking to the detail pages.
    let index = basic_auth(client.get(format!("{}/admin/people", h.base_url)))
        .send()
        .await
        .expect("fetch people index");
    assert_eq!(index.status(), StatusCode::OK);
    let index_body = index.text().await.expect("people index body");
    for name in ["Alice", "Ursprünglicher Eigentümer", "Deutsche Wohnbau SE"] {
        assert!(
            index_body.contains(name),
            "{name} listed, got {index_body:?}"
        );
    }
    assert!(
        index_body.contains("/admin/people/"),
        "index links to the detail pages, got {index_body:?}"
    );
    // The index lists roles instead of the e-mail; the e-mail is only in the
    // markup for wide viewports.
    assert!(
        index_body.contains("Ansprechpartner von Haus Sonnenschein"),
        "roles summarized on the index, got {index_body:?}"
    );
    assert!(
        index_body.contains("Eigentümer von EG links"),
        "apartment-owner role in the summary, got {index_body:?}"
    );
    assert!(
        index_body.contains(r#"class="email-if-space""#),
        "e-mail column only rendered for wide viewports, got {index_body:?}"
    );

    // The apartment page links owner and tenant names to their person pages.
    let apartment = basic_auth(client.get(apt_url.clone()))
        .send()
        .await
        .expect("fetch apartment page")
        .text()
        .await
        .expect("apartment body");
    assert!(
        apartment.contains("/admin/people/") && apartment.contains("Ursprünglicher Eigentümer"),
        "apartment roles link to the person page, got {apartment:?}"
    );

    // The person page of the owner/tenant person shows both roles with links
    // to the objects.
    let person = people
        .iter()
        .find(|p| p.name == "Ursprünglicher Eigentümer")
        .expect("owner/tenant person");
    let detail = basic_auth(client.get(format!("{}/admin/people/{}", h.base_url, person.id)))
        .send()
        .await
        .expect("fetch person page");
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body = detail.text().await.expect("person page body");
    assert!(
        detail_body.contains("Eigentümer von") && detail_body.contains("Mieter von"),
        "both roles listed, got {detail_body:?}"
    );
    assert!(
        detail_body.contains(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        )),
        "role links to the apartment page, got {detail_body:?}"
    );

    // The building pages link the Ansprechpartner and the building owner to
    // their person pages.
    let building = basic_auth(client.get(format!("{}/admin/buildings/{building_id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page")
        .text()
        .await
        .expect("building body");
    assert!(
        building.contains("/admin/people/") && building.contains("Alice"),
        "Ansprechpartner links to the person page, got {building:?}"
    );
    let owned_building =
        basic_auth(client.get(format!("{}/admin/buildings/{owned_id}", h.base_url)))
            .send()
            .await
            .expect("fetch owned building page")
            .text()
            .await
            .expect("owned building body");
    assert!(
        owned_building.contains("/admin/people/") && owned_building.contains("Deutsche Wohnbau SE"),
        "building owner links to the person page, got {owned_building:?}"
    );

    // A person page of the company person renders its building-owner role.
    let company = people
        .iter()
        .find(|p| p.name == "Deutsche Wohnbau SE")
        .expect("company person");
    let company_page =
        basic_auth(client.get(format!("{}/admin/people/{}", h.base_url, company.id)))
            .send()
            .await
            .expect("fetch company person page")
            .text()
            .await
            .expect("company person body");
    assert!(
        company_page.contains("Gebäudeeigentümer von") && company_page.contains("Geschäftshaus"),
        "building-owner role listed, got {company_page:?}"
    );
    assert!(
        company_page.contains("seit 1995-01-01"),
        "open period shown"
    );

    // Unknown person ids 404.
    let missing = basic_auth(client.get(format!("{}/admin/people/does-not-exist", h.base_url)))
        .send()
        .await
        .expect("fetch unknown person");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

/// The person page's contact form edits name and e-mail directly: the change
/// takes effect everywhere the person appears (ownership, tenancy, admin),
/// and an e-mail that already belongs to another person is rejected.
#[tokio::test]
async fn person_can_be_edited_via_its_page() {
    let h = harness::start().await;
    let client = admin_client();

    let building_id = create_building(&h, &client, "Haus Sonnenschein").await;
    let apartment_id = create_apartment(&h, &client, &building_id, "EG links").await;
    let apt_url = format!(
        "{}/admin/buildings/{building_id}/apartments/{apartment_id}",
        h.base_url
    );

    // The page of the Ansprechpartner shows the contact form.
    let alice = queries::list_people(&h.pool)
        .await
        .expect("list people")
        .into_iter()
        .find(|p| p.email == "alice@example.com")
        .expect("Alice person");
    let page = basic_auth(client.get(format!("{}/admin/people/{}", h.base_url, alice.id)))
        .send()
        .await
        .expect("fetch person page");
    let page_body = page.text().await.expect("person page body");
    assert!(
        page_body.contains("Kontaktdaten") && page_body.contains(r#"value="Alice""#),
        "contact form with the current name, got {page_body:?}"
    );

    // Rename the person via the form; the building page and the apartment
    // page follow, because all roles share the person row.
    let rename = basic_auth(client.post(format!("{}/admin/people/{}", h.base_url, alice.id)))
        .form(&[("name", "Alice Liddell"), ("email", "alice@example.com")])
        .send()
        .await
        .expect("rename person");
    assert_eq!(
        rename.status(),
        StatusCode::SEE_OTHER,
        "person update redirects back to the person page"
    );
    assert_eq!(
        rename
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(format!("/admin/people/{}", alice.id).as_str())
    );

    let building = basic_auth(client.get(format!("{}/admin/buildings/{building_id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page")
        .text()
        .await
        .expect("building body");
    assert!(
        building.contains("Alice Liddell"),
        "renamed Ansprechpartner on the building page, got {building:?}"
    );

    // The person page itself shows the new name in the header and the form.
    let page = basic_auth(client.get(format!("{}/admin/people/{}", h.base_url, alice.id)))
        .send()
        .await
        .expect("fetch person page")
        .text()
        .await
        .expect("person page body");
    assert!(
        page.contains("Alice Liddell") && page.contains(r#"value="alice@example.com""#),
        "renamed person on its page, got {page:?}"
    );

    // An e-mail that belongs to another person is rejected inline.
    let owner = queries::list_people(&h.pool)
        .await
        .expect("list people")
        .into_iter()
        .find(|p| p.email == "urspruenglich@example.com")
        .expect("owner person");
    let conflict = basic_auth(client.post(format!("{}/admin/people/{}", h.base_url, alice.id)))
        .form(&[
            ("name", "Alice Liddell"),
            ("email", "urspruenglich@example.com"),
        ])
        .send()
        .await
        .expect("conflicting person update");
    assert_eq!(
        conflict.status(),
        StatusCode::BAD_REQUEST,
        "an e-mail of another person must be rejected"
    );
    let conflict_body = conflict.text().await.expect("conflict body");
    assert!(
        conflict_body.contains("existiert bereits"),
        "duplicate e-mail message shown inline, got {conflict_body:?}"
    );
    assert!(
        conflict_body.contains(r#"value="urspruenglich@example.com""#),
        "submitted values preserved after the rejection, got {conflict_body:?}"
    );

    // Alice is unchanged (the person page still shows the old values).
    let alice_after = queries::get_person(&h.pool, &alice.id)
        .await
        .expect("get person")
        .expect("Alice exists");
    assert_eq!(alice_after.name, "Alice Liddell");
    assert_eq!(alice_after.email, "alice@example.com");

    // A malformed address is rejected with the database's message as well;
    // the owner person is untouched and still the same row.
    let malformed = basic_auth(client.post(format!("{}/admin/people/{}", h.base_url, owner.id)))
        .form(&[
            ("name", "Ursprünglicher Eigentümer"),
            ("email", "no-at.example"),
        ])
        .send()
        .await
        .expect("malformed person update");
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert!(
        malformed
            .text()
            .await
            .expect("malformed body")
            .contains("E-Mail-Adresse"),
        "malformed e-mail message shown inline"
    );
    assert_eq!(
        queries::list_people(&h.pool)
            .await
            .expect("list people")
            .len(),
        2
    );

    // The apartment page still shows the owner's name from the shared row.
    let apartment = basic_auth(client.get(apt_url.clone()))
        .send()
        .await
        .expect("fetch apartment page")
        .text()
        .await
        .expect("apartment body");
    assert!(
        apartment.contains("Ursprünglicher Eigentümer"),
        "owner unchanged, got {apartment:?}"
    );
}

/// The ownership structure is a visual choice when creating a building:
/// either one owner for the whole building (then the apartments legally have
/// no own owners, and the apartment page does not offer "Eigentümer
/// hinzufügen") or individually owned flats (WEG). Owner fields submitted
/// with the WEG variant are ignored.
#[tokio::test]
async fn building_ownership_choice_governs_the_apartment_page() {
    let h = harness::start().await;
    let client = admin_client();

    // The form offers both variants; the owner fields are hidden behind the
    // "one building owner" choice (default: individually owned flats).
    let form = basic_auth(client.get(format!("{}/admin/buildings/new", h.base_url)))
        .send()
        .await
        .expect("fetch new-building page")
        .text()
        .await
        .expect("new-building body");
    assert!(
        form.contains(r#"<input type="radio" name="ownership_style" value="apartments" checked"#),
        "apartments variant preselected, got {form:?}"
    );
    assert!(
        form.contains(r#"id="building-owner-fields" class="hidden""#),
        "owner fields hidden for the apartments variant, got {form:?}"
    );

    let create = |fields: &[(&str, &str)]| {
        let mut all = vec![
            ("name", "Testhaus"),
            ("description", ""),
            ("admin_name", ""),
            ("admin_email", ""),
        ];
        all.extend_from_slice(fields);
        basic_auth(client.post(format!("{}/admin/buildings", h.base_url)))
            .form(&all)
            .send()
    };

    // WEG variant: apartments get their own owners and the apartment page
    // offers "Eigentümer hinzufügen".
    let weg = create(&[("ownership_style", "apartments")])
        .await
        .expect("create WEG building");
    assert_eq!(weg.status(), StatusCode::SEE_OTHER);
    let weg_id = weg
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location")
        .trim_start_matches("/admin/buildings/")
        .to_string();
    let apartment_id = create_apartment(&h, &client, &weg_id, "EG links").await;
    let weg_apt = basic_auth(client.get(format!(
        "{}/admin/buildings/{weg_id}/apartments/{apartment_id}",
        h.base_url
    )))
    .send()
    .await
    .expect("fetch WEG apartment page")
    .text()
    .await
    .expect("WEG apartment body");
    assert!(
        weg_apt.contains("Eigentümer hinzufügen"),
        "per-apartment owners offered for the WEG variant, got {weg_apt:?}"
    );

    // Because the flats are individually owned, the building page must not
    // offer a building owner anymore.
    let weg_building = basic_auth(client.get(format!("{}/admin/buildings/{weg_id}", h.base_url)))
        .send()
        .await
        .expect("fetch WEG building page")
        .text()
        .await
        .expect("WEG building body");
    assert!(
        !weg_building.contains("Eigentümer hinzufügen"),
        "no building owner once flats are individually owned, got {weg_building:?}"
    );
    assert!(
        weg_building.contains("solange Wohnungen eigene Eigentümer haben"),
        "the WEG explanation is shown on the building page, got {weg_building:?}"
    );

    // "One building owner" requires the owner fields.
    let missing_owner = create(&[("ownership_style", "building")])
        .await
        .expect("create building without owner fields");
    assert_eq!(
        missing_owner.status(),
        StatusCode::BAD_REQUEST,
        "building-owner variant without owner fields must be rejected"
    );
    let missing_owner = missing_owner.text().await.expect("rejection body");
    assert!(
        missing_owner.contains("müssen Name, E-Mail-Adresse und Beginn"),
        "clear message for the missing owner fields, got {missing_owner:?}"
    );

    // With the owner given, the building is wholly owned: apartments can be
    // created without an owner and the apartment page does not offer "Eigentümer
    // hinzufügen".
    let owned = create(&[
        ("ownership_style", "building"),
        ("owner_name", "Deutsche Wohnbau SE"),
        ("owner_email", "service@deutsche-wohnbau.example"),
        ("owner_start_date", "1995-01-01"),
    ])
    .await
    .expect("create wholly-owned building");
    assert_eq!(owned.status(), StatusCode::SEE_OTHER);
    let owned_id = owned
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location")
        .trim_start_matches("/admin/buildings/")
        .to_string();
    let owned_apt = basic_auth(client.post(format!(
        "{}/admin/buildings/{owned_id}/apartments",
        h.base_url
    )))
    .form(&[
        ("name", "EG links"),
        ("description", ""),
        ("owner_name", ""),
        ("owner_email", ""),
        ("owner_start_date", ""),
        ("owner_end_date", ""),
    ])
    .send()
    .await
    .expect("create building-owned apartment");
    assert_eq!(owned_apt.status(), StatusCode::SEE_OTHER);

    // The new-apartment form of a wholly-owned building does not offer the
    // owner fields at all…
    let owned_form = basic_auth(client.get(format!(
        "{}/admin/buildings/{owned_id}/apartments/new",
        h.base_url
    )))
    .send()
    .await
    .expect("fetch new apartment page")
    .text()
    .await
    .expect("new apartment body");
    assert!(
        !owned_form.contains(r#"name="owner_name""#),
        "no owner fields for a wholly-owned building, got {owned_form:?}"
    );

    // …and even a hand-crafted submission with owner fields is rejected by
    // the database (the ownership forms are mutually exclusive).
    let crafted = basic_auth(client.post(format!(
        "{}/admin/buildings/{owned_id}/apartments",
        h.base_url
    )))
    .form(&[
        ("name", "EG links"),
        ("description", ""),
        ("owner_name", "Alice"),
        ("owner_email", "alice@example.com"),
        ("owner_start_date", "2026-01-01"),
        ("owner_end_date", ""),
    ])
    .send()
    .await
    .expect("crafted apartment-with-owner submission");
    assert_eq!(
        crafted.status(),
        StatusCode::BAD_REQUEST,
        "apartment with owner in a wholly-owned building must be rejected"
    );
    assert!(
        crafted
            .text()
            .await
            .expect("crafted rejection body")
            .contains("keinen eigenen Eigentümer"),
        "the database's mutual-exclusivity message is shown inline"
    );
    assert_eq!(
        queries::list_apartments(&h.pool, &owned_id)
            .await
            .expect("list apartments")
            .len(),
        1,
        "the rejected create added no apartment (the earlier building-owned one remains)"
    );

    let owned_apartment_id = queries::list_apartments(&h.pool, &owned_id)
        .await
        .expect("list apartments")[0]
        .id
        .clone();
    let page = basic_auth(client.get(format!(
        "{}/admin/buildings/{owned_id}/apartments/{owned_apartment_id}",
        h.base_url
    )))
    .send()
    .await
    .expect("fetch building-owned apartment page")
    .text()
    .await
    .expect("building-owned apartment body");
    assert!(
        !page.contains("Eigentümer hinzufügen"),
        "no per-apartment owner form for a wholly-owned building, got {page:?}"
    );
    assert!(
        page.contains("gehört als Ganzes dem Gebäudeeigentümer"),
        "the WEG-split explanation is shown, got {page:?}"
    );

    // The apartments have no dedicated owners, so the building page still
    // offers adding/seeing a building owner.
    let owned_building =
        basic_auth(client.get(format!("{}/admin/buildings/{owned_id}", h.base_url)))
            .send()
            .await
            .expect("fetch wholly-owned building page")
            .text()
            .await
            .expect("wholly-owned building body");
    assert!(
        owned_building.contains("Eigentümer hinzufügen")
            && owned_building.contains("Deutsche Wohnbau SE"),
        "building owner still offered and listed, got {owned_building:?}"
    );

    // Owner fields submitted with the apartments variant are ignored.
    let ignored = create(&[
        ("ownership_style", "apartments"),
        ("owner_name", "Ignored GmbH"),
        ("owner_email", "ignored@example.com"),
        ("owner_start_date", "2026-01-01"),
    ])
    .await
    .expect("create WEG building with stray owner fields");
    assert_eq!(ignored.status(), StatusCode::SEE_OTHER);
    let ignored_id = ignored
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .expect("Location")
        .trim_start_matches("/admin/buildings/")
        .to_string();
    assert_eq!(
        queries::list_building_owners(&h.pool, &ignored_id)
            .await
            .expect("list building owners")
            .len(),
        0,
        "owner fields must be ignored for the apartments variant"
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

/// Adding a new tenant while the previous tenancy is still current asks
/// whether the previous tenancy shall end on the day before the new one
/// begins. "Yes" closes it and adds the new tenant; "No" is told that the
/// operation would fail. An adjacent (non-overlapping) start goes through
/// directly without asking.
#[tokio::test]
async fn adding_tenant_asks_to_close_previous_tenancy() {
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

    // An adjacent (non-overlapping) tenancy is accepted without asking.
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

    // The new tenant starts while Karl is still current: the first submission
    // asks whether Karl's tenancy shall end on the day before.
    let ask = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Petra"),
            ("email", "petra@example.com"),
            ("start_date", "2026-08-01"),
        ])
        .send()
        .await
        .expect("ask about previous tenancy");
    assert_eq!(ask.status(), StatusCode::OK, "asking is not a rejection");
    let ask_body = ask.text().await.expect("confirmation body");
    assert!(
        ask_body.contains("Karl") && ask_body.contains("2026-07-31"),
        "confirmation names the previous tenant and the day before, got {ask_body:?}"
    );
    assert!(
        ask_body.contains("schlägt das Anlegen des neuen Mietverhältnisses fehl"),
        "confirmation warns that declining fails, got {ask_body:?}"
    );
    assert!(
        ask_body.contains(r#"name="close_previous" value="yes""#)
            && ask_body.contains(r#"name="close_previous" value="no""#),
        "confirmation offers yes/no buttons, got {ask_body:?}"
    );

    // Declining ("no") tells the user the operation would fail.
    let declined = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Petra"),
            ("email", "petra@example.com"),
            ("start_date", "2026-08-01"),
            ("close_previous", "no"),
        ])
        .send()
        .await
        .expect("decline closing the previous tenancy");
    assert_eq!(declined.status(), StatusCode::BAD_REQUEST);
    assert!(
        declined
            .text()
            .await
            .expect("decline body")
            .contains("kann das neue Mietverhältnis nicht angelegt werden"),
        "declining is reported as failing"
    );
    // Nothing was created.
    assert_eq!(
        queries::list_tenancies(&h.pool, &apartment_id)
            .await
            .expect("list tenancies")
            .len(),
        2,
        "declining added no tenancy"
    );

    // Confirming ("yes") closes Karl on 2026-07-31 and adds Petra.
    let confirmed = basic_auth(client.post(format!("{apt_url}/tenancies")))
        .form(&[
            ("name", "Petra"),
            ("email", "petra@example.com"),
            ("start_date", "2026-08-01"),
            ("close_previous", "yes"),
        ])
        .send()
        .await
        .expect("confirm closing the previous tenancy");
    assert_eq!(confirmed.status(), StatusCode::SEE_OTHER);

    let tenancies = queries::list_tenancies(&h.pool, &apartment_id)
        .await
        .expect("list tenancies");
    let karl = tenancies
        .iter()
        .find(|t| t.name == "Karl")
        .expect("Karl tenancy");
    assert_eq!(
        karl.end_date.as_deref(),
        Some("2026-07-31"),
        "previous tenancy ends the day before"
    );
    assert!(
        tenancies.iter().any(|t| t.name == "Petra"),
        "new tenant added"
    );

    // Updating a tenancy into an overlapping period is still rejected.
    let petra_id = tenancies
        .iter()
        .find(|t| t.name == "Petra")
        .expect("Petra tenancy")
        .id
        .clone();
    let update = basic_auth(client.post(format!("{apt_url}/tenancies/{petra_id}")))
        .form(&[
            ("name", "Petra"),
            ("email", "petra@example.com"),
            ("start_date", "2026-07-15"),
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

/// Adding a new owner while the previous ownership is still current asks
/// whether the previous ownership shall end on the day before the new one
/// begins. "Yes" closes it and adds the new owner; "No" is told that the
/// operation would fail. An adjacent (already tiled) start goes through
/// directly without asking.
#[tokio::test]
async fn adding_owner_asks_to_close_previous_ownership() {
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

    // An adjacent (already tiled) start goes through directly without asking.
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

    // The new owner starts while Karl is still current: the first submission
    // asks whether Karl's ownership shall end on the day before.
    let ask = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Karla"),
            ("email", "karla@example.com"),
            ("start_date", "2026-08-01"),
        ])
        .send()
        .await
        .expect("ask about previous ownership");
    assert_eq!(ask.status(), StatusCode::OK, "asking is not a rejection");
    let ask_body = ask.text().await.expect("confirmation body");
    assert!(
        ask_body.contains("Karl") && ask_body.contains("2026-07-31"),
        "confirmation names the previous owner and the day before, got {ask_body:?}"
    );
    assert!(
        ask_body.contains("schlägt das Anlegen des neuen Eigentums fehl"),
        "confirmation warns that declining fails, got {ask_body:?}"
    );
    assert!(
        ask_body.contains(r#"name="close_previous" value="yes""#)
            && ask_body.contains(r#"name="close_previous" value="no""#),
        "confirmation offers yes/no buttons, got {ask_body:?}"
    );

    // Declining ("no") tells the user the operation would fail.
    let declined = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Karla"),
            ("email", "karla@example.com"),
            ("start_date", "2026-08-01"),
            ("close_previous", "no"),
        ])
        .send()
        .await
        .expect("decline closing the previous ownership");
    assert_eq!(declined.status(), StatusCode::BAD_REQUEST);
    assert!(
        declined
            .text()
            .await
            .expect("decline body")
            .contains("kann das neue Eigentum nicht angelegt werden"),
        "declining is reported as failing"
    );
    // Nothing was created.
    assert_eq!(
        queries::list_ownerships(&h.pool, &apartment_id)
            .await
            .expect("list ownerships")
            .len(),
        3,
        "declining added no ownership (initial + Otto + Karl remain)"
    );

    // Confirming ("yes") closes Karl on 2026-07-31 and adds Karla.
    let confirmed = basic_auth(client.post(format!("{apt_url}/ownerships")))
        .form(&[
            ("name", "Karla"),
            ("email", "karla@example.com"),
            ("start_date", "2026-08-01"),
            ("close_previous", "yes"),
        ])
        .send()
        .await
        .expect("confirm closing the previous ownership");
    assert_eq!(confirmed.status(), StatusCode::SEE_OTHER);

    let owners = queries::list_ownerships(&h.pool, &apartment_id)
        .await
        .expect("list ownerships");
    let karl = owners.iter().find(|o| o.name == "Karl").expect("Karl");
    assert_eq!(
        karl.end_date.as_deref(),
        Some("2026-07-31"),
        "previous ownership ends the day before"
    );
    assert!(owners.iter().any(|o| o.name == "Karla"), "new owner added");

    // Updating an ownership into an overlapping period is still rejected.
    let karla_id = owners
        .iter()
        .find(|o| o.name == "Karla")
        .expect("Karla ownership")
        .id
        .clone();
    let update = basic_auth(client.post(format!("{apt_url}/ownerships/{karla_id}")))
        .form(&[
            ("name", "Karla"),
            ("email", "karla@example.com"),
            ("start_date", "2026-07-15"),
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

/// The danger zone exposes the building's rotation seed as a dropdown with
/// the current value selected; changing it affects only that building.
#[tokio::test]
async fn rotation_seed_can_be_changed_per_building() {
    let h = harness::start().await;
    let client = admin_client();

    let first = create_building(&h, &client, "Haus A").await;
    let second = create_building(&h, &client, "Haus B").await;

    // Two apartments, so the dropdown must offer exactly the phases 0..1.
    for name in ["EG links", "OG rechts"] {
        create_apartment(&h, &client, &first, name).await;
    }

    // The building page shows the danger zone with the dropdown, the current
    // value (0) selected, and a form posting to the rotation_seed endpoint.
    let page = basic_auth(client.get(format!("{}/admin/buildings/{first}", h.base_url)))
        .send()
        .await
        .expect("fetch building page");
    assert_eq!(page.status(), StatusCode::OK);
    let body = page.text().await.expect("building body");
    assert!(
        body.contains("Gefahrenzone"),
        "danger zone section, got {body:?}"
    );
    assert!(
        body.contains(r#"<option value="0" selected>0</option>"#),
        "current seed selected, got {body:?}"
    );
    assert!(
        body.contains(r#"<option value="1">1</option>"#),
        "dropdown offers every distinct phase 0..n-1, got {body:?}"
    );
    assert!(
        !body.contains(r#"<option value="2">"#),
        "shifting by the apartment count repeats phase 0 and must not be offered, got {body:?}"
    );
    assert!(
        body.contains("/rotation_seed"),
        "danger zone form targets the rotation_seed endpoint"
    );

    // Posting a new seed redirects back to the building page.
    let update = basic_auth(client.post(format!(
        "{}/admin/buildings/{first}/rotation_seed",
        h.base_url
    )))
    .form(&[("rotation_seed", "3")])
    .send()
    .await
    .expect("set rotation seed");
    assert_eq!(update.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        update
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(format!("/admin/buildings/{first}").as_str())
    );

    // The changed building now shows the new value as selected.
    let after = basic_auth(client.get(format!("{}/admin/buildings/{first}", h.base_url)))
        .send()
        .await
        .expect("fetch building page after change");
    assert!(
        after
            .text()
            .await
            .expect("after body")
            .contains(r#"<option value="3" selected>3</option>"#),
        "new seed selected on the building page"
    );

    // Other buildings keep their default seed: the change is scoped.
    let other = basic_auth(client.get(format!("{}/admin/buildings/{second}", h.base_url)))
        .send()
        .await
        .expect("fetch other building page");
    assert!(
        other
            .text()
            .await
            .expect("other body")
            .contains(r#"<option value="0" selected>0</option>"#),
        "other building must keep its default seed"
    );
}

/// Buildings can be edited: the edit page pre-fills the current values
/// (including the Ansprechpartner), the update redirects back to the building
/// page, and the new name/description and contact show up there. Leaving both
/// contact fields blank keeps the current Ansprechpartner.
#[tokio::test]
async fn building_can_be_edited() {
    let h = harness::start().await;
    let client = admin_client();

    let id = create_building(&h, &client, "Haus Sonnenschein").await;

    // The edit page pre-fills the current values, including the contact
    // (create_building above stores Alice/alice@example.com).
    let edit = basic_auth(client.get(format!("{}/admin/buildings/{id}/edit", h.base_url)))
        .send()
        .await
        .expect("fetch edit page");
    assert_eq!(edit.status(), StatusCode::OK);
    let body = edit.text().await.expect("edit body");
    assert!(body.contains("Gebäude bearbeiten"), "heading, got {body:?}");
    assert!(
        body.contains(r#"value="Haus Sonnenschein""#),
        "name pre-filled, got {body:?}"
    );
    assert!(
        body.contains(r#"value="A test building""#),
        "description pre-filled, got {body:?}"
    );
    assert!(
        body.contains(r#"value="Alice""#),
        "admin name pre-filled, got {body:?}"
    );
    assert!(
        body.contains(r#"value="alice@example.com""#),
        "admin email pre-filled, got {body:?}"
    );

    // Saving redirects back to the building page. The Ansprechpartner is
    // edited together with the building fields.
    let update = basic_auth(client.post(format!("{}/admin/buildings/{id}/update", h.base_url)))
        .form(&[
            ("name", "Haus Regenbogen"),
            ("description", "A nicer building"),
            ("admin_name", "Bob"),
            ("admin_email", "bob@example.com"),
        ])
        .send()
        .await
        .expect("update building");
    assert_eq!(update.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        update
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some(format!("/admin/buildings/{id}").as_str())
    );

    // The building page shows the edited values and the new contact.
    let page = basic_auth(client.get(format!("{}/admin/buildings/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page");
    let body = page.text().await.expect("building body");
    assert!(body.contains("Haus Regenbogen"), "new name, got {body:?}");
    assert!(
        body.contains("A nicer building"),
        "new description, got {body:?}"
    );
    assert!(
        body.contains(r#"<a href="mailto:bob@example.com">bob@example.com</a>"#),
        "new contact shown as mailto link, got {body:?}"
    );
    assert!(
        body.contains(">Bob</a>") && body.contains("/admin/people/"),
        "new contact links to the person page, got {body:?}"
    );

    // Leaving both contact fields blank keeps the current Ansprechpartner.
    let update = basic_auth(client.post(format!("{}/admin/buildings/{id}/update", h.base_url)))
        .form(&[
            ("name", "Haus Regenbogen"),
            ("description", "A nicer building"),
            ("admin_name", ""),
            ("admin_email", ""),
        ])
        .send()
        .await
        .expect("update building without contact");
    assert_eq!(update.status(), StatusCode::SEE_OTHER);
    let page = basic_auth(client.get(format!("{}/admin/buildings/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page after blank contact");
    let body = page
        .text()
        .await
        .expect("building body after blank contact");
    assert!(
        body.contains(">Bob</a> (<a href=\"mailto:bob@example.com\">"),
        "contact unchanged when fields are blank, got {body:?}"
    );
}

/// The Ansprechpartner is optional when creating a building: the form marks
/// it as such, and blank contact fields create a building without one.
#[tokio::test]
async fn building_can_be_created_without_ansprechpartner() {
    let h = harness::start().await;
    let client = admin_client();

    // The new-building form marks the Ansprechpartner as optional.
    let form = basic_auth(client.get(format!("{}/admin/buildings/new", h.base_url)))
        .send()
        .await
        .expect("fetch new-building page");
    let form_body = form.text().await.expect("new-building body");
    assert!(
        form_body.contains("Ansprechpartner") && form_body.contains("(optional)"),
        "contact marked optional on the create form, got {form_body:?}"
    );

    let resp = basic_auth(client.post(format!("{}/admin/buildings", h.base_url)))
        .form(&[
            ("name", "Haus Kontaktlos"),
            ("description", ""),
            ("admin_name", ""),
            ("admin_email", ""),
        ])
        .send()
        .await
        .expect("create building without contact");
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
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
    let id = location.trim_start_matches("/admin/buildings/").to_string();

    // The building page shows the building, but no contact line.
    let page = basic_auth(client.get(format!("{}/admin/buildings/{id}", h.base_url)))
        .send()
        .await
        .expect("fetch building page");
    let body = page.text().await.expect("building body");
    assert!(
        body.contains("Haus Kontaktlos"),
        "building shown, got {body:?}"
    );
    assert!(
        !body.contains("Ansprechpartner"),
        "no contact line without Ansprechpartner, got {body:?}"
    );
}
