use axum::{
    Router,
    routing::{get, post},
};

use crate::database::Database;

mod assets;
mod components;
mod htmx;
mod layouts;
mod pages;

pub(super) fn router(database: Database) -> Router {
    Router::new()
        .route("/", get(pages::counter_page))
        .route("/increment", post(pages::increment))
        .route(
            "/inventory",
            get(pages::inventory_page).post(pages::update_stock),
        )
        .route("/fragments/inventory-rows", get(pages::inventory_rows))
        .with_state(database)
        .merge(assets::router())
}
