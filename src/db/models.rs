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
    /// The person behind the Ansprechpartner (joined `people` row).
    pub person_id: String,
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
    /// Manual display order within the building; 0-based, gaps allowed.
    pub position: i64,
    pub created_at: String,
}

/// Master data of an owner: contact data shared by all ownership periods and
/// building-owner periods of this person (see 0007_people.sql). Identified by
/// its e-mail address (case-insensitive); `name`/`email` are current contact
/// data, not period snapshots.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub email: String,
    pub created_at: String,
}

/// A building-owner period; `name`/`email` come from the joined `people` row.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct BuildingOwner {
    pub id: String,
    pub building_id: String,
    pub person_id: String,
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
}

/// An apartment ownership period; `name`/`email` come from the joined
/// `people` row.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Ownership {
    pub id: String,
    pub apartment_id: String,
    pub person_id: String,
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
}

/// A tenancy; `name`/`email` come from the joined `people` row.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Tenancy {
    pub id: String,
    pub apartment_id: String,
    pub person_id: String,
    pub name: String,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
}

/// One row of the "person roles" listing: a person in one role (admin,
/// building owner, apartment owner, tenant) with the object it refers to.
/// Produced by the UNION query in `queries::list_person_roles`; `kind` is one
/// of `admin`, `building_owner`, `apartment_owner`, `tenant`.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct PersonRoleRow {
    pub person_id: String,
    pub person_name: String,
    pub person_email: String,
    pub kind: String,
    pub building_id: Option<String>,
    pub building_name: Option<String>,
    pub apartment_id: Option<String>,
    pub apartment_name: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}
