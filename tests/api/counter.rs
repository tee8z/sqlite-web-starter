use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};

use crate::helpers::{TestApp, body_text};

#[tokio::test]
async fn an_acknowledged_increment_is_visible_to_the_next_read() {
    let app = TestApp::spawn().await;

    for (method, expected) in [(Method::GET, 0), (Method::POST, 1), (Method::GET, 1)] {
        let response = app.request(method, "/counter").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let value: Value = serde_json::from_str(&body_text(response).await).unwrap();
        assert_eq!(value, json!({ "value": expected }));
    }

    app.shutdown().await;
}

#[tokio::test]
async fn a_write_that_fails_admission_is_throttled_rather_than_reported_unavailable() {
    let app = TestApp::spawn_without_writer().await;

    // Nothing ran, so the caller may retry. A 5xx here would let proxy outlier
    // detection eject the sole replica over transient backpressure.
    for uri in ["/counter", "/increment"] {
        let response = app.request(Method::POST, uri).await;
        assert_eq!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "{uri} should throttle a rejected write"
        );
        assert_eq!(
            response.headers()[header::RETRY_AFTER],
            "1",
            "{uri} must tell the caller how long to wait"
        );
    }

    // Reads do not depend on write admission and stay available.
    let response = app.request(Method::GET, "/counter").await;
    assert_eq!(response.status(), StatusCode::OK);

    app.shutdown().await;
}
