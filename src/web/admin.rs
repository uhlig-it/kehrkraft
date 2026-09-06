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
    pub title: &'static str,
    pub building: Building,
    pub error: Option<String>,
    pub name: String,
    pub description: String,
    /// Initial owner of the apartment, collected in the same form because an
    /// apartment must always have at least one ownership record.
    pub owner_name: String,
    pub owner_email: String,
    pub owner_start_date: String,
    pub owner_end_date: Option<String>,
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

/// Extract a database-level error message so it can be shown inline on the
/// form that caused it. All validation rules live in the database (triggers
/// that `RAISE(ABORT, …)` with a German message, see 0004_validation_in_db.sql);
/// `None` means a transport/connection-level failure, not a rejection.
fn db_message(err: &sqlx::Error) -> Option<String> {
    err.as_database_error().map(|e| e.message().to_owned())
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
        Err(err) => match db_message(&err) {
            Some(msg) => render_bad_request(BuildingsNewTemplate {
                title: "Neues Gebäude anlegen",
                error: Some(msg),
                name: form.name,
                description: form.description,
                admin_name: form.admin_name,
                admin_email: form.admin_email,
            }),
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Gebäude konnte nicht angelegt werden.",
            )
                .into_response(),
        },
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
    /// Initial owner of the apartment; the database requires an ownership
    /// record to exist from the moment the apartment is created.
    pub owner_name: String,
    pub owner_email: String,
    pub owner_start_date: String,
    pub owner_end_date: Option<String>,
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
        title: "Neue Wohnung",
        building,
        error: None,
        name: String::new(),
        description: String::new(),
        owner_name: String::new(),
        owner_email: String::new(),
        owner_start_date: String::new(),
        owner_end_date: None,
    })
}

pub async fn apartments_create(
    Path(building_id): Path<String>,
    State(pool): State<Db>,
    Form(form): Form<ApartmentForm>,
) -> impl axum::response::IntoResponse {
    let end_opt = normalize_end_date(form.owner_end_date.as_deref());
    match queries::create_apartment(
        &pool,
        &building_id,
        &form.name,
        &form.description,
        &queries::NewOwner {
            name: &form.owner_name,
            email: &form.owner_email,
            start_date: &form.owner_start_date,
            end_date: end_opt,
        },
    )
    .await
    {
        Ok(apartment) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{}",
            apartment.id
        ))
        .into_response(),
        Err(err) => match db_message(&err) {
            Some(msg) => {
                let building = match load_building(&pool, &building_id).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(ApartmentsNewTemplate {
                    title: "Neue Wohnung",
                    building,
                    error: Some(msg),
                    name: form.name,
                    description: form.description,
                    owner_name: form.owner_name,
                    owner_email: form.owner_email,
                    owner_start_date: form.owner_start_date,
                    owner_end_date: form.owner_end_date,
                })
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Wohnung konnte nicht angelegt werden.",
            )
                .into_response(),
        },
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
    match queries::update_apartment(&pool, &apartment_id, &form.name, &form.description).await {
        Ok(_) => Redirect::to(&format!(
            "/admin/buildings/{building_id}/apartments/{apartment_id}"
        ))
        .into_response(),
        Err(err) => match db_message(&err) {
            Some(msg) => {
                let building = match load_building(&pool, &building_id).await {
                    Ok(b) => b,
                    Err(err) => return err.into_response(),
                };
                render_bad_request(ApartmentsEditTemplate {
                    title: "Wohnung bearbeiten".to_string(),
                    building,
                    apartment,
                    error: Some(msg),
                    name: form.name,
                    description: form.description,
                })
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Wohnung konnte nicht gespeichert werden.",
            )
                .into_response(),
        },
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

    // The submitted ids must be exactly this building's apartments — no
    // additions, omissions, or duplicates. The db layer validates this inside
    // the same transaction that persists the new order.
    match queries::reorder_apartments(&pool, &building_id, &items).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Ungültige Reihenfolge der Wohnungen",
            )
                .into_response()
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Die Reihenfolge konnte nicht gespeichert werden.",
            )
                .into_response()
        }
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

    // Field checks, chain tiling, and tenancy overlaps are enforced by the
    // database (triggers); its rejection message is shown inline.
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
        Err(err) => match db_message(&err) {
            Some(msg) => {
                render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    msg,
                    Some(form),
                    None,
                )
                .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Eigentum konnte nicht angelegt werden.",
            )
                .into_response(),
        },
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
        Err(err) => match db_message(&err) {
            Some(msg) => {
                render_ownership_edit_error(&pool, &building_id, &apartment, &ownership, msg, form)
                    .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Eigentum konnte nicht gespeichert werden.",
            )
                .into_response(),
        },
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
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Nicht gefunden").into_response(),
    }

    // The database rejects deleting the apartment's last ownership or a period
    // in the middle of the ownership chain (see `ownerships_guard_delete`).
    match queries::delete_ownership(&pool, &ownership_id).await {
        Ok(_) => redirect_after_post(
            &headers,
            &format!("/admin/buildings/{building_id}/apartments/{apartment_id}"),
        ),
        Err(err) => match db_message(&err) {
            Some(msg) => {
                render_apartment_show_error(&pool, &building_id, &apartment_id, msg, None, None)
                    .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Eigentum konnte nicht gelöscht werden.",
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

    // Field checks and the "at most one active tenancy" rule are enforced by
    // the database (triggers); its rejection message is shown inline.
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
        Err(err) => match db_message(&err) {
            Some(msg) => {
                render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    msg,
                    None,
                    Some(form),
                )
                .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Mietverhältnis konnte nicht angelegt werden.",
            )
                .into_response(),
        },
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
        Err(err) => match db_message(&err) {
            Some(msg) => {
                render_tenancy_edit_error(&pool, &building_id, &apartment, &tenancy, msg, form)
                    .await
            }
            None => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Mietverhältnis konnte nicht gespeichert werden.",
            )
                .into_response(),
        },
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
