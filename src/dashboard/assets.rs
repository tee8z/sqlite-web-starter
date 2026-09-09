use axum::{
    Router,
    http::header,
    response::{IntoResponse, Response},
    routing::get,
};

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

const CACHE_POLICY: &str = "public, max-age=31536000, immutable";
const CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";
const JS_CONTENT_TYPE: &str = "text/javascript; charset=utf-8";

pub fn router() -> Router {
    Router::new()
        .route(CSS_URL, get(stylesheet))
        .route(JS_URL, get(javascript))
}

async fn stylesheet() -> Response {
    asset(CSS_CONTENT_TYPE, CSS_BYTES)
}

async fn javascript() -> Response {
    asset(JS_CONTENT_TYPE, JS_BYTES)
}

fn asset(content_type: &'static str, bytes: &'static [u8]) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, CACHE_POLICY),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::*;

    async fn request(method: Method, url: &str) -> Response {
        router()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(url)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn hashed_urls_serve_embedded_bytes_with_immutable_cache_headers() {
        for (url, expected_type, expected_bytes) in [
            (CSS_URL, "text/css; charset=utf-8", CSS_BYTES),
            (JS_URL, "text/javascript; charset=utf-8", JS_BYTES),
        ] {
            let response = request(Method::GET, url).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_TYPE], expected_type);
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "public, max-age=31536000, immutable"
            );
            assert_eq!(
                response.headers()[header::X_CONTENT_TYPE_OPTIONS],
                "nosniff"
            );
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(!body.is_empty());
            assert_eq!(body.as_ref(), expected_bytes);
        }
    }

    #[tokio::test]
    async fn head_preserves_get_headers_without_sending_asset_bytes() {
        for url in [CSS_URL, JS_URL] {
            let get = request(Method::GET, url).await;
            let head = request(Method::HEAD, url).await;
            assert_eq!(head.status(), StatusCode::OK);
            assert_eq!(head.headers(), get.headers());
            assert!(
                to_bytes(head.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[tokio::test]
    async fn unknown_hashes_and_unversioned_paths_are_not_served() {
        for url in [
            format!("/assets/site.{}.css", "0".repeat(64)),
            format!("/assets/site.{}.js", "0".repeat(64)),
            "/assets/site.css".to_owned(),
            "/assets/site.js".to_owned(),
        ] {
            let response = request(Method::GET, &url).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(!response.headers().contains_key(header::CACHE_CONTROL));
        }
    }

    #[tokio::test]
    async fn asset_urls_reject_post() {
        for url in [CSS_URL, JS_URL] {
            assert_eq!(
                request(Method::POST, url).await.status(),
                StatusCode::METHOD_NOT_ALLOWED
            );
        }
    }
}
