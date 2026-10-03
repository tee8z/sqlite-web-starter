use axum::http::{Method, StatusCode, header};

use crate::helpers::{TestApp, body_text};

#[tokio::test]
async fn a_plain_form_post_saves_the_quantity_and_redirects_to_the_page() {
    let app = TestApp::spawn().await;

    let response = app.form("/inventory", "item=2&quantity=30").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/inventory");

    let page = body_text(app.request(Method::GET, "/inventory").await).await;
    assert!(page.contains("<tr><td>Pencils</td><td>30</td></tr>"));
    assert!(page.contains("<tr><td>Notebooks</td><td>12</td></tr>"));

    app.shutdown().await;
}

#[tokio::test]
async fn an_htmx_post_gets_the_inventory_back_with_the_new_quantity() {
    let app = TestApp::spawn().await;

    let response = app.htmx_form("/inventory", "item=3&quantity=0").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::LOCATION));
    let fragment = body_text(response).await;
    assert!(fragment.starts_with("<section id=\"inventory\""));
    assert!(fragment.contains("<tr><td>Mugs</td><td>0</td></tr>"));
    assert!(fragment.contains("role=\"status\">Saved.</p>"));
    // A saved form starts over empty.
    assert!(fragment.contains(
        "name=\"quantity\" type=\"text\" inputmode=\"numeric\" autocomplete=\"off\" value=\"\""
    ));

    app.shutdown().await;
}

#[tokio::test]
async fn an_invalid_form_comes_back_with_what_was_typed_and_changes_nothing() {
    let app = TestApp::spawn().await;

    let response = app.htmx_form("/inventory", "item=2&quantity=lots").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let fragment = body_text(response).await;
    assert!(fragment.starts_with("<section id=\"inventory\""));
    assert!(fragment.contains("value=\"lots\""));
    assert!(fragment.contains("<option value=\"2\" selected>Pencils</option>"));
    assert!(fragment.contains("Enter a whole number from 0 to 9999."));
    assert!(fragment.contains("<tr><td>Pencils</td><td>48</td></tr>"));

    // Without JavaScript the same errors arrive in a whole page.
    let response = app.form("/inventory", "item=2&quantity=-4").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let page = body_text(response).await;
    assert!(page.starts_with("<!DOCTYPE html>"));
    assert!(page.contains("value=\"-4\""));
    assert!(page.contains("Enter a whole number from 0 to 9999."));

    // A missing field is an error to show, not a request to refuse.
    let response = app.htmx_form("/inventory", "").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let fragment = body_text(response).await;
    assert!(fragment.contains("Choose an item."));

    app.shutdown().await;
}

#[tokio::test]
async fn an_item_that_does_not_exist_is_reported_beside_the_item_field() {
    let app = TestApp::spawn().await;

    let response = app.htmx_form("/inventory", "item=99&quantity=5").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let fragment = body_text(response).await;
    assert!(fragment.contains("That item is no longer in the inventory."));
    assert!(fragment.contains("value=\"5\""));

    app.shutdown().await;
}

#[tokio::test]
async fn the_refresh_gets_only_the_rows() {
    let app = TestApp::spawn().await;

    let response = app.htmx(Method::GET, "/fragments/inventory-rows").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_text(response).await,
        "<tr><td>Notebooks</td><td>12</td></tr>\
         <tr><td>Pencils</td><td>48</td></tr>\
         <tr><td>Mugs</td><td>6</td></tr>"
    );

    app.shutdown().await;
}

#[tokio::test]
async fn a_rejected_write_is_throttled_and_explained_in_the_form() {
    let app = TestApp::spawn_without_writer().await;

    let response = app.htmx_form("/inventory", "item=2&quantity=30").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "1");
    let fragment = body_text(response).await;
    assert!(fragment.starts_with("<section id=\"inventory\""));
    assert!(fragment.contains("role=\"alert\">The server is busy."));
    // Nothing was saved, and the form still holds the visitor's input.
    assert!(fragment.contains("<tr><td>Pencils</td><td>48</td></tr>"));
    assert!(fragment.contains("value=\"30\""));

    app.shutdown().await;
}
