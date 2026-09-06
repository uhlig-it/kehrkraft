use base64::Engine as _;
use rand::RngCore;
use sqlx::SqlitePool;

use crate::db::models::{Apartment, Building, BuildingAdministrator, Ownership, Tenancy};
use crate::db::Db;

fn gen_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
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
            SELECT id, building_id, name, email, created_at
            FROM building_administrators
            WHERE building_id = ?
            ORDER BY created_at ASC
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

pub async fn create_building(
    pool: &SqlitePool,
    name: &str,
    description: &str,
    admin_name: &str,
    admin_email: &str,
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

    let admin_id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO building_administrators (id, building_id, name, email)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(&admin_id)
    .bind(&building_id)
    .bind(admin_name)
    .bind(admin_email)
    .execute(&mut *tx)
    .await?;

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

pub async fn delete_building(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM buildings WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
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

/// The initial ownership record that must be created together with its
/// apartment; the database rejects apartments without an ownership record
/// (see the `apartments_require_ownership` trigger in 0004_validation_in_db.sql).
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

    // The ownerships FK is DEFERRABLE INITIALLY DEFERRED, so the first
    // ownership may (and must, see the trigger above) be inserted before its
    // apartment within the same transaction.
    let ownership_id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO ownerships (id, apartment_id, name, email, start_date, end_date)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&ownership_id)
    .bind(&id)
    .bind(initial_owner.name)
    .bind(initial_owner.email)
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
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM ownerships
        WHERE apartment_id = ?
        ORDER BY start_date ASC, name ASC
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
        SELECT o.id, o.apartment_id, o.name, o.email, o.start_date, o.end_date, o.created_at
        FROM ownerships o
        INNER JOIN apartments a ON a.id = o.apartment_id
        WHERE a.building_id = ?
        ORDER BY o.start_date ASC, o.name ASC
        "#,
    )
    .bind(building_id)
    .fetch_all(pool)
    .await
}

pub async fn get_ownership(pool: &Db, id: &str) -> Result<Option<Ownership>, sqlx::Error> {
    sqlx::query_as::<_, Ownership>(
        r#"
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM ownerships
        WHERE id = ?
        "#,
    )
    .bind(id)
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
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO ownerships (id, apartment_id, name, email, start_date, end_date)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(apartment_id)
    .bind(name)
    .bind(email)
    .bind(start_date)
    .bind(end_date)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Ownership>(
        r#"
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM ownerships
        WHERE id = ?
        "#,
    )
    .bind(&id)
    .fetch_one(pool)
    .await
}

pub async fn update_ownership(
    pool: &Db,
    id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Ownership, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE ownerships
        SET name = ?, email = ?, start_date = ?, end_date = ?
        WHERE id = ?
        "#,
    )
    .bind(name)
    .bind(email)
    .bind(start_date)
    .bind(end_date)
    .bind(id)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Ownership>(
        r#"
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM ownerships
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn delete_ownership(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM ownerships WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// Tenancies CRUD

pub async fn list_tenancies(pool: &Db, apartment_id: &str) -> Result<Vec<Tenancy>, sqlx::Error> {
    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM tenancies
        WHERE apartment_id = ?
        ORDER BY start_date ASC, name ASC
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
        SELECT t.id, t.apartment_id, t.name, t.email, t.start_date, t.end_date, t.created_at
        FROM tenancies t
        INNER JOIN apartments a ON a.id = t.apartment_id
        WHERE a.building_id = ?
        ORDER BY t.start_date ASC, t.name ASC
        "#,
    )
    .bind(building_id)
    .fetch_all(pool)
    .await
}

pub async fn get_tenancy(pool: &Db, id: &str) -> Result<Option<Tenancy>, sqlx::Error> {
    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM tenancies
        WHERE id = ?
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
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO tenancies (id, apartment_id, name, email, start_date, end_date)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(apartment_id)
    .bind(name)
    .bind(email)
    .bind(start_date)
    .bind(end_date)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM tenancies
        WHERE id = ?
        "#,
    )
    .bind(&id)
    .fetch_one(pool)
    .await
}

pub async fn update_tenancy(
    pool: &Db,
    id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Tenancy, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE tenancies
        SET name = ?, email = ?, start_date = ?, end_date = ?
        WHERE id = ?
        "#,
    )
    .bind(name)
    .bind(email)
    .bind(start_date)
    .bind(end_date)
    .bind(id)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Tenancy>(
        r#"
        SELECT id, apartment_id, name, email, start_date, end_date, created_at
        FROM tenancies
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn delete_tenancy(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM tenancies WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
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
}
