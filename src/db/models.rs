#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Building {
    pub id: String,
    pub name: String,
    pub description: String,
    pub secret_slug: String,
    pub rotation_seed: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct BuildingAdministrator {
    pub id: String,
    pub building_id: String,
    pub name: String,
    pub email: String,
    pub created_at: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Apartment {
    pub id: String,
    pub building_id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Ownership {
    pub id: String,
    pub apartment_id: String,
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Tenancy {
    pub id: String,
    pub apartment_id: String,
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
}
