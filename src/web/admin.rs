use crate::scheduler::{self, WeekAssignment};
use askama::Template;
use askama_axum::IntoResponse;
use axum::extract::{Path, State};
use axum::response::IntoResponse as AxumIntoResponse;
use axum::response::Redirect;
use axum::Form;
use chrono::{Datelike, Local};

use crate::db::models::{Plan, PlanAdministrator, Tenant};
use crate::db::queries;
use crate::db::Db;

#[derive(Template)]
#[template(path = "admin/dashboard.html")]
pub struct DashboardTemplate<'a> {
    pub title: &'a str,
}

pub async fn dashboard() -> impl axum::response::IntoResponse {
    DashboardTemplate {
        title: "Admin Dashboard",
    }
    .into_response()
}

#[derive(Template)]
#[template(path = "admin/plans/index.html")]
pub struct PlansIndexTemplate {
    pub title: &'static str,
    pub plans: Vec<Plan>,
}

#[derive(Template)]
#[template(path = "admin/plans/new.html")]
pub struct PlansNewTemplate {
    pub title: &'static str,
}

#[derive(Template)]
#[template(path = "admin/plans/show.html")]
pub struct PlansShowTemplate {
    pub title: String,
    pub plan: Plan,
    pub admins: Vec<PlanAdministrator>,
}

#[derive(Template)]
#[template(path = "admin/tenants/index.html")]
pub struct TenantsIndexTemplate {
    pub title: String,
    pub plan_id: String,
    pub tenants: Vec<Tenant>,
}

#[derive(Template)]
#[template(path = "admin/tenants/new.html")]
pub struct TenantsNewTemplate {
    pub title: String,
    pub plan_id: String,
}

#[derive(Template)]
#[template(path = "admin/tenants/edit.html")]
pub struct TenantsEditTemplate {
    pub title: String,
    pub plan_id: String,
    pub tenant: Tenant,
}

#[derive(Template)]
#[template(path = "admin/plans/schedule.html")]
pub struct PlansScheduleTemplate {
    pub title: String,
    pub plan: Plan,
    pub year: i32,
    pub schedule: Vec<WeekAssignment>,
}

#[derive(serde::Deserialize)]
pub struct CreatePlanForm {
    pub name: String,
    pub admin_name: String,
    pub admin_email: String,
}

pub async fn plans_index(State(pool): State<Db>) -> impl axum::response::IntoResponse {
    match queries::list_plans(&pool).await {
        Ok(plans) => PlansIndexTemplate {
            title: "Plans",
            plans,
        }
        .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load plans",
        )
            .into_response(),
    }
}

pub async fn plans_new() -> impl axum::response::IntoResponse {
    PlansNewTemplate { title: "New Plan" }.into_response()
}

pub async fn plans_create(
    State(pool): State<Db>,
    Form(form): Form<CreatePlanForm>,
) -> impl axum::response::IntoResponse {
    match queries::create_plan(&pool, &form.name, &form.admin_name, &form.admin_email).await {
        Ok(plan) => Redirect::to(&format!("/admin/plans/{}", plan.id)).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create plan",
        )
            .into_response(),
    }
}

pub async fn plans_show(
    Path(id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    match queries::get_plan(&pool, &id).await {
        Ok(Some((plan, admins))) => PlansShowTemplate {
            title: format!("Plan: {}", plan.name),
            plan,
            admins,
        }
        .into_response(),
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load plan",
        )
            .into_response(),
    }
}

pub async fn plans_schedule(
    Path(id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    match queries::get_plan(&pool, &id).await {
        Ok(Some((plan, _admins))) => {
            let year = Local::now().date_naive().year();
            match scheduler::schedule_for_year(&plan.id, year, &pool).await {
                Ok(schedule) => PlansScheduleTemplate {
                    title: format!("Schedule Preview: {} ({})", plan.name, year),
                    plan,
                    year,
                    schedule,
                }
                .into_response(),
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
            "Failed to load plan",
        )
            .into_response(),
    }
}

pub async fn plans_delete(
    Path(id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    let _ = queries::delete_plan(&pool, &id).await;
    Redirect::to("/admin/plans").into_response()
}

#[derive(serde::Deserialize)]
pub struct CreateTenantForm {
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct UpdateTenantForm {
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
}

fn validate_tenant_input(
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Name must not be empty".into());
    }
    let email = email.trim();
    if let Some(at_pos) = email.find('@') {
        if !email[at_pos + 1..].contains('.') {
            return Err("Email must contain a dot after '@'".into());
        }
    } else {
        return Err("Email must contain '@'".into());
    }

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

pub async fn tenants_index(
    Path(plan_id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    match queries::list_tenants(&pool, &plan_id).await {
        Ok(tenants) => TenantsIndexTemplate {
            title: "Tenants".to_string(),
            plan_id,
            tenants,
        }
        .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load tenants",
        )
            .into_response(),
    }
}

pub async fn tenants_new(Path(plan_id): Path<String>) -> impl axum::response::IntoResponse {
    TenantsNewTemplate {
        title: "New Tenant".to_string(),
        plan_id,
    }
    .into_response()
}

pub async fn tenants_create(
    Path(plan_id): Path<String>,
    State(pool): State<Db>,
    Form(form): Form<CreateTenantForm>,
) -> impl axum::response::IntoResponse {
    let end_opt = form.end_date.as_deref().and_then(|s| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });
    if let Err(msg) = validate_tenant_input(&form.name, &form.email, &form.start_date, end_opt) {
        return (axum::http::StatusCode::BAD_REQUEST, msg).into_response();
    }

    match queries::create_tenant(
        &pool,
        &plan_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await
    {
        Ok(_) => Redirect::to(&format!("/admin/plans/{}/tenants", plan_id)).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create tenant",
        )
            .into_response(),
    }
}

pub async fn tenants_edit(
    Path((plan_id, tenant_id)): Path<(String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    match queries::get_tenant(&pool, &tenant_id).await {
        Ok(Some(tenant)) => {
            if tenant.plan_id != plan_id {
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
            TenantsEditTemplate {
                title: format!("Edit Tenant {}", tenant.name),
                plan_id,
                tenant,
            }
            .into_response()
        }
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load tenant",
        )
            .into_response(),
    }
}

pub async fn tenants_update(
    Path((plan_id, tenant_id)): Path<(String, String)>,
    State(pool): State<Db>,
    Form(form): Form<UpdateTenantForm>,
) -> impl axum::response::IntoResponse {
    // Ensure tenant belongs to plan
    match queries::get_tenant(&pool, &tenant_id).await {
        Ok(Some(existing)) => {
            if existing.plan_id != plan_id {
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }

    let end_opt = form.end_date.as_deref().and_then(|s| {
        let s = s.trim();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });
    if let Err(msg) = validate_tenant_input(&form.name, &form.email, &form.start_date, end_opt) {
        return (axum::http::StatusCode::BAD_REQUEST, msg).into_response();
    }

    match queries::update_tenant(
        &pool,
        &tenant_id,
        &form.name,
        &form.email,
        &form.start_date,
        end_opt,
    )
    .await
    {
        Ok(_) => Redirect::to(&format!("/admin/plans/{}/tenants", plan_id)).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to update tenant",
        )
            .into_response(),
    }
}

pub async fn tenants_delete(
    Path((plan_id, tenant_id)): Path<(String, String)>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    // Ensure tenant belongs to plan
    match queries::get_tenant(&pool, &tenant_id).await {
        Ok(Some(existing)) => {
            if existing.plan_id != plan_id {
                return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response();
            }
        }
        _ => return (axum::http::StatusCode::NOT_FOUND, "Not found").into_response(),
    }

    let _ = queries::delete_tenant(&pool, &tenant_id).await;
    Redirect::to(&format!("/admin/plans/{}/tenants", plan_id)).into_response()
}

#[cfg(test)]
mod tests {
    use super::validate_tenant_input;

    #[test]
    fn validation_accepts_valid_inputs() {
        assert!(validate_tenant_input("Bob", "bob@example.com", "2024-01-01", None).is_ok());
        assert!(
            validate_tenant_input("Bob", "bob@example.com", "2024-01-01", Some("2024-12-31"))
                .is_ok()
        );
        assert!(
            validate_tenant_input("Bob", "b.o.b@sub.example.co.uk", "2024-01-01", None).is_ok()
        );
    }

    #[test]
    fn validation_rejects_invalid() {
        assert!(validate_tenant_input("", "bob@example.com", "2024-01-01", None).is_err());
        assert!(validate_tenant_input("Bob", "bobexample.com", "2024-01-01", None).is_err());
        assert!(validate_tenant_input("Bob", "bob@", "2024-01-01", None).is_err());
        assert!(validate_tenant_input("Bob", "bob@example", "2024-01-01", None).is_err());
        assert!(validate_tenant_input("Bob", "bob@example.com", "2024-13-01", None).is_err());
        assert!(
            validate_tenant_input("Bob", "bob@example.com", "2024-01-02", Some("2024-01-01"))
                .is_err()
        );
    }
}
