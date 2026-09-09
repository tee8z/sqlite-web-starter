use axum::{
    Router,
    routing::{get, post},
};

use crate::database::Database;

mod assets;
mod components;
mod layouts;
mod pages;

pub(super) fn router(database: Database) -> Router {
    Router::new()
        .route("/", get(pages::index))
        .route("/increment", post(pages::increment))
        .with_state(database)
        .merge(assets::router())
}
