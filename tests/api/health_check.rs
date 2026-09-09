use axum::http::{Method, StatusCode};

use crate::helpers::{TestApp, body_text};

#[tokio::test]
async fn healthy_returns_an_empty_success_response() {
    let app = TestApp::spawn().await;

    let response = app.request(Method::GET, "/healthy").await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response).await.is_empty());
    app.shutdown().await;
}

#[tokio::test]
async fn readiness_stops_while_liveness_remains_available() {
    let app = TestApp::spawn().await;
    assert_eq!(
        app.request(Method::GET, "/ready").await.status(),
        StatusCode::OK
    );

    app.database.stop_readiness();

    assert_eq!(
        app.request(Method::GET, "/ready").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        app.request(Method::GET, "/healthy").await.status(),
        StatusCode::OK
    );
    app.shutdown().await;
}
