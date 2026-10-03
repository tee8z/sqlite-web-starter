use axum::{
    Form,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use maud::{Markup, html};
use serde::Deserialize;

use super::{
    components::{
        contention, counter,
        inventory::{self, Notice, StockForm},
    },
    htmx,
    layouts::{self, CurrentPage, PageConfig},
};
use crate::{
    database::{Database, WriteError},
    routes::{ApiError, read_error, write_error},
};

const COUNTER: PageConfig = PageConfig {
    title: "Counter · SQLite Web Starter",
    current_page: CurrentPage::Counter,
};

const INVENTORY: PageConfig = PageConfig {
    title: "Inventory · SQLite Web Starter",
    current_page: CurrentPage::Inventory,
};

/// The whole document, or only its content when htmx swaps the page in.
fn page(headers: &HeaderMap, config: &PageConfig, content: Markup) -> Response {
    let markup = if htmx::wants_fragment(headers) {
        layouts::page_fragment(config, content)
    } else {
        layouts::base(config, content)
    };
    htmx::html(StatusCode::OK, markup)
}

pub(super) async fn counter_page(
    headers: HeaderMap,
    State(database): State<Database>,
) -> Result<Response, ApiError> {
    let value = database.counter().await.map_err(read_error)?;
    Ok(page(
        &headers,
        &COUNTER,
        counter_content(value, counter::Status::Idle),
    ))
}

fn counter_content(value: i64, status: counter::Status) -> Markup {
    html! {
        (counter::render(value, status))
        (contention::render())
    }
}

/// htmx gets the counter back to swap in; a plain form post is redirected to
/// the page. A failed write answers both with the counter and what went wrong.
pub(super) async fn increment(
    headers: HeaderMap,
    State(database): State<Database>,
) -> Result<Response, ApiError> {
    let fragment = htmx::wants_fragment(&headers);
    match database.increment().await {
        Ok(value) if fragment => Ok(htmx::html(
            StatusCode::OK,
            counter::render(value, counter::Status::Saved),
        )),
        Ok(_) => Ok(Redirect::to("/").into_response()),
        Err(error) => {
            let status = counter::Status::Failed(failure_message(&error));
            let value = database.counter().await.map_err(read_error)?;
            let markup = if fragment {
                counter::render(value, status)
            } else {
                layouts::base(&COUNTER, counter_content(value, status))
            };
            Ok(failed_write(error, markup))
        }
    }
}

pub(super) async fn inventory_page(
    headers: HeaderMap,
    State(database): State<Database>,
) -> Result<Response, ApiError> {
    let items = database.inventory().await.map_err(read_error)?;
    Ok(page(
        &headers,
        &INVENTORY,
        inventory::render(&items, &StockForm::default()),
    ))
}

/// The table's rows alone, for the request that refreshes them.
pub(super) async fn inventory_rows(State(database): State<Database>) -> Response {
    match database.inventory().await {
        Ok(items) => htmx::html(StatusCode::OK, inventory::rows(&items)),
        Err(error) => htmx::html(read_error(error).0, inventory::rows_unavailable()),
    }
}

#[derive(Deserialize)]
pub(super) struct StockInput {
    #[serde(default)]
    item: String,
    #[serde(default)]
    quantity: String,
}

/// Saves one item's quantity. A saved plain form post is redirected to the
/// page; every other outcome answers with the inventory and its form, the
/// form holding what was typed and what was wrong with it.
pub(super) async fn update_stock(
    headers: HeaderMap,
    State(database): State<Database>,
    Form(input): Form<StockInput>,
) -> Result<Response, ApiError> {
    let fragment = htmx::wants_fragment(&headers);
    let mut form = StockForm::new(input.item, input.quantity);
    let mut failed = None;
    if let Some((id, quantity)) = form.validate() {
        match database.set_quantity(id, quantity).await {
            Ok(true) if fragment => {
                form = StockForm {
                    notice: Some(Notice::Saved),
                    ..StockForm::default()
                }
            }
            Ok(true) => return Ok(Redirect::to("/inventory").into_response()),
            Ok(false) => form.item_error = Some("That item is no longer in the inventory."),
            Err(error) => {
                form.notice = Some(Notice::Failed(failure_message(&error)));
                failed = Some(error);
            }
        }
    }
    let items = database.inventory().await.map_err(read_error)?;
    let section = inventory::render(&items, &form);
    let markup = if fragment {
        section
    } else {
        layouts::base(&INVENTORY, section)
    };
    Ok(match failed {
        Some(error) => failed_write(error, markup),
        None if form.has_errors() => htmx::html(StatusCode::UNPROCESSABLE_ENTITY, markup),
        None => htmx::html(StatusCode::OK, markup),
    })
}

fn failure_message(error: &WriteError) -> &'static str {
    match error {
        WriteError::Unavailable => "The server is busy. Wait a moment, then try again.",
        // A lost reply can follow a committed write, so nothing here retries.
        WriteError::OutcomeUnknown => {
            "Could not confirm the update. Reload the page before trying again."
        }
        WriteError::Database(_) => "The update could not be saved.",
    }
}

/// The status and `Retry-After` the API gives a failed write, with a page or
/// fragment for the body: htmx swaps error replies in like any other.
fn failed_write(error: WriteError, markup: Markup) -> Response {
    let rejected = write_error(error);
    let mut response = htmx::html(rejected.status(), markup);
    if let Some(retry_after) = rejected.headers().get(header::RETRY_AFTER) {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, retry_after.clone());
    }
    response
}
