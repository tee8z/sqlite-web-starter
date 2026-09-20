use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, bail};
use sqlx::{
    Connection, SqliteConnection, SqlitePool,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

static MIGRATOR: Migrator = sqlx::migrate!();

pub mod contention;

/// HTTP handlers receive capabilities, never a writable connection or pool.
#[derive(Clone)]
pub struct Database {
    readers: SqlitePool,
    commands: mpsc::Sender<Command>,
    ready: Arc<AtomicBool>,
}

pub struct InventoryItem {
    pub name: String,
    pub quantity: i64,
}

struct Command {
    reply: oneshot::Sender<Result<i64, String>>,
}

pub enum WriteError {
    Unavailable,
    OutcomeUnknown,
    Database(String),
}

pub struct Writer {
    connection: SqliteConnection,
    readers: SqlitePool,
    commands: mpsc::Receiver<Command>,
    ready: Arc<AtomicBool>,
    // Only the isolated contention lab sets a delay; normal writes never sleep.
    transaction_hold: Duration,
}

impl Database {
    pub async fn open(path: &Path, capacity: usize) -> Result<(Self, Writer)> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(5));
        let mut connection = SqliteConnection::connect_with(&options).await?;
        // SQLx's direct entry point also makes this future Send for HTTP callers.
        MIGRATOR.run_direct(None, &mut connection, false).await?;
        let readers = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path)
                    .read_only(true)
                    .pragma("query_only", "ON")
                    .busy_timeout(Duration::from_secs(5)),
            )
            .await?;
        let (commands, receiver) = mpsc::channel(capacity);
        let ready = Arc::new(AtomicBool::new(true));
        let writer = Writer {
            connection,
            readers: readers.clone(),
            commands: receiver,
            ready: ready.clone(),
            transaction_hold: Duration::ZERO,
        };
        Ok((
            Self {
                readers,
                commands,
                ready,
            },
            writer,
        ))
    }

    pub async fn counter(&self) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT value FROM counter WHERE id = 1")
            .fetch_one(&self.readers)
            .await
    }

    pub async fn inventory(&self) -> Result<Vec<InventoryItem>, sqlx::Error> {
        let rows =
            sqlx::query_as::<_, (String, i64)>("SELECT name, quantity FROM inventory ORDER BY id")
                .fetch_all(&self.readers)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(name, quantity)| InventoryItem { name, quantity })
            .collect())
    }

    pub async fn increment(&self) -> Result<i64, WriteError> {
        let (reply, response) = oneshot::channel();
        // Full/closed means rejected before admission. A lost reply after this
        // point means an unknown outcome: the accepted write still executes.
        self.commands
            .try_send(Command { reply })
            .map_err(|_| WriteError::Unavailable)?;
        response
            .await
            .map_err(|_| WriteError::OutcomeUnknown)?
            .map_err(WriteError::Database)
    }

    pub fn stop_readiness(&self) {
        self.ready.store(false, Ordering::Release);
    }

    pub async fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
            && !self.commands.is_closed()
            && self.counter().await.is_ok()
    }
}

impl Writer {
    pub async fn run(mut self, shutdown: CancellationToken) -> Result<()> {
        let result = self.process(shutdown).await;
        self.ready.store(false, Ordering::Release);
        self.commands.close();
        // Close readers first, then the sole writable connection. Litestream
        // attempts its final sync after this child process exits successfully.
        self.readers.close().await;
        let close = self.connection.close().await;
        result?;
        close?;
        Ok(())
    }

    async fn process(&mut self, shutdown: CancellationToken) -> Result<()> {
        loop {
            let command = tokio::select! {
                biased;
                () = shutdown.cancelled() => {
                    self.commands.close();
                    break;
                }
                command = self.commands.recv() => {
                    let Some(command) = command else { bail!("writer channel closed unexpectedly") };
                    command
                }
            };
            self.execute(command).await?;
        }
        // close() rejects new commands but preserves all accepted commands.
        while let Some(command) = self.commands.recv().await {
            self.execute(command).await?;
        }
        Ok(())
    }

    async fn execute(&mut self, command: Command) -> Result<()> {
        let result = increment_connection(&mut self.connection, self.transaction_hold).await;
        match result {
            Ok(value) => {
                // A failed reply means the caller is gone and saw OutcomeUnknown.
                // This is the only place that knows the write actually landed,
                // so record it rather than dropping the outcome on the floor.
                if command.reply.send(Ok(value)).is_err() {
                    tracing::warn!(value, "committed a write whose caller had gone");
                }
            }
            Err(error) => {
                if command.reply.send(Err(error.to_string())).is_err() {
                    tracing::warn!(%error, "write failed and the caller had gone");
                }
                return Err(error.into());
            }
        }
        Ok(())
    }
}

async fn increment_connection(
    connection: &mut SqliteConnection,
    hold: Duration,
) -> Result<i64, sqlx::Error> {
    let mut transaction = connection.begin().await?;
    let value = sqlx::query_scalar::<_, i64>(
        "UPDATE counter SET value = value + 1 WHERE id = 1 RETURNING value",
    )
    .fetch_one(&mut *transaction)
    .await?;
    if !hold.is_zero() {
        tokio::time::sleep(hold).await;
    }
    transaction.commit().await?;
    Ok(value)
}

#[cfg(test)]
mod tests;
