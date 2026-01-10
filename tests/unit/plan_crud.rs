use kehrkraft::db::{migrate, Db};
use kehrkraft::db::queries;
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn plan_crud_works() {
    let pool: Db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory");

    migrate(&pool).await.expect("apply migrations");

    let plan = queries::create_plan(&pool, "Test Plan", "Alice", "alice@example.com")
        .await
        .expect("create plan");

    let plans = queries::list_plans(&pool).await.expect("list plans");
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].id, plan.id);

    let got = queries::get_plan(&pool, &plan.id)
        .await
        .expect("get plan")
        .expect("plan exists");
    assert_eq!(got.0.name, "Test Plan");
    assert_eq!(got.1.len(), 1);
    assert_eq!(got.1[0].name, "Alice");

    let deleted = queries::delete_plan(&pool, &plan.id).await.expect("delete plan");
    assert!(deleted);

    let plans_after = queries::list_plans(&pool).await.expect("list plans after delete");
    assert_eq!(plans_after.len(), 0);
}
