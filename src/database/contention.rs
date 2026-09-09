//! Real lock contention on disposable databases, never on the saved counter.
use std::{path::Path, sync::Arc, time::Instant};

use anyhow::{Result, anyhow};
use serde::Serialize;
use tokio::{sync::Barrier, task::JoinSet};

use super::*;

const WRITERS: usize = 12;
const HOLD: Duration = Duration::from_millis(75);

#[derive(Serialize)]
pub struct Comparison {
    writers: usize,
    hold_ms: u128,
    busy_timeout_ms: u64,
    direct: Round,
    queued: Round,
}

#[derive(Serialize)]
struct Round {
    committed: usize,
    busy: usize,
    final_value: i64,
    elapsed_ms: f64,
    requests: Vec<Request>,
}

#[derive(Serialize)]
struct Request {
    request: usize,
    outcome: Outcome,
    value: Option<i64>,
    started_ms: f64,
    elapsed_ms: f64,
}

#[derive(Serialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Committed,
    Busy,
}

pub async fn run() -> Result<Comparison> {
    let directory = tempfile::tempdir()?;
    let direct = direct(&directory.path().join("direct.sqlite")).await?;
    let queued = queued(&directory.path().join("queued.sqlite")).await?;
    Ok(Comparison {
        writers: WRITERS,
        hold_ms: HOLD.as_millis(),
        busy_timeout_ms: 0,
        direct,
        queued,
    })
}

async fn direct(path: &Path) -> Result<Round> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::ZERO);
    // Keep the setup connection open to read the final value after the burst.
    let mut observer = SqliteConnection::connect_with(&options).await?;
    MIGRATOR.run_direct(None, &mut observer, false).await?;
    let mut connections = Vec::with_capacity(WRITERS);
    for _ in 0..WRITERS {
        connections.push(SqliteConnection::connect_with(&options).await?);
    }

    let barrier = Arc::new(Barrier::new(WRITERS));
    let mut callers = JoinSet::new();
    let start = Instant::now();
    for (index, mut connection) in connections.into_iter().enumerate() {
        let barrier = barrier.clone();
        callers.spawn(async move {
            barrier.wait().await;
            let requested = Instant::now();
            let result = increment_connection(&mut connection, HOLD).await;
            let request = Request::record(index + 1, start, requested, result);
            connection.close().await?;
            request
        });
    }
    let requests = collect(&mut callers).await?;
    let elapsed = start.elapsed();
    let final_value = sqlx::query_scalar("SELECT value FROM counter WHERE id = 1")
        .fetch_one(&mut observer)
        .await?;
    observer.close().await?;
    Ok(Round::new(requests, final_value, elapsed))
}

async fn queued(path: &Path) -> Result<Round> {
    let (database, mut writer) = Database::open(path, WRITERS).await?;
    sqlx::query("PRAGMA busy_timeout = 0")
        .execute(&mut writer.connection)
        .await?;
    writer.transaction_hold = HOLD;
    let shutdown = CancellationToken::new();
    let requests = async {
        let barrier = Arc::new(Barrier::new(WRITERS));
        let mut callers = JoinSet::new();
        let start = Instant::now();
        for request in 1..=WRITERS {
            let database = database.clone();
            let barrier = barrier.clone();
            callers.spawn(async move {
                barrier.wait().await;
                let requested = Instant::now();
                // Use the application's actual mpsc admission and oneshot reply.
                let value = database.increment().await.map_err(|error| match error {
                    WriteError::Unavailable => anyhow!("lab write queue unavailable"),
                    WriteError::OutcomeUnknown => anyhow!("lab write outcome unknown"),
                    WriteError::Database(error) => anyhow!(error),
                })?;
                Request::record(request, start, requested, Ok(value))
            });
        }
        let requests = collect(&mut callers).await?;
        let elapsed = start.elapsed();
        let final_value = database.counter().await?;
        shutdown.cancel();
        Ok::<_, anyhow::Error>(Round::new(requests, final_value, elapsed))
    };
    // The writer is scoped to this future. Cancellation cannot detach a writer
    // task or keep accepting work after the experiment's HTTP handler is gone.
    let ((), round) = tokio::try_join!(writer.run(shutdown.clone()), requests)?;
    Ok(round)
}

async fn collect(callers: &mut JoinSet<Result<Request>>) -> Result<Vec<Request>> {
    let mut requests = Vec::with_capacity(WRITERS);
    while let Some(request) = callers.join_next().await {
        requests.push(request??);
    }
    requests.sort_unstable_by_key(|request| request.request);
    Ok(requests)
}

impl Request {
    fn record(
        request: usize,
        start: Instant,
        requested: Instant,
        result: Result<i64, sqlx::Error>,
    ) -> Result<Self> {
        let elapsed_ms = requested.elapsed().as_secs_f64() * 1000.0;
        let (outcome, value) = match result {
            Ok(value) => (Outcome::Committed, Some(value)),
            // Match SQLITE_BUSY (5), including extended BUSY codes. Other
            // failures are experiment errors, not evidence of lock contention.
            Err(sqlx::Error::Database(error))
                if error
                    .code()
                    .and_then(|code| code.parse::<i32>().ok())
                    .is_some_and(|code| code & 0xff == 5) =>
            {
                (Outcome::Busy, None)
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            request,
            outcome,
            value,
            started_ms: requested.duration_since(start).as_secs_f64() * 1000.0,
            elapsed_ms,
        })
    }
}

impl Round {
    fn new(requests: Vec<Request>, final_value: i64, elapsed: Duration) -> Self {
        let committed = requests
            .iter()
            .filter(|request| request.outcome == Outcome::Committed)
            .count();
        let busy = requests.len() - committed;
        Self {
            committed,
            busy,
            final_value,
            elapsed_ms: elapsed.as_secs_f64() * 1000.0,
            requests,
        }
    }
}

#[cfg(test)]
mod tests;
