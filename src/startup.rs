use std::{env, io::IsTerminal, net::Ipv4Addr, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow};
use axum::Router;
use tokio::{
    net::TcpListener,
    task::{JoinError, JoinHandle},
};
use tokio_util::sync::CancellationToken;

use crate::{database::Database, replication, routes};

type TaskResult = Result<Result<()>, JoinError>;

struct Configuration {
    sqlite_path: PathBuf,
    port: u16,
    shutdown_timeout: Duration,
}

impl Configuration {
    fn from_env() -> Result<Self> {
        let sqlite_path = env::var("SQLITE_PATH").unwrap_or_else(|_| "./data/demo.sqlite".into());
        let port = env::var("PORT")
            .unwrap_or_else(|_| "3000".into())
            .parse()
            .context("PORT must be a valid TCP port")?;
        let timeout: u64 = env::var("SHUTDOWN_TIMEOUT_SECONDS")
            .unwrap_or_else(|_| "25".into())
            .parse()
            .context("SHUTDOWN_TIMEOUT_SECONDS must be an integer")?;
        anyhow::ensure!(timeout > 0, "SHUTDOWN_TIMEOUT_SECONDS must be positive");
        Ok(Self {
            sqlite_path: sqlite_path.into(),
            port,
            shutdown_timeout: Duration::from_secs(timeout),
        })
    }
}

/// Owns process resources. Build initializes dependencies and binds listeners;
/// run_until_stop supervises tasks and drains HTTP before the database writer.
struct Application {
    runtime: ApplicationRuntime,
}

struct ApplicationRuntime {
    database: Database,
    cancellation: CancellationToken,
    database_shutdown: CancellationToken,
    requested: CancellationToken,
    shutdown_timeout: Duration,
    http: Option<JoinHandle<Result<()>>>,
    writer: Option<JoinHandle<Result<()>>>,
}

pub async fn run_until_stop() -> Result<()> {
    // Register before initialization so a TERM received during startup persists.
    let requested = CancellationToken::new();
    let signal_task = install_termination_signal(requested.clone())?;
    let result = async {
        let ansi = match env::var("RUST_LOG_STYLE").as_deref() {
            Ok("always") => true,
            Ok("never") => false,
            _ => {
                std::io::stderr().is_terminal()
                    && env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
            }
        };
        tracing_subscriber::fmt()
            .compact()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with_writer(std::io::stderr)
            .with_ansi(ansi)
            .try_init()
            .map_err(|error| anyhow!("initialize tracing: {error}"))?;
        let configuration = Configuration::from_env()?;
        if let Some(application) = Application::build(configuration, requested).await? {
            application.run_until_stop().await?;
        }
        Ok(())
    }
    .await;
    signal_task.abort();
    let _ = signal_task.await;
    result
}

impl Application {
    async fn build(
        configuration: Configuration,
        requested: CancellationToken,
    ) -> Result<Option<Self>> {
        if requested.is_cancelled() {
            return Ok(None);
        }
        let (database, writer) = Database::open(&configuration.sqlite_path, 64)
            .await
            .context("initialize SQLite")?;
        let database_shutdown = CancellationToken::new();
        let writer = tokio::spawn(writer.run(database_shutdown.clone()));
        let mut runtime = ApplicationRuntime {
            database,
            cancellation: CancellationToken::new(),
            database_shutdown,
            requested,
            shutdown_timeout: configuration.shutdown_timeout,
            http: None,
            writer: Some(writer),
        };

        let prepare = async {
            replication::wait_until_ready(&configuration.sqlite_path, &runtime.requested)
                .await
                .context("initialize Litestream replication")?;
            if runtime.requested.is_cancelled() {
                return Ok(None);
            }
            let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, configuration.port))
                .await
                .context("bind HTTP listener")?;
            let address = listener
                .local_addr()
                .context("read HTTP listener address")?;
            Ok(Some((listener, address)))
        };
        let prepared = tokio::select! {
            biased;
            () = runtime.requested.cancelled() => Ok(None),
            completion = wait_for_task(&mut runtime.writer) => {
                Err(task_failure("database writer", completion, true).unwrap())
            }
            result = prepare => result,
        };
        let (listener, address) = match prepared {
            Ok(Some(listener)) if !runtime.requested.is_cancelled() => listener,
            result => {
                // Startup failures and signals must still close SQLite cleanly.
                let cleanup = runtime.shutdown().await;
                return first_failure(result.map(|_| ()), cleanup).map(|()| None);
            }
        };
        runtime.http = Some(spawn_http(
            listener,
            routes::router(runtime.database.clone()),
            runtime.cancellation.clone(),
        ));
        tracing::info!(
            %address,
            sqlite_path = %configuration.sqlite_path.display(),
            "HTTP server listening; open http://localhost:{}/",
            address.port()
        );
        Ok(Some(Self { runtime }))
    }

    async fn run_until_stop(self) -> Result<()> {
        self.runtime.run_until_stop().await
    }
}

impl ApplicationRuntime {
    async fn run_until_stop(mut self) -> Result<()> {
        let failure = tokio::select! {
            biased;
            () = self.requested.cancelled() => None,
            completion = wait_for_task(&mut self.http) => {
                task_failure("HTTP", completion, true)
            }
            completion = wait_for_task(&mut self.writer) => {
                task_failure("database writer", completion, true)
            }
        };
        first_failure(failure.map_or(Ok(()), Err), self.shutdown().await)
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.database.stop_readiness();
        self.cancellation.cancel();
        let timeout = self.shutdown_timeout;
        match tokio::time::timeout(timeout, self.drain()).await {
            Ok(result) => result,
            Err(_) => {
                self.database_shutdown.cancel();
                self.abort_tasks();
                if self.http.is_some() {
                    let _ = wait_for_task(&mut self.http).await;
                }
                if self.writer.is_some() {
                    let _ = wait_for_task(&mut self.writer).await;
                }
                Err(anyhow!(
                    "shutdown exceeded {}s; accepted write outcomes may be unknown",
                    timeout.as_secs_f64()
                ))
            }
        }
    }

    async fn drain(&mut self) -> Result<()> {
        let http_failure = if self.http.is_some() {
            task_failure("HTTP", wait_for_task(&mut self.http).await, false)
        } else {
            None
        };
        // HTTP handlers may still submit writes while draining. Only stop the
        // writer after every handler finishes, then await its connection closure.
        self.database_shutdown.cancel();
        let writer_failure = if self.writer.is_some() {
            task_failure(
                "database writer",
                wait_for_task(&mut self.writer).await,
                false,
            )
        } else {
            None
        };
        first_failure(
            http_failure.map_or(Ok(()), Err),
            writer_failure.map_or(Ok(()), Err),
        )
    }

    fn abort_tasks(&self) {
        for task in [&self.http, &self.writer].into_iter().flatten() {
            task.abort();
        }
    }
}

impl Drop for ApplicationRuntime {
    fn drop(&mut self) {
        // Dropping a JoinHandle detaches it; never leave an unsupervised task.
        self.abort_tasks();
    }
}

fn spawn_http(
    listener: TcpListener,
    router: Router,
    shutdown: CancellationToken,
) -> JoinHandle<Result<()>> {
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .context("serve HTTP")
    })
}

async fn wait_for_task(task: &mut Option<JoinHandle<Result<()>>>) -> TaskResult {
    let result = match task.as_mut() {
        Some(task) => task.await,
        None => std::future::pending().await,
    };
    task.take();
    result
}

fn task_failure(name: &str, result: TaskResult, unexpected: bool) -> Option<anyhow::Error> {
    match result {
        Ok(Ok(())) if unexpected => Some(anyhow!("{name} task stopped unexpectedly")),
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.context(format!("{name} task failed"))),
        Err(error) => Some(anyhow!(error).context(format!("{name} task panicked or was aborted"))),
    }
}

fn first_failure(first: Result<()>, second: Result<()>) -> Result<()> {
    match (first, second) {
        (Err(error), Err(additional)) => {
            tracing::error!(error = %format_args!("{additional:#}"), "additional shutdown failure");
            Err(error)
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(unix)]
fn install_termination_signal(requested: CancellationToken) -> Result<JoinHandle<()>> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(tokio::spawn(async move {
        tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} }
        requested.cancel();
    }))
}

#[cfg(not(unix))]
fn install_termination_signal(requested: CancellationToken) -> Result<JoinHandle<()>> {
    Ok(tokio::spawn(async move {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "termination signal failed");
        }
        requested.cancel();
    }))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::SocketAddr,
        path::Path,
        sync::Arc,
    };

    use axum::routing::post;
    use tokio::sync::Notify;

    use super::*;

    async fn runtime(path: &Path) -> ApplicationRuntime {
        let (database, writer) = Database::open(path, 8).await.unwrap();
        let database_shutdown = CancellationToken::new();
        ApplicationRuntime {
            database,
            cancellation: CancellationToken::new(),
            database_shutdown: database_shutdown.clone(),
            requested: CancellationToken::new(),
            shutdown_timeout: Duration::from_secs(3),
            http: None,
            writer: Some(tokio::spawn(writer.run(database_shutdown))),
        }
    }

    async fn serve(runtime: &mut ApplicationRuntime, router: Router) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        runtime.http = Some(spawn_http(listener, router, runtime.cancellation.clone()));
        address
    }

    fn post_increment(address: SocketAddr) -> String {
        let mut stream =
            std::net::TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .write_all(
                b"POST /increment HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[tokio::test]
    async fn shutdown_finishes_an_accepted_http_request_before_stopping_the_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("demo.sqlite");
        let mut runtime = runtime(&path).await;
        let requested = runtime.requested.clone();
        let http_shutdown = runtime.cancellation.clone();
        let writer_shutdown = runtime.database_shutdown.clone();
        let database = runtime.database.clone();
        let admitted = Arc::new(Notify::new());
        let finish_request = Arc::new(Notify::new());
        let handler_database = database.clone();
        let handler_admitted = admitted.clone();
        let handler_finish = finish_request.clone();
        let router = Router::new().route(
            "/increment",
            post(move || {
                let database = handler_database.clone();
                let admitted = handler_admitted.clone();
                let finish = handler_finish.clone();
                async move {
                    admitted.notify_one();
                    finish.notified().await;
                    database.increment().await.ok().unwrap().to_string()
                }
            }),
        );
        let address = serve(&mut runtime, router).await;
        let running = tokio::spawn(runtime.run_until_stop());
        let client = tokio::task::spawn_blocking(move || post_increment(address));
        tokio::time::timeout(Duration::from_secs(3), admitted.notified())
            .await
            .unwrap();

        requested.cancel();
        tokio::time::timeout(Duration::from_secs(3), http_shutdown.cancelled())
            .await
            .unwrap();
        assert!(!database.is_ready().await);
        assert!(!writer_shutdown.is_cancelled());
        assert!(!running.is_finished());

        finish_request.notify_one();
        let response = client.await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
        assert!(response.ends_with("\r\n\r\n1"), "{response}");
        running.await.unwrap().unwrap();
        assert!(writer_shutdown.is_cancelled());
        assert!(
            database.counter().await.is_err(),
            "read pool must be closed"
        );

        let (reopened, writer) = Database::open(&path, 8).await.unwrap();
        assert_eq!(reopened.counter().await.unwrap(), 1);
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        writer.run(shutdown).await.unwrap();
    }

    #[tokio::test]
    async fn an_unexpected_writer_exit_stops_http() {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = runtime(&directory.path().join("demo.sqlite")).await;
        let http_shutdown = runtime.cancellation.clone();
        let database = runtime.database.clone();
        let router = routes::router(database.clone());
        let address = serve(&mut runtime, router).await;
        runtime.writer.as_ref().unwrap().abort();

        let error = tokio::time::timeout(Duration::from_secs(3), runtime.run_until_stop())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("database writer"), "{error:#}");
        assert!(http_shutdown.is_cancelled());
        assert!(!database.is_ready().await);
        assert!(tokio::net::TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn an_unexpected_http_exit_drains_and_closes_the_writer() {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime = runtime(&directory.path().join("demo.sqlite")).await;
        let writer_shutdown = runtime.database_shutdown.clone();
        let database = runtime.database.clone();
        let router = routes::router(database.clone());
        serve(&mut runtime, router).await;
        runtime.http.as_ref().unwrap().abort();

        let error = tokio::time::timeout(Duration::from_secs(3), runtime.run_until_stop())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("HTTP"), "{error:#}");
        assert!(writer_shutdown.is_cancelled());
        assert!(
            database.counter().await.is_err(),
            "read pool must be closed"
        );
    }
}
