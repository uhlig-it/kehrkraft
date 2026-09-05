use crate::db::models::{Apartment, Building, BuildingAdministrator, Ownership, Tenancy};
use crate::db::queries;
use crate::db::Db;
use crate::scheduler::{self, WeekAssignment};
use askama::Template;
use axum::extract::{Path, RawForm, State};
use axum::response::IntoResponse as _;
use axum::response::Redirect;
use axum::Form;
use chrono::{Datelike, Local, NaiveDate};

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

#[derive(Template)]
#[template(path = "admin/buildings/show.html")]
pub struct BuildingsShowTemplate {
    pub title: String,
    pub building: Building,
    pub admins: Vec<BuildingAdministrator>,
    pub apartments: Vec<Apartment>,
    pub year: i32,
    pub schedule: Vec<WeekAssignment>,
}

#[derive(Template)]
#[template(path = "admin/buildings/schedule.html")]
pub struct BuildingsScheduleTemplate {
    pub title: String,
    pub building: Building,
    pub year: i32,
    pub schedule: Vec<WeekAssignment>,
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
    pub apartments: Vec<Apartment>,
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
        return Err("Name must not be empty".into());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(format!("Name must not exceed {MAX_NAME_LEN} characters"));
    }
    Ok(())
}

fn validate_email(email: &str) -> Result<(), String> {
    let email = email.trim();
    if let Some(at_pos) = email.find('@') {
        if !email[at_pos + 1..].contains('.') {
            return Err("Email must contain a dot after '@'".into());
        }
    } else {
        return Err("Email must contain '@'".into());
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
        return Err("Name must not be empty".into());
    }
    validate_email(email)?;

    let start = chrono::NaiveDate::parse_from_str(start_date, "%Y-%m-%d")
        .map_err(|_| "start_date must be YYYY-MM-DD".to_string())?;
    if let Some(ed) = end_date {
        if !ed.trim().is_empty() {
            let end = chrono::NaiveDate::parse_from_str(ed, "%Y-%m-%d")
                .map_err(|_| "end_date must be YYYY-MM-DD".to_string())?;
            if start > end {
                return Err("start_date must be before or equal to end_date".into());
            }
        }
    }
    Ok(())
}

pub async fn buildings_index(State(pool): State<Db>) -> impl axum::response::IntoResponse {
    match queries::list_buildings(&pool).await {
        Ok(buildings) => render(BuildingsIndexTemplate {
            title: "Buildings",
            buildings,
        }),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load buildings",
        )
            .into_response(),
    }
}

pub async fn buildings_new() -> impl axum::response::IntoResponse {
    render(BuildingsNewTemplate {
        title: "New Building",
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
                .then(|| "Administrator name must not be empty".to_string())
        })
        .or_else(|| validate_email(&form.admin_email).err());
    if let Some(msg) = error {
        return render_bad_request(BuildingsNewTemplate {
            title: "New Building",
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
            "Failed to create building",
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
                Ok(apartments) => apartments,
                Err(_) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to load apartments",
                    )
                        .into_response()
                }
            };
            // Compact schedule: only the remaining weeks of the current year.
            let year = Local::now().date_naive().year();
            let schedule: Vec<WeekAssignment> =
                match scheduler::schedule_for_year(&building.id, year, &pool).await {
                    Ok(weeks) => {
                        let today = Local::now().date_naive();
                        weeks
                            .into_iter()
                            .filter(|w| w.end >= today)
                            .take(12)
                            .collect()
                    }
                    Err(_) => {
                        return (
                            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to compute schedule",
                        )
                            .into_response()
                    }
                };
            render(BuildingsShowTemplate {
                title: format!("Building: {}", building.name),
                building,
                admins,
                apartments,
                year,
                schedule,
            })
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load building",
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
            match scheduler::schedule_for_year(&building.id, year, &pool).await {
                Ok(schedule) => render(BuildingsScheduleTemplate {
                    title: format!("Schedule Preview: {} ({})", building.name, year),
                    building,
                    year,
                    schedule,
                }),
                Err(_) => (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to compute schedule",
                )
                    .into_response(),
            }
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load building",
        )
            .into_response(),
    }
}

pub async fn buildings_delete(
    Path(id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    let _ = queries::delete_building(&pool, &id).await;
    Redirect::to("/admin").into_response()
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
        Ok(None) => Err((axum::http::StatusCode::NOT_FOUND, "Not found")),
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load building",
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
                return Err((axum::http::StatusCode::NOT_FOUND, "Not found"));
            }
            Ok(apartment)
        }
        Ok(None) => Err((axum::http::StatusCode::NOT_FOUND, "Not found")),
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load apartment",
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
            title: format!("Apartment: {}", apartment.name),
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
            "Failed to load apartment details",
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
        title: format!("New Apartment: {}", building.name),
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
            title: format!("New Apartment: {}", building.name),
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
            "Failed to create apartment",
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
        title: format!("Edit Apartment: {}", apartment.name),
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
            title: format!("Apartment: {}", apartment.name),
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
            "Failed to load apartment details",
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
            title: format!("Edit Apartment: {}", apartment.name),
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
            "Failed to update apartment",
        )
            .into_response(),
    }
}

pub async fn apartments_delete(
    Path((building_id, apartment_id)): Path<(String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        return err.into_response();
    }
    let _ = queries::delete_apartment(&pool, &apartment_id).await;
    Redirect::to(&format!("/admin/buildings/{building_id}")).into_response()
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
                "Failed to load apartments",
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
            "Invalid apartment order",
        )
            .into_response();
    }

    if queries::reorder_apartments(&pool, &building_id, &items)
        .await
        .is_err()
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to reorder apartments",
        )
            .into_response();
    }

    // Re-render just the table body so htmx can swap in the new order.
    let building = match load_building(&pool, &building_id).await {
        Ok(b) => b,
        Err(err) => return err.into_response(),
    };
    let apartments = match queries::list_apartments(&pool, &building_id).await {
        Ok(list) => list,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to reload apartments",
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

    // Enforce: at most one active ownership per apartment at any point in time
    match queries::list_ownerships(&pool, &apartment_id).await {
        Ok(existing) => {
            if existing
                .iter()
                .any(|o| overlaps_ownership(o, start, end, None))
            {
                return render_apartment_show_error(
                    &pool,
                    &building_id,
                    &apartment_id,
                    "Ownership overlaps an existing ownership of this apartment".to_string(),
                    Some(form),
                    None,
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load ownerships",
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
            "Failed to create ownership",
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
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
            let form = OwnershipForm {
                name: ownership.name.clone(),
                email: ownership.email.clone(),
                start_date: ownership.start_date.clone(),
                end_date: ownership.end_date.clone(),
            };
            render(OwnershipsEditTemplate {
                title: format!("Edit Owner {}", ownership.name),
                building,
                apartment,
                ownership,
                error: None,
                form,
            })
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load ownership",
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
        title: format!("Edit Owner {}", ownership.name),
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
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
            existing
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    };

    let end_opt = normalize_end_date(form.end_date.as_deref());
    if let Err(msg) = validate_person_input(&form.name, &form.email, &form.start_date, end_opt) {
        return render_ownership_edit_error(&pool, &building_id, &apartment, &ownership, msg, form)
            .await;
    }

    let start = NaiveDate::parse_from_str(&form.start_date, "%Y-%m-%d").expect("validated date");
    let end = end_opt.and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

    // Enforce: at most one active ownership per apartment at any point in time
    match queries::list_ownerships(&pool, &apartment_id).await {
        Ok(existing) => {
            if existing
                .iter()
                .any(|o| overlaps_ownership(o, start, end, Some(&ownership_id)))
            {
                return render_ownership_edit_error(
                    &pool,
                    &building_id,
                    &apartment,
                    &ownership,
                    "Ownership overlaps an existing ownership of this apartment".to_string(),
                    form,
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load ownerships",
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
            "Failed to update ownership",
        )
            .into_response(),
    }
}

pub async fn ownerships_delete(
    Path((building_id, apartment_id, ownership_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        return err.into_response();
    }
    match queries::get_ownership(&pool, &ownership_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }

    let _ = queries::delete_ownership(&pool, &ownership_id).await;
    Redirect::to(&format!(
        "/admin/buildings/{building_id}/apartments/{apartment_id}"
    ))
    .into_response()
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
                    "Tenancy overlaps an existing tenancy of this apartment".to_string(),
                    None,
                    Some(form),
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load tenancies",
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
            "Failed to create tenancy",
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
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
            let form = TenancyForm {
                name: tenancy.name.clone(),
                email: tenancy.email.clone(),
                start_date: tenancy.start_date.clone(),
                end_date: tenancy.end_date.clone(),
            };
            render(TenanciesEditTemplate {
                title: format!("Edit Tenant {}", tenancy.name),
                building,
                apartment,
                tenancy,
                error: None,
                form,
            })
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load tenancy",
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
        title: format!("Edit Tenant {}", tenancy.name),
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
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
            existing
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
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
                    "Tenancy overlaps an existing tenancy of this apartment".to_string(),
                    form,
                )
                .await;
            }
        }
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load tenancies",
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
            "Failed to update tenancy",
        )
            .into_response(),
    }
}

pub async fn tenancies_delete(
    Path((building_id, apartment_id, tenancy_id)): Path<(String, String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    if let Err(err) = load_apartment_owned_by(&pool, &building_id, &apartment_id).await {
        return err.into_response();
    }
    match queries::get_tenancy(&pool, &tenancy_id).await {
        Ok(Some(existing)) => {
            if existing.apartment_id != apartment_id {
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }

    let _ = queries::delete_tenancy(&pool, &tenancy_id).await;
    Redirect::to(&format!(
        "/admin/buildings/{building_id}/apartments/{apartment_id}"
    ))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::{overlaps_ownership, overlaps_tenancy, periods_overlap, Ownership, Tenancy};
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
}
