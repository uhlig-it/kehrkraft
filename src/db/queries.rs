use base64::Engine as _;
use rand::RngCore;
use sqlx::SqlitePool;

use crate::db::models::{Plan, PlanAdministrator, Tenant};
use crate::db::Db;

fn gen_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn list_plans(pool: &Db) -> Result<Vec<Plan>, sqlx::Error> {
    sqlx::query_as::<_, Plan>(
        r#"
        SELECT id, name, secret_slug, rotation_seed, created_at, updated_at
        FROM plans
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await
}

pub async fn get_plan(
    pool: &Db,
    id: &str,
) -> Result<Option<(Plan, Vec<PlanAdministrator>)>, sqlx::Error> {
    let plan_opt = sqlx::query_as::<_, Plan>(
        r#"
        SELECT id, name, secret_slug, rotation_seed, created_at, updated_at
        FROM plans
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    if let Some(plan) = plan_opt {
        let admins = sqlx::query_as::<_, PlanAdministrator>(
            r#"
            SELECT id, plan_id, name, email, created_at
            FROM plan_administrators
            WHERE plan_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(&plan.id)
        .fetch_all(pool)
        .await?;
        Ok(Some((plan, admins)))
    } else {
        Ok(None)
    }
}

pub async fn get_plan_by_slug(pool: &Db, slug: &str) -> Result<Option<Plan>, sqlx::Error> {
    sqlx::query_as::<_, Plan>(
        r#"
        SELECT id, name, secret_slug, rotation_seed, created_at, updated_at
        FROM plans
        WHERE secret_slug = ?
        "#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
}

pub async fn create_plan(
    pool: &SqlitePool,
    name: &str,
    admin_name: &str,
    admin_email: &str,
) -> Result<Plan, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let plan_id = gen_token();
    let secret_slug = gen_token();

    sqlx::query(
        r#"
        INSERT INTO plans (id, name, secret_slug, rotation_seed)
        VALUES (?, ?, ?, 0)
        "#,
    )
    .bind(&plan_id)
    .bind(name)
    .bind(&secret_slug)
    .execute(&mut *tx)
    .await?;

    let admin_id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO plan_administrators (id, plan_id, name, email)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(&admin_id)
    .bind(&plan_id)
    .bind(admin_name)
    .bind(admin_email)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let plan = sqlx::query_as::<_, Plan>(
        r#"
        SELECT id, name, secret_slug, rotation_seed, created_at, updated_at
        FROM plans
        WHERE id = ?
        "#,
    )
    .bind(&plan_id)
    .fetch_one(pool)
    .await?;

    Ok(plan)
}

pub async fn delete_plan(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM plans WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// Tenants CRUD

pub async fn list_tenants(pool: &Db, plan_id: &str) -> Result<Vec<Tenant>, sqlx::Error> {
    sqlx::query_as::<_, Tenant>(
        r#"
        SELECT id, plan_id, name, email, start_date, end_date, created_at
        FROM tenants
        WHERE plan_id = ?
        ORDER BY start_date ASC, name ASC
        "#,
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await
}

pub async fn get_tenant(pool: &Db, id: &str) -> Result<Option<Tenant>, sqlx::Error> {
    sqlx::query_as::<_, Tenant>(
        r#"
        SELECT id, plan_id, name, email, start_date, end_date, created_at
        FROM tenants
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn create_tenant(
    pool: &Db,
    plan_id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Tenant, sqlx::Error> {
    let id = gen_token();
    sqlx::query(
        r#"
        INSERT INTO tenants (id, plan_id, name, email, start_date, end_date)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(plan_id)
    .bind(name)
    .bind(email)
    .bind(start_date)
    .bind(end_date)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, Tenant>(
        r#"
        SELECT id, plan_id, name, email, start_date, end_date, created_at
        FROM tenants
        WHERE id = ?
        "#,
    )
    .bind(&id)
    .fetch_one(pool)
    .await
}

pub async fn update_tenant(
    pool: &Db,
    id: &str,
    name: &str,
    email: &str,
    start_date: &str,
    end_date: Option<&str>,
) -> Result<Tenant, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE tenants
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

    sqlx::query_as::<_, Tenant>(
        r#"
        SELECT id, plan_id, name, email, start_date, end_date, created_at
        FROM tenants
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn delete_tenant(pool: &Db, id: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM tenants WHERE id = ?")
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
    async fn tenants_crud_works() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory");

        migrate(&pool).await.expect("migrate");

        // Create plan to attach tenants
        let plan = create_plan(&pool, "Test Plan", "Alice", "alice@example.com")
            .await
            .expect("create plan");

        // Create tenant
        let t = create_tenant(
            &pool,
            &plan.id,
            "Bob",
            "bob@example.com",
            "2024-01-01",
            None,
        )
        .await
        .expect("create tenant");

        assert_eq!(t.name, "Bob");
        assert_eq!(t.plan_id, plan.id);

        // List tenants
        let list = list_tenants(&pool, &plan.id).await.expect("list tenants");
        assert_eq!(list.len(), 1);

        // Update tenant
        let t2 = update_tenant(
            &pool,
            &t.id,
            "Bobby",
            "bobby@example.com",
            "2024-01-01",
            Some("2024-12-31"),
        )
        .await
        .expect("update tenant");
        assert_eq!(t2.name, "Bobby");
        assert_eq!(t2.end_date.as_deref(), Some("2024-12-31"));

        // Delete tenant
        let deleted = delete_tenant(&pool, &t2.id).await.expect("delete tenant");
        assert!(deleted);

        let list2 = list_tenants(&pool, &plan.id).await.expect("list tenants 2");
        assert_eq!(list2.len(), 0);
    }
}
