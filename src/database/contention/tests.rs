use super::*;

#[tokio::test]
async fn comparison_measures_contention_and_serializes_every_queued_write() -> Result<()> {
    let comparison = run().await?;

    assert_eq!(comparison.writers, WRITERS);
    assert_eq!(comparison.busy_timeout_ms, 0);
    assert!(comparison.direct.committed > 0);
    assert!(comparison.direct.busy > 0);
    assert_eq!(comparison.queued.committed, WRITERS);
    assert_eq!(comparison.queued.busy, 0);

    for round in [&comparison.direct, &comparison.queued] {
        assert_eq!(round.requests.len(), WRITERS);
        assert_eq!(round.committed + round.busy, WRITERS);
        assert_eq!(round.final_value, round.committed as i64);
        assert_eq!(
            round.requests.iter().map(|r| r.request).collect::<Vec<_>>(),
            (1..=WRITERS).collect::<Vec<_>>()
        );
        let mut values = Vec::new();
        for request in &round.requests {
            match request.outcome {
                Outcome::Committed => values.push(request.value.expect("committed value")),
                Outcome::Busy => assert_eq!(request.value, None),
            }
        }
        values.sort_unstable();
        assert_eq!(values, (1..=round.committed as i64).collect::<Vec<_>>());
    }
    Ok(())
}

#[tokio::test]
async fn busy_errors_are_measured_but_other_database_errors_propagate() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let options = SqliteConnectOptions::new()
        .filename(directory.path().join("locks.sqlite"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::ZERO);
    let mut holder = SqliteConnection::connect_with(&options).await?;
    MIGRATOR.run_direct(None, &mut holder, false).await?;
    let mut contender = SqliteConnection::connect_with(&options).await?;

    // Acquire the write lock before trying the competing connection so this
    // assertion does not depend on task scheduling or an artificial delay.
    sqlx::query("BEGIN IMMEDIATE").execute(&mut holder).await?;
    let start = Instant::now();
    let result = increment_connection(&mut contender, Duration::ZERO).await;
    let request = Request::record(1, start, start, result)?;
    assert_eq!(request.outcome, Outcome::Busy);
    assert_eq!(request.value, None);
    sqlx::query("ROLLBACK").execute(&mut holder).await?;

    sqlx::query("DROP TABLE counter")
        .execute(&mut holder)
        .await?;
    let start = Instant::now();
    let result = increment_connection(&mut contender, Duration::ZERO).await;
    let error = Request::record(2, start, start, result)
        .err()
        .expect("missing table must fail the experiment, not count as busy");
    assert!(matches!(
        error.downcast_ref::<sqlx::Error>(),
        Some(sqlx::Error::Database(error)) if error.code().as_deref() == Some("1")
    ));

    contender.close().await?;
    holder.close().await?;
    Ok(())
}
