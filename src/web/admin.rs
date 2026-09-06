use crate::db::models::{Apartment, Building, BuildingAdministrator, Ownership, Tenancy};
use crate::db::queries;
use crate::db::Db;
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
}

/// One row of the Kehrwoche roster as the templates render it: dates in
/// German display format, plus a flag for the week that is currently running.
pub struct ScheduleRow {
    pub iso_week: u32,
    pub start: String,
    pub end: String,
    pub assignee_name: Option<String>,
    pub delegated: bool,
    pub is_current: bool,
}

/// Map scheduler weeks to display rows, marking the week containing `today`.
fn schedule_rows(schedule: Vec<WeekAssignment>, today: NaiveDate) -> Vec<ScheduleRow> {
    schedule
        .into_iter()
        .map(|w| ScheduleRow {
            iso_week: w.iso_week,
            start: w.start.format("%d.%m.%Y").to_string(),
            end: w.end.format("%d.%m.%Y").to_string(),
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
    pub apartments: Vec<ApartmentRow>,
    pub year: i32,
    pub schedule: Vec<ScheduleRow>,
}

#[derive(Template)]
#[template(path = "admin/buildings/schedule.html")]
pub struct BuildingsScheduleTemplate {
    pub title: String,
    pub building: Building,
    pub year: i32,
    pub schedule: Vec<ScheduleRow>,
}

#[derive(Template)]
#[template(path = "admin/apartments/new.html")]
pub struct ApartmentsNewTemplate {
    pub title: String,
    pub building: Building,
    pub error: Option<String>,
    pub name: String,
    pub description: String,
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
}

#[derive(Template)]
#[template(path = "admin/apartments/_table_body.html")]
pub struct ApartmentsTableBodyTemplate {
    pub building: Building,
    pub apartments: Vec<ApartmentRow>,
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
}

// --- Buildings ---

#[derive(serde::Deserialize)]
pub struct CreateBuildingForm {
    pub name: String,
    pub description: String,
    pub admin_name: String,
    pub admin_email: String,
}

const MAX_NAME_LEN: usize = 30;

fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Name darf nicht leer sein.".into());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(format!("Name darf höchstens {MAX_NAME_LEN} Zeichen haben."));
    }
    Ok(())
}

fn validate_email(email: &str) -> Result<(), String> {
    let email = email.trim();
    if let Some(at_pos) = email.find('@') {
        if !email[at_pos + 1..].contains('.') {
            return Err("Die E-Mail-Adresse muss nach dem '@' einen Punkt enthalten.".into());
        }
    } else {
        return Err("Die E-Mail-Adresse muss ein '@' enthalten.".into());
    }
    Ok(())
}

fn validate_person_input(
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Name darf nicht leer sein.".into());
    }
    validate_email(email)?;

    let start = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
        .map_err(|_| "Startdatum muss im Format JJJJ-MM-TT vorliegen.".to_string())?;
    if let Some(ed) = end_date {
        if !ed.trim().is_empty() {
            let end = chrono::NaiveDate::parse_from_str(ed, "%Y-%m-%d")
                .map_err(|_| "Enddatum muss im Format JJJJ-MM-TT vorliegen.".to_string())?;
            if start > end {
                return Err("Das Startdatum darf nicht nach dem Enddatum liegen.".into());
            }
        }
    }
    Ok(())
}

pub async fn buildings_index(State(pool): State<Db>) -> impl axum::response::IntoResponse {
    match queries::list_buildings(&pool).await {
        Ok(buildings) => render(BuildingsIndexTemplate {
            title: "Gebäude",
            buildings,
        }),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Gebäude konnten nicht geladen werden.",
        )
            .into_response(),
    }
}

pub async fn buildings_new() -> impl axum::response::IntoResponse {
    render(BuildingsNewTemplate {
        title: "Neues Gebäude anlegen",
        error: None,
        name: String::new(),
        description: String::new(),
        admin_name: String::new(),
        admin_email: String::new(),
    })
}

pub async fn buildings_create(
    State(pool): State<Db>,
    Form(form): Form<CreateBuildingForm>,
) -> impl axum::response::IntoResponse {
    // First validation error, respecting the original check order.
    let error = validate_name(&form.name)
        .err()
        .or_else(|| {
            form.admin_name
                .trim()
                .is_empty()
                .then(|| "Name des Ansprechpartners darf nicht leer sein.".to_string())
        })
        .or_else(|| validate_email(&form.admin_email).err());
    if let Some(msg) = error {
        return render_bad_request(BuildingsNewTemplate {
            title: "Neues Gebäude anlegen",
            error: Some(msg),
            name: form.name,
            description: form.description,
            admin_name: form.admin_name,
            admin_email: form.admin_email,
        });
    }

    match queries::create_building(
        &pool,
        &form.name,
        &form.description,
        &form.admin_name,
        &form.admin_email,
    )
    .await
    {
        Ok(building) => Redirect::to(&format!("/admin/buildings/{}", building.id)).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Gebäude konnte nicht angelegt werden.",
        )
            .into_response(),
    }
}

pub async fn buildings_show(
    Path(id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    match queries::get_building(&pool, &id).await {
        Ok(Some((building, admins))) => {
            let apartments = match queries::list_apartments(&pool, &building.id).await {
                Ok(apartments) => apartment_rows(apartments),
                Err(_) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Wohnungen konnten nicht geladen werden.",
                    )
                        .into_response()
                }
            };
            // Compact schedule: only the remaining weeks of the current year.
            let year = Local::now().date_naive().year();
            let today = Local::now().date_naive();
            let schedule: Vec<WeekAssignment> =
                match scheduler::schedule_for_year(&building.id, year, &pool).await {
                    Ok(weeks) => weeks
                        .into_iter()
                        .filter(|w| w.end >= today)
                        .take(12)
                        .collect(),
                    Err(_) => {
                        return (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            "Der Plan konnte nicht berechnet werden.",
                        )
                            .into_response()
                    }
                };
            render(BuildingsShowTemplate {
                title: building.name.clone(),
                building,
                admins,
                apartments,
                year,
                schedule: schedule_rows(schedule, today),
            })
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Gebäude konnte nicht geladen werden.",
        )
            .into_response(),
    }
}

pub async fn buildings_schedule(
    Path(id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    match queries::get_building(&pool, &id).await {
        Ok(Some((building, _admins))) => {
            let year = Local::now().date_naive().year();
            let today = Local::now().date_naive();
            match scheduler::schedule_for_year(&building.id, year, &pool).await {
                Ok(schedule) => render(BuildingsScheduleTemplate {
                    title: "Jahresplan".to_string(),
                    building,
                    year,
                    schedule: schedule_rows(schedule, today),
                }),
                Err(_) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Der Plan konnte nicht berechnet werden.",
                )
                    .into_response(),
            }
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Gebäude konnte nicht geladen werden.",
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

// --- Apartments ---

#[derive(serde::Deserialize)]
pub struct ApartmentForm {
    pub name: String,
    pub description: String,
}

/// Load building or return a short error; used by apartment subroutes.
async fn load_building(
    pool: &Db,
    building_id: &str,
) -> Result<Building, (axum::http::StatusCode, &'static str)> {
    match queries::get_building(pool, building_id).await {
        Ok(Some((building, _))) => Ok(building),
        Ok(None) => Err((axum::http::StatusCode::NOT_FOUND, "Nicht gefunden")),
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Gebäude konnte nicht geladen werden.",
        )),
    }
}

/// Load apartment or return a short error.
async fn load_apartment_owned_by(
    pool: &Db,
    building_id: &str,
    apartment_id: &str,
) -> Result<Apartment, (axum::http::StatusCode, &'static str)> {
    match queries::get_apartment(pool, apartment_id).await {
        Ok(Some(apartment)) => {
            if apartment.building_id != building_id {
                return Err((axum::http::StatusCode::NOT_FOUND, "Nicht gefunden"));
            }
            Ok(apartment)
        }
        Ok(None) => Err((axum::http::StatusCode::NOT_FOUND, "Nicht gefunden")),
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Wohnung konnte nicht geladen werden.",
        )),
    }
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
) -> axum::response::Response {
    let building = match load_building(pool, building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(pool, building_id, apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    match (
        queries::list_ownerships(pool, apartment_id).await,
        queries::list_tenancies(pool, apartment_id).await,
    ) {
        (Ok(ownerships), Ok(tenancies)) => render_bad_request(ApartmentsShowTemplate {
            title: apartment.name.clone(),
            building,
            apartment,
            ownerships,
            tenancies,
            error: Some(error),
            owner_form,
            tenant_form,
        }),
        _ => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Wohnungsdaten konnten nicht geladen werden.",
        )
            .into_response(),
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
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    render(ApartmentsNewTemplate {
        title: "Neue Wohnung".to_string(),
        building,
        error: None,
        name: String::new(),
        description: String::new(),
    })
}

pub async fn apartments_create(
    Path(building_id): Path<String>,
    State(pool): State<Db>,
    Form(form): Form<ApartmentForm>,
) -> impl axum::response::IntoResponse {
    if let Err(msg) = validate_name(&form.name) {
        let building = match load_building(&pool, &building_id).await {
            Ok(b) => b,
            Err(err) => return err.into_response(),
        };
        return render_bad_request(ApartmentsNewTemplate {
            title: "Neue Wohnung".to_string(),
            building,
            error: Some(msg),
            name: form.name,
            description: form.description,
        });
    }

    match queries::create_apartment(&pool, &building_id, &form.name, &form.description).await {
        Ok(apartment) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{}",
            apartment.id
        ))
        .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Wohnung konnte nicht angelegt werden.",
        )
            .into_response(),
    }
}

pub async fn apartments_edit(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    let name = apartment.name.clone();
    let description = apartment.description.clone();
    render(ApartmentsEditTemplate {
        title: "Wohnung bearbeiten".to_string(),
        building,
        apartment,
        error: None,
        name,
        description,
    })
}

pub async fn apartments_show(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };

    match (
        queries::list_ownerships(&pool, &apartment_id).await,
        queries::list_tenancies(&pool, &apartment_id).await,
    ) {
        (Ok(ownerships), Ok(tenancies)) => render(ApartmentsShowTemplate {
            title: apartment.name.clone(),
            building,
            apartment,
            ownerships,
            tenancies,
            error: None,
            owner_form: None,
            tenant_form: None,
        }),
        _ => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Wohnungsdaten konnten nicht geladen werden.",
        )
            .into_response(),
    }
}

pub async fn apartments_update(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    Form(form): Form<ApartmentForm>,
) -> impl axum::response::IntoResponse {
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    if let Err(msg) = validate_name(&form.name) {
        let building = match load_building(&pool, &building_id).await {
            Ok(b) => b,
            Err(err) => return err.into_response(),
        };
        return render_bad_request(ApartmentsEditTemplate {
            title: "Wohnung bearbeiten".to_string(),
            building,
            apartment,
            error: Some(msg),
            name: form.name,
            description: form.description,
        });
    }

    match queries::update_apartment(&pool, &apartment_id, &form.name, &form.description).await {
        Ok(_) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        ))
        .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Wohnung konnte nicht gespeichert werden.",
        )
            .into_response(),
    }
}

pub async fn apartments_delete(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
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
    RawForm(body): RawForm,
) -> impl axum::response::IntoResponse {
    // Apartment ids in their new DOM order, one `item` field per row.
    let items: Vec<String> = form_urlencoded::parse(&body)
        .filter(|(key, _)| key == "item")
        .map(|(_, value)| value.into_owned())
        .collect();

    // The submitted ids must be exactly this building's apartments: no
    // additions, omissions, or duplicates.
    let current = match queries::list_apartments(&pool, &building_id).await {
        Ok(list) => list,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Wohnungen konnten nicht geladen werden.",
            )
                .into_response()
        }
    };
    let mut current_ids: Vec<&str> = current.iter().map(|a| a.id.as_str()).collect();
    current_ids.sort_unstable();
    let mut submitted: Vec<&str> = items.iter().map(String::as_str).collect();
    submitted.sort_unstable();
    if current_ids != submitted {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Ungültige Reihenfolge der Wohnungen",
        )
            .into_response();
    }

    if queries::reorder_apartments(&pool, &building_id, &items)
        .await
        .is_err()
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Die Reihenfolge konnte nicht gespeichert werden.",
        )
            .into_response();
    }

    // Re-render just the table body so htmx can swap in the new order.
    let building = match load_building(&pool, &building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartments = match queries::list_apartments(&pool, &building_id).await {
        Ok(list) => apartment_rows(list),
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Wohnungen konnten nicht neu geladen werden.",
            )
                .into_response()
        }
    };
    render(ApartmentsTableBodyTemplate {
        building,
        apartments,
    })
}

// --- Ownerships ---

#[derive(serde::Deserialize)]
pub struct OwnershipForm {
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

fn normalize_end_date(end_date: Option<&str>) -> Option<&str> {
    end_date.map(str::trim).filter(|s| !s.is_empty())
}

pub async fn ownerships_create(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    Form(form): Form<OwnershipForm>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        return err.into_response();
    }
    let end_opt = normalize_end_date(form.end_date.as_deref());
    if let Err(msg) = validate_person_input(&form.name, &form.email, &form.start_date, end_opt) {
        return render_apartment_show_error(
            &pool,
            &building_id,
            &apartment_id,
            msg,
            Some(form),
            None,
        )
        .await;
    }

    let start = NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").expect("validated date");
    let end = end_opt.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

    // Ownership periods must tile the apartment's timeline: no overlaps, no
    // gaps, so the apartment never has an unassigned week.
    match queries::list_ownerships(&pool, &apartment_id).await {
        Ok(existing) => {
            if let Some(reason) = ownership_chain_violation(&existing, None, start, end) {
                return render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    reason,
                    Some(form),
                    None,
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Eigentümer konnten nicht geladen werden.",
            )
                .into_response()
        }
    }

    match queries::create_ownership(
        &pool,
        &apartment_id,
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
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Eigentum konnte nicht angelegt werden.",
        )
            .into_response(),
    }
}

pub async fn ownerships_edit(
    Path((building_id, apartment_id, ownership_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    match queries::get_ownership(&pool, &ownership_id).await {
        Ok(Some(ownership)) => {
            if ownership.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response();
            }
            let form = OwnershipForm {
                name: ownership.name.clone(),
                email: ownership.email.clone(),
                start_date: ownership.start_date.clone(),
                end_date: ownership.end_date.clone(),
            };
            render(OwnershipsEditTemplate {
                title: "Eigentümer bearbeiten".to_string(),
                building,
                apartment,
                ownership,
                error: None,
                form,
            })
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Eigentum konnte nicht geladen werden.",
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
) -> axum::response::Response {
    let building = match load_building(pool, building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    render_bad_request(OwnershipsEditTemplate {
        title: "Eigentümer bearbeiten".to_string(),
        building,
        apartment: apartment.clone(),
        ownership: ownership.clone(),
        error: Some(error),
        form,
    })
}

pub async fn ownerships_update(
    Path((building_id, apartment_id, ownership_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    Form(form): Form<OwnershipForm>,
) -> impl axum::response::IntoResponse {
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    // Ensure the ownership belongs to this apartment
    let ownership = match queries::get_ownership(&pool, &ownership_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response();
            }
            existing
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
    };

    let end_opt = normalize_end_date(form.end_date.as_deref());
    if let Err(msg) = validate_person_input(&form.name, &form.email, &form.start_date, end_opt) {
        return render_ownership_edit_error(&pool, &building_id, &apartment, &ownership, msg, form)
            .await;
    }

    let start = NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").expect("validated date");
    let end = end_opt.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

    // Ownership periods must tile the apartment's timeline: no overlaps, no
    // gaps, so the apartment never has an unassigned week.
    match queries::list_ownerships(&pool, &apartment_id).await {
        Ok(existing) => {
            if let Some(reason) =
                ownership_chain_violation(&existing, Some(&ownership_id), start, end)
            {
                return render_ownership_edit_error(
                    &pool,
                    &building_id,
                    &apartment,
                    &ownership,
                    reason,
                    form,
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Eigentümer konnten nicht geladen werden.",
            )
                .into_response()
        }
    }

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
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Eigentum konnte nicht gespeichert werden.",
        )
            .into_response(),
    }
}

pub async fn ownerships_delete(
    Path((building_id, apartment_id, ownership_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        return err.into_response();
    }
    match queries::get_ownership(&pool, &ownership_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response();
            }
            // Deleting a period between two others would open a hole in the
            // ownership chain and leave weeks without an owner; only the first
            // or the last period of the chain may be deleted.
            match queries::list_ownerships(&pool, &apartment_id).await {
                Ok(all) => {
                    let own_start = NaiveDate::parse_from_str(&existing.start_date, "%Y-%m-%d")
                        .expect("validated start_date");
                    let neighbor_starts: Vec<NaiveDate> = all
                        .iter()
                        .filter(|o| o.id != ownership_id)
                        .filter_map(|o| NaiveDate::parse_from_str(&o.start_date, "%Y-%m-%d").ok())
                        .collect();
                    let is_first = neighbor_starts.iter().all(|s| *s > own_start);
                    let is_last = neighbor_starts.iter().all(|s| *s < own_start);
                    if !is_first && !is_last {
                        return render_apartment_show_error(
                            &pool,
                            &building_id,
                            &apartment_id,
                            "Dieses Eigentum liegt zwischen zwei anderen Eigentümerzeiträumen. \
                             Es kann nur das erste oder das letzte Eigentum gelöscht werden."
                                .to_string(),
                            None,
                            None,
                        )
                        .await;
                    }
                }
                Err(_) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Eigentümer konnten nicht geladen werden.",
                    )
                        .into_response()
                }
            }
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
    }

    let _ = queries::delete_ownership(&pool, &ownership_id).await;
    redirect_after_post(
        &headers,
        &format!("/admin/buildings/{building_id}/apartments/{apartment_id}"),
    )
}

// --- Tenancies ---

#[derive(serde::Deserialize)]
pub struct TenancyForm {
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

/// True when [start1, end1] and [start2, end2] overlap; a None end is open-ended.
/// The domain model allows at most one active tenancy per apartment.
fn periods_overlap(
    start1: NaiveDate,
    end1: Option<NaiveDate>,
    start2: NaiveDate,
    end2: Option<NaiveDate>,
) -> bool {
    let end1_eff = end1.unwrap_or(NaiveDate::MAX);
    let end2_eff = end2.unwrap_or(NaiveDate::MAX);
    start1 <= end2_eff && start2 <= end1_eff
}

pub async fn tenancies_create(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
    Form(form): Form<TenancyForm>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        return err.into_response();
    }
    let end_opt = normalize_end_date(form.end_date.as_deref());
    if let Err(msg) = validate_person_input(&form.name, &form.email, &form.start_date, end_opt) {
        return render_apartment_show_error(
            &pool,
            &building_id,
            &apartment_id,
            msg,
            None,
            Some(form),
        )
        .await;
    }

    let start = NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").expect("validated date");
    let end = end_opt.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

    // Enforce: at most one active tenancy per apartment at any point in time
    match queries::list_tenancies(&pool, &apartment_id).await {
        Ok(existing) => {
            if existing
                .iter()
                .any(|t| overlaps_tenancy(t, start, end, None))
            {
                return render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    "Das Mietverhältnis überschneidet ein bestehendes Mietverhältnis \
                     dieser Wohnung."
                        .to_string(),
                    None,
                    Some(form),
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Mietverhältnisse konnten nicht geladen werden.",
            )
                .into_response()
        }
    }

    match queries::create_tenancy(
        &pool,
        &apartment_id,
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
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Mietverhältnis konnte nicht angelegt werden.",
        )
            .into_response(),
    }
}

pub async fn tenancies_edit(
    Path((building_id, apartment_id, tenancy_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    let building = match load_building(&pool, &building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    match queries::get_tenancy(&pool, &tenancy_id).await {
        Ok(Some(tenancy)) => {
            if tenancy.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response();
            }
            let form = TenancyForm {
                name: tenancy.name.clone(),
                email: tenancy.email.clone(),
                start_date: tenancy.start_date.clone(),
                end_date: tenancy.end_date.clone(),
            };
            render(TenanciesEditTemplate {
                title: "Mieter bearbeiten".to_string(),
                building,
                apartment,
                tenancy,
                error: None,
                form,
            })
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Mietverhältnis konnte nicht geladen werden.",
        )
            .into_response(),
    }
}

/// Does the dated record (start_date/end_date strings) overlap [start, end]?
fn record_overlaps(
    start_date: &str,
    end_date: Option<&str>,
    start: NaiveDate,
    end: Option<NaiveDate>,
) -> bool {
    match NaiveDate::parse_from_str(start_date, "%Y-%m-%d") {
        Ok(r_start) => {
            let r_end = end_date.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            periods_overlap(r_start, r_end, start, end)
        }
        Err(_) => false,
    }
}

/// Does the tenancy record overlap [start, end] (excluding `exclude_id`)?
fn overlaps_tenancy(
    t: &Tenancy,
    start: NaiveDate,
    end: Option<NaiveDate>,
    exclude_id: Option<&str>,
) -> bool {
    if exclude_id == Some(t.id.as_str()) {
        return false;
    }
    record_overlaps(&t.start_date, t.end_date.as_deref(), start, end)
}

/// Does the ownership record overlap [start, end] (excluding `exclude_id`)?
fn overlaps_ownership(
    o: &Ownership,
    start: NaiveDate,
    end: Option<NaiveDate>,
    exclude_id: Option<&str>,
) -> bool {
    if exclude_id == Some(o.id.as_str()) {
        return false;
    }
    record_overlaps(&o.start_date, o.end_date.as_deref(), start, end)
}

/// Ownership periods of an apartment must tile its timeline seamlessly: every
/// period starts on the day after the previous one ends (and, except for the
/// last, ends on the day before the next one starts). That guarantees the
/// apartment always has exactly one covering owner and the schedule never has
/// an unassigned week between owners. Returns a human-readable reason when
/// inserting/replacing the period [start, end] (`exclude_id` skips the record
/// being updated) would break the chain.
fn ownership_chain_violation(
    existing: &[Ownership],
    exclude_id: Option<&str>,
    start: NaiveDate,
    end: Option<NaiveDate>,
) -> Option<String> {
    let others: Vec<&Ownership> = existing
        .iter()
        .filter(|o| exclude_id != Some(o.id.as_str()))
        .collect();

    if others
        .iter()
        .any(|o| overlaps_ownership(o, start, end, None))
    {
        return Some(
            "Das Eigentum überschneidet ein bestehendes Eigentum dieser Wohnung".to_string(),
        );
    }

    // Neighbors as parsed (start, end) pairs; dates are validated on input.
    let neighbors: Vec<(NaiveDate, Option<NaiveDate>)> = others
        .iter()
        .filter_map(|o| {
            let s = NaiveDate::parse_from_str(&o.start_date, "%Y-%m-%d").ok()?;
            let e = o
                .end_date
                .as_deref()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            Some((s, e))
        })
        .collect();

    // The period must start on the day after the previous one ends.
    let prev_end = neighbors
        .iter()
        .filter(|(_, e)| e.is_some_and(|e| e < start))
        .map(|(_, e)| e.expect("filtered"))
        .max();
    if let Some(prev_end) = prev_end {
        let expected = prev_end + chrono::Duration::days(1);
        if start != expected {
            return Some(format!(
                "Das Eigentum muss am Tag nach dem Ende des vorherigen Eigentums beginnen \
                 (erwartet: {expected}); so blieben {} Tage ohne Eigentümer",
                start.signed_duration_since(prev_end).num_days() - 1
            ));
        }
    }

    // ...and end on the day before the next one starts.
    let next_start = neighbors
        .iter()
        .filter(|(s, _)| end.is_some_and(|e| *s > e))
        .map(|(s, _)| *s)
        .min();
    if let (Some(next_start), Some(end)) = (next_start, end) {
        let expected = next_start - chrono::Duration::days(1);
        if end != expected {
            return Some(format!(
                "Das Eigentum muss am Tag vor dem Beginn des nächsten Eigentums enden \
                 (erwartet: {expected}); so blieben {} Tage ohne Eigentümer",
                next_start.signed_duration_since(end).num_days() - 1
            ));
        }
    }

    None
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
) -> axum::response::Response {
    let building = match load_building(pool, building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    render_bad_request(TenanciesEditTemplate {
        title: "Mieter bearbeiten".to_string(),
        building,
        apartment: apartment.clone(),
        tenancy: tenancy.clone(),
        error: Some(error),
        form,
    })
}

pub async fn tenancies_update(
    Path((building_id, apartment_id, tenancy_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    Form(form): Form<TenancyForm>,
) -> impl axum::response::IntoResponse {
    let apartment = match load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        Ok(a) => a,
        Err(err) => return err.into_response(),
    };
    // Ensure the tenancy belongs to this apartment
    let tenancy = match queries::get_tenancy(&pool, &tenancy_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response();
            }
            existing
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
    };

    let end_opt = normalize_end_date(form.end_date.as_deref());
    if let Err(msg) = validate_person_input(&form.name, &form.email, &form.start_date, end_opt) {
        return render_tenancy_edit_error(&pool, &building_id, &apartment, &tenancy, msg, form)
            .await;
    }

    let start = NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").expect("validated date");
    let end = end_opt.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

    // Enforce: at most one active tenancy per apartment at any point in time
    match queries::list_tenancies(&pool, &apartment_id).await {
        Ok(existing) => {
            if existing
                .iter()
                .any(|t| overlaps_tenancy(t, start, end, Some(&tenancy_id)))
            {
                return render_tenancy_edit_error(
                    &pool,
                    &building_id,
                    &apartment,
                    &tenancy,
                    "Das Mietverhältnis überschneidet ein bestehendes Mietverhältnis \
                     dieser Wohnung."
                        .to_string(),
                    form,
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Mietverhältnisse konnten nicht geladen werden.",
            )
                .into_response()
        }
    }

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
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Mietverhältnis konnte nicht gespeichert werden.",
        )
            .into_response(),
    }
}

pub async fn tenancies_delete(
    Path((building_id, apartment_id, tenancy_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        return err.into_response();
    }
    match queries::get_tenancy(&pool, &tenancy_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response();
            }
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
    }

    let _ = queries::delete_tenancy(&pool, &tenancy_id).await;
    redirect_after_post(
        &headers,
        &format!("/admin/buildings/{building_id}/apartments/{apartment_id}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        overlaps_ownership, overlaps_tenancy, ownership_chain_violation, periods_overlap,
        Ownership, Tenancy,
    };
    use chrono::NaiveDate;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn t(id: &str, start: &str, end: Option<&str>) -> Tenancy {
        Tenancy {
            id: id.into(),
            apartment_id: "apt".into(),
            name: "Tenant".into(),
            email: "t@example.com".into(),
            start_date: start.into(),
            end_date: end.map(str::to_string),
            created_at: String::new(),
        }
    }

    fn o(id: &str, start: &str, end: Option<&str>) -> Ownership {
        Ownership {
            id: id.into(),
            apartment_id: "apt".into(),
            name: "Owner".into(),
            email: "o@example.com".into(),
            start_date: start.into(),
            end_date: end.map(str::to_string),
            created_at: String::new(),
        }
    }

    #[test]
    fn overlap_detection() {
        // Same period
        assert!(periods_overlap(
            d("2024-01-01"),
            Some(d("2024-12-31")),
            d("2024-01-01"),
            Some(d("2024-12-31"))
        ));
        // Nested
        assert!(periods_overlap(
            d("2024-01-01"),
            Some(d("2024-12-31")),
            d("2024-03-01"),
            Some(d("2024-04-01"))
        ));
        // Adjacent but not overlapping (end exclusive at midnight)
        assert!(!periods_overlap(
            d("2024-01-01"),
            Some(d("2024-01-31")),
            d("2024-02-01"),
            Some(d("2024-02-28"))
        ));
        // Open-ended overlaps everything after its start
        assert!(periods_overlap(
            d("2024-01-01"),
            None,
            d("2025-06-01"),
            None
        ));
        // Open-ended vs. closed
        assert!(periods_overlap(
            d("2024-01-01"),
            None,
            d("2024-06-01"),
            Some(d("2024-06-30"))
        ));
        assert!(periods_overlap(
            d("2024-06-01"),
            Some(d("2024-06-30")),
            d("2024-01-01"),
            None
        ));
    }

    #[test]
    fn validation_accepts_valid_inputs() {
        assert!(super::validate_person_input("Bob", "bob@example.com", "2024-01-01", None).is_ok());
        assert!(super::validate_person_input(
            "Bob",
            "bob@example.com",
            "2024-01-01",
            Some("2024-12-31")
        )
        .is_ok());
        assert!(
            super::validate_person_input("Bob", "b.o.b@sub.example.co.uk", "2024-01-01", None)
                .is_ok()
        );
    }

    #[test]
    fn validation_rejects_invalid() {
        assert!(super::validate_person_input("", "bob@example.com", "2024-01-01", None).is_err());
        assert!(super::validate_person_input("Bob", "bobexample.com", "2024-01-01", None).is_err());
        assert!(super::validate_person_input("Bob", "bob@", "2024-01-01", None).is_err());
        assert!(super::validate_person_input("Bob", "bob@example", "2024-01-01", None).is_err());
        assert!(
            super::validate_person_input("Bob", "bob@example.com", "2024-13-01", None).is_err()
        );
        assert!(super::validate_person_input(
            "Bob",
            "bob@example.com",
            "2024-01-02",
            Some("2024-01-01")
        )
        .is_err());
    }

    #[test]
    fn name_length_limit() {
        assert!(super::validate_name("Ok").is_ok());
        assert!(super::validate_name("").is_err());
        assert!(super::validate_name("   ").is_err());
        let thirty = "x".repeat(30);
        assert!(super::validate_name(&thirty).is_ok());
        assert!(super::validate_name(&format!("{thirty}x")).is_err());
    }

    #[test]
    fn tenancy_overlap_detection() {
        // Overlapping periods are detected, including open-ended records.
        assert!(overlaps_tenancy(
            &t("t1", "2024-01-01", Some("2024-06-30")),
            d("2024-06-01"),
            None,
            None
        ));
        assert!(overlaps_tenancy(
            &t("t1", "2024-01-01", None),
            d("2025-01-01"),
            None,
            None
        ));
        // Adjacent (non-overlapping) periods are accepted.
        assert!(!overlaps_tenancy(
            &t("t1", "2024-01-01", Some("2024-06-30")),
            d("2024-07-01"),
            None,
            None
        ));
        // Updating a record does not reject itself (exclude_id), even when
        // another record with the same span would be rejected.
        assert!(!overlaps_tenancy(
            &t("t1", "2024-01-01", Some("2024-06-30")),
            d("2024-01-01"),
            Some(d("2024-12-31")),
            Some("t1")
        ));
        assert!(overlaps_tenancy(
            &t("t1", "2024-01-01", Some("2024-06-30")),
            d("2024-01-01"),
            Some(d("2024-12-31")),
            Some("other")
        ));
    }

    #[test]
    fn ownership_overlap_detection() {
        // Overlapping periods are detected, including open-ended records.
        assert!(overlaps_ownership(
            &o("o1", "2024-01-01", Some("2024-06-30")),
            d("2024-06-01"),
            None,
            None
        ));
        assert!(overlaps_ownership(
            &o("o1", "2024-01-01", None),
            d("2025-01-01"),
            None,
            None
        ));
        // Adjacent (non-overlapping) periods are accepted.
        assert!(!overlaps_ownership(
            &o("o1", "2024-01-01", Some("2024-06-30")),
            d("2024-07-01"),
            None,
            None
        ));
        // Updating a record does not reject itself (exclude_id), even when
        // another record with the same span would be rejected.
        assert!(!overlaps_ownership(
            &o("o1", "2024-01-01", Some("2024-06-30")),
            d("2024-01-01"),
            Some(d("2024-12-31")),
            Some("o1")
        ));
        assert!(overlaps_ownership(
            &o("o1", "2024-01-01", Some("2024-06-30")),
            d("2024-01-01"),
            Some(d("2024-12-31")),
            Some("other")
        ));
    }

    #[test]
    fn ownership_chain_violation_detection() {
        // Chain: r1 covers Jan 1 - Jun 30, r2 is open-ended from July 1.
        let records = [
            o("1", "2026-01-01", Some("2026-06-30")),
            o("2", "2026-07-01", None),
        ];
        // A record with the same span as r2 overlaps it and is rejected.
        assert!(ownership_chain_violation(&records, None, d("2026-07-01"), None).is_some());
        // A gap after the previous period is rejected.
        assert!(ownership_chain_violation(&records, None, d("2026-07-03"), None).is_some());
        // A tiled period before the chain start is fine (becomes the first).
        assert!(
            ownership_chain_violation(&records, None, d("2025-01-01"), Some(d("2025-12-31")))
                .is_none()
        );
        // An open-ended ownership cannot be followed by another one (overlap).
        assert!(ownership_chain_violation(&records, None, d("2026-08-01"), None).is_some());
        // Updating record 2 without breaking the chain is fine.
        assert!(ownership_chain_violation(&records, Some("2"), d("2026-07-01"), None).is_none());
        // Updating record 2 into a gap is rejected.
        assert!(ownership_chain_violation(&records, Some("2"), d("2026-07-02"), None).is_some());

        // With only r1, an adjacent continuation is accepted and an overlap/gap
        // inside the period is not.
        let single = [o("1", "2026-01-01", Some("2026-06-30"))];
        assert!(ownership_chain_violation(&single, None, d("2026-07-01"), None).is_none());
        assert!(
            ownership_chain_violation(&single, None, d("2026-03-01"), Some(d("2026-04-30")))
                .is_some()
        );

        // With a successor, the new period must end right before it starts.
        let records2 = [
            o("1", "2026-01-01", Some("2026-06-30")),
            o("2", "2026-07-01", Some("2026-08-31")),
            o("3", "2026-09-01", None),
        ];
        assert!(
            ownership_chain_violation(&records2, None, d("2025-05-01"), Some(d("2025-12-31")))
                .is_none()
        );
        // A first period ending before its successor leaves a gap.
        assert!(
            ownership_chain_violation(&records2, None, d("2025-05-01"), Some(d("2025-05-31")))
                .is_some()
        );
        // A period overlapping a neighbor is rejected.
        assert!(ownership_chain_violation(&records2, None, d("2026-08-15"), None).is_some());

        // A hole in the middle is exactly where a tiled new period fits.
        let records3 = [
            o("1", "2026-01-01", Some("2026-03-31")),
            o("2", "2026-09-01", None),
        ];
        assert!(
            ownership_chain_violation(&records3, None, d("2026-04-01"), Some(d("2026-08-31")))
                .is_none()
        );
        assert!(
            ownership_chain_violation(&records3, None, d("2026-04-02"), Some(d("2026-08-31")))
                .is_some()
        );
        assert!(
            ownership_chain_violation(&records3, None, d("2026-04-01"), Some(d("2026-08-30")))
                .is_some()
        );
    }
}
