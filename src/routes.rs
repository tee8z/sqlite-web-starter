use std::{sync::Arc, time::Duration};

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::{
    dashboard,
    database::{Database, WriteError, contention},
};

pub fn router(database: Database) -> Router {
    Router::new()
        .route("/counter", get(read_counter).post(increment_counter))
        .route(
            "/contention",
            post(run_contention).layer(Extension(Arc::new(Semaphore::new(1)))),
        )
        .route("/ready", get(ready))
        .route("/healthy", get(healthy))
        .with_state(database.clone())
        .merge(dashboard::router(database))
}

#[derive(Serialize)]
struct Counter {
    value: i64,
}

pub type ApiError = (StatusCode, &'static str);

/// A queued write commits in about a millisecond, so a full queue drains fast.
/// One second gives a retry storm somewhere to wait without stalling callers.
const RETRY_AFTER_SECONDS: &str = "1";

pub fn read_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "read failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "database read failed")
}

pub fn write_error(error: WriteError) -> Response {
    match error {
        // Backpressure, not a failed server. A 5xx here would let proxy outlier
        // detection eject the only replica over a queue that drains in
        // milliseconds. Retry-After tells callers how long to back off.
        WriteError::Unavailable => (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, RETRY_AFTER_SECONDS)],
            "write queue full; retry after the given delay",
        )
            .into_response(),
        WriteError::OutcomeUnknown => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "write outcome unknown; do not retry blindly",
        )
            .into_response(),
        WriteError::Database(error) => {
            tracing::error!(%error, "write failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "database write failed").into_response()
        }
    }
}

async fn read_counter(State(database): State<Database>) -> Result<Json<Counter>, ApiError> {
    database
        .counter()
        .await
        .map(|value| Json(Counter { value }))
        .map_err(read_error)
}

async fn increment_counter(State(database): State<Database>) -> Result<Json<Counter>, Response> {
    database
        .increment()
        .await
        .map(|value| Json(Counter { value }))
        .map_err(write_error)
}

async fn run_contention(
    Extension(experiments): Extension<Arc<Semaphore>>,
) -> Result<Json<contention::Comparison>, ApiError> {
    let _permit = experiments.try_acquire().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "a contention experiment is already running; try again shortly",
        )
    })?;
    match tokio::time::timeout(Duration::from_secs(10), contention::run()).await {
        Ok(Ok(comparison)) => Ok(Json(comparison)),
        Ok(Err(error)) => {
            tracing::error!(%error, "contention experiment failed");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "contention experiment failed",
            ))
        }
        Err(_) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            "contention experiment timed out",
        )),
    }
}

async fn ready(State(database): State<Database>) -> StatusCode {
    if database.is_ready().await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn healthy() -> StatusCode {
    StatusCode::OK
}
