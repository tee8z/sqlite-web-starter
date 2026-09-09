use axum::http::{Method, StatusCode, header};

use crate::helpers::{TestApp, body_text};

#[tokio::test]
async fn the_form_redirects_to_a_page_with_the_saved_counter() {
    let app = TestApp::spawn().await;

    let response = app.request(Method::POST, "/increment").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/");

    let response = app.request(Method::GET, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    let page = body_text(response).await;
    let counter = page
        .split_once("id=\"counter-value\"")
        .expect("page contains the counter output")
        .1
        .split_once('>')
        .unwrap()
        .1
        .split_once("</output>")
        .unwrap()
        .0;
    assert_eq!(counter, "1");

    app.shutdown().await;
}

#[tokio::test]
async fn the_maud_page_references_styles_and_scripts_served_by_the_app() {
    let app = TestApp::spawn().await;

    let response = app.request(Method::GET, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
    let page = body_text(response).await;
    let stylesheet = attribute(&page, "<link", "href");
    let javascript = attribute(&page, "<script", "src");

    for (url, extension, content_type) in [
        (stylesheet, ".css", "text/css; charset=utf-8"),
        (javascript, ".js", "text/javascript; charset=utf-8"),
    ] {
        assert!(url.starts_with("/assets/site."), "{url}");
        assert!(url.ends_with(extension), "{url}");
        let response = app.request(Method::GET, url).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], content_type);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert!(!body_text(response).await.is_empty());
    }

    app.shutdown().await;
}

fn attribute<'a>(page: &'a str, tag: &str, name: &str) -> &'a str {
    let element = page
        .split_once(tag)
        .unwrap_or_else(|| panic!("page contains {tag}"))
        .1
        .split_once('>')
        .unwrap()
        .0;
    element
        .split_once(&format!("{name}=\""))
        .unwrap_or_else(|| panic!("{tag} contains {name}"))
        .1
        .split_once('"')
        .unwrap()
        .0
}
