use axum::http::{Method, Request, StatusCode, header};

use crate::helpers::{TestApp, body_text, htmx};

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

    // Bulma then the site's styles; htmx then the site's script.
    let urls = asset_urls(&page);
    let names: Vec<_> = urls
        .iter()
        .map(|url| {
            let (stem, rest) = url["/assets/".len()..].split_once('.').unwrap();
            (stem, rest.rsplit_once('.').unwrap().1)
        })
        .collect();
    assert_eq!(
        names,
        [
            ("bulma", "css"),
            ("site", "css"),
            ("htmx", "js"),
            ("site", "js")
        ]
    );
    for (url, (_, extension)) in urls.iter().zip(names) {
        let content_type = match extension {
            "css" => "text/css; charset=utf-8",
            _ => "text/javascript; charset=utf-8",
        };
        let response = app.request(Method::GET, url).await;
        assert_eq!(response.status(), StatusCode::OK, "{url}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], content_type);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert!(!body_text(response).await.is_empty());
    }

    app.shutdown().await;
}

#[tokio::test]
async fn one_url_serves_the_document_or_the_content_htmx_swaps_in() {
    let app = TestApp::spawn().await;

    for (uri, title, content) in [
        ("/", "Counter", "id=\"counter\""),
        ("/inventory", "Inventory", "id=\"inventory\""),
    ] {
        let response = app.request(Method::GET, uri).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::VARY],
            "HX-Request, HX-History-Restore-Request"
        );
        let page = body_text(response).await;
        assert!(page.starts_with("<!DOCTYPE html>"), "{uri}");
        assert!(page.contains("<main id=\"main-content\">"), "{uri}");
        assert!(page.contains(content), "{uri}");
        assert!(!page.contains("hx-swap-oob"), "{uri}");

        let response = app.htmx(Method::GET, uri).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let fragment = body_text(response).await;
        // The title, the tabs out of band, then the content; no document around it.
        assert!(
            fragment.starts_with(&format!("<title>{title} · SQLite Web Starter</title>")),
            "{uri}"
        );
        assert!(fragment.contains("<nav id=\"site-tabs\""), "{uri}");
        assert!(fragment.contains("hx-swap-oob=\"true\""), "{uri}");
        assert!(fragment.contains(content), "{uri}");
        assert!(!fragment.contains("<html"), "{uri}");
        assert!(!fragment.contains("id=\"main-content\""), "{uri}");
    }

    app.shutdown().await;
}

#[tokio::test]
async fn going_back_in_history_gets_the_whole_document() {
    let app = TestApp::spawn().await;

    let request =
        htmx(Request::builder().uri("/inventory")).header("HX-History-Restore-Request", "true");
    let page = body_text(app.send(request, None).await).await;
    assert!(page.starts_with("<!DOCTYPE html>"));
    assert!(page.contains("id=\"inventory\""));

    app.shutdown().await;
}

#[tokio::test]
async fn an_htmx_increment_gets_the_counter_back_instead_of_a_redirect() {
    let app = TestApp::spawn().await;

    let response = app.htmx(Method::POST, "/increment").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::LOCATION));
    let fragment = body_text(response).await;
    assert!(fragment.starts_with("<section id=\"counter\""));
    assert!(fragment.contains(">1</output>"));
    assert!(fragment.contains("Saved."));

    app.shutdown().await;
}

#[tokio::test]
async fn a_rejected_increment_is_explained_in_html_with_the_api_status() {
    let app = TestApp::spawn_without_writer().await;

    let response = app.htmx(Method::POST, "/increment").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "1");
    let fragment = body_text(response).await;
    assert!(fragment.starts_with("<section id=\"counter\""));
    assert!(fragment.contains(">0</output>"));
    assert!(fragment.contains("role=\"alert\">The server is busy."));

    // Without JavaScript the same reply is a whole page.
    let response = app.request(Method::POST, "/increment").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let page = body_text(response).await;
    assert!(page.starts_with("<!DOCTYPE html>"));
    assert!(page.contains("role=\"alert\">The server is busy."));

    app.shutdown().await;
}

/// Every `/assets/` URL the page links to, in document order.
fn asset_urls(page: &str) -> Vec<String> {
    page.split("=\"/assets/")
        .skip(1)
        .map(|rest| {
            let (file, _) = rest.split_once('"').expect("the attribute is closed");
            format!("/assets/{file}")
        })
        .collect()
}
