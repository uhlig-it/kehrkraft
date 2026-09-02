use kehrkraft::db::queries;
use kehrkraft::db::{migrate, Db};
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn building_crud_works() {
    let pool: Db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect in-memory");

    migrate(&pool).await.expect("apply migrations");

    let building = queries::create_building(
        &pool,
        "Haus Sonnenschein",
        "A nice building",
        "Alice",
        "alice@example.com",
    )
    .await
    .expect("create building");

    let buildings = queries::list_buildings(&pool)
        .await
        .expect("list buildings");
    assert_eq!(buildings.len(), 1);
    assert_eq!(buildings[0].id, building.id);

    let got = queries::get_building(&pool, &building.id)
        .await
        .expect("get building")
        .expect("building exists");
    assert_eq!(got.0.name, "Haus Sonnenschein");
    assert_eq!(got.0.description, "A nice building");
    assert_eq!(got.1.len(), 1);
    assert_eq!(got.1[0].name, "Alice");

    let by_slug = queries::get_building_by_slug(&pool, &building.secret_slug)
        .await
        .expect("get by slug")
        .expect("building exists");
    assert_eq!(by_slug.id, building.id);

    let deleted = queries::delete_building(&pool, &building.id)
        .await
        .expect("delete building");
    assert!(deleted);

    let buildings_after = queries::list_buildings(&pool)
        .await
        .expect("list buildings after delete");
    assert_eq!(buildings_after.len(), 0);
}
