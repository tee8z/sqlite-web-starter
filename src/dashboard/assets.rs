//! The page's stylesheets and scripts, bundled by `build.rs` and embedded in
//! the executable.
//!
//! Each asset's URL contains a hash of its bytes, so a URL never changes
//! meaning and browsers may cache it for a year. Any other `/assets/` path,
//! including an old hash, is not found.

use axum::{
    Router,
    extract::Path,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

/// One embedded file: its hashed URL, type, and bytes (plain and gzipped).
pub struct Asset {
    pub url: &'static str,
    pub content_type: &'static str,
    pub bytes: &'static [u8],
    pub gzip: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

const CACHE_POLICY: &str = "public, max-age=31536000, immutable";

pub fn router() -> Router {
    Router::new().route("/assets/{file}", get(serve))
}

/// Gzipped when the browser accepts it: Bulma is 677 kB plain, 65 kB gzipped.
async fn serve(Path(file): Path<String>, headers: HeaderMap) -> Response {
    let Some(asset) = ALL
        .iter()
        .find(|asset| asset.url.strip_prefix("/assets/") == Some(file.as_str()))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let accepts_gzip = headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|coding| {
            let mut parts = coding.split(';').map(str::trim);
            parts.next() == Some("gzip") && !parts.any(|parameter| parameter == "q=0")
        });
    let mut response = if accepts_gzip {
        ([(header::CONTENT_ENCODING, "gzip")], asset.gzip).into_response()
    } else {
        asset.bytes.into_response()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(asset.content_type),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_POLICY),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use flate2::read::GzDecoder;
    use tower::ServiceExt;

    use super::*;

    async fn request(method: Method, url: &str, accept_encoding: Option<&str>) -> Response {
        let mut request = Request::builder().method(method).uri(url);
        if let Some(value) = accept_encoding {
            request = request.header(header::ACCEPT_ENCODING, value);
        }
        router()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn hashed_urls_serve_embedded_bytes_with_immutable_cache_headers() {
        for asset in ALL {
            let response = request(Method::GET, asset.url, None).await;
            assert_eq!(response.status(), StatusCode::OK, "{}", asset.url);
            assert_eq!(response.headers()[header::CONTENT_TYPE], asset.content_type);
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "public, max-age=31536000, immutable"
            );
            assert_eq!(
                response.headers()[header::X_CONTENT_TYPE_OPTIONS],
                "nosniff"
            );
            assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(!body.is_empty(), "{}", asset.url);
            assert_eq!(body.as_ref(), asset.bytes);
        }
    }

    #[tokio::test]
    async fn browsers_that_accept_gzip_get_the_same_bytes_compressed() {
        for asset in ALL {
            let response = request(Method::GET, asset.url, Some("br, gzip;q=0.8")).await;
            assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
            assert_eq!(response.headers()[header::VARY], "Accept-Encoding");
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let mut plain = Vec::new();
            GzDecoder::new(body.as_ref())
                .read_to_end(&mut plain)
                .unwrap();
            assert_eq!(plain, asset.bytes, "{}", asset.url);
        }
        let refused = request(Method::GET, SITE_JS.url, Some("gzip;q=0, identity")).await;
        assert!(!refused.headers().contains_key(header::CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn head_preserves_get_headers_without_sending_asset_bytes() {
        for asset in ALL {
            let get = request(Method::GET, asset.url, None).await;
            let head = request(Method::HEAD, asset.url, None).await;
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
            "/assets/bulma.css".to_owned(),
            "/assets/htmx.js".to_owned(),
        ] {
            let response = request(Method::GET, &url, None).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(!response.headers().contains_key(header::CACHE_CONTROL));
        }
    }

    #[tokio::test]
    async fn asset_urls_reject_post() {
        for asset in ALL {
            assert_eq!(
                request(Method::POST, asset.url, None).await.status(),
                StatusCode::METHOD_NOT_ALLOWED
            );
        }
    }
}
