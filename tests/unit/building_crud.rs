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

    let updated = queries::update_building(
        &pool,
        &building.id,
        "Haus Regenbogen",
        "A nicer building",
        None,
    )
    .await
    .expect("update building");
    assert_eq!(updated.name, "Haus Regenbogen");
    assert_eq!(updated.description, "A nicer building");

    let got_after_update = queries::get_building(&pool, &building.id)
        .await
        .expect("get building after update")
        .expect("building exists");
    assert_eq!(got_after_update.0.name, "Haus Regenbogen");
    assert_eq!(got_after_update.0.description, "A nicer building");

    // With a contact given, the update also sets the Ansprechpartner (the
    // oldest administrator row) in the same transaction.
    queries::update_building(
        &pool,
        &building.id,
        "Haus Regenbogen",
        "A nicer building",
        Some(("Bob", "bob@example.com")),
    )
    .await
    .expect("update building and administrator");
    let with_bob = queries::get_building(&pool, &building.id)
        .await
        .expect("get building")
        .expect("building exists");
    assert_eq!(with_bob.1.len(), 1);
    assert_eq!(with_bob.1[0].name, "Bob");
    assert_eq!(with_bob.1[0].email, "bob@example.com");

    // A building without any administrator row: the update inserts one.
    sqlx::query("DELETE FROM building_administrators WHERE building_id = ?")
        .bind(&building.id)
        .execute(&pool)
        .await
        .expect("delete administrators");
    queries::update_building(
        &pool,
        &building.id,
        "Haus Regenbogen",
        "A nicer building",
        Some(("Carol", "carol@example.com")),
    )
    .await
    .expect("update building and insert administrator");
    let with_carol = queries::get_building(&pool, &building.id)
        .await
        .expect("get building")
        .expect("building exists");
    assert_eq!(with_carol.1.len(), 1);
    assert_eq!(with_carol.1[0].name, "Carol");
    assert_eq!(with_carol.1[0].email, "carol@example.com");

    let by_slug = queries::get_building_by_slug(&pool, &building.secret_slug)
        .await
        .expect("get by slug")
        .expect("building exists");
    assert_eq!(by_slug.id, building.id);

    // The Ansprechpartner is optional: blank contact fields create a building
    // without an administrator row.
    let contactless = queries::create_building(&pool, "Haus Kontaktlos", "", "", "")
        .await
        .expect("create building without contact");
    let got_contactless = queries::get_building(&pool, &contactless.id)
        .await
        .expect("get contactless building")
        .expect("building exists");
    assert_eq!(got_contactless.0.name, "Haus Kontaktlos");
    assert_eq!(got_contactless.1.len(), 0);

    // A partially filled contact is rejected by the database triggers.
    let partial = queries::create_building(&pool, "Haus Halber Kontakt", "", "Dana", "")
        .await
        .expect_err("name without e-mail must be rejected");
    assert!(
        partial
            .as_database_error()
            .map(|e| e.message().contains("E-Mail-Adresse"))
            .unwrap_or(false),
        "expected an e-mail validation message, got {partial:?}"
    );
    let buildings_after_partial = queries::list_buildings(&pool)
        .await
        .expect("list buildings after partial create");
    assert_eq!(
        buildings_after_partial.len(),
        2,
        "failed create leaves no row"
    );

    let deleted = queries::delete_building(&pool, &building.id)
        .await
        .expect("delete building");
    assert!(deleted);
    let deleted_contactless = queries::delete_building(&pool, &contactless.id)
        .await
        .expect("delete contactless building");
    assert!(deleted_contactless);

    let buildings_after = queries::list_buildings(&pool)
        .await
        .expect("list buildings after delete");
    assert_eq!(buildings_after.len(), 0);
}
