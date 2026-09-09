use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Redirect},
};
use maud::html;

use super::{
    components::{contention, counter, inventory},
    layouts,
};
use crate::{
    database::Database,
    routes::{ApiError, read_error, write_error},
};

pub(super) async fn index(State(database): State<Database>) -> Result<impl IntoResponse, ApiError> {
    let (value, items) =
        tokio::try_join!(database.counter(), database.inventory()).map_err(read_error)?;
    let page = layouts::base(html! {
        (counter::render(value))
        (contention::render())
        (inventory::render(&items))
    });
    // Revalidate HTML so a deployment can select the current content-hashed assets.
    Ok(([(header::CACHE_CONTROL, "no-cache")], page))
}

pub(super) async fn increment(State(database): State<Database>) -> Result<Redirect, ApiError> {
    database.increment().await.map_err(write_error)?;
    Ok(Redirect::to("/"))
}
