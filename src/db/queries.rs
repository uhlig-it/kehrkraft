use base64::Engine as _;
use rand::RngCore;
use sqlx::SqlitePool;

use crate::db::models::{Plan, PlanAdministrator};
use crate::db::Db;

fn gen_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
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

pub async fn get_plan(pool: &Db, id: &str) -> Result<Option<(Plan, Vec<PlanAdministrator>)>, sqlx::Error> {
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
