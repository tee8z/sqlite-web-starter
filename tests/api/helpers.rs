use std::time::Duration;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, header, request},
    response::Response,
};
use sqlite_web_starter::{database::Database, routes};
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

pub struct TestApp {
    pub database: Database,
    router: Router,
    shutdown: CancellationToken,
    writer: Option<JoinHandle<anyhow::Result<()>>>,
    _directory: TempDir,
}

impl TestApp {
    pub async fn spawn() -> Self {
        let directory = tempfile::tempdir().expect("create test directory");
        let (database, writer) = Database::open(&directory.path().join("test.sqlite"), 64)
            .await
            .expect("initialize test database");
        let shutdown = CancellationToken::new();
        let writer = tokio::spawn(writer.run(shutdown.clone()));

        Self {
            router: routes::router(database.clone()),
            database,
            shutdown,
            writer: Some(writer),
            _directory: directory,
        }
    }

    /// Builds an app whose writer is dropped before it ever runs, so the
    /// command channel is closed and every write fails admission. This reaches
    /// `WriteError::Unavailable` deterministically, without racing a full queue.
    pub async fn spawn_without_writer() -> Self {
        let directory = tempfile::tempdir().expect("create test directory");
        let (database, writer) = Database::open(&directory.path().join("test.sqlite"), 64)
            .await
            .expect("initialize test database");
        // Dropping the writer closes the command channel, so writes fail
        // admission. The read pool stays open through the Database handle.
        drop(writer);

        Self {
            router: routes::router(database.clone()),
            database,
            shutdown: CancellationToken::new(),
            writer: None,
            _directory: directory,
        }
    }

    pub async fn request(&self, method: Method, uri: &str) -> Response {
        self.send(Request::builder().method(method).uri(uri), None)
            .await
    }

    /// A request as htmx sends it, asking for the HTML to swap in.
    pub async fn htmx(&self, method: Method, uri: &str) -> Response {
        self.send(htmx(Request::builder().method(method).uri(uri)), None)
            .await
    }

    /// A form a browser posts without JavaScript.
    pub async fn form(&self, uri: &str, fields: &str) -> Response {
        self.send(
            Request::builder().method(Method::POST).uri(uri),
            Some(fields),
        )
        .await
    }

    /// The same form, posted by htmx.
    pub async fn htmx_form(&self, uri: &str, fields: &str) -> Response {
        self.send(
            htmx(Request::builder().method(Method::POST).uri(uri)),
            Some(fields),
        )
        .await
    }

    pub async fn send(&self, request: request::Builder, form: Option<&str>) -> Response {
        let request = match form {
            Some(fields) => request
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(fields.to_owned())),
            None => request.body(Body::empty()),
        }
        .expect("build test request");
        tokio::time::timeout(Duration::from_secs(3), self.router.clone().oneshot(request))
            .await
            .expect("request timed out")
            .expect("serve test request")
    }

    pub async fn shutdown(mut self) {
        self.database.stop_readiness();
        self.shutdown.cancel();
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        // Keep the handle in self until completion so Drop also covers timeouts.
        tokio::time::timeout(Duration::from_secs(3), writer)
            .await
            .expect("database shutdown timed out")
            .expect("database writer panicked")
            .expect("close test database");
        self.writer.take();
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(writer) = &self.writer {
            // A failed assertion must not leave a detached writer running.
            writer.abort();
        }
    }
}

pub fn htmx(request: request::Builder) -> request::Builder {
    request.header("HX-Request", "true")
}

pub async fn body_text(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read response body");
    String::from_utf8(bytes.to_vec()).expect("response body is UTF-8")
}
