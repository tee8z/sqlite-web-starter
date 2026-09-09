use super::*;

#[tokio::test]
async fn read_pool_rejects_writes() {
    let directory = tempfile::tempdir().unwrap();
    let (database, writer) = Database::open(&directory.path().join("demo.sqlite"), 8)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE counter SET value = 99")
            .execute(&database.readers)
            .await
            .is_err()
    );
    assert_eq!(database.counter().await.unwrap(), 0);
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    writer.run(shutdown).await.unwrap();
}

#[tokio::test]
async fn migrations_run_once_and_reopening_preserves_changed_seed_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("demo.sqlite");
    let (database, mut writer) = Database::open(&path, 8).await.unwrap();
    let migrations: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&database.readers)
            .await
            .unwrap();
    assert_eq!(migrations, [(1, true)]);
    assert_eq!(database.counter().await.unwrap(), 0);
    let items = database.inventory().await.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(
        (items[0].name.as_str(), items[0].quantity),
        ("Notebooks", 12)
    );
    sqlx::query("UPDATE inventory SET quantity = 7 WHERE id = 1")
        .execute(&mut writer.connection)
        .await
        .unwrap();
    sqlx::query("DELETE FROM inventory WHERE id = 3")
        .execute(&mut writer.connection)
        .await
        .unwrap();
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    writer.run(shutdown).await.unwrap();

    let (reopened, writer) = Database::open(&path, 8).await.unwrap();
    let items = reopened.inventory().await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].quantity, 7);
    let migrations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&reopened.readers)
        .await
        .unwrap();
    assert_eq!(migrations, 1);
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    writer.run(shutdown).await.unwrap();
}

#[tokio::test]
async fn migration_adopts_an_existing_demo_without_overwriting_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("demo.sqlite");
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE counter (
            id INTEGER PRIMARY KEY CHECK (id = 1), value INTEGER NOT NULL
         );
         INSERT INTO counter (id, value) VALUES (1, 33);
         CREATE TABLE inventory (
            id INTEGER PRIMARY KEY, name TEXT NOT NULL, quantity INTEGER NOT NULL
         );
         INSERT INTO inventory (id, name, quantity) VALUES (1, 'Saved notebooks', 7);",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();

    let (database, writer) = Database::open(&path, 8).await.unwrap();
    assert_eq!(database.counter().await.unwrap(), 33);
    let items = database.inventory().await.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(
        (items[0].name.as_str(), items[0].quantity),
        ("Saved notebooks", 7)
    );
    let migrations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = TRUE")
            .fetch_one(&database.readers)
            .await
            .unwrap();
    assert_eq!(migrations, 1);
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    writer.run(shutdown).await.unwrap();
}

#[tokio::test]
async fn concurrent_increments_are_committed_before_reply() {
    let directory = tempfile::tempdir().unwrap();
    let (database, writer) = Database::open(&directory.path().join("demo.sqlite"), 64)
        .await
        .unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(writer.run(shutdown.clone()));
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..64 {
        let database = database.clone();
        requests.spawn(async move {
            let value = database.increment().await.ok().unwrap();
            assert!(database.counter().await.unwrap() >= value);
            value
        });
    }
    let mut values = Vec::new();
    while let Some(result) = requests.join_next().await {
        values.push(result.unwrap());
    }
    values.sort_unstable();
    assert_eq!(values, (1..=64).collect::<Vec<_>>());
    assert_eq!(database.counter().await.unwrap(), 64);
    shutdown.cancel();
    task.await.unwrap().unwrap();
    assert!(!database.is_ready().await);
}

#[tokio::test]
async fn shutdown_drains_admitted_writes_even_when_a_reply_is_dropped() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("demo.sqlite");
    let (database, writer) = Database::open(&path, 4).await.unwrap();
    let mut responses = Vec::new();
    for _ in 0..4 {
        let (reply, response) = oneshot::channel();
        assert!(database.commands.try_send(Command { reply }).is_ok());
        responses.push(response);
    }
    assert!(matches!(
        database.increment().await,
        Err(WriteError::Unavailable)
    ));
    drop(responses.remove(0));
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    writer.run(shutdown).await.unwrap();
    for (index, response) in responses.into_iter().enumerate() {
        assert_eq!(response.await.unwrap().unwrap(), index as i64 + 2);
    }
    let (reopened, writer) = Database::open(&path, 4).await.unwrap();
    assert_eq!(reopened.counter().await.unwrap(), 4);
    let shutdown = CancellationToken::new();
    shutdown.cancel();
    writer.run(shutdown).await.unwrap();
}
