use crate::db::models::{
    Apartment, Building, BuildingAdministrator, BuildingOwner, Ownership, Person, PersonRoleRow,
    Tenancy,
};
use crate::db::queries;
use crate::db::Db;
use crate::i18n::{self, Ui};
use crate::scheduler::{self, WeekAssignment};
use askama::Template;
use axum::extract::{Path, RawForm, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::response::IntoResponse as _;
use axum::response::Redirect;
use axum::Form;
use chrono::{Datelike, Local, NaiveDate};
use std::collections::HashMap;

/// Render an Askama template into an axum response.
/// (askama_axum was removed in askama 0.13; this is the replacement.)
fn render(t: impl Template) -> axum::response::Response {
    match t.render() {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(err) => {
            tracing::error!(%err, "template render failed");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Template render error",
            )
                .into_response()
        }
    }
}

/// Render a template as a 400 Bad Request; used to show a validation error
/// inline on the page whose form produced it.
fn render_bad_request(t: impl Template) -> axum::response::Response {
    (axum::http::StatusCode::BAD_REQUEST, render(t)).into_response()
}

/// Redirect after a POST. htmx requests (identified by the `HX-Request`
/// header) get an `HX-Redirect` response header so htmx performs a full-page
/// navigation instead of swapping the response body into the DOM; plain form
/// posts keep the classic 303 redirect.
fn redirect_after_post(headers: &HeaderMap, destination: &str) -> axum::response::Response {
    if headers.contains_key("hx-request") {
        (
            axum::http::StatusCode::OK,
            [(
                HeaderName::from_static("hx-redirect"),
                HeaderValue::from_str(destination)
                    .expect("redirect destination is a valid header value"),
            )],
        )
            .into_response()
    } else {
        Redirect::to(destination).into_response()
    }
}

#[derive(Template)]
#[template(path = "admin/buildings/index.html")]
pub struct BuildingsIndexTemplate {
    pub title: &'static str,
    pub buildings: Vec<Building>,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/buildings/new.html")]
pub struct BuildingsNewTemplate {
    pub title: &'static str,
    pub error: Option<String>,
    /// Submitted values, preserved when validation fails.
    pub name: String,
    pub description: String,
    pub admin_name: String,
    pub admin_email: String,
    /// The building's ownership structure, chosen visually on the form:
    /// "apartments" (each flat gets its own owner, WEG) or "building" (one
    /// person/company owns the whole building).
    pub ownership_style: String,
    /// Optional initial building owner, collected in the same form so a
    /// wholly-owned building needs no separate owner-creation step.
    pub owner_name: String,
    pub owner_email: String,
    pub owner_start_date: String,
    /// Known owners, offered as suggestions on the name/e-mail fields.
    pub people: Vec<Person>,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/buildings/edit.html")]
pub struct BuildingsEditTemplate {
    pub title: &'static str,
    pub building: Building,
    pub error: Option<String>,
    /// Submitted values, preserved when validation fails.
    pub name: String,
    pub description: String,
    pub admin_name: String,
    pub admin_email: String,
    /// Known people, offered as suggestions on the Ansprechpartner fields.
    pub people: Vec<Person>,
    pub ui: Ui,
}

/// One row of the Kehrwoche roster as the templates render it: dates in the
/// UI language's display format, plus a flag for the week that is currently
/// running.
pub struct ScheduleRow {
    pub iso_week: u32,
    pub start: String,
    pub end: String,
    pub assignee_name: Option<String>,
    pub delegated: bool,
    pub is_current: bool,
}

/// Map scheduler weeks to display rows, marking the week containing `today`.
fn schedule_rows(schedule: Vec<WeekAssignment>, today: NaiveDate, ui: Ui) -> Vec<ScheduleRow> {
    let fmt = i18n::date_format(ui.lang);
    schedule
        .into_iter()
        .map(|w| ScheduleRow {
            iso_week: w.iso_week,
            start: w.start.format(fmt).to_string(),
            end: w.end.format(fmt).to_string(),
            assignee_name: w.assignee_name,
            delegated: w.delegated,
            is_current: w.start <= today && today <= w.end,
        })
        .collect()
}

/// One row of the floor stack on the building page: the apartment itself, the
/// plate letter shown next to its name, and its position in the cleaning
/// rotation (creation order, the rotation's tie-breaks mirror the scheduler).
pub struct ApartmentRow {
    pub apartment: Apartment,
    pub plate: String,
    pub rotation: u32,
}

/// Map the buildings' apartments (display order) to rows with a rotation rank.
fn apartment_rows(apartments: Vec<Apartment>) -> Vec<ApartmentRow> {
    let mut rotation: Vec<&Apartment> = apartments.iter().collect();
    rotation.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let rank: HashMap<String, u32> = rotation
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id.clone(), (i + 1) as u32))
        .collect();
    apartments
        .into_iter()
        .map(|a| {
            let plate = a
                .name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            let rotation = rank.get(&a.id).copied().unwrap_or(0);
            ApartmentRow {
                apartment: a,
                plate,
                rotation,
            }
        })
        .collect()
}

#[derive(Template)]
#[template(path = "admin/buildings/show.html")]
pub struct BuildingsShowTemplate {
    pub title: String,
    pub building: Building,
    pub admins: Vec<BuildingAdministrator>,
    pub building_owners: Vec<BuildingOwner>,
    pub apartments: Vec<ApartmentRow>,
    pub year: i32,
    pub schedule: Vec<ScheduleRow>,
    pub rotation_options: Vec<(i64, bool)>,
    /// Whether any apartment has its own ownership record. Then the building
    /// is a WEG, and adding a building owner is not offered.
    pub has_apartment_owners: bool,
    pub error: Option<String>,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/buildings/schedule.html")]
pub struct BuildingsScheduleTemplate {
    pub title: String,
    pub building: Building,
    pub year: i32,
    pub schedule: Vec<ScheduleRow>,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/apartments/new.html")]
pub struct ApartmentsNewTemplate {
    pub title: &'static str,
    pub building: Building,
    pub error: Option<String>,
    pub name: String,
    pub description: String,
    /// Whether the building has a building owner; then the first-owner fields
    /// are optional and the apartment may instead rely on the building owner.
    pub has_building_owner: bool,
    /// Initial owner of the apartment, collected in the same form because an
    /// apartment must always have at least one ownership record (unless a
    /// building owner covers it).
    pub owner_name: String,
    pub owner_email: String,
    pub owner_start_date: String,
    pub owner_end_date: Option<String>,
    /// Known owners, offered as suggestions on the name/e-mail fields.
    pub people: Vec<Person>,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/apartments/edit.html")]
pub struct ApartmentsEditTemplate {
    pub title: String,
    pub building: Building,
    pub apartment: Apartment,
    pub error: Option<String>,
    pub name: String,
    pub description: String,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/apartments/show.html")]
pub struct ApartmentsShowTemplate {
    pub title: String,
    pub building: Building,
    pub apartment: Apartment,
    pub ownerships: Vec<Ownership>,
    pub tenancies: Vec<Tenancy>,
    pub error: Option<String>,
    /// Submitted values, preserved when an "Add Owner" submission fails.
    pub owner_form: Option<OwnershipForm>,
    /// Submitted values, preserved when an "Add Tenant" submission fails.
    pub tenant_form: Option<TenancyForm>,
    /// Shown above the owner form when the new ownership would start before
    /// the previous one ends: we ask whether the previous ownership shall end
    /// the day before the new one begins.
    pub owner_confirmation: Option<OwnershipConfirmation>,
    /// Shown above the tenant form when the new tenancy would start before
    /// the previous one ends: we ask whether the previous tenancy shall end
    /// the day before the new one begins.
    pub tenant_confirmation: Option<TenancyConfirmation>,
    /// Known owners, offered as suggestions on the owner name/e-mail fields.
    pub people: Vec<Person>,
    /// Whether the building as a whole currently has a building owner. Then
    /// the apartment cannot legally have its own owners (no WEG split), and
    /// the "Eigentümer hinzufügen" form is not offered.
    pub has_building_owner: bool,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/apartments/_table_body.html")]
pub struct ApartmentsTableBodyTemplate {
    pub building: Building,
    pub apartments: Vec<ApartmentRow>,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/ownerships/edit.html")]
pub struct OwnershipsEditTemplate {
    pub title: String,
    pub building: Building,
    pub apartment: Apartment,
    pub ownership: Ownership,
    pub error: Option<String>,
    /// Input values: the record's values, or the submitted ones after a failed update.
    pub form: OwnershipForm,
    /// Shown above the form when the submitted end date would drop today's
    /// coverage: we ask whether the end date shall really be set.
    pub end_warning: Option<EndDateWarning>,
    /// Known owners, offered as suggestions on the name/e-mail fields.
    pub people: Vec<Person>,
    pub ui: Ui,
}

#[derive(Template)]
#[template(path = "admin/tenancies/edit.html")]
pub struct TenanciesEditTemplate {
    pub title: String,
    pub building: Building,
    pub apartment: Apartment,
    pub tenancy: Tenancy,
    pub error: Option<String>,
    /// Input values: the record's values, or the submitted ones after a failed update.
    pub form: TenancyForm,
    /// Known people, offered as suggestions on the name/e-mail fields.
    pub people: Vec<Person>,
    pub ui: Ui,
}

// --- Buildings ---

#[derive(serde::Deserialize)]
pub struct CreateBuildingForm {
    pub name: String,
    pub description: String,
    pub admin_name: String,
    pub admin_email: String,
    /// Visual choice of the ownership structure: "apartments" (each flat
    /// gets its own owner, WEG) or "building" (one person/company owns the
    /// whole building). `#[serde(default)]` keeps older clients that omit
    /// the field working; missing means the apartments variant, i.e. no
    /// building owner.
    #[serde(default)]
    pub ownership_style: String,
    /// Owner fields of the "building" variant; when the style is
    /// "apartments", they are ignored. `#[serde(default)]` keeps older
    /// clients that omit the fields working.
    #[serde(default)]
    pub owner_name: String,
    #[serde(default)]
    pub owner_email: String,
    #[serde(default)]
    pub owner_start_date: String,
}

/// Form data of the "edit building" page. The Ansprechpartner fields are
/// optional: when both are left blank, the existing administrator record is
/// kept unchanged.
#[derive(serde::Deserialize)]
pub struct UpdateBuildingForm {
    pub name: String,
    pub description: String,
    pub admin_name: String,
    pub admin_email: String,
}

/// Extract a database-level error message so it can be shown inline on the
/// form that caused it. All validation rules live in the database (triggers
/// that `RAISE(ABORT, …)` with a stable `ERR_*` code, see 0012); the code is
/// translated into the UI language here. `None` means a transport/connection-
/// level failure, not a rejection.
fn db_message(err: &sqlx::Error, ui: Ui) -> Option<String> {
    err.as_database_error()
        .map(|e| i18n::db_msg(ui.lang, e.message()))
}

/// Load the known people (owner master data) for the suggestion lists, or
/// return a short error response.
async fn load_people(pool: &Db, ui: Ui) -> Result<Vec<Person>, (axum::http::StatusCode, String)> {
    queries::list_people(pool).await.map_err(|_| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.people_load").to_string(),
        )
    })
}

pub async fn buildings_index(State(pool): State<Db>, ui: Ui) -> impl axum::response::IntoResponse {
    match queries::list_buildings(&pool).await {
        Ok(buildings) => render(BuildingsIndexTemplate {
            title: i18n::msg(ui.lang, "buildings.heading"),
            buildings,
            ui,
        }),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.buildings_load"),
        )
            .into_response(),
    }
}

pub async fn buildings_new(State(pool): State<Db>, ui: Ui) -> impl axum::response::IntoResponse {
    let people = match load_people(&pool, ui).await {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    render(BuildingsNewTemplate {
        title: i18n::msg(ui.lang, "buildings.new_title"),
        error: None,
        name: String::new(),
        description: String::new(),
        admin_name: String::new(),
        admin_email: String::new(),
        ownership_style: "apartments".to_string(),
        owner_name: String::new(),
        owner_email: String::new(),
        owner_start_date: String::new(),
        people,
        ui,
    })
}

pub async fn buildings_create(
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<CreateBuildingForm>,
) -> impl axum::response::IntoResponse {
    // The ownership structure chosen on the form decides whether the owner
    // fields apply: "apartments" (the default, also for legacy clients)
    // creates a WEG-style building without an owner.
    let owner_input = if form.ownership_style == "building" {
        if form.owner_name.trim().is_empty()
            && form.owner_email.trim().is_empty()
            && form.owner_start_date.trim().is_empty()
        {
            let people = match load_people(&pool, ui).await {
                Ok(p) => p,
                Err(err) => return err.into_response(),
            };
            return render_bad_request(BuildingsNewTemplate {
                title: i18n::msg(ui.lang, "buildings.new_title"),
                error: Some(i18n::msg(ui.lang, "buildings.owner_required").to_string()),
                name: form.name,
                description: form.description,
                admin_name: form.admin_name,
                admin_email: form.admin_email,
                ownership_style: form.ownership_style,
                owner_name: form.owner_name,
                owner_email: form.owner_email,
                owner_start_date: form.owner_start_date,
                people,
                ui,
            });
        }
        Some(queries::NewOwner {
            name: &form.owner_name,
            email: &form.owner_email,
            start_date: &form.owner_start_date,
            end_date: None,
        })
    } else {
        None
    };
    let result = match owner_input {
        Some(owner) => {
            queries::create_building_with_owner(
                &pool,
                &form.name,
                &form.description,
                &form.admin_name,
                &form.admin_email,
                &owner,
            )
            .await
        }
        None => {
            queries::create_building(
                &pool,
                &form.name,
                &form.description,
                &form.admin_name,
                &form.admin_email,
            )
            .await
        }
    };
    match result {
        Ok(building) => Redirect::to(&format!("/admin/buildings/{}", building.id)).into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(BuildingsNewTemplate {
                    title: i18n::msg(ui.lang, "buildings.new_title"),
                    error: Some(msg),
                    name: form.name,
                    description: form.description,
                    admin_name: form.admin_name,
                    admin_email: form.admin_email,
                    ownership_style: form.ownership_style,
                    owner_name: form.owner_name,
                    owner_email: form.owner_email,
                    owner_start_date: form.owner_start_date,
                    people,
                    ui,
                })
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.building_create"),
            )
                .into_response(),
        },
    }
}

pub async fn buildings_edit(
    Path(id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    // get_building also returns the administrators, so the form can pre-fill
    // the Ansprechpartner fields with the current contact.
    let (building, admins) = match queries::get_building(&pool, &id).await {
        Ok(Some(found)) => found,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.building_load"),
            )
                .into_response()
        }
    };
    let people = match load_people(&pool, ui).await {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    let admin = admins.first();
    let name = building.name.clone();
    let description = building.description.clone();
    render(BuildingsEditTemplate {
        title: i18n::msg(ui.lang, "buildings.edit_title"),
        building,
        error: None,
        name,
        description,
        admin_name: admin.map_or_else(String::new, |a| a.name.clone()),
        admin_email: admin.map_or_else(String::new, |a| a.email.clone()),
        people,
        ui,
    })
}

pub async fn buildings_update(
    Path(id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<UpdateBuildingForm>,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };

    // The Ansprechpartner is optional on the edit form: when both fields are
    // blank, the existing administrator record stays untouched.
    let administrator = match (form.admin_name.as_str(), form.admin_email.as_str()) {
        (name, email) if name.trim().is_empty() && email.trim().is_empty() => None,
        (name, email) => Some((name, email)),
    };

    match queries::update_building(&pool, &id, &form.name, &form.description, administrator).await {
        Ok(_) => Redirect::to(&format!("/admin/buildings/{id}")).into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(BuildingsEditTemplate {
                    title: i18n::msg(ui.lang, "buildings.edit_title"),
                    building,
                    error: Some(msg),
                    name: form.name,
                    description: form.description,
                    admin_name: form.admin_name,
                    admin_email: form.admin_email,
                    people,
                    ui,
                })
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.building_save"),
            )
                .into_response(),
        },
    }
}

/// Render the full building page: header, upcoming schedule, apartments and
/// building owners. `error` is shown inline (used by failed building-owner
/// submissions); `buildings_show` passes `None`.
async fn render_building_page(
    pool: &Db,
    building_id: &str,
    error: Option<String>,
    ui: Ui,
) -> axum::response::Response {
    match queries::get_building(pool, building_id).await {
        Ok(Some((building, admins))) => {
            let apartments = match queries::list_apartments(pool, &building.id).await {
                Ok(apartments) => apartment_rows(apartments),
                Err(_) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        i18n::msg(ui.lang, "error.apartments_load"),
                    )
                        .into_response()
                }
            };
            let building_owners = match queries::list_building_owners(pool, &building.id).await {
                Ok(owners) => owners,
                Err(_) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        i18n::msg(ui.lang, "error.building_owners_load"),
                    )
                        .into_response()
                }
            };
            // Compact schedule: only the remaining weeks of the current year.
            let year = Local::now().date_naive().year();
            let today = Local::now().date_naive();
            let schedule: Vec<WeekAssignment> =
                match scheduler::schedule_for_year(&building.id, year, pool).await {
                    Ok(weeks) => weeks
                        .into_iter()
                        .filter(|w| w.end >= today)
                        .take(12)
                        .collect(),
                    Err(_) => {
                        return (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            i18n::msg(ui.lang, "error.schedule_compute"),
                        )
                            .into_response()
                    }
                };
            // The rotation phase repeats with the apartment count, so the
            // dropdown offers exactly the distinct phases 0..n-1; the current
            // value is always included even when it lies outside that range.
            let rotation_options = rotation_seed_options(building.rotation_seed, apartments.len());
            let has_apartment_owners =
                match queries::building_has_apartment_owners(pool, &building.id).await {
                    Ok(has) => has,
                    Err(_) => {
                        return (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            i18n::msg(ui.lang, "error.ownership_structure_load"),
                        )
                            .into_response()
                    }
                };
            render(BuildingsShowTemplate {
                title: building.name.clone(),
                building,
                admins,
                building_owners,
                apartments,
                year,
                schedule: schedule_rows(schedule, today, ui),
                rotation_options,
                has_apartment_owners,
                error,
                ui,
            })
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            i18n::msg(ui.lang, "common.not_found"),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.building_load"),
        )
            .into_response(),
    }
}

pub async fn buildings_show(
    Path(id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    render_building_page(&pool, &id, None, ui).await
}

pub async fn buildings_schedule(
    Path(id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    match queries::get_building(&pool, &id).await {
        Ok(Some((building, _admins))) => {
            let year = Local::now().date_naive().year();
            let today = Local::now().date_naive();
            match scheduler::schedule_for_year(&building.id, year, &pool).await {
                Ok(schedule) => render(BuildingsScheduleTemplate {
                    title: i18n::msg(ui.lang, "schedule.title").to_string(),
                    building,
                    year,
                    schedule: schedule_rows(schedule, today, ui),
                    ui,
                }),
                Err(_) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    i18n::msg(ui.lang, "error.schedule_compute"),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            i18n::msg(ui.lang, "common.not_found"),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.building_load"),
        )
            .into_response(),
    }
}

pub async fn buildings_delete(
    Path(id): Path<String>,
    State(pool): State<Db>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let _ = queries::delete_building(&pool, &id).await;
    redirect_after_post(&headers, "/admin")
}

/// Form data of the danger zone rotation-offset dropdown.
#[derive(serde::Deserialize)]
pub struct RotationSeedForm {
    pub rotation_seed: i64,
}

fn rotation_seed_options(current: i64, apartment_count: usize) -> Vec<(i64, bool)> {
    // The rotation phase repeats with the number of apartments (shifting by
    // the count equals shifting by 0), so the canonical options are exactly
    // 0..n-1; anything beyond would only repeat an equivalent phase. The
    // current value is always included so the dropdown stays in sync even if
    // the stored seed lies outside that range.
    let mut options: Vec<(i64, bool)> = (0..apartment_count as i64)
        .map(|v| (v, v == current))
        .collect();
    if !options.iter().any(|(v, _)| *v == current) {
        options.push((current, true));
    }
    options
}

/// Change the rotation offset of one building. The building is addressed in
/// the path (scoped, never global) and must exist.
pub async fn buildings_rotation_seed_update(
    Path(id): Path<String>,
    State(pool): State<Db>,
    headers: HeaderMap,
    ui: Ui,
    Form(form): Form<RotationSeedForm>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_building(&pool, &id, ui).await {
        return err.into_response();
    }
    match queries::update_building_rotation_seed(&pool, &id, form.rotation_seed).await {
        Ok(_) => redirect_after_post(&headers, &format!("/admin/buildings/{id}")),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.rotation_save"),
        )
            .into_response(),
    }
}

// --- Apartments ---

/// Form data of the "new apartment" page: besides name and description it
/// collects the initial owner, because the database requires an ownership
/// record to exist from the moment the apartment is created. The owner
/// fields are not rendered when the building has a building owner (the
/// apartment then belongs to the building as a whole, and the database
/// rejects such an ownership anyway, see 0010); `#[serde(default)]` keeps
/// clients without the hidden fields working.
#[derive(serde::Deserialize)]
pub struct CreateApartmentForm {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub owner_name: String,
    #[serde(default)]
    pub owner_email: String,
    #[serde(default)]
    pub owner_start_date: String,
    #[serde(default)]
    pub owner_end_date: Option<String>,
}

/// Form data of the "edit apartment" page. Only name and description are
/// edited there; owners are managed separately on the apartment page, so this
/// form deliberately has no owner fields.
#[derive(serde::Deserialize)]
pub struct UpdateApartmentForm {
    pub name: String,
    pub description: String,
}

/// Load building or return a short error; used by apartment subroutes.
async fn load_building(
    pool: &Db,
    building_id: &str,
    ui: Ui,
) -> Result<Building, (axum::http::StatusCode, String)> {
    match queries::get_building(pool, building_id).await {
        Ok(Some((building, _))) => Ok(building),
        Ok(None) => Err((
            axum::http::StatusCode::NOT_FOUND,
            i18n::msg(ui.lang, "common.not_found").to_string(),
        )),
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.building_load").to_string(),
        )),
    }
}

/// Load apartment or return a short error.
async fn load_apartment_owned_by(
    pool: &Db,
    building_id: &str,
    apartment_id: &str,
    ui: Ui,
) -> Result<Apartment, (axum::http::StatusCode, String)> {
    match queries::get_apartment(pool, apartment_id).await {
        Ok(Some(apartment)) => {
            if apartment.building_id != building_id {
                return Err((
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found").to_string(),
                ));
            }
            Ok(apartment)
        }
        Ok(None) => Err((
            axum::http::StatusCode::NOT_FOUND,
            i18n::msg(ui.lang, "common.not_found").to_string(),
        )),
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.apartment_load").to_string(),
        )),
    }
}

/// Everything the apartment show page needs, loaded in one pass.
struct ApartmentShowParts {
    building: Building,
    apartment: Apartment,
    ownerships: Vec<Ownership>,
    tenancies: Vec<Tenancy>,
    people: Vec<Person>,
    has_building_owner: bool,
}

async fn load_apartment_show(
    pool: &Db,
    building_id: &str,
    apartment_id: &str,
    ui: Ui,
) -> Result<ApartmentShowParts, (axum::http::StatusCode, String)> {
    let building = load_building(pool, building_id, ui).await?;
    let apartment = load_apartment_owned_by(pool, building_id, apartment_id, ui).await?;
    let (ownerships, tenancies) = match (
        queries::list_ownerships(pool, apartment_id).await,
        queries::list_tenancies(pool, apartment_id).await,
    ) {
        (Ok(ownerships), Ok(tenancies)) => (ownerships, tenancies),
        _ => {
            return Err((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.apartment_data_load").to_string(),
            ))
        }
    };
    let people = load_people(pool, ui).await?;
    let has_building_owner = queries::get_current_building_owner(pool, building_id)
        .await
        .map(|owner| owner.is_some())
        .unwrap_or(false);
    Ok(ApartmentShowParts {
        building,
        apartment,
        ownerships,
        tenancies,
        people,
        has_building_owner,
    })
}

/// Re-render the apartment page with an inline error, preserving the submitted
/// form values. Used when an "Add Owner"/"Add Tenant" submission fails.
async fn render_apartment_show_error(
    pool: &Db,
    building_id: &str,
    apartment_id: &str,
    error: String,
    owner_form: Option<OwnershipForm>,
    tenant_form: Option<TenancyForm>,
    ui: Ui,
) -> axum::response::Response {
    match load_apartment_show(pool, building_id, apartment_id, ui).await {
        Ok(parts) => render_bad_request(ApartmentsShowTemplate {
            title: parts.apartment.name.clone(),
            building: parts.building,
            apartment: parts.apartment,
            ownerships: parts.ownerships,
            tenancies: parts.tenancies,
            error: Some(error),
            owner_form,
            tenant_form,
            owner_confirmation: None,
            tenant_confirmation: None,
            people: parts.people,
            has_building_owner: parts.has_building_owner,
            ui,
        }),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// Re-render the apartment page asking whether the previous ownership/tenancy
/// shall end on the day before the new one begins. The submitted values stay
/// in `owner_form`/`tenant_form`; exactly one of the two confirmations is set.
#[allow(clippy::too_many_arguments)]
async fn render_apartment_show_confirmation(
    pool: &Db,
    building_id: &str,
    apartment_id: &str,
    owner_confirmation: Option<OwnershipConfirmation>,
    tenant_confirmation: Option<TenancyConfirmation>,
    owner_form: Option<OwnershipForm>,
    tenant_form: Option<TenancyForm>,
    ui: Ui,
) -> axum::response::Response {
    match load_apartment_show(pool, building_id, apartment_id, ui).await {
        Ok(parts) => render(ApartmentsShowTemplate {
            title: parts.apartment.name.clone(),
            building: parts.building,
            apartment: parts.apartment,
            ownerships: parts.ownerships,
            tenancies: parts.tenancies,
            error: None,
            owner_form,
            tenant_form,
            owner_confirmation,
            tenant_confirmation,
            people: parts.people,
            has_building_owner: parts.has_building_owner,
            ui,
        }),
        Err((status, msg)) => (status, msg).into_response(),
    }
}

/// The apartments list lives on the building page now; keep old bookmarks working.
pub async fn apartments_index_redirect(
    Path(building_id): Path<String>,
) -> impl axum::response::IntoResponse {
    Redirect::permanent(&format!("/admin/buildings/{building_id}")).into_response()
}

pub async fn apartments_new(
    Path(building_id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let people = match load_people(&pool, ui).await {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    let has_building_owner = queries::get_current_building_owner(&pool, &building_id)
        .await
        .map(|owner| owner.is_some())
        .unwrap_or(false);
    render(ApartmentsNewTemplate {
        title: i18n::msg(ui.lang, "apartments.new"),
        building,
        error: None,
        name: String::new(),
        description: String::new(),
        has_building_owner,
        owner_name: String::new(),
        owner_email: String::new(),
        owner_start_date: String::new(),
        owner_end_date: None,
        people,
        ui,
    })
}

pub async fn apartments_create(
    Path(building_id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<CreateApartmentForm>,
) -> impl axum::response::IntoResponse {
    // When the building has a building owner, the owner fields are optional:
    // an empty owner form creates an apartment that belongs to the building
    // owner. Otherwise the first owner is required (the database rejects an
    // apartment without any ownership).
    let has_building_owner = queries::get_current_building_owner(&pool, &building_id)
        .await
        .map(|owner| owner.is_some())
        .unwrap_or(false);
    let end_opt = normalize_end_date(form.owner_end_date.as_deref());
    let owner_input = if has_building_owner
        && form.owner_name.trim().is_empty()
        && form.owner_email.trim().is_empty()
        && form.owner_start_date.trim().is_empty()
    {
        None
    } else {
        Some(queries::NewOwner {
            name: &form.owner_name,
            email: &form.owner_email,
            start_date: &form.owner_start_date,
            end_date: end_opt,
        })
    };
    let result = match owner_input {
        Some(owner) => {
            queries::create_apartment(&pool, &building_id, &form.name, &form.description, &owner)
                .await
        }
        None => {
            queries::create_apartment_building_owned(
                &pool,
                &building_id,
                &form.name,
                &form.description,
            )
            .await
        }
    };
    match result {
        Ok(apartment) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{}",
            apartment.id
        ))
        .into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                let building = match load_building(&pool, &building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(ApartmentsNewTemplate {
                    title: i18n::msg(ui.lang, "apartments.new"),
                    building,
                    error: Some(msg),
                    name: form.name,
                    description: form.description,
                    has_building_owner,
                    owner_name: form.owner_name,
                    owner_email: form.owner_email,
                    owner_start_date: form.owner_start_date,
                    owner_end_date: form.owner_end_date,
                    people,
                    ui,
                })
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.apartment_create"),
            )
                .into_response(),
        },
    }
}

pub async fn apartments_edit(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    let name = apartment.name.clone();
    let description = apartment.description.clone();
    render(ApartmentsEditTemplate {
        title: i18n::msg(ui.lang, "apartments.edit_title").to_string(),
        building,
        apartment,
        error: None,
        name,
        description,
        ui,
    })
}

pub async fn apartments_show(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };

    match (
        queries::list_ownerships(&pool, &apartment_id).await,
        queries::list_tenancies(&pool, &apartment_id).await,
    ) {
        (Ok(ownerships), Ok(tenancies)) => {
            let people = match load_people(&pool, ui).await {
                Ok(p) => p,
                Err(err) => return err.into_response(),
            };
            let has_building_owner = queries::get_current_building_owner(&pool, &building_id)
                .await
                .map(|owner| owner.is_some())
                .unwrap_or(false);
            render(ApartmentsShowTemplate {
                title: apartment.name.clone(),
                building,
                apartment,
                ownerships,
                tenancies,
                error: None,
                owner_form: None,
                tenant_form: None,
                owner_confirmation: None,
                tenant_confirmation: None,
                people,
                has_building_owner,
                ui,
            })
        }
        _ => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.apartment_data_load"),
        )
            .into_response(),
    }
}

pub async fn apartments_update(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<UpdateApartmentForm>,
) -> impl axum::response::IntoResponse {
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    match queries::update_apartment(&pool, &apartment_id, &form.name, &form.description).await {
        Ok(_) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        ))
        .into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                let building = match load_building(&pool, &building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(ApartmentsEditTemplate {
                    title: i18n::msg(ui.lang, "apartments.edit_title").to_string(),
                    building,
                    apartment,
                    error: Some(msg),
                    name: form.name,
                    description: form.description,
                    ui,
                })
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.apartment_save"),
            )
                .into_response(),
        },
    }
}

pub async fn apartments_delete(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    headers: HeaderMap,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        return err.into_response();
    }
    let _ = queries::delete_apartment(&pool, &apartment_id).await;
    redirect_after_post(&headers, &format!("/admin/buildings/{building_id}"))
}

/// Persist a manually chosen apartment order. Receives the apartment ids as
/// repeated `item` form fields (in their new DOM order, sent by htmx when the
/// drag-and-drop ends) and responds with the re-rendered table body so htmx
/// can swap in the new order.
///
/// The body is parsed from the raw form pairs: axum's `Form` extractor uses
/// `serde_urlencoded`, which cannot deserialize repeated form fields into a
/// `Vec` and rejects the request with `invalid type: string "…", expected a
/// sequence`.
pub async fn apartments_reorder(
    Path(building_id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
    RawForm(body): RawForm,
) -> impl axum::response::IntoResponse {
    // Apartment ids in their new DOM order, one `item` field per row.
    let items: Vec<String> = form_urlencoded::parse(&body)
        .filter(|(key, _)| key == "item")
        .map(|(_, value)| value.into_owned())
        .collect();

    // The submitted ids must be exactly this building's apartments — no
    // additions, omissions, or duplicates. The db layer validates this inside
    // the same transaction that persists the new order.
    match queries::reorder_apartments(&pool, &building_id, &items).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                i18n::msg(ui.lang, "error.apartment_order"),
            )
                .into_response()
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.order_save"),
            )
                .into_response()
        }
    }

    // Re-render just the table body so htmx can swap in the new order.
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartments = match queries::list_apartments(&pool, &building_id).await {
        Ok(list) => apartment_rows(list),
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.apartments_reload"),
            )
                .into_response()
        }
    };
    render(ApartmentsTableBodyTemplate {
        building,
        apartments,
        ui,
    })
}

// --- Ownerships ---

/// The question asked when adding an owner would start before the previous
/// ownership ends: whether the previous ownership shall end on the day before
/// the new one begins. The submitted values stay in `owner_form`.
pub struct OwnershipConfirmation {
    pub previous_name: String,
    /// The day before the new ownership's start date.
    pub previous_end: String,
    /// The new ownership's start date.
    pub new_start: String,
}

/// The tenant analogue of [`OwnershipConfirmation`].
pub struct TenancyConfirmation {
    pub previous_name: String,
    /// The day before the new tenancy's start date.
    pub previous_end: String,
    /// The new tenancy's start date.
    pub new_start: String,
}

/// Shown when an edit would set the currently-covering period's end date
/// before today, leaving the apartment or building without a covering owner
/// until a new one is recorded.
pub struct EndDateWarning {
    /// The proposed end date (before today).
    pub end_date: String,
    /// The day after the proposed end date; the coverage ends then.
    pub uncovered_from: String,
}

/// The day before the given canonical YYYY-MM-DD date, if it parses.
fn previous_day(date: &str) -> Option<String> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok().map(|d| {
        (d - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// The day after the given canonical YYYY-MM-DD date, if it parses.
fn next_day(date: &str) -> Option<String> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok().map(|d| {
        (d + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// The warning shown when an edit would set a currently-covering period's end
/// date before today.
fn end_date_warning(end_date: &str) -> EndDateWarning {
    EndDateWarning {
        end_date: end_date.to_string(),
        uncovered_from: next_day(end_date).unwrap_or_else(|| end_date.to_string()),
    }
}

/// Whether setting the period's end date to `new_end` drops its coverage of
/// `today`: the period currently covers today and would end before it. Used to
/// warn before applying such an edit (the periods tile the timeline, so no
/// other period can take over).
fn end_drops_today_coverage(
    existing_start: &str,
    existing_end: Option<&str>,
    new_end: Option<NaiveDate>,
    today: NaiveDate,
) -> bool {
    let covers_today = NaiveDate::parse_from_str(existing_start, "%Y-%m-%d")
        .map(|s| s <= today)
        .unwrap_or(false)
        && existing_end
            .and_then(|e| NaiveDate::parse_from_str(e, "%Y-%m-%d").ok())
            .is_none_or(|e| e >= today);
    covers_today && new_end.is_some_and(|d| d < today)
}

#[derive(serde::Deserialize)]
pub struct OwnershipForm {
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    /// "yes"/"no" answer to the question whether the previous ownership
    /// shall end on the day before the new one begins. Absent on the first
    /// submission; only present when the confirmation form was shown.
    #[serde(default)]
    pub close_previous: Option<String>,
    /// "yes"/"no" answer to the question whether the end date shall really be
    /// set before today (the apartment would lose its covering owner). Absent
    /// on the first submission; only present when the warning was shown.
    #[serde(default)]
    pub confirm_end: Option<String>,
}

fn normalize_end_date(end_date: Option<&str>) -> Option<&str> {
    end_date.map(str::trim).filter(|s| !s.is_empty())
}

/// Turn the outcome of creating an ownership into a response: redirect on
/// success, re-render with the database's rejection message on failure.
async fn ownership_created(
    pool: &Db,
    building_id: &str,
    apartment_id: &str,
    form: OwnershipForm,
    result: Result<Ownership, sqlx::Error>,
    ui: Ui,
) -> axum::response::Response {
    match result {
        Ok(_) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        ))
        .into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                render_apartment_show_error(
                    pool,
                    building_id,
                    apartment_id,
                    msg,
                    Some(form),
                    None,
                    ui,
                )
                .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.ownership_create"),
            )
                .into_response(),
        },
    }
}

pub async fn ownerships_create(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<OwnershipForm>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        return err.into_response();
    }
    let end_opt = normalize_end_date(form.end_date.as_deref());

    // If the new ownership would start while the previous one is still current
    // (it does not already end on the day before), the database rejects the
    // insert. Ask the user whether the previous ownership shall end on the day
    // before the new one begins; the answer comes back as `close_previous`.
    if let Some(prev) = queries::get_ownership_on(&pool, &apartment_id, &form.start_date)
        .await
        .ok()
        .flatten()
    {
        let prev_end = match previous_day(&form.start_date) {
            Some(d) => d,
            None => {
                // Unparseable start date: the database reports it.
                let result = queries::create_ownership(
                    &pool,
                    &apartment_id,
                    &form.name,
                    &form.email,
                    &form.start_date,
                    end_opt,
                )
                .await;
                return ownership_created(&pool, &building_id, &apartment_id, form, result, ui)
                    .await;
            }
        };
        match form.close_previous.as_deref() {
            None => {
                return render_apartment_show_confirmation(
                    &pool,
                    &building_id,
                    &apartment_id,
                    Some(OwnershipConfirmation {
                        previous_name: prev.name,
                        previous_end: prev_end,
                        new_start: form.start_date.clone(),
                    }),
                    None,
                    Some(form),
                    None,
                    ui,
                )
                .await
            }
            Some("yes") => {
                let result = queries::create_ownership_closing_previous(
                    &pool,
                    &apartment_id,
                    queries::PreviousPeriod {
                        id: &prev.id,
                        end_date: &prev_end,
                    },
                    &form.name,
                    &form.email,
                    &form.start_date,
                    end_opt,
                )
                .await;
                return ownership_created(&pool, &building_id, &apartment_id, form, result, ui)
                    .await;
            }
            _ => {
                // The user declined to end the previous ownership the day
                // before; the operation cannot succeed, so tell them.
                return render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    i18n::msgf(
                        ui.lang,
                        "ownership.decline_close",
                        &[prev.name.as_str(), prev_end.as_str()],
                    ),
                    Some(form),
                    None,
                    ui,
                )
                .await;
            }
        }
    }

    // The start is neither covered nor tiled: if an earlier period already
    // ended, the new one must begin on the day after it. Check here so the
    // rejection can name the actual dates (SQLite trigger messages cannot
    // interpolate values); the database still enforces the rule.
    if let Some(prev_end) =
        queries::get_ownership_end_before(&pool, &apartment_id, &form.start_date)
            .await
            .ok()
            .flatten()
    {
        if let Some(next) = next_day(&prev_end) {
            if next != form.start_date {
                return render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    i18n::msgf(
                        ui.lang,
                        "ownership.gap_dates",
                        &[prev_end.as_str(), next.as_str(), form.start_date.as_str()],
                    ),
                    Some(form),
                    None,
                    ui,
                )
                .await;
            }
        }
    }

    // No previous ownership covering the new start (the chain is already
    // tiled): create directly; the database enforces the remaining rules.
    let result = queries::create_ownership(
        &pool,
        &apartment_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await;
    ownership_created(&pool, &building_id, &apartment_id, form, result, ui).await
}

pub async fn ownerships_edit(
    Path((building_id, apartment_id, ownership_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    match queries::get_ownership(&pool, &ownership_id).await {
        Ok(Some(ownership)) => {
            if ownership.apartment_id != apartment_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
            let people = match load_people(&pool, ui).await {
                Ok(p) => p,
                Err(err) => return err.into_response(),
            };
            let form = OwnershipForm {
                name: ownership.name.clone(),
                email: ownership.email.clone(),
                start_date: ownership.start_date.clone(),
                end_date: ownership.end_date.clone(),
                close_previous: None,
                confirm_end: None,
            };
            render(OwnershipsEditTemplate {
                title: i18n::msg(ui.lang, "ownership.edit_title").to_string(),
                building,
                apartment,
                ownership,
                error: None,
                form,
                end_warning: None,
                people,
                ui,
            })
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            i18n::msg(ui.lang, "common.not_found"),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.ownership_load"),
        )
            .into_response(),
    }
}

/// Re-render the ownership edit page with an inline error, preserving the
/// submitted form values.
async fn render_ownership_edit_error(
    pool: &Db,
    building_id: &str,
    apartment: &Apartment,
    ownership: &Ownership,
    error: String,
    form: OwnershipForm,
    ui: Ui,
) -> axum::response::Response {
    let building = match load_building(pool, building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let people = match load_people(pool, ui).await {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    render_bad_request(OwnershipsEditTemplate {
        title: i18n::msg(ui.lang, "ownership.edit_title").to_string(),
        building,
        apartment: apartment.clone(),
        ownership: ownership.clone(),
        error: Some(error),
        form,
        end_warning: None,
        people,
        ui,
    })
}

/// The warning if setting `new_end` on `ownership` would drop the apartment's
/// coverage of today (unless a covering building owner keeps the apartment
/// covered — the legacy 0010 cleanup case).
async fn ownership_end_date_warning(
    pool: &Db,
    apartment: &Apartment,
    ownership: &Ownership,
    new_end: Option<&str>,
) -> Option<EndDateWarning> {
    let new_end = new_end?;
    let new_end_date = NaiveDate::parse_from_str(new_end, "%Y-%m-%d").ok()?;
    if !end_drops_today_coverage(
        &ownership.start_date,
        ownership.end_date.as_deref(),
        Some(new_end_date),
        Local::now().date_naive(),
    ) {
        return None;
    }
    if queries::get_current_building_owner(pool, &apartment.building_id)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return None;
    }
    Some(end_date_warning(new_end))
}

/// Re-render the ownership edit page asking whether the end date shall really
/// be set before today, preserving the submitted values.
async fn render_ownership_edit_warning(
    pool: &Db,
    building_id: &str,
    apartment: &Apartment,
    ownership: &Ownership,
    form: OwnershipForm,
    end_warning: EndDateWarning,
    ui: Ui,
) -> axum::response::Response {
    let building = match load_building(pool, building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let people = match load_people(pool, ui).await {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    render(OwnershipsEditTemplate {
        title: i18n::msg(ui.lang, "ownership.edit_title").to_string(),
        building,
        apartment: apartment.clone(),
        ownership: ownership.clone(),
        error: None,
        form,
        end_warning: Some(end_warning),
        people,
        ui,
    })
}

pub async fn ownerships_update(
    Path((building_id, apartment_id, ownership_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<OwnershipForm>,
) -> impl axum::response::IntoResponse {
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    // Ensure the ownership belongs to this apartment
    let ownership = match queries::get_ownership(&pool, &ownership_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
            existing
        }
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
    };

    let end_opt = normalize_end_date(form.end_date.as_deref());

    // Setting the end date of the currently-covering period before today would
    // leave the apartment without an owner; ask for confirmation first.
    if let Some(warning) = ownership_end_date_warning(&pool, &apartment, &ownership, end_opt).await
    {
        match form.confirm_end.as_deref() {
            None => {
                return render_ownership_edit_warning(
                    &pool,
                    &building_id,
                    &apartment,
                    &ownership,
                    form,
                    warning,
                    ui,
                )
                .await;
            }
            Some("no") => {
                return Redirect::to(&format!(
                    "/admin/buildings/{building_id}/apartments/{apartment_id}/ownerships/{ownership_id}/edit"
                ))
                .into_response();
            }
            _ => {} // "yes": apply the update below.
        }
    }

    // Field checks and chain tiling are enforced by the database (triggers);
    // its rejection message is shown inline.
    match queries::update_ownership(
        &pool,
        &ownership_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await
    {
        Ok(_) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        ))
        .into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                render_ownership_edit_error(
                    &pool,
                    &building_id,
                    &apartment,
                    &ownership,
                    msg,
                    form,
                    ui,
                )
                .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.ownership_save"),
            )
                .into_response(),
        },
    }
}

pub async fn ownerships_delete(
    Path((building_id, apartment_id, ownership_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    headers: HeaderMap,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        return err.into_response();
    }
    match queries::get_ownership(&pool, &ownership_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
        }
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
    }

    // The database rejects deleting the apartment's last ownership or a period
    // in the middle of the ownership chain (see `ownerships_guard_delete`).
    match queries::delete_ownership(&pool, &ownership_id).await {
        Ok(_) => redirect_after_post(
            &headers,
            &format!("/admin/buildings/{building_id}/apartments/{apartment_id}"),
        ),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                render_apartment_show_error(&pool, &building_id, &apartment_id, msg, None, None, ui)
                    .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.ownership_delete"),
            )
                .into_response(),
        },
    }
}

// --- Tenancies ---

#[derive(serde::Deserialize)]
pub struct TenancyForm {
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    /// "yes"/"no" answer to the question whether the previous tenancy shall
    /// end on the day before the new one begins. Absent on the first
    /// submission; only present when the confirmation form was shown.
    #[serde(default)]
    pub close_previous: Option<String>,
}

/// Turn the outcome of creating a tenancy into a response: redirect on
/// success, re-render with the database's rejection message on failure.
async fn tenancy_created(
    pool: &Db,
    building_id: &str,
    apartment_id: &str,
    form: TenancyForm,
    result: Result<Tenancy, sqlx::Error>,
    ui: Ui,
) -> axum::response::Response {
    match result {
        Ok(_) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        ))
        .into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                render_apartment_show_error(
                    pool,
                    building_id,
                    apartment_id,
                    msg,
                    None,
                    Some(form),
                    ui,
                )
                .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.tenancy_create"),
            )
                .into_response(),
        },
    }
}

pub async fn tenancies_create(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<TenancyForm>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        return err.into_response();
    }
    let end_opt = normalize_end_date(form.end_date.as_deref());

    // If the new tenancy would start while the previous one is still current
    // (it does not already end on the day before), the database rejects the
    // insert. Ask the user whether the previous tenancy shall end on the day
    // before the new one begins; the answer comes back as `close_previous`.
    if let Some(prev) = queries::get_tenancy_on(&pool, &apartment_id, &form.start_date)
        .await
        .ok()
        .flatten()
    {
        let prev_end = match previous_day(&form.start_date) {
            Some(d) => d,
            None => {
                // Unparseable start date: the database reports it.
                let result = queries::create_tenancy(
                    &pool,
                    &apartment_id,
                    &form.name,
                    &form.email,
                    &form.start_date,
                    end_opt,
                )
                .await;
                return tenancy_created(&pool, &building_id, &apartment_id, form, result, ui).await;
            }
        };
        match form.close_previous.as_deref() {
            None => {
                return render_apartment_show_confirmation(
                    &pool,
                    &building_id,
                    &apartment_id,
                    None,
                    Some(TenancyConfirmation {
                        previous_name: prev.name,
                        previous_end: prev_end,
                        new_start: form.start_date.clone(),
                    }),
                    None,
                    Some(form),
                    ui,
                )
                .await
            }
            Some("yes") => {
                let result = queries::create_tenancy_closing_previous(
                    &pool,
                    &apartment_id,
                    queries::PreviousPeriod {
                        id: &prev.id,
                        end_date: &prev_end,
                    },
                    &form.name,
                    &form.email,
                    &form.start_date,
                    end_opt,
                )
                .await;
                return tenancy_created(&pool, &building_id, &apartment_id, form, result, ui).await;
            }
            _ => {
                // The user declined to end the previous tenancy the day
                // before; the operation cannot succeed, so tell them.
                return render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    i18n::msgf(
                        ui.lang,
                        "tenancy.decline_close",
                        &[prev.name.as_str(), prev_end.as_str()],
                    ),
                    None,
                    Some(form),
                    ui,
                )
                .await;
            }
        }
    }

    // No previous tenancy covering the new start: create directly; the
    // database enforces the remaining rules.
    let result = queries::create_tenancy(
        &pool,
        &apartment_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await;
    tenancy_created(&pool, &building_id, &apartment_id, form, result, ui).await
}

pub async fn tenancies_edit(
    Path((building_id, apartment_id, tenancy_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    match queries::get_tenancy(&pool, &tenancy_id).await {
        Ok(Some(tenancy)) => {
            if tenancy.apartment_id != apartment_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
            let people = match load_people(&pool, ui).await {
                Ok(p) => p,
                Err(err) => return err.into_response(),
            };
            let form = TenancyForm {
                name: tenancy.name.clone(),
                email: tenancy.email.clone(),
                start_date: tenancy.start_date.clone(),
                end_date: tenancy.end_date.clone(),
                close_previous: None,
            };
            render(TenanciesEditTemplate {
                title: i18n::msg(ui.lang, "tenancy.edit_title").to_string(),
                building,
                apartment,
                tenancy,
                error: None,
                form,
                people,
                ui,
            })
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            i18n::msg(ui.lang, "common.not_found"),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.tenancy_load"),
        )
            .into_response(),
    }
}

/// Re-render the tenancy edit page with an inline error, preserving the
/// submitted form values.
async fn render_tenancy_edit_error(
    pool: &Db,
    building_id: &str,
    apartment: &Apartment,
    tenancy: &Tenancy,
    error: String,
    form: TenancyForm,
    ui: Ui,
) -> axum::response::Response {
    let building = match load_building(pool, building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let people = match load_people(pool, ui).await {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    render_bad_request(TenanciesEditTemplate {
        title: i18n::msg(ui.lang, "tenancy.edit_title").to_string(),
        building,
        apartment: apartment.clone(),
        tenancy: tenancy.clone(),
        error: Some(error),
        form,
        people,
        ui,
    })
}

pub async fn tenancies_update(
    Path((building_id, apartment_id, tenancy_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<TenancyForm>,
) -> impl axum::response::IntoResponse {
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    // Ensure the tenancy belongs to this apartment
    let tenancy = match queries::get_tenancy(&pool, &tenancy_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
            existing
        }
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
    };

    let end_opt = normalize_end_date(form.end_date.as_deref());

    // Field checks and the "at most one active tenancy" rule are enforced by
    // the database (triggers); its rejection message is shown inline.
    match queries::update_tenancy(
        &pool,
        &tenancy_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await
    {
        Ok(_) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        ))
        .into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                render_tenancy_edit_error(&pool, &building_id, &apartment, &tenancy, msg, form, ui)
                    .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.tenancy_save"),
            )
                .into_response(),
        },
    }
}

pub async fn tenancies_delete(
    Path((building_id, apartment_id, tenancy_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    headers: HeaderMap,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id, ui).await {
        return err.into_response();
    }
    match queries::get_tenancy(&pool, &tenancy_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
        }
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
    }

    let _ = queries::delete_tenancy(&pool, &tenancy_id).await;
    redirect_after_post(
        &headers,
        &format!("/admin/buildings/{building_id}/apartments/{apartment_id}"),
    )
}

// --- People ---

#[derive(Template)]
#[template(path = "admin/people/index.html")]
pub struct PeopleIndexTemplate {
    pub title: &'static str,
    /// One row per person: the person, plus a compact summary of their
    /// roles. The e-mail is shown only when the viewport has room for it
    /// (see the `email-if-space` CSS class).
    pub entries: Vec<PersonIndexEntry>,
    pub ui: Ui,
}

/// One row of the people index.
pub struct PersonIndexEntry {
    pub person: Person,
    pub roles_summary: String,
}

#[derive(Template)]
#[template(path = "admin/people/show.html")]
pub struct PeopleShowTemplate {
    pub title: String,
    pub person: Person,
    /// The person's roles, each with a link to the object it refers to.
    pub roles: Vec<PersonRole>,
    pub error: Option<String>,
    /// Contact form values: the person's current ones, or the submitted ones
    /// after a failed update.
    pub form: PersonForm,
    pub ui: Ui,
}

/// Form data of the person page's contact form. Since 0009, the database
/// rejects an e-mail address that already belongs to another person.
#[derive(serde::Deserialize)]
pub struct PersonForm {
    pub name: String,
    pub email: String,
}

/// A link target used by the role entries of the person page.
pub struct BuildingLink {
    pub id: String,
    pub name: String,
}

/// A link target used by the apartment role entries of the person page.
pub struct ApartmentLink {
    pub id: String,
    pub name: String,
    pub building_id: String,
    pub building_name: String,
}

/// One role of a person on the person page.
pub enum PersonRole {
    Admin {
        building: BuildingLink,
    },
    BuildingOwner {
        building: BuildingLink,
        start_date: String,
        end_date: Option<String>,
    },
    ApartmentOwner {
        apartment: ApartmentLink,
        start_date: String,
        end_date: Option<String>,
    },
    Tenant {
        apartment: ApartmentLink,
        start_date: String,
        end_date: Option<String>,
    },
}

/// Map one role row of [`queries::list_person_roles`] to its display entry;
/// the `kind` column hands the row to its variant.
fn person_role(row: PersonRoleRow) -> PersonRole {
    let building = || BuildingLink {
        id: row.building_id.clone().expect("building id of a role row"),
        name: row
            .building_name
            .clone()
            .expect("building name of a role row"),
    };
    let apartment = || ApartmentLink {
        id: row
            .apartment_id
            .clone()
            .expect("apartment id of a role row"),
        name: row
            .apartment_name
            .clone()
            .expect("apartment name of a role row"),
        building_id: row.building_id.clone().expect("building id of a role row"),
        building_name: row
            .building_name
            .clone()
            .expect("building name of a role row"),
    };
    let start_date = || row.start_date.clone().expect("start date of a role row");
    match row.kind.as_str() {
        "admin" => PersonRole::Admin {
            building: building(),
        },
        "building_owner" => PersonRole::BuildingOwner {
            building: building(),
            start_date: start_date(),
            end_date: row.end_date,
        },
        "apartment_owner" => PersonRole::ApartmentOwner {
            apartment: apartment(),
            start_date: start_date(),
            end_date: row.end_date,
        },
        "tenant" => PersonRole::Tenant {
            apartment: apartment(),
            start_date: start_date(),
            end_date: row.end_date,
        },
        other => unreachable!("unknown role kind {other:?}"),
    }
}

fn person_roles(rows: Vec<PersonRoleRow>) -> Vec<PersonRole> {
    rows.into_iter().map(person_role).collect()
}

/// The short role label shown in the people index listing.
fn role_label(role: &PersonRole, ui: Ui) -> String {
    match role {
        PersonRole::Admin { building } => {
            i18n::msgf(ui.lang, "role.label.contact", &[building.name.as_str()])
        }
        PersonRole::BuildingOwner { building, .. } => i18n::msgf(
            ui.lang,
            "role.label.building_owner",
            &[building.name.as_str()],
        ),
        PersonRole::ApartmentOwner { apartment, .. } => i18n::msgf(
            ui.lang,
            "role.label.owner",
            &[apartment.name.as_str(), apartment.building_name.as_str()],
        ),
        PersonRole::Tenant { apartment, .. } => i18n::msgf(
            ui.lang,
            "role.label.tenant",
            &[apartment.name.as_str(), apartment.building_name.as_str()],
        ),
    }
}

/// A compact, truncated role summary for the people index: the first two
/// roles, then "+N weitere". The person page lists the roles in full.
fn roles_summary(roles: &[PersonRole], ui: Ui) -> String {
    const MAX_SUMMARIZED: usize = 2;
    let labels: Vec<String> = roles.iter().map(|r| role_label(r, ui)).collect();
    if labels.len() > MAX_SUMMARIZED {
        let extra = (labels.len() - MAX_SUMMARIZED).to_string();
        format!(
            "{} {}",
            labels[..MAX_SUMMARIZED].join(" · "),
            i18n::msgf(ui.lang, "people.more_roles", &[&extra])
        )
    } else {
        labels.join(" · ")
    }
}

pub async fn people_index(State(pool): State<Db>, ui: Ui) -> impl axum::response::IntoResponse {
    let people = match queries::list_people(&pool).await {
        Ok(p) => p,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.people_load"),
            )
                .into_response()
        }
    };
    // All role rows in one query, grouped per person.
    let rows = match queries::list_person_roles(&pool, None).await {
        Ok(rows) => rows,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.roles_load"),
            )
                .into_response()
        }
    };
    let mut roles_by_person: HashMap<String, Vec<PersonRole>> = HashMap::new();
    for row in rows {
        let person_id = row.person_id.clone();
        roles_by_person
            .entry(person_id)
            .or_default()
            .push(person_role(row));
    }
    let entries = people
        .into_iter()
        .map(|person| PersonIndexEntry {
            roles_summary: roles_summary(
                roles_by_person
                    .get(&person.id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                ui,
            ),
            person,
        })
        .collect();
    render(PeopleIndexTemplate {
        title: i18n::msg(ui.lang, "nav.people"),
        entries,
        ui,
    })
}

/// Render the person page; `error` is shown inline (used by failed contact
/// submissions), `form` carries the submitted values then. `bad_request`
/// maps the response to 400, mirroring the inline-error pages of the other
/// forms.
async fn render_person_page(
    pool: &Db,
    person_id: &str,
    error: Option<String>,
    form: PersonForm,
    bad_request: bool,
    ui: Ui,
) -> axum::response::Response {
    let person = match queries::get_person(pool, person_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.person_load"),
            )
                .into_response()
        }
    };
    let roles = match queries::list_person_roles(pool, Some(person_id)).await {
        Ok(rows) => person_roles(rows),
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.roles_load"),
            )
                .into_response()
        }
    };
    let template = PeopleShowTemplate {
        title: person.name.clone(),
        person,
        roles,
        error,
        form,
        ui,
    };
    if bad_request {
        render_bad_request(template)
    } else {
        render(template)
    }
}

pub async fn people_show(
    Path(person_id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let person = match queries::get_person(&pool, &person_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.person_load"),
            )
                .into_response()
        }
    };
    render_person_page(
        &pool,
        &person_id,
        None,
        PersonForm {
            name: person.name.clone(),
            email: person.email.clone(),
        },
        false,
        ui,
    )
    .await
}

pub async fn people_update(
    Path(person_id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<PersonForm>,
) -> impl axum::response::IntoResponse {
    // Name/e-mail format and the unique e-mail identity are enforced by the
    // database (triggers on `people`); its rejection message is shown inline.
    match queries::update_person(&pool, &person_id, &form.name, &form.email).await {
        Ok(_) => Redirect::to(&format!("/admin/people/{person_id}")).into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => render_person_page(&pool, &person_id, Some(msg), form, true, ui).await,
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.person_save"),
            )
                .into_response(),
        },
    }
}

/// Form data of the building-owner add/edit forms.
#[derive(serde::Deserialize)]
pub struct BuildingOwnerForm {
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    /// "yes"/"no" answer to the question whether the previous building owner
    /// shall end on the day before the new one begins. Absent on the first
    /// submission; only present when the confirmation form was shown.
    #[serde(default)]
    pub close_previous: Option<String>,
    /// "yes"/"no" answer to the question whether the end date shall really be
    /// set before today (the building would lose its covering owner). Absent
    /// on the first submission; only present when the warning was shown.
    #[serde(default)]
    pub confirm_end: Option<String>,
}

/// The question asked when adding a building owner would start before the
/// previous building-owner period ends: whether that period shall end on the
/// day before the new one begins. The submitted values stay in `form`.
pub struct BuildingOwnerConfirmation {
    pub previous_name: String,
    /// The day before the new building owner's start date.
    pub previous_end: String,
    /// The new building owner's start date.
    pub new_start: String,
}

#[derive(Template)]
#[template(path = "admin/buildings/building_owner_form.html")]
pub struct BuildingOwnerFormTemplate {
    pub title: String,
    pub building: Building,
    pub error: Option<String>,
    pub form: BuildingOwnerForm,
    /// Shown above the form when the new building owner would start before the
    /// previous one ends: we ask whether the previous period shall end the day
    /// before the new one begins.
    pub confirmation: Option<BuildingOwnerConfirmation>,
    /// Shown above the form when the submitted end date would drop today's
    /// coverage: we ask whether the end date shall really be set.
    pub end_warning: Option<EndDateWarning>,
    /// Form action URL (create or update endpoint).
    pub action: String,
    pub submit_label: String,
    pub cancel_url: String,
    /// Known owners, offered as suggestions on the name/e-mail fields.
    pub people: Vec<Person>,
    pub ui: Ui,
}

/// One of the possible notices shown above the building-owner form: either a
/// rejection message from a failed submission, the confirmation question
/// whether the previous building owner shall end on the day before the new one
/// begins, or the warning that a submitted end date would drop today's
/// coverage.
enum BuildingOwnerFormNotice {
    Error(String),
    Confirmation(BuildingOwnerConfirmation),
    EndDateWarning(EndDateWarning),
}

#[allow(clippy::too_many_arguments)]
fn building_owner_form_template(
    title: String,
    building: Building,
    notice: Option<BuildingOwnerFormNotice>,
    form: BuildingOwnerForm,
    building_id: &str,
    owner_id: Option<&str>,
    people: Vec<Person>,
    ui: Ui,
) -> BuildingOwnerFormTemplate {
    let (error, confirmation, end_warning) = match notice {
        None => (None, None, None),
        Some(BuildingOwnerFormNotice::Error(msg)) => (Some(msg), None, None),
        Some(BuildingOwnerFormNotice::Confirmation(confirmation)) => {
            (None, Some(confirmation), None)
        }
        Some(BuildingOwnerFormNotice::EndDateWarning(warning)) => (None, None, Some(warning)),
    };
    let action = match owner_id {
        Some(owner_id) => format!("/admin/buildings/{building_id}/building_owners/{owner_id}"),
        None => format!("/admin/buildings/{building_id}/building_owners"),
    };
    let submit_label = if owner_id.is_some() {
        i18n::msg(ui.lang, "common.save_changes").to_string()
    } else {
        i18n::msg(ui.lang, "owners.add").to_string()
    };
    BuildingOwnerFormTemplate {
        title,
        cancel_url: format!("/admin/buildings/{building_id}"),
        building,
        error,
        form,
        confirmation,
        end_warning,
        action,
        submit_label,
        people,
        ui,
    }
}

pub async fn building_owners_new(
    Path(building_id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let people = match load_people(&pool, ui).await {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    render(building_owner_form_template(
        i18n::msg(ui.lang, "building_owner.new_title").to_string(),
        building,
        None,
        BuildingOwnerForm {
            name: String::new(),
            email: String::new(),
            start_date: String::new(),
            end_date: None,
            close_previous: None,
            confirm_end: None,
        },
        &building_id,
        None,
        people,
        ui,
    ))
}

/// Turn the outcome of creating a building owner into a response: redirect on
/// success, re-render with the database's rejection message on failure.
async fn building_owner_created(
    pool: &Db,
    building_id: &str,
    form: BuildingOwnerForm,
    result: Result<BuildingOwner, sqlx::Error>,
    ui: Ui,
) -> axum::response::Response {
    match result {
        Ok(_) => Redirect::to(&format!("/admin/buildings/{building_id}")).into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                let building = match load_building(pool, building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                let people = match load_people(pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(building_owner_form_template(
                    i18n::msg(ui.lang, "building_owner.new_title").to_string(),
                    building,
                    Some(BuildingOwnerFormNotice::Error(msg)),
                    form,
                    building_id,
                    None,
                    people,
                    ui,
                ))
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.building_owner_create"),
            )
                .into_response(),
        },
    }
}

pub async fn building_owners_create(
    Path(building_id): Path<String>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<BuildingOwnerForm>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_building(&pool, &building_id, ui).await {
        return err.into_response();
    }
    let end_opt = normalize_end_date(form.end_date.as_deref());

    // If the new building owner would start while the previous one is still
    // current (it does not already end on the day before), the database
    // rejects the insert. Ask the user whether the previous building owner
    // shall end on the day before the new one begins; the answer comes back as
    // `close_previous`.
    if let Some(prev) = queries::get_building_owner_on(&pool, &building_id, &form.start_date)
        .await
        .ok()
        .flatten()
    {
        let prev_end = match previous_day(&form.start_date) {
            Some(d) => d,
            None => {
                // Unparseable start date: the database reports it.
                let result = queries::create_building_owner(
                    &pool,
                    &building_id,
                    &form.name,
                    &form.email,
                    &form.start_date,
                    end_opt,
                )
                .await;
                return building_owner_created(&pool, &building_id, form, result, ui).await;
            }
        };
        match form.close_previous.as_deref() {
            None => {
                let building = match load_building(&pool, &building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                let new_start = form.start_date.clone();
                return render(building_owner_form_template(
                    i18n::msg(ui.lang, "building_owner.new_title").to_string(),
                    building,
                    Some(BuildingOwnerFormNotice::Confirmation(
                        BuildingOwnerConfirmation {
                            previous_name: prev.name,
                            previous_end: prev_end,
                            new_start,
                        },
                    )),
                    form,
                    &building_id,
                    None,
                    people,
                    ui,
                ));
            }
            Some("yes") => {
                let result = queries::create_building_owner_closing_previous(
                    &pool,
                    &building_id,
                    queries::PreviousPeriod {
                        id: &prev.id,
                        end_date: &prev_end,
                    },
                    &form.name,
                    &form.email,
                    &form.start_date,
                    end_opt,
                )
                .await;
                return building_owner_created(&pool, &building_id, form, result, ui).await;
            }
            _ => {
                // The user declined to end the previous building owner the day
                // before; the operation cannot succeed, so tell them.
                let building = match load_building(&pool, &building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                return render_bad_request(building_owner_form_template(
                    i18n::msg(ui.lang, "building_owner.new_title").to_string(),
                    building,
                    Some(BuildingOwnerFormNotice::Error(i18n::msgf(
                        ui.lang,
                        "building_owner.decline_close",
                        &[prev.name.as_str(), prev_end.as_str()],
                    ))),
                    form,
                    &building_id,
                    None,
                    people,
                    ui,
                ));
            }
        }
    }

    // The start is neither covered nor tiled: if an earlier period already
    // ended, the new one must begin on the day after it. Check here so the
    // rejection can name the actual dates (SQLite trigger messages cannot
    // interpolate values); the database still enforces the rule.
    if let Some(prev_end) =
        queries::get_building_owner_end_before(&pool, &building_id, &form.start_date)
            .await
            .ok()
            .flatten()
    {
        if let Some(next) = next_day(&prev_end) {
            if next != form.start_date {
                let building = match load_building(&pool, &building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                return render_bad_request(building_owner_form_template(
                    i18n::msg(ui.lang, "building_owner.new_title").to_string(),
                    building,
                    Some(BuildingOwnerFormNotice::Error(i18n::msgf(
                        ui.lang,
                        "building_owner.gap_dates",
                        &[prev_end.as_str(), next.as_str(), form.start_date.as_str()],
                    ))),
                    form,
                    &building_id,
                    None,
                    people,
                    ui,
                ));
            }
        }
    }

    // No previous building owner covering the new start (the chain is already
    // tiled): create directly; the database enforces the remaining rules.
    let result = queries::create_building_owner(
        &pool,
        &building_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await;
    building_owner_created(&pool, &building_id, form, result, ui).await
}

pub async fn building_owners_edit(
    Path((building_id, owner_id)): Path<(String, String)>,
    State(pool): State<Db>,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id, ui).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    match queries::get_building_owner(&pool, &owner_id).await {
        Ok(Some(owner)) => {
            if owner.building_id != building_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
            let people = match load_people(&pool, ui).await {
                Ok(p) => p,
                Err(err) => return err.into_response(),
            };
            let form = BuildingOwnerForm {
                name: owner.name.clone(),
                email: owner.email.clone(),
                start_date: owner.start_date.clone(),
                end_date: owner.end_date.clone(),
                close_previous: None,
                confirm_end: None,
            };
            render(building_owner_form_template(
                i18n::msg(ui.lang, "building_owner.edit_title").to_string(),
                building,
                None,
                form,
                &building_id,
                Some(&owner_id),
                people,
                ui,
            ))
        }
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            i18n::msg(ui.lang, "common.not_found"),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            i18n::msg(ui.lang, "error.building_owner_load"),
        )
            .into_response(),
    }
}

/// The warning if setting `new_end` on `owner` would drop the building's
/// coverage of today (only relevant while apartments actually depend on the
/// building owner).
async fn building_owner_end_date_warning(
    pool: &Db,
    building_id: &str,
    owner: &BuildingOwner,
    new_end: Option<&str>,
) -> Option<EndDateWarning> {
    let new_end = new_end?;
    let new_end_date = NaiveDate::parse_from_str(new_end, "%Y-%m-%d").ok()?;
    if !end_drops_today_coverage(
        &owner.start_date,
        owner.end_date.as_deref(),
        Some(new_end_date),
        Local::now().date_naive(),
    ) {
        return None;
    }
    if !queries::building_has_apartments_depending_on_building_owner(pool, building_id)
        .await
        .unwrap_or(false)
    {
        return None;
    }
    Some(end_date_warning(new_end))
}

pub async fn building_owners_update(
    Path((building_id, owner_id)): Path<(String, String)>,
    State(pool): State<Db>,
    ui: Ui,
    Form(form): Form<BuildingOwnerForm>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_building(&pool, &building_id, ui).await {
        return err.into_response();
    }
    // Ensure the building owner belongs to this building.
    let existing = match queries::get_building_owner(&pool, &owner_id).await {
        Ok(Some(existing)) => {
            if existing.building_id != building_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
            existing
        }
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
    };
    let end_opt = normalize_end_date(form.end_date.as_deref());

    // Setting the end date of the currently-covering period before today would
    // leave the building without an owner; ask for confirmation first.
    if let Some(warning) =
        building_owner_end_date_warning(&pool, &building_id, &existing, end_opt).await
    {
        match form.confirm_end.as_deref() {
            None => {
                let building = match load_building(&pool, &building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                return render(building_owner_form_template(
                    i18n::msg(ui.lang, "building_owner.edit_title").to_string(),
                    building,
                    Some(BuildingOwnerFormNotice::EndDateWarning(warning)),
                    form,
                    &building_id,
                    Some(&owner_id),
                    people,
                    ui,
                ));
            }
            Some("no") => {
                return Redirect::to(&format!(
                    "/admin/buildings/{building_id}/building_owners/{owner_id}/edit"
                ))
                .into_response();
            }
            _ => {} // "yes": apply the update below.
        }
    }

    // Field checks and chain tiling are enforced by the database (triggers);
    // its rejection message is shown inline.
    match queries::update_building_owner(
        &pool,
        &owner_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await
    {
        Ok(_) => Redirect::to(&format!("/admin/buildings/{building_id}")).into_response(),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => {
                let building = match load_building(&pool, &building_id, ui).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                let people = match load_people(&pool, ui).await {
                    Ok(p) => p,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(building_owner_form_template(
                    i18n::msg(ui.lang, "building_owner.edit_title").to_string(),
                    building,
                    Some(BuildingOwnerFormNotice::Error(msg)),
                    form,
                    &building_id,
                    Some(&owner_id),
                    people,
                    ui,
                ))
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.building_owner_save"),
            )
                .into_response(),
        },
    }
}

pub async fn building_owners_delete(
    Path((building_id, owner_id)): Path<(String, String)>,
    State(pool): State<Db>,
    headers: HeaderMap,
    ui: Ui,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_building(&pool, &building_id, ui).await {
        return err.into_response();
    }
    match queries::get_building_owner(&pool, &owner_id).await {
        Ok(Some(existing)) => {
            if existing.building_id != building_id {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    i18n::msg(ui.lang, "common.not_found"),
                )
                    .into_response();
            }
        }
        _ => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                i18n::msg(ui.lang, "common.not_found"),
            )
                .into_response()
        }
    }

    // The database rejects deleting a period in the middle of the chain or
    // the last owner while apartments depend on it (see
    // `building_owners_guard_*` in 0005_building_owners.sql); its message is
    // shown inline on the building page.
    match queries::delete_building_owner(&pool, &owner_id).await {
        Ok(_) => redirect_after_post(&headers, &format!("/admin/buildings/{building_id}")),
        Err(err) => match db_message(&err, ui) {
            Some(msg) => (
                axum::http::StatusCode::BAD_REQUEST,
                render_building_page(&pool, &building_id, Some(msg), ui).await,
            )
                .into_response(),
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                i18n::msg(ui.lang, "error.building_owner_delete"),
            )
                .into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::rotation_seed_options;

    #[test]
    fn options_cover_exactly_zero_to_n_minus_one() {
        assert_eq!(
            rotation_seed_options(0, 3),
            vec![(0, true), (1, false), (2, false)]
        );
    }

    #[test]
    fn options_include_out_of_range_current() {
        assert_eq!(
            rotation_seed_options(12, 3),
            vec![(0, false), (1, false), (2, false), (12, true)]
        );
    }

    #[test]
    fn apartmentless_building_offers_only_zero() {
        assert_eq!(rotation_seed_options(0, 0), vec![(0, true)]);
        assert_eq!(rotation_seed_options(5, 0), vec![(5, true)]);
    }
}
