use askama::Template;
use askama_axum::IntoResponse;
use axum::response::IntoResponse as AxumIntoResponse;
use axum::extract::{Path, State};
use axum::response::Redirect;
use axum::Form;

use crate::db::models::{Plan, PlanAdministrator};
use crate::db::queries;
use crate::db::Db;

#[derive(Template)]
#[template(path = "admin/dashboard.html")]
pub struct DashboardTemplate<'a> {
    pub title: &'a str,
}

pub async fn dashboard() -> impl axum::response::IntoResponse {
    DashboardTemplate { title: "Admin Dashboard" }.into_response()
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

#[derive(serde::Deserialize)]
pub struct CreatePlanForm {
    pub name: String,
    pub admin_name: String,
    pub admin_email: String,
}

pub async fn plans_index(State(pool): State<Db>) -> impl axum::response::IntoResponse {
    match queries::list_plans(&pool).await {
        Ok(plans) => PlansIndexTemplate { title: "Plans", plans }.into_response(),
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

pub async fn plans_delete(
    Path(id): Path<String>,
    State(pool): State<Db>,
) -> impl axum::response::IntoResponse {
    let _ = queries::delete_plan(&pool, &id).await;
    Redirect::to("/admin/plans").into_response()
}
