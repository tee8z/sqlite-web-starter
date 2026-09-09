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
