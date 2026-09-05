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

pub async fn create_apartment(
    pool: &Db,
    building_id: &str,
    name: &str,
    description: &str,
) -> Result<Apartment, sqlx::Error> {
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
    .execute(pool)
    .await?;

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
pub async fn reorder_apartments(
    pool: &Db,
    building_id: &str,
    ordered_ids: &[String],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    for (index, id) in ordered_ids.iter().enumerate() {
        sqlx::query("UPDATE apartments SET position = ? WHERE id = ? AND building_id = ?")
            .bind(index as i64)
            .bind(id)
            .bind(building_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
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

        let apt = create_apartment(&pool, &building.id, "EG links", "Ground floor left")
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

        // Ownership
        let o = create_ownership(&pool, &apt.id, "Bob", "bob@example.com", "2024-01-01", None)
            .await
            .expect("create ownership");
        assert_eq!(o.name, "Bob");
        let o_list = list_ownerships(&pool, &apt.id)
            .await
            .expect("list ownerships");
        assert_eq!(o_list.len(), 1);
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
        assert_eq!(all_o.len(), 1);
        let all_t = list_tenancies_for_building(&pool, &building.id)
            .await
            .expect("tenancies for building");
        assert_eq!(all_t.len(), 1);

        // Delete tenancy and ownership; apartment delete cascades the rest
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

        let a = create_apartment(&pool, &building.id, "Erdgeschoss", "")
            .await
            .expect("create apartment");
        let b = create_apartment(&pool, &building.id, "Obergeschoss", "")
            .await
            .expect("create apartment");
        let c = create_apartment(&pool, &building.id, "Dachgeschoss", "")
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
        reorder_apartments(
            &pool,
            &building.id,
            &[c.id.clone(), a.id.clone(), b.id.clone()],
        )
        .await
        .expect("reorder apartments");

        let reordered: Vec<String> = list_apartments(&pool, &building.id)
            .await
            .expect("list apartments after reorder")
            .into_iter()
            .map(|ap| ap.id)
            .collect();
        assert_eq!(reordered, vec![c.id, a.id, b.id]);
    }
}
