use base64::Engine as _;
use rand::RngCore;

use crate::db::models::{
    Apartment, Building, BuildingAdministrator, BuildingOwner, Ownership, Person, PersonRoleRow,
    Tenancy,
};
use crate::db::Db;

fn gen_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

// People (owner master data)

/// All people, for the suggestion lists of the owner forms. Sorted by name so
/// the suggestions read naturally.
pub async fn list_people(pool: &Db) -> Result<Vec<Person>, sqlx::Error> {
    sqlx::query_as::<_, Person>(
        r#"
        SELECT id, name, email, created_at
        FROM people
        ORDER BY lower(name), id
        "#,
    )
    .fetch_all(pool)
    .await
}

/// The person behind an owner entry (see 0007_people.sql): created when no
/// one with this e-mail exists yet, otherwise reused — the e-mail address is
/// the identity, compared case-insensitively after trimming. Name and e-mail
/// are current contact data stored trimmed, so a differing name on an
/// existing person updates that person instead of creating a duplicate.
///
/// Runs inside the caller's transaction: when the surrounding write is
/// rejected by the database (chain tiling, date format, …), the person
/// creation rolls back with it and leaves no orphan behind.
async fn resolve_person(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    name: &str,
    email: &str,
) -> Result<String, sqlx::Error> {
    let name = name.trim();
    let email = email.trim();
    let existing: Option<String> =
        sqlx::query_scalar("SELECT id FROM people WHERE lower(trim(email)) = lower(?)")
            .bind(email)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(id) = existing {
        sqlx::query("UPDATE people SET name = ? WHERE id = ? AND name != ?")
            .bind(name)
            .bind(&id)
            .bind(name)
            .execute(&mut **tx)
            .await?;
        return Ok(id);
    }
    let id = gen_token();
    sqlx::query("INSERT INTO people (id, name, email) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(email)
        .execute(&mut **tx)
        .await?;
    Ok(id)
}

// Buildings CRUD

pub async fn list_buildings(pool: &Db) -> Result<Vec<Building>, sqlx::Error> {
    sqlx::query_as::<_, Building>(
        r#"
        SELECT id, name, description, secret_slug, rotation_seed, created_at, updated_at
        FROM buildings
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
}

pub async fn get_building(
    pool: &Db,
    id: &str,
) -> Result<Option<(Building, Vec<BuildingAdministrator>)>, sqlx::Error> {
    let building_opt = sqlx::query_as::<_, Building>(
        r#"
        SELECT id, name, description, secret_slug, rotation_seed, created_at, updated_at
        FROM buildings
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    if let Some(building) = building_opt {
        let admins = sqlx::query_as::<_, BuildingAdministrator>(
            r#"
            SELECT a.id, a.building_id, a.person_id, p.name, p.email, a.created_at
            FROM building_administrators a
            INNER JOIN people p ON p.id = a.person_id
            WHERE a.building_id = ?
            ORDER BY a.created_at ASC
            "#,
        )
        .bind(&building.id)
        .fetch_all(pool)
        .await?;
        Ok(Some((building, admins)))
    } else {
        Ok(None)
    }
}

pub async fn get_building_by_slug(pool: &Db, slug: &str) -> Result<Option<Building>, sqlx::Error> {
    sqlx::query_as::<_, Building>(
        r#"
        SELECT id, name, description, secret_slug, rotation_seed, created_at, updated_at
        FROM buildings
        WHERE secret_slug = ?
        "#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
}

/// Create a building together with its first Ansprechpartner. The
/// Ansprechpartner is optional: when both `admin_name` and `admin_email` are
/// blank, the building is created without one (a partially filled contact is
/// still inserted and rejected by the database triggers).
pub async fn create_building(
    pool: &Db,
    name: &str,
    description: &str,
    admin_name: &str,
    admin_email: &str,
) -> Result<Building, sqlx::Error> {
    create_building_impl(pool, name, description, admin_name, admin_email, None).await
}

/// Like [`create_building`], but additionally records an initial owner for
/// the whole building (one entity owns all apartments, see 0005). Passed
/// through [`NewOwner`], so the same fields as an apartment's first owner
/// apply; the person is created or reused inside the same transaction.
pub async fn create_building_with_owner(
    pool: &Db,
    name: &str,
    description: &str,
    admin_name: &str,
    admin_email: &str,
    initial_owner: &NewOwner<'_>,
) -> Result<Building, sqlx::Error> {
    create_building_impl(
        pool,
        name,
        description,
        admin_name,
        admin_email,
        Some(initial_owner),
    )
    .await
}

async fn create_building_impl(
    pool: &Db,
    name: &str,
    description: &str,
    admin_name: &str,
    admin_email: &str,
    initial_owner: Option<&NewOwner<'_>>,
) -> Result<Building, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let building_id = gen_token();
    let secret_slug = gen_token();

    sqlx::query(
        r#"
        INSERT INTO buildings (id, name, description, secret_slug, rotation_seed)
        VALUES (?, ?, ?, ?, 0)
        "#,
    )
    .bind(&building_id)
    .bind(name)
    .bind(description)
    .bind(&secret_slug)
    .execute(&mut *tx)
    .await?;

    if !admin_name.trim().is_empty() || !admin_email.trim().is_empty() {
        let admin_id = gen_token();
        // The person comes first (immediate FK); the Ansprechpartner row then
        // references it like every other contact.
        let person_id = resolve_person(&mut tx, admin_name, admin_email).await?;
        sqlx::query(
            r#"
            INSERT INTO building_administrators (id, building_id, person_id)
            VALUES (?, ?, ?)
            "#,
        )
        .bind(&admin_id)
        .bind(&building_id)
        .bind(person_id)
        .execute(&mut *tx)
        .await?;
    }

    // The person comes first (immediate FK); the building-owner period may
    // precede its building because that FK is DEFERRABLE INITIALLY DEFERRED.
    if let Some(owner) = initial_owner {
        let person_id = resolve_person(&mut tx, owner.name, owner.email).await?;
        let owner_id = gen_token();
        sqlx::query(
            r#"
            INSERT INTO building_owners (id, building_id, person_id, start_date, end_date)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&owner_id)
        .bind(&building_id)
        .bind(person_id)
        .bind(owner.start_date)
        .bind(owner.end_date)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let building = sqlx::query_as::<_, Building>(
        r#"
        SELECT id, name, description, secret_slug, rotation_seed, created_at, updated_at
        FROM buildings
        WHERE id = ?
        "#,
    )
    .bind(&building_id)
    .fetch_one(pool)
    .await?;

    Ok(building)
}

/// Update a building's name and description, and optionally its
/// Ansprechpartner: when `administrator` is `Some((name, email))` the
/// building's oldest administrator row (the one created with the building) is
/// set to it, or a new row is inserted when the building has none. All
/// changes happen in one transaction; the database rejects empty names and
/// malformed e-mails.
pub async fn update_building(
    pool: &Db,
    id: &str,
    name: &str,
    description: &str,
    administrator: Option<(&str, &str)>,
) -> Result<Building, sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE buildings SET name = ?, description = ? WHERE id = ?")
        .bind(name)
        .bind(description)
        .bind(id)
        .execute(&mut *tx)
        .await?;

    if let Some((admin_name, admin_email)) = administrator {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM building_administrators WHERE building_id = ? ORDER BY created_at ASC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        // Re-resolve the person like every other contact edit: a known e-mail
        // reuses (and possibly renames) the person, an unknown one creates it.
        let person_id = resolve_person(&mut tx, admin_name, admin_email).await?;
        match existing {
            Some(admin_id) => {
                sqlx::query("UPDATE building_administrators SET person_id = ? WHERE id = ?")
                    .bind(person_id)
                    .bind(admin_id)
                    .execute(&mut *tx)
                    .await?;
            }
            None => {
                let admin_id = gen_token();
                sqlx::query(
                    r#"
                    INSERT INTO building_administrators (id, building_id, person_id)
                    VALUES (?, ?, ?)
                    "#,
                )
                .bind(admin_id)
                .bind(id)
                .bind(person_id)
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;

    sqlx::query_as::<_, Building>(
        r#"
        SELECT id, name, description, secret_slug, rotation_seed, created_at, updated_at
        FROM buildings
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn delete_building(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM buildings WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Change the rotation offset of a single building. Scoped by `id`; other
/// buildings keep their seed.
pub async fn update_building_rotation_seed(
    pool: &Db,
    id: &str,
    rotation_seed: i64,
) -> Result<Building, sqlx::Error> {
    sqlx::query("UPDATE buildings SET rotation_seed = ? WHERE id = ?")
        .bind(rotation_seed)
        .bind(id)
        .execute(pool)
        .await?;

    sqlx::query_as::<_, Building>(
        r#"
        SELECT id, name, description, secret_slug, rotation_seed, created_at, updated_at
        FROM buildings
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

// Apartments CRUD

pub async fn list_apartments(pool: &Db, building_id: &str) -> Result<Vec<Apartment>, sqlx::Error> {
    sqlx::query_as::<_, Apartment>(
        r#"
        SELECT id, building_id, name, description, position, created_at
        FROM apartments
        WHERE building_id = ?
        ORDER BY position ASC, name ASC, id ASC
        "#,
    )
    .bind(building_id)
    .fetch_all(pool)
    .await
}

pub async fn get_apartment(pool: &Db, id: &str) -> Result<Option<Apartment>, sqlx::Error> {
    sqlx::query_as::<_, Apartment>(
        r#"
        SELECT id, building_id, name, description, position, created_at
        FROM apartments
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// The initial ownership record that is normally created together with its
/// apartment; the database rejects apartments without any coverage (neither
/// their own ownership record nor a building owner, see the
/// `apartments_require_ownership` trigger in 0004/0005).
pub struct NewOwner<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub start_date: &'a str,
    pub end_date: Option<&'a str>,
}

pub async fn create_apartment(
    pool: &Db,
    building_id: &str,
    name: &str,
    description: &str,
    initial_owner: &NewOwner<'_>,
) -> Result<Apartment, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id = gen_token();

    // The person comes first (immediate FK); the ownerships FK is DEFERRABLE
    // INITIALLY DEFERRED, so the first ownership may (and must, see the
    // trigger above) be inserted before its apartment within the same
    // transaction. The person insert rolls back with any rejection.
    let person_id = resolve_person(&mut tx, initial_owner.name, initial_owner.email).await?;
    let ownership_id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO ownerships (id, apartment_id, person_id, start_date, end_date)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&ownership_id)
    .bind(&id)
    .bind(person_id)
    .bind(initial_owner.start_date)
    .bind(initial_owner.end_date)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO apartments (id, building_id, name, description, position)
        VALUES (?, ?, ?, ?, COALESCE((SELECT MAX(position) FROM apartments WHERE building_id = ?), -1) + 1)
        "#,
    )
    .bind(&id)
    .bind(building_id)
    .bind(name)
    .bind(description)
    .bind(building_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    sqlx::query_as::<_, Apartment>(
        r#"
        SELECT id, building_id, name, description, position, created_at
        FROM apartments
        WHERE id = ?
        "#,
    )
    .bind(&id)
    .fetch_one(pool)
    .await
}

/// Create an apartment without an initial ownership record; it is then owned
/// by the building's owner (see `building_owners` in 0005_building_owners.sql).
/// The `apartments_require_ownership` trigger rejects it unless the building
/// has a building owner covering the current date.
pub async fn create_apartment_building_owned(
    pool: &Db,
    building_id: &str,
    name: &str,
    description: &str,
) -> Result<Apartment, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let id = gen_token();

    sqlx::query(
        r#"
        INSERT INTO apartments (id, building_id, name, description, position)
        VALUES (?, ?, ?, ?, COALESCE((SELECT MAX(position) FROM apartments WHERE building_id = ?), -1) + 1)
        "#,
    )
    .bind(&id)
    .bind(building_id)
    .bind(name)
    .bind(description)
    .bind(building_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    sqlx::query_as::<_, Apartment>(
        r#"
        SELECT id, building_id, name, description, position, created_at
        FROM apartments
        WHERE id = ?
        "#,
    )
    .bind(&id)
    .fetch_one(pool)
    .await
}

pub async fn update_apartment(
    pool: &Db,
    id: &str,
    name: &str,
    description: &str,
) -> Result<Apartment, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE apartments
        SET name = ?, description = ?
        WHERE id = ?
        "#,
    )
    .bind(name)
    .bind(description)
    .bind(id)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Apartment>(
        r#"
        SELECT id, building_id, name, description, position, created_at
        FROM apartments
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn delete_apartment(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM apartments WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Persist a manual display order for all apartments of a building.
/// Positions are rewritten as 0-based indexes in the submitted order.
///
/// The submitted ids must be exactly the building's apartments — no
/// additions, omissions, or duplicates; otherwise `Ok(false)` is returned and
/// nothing is written.
pub async fn reorder_apartments(
    pool: &Db,
    building_id: &str,
    ordered_ids: &[String],
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM apartments WHERE building_id = ?")
        .bind(building_id)
        .fetch_one(&mut *tx)
        .await?;
    if count != ordered_ids.len() as i64 {
        return Ok(false);
    }
    // Duplicate ids would pass the per-row updates below, so reject them here.
    let unique: std::collections::HashSet<&str> = ordered_ids.iter().map(String::as_str).collect();
    if unique.len() != ordered_ids.len() {
        return Ok(false);
    }
    for (index, id) in ordered_ids.iter().enumerate() {
        let res =
            sqlx::query("UPDATE apartments SET position = ? WHERE id = ? AND building_id = ?")
                .bind(index as i64)
                .bind(id)
                .bind(building_id)
                .execute(&mut *tx)
                .await?;
        if res.rows_affected() != 1 {
            return Ok(false);
        }
    }
    tx.commit().await?;
    Ok(true)
}

// Ownerships CRUD

pub async fn list_ownerships(pool: &Db, apartment_id: &str) -> Result<Vec<Ownership>, sqlx::Error> {
    sqlx::query_as::<_, Ownership>(
        r#"
        SELECT o.id, o.apartment_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM ownerships o
        INNER JOIN people p ON p.id = o.person_id
        WHERE o.apartment_id = ?
        ORDER BY o.start_date ASC, p.name ASC
        "#,
    )
    .bind(apartment_id)
    .fetch_all(pool)
    .await
}

pub async fn list_ownerships_for_building(
    pool: &Db,
    building_id: &str,
) -> Result<Vec<Ownership>, sqlx::Error> {
    sqlx::query_as::<_, Ownership>(
        r#"
        SELECT o.id, o.apartment_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM ownerships o
        INNER JOIN people p ON p.id = o.person_id
        INNER JOIN apartments a ON a.id = o.apartment_id
        WHERE a.building_id = ?
        ORDER BY o.start_date ASC, p.name ASC
        "#,
    )
    .bind(building_id)
    .fetch_all(pool)
    .await
}

pub async fn get_ownership(pool: &Db, id: &str) -> Result<Option<Ownership>, sqlx::Error> {
    sqlx::query_as::<_, Ownership>(
        r#"
        SELECT o.id, o.apartment_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM ownerships o
        INNER JOIN people p ON p.id = o.person_id
        WHERE o.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// The ownership covering `date` (started on or before it and not ended yet),
/// if any. With the seamless chain an apartment has at most one such period.
pub async fn get_ownership_on(
    pool: &Db,
    apartment_id: &str,
    date: &str,
) -> Result<Option<Ownership>, sqlx::Error> {
    sqlx::query_as::<_, Ownership>(
        r#"
        SELECT o.id, o.apartment_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM ownerships o
        INNER JOIN people p ON p.id = o.person_id
        WHERE o.apartment_id = ?
          AND o.start_date <= ?
          AND (o.end_date IS NULL OR o.end_date >= ?)
        ORDER BY o.start_date DESC
        LIMIT 1
        "#,
    )
    .bind(apartment_id)
    .bind(date)
    .bind(date)
    .fetch_optional(pool)
    .await
}

pub async fn create_ownership(
    pool: &Db,
    apartment_id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Ownership, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // The person insert is part of the transaction: a chain-tiling or date
    // rejection rolls it back and leaves no person without a period behind.
    let person_id = resolve_person(&mut tx, name, email).await?;
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO ownerships (id, apartment_id, person_id, start_date, end_date)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(apartment_id)
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_ownership(pool, &id)
        .await
        .map(|o| o.expect("ownership just inserted"))
}

/// Identifies the previous period that a new period replaces: its id and the
/// date on which it shall end (normally the day before the new period starts).
/// Used by the "closing previous" create operations.
pub struct PreviousPeriod<'a> {
    pub id: &'a str,
    pub end_date: &'a str,
}

/// Create a new ownership and simultaneously close the previous one on the
/// day before the new period begins, in one transaction. Used when the user
/// confirms that the previous owner's open period shall end the day before
/// the new ownership starts. The chain-tiling triggers validate the combined
/// change; on rejection everything rolls back.
pub async fn create_ownership_closing_previous(
    pool: &Db,
    apartment_id: &str,
    previous: PreviousPeriod<'_>,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Ownership, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE ownerships SET end_date = ? WHERE id = ?")
        .bind(previous.end_date)
        .bind(previous.id)
        .execute(&mut *tx)
        .await?;
    let person_id = resolve_person(&mut tx, name, email).await?;
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO ownerships (id, apartment_id, person_id, start_date, end_date)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(apartment_id)
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_ownership(pool, &id)
        .await
        .map(|o| o.expect("ownership just inserted"))
}

/// Update an ownership period. Name and e-mail re-resolve the person (by
/// e-mail), so the edit form can both correct the person's contact data and
/// move the period to another person (new e-mail); the period's dates are
/// stored on the row itself.
pub async fn update_ownership(
    pool: &Db,
    id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Ownership, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let person_id = resolve_person(&mut tx, name, email).await?;
    sqlx::query(
        r#"
        UPDATE ownerships
        SET person_id = ?, start_date = ?, end_date = ?
        WHERE id = ?
        "#,
    )
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_ownership(pool, id)
        .await
        .map(|o| o.expect("ownership just updated"))
}

pub async fn delete_ownership(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM ownerships WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// Building owners CRUD

/// All building-owner periods of a building, oldest first.
pub async fn list_building_owners(
    pool: &Db,
    building_id: &str,
) -> Result<Vec<BuildingOwner>, sqlx::Error> {
    sqlx::query_as::<_, BuildingOwner>(
        r#"
        SELECT o.id, o.building_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM building_owners o
        INNER JOIN people p ON p.id = o.person_id
        WHERE o.building_id = ?
        ORDER BY o.start_date ASC, p.name ASC
        "#,
    )
    .bind(building_id)
    .fetch_all(pool)
    .await
}

/// The building-owner period covering the current date, if any.
pub async fn get_current_building_owner(
    pool: &Db,
    building_id: &str,
) -> Result<Option<BuildingOwner>, sqlx::Error> {
    sqlx::query_as::<_, BuildingOwner>(
        r#"
        SELECT o.id, o.building_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM building_owners o
        INNER JOIN people p ON p.id = o.person_id
        WHERE o.building_id = ?
          AND o.start_date <= date('now', 'localtime')
          AND (o.end_date IS NULL OR o.end_date >= date('now', 'localtime'))
        ORDER BY o.start_date DESC
        LIMIT 1
        "#,
    )
    .bind(building_id)
    .fetch_optional(pool)
    .await
}

pub async fn get_building_owner(pool: &Db, id: &str) -> Result<Option<BuildingOwner>, sqlx::Error> {
    sqlx::query_as::<_, BuildingOwner>(
        r#"
        SELECT o.id, o.building_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM building_owners o
        INNER JOIN people p ON p.id = o.person_id
        WHERE o.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// The building-owner period covering `date` (started on or before it and not
/// ended yet), if any. With the seamless chain a building has at most one such
/// period.
pub async fn get_building_owner_on(
    pool: &Db,
    building_id: &str,
    date: &str,
) -> Result<Option<BuildingOwner>, sqlx::Error> {
    sqlx::query_as::<_, BuildingOwner>(
        r#"
        SELECT o.id, o.building_id, o.person_id, p.name, p.email,
               o.start_date, o.end_date, o.created_at
        FROM building_owners o
        INNER JOIN people p ON p.id = o.person_id
        WHERE o.building_id = ?
          AND o.start_date <= ?
          AND (o.end_date IS NULL OR o.end_date >= ?)
        ORDER BY o.start_date DESC
        LIMIT 1
        "#,
    )
    .bind(building_id)
    .bind(date)
    .bind(date)
    .fetch_optional(pool)
    .await
}

pub async fn create_building_owner(
    pool: &Db,
    building_id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<BuildingOwner, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // The person insert is part of the transaction: a chain-tiling or date
    // rejection rolls it back and leaves no person without a period behind.
    let person_id = resolve_person(&mut tx, name, email).await?;
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO building_owners (id, building_id, person_id, start_date, end_date)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(building_id)
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_building_owner(pool, &id)
        .await
        .map(|o| o.expect("building owner just inserted"))
}

/// Create a new building-owner period and simultaneously close the previous
/// one on the day before the new period begins, in one transaction. Used when
/// the user confirms that the previous building owner's open period shall end
/// the day before the new building owner starts. The chain-tiling triggers
/// validate the combined change; on rejection everything rolls back.
pub async fn create_building_owner_closing_previous(
    pool: &Db,
    building_id: &str,
    previous: PreviousPeriod<'_>,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<BuildingOwner, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE building_owners SET end_date = ? WHERE id = ?")
        .bind(previous.end_date)
        .bind(previous.id)
        .execute(&mut *tx)
        .await?;
    let person_id = resolve_person(&mut tx, name, email).await?;
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO building_owners (id, building_id, person_id, start_date, end_date)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(building_id)
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_building_owner(pool, &id)
        .await
        .map(|o| o.expect("building owner just inserted"))
}

/// Update a building-owner period; person resolution behaves exactly like
/// [`update_ownership`].
pub async fn update_building_owner(
    pool: &Db,
    id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<BuildingOwner, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let person_id = resolve_person(&mut tx, name, email).await?;
    sqlx::query(
        r#"
        UPDATE building_owners
        SET person_id = ?, start_date = ?, end_date = ?
        WHERE id = ?
        "#,
    )
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_building_owner(pool, id)
        .await
        .map(|o| o.expect("building owner just updated"))
}

/// Whether any apartment of the building has its own ownership record (any
/// period, past or present). Once flats are individually owned, the building
/// is a WEG and cannot additionally be wholly owned — a building owner must
/// then not be added.
pub async fn building_has_apartment_owners(
    pool: &Db,
    building_id: &str,
) -> Result<bool, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM ownerships o
        INNER JOIN apartments a ON a.id = o.apartment_id
        WHERE a.building_id = ?
        "#,
    )
    .bind(building_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn delete_building_owner(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM building_owners WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// Tenancies CRUD

pub async fn list_tenancies(pool: &Db, apartment_id: &str) -> Result<Vec<Tenancy>, sqlx::Error> {
    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT t.id, t.apartment_id, t.person_id, p.name, p.email,
               t.start_date, t.end_date, t.created_at
        FROM tenancies t
        INNER JOIN people p ON p.id = t.person_id
        WHERE t.apartment_id = ?
        ORDER BY t.start_date ASC, p.name ASC
        "#,
    )
    .bind(apartment_id)
    .fetch_all(pool)
    .await
}

pub async fn list_tenancies_for_building(
    pool: &Db,
    building_id: &str,
) -> Result<Vec<Tenancy>, sqlx::Error> {
    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT t.id, t.apartment_id, t.person_id, p.name, p.email,
               t.start_date, t.end_date, t.created_at
        FROM tenancies t
        INNER JOIN people p ON p.id = t.person_id
        INNER JOIN apartments a ON a.id = t.apartment_id
        WHERE a.building_id = ?
        ORDER BY t.start_date ASC, p.name ASC
        "#,
    )
    .bind(building_id)
    .fetch_all(pool)
    .await
}

/// The tenancy covering `date` (started on or before it and not ended yet),
/// if any. The no-overlap rule allows at most one such period.
pub async fn get_tenancy_on(
    pool: &Db,
    apartment_id: &str,
    date: &str,
) -> Result<Option<Tenancy>, sqlx::Error> {
    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT t.id, t.apartment_id, t.person_id, p.name, p.email,
               t.start_date, t.end_date, t.created_at
        FROM tenancies t
        INNER JOIN people p ON p.id = t.person_id
        WHERE t.apartment_id = ?
          AND t.start_date <= ?
          AND (t.end_date IS NULL OR t.end_date >= ?)
        ORDER BY t.start_date DESC
        LIMIT 1
        "#,
    )
    .bind(apartment_id)
    .bind(date)
    .bind(date)
    .fetch_optional(pool)
    .await
}

pub async fn get_tenancy(pool: &Db, id: &str) -> Result<Option<Tenancy>, sqlx::Error> {
    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT t.id, t.apartment_id, t.person_id, p.name, p.email,
               t.start_date, t.end_date, t.created_at
        FROM tenancies t
        INNER JOIN people p ON p.id = t.person_id
        WHERE t.id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn create_tenancy(
    pool: &Db,
    apartment_id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Tenancy, sqlx::Error> {
    let mut tx = pool.begin().await?;
    // The person insert is part of the transaction: an overlap or date
    // rejection rolls it back and leaves no person without a period behind.
    let person_id = resolve_person(&mut tx, name, email).await?;
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO tenancies (id, apartment_id, person_id, start_date, end_date)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(apartment_id)
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_tenancy(pool, &id)
        .await
        .map(|t| t.expect("tenancy just inserted"))
}

/// Create a new tenancy and simultaneously close the previous one on the day
/// before the new tenancy begins, in one transaction. Used when the user
/// confirms that the previous tenant's open period shall end the day before
/// the new tenancy starts. The no-overlap triggers validate the combined
/// change; on rejection everything rolls back.
pub async fn create_tenancy_closing_previous(
    pool: &Db,
    apartment_id: &str,
    previous: PreviousPeriod<'_>,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Tenancy, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE tenancies SET end_date = ? WHERE id = ?")
        .bind(previous.end_date)
        .bind(previous.id)
        .execute(&mut *tx)
        .await?;
    let person_id = resolve_person(&mut tx, name, email).await?;
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO tenancies (id, apartment_id, person_id, start_date, end_date)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(apartment_id)
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_tenancy(pool, &id)
        .await
        .map(|t| t.expect("tenancy just inserted"))
}

/// Update a tenancy; person resolution behaves exactly like
/// [`update_ownership`] (identity by e-mail, name sync).
pub async fn update_tenancy(
    pool: &Db,
    id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Tenancy, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let person_id = resolve_person(&mut tx, name, email).await?;
    sqlx::query(
        r#"
        UPDATE tenancies
        SET person_id = ?, start_date = ?, end_date = ?
        WHERE id = ?
        "#,
    )
    .bind(person_id)
    .bind(start_date)
    .bind(end_date)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_tenancy(pool, id)
        .await
        .map(|t| t.expect("tenancy just updated"))
}

pub async fn delete_tenancy(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM tenancies WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// People (person page)

pub async fn get_person(pool: &Db, id: &str) -> Result<Option<Person>, sqlx::Error> {
    sqlx::query_as::<_, Person>(
        r#"
        SELECT id, name, email, created_at
        FROM people
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Update the person's current contact data. The e-mail address is the
/// person's identity: an address that already belongs to another person is
/// rejected by the `people_email_unique_*` triggers of 0009 with a German
/// message (shown inline on the person page), like every other database rule.
/// Name and e-mail are stored trimmed, mirroring [`resolve_person`].
pub async fn update_person(
    pool: &Db,
    id: &str,
    name: &str,
    email: &str,
) -> Result<Person, sqlx::Error> {
    let res = sqlx::query("UPDATE people SET name = ?, email = ? WHERE id = ?")
        .bind(name.trim())
        .bind(email.trim())
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    get_person(pool, id)
        .await
        .map(|p| p.expect("person just updated"))
}

/// Everything one person refers to, as role rows: administered buildings,
/// owned buildings, owned apartments, rented apartments. With `None` for
/// `person_id`, every role row of every person is returned, ordered by person
/// then by role (apartment owner, building owner, admin, tenant) and object
/// name — the shape both the people index and the person page group.
pub async fn list_person_roles(
    pool: &Db,
    person_id: Option<&str>,
) -> Result<Vec<PersonRoleRow>, sqlx::Error> {
    sqlx::query_as::<_, PersonRoleRow>(
        r#"
        SELECT * FROM (
            SELECT p.id AS person_id, p.name AS person_name, p.email AS person_email,
                   'admin' AS kind,
                   b.id AS building_id, b.name AS building_name,
                   NULL AS apartment_id, NULL AS apartment_name,
                   NULL AS start_date, NULL AS end_date
            FROM building_administrators a
            INNER JOIN people p ON p.id = a.person_id
            INNER JOIN buildings b ON b.id = a.building_id
            UNION ALL
            SELECT p.id, p.name, p.email, 'building_owner',
                   b.id, b.name, NULL, NULL, bo.start_date, bo.end_date
            FROM building_owners bo
            INNER JOIN people p ON p.id = bo.person_id
            INNER JOIN buildings b ON b.id = bo.building_id
            UNION ALL
            SELECT p.id, p.name, p.email, 'apartment_owner',
                   b.id, b.name, a.id, a.name, o.start_date, o.end_date
            FROM ownerships o
            INNER JOIN people p ON p.id = o.person_id
            INNER JOIN apartments a ON a.id = o.apartment_id
            INNER JOIN buildings b ON b.id = a.building_id
            UNION ALL
            SELECT p.id, p.name, p.email, 'tenant',
                   b.id, b.name, a.id, a.name, t.start_date, t.end_date
            FROM tenancies t
            INNER JOIN people p ON p.id = t.person_id
            INNER JOIN apartments a ON a.id = t.apartment_id
            INNER JOIN buildings b ON b.id = a.building_id
        )
        WHERE (? IS NULL OR person_id = ?)
        ORDER BY lower(person_name), person_id,
                 CASE kind
                     WHEN 'apartment_owner' THEN 0
                     WHEN 'building_owner' THEN 1
                     WHEN 'admin' THEN 2
                     WHEN 'tenant' THEN 3
                     ELSE 4
                 END,
                 lower(COALESCE(building_name, '')),
                 lower(COALESCE(apartment_name, ''))
        "#,
    )
    .bind(person_id)
    .bind(person_id)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn building_crud_works() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(
            &pool,
            "Haus Sonnenschein",
            "A nice building",
            "Alice",
            "alice@example.com",
        )
        .await
        .expect("create building");

        assert_eq!(building.name, "Haus Sonnenschein");
        assert_eq!(building.description, "A nice building");
        assert!(!building.secret_slug.is_empty());

        let buildings = list_buildings(&pool).await.expect("list buildings");
        assert_eq!(buildings.len(), 1);

        let got = get_building(&pool, &building.id)
            .await
            .expect("get building")
            .expect("building exists");
        assert_eq!(got.0.name, "Haus Sonnenschein");
        assert_eq!(got.1.len(), 1);
        assert_eq!(got.1[0].name, "Alice");

        let by_slug = get_building_by_slug(&pool, &building.secret_slug)
            .await
            .expect("get building by slug")
            .expect("building exists");
        assert_eq!(by_slug.id, building.id);

        let deleted = delete_building(&pool, &building.id)
            .await
            .expect("delete building");
        assert!(deleted);

        let after = list_buildings(&pool).await.expect("list after delete");
        assert_eq!(after.len(), 0);
    }

    #[tokio::test]
    async fn rotation_seed_update_is_scoped_to_building() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let first = create_building(&pool, "First", "", "Alice", "alice@example.com")
            .await
            .expect("create building");
        let second = create_building(&pool, "Second", "", "Bob", "bob@example.com")
            .await
            .expect("create building");
        assert_eq!(first.rotation_seed, 0);
        assert_eq!(second.rotation_seed, 0);

        let updated = update_building_rotation_seed(&pool, &first.id, 3)
            .await
            .expect("update seed");
        assert_eq!(updated.rotation_seed, 3);

        let (got_first, _) = get_building(&pool, &first.id)
            .await
            .expect("get first")
            .expect("first exists");
        assert_eq!(got_first.rotation_seed, 3);
        let (got_second, _) = get_building(&pool, &second.id)
            .await
            .expect("get second")
            .expect("second exists");
        assert_eq!(
            got_second.rotation_seed, 0,
            "other building must keep its seed"
        );
    }

    #[tokio::test]
    async fn apartment_ownership_tenancy_crud_works() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");

        let apt = create_apartment(
            &pool,
            &building.id,
            "EG links",
            "Ground floor left",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2023-01-01",
                end_date: Some("2023-12-31"),
            },
        )
        .await
        .expect("create apartment");
        assert_eq!(apt.name, "EG links");
        assert_eq!(apt.building_id, building.id);

        let list = list_apartments(&pool, &building.id)
            .await
            .expect("list apartments");
        assert_eq!(list.len(), 1);

        let apt2 = update_apartment(&pool, &apt.id, "EG links (neu)", "Renamed")
            .await
            .expect("update apartment");
        assert_eq!(apt2.name, "EG links (neu)");

        // The apartment was created with its first ownership; the next
        // ownership must tile the chain (2024-01-01 follows the 2023 period).
        let o = create_ownership(&pool, &apt.id, "Bob", "bob@example.com", "2024-01-01", None)
            .await
            .expect("create ownership");
        assert_eq!(o.name, "Bob");
        let o_list = list_ownerships(&pool, &apt.id)
            .await
            .expect("list ownerships");
        assert_eq!(o_list.len(), 2);
        let o2 = update_ownership(
            &pool,
            &o.id,
            "Bobby",
            "bobby@example.com",
            "2024-01-01",
            Some("2024-12-31"),
        )
        .await
        .expect("update ownership");
        assert_eq!(o2.end_date.as_deref(), Some("2024-12-31"));

        // Tenancy
        let t = create_tenancy(
            &pool,
            &apt.id,
            "Carla",
            "carla@example.com",
            "2024-06-01",
            None,
        )
        .await
        .expect("create tenancy");
        assert_eq!(t.name, "Carla");
        let t_list = list_tenancies(&pool, &apt.id)
            .await
            .expect("list tenancies");
        assert_eq!(t_list.len(), 1);

        // Building-level listing includes both records
        let all_o = list_ownerships_for_building(&pool, &building.id)
            .await
            .expect("ownerships for building");
        assert_eq!(all_o.len(), 2);
        let all_t = list_tenancies_for_building(&pool, &building.id)
            .await
            .expect("tenancies for building");
        assert_eq!(all_t.len(), 1);

        // Delete tenancy and the last ownership of the chain (allowed; the
        // remaining 2023 ownership is the apartment's last and may only
        // disappear together with the apartment via the cascade).
        assert!(delete_tenancy(&pool, &t.id).await.expect("delete tenancy"));
        assert!(delete_ownership(&pool, &o2.id)
            .await
            .expect("delete ownership"));
        assert!(delete_apartment(&pool, &apt.id)
            .await
            .expect("delete apartment"));
        let list2 = list_apartments(&pool, &building.id)
            .await
            .expect("list apartments after delete");
        assert_eq!(list2.len(), 0);
    }

    #[tokio::test]
    async fn apartment_requires_initial_ownership() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");

        // An apartment without an ownership record is impossible.
        let err = sqlx::query(
            "INSERT INTO apartments (id, building_id, name, description, position) VALUES (?, ?, 'Alone', '', 0)",
        )
        .bind(&building.id)
        .execute(&pool)
        .await
        .expect_err("apartment without ownership must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("Eigentum"));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM apartments")
            .fetch_one(&pool)
            .await
            .expect("count apartments");
        assert_eq!(count, 0);

        // And the last ownership of an apartment cannot be deleted either.
        let apt = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2024-01-01",
                end_date: None,
            },
        )
        .await
        .expect("create apartment");
        let owners = list_ownerships(&pool, &apt.id)
            .await
            .expect("list ownerships");
        assert_eq!(owners.len(), 1);
        let err = delete_ownership(&pool, &owners[0].id)
            .await
            .expect_err("deleting the last ownership must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("letzte Eigentum"));
        // The apartment still has its owner.
        assert_eq!(
            list_ownerships(&pool, &apt.id)
                .await
                .expect("list ownerships")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn invalid_input_is_rejected_in_db() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");
        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");

        let check = |expected: &str, err: sqlx::Error| {
            let msg = err
                .as_database_error()
                .expect("database error")
                .message()
                .to_string();
            assert!(
                msg.contains(expected),
                "expected {expected:?} in message, got {msg:?}"
            );
        };

        check(
            "Name darf nicht leer sein.",
            create_apartment(
                &pool,
                &building.id,
                "EG",
                "",
                &NewOwner {
                    name: " ",
                    email: "a@b.de",
                    start_date: "2024-01-01",
                    end_date: None,
                },
            )
            .await
            .expect_err("empty owner name must be rejected"),
        );
        check(
            "höchstens 30 Zeichen",
            create_apartment(
                &pool,
                &building.id,
                &"y".repeat(31),
                "",
                &NewOwner {
                    name: "Bob",
                    email: "a@b.de",
                    start_date: "2024-01-01",
                    end_date: None,
                },
            )
            .await
            .expect_err("over-long apartment name must be rejected"),
        );
        check(
            "E-Mail-Adresse",
            create_apartment(
                &pool,
                &building.id,
                "EG",
                "",
                &NewOwner {
                    name: "Bob",
                    email: "bob.example.com",
                    start_date: "2024-01-01",
                    end_date: None,
                },
            )
            .await
            .expect_err("invalid e-mail must be rejected"),
        );
        check(
            "Startdatum muss im Format",
            create_apartment(
                &pool,
                &building.id,
                "EG",
                "",
                &NewOwner {
                    name: "Bob",
                    email: "bob@example.com",
                    start_date: "2024-13-01",
                    end_date: None,
                },
            )
            .await
            .expect_err("invalid start date must be rejected"),
        );
        check(
            "Startdatum darf nicht nach dem Enddatum",
            create_apartment(
                &pool,
                &building.id,
                "EG",
                "",
                &NewOwner {
                    name: "Bob",
                    email: "bob@example.com",
                    start_date: "2024-02-01",
                    end_date: Some("2024-01-01"),
                },
            )
            .await
            .expect_err("end before start must be rejected"),
        );

        // Building names are validated as well.
        let err = create_building(&pool, &"x".repeat(31), "", "Alice", "alice@example.com")
            .await
            .expect_err("building name too long must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("höchstens 30 Zeichen"));
    }

    #[tokio::test]
    async fn ownership_chain_is_enforced_in_db() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");
        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");
        let apt = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Ada",
                email: "ada@example.com",
                start_date: "2026-01-01",
                end_date: Some("2026-06-30"),
            },
        )
        .await
        .expect("create apartment");
        let ada = list_ownerships(&pool, &apt.id)
            .await
            .expect("initial ownership")[0]
            .id
            .clone();

        let reject = |expected: &str, result: sqlx::Error| {
            let msg = result.as_database_error().expect("database error");
            assert!(
                msg.message().contains(expected),
                "expected {expected:?} in message, got {:?}",
                msg.message()
            );
        };

        // Overlapping the existing period is rejected.
        reject(
            "überschneidet",
            create_ownership(&pool, &apt.id, "Bo", "bo@example.com", "2026-06-01", None)
                .await
                .expect_err("overlap must be rejected"),
        );
        // A gap after the previous period is rejected.
        reject(
            "muss am Tag nach dem Ende",
            create_ownership(&pool, &apt.id, "Bo", "bo@example.com", "2026-07-02", None)
                .await
                .expect_err("gap must be rejected"),
        );
        // A tiled continuation is accepted.
        let bo = create_ownership(
            &pool,
            &apt.id,
            "Bo",
            "bo@example.com",
            "2026-07-01",
            Some("2026-12-31"),
        )
        .await
        .expect("tiled continuation");
        // And one more, keeping the chain tiled.
        create_ownership(&pool, &apt.id, "Cy", "cy@example.com", "2027-01-01", None)
            .await
            .expect("tiled successor");

        // Updating Bo into a gap (starting the day after Ada's end) is rejected.
        reject(
            "muss am Tag nach dem Ende",
            update_ownership(
                &pool,
                &bo.id,
                "Bo",
                "bo@example.com",
                "2026-07-02",
                Some("2026-12-31"),
            )
            .await
            .expect_err("gap update must be rejected"),
        );
        // Updating Bo into an overlap is rejected.
        reject(
            "überschneidet",
            update_ownership(
                &pool,
                &bo.id,
                "Bo",
                "bo@example.com",
                "2026-06-01",
                Some("2026-12-31"),
            )
            .await
            .expect_err("overlap update must be rejected"),
        );

        // Deleting the middle record (Bo, between Ada and Cy) is rejected...
        reject(
            "zwischen zwei anderen",
            delete_ownership(&pool, &bo.id)
                .await
                .expect_err("middle delete must be rejected"),
        );
        // ...the last record (Cy) may be deleted...
        let cy = list_ownerships(&pool, &apt.id)
            .await
            .expect("list ownerships")
            .into_iter()
            .find(|o| o.name == "Cy")
            .expect("Cy");
        assert!(delete_ownership(&pool, &cy.id)
            .await
            .expect("delete last ownership of a chain"));
        // ...and then Bo may go (now the last of the remaining chain).
        assert!(delete_ownership(&pool, &bo.id)
            .await
            .expect("delete now-last ownership"));
        // But Ada is the last remaining record and is protected.
        reject(
            "letzte Eigentum",
            delete_ownership(&pool, &ada)
                .await
                .expect_err("last ownership delete must be rejected"),
        );
    }

    #[tokio::test]
    async fn tenancy_overlap_is_enforced_in_db() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");
        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");
        let apt = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2024-01-01",
                end_date: None,
            },
        )
        .await
        .expect("create apartment");

        create_tenancy(
            &pool,
            &apt.id,
            "Nina",
            "nina@example.com",
            "2026-01-01",
            Some("2026-06-30"),
        )
        .await
        .expect("first tenancy");

        // Overlap is rejected.
        let err = create_tenancy(
            &pool,
            &apt.id,
            "Karl",
            "karl@example.com",
            "2026-06-01",
            None,
        )
        .await
        .expect_err("overlapping tenancy must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("überschneidet ein bestehendes Mietverhältnis"));

        // Adjacent is fine.
        let karl = create_tenancy(
            &pool,
            &apt.id,
            "Karl",
            "karl@example.com",
            "2026-07-01",
            None,
        )
        .await
        .expect("adjacent tenancy");

        // Updating Karl into the overlap is rejected.
        let err = update_tenancy(
            &pool,
            &karl.id,
            "Karl",
            "karl@example.com",
            "2026-06-01",
            None,
        )
        .await
        .expect_err("overlapping tenancy update must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("überschneidet"));
    }

    #[tokio::test]
    async fn apartment_reorder_persists_position() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");

        let owner = || NewOwner {
            name: "Alice",
            email: "alice@example.com",
            start_date: "2023-01-01",
            end_date: Some("2023-12-31"),
        };
        let a = create_apartment(&pool, &building.id, "Erdgeschoss", "", &owner())
            .await
            .expect("create apartment");
        let b = create_apartment(&pool, &building.id, "Obergeschoss", "", &owner())
            .await
            .expect("create apartment");
        let c = create_apartment(&pool, &building.id, "Dachgeschoss", "", &owner())
            .await
            .expect("create apartment");

        // New apartments are appended in creation order.
        let initial: Vec<String> = list_apartments(&pool, &building.id)
            .await
            .expect("list apartments")
            .into_iter()
            .map(|ap| ap.id)
            .collect();
        assert_eq!(initial, vec![a.id.clone(), b.id.clone(), c.id.clone()]);

        // Reorder to c, a, b.
        assert!(reorder_apartments(
            &pool,
            &building.id,
            &[c.id.clone(), a.id.clone(), b.id.clone()],
        )
        .await
        .expect("reorder apartments"));

        let reordered: Vec<String> = list_apartments(&pool, &building.id)
            .await
            .expect("list apartments after reorder")
            .into_iter()
            .map(|ap| ap.id)
            .collect();
        assert_eq!(reordered, vec![c.id, a.id, b.id]);
    }

    #[tokio::test]
    async fn reorder_rejects_wrong_sets() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");
        let owner = NewOwner {
            name: "Alice",
            email: "alice@example.com",
            start_date: "2023-01-01",
            end_date: Some("2023-12-31"),
        };
        let a = create_apartment(&pool, &building.id, "Erdgeschoss", "", &owner)
            .await
            .expect("create apartment");
        let b = create_apartment(&pool, &building.id, "Obergeschoss", "", &owner)
            .await
            .expect("create apartment");

        // An id from another building.
        assert!(
            !reorder_apartments(&pool, &building.id, &[a.id.clone(), "foreign".into()],)
                .await
                .expect("foreign id")
        );
        // An omitted apartment.
        assert!(
            !reorder_apartments(&pool, &building.id, std::slice::from_ref(&a.id))
                .await
                .expect("omitted apartment")
        );
        // A duplicate.
        assert!(
            !reorder_apartments(&pool, &building.id, &[a.id.clone(), a.id.clone()],)
                .await
                .expect("duplicate")
        );
        // The original order is untouched by rejected reorders.
        let order: Vec<String> = list_apartments(&pool, &building.id)
            .await
            .expect("list apartments")
            .into_iter()
            .map(|ap| ap.id)
            .collect();
        assert_eq!(order, vec![a.id, b.id]);
    }

    #[tokio::test]
    async fn building_owner_crud_works() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");

        // First owner period, closed.
        let first = create_building_owner(
            &pool,
            &building.id,
            "Deutsche Wohnbau SE",
            "service@deutsche-wohnbau.example",
            "1995-01-01",
            Some("2000-12-31"),
        )
        .await
        .expect("create building owner");
        assert_eq!(first.name, "Deutsche Wohnbau SE");

        // Owners of a building must tile its timeline: overlapping or gap
        // periods are rejected.
        let err = create_building_owner(
            &pool,
            &building.id,
            "Bauverein Ost",
            "bauverein@example.com",
            "2000-06-01",
            Some("2001-12-31"),
        )
        .await
        .expect_err("overlapping building owner must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("überschneidet"));
        let err = create_building_owner(
            &pool,
            &building.id,
            "Bauverein Ost",
            "bauverein@example.com",
            "2001-03-01",
            Some("2005-12-31"),
        )
        .await
        .expect_err("gapped building owner must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("Tag nach dem Ende"));

        // Tiled successors: a second period (closed), then a third period
        // that is open-ended and thus covers today (the current owner).
        let second = create_building_owner(
            &pool,
            &building.id,
            "Bauverein Ost",
            "bauverein@example.com",
            "2001-01-01",
            Some("2005-12-31"),
        )
        .await
        .expect("create tiled building owner");
        let third = create_building_owner(
            &pool,
            &building.id,
            "Wohnen am Ring GmbH",
            "verwaltung@wohnen-am-ring.example",
            "2006-01-01",
            None,
        )
        .await
        .expect("create open-ended building owner");
        let current = get_current_building_owner(&pool, &building.id)
            .await
            .expect("current building owner")
            .expect("open-ended owner covers today");
        assert_eq!(current.id, third.id);

        let list = list_building_owners(&pool, &building.id)
            .await
            .expect("list building owners");
        assert_eq!(list.len(), 3);

        let updated = update_building_owner(
            &pool,
            &first.id,
            "Deutsche Wohnbau AG",
            "service@deutsche-wohnbau.example",
            "1995-01-01",
            Some("2000-12-31"),
        )
        .await
        .expect("update building owner");
        assert_eq!(updated.name, "Deutsche Wohnbau AG");

        // A period between two others cannot be deleted (no holes in the
        // chain); the first and the last period can.
        let err = delete_building_owner(&pool, &second.id)
            .await
            .expect_err("middle period must be protected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("zwischen zwei anderen"));
        assert!(delete_building_owner(&pool, &first.id)
            .await
            .expect("delete building owner"));
        assert!(delete_building_owner(&pool, &third.id)
            .await
            .expect("delete building owner"));
        // The protected middle period is still there.
        let list = list_building_owners(&pool, &building.id)
            .await
            .expect("list building owners after delete");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, second.id);
    }

    #[tokio::test]
    async fn building_owner_covers_apartments_without_ownership() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");

        // Without a building owner the apartment is rejected…
        let err = create_apartment_building_owned(&pool, &building.id, "EG", "")
            .await
            .expect_err("apartment without any owner must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("Eigentum"));

        // …with a covering building owner it is accepted without ownership.
        create_building_owner(
            &pool,
            &building.id,
            "Deutsche Wohnbau SE",
            "service@deutsche-wohnbau.example",
            "1995-01-01",
            None,
        )
        .await
        .expect("create building owner");
        let apt = create_apartment_building_owned(&pool, &building.id, "EG links", "")
            .await
            .expect("apartment covered by building owner");
        assert_eq!(apt.name, "EG links");
        assert!(
            list_ownerships(&pool, &apt.id)
                .await
                .expect("list ownerships")
                .is_empty(),
            "the apartment has no per-apartment ownership"
        );

        // The last building owner cannot be deleted while an apartment depends
        // on it.
        let owner_id = list_building_owners(&pool, &building.id)
            .await
            .expect("list building owners")[0]
            .id
            .clone();
        let err = delete_building_owner(&pool, &owner_id)
            .await
            .expect_err("last needed building owner must be protected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("letzte Gebäudeeigentümer"));

        // The two ownership forms are mutually exclusive (0010): this
        // building-owned apartment must not be able to acquire an ownership
        // record, not even through the query layer.
        let err = create_ownership(
            &pool,
            &apt.id,
            "Eigentümer GmbH",
            "eigentuemer@example.com",
            "2020-01-01",
            None,
        )
        .await
        .expect_err("ownership in a building-owned apartment must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("keinen eigenen Eigentümer"));
        assert!(
            list_ownerships(&pool, &apt.id)
                .await
                .expect("list ownerships")
                .is_empty(),
            "the rejected insert left no ownership behind"
        );
    }

    #[tokio::test]
    async fn ownership_and_building_owner_are_mutually_exclusive() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        let building = create_building(&pool, "Test Building", "", "", "")
            .await
            .expect("create building");

        // A building owner may be added while the building has no apartments
        // and no pending ownerships.
        create_building_owner(
            &pool,
            &building.id,
            "Deutsche Wohnbau SE",
            "service@deutsche-wohnbau.example",
            "1995-01-01",
            None,
        )
        .await
        .expect("create building owner");

        // Creating an apartment TOGETHER with an initial owner in a
        // wholly-owned building is rejected (the create-apartment flow, 0010).
        let err = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2026-01-01",
                end_date: None,
            },
        )
        .await
        .expect_err("apartment with owner in a wholly-owned building must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("keinen eigenen Eigentümer"));
        assert!(
            list_apartments(&pool, &building.id)
                .await
                .expect("list apartments")
                .is_empty(),
            "the rejected create left no apartment behind (transaction rollback)"
        );

        // …a building-owned apartment without its own owner is fine…
        let apt = create_apartment_building_owned(&pool, &building.id, "EG", "")
            .await
            .expect("building-owned apartment");

        // …and adding an ownership to it later is rejected as well (the
        // add-owner flow, 0010).
        let err = create_ownership(
            &pool,
            &apt.id,
            "Alice",
            "alice@example.com",
            "2026-01-01",
            None,
        )
        .await
        .expect_err("ownership added to a building-owned apartment must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("keinen eigenen Eigentümer"));
        // The rejected person insert rolled back with the ownership.
        assert_eq!(
            list_people(&pool).await.expect("list people").len(),
            1,
            "only the building owner's person exists"
        );

        // The last ownership of an apartment is protected without a covering
        // building owner — the reverse direction: a WEG apartment cannot shed
        // its owner, so a building owner can never be introduced later.
        let weg = create_building(&pool, "WEG-Block", "", "", "")
            .await
            .expect("create WEG building");
        let weg_apt = create_apartment(
            &pool,
            &weg.id,
            "EG",
            "",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2026-01-01",
                end_date: None,
            },
        )
        .await
        .expect("WEG apartment with owner");
        assert!(
            delete_ownership(&pool, &owners_id(&pool, &weg_apt.id).await[0].id)
                .await
                .is_err_and(|e| {
                    e.as_database_error()
                        .is_some_and(|d| d.message().contains("letzte Eigentum"))
                })
        );

        // A building owner cannot be added while apartments have ownerships
        // (0010) — the WEG stays a WEG.
        let err = create_building_owner(
            &pool,
            &weg.id,
            "Deutsche Wohnbau SE",
            "service@deutsche-wohnbau.example",
            "2026-01-01",
            None,
        )
        .await
        .expect_err("building owner in a WEG must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("kann keinen Gebäudeeigentümer haben"));
    }

    /// Helper: ownerships of an apartment (for the exclusive test above).
    async fn owners_id(pool: &Db, apartment_id: &str) -> Vec<Ownership> {
        list_ownerships(pool, apartment_id)
            .await
            .expect("list ownerships")
    }

    #[tokio::test]
    async fn owner_person_is_shared_across_periods_and_roles() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");
        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");
        // The building owner lives in a second, wholly-owned building: the two
        // ownership forms are mutually exclusive (0010), and the person row is
        // shared across roles and buildings.
        let owned_building = create_building(&pool, "Owned Block", "", "", "")
            .await
            .expect("create wholly-owned building");

        // One apartment owned by Alice (period 2023), a building-level owner
        // with the same e-mail but a fuller name, and a second apartment also
        // owned by Alice — all three periods must share a single person row.
        let apt = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2023-01-01",
                end_date: Some("2023-12-31"),
            },
        )
        .await
        .expect("create apartment");
        let apt2 = create_apartment(
            &pool,
            &building.id,
            "OG",
            "",
            &NewOwner {
                name: "Alice Liddell",
                email: "ALICE@example.com", // same person, different case
                start_date: "2024-01-01",
                end_date: None,
            },
        )
        .await
        .expect("create second apartment");
        create_building_owner(
            &pool,
            &owned_building.id,
            "Alice Liddell",
            "alice@example.com",
            "2025-01-01",
            None,
        )
        .await
        .expect("create building owner");

        let people = list_people(&pool).await.expect("list people");
        assert_eq!(
            people.len(),
            1,
            "one e-mail must yield exactly one person, got {people:?}"
        );
        // The newest submission (case variant included) wins as current name.
        assert_eq!(people[0].name, "Alice Liddell");
        assert_eq!(people[0].email, "alice@example.com");

        // All periods reference the same person.
        let owners = list_ownerships_for_building(&pool, &building.id)
            .await
            .expect("list ownerships");
        assert_eq!(owners.len(), 2);
        assert!(owners.iter().all(|o| o.person_id == people[0].id));
        let building_owners = list_building_owners(&pool, &owned_building.id)
            .await
            .expect("list building owners");
        assert_eq!(building_owners.len(), 1);
        assert_eq!(building_owners[0].person_id, people[0].id);

        // Renaming the person through one period renames all periods at once:
        // the person's data is shared current contact data, not a snapshot.
        let updated = update_building_owner(
            &pool,
            &building_owners[0].id,
            "Liddell Immobilien GmbH",
            "alice@example.com",
            "2025-01-01",
            None,
        )
        .await
        .expect("update building owner");
        assert_eq!(updated.name, "Liddell Immobilien GmbH");
        let owners = list_ownerships_for_building(&pool, &building.id)
            .await
            .expect("list ownerships after rename");
        assert!(owners
            .iter()
            .all(|o| o.name == "Liddell Immobilien GmbH" && o.person_id == people[0].id));
        assert_eq!(
            list_building_owners(&pool, &owned_building.id)
                .await
                .expect("list building owners after rename")[0]
                .name,
            "Liddell Immobilien GmbH"
        );
        assert_eq!(list_people(&pool).await.expect("list people").len(), 1);

        // Moving a period to another person (different e-mail) leaves the
        // others untouched.
        let moved = update_ownership(
            &pool,
            &owners[1].id,
            "Max Mustermann",
            "max@example.com",
            "2024-01-01",
            None,
        )
        .await
        .expect("move ownership to another person");
        assert_eq!(moved.name, "Max Mustermann");
        let people = list_people(&pool).await.expect("list people");
        assert_eq!(people.len(), 2);
        assert_ne!(people[0].id, people[1].id);
        assert_eq!(owners[0].person_id, people[0].id);
        assert_eq!(moved.person_id, people[1].id);
        let _ = apt;
        let _ = apt2;
    }

    #[tokio::test]
    async fn person_without_period_is_removed() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");
        let building = create_building(&pool, "Test Building", "", "", "")
            .await
            .expect("create building");

        // Two tiled periods: 2023 (Ada) and 2024 (open-ended, Bo). The 2023
        // period may be deleted (it is not the last one), which would orphan
        // its person — the cleanup trigger must remove Ada.
        let apt = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Ada",
                email: "ada@example.com",
                start_date: "2023-01-01",
                end_date: Some("2023-12-31"),
            },
        )
        .await
        .expect("create apartment");
        let bo = create_ownership(&pool, &apt.id, "Bo", "bo@example.com", "2024-01-01", None)
            .await
            .expect("create successor ownership");
        assert_eq!(list_people(&pool).await.expect("list people").len(), 2);

        let ada_id = list_ownerships(&pool, &apt.id)
            .await
            .expect("list ownerships")
            .into_iter()
            .find(|o| o.name == "Ada")
            .expect("Ada period")
            .id;
        assert!(delete_ownership(&pool, &ada_id)
            .await
            .expect("delete non-last ownership"));
        let people = list_people(&pool).await.expect("list people after delete");
        assert_eq!(
            people.len(),
            1,
            "orphaned Ada must be removed, got {people:?}"
        );
        assert_eq!(people[0].name, "Bo");

        // Deleting the last ownership is rejected by the guard, so Bo stays.
        let owners = list_ownerships(&pool, &apt.id)
            .await
            .expect("list ownerships");
        assert_eq!(owners.len(), 1);
        assert!(delete_ownership(&pool, &owners[0].id)
            .await
            .is_err_and(|e| {
                e.as_database_error()
                    .is_some_and(|d| d.message().contains("letzte Eigentum"))
            }));
        assert_eq!(list_people(&pool).await.expect("list people").len(), 1);

        // A person referenced by a period elsewhere survives the cascade when
        // the apartment (and its ownership) goes.
        let other = create_building(&pool, "Other", "", "", "")
            .await
            .expect("create other building");
        create_building_owner(&pool, &other.id, "Bo", "bo@example.com", "2020-01-01", None)
            .await
            .expect("create building owner for Bo");
        assert!(delete_apartment(&pool, &apt.id)
            .await
            .expect("delete apartment"));
        let people = list_people(&pool).await.expect("list people after cascade");
        assert_eq!(people.len(), 1, "Bo must survive, got {people:?}");
        assert_eq!(people[0].name, "Bo");
        let _ = bo;
    }

    #[tokio::test]
    async fn tenant_and_admin_share_the_person_table() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");
        // Alice is the Ansprechpartner of the building and the owner of the
        // apartment; Bob is the tenant of the same apartment — one person row
        // per role set.
        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");
        let apt = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2026-01-01",
                end_date: None,
            },
        )
        .await
        .expect("create apartment");
        let tenancy = create_tenancy(&pool, &apt.id, "Bob", "bob@example.com", "2026-02-01", None)
            .await
            .expect("create tenancy");

        let people = list_people(&pool).await.expect("list people");
        assert_eq!(people.len(), 2, "admin/owner + tenant, got {people:?}");
        let (bob, alice) = if people[0].name == "Bob" {
            (&people[0], &people[1])
        } else {
            (&people[1], &people[0])
        };
        assert_eq!(alice.name, "Alice");
        assert_eq!(bob.name, "Bob");

        // The admin row and the ownership reference Alice, the tenancy Bob.
        let (_, admins) = get_building(&pool, &building.id)
            .await
            .expect("get building")
            .unwrap();
        assert_eq!(admins.len(), 1);
        assert_eq!(admins[0].person_id, alice.id);
        let owners = list_ownerships(&pool, &apt.id)
            .await
            .expect("list ownerships");
        assert_eq!(owners[0].person_id, alice.id);
        assert_eq!(tenancy.person_id, bob.id);

        // Renaming Bob through the tenancy keeps the single tenant person;
        // the delegation in the schedule picks up the person's current name.
        let renamed = update_tenancy(
            &pool,
            &tenancy.id,
            "Bob Schiller",
            "bob@example.com",
            "2026-02-01",
            None,
        )
        .await
        .expect("rename tenant");
        assert_eq!(renamed.name, "Bob Schiller");
        assert_eq!(list_people(&pool).await.expect("list people").len(), 2);

        // Reusing the tenant's person as Ansprechpartner of another building
        // links the same row.
        let other = create_building(&pool, "Other", "", "Bob Schiller", "bob@example.com")
            .await
            .expect("create other building");
        let (_, admins) = get_building(&pool, &other.id)
            .await
            .expect("get other building")
            .unwrap();
        assert_eq!(admins.len(), 1);
        assert_eq!(admins[0].person_id, bob.id);
        assert_eq!(admins[0].name, "Bob Schiller");
        assert_eq!(list_people(&pool).await.expect("list people").len(), 2);

        // Deleting the tenancy removes Bob's person only once the admin row
        // is gone too; deleting the admin row keeps Alice (owner).
        assert!(delete_tenancy(&pool, &tenancy.id)
            .await
            .expect("delete tenancy"));
        assert_eq!(list_people(&pool).await.expect("list people").len(), 2);
        assert!(delete_building(&pool, &other.id)
            .await
            .expect("delete other building"));
        let people = list_people(&pool)
            .await
            .expect("list people after cascades");
        assert_eq!(
            people.len(),
            1,
            "Bob must go with his last reference, got {people:?}"
        );
        assert_eq!(people[0].name, "Alice");
    }

    #[tokio::test]
    async fn person_contact_data_can_be_edited() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");
        let building = create_building(&pool, "Test Building", "", "Alice", "alice@example.com")
            .await
            .expect("create building");
        let apt = create_apartment(
            &pool,
            &building.id,
            "EG",
            "",
            &NewOwner {
                name: "Alice",
                email: "alice@example.com",
                start_date: "2026-01-01",
                end_date: None,
            },
        )
        .await
        .expect("create apartment");
        create_tenancy(&pool, &apt.id, "Bob", "bob@example.com", "2026-02-01", None)
            .await
            .expect("create tenancy");
        let alice = list_people(&pool)
            .await
            .expect("list people")
            .into_iter()
            .find(|p| p.email == "alice@example.com")
            .expect("Alice person");

        // Rename/re-address the person directly; every joined view follows.
        let updated = update_person(&pool, &alice.id, "Alice Liddell", "alice@liddell.example")
            .await
            .expect("update person");
        assert_eq!(updated.name, "Alice Liddell");
        assert_eq!(updated.email, "alice@liddell.example");
        let (_, admins) = get_building(&pool, &building.id)
            .await
            .expect("get building")
            .unwrap();
        assert_eq!(admins[0].name, "Alice Liddell");
        assert_eq!(
            list_ownerships(&pool, &apt.id)
                .await
                .expect("list ownerships")[0]
                .email,
            "alice@liddell.example"
        );

        // The e-mail is the person's identity: an address that already belongs
        // to another person is rejected (0009 trigger).
        let err = update_person(&pool, &alice.id, "Alice Liddell", "bob@example.com")
            .await
            .expect_err("duplicate e-mail must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("existiert bereits"));
        // ...and so are malformed addresses and empty names (people triggers).
        let err = update_person(&pool, &alice.id, "Alice Liddell", "no-at.example")
            .await
            .expect_err("malformed e-mail must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("E-Mail-Adresse"));
        let err = update_person(&pool, &alice.id, "  ", "alice@liddell.example")
            .await
            .expect_err("empty name must be rejected");
        assert!(err
            .as_database_error()
            .expect("database error")
            .message()
            .contains("Name darf nicht leer sein"));
        // A failed update leaves the person untouched.
        let person = get_person(&pool, &alice.id)
            .await
            .expect("get person")
            .expect("Alice exists");
        assert_eq!(person.name, "Alice Liddell");
        assert_eq!(person.email, "alice@liddell.example");

        // Unknown ids yield RowNotFound.
        assert!(matches!(
            update_person(&pool, "does-not-exist", "X", "x@example.com").await,
            Err(sqlx::Error::RowNotFound)
        ));
    }
}
