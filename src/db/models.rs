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
    /// Optional display name of the person (joined `people` row); see
    /// [`person_effective_name`].
    pub display_name: Option<String>,
    pub email: String,
    pub created_at: String,
}

impl BuildingAdministrator {
    /// The name shown in contact displays: the display name when set,
    /// the person's name otherwise.
    pub fn effective_name(&self) -> String {
        person_effective_name(&self.name, self.display_name.as_deref())
    }
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

/// Master data of a person: contact data shared by all ownership periods,
/// building-owner periods, tenancies and administrator rows of this person
/// (see 0007_people.sql). Identified by its e-mail address (case-insensitive);
/// `name`/`email`/`display_name` are current contact data, not period
/// snapshots.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Person {
    pub id: String,
    pub name: String,
    /// Optional display name: when set, the UI shows it instead of `name`
    /// wherever the person is displayed (see [`person_effective_name`]).
    pub display_name: Option<String>,
    pub email: String,
    pub created_at: String,
}

impl Person {
    /// The name shown in displays: the display name when set, the person's
    /// name otherwise.
    pub fn effective_name(&self) -> String {
        person_effective_name(&self.name, self.display_name.as_deref())
    }
}

/// The name shown for a person: the display name when set (non-empty after
/// trimming), falling back to the person's name. Used everywhere a person is
/// displayed as an owner, tenant or contact.
pub fn person_effective_name(name: &str, display_name: Option<&str>) -> String {
    match display_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(display) => display.to_string(),
        None => name.to_string(),
    }
}

/// Join effective names the way owners/contacts are printed: "A",
/// "A & B", "A, B & C".
pub fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [single] => single.clone(),
        _ => {
            let (last, rest) = names.split_last().expect("non-empty names");
            format!("{} & {last}", rest.join(", "))
        }
    }
}

/// One person of a (possibly multi-person) ownership period, joined from
/// `people` via the `ownership_people`/`building_owner_people` join tables
/// (migration 0013). `position` is the display order within the period.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct PersonRef {
    pub person_id: String,
    pub name: String,
    /// Optional display name of the person; see [`person_effective_name`].
    pub display_name: Option<String>,
    pub email: String,
    pub position: i64,
}

impl PersonRef {
    /// The name shown in owner displays: the display name when set, the
    /// person's name otherwise.
    pub fn effective_name(&self) -> String {
        person_effective_name(&self.name, self.display_name.as_deref())
    }
}

/// A building-owner period; `people` is the (possibly multiple) group of
/// owners of this period, joined from `building_owner_people` + `people`.
#[derive(Debug, Clone)]
pub struct BuildingOwner {
    pub id: String,
    pub building_id: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
    pub people: Vec<PersonRef>,
}

impl BuildingOwner {
    /// The owners' names joined for display ("A & B", "A, B & C"); each name
    /// is the effective name (display name when set).
    pub fn label(&self) -> String {
        join_names(&self.people_names())
    }

    /// The effective names of the period's owners, in display order.
    pub fn people_names(&self) -> Vec<String> {
        self.people.iter().map(PersonRef::effective_name).collect()
    }
}

/// An apartment ownership period; `people` is the (possibly multiple) group
/// of owners of this period, joined from `ownership_people` + `people`.
#[derive(Debug, Clone)]
pub struct Ownership {
    pub id: String,
    pub apartment_id: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
    pub people: Vec<PersonRef>,
}

impl Ownership {
    /// The owners' names joined for display ("A & B", "A, B & C"); each name
    /// is the effective name (display name when set).
    pub fn label(&self) -> String {
        join_names(&self.people_names())
    }

    /// The effective names of the period's owners, in display order.
    pub fn people_names(&self) -> Vec<String> {
        self.people.iter().map(PersonRef::effective_name).collect()
    }
}

/// A tenancy; `name`/`email`/`display_name` come from the joined `people` row.
#[derive(sqlx::FromRow, Debug, Clone)]
pub struct Tenancy {
    pub id: String,
    pub apartment_id: String,
    pub person_id: String,
    pub name: String,
    /// Optional display name of the person; see [`person_effective_name`].
    pub display_name: Option<String>,
    pub email: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub created_at: String,
}

impl Tenancy {
    /// The name shown in tenant displays: the display name when set, the
    /// person's name otherwise.
    pub fn effective_name(&self) -> String {
        person_effective_name(&self.name, self.display_name.as_deref())
    }
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
