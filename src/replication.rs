use std::{env, path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use tokio::{process::Command, time::Instant};
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct SyncResult {
    txid: u64,
    replica_txid: u64,
}

/// The container entrypoint enables this barrier; local `cargo run` needs no daemon.
pub async fn wait_until_ready(path: &Path, shutdown: &CancellationToken) -> Result<()> {
    let Some(socket) = env::var_os("LITESTREAM_SOCKET") else {
        return Ok(());
    };
    ensure!(!socket.is_empty(), "LITESTREAM_SOCKET must not be empty");
    let seconds: u64 = env::var("LITESTREAM_STARTUP_TIMEOUT_SECONDS")
        .unwrap_or_else(|_| "60".into())
        .parse()
        .context("invalid LITESTREAM_STARTUP_TIMEOUT_SECONDS")?;
    ensure!(
        seconds > 0,
        "LITESTREAM_STARTUP_TIMEOUT_SECONDS must be positive"
    );
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last_error = "no sync completed".to_owned();
    tracing::info!("waiting for Litestream's initial replica sync");

    loop {
        // -wait syncs the WAL locally, then uploads it. A socket connection or
        // a zero transaction ID alone does not establish database initialization.
        let mut command = Command::new("litestream");
        command
            .args(["sync", "-wait", "-json", "-timeout", "5", "-socket"])
            .arg(&socket)
            .arg(path)
            .kill_on_drop(true);
        let output = tokio::select! {
            biased;
            () = shutdown.cancelled() => return Ok(()),
            () = tokio::time::sleep_until(deadline) => {
                bail!("Litestream startup sync exceeded {seconds}s: {last_error}")
            }
            output = command.output() => output.context("run litestream sync")?,
        };
        if output.status.success() {
            let sync: SyncResult = serde_json::from_slice(&output.stdout)
                .context("invalid litestream sync response")?;
            if sync.txid > 0 && sync.replica_txid >= sync.txid {
                tracing::info!(txid = sync.txid, "Litestream initial replica sync complete");
                return Ok(());
            }
            last_error = format!(
                "incomplete sync: local txid {}, replica txid {}",
                sync.txid, sync.replica_txid
            );
        } else {
            last_error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        }
        // Retry failed syncs within a deadline; elapsed time never grants readiness.
        tokio::select! {
            biased;
            () = shutdown.cancelled() => return Ok(()),
            () = tokio::time::sleep_until(deadline) => {
                bail!("Litestream startup sync exceeded {seconds}s: {last_error}")
            }
            () = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
}
