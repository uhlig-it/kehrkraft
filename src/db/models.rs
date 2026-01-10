#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub secret_slug: String,
    pub rotation_seed: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct PlanAdministrator {
    pub id: String,
    pub plan_id: String,
    pub name: String,
    pub email: String,
    pub created_at: String,
}
