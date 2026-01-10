use askama::Template;
use askama_axum::IntoResponse;

#[derive(Template)]
#[template(path = "admin/dashboard.html")]
pub struct DashboardTemplate<'a> {
    pub title: &'a str,
}

pub async fn dashboard() -> impl axum::response::IntoResponse {
    DashboardTemplate { title: "Admin Dashboard" }.into_response()
}
