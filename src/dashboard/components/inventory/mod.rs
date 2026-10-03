use maud::{Markup, html};

use crate::database::InventoryItem;

/// The largest quantity the form accepts.
const MAX_QUANTITY: i64 = 9_999;

/// The stock form as it was submitted: what was typed, what was wrong with
/// it, and what became of it. A rejected form is rendered again with these,
/// so nothing typed is lost and each error sits beside its field.
#[derive(Default)]
pub struct StockForm {
    pub item: String,
    pub quantity: String,
    pub item_error: Option<&'static str>,
    pub quantity_error: Option<&'static str>,
    pub notice: Option<Notice>,
}

pub enum Notice {
    Saved,
    Failed(&'static str),
}

impl StockForm {
    pub fn new(item: String, quantity: String) -> Self {
        Self {
            item,
            quantity,
            ..Self::default()
        }
    }

    /// The item's id and its new quantity, or errors to show beside the fields.
    pub fn validate(&mut self) -> Option<(i64, i64)> {
        let id = self.item.trim().parse::<i64>().ok();
        if id.is_none() {
            self.item_error = Some("Choose an item.");
        }
        let quantity = self
            .quantity
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|quantity| (0..=MAX_QUANTITY).contains(quantity));
        if quantity.is_none() {
            self.quantity_error = Some("Enter a whole number from 0 to 9999.");
        }
        id.zip(quantity)
    }

    pub fn has_errors(&self) -> bool {
        self.item_error.is_some() || self.quantity_error.is_some()
    }
}

/// The table and the form that changes it. The form's reply is this section
/// again, so one swap shows the new quantity or the form's errors.
pub fn render(inventory: &[InventoryItem], form: &StockForm) -> Markup {
    html! {
        section id="inventory" class="box" aria-labelledby="inventory-heading" {
            h2 id="inventory-heading" class="title is-5" { "Sample inventory" }
            table id="sample-data" class="table is-fullwidth is-striped" {
                caption { "Read from SQLite, and again every five seconds." }
                thead { tr { th scope="col" { "Item" } th scope="col" { "Quantity" } } }
                // Another visitor's change shows up without a reload.
                tbody hx-get="/fragments/inventory-rows" hx-trigger="every 5s" {
                    (rows(inventory))
                }
            }
            (stock_form(inventory, form))
        }
    }
}

/// Only the rows: the table renders them, and so does the request that
/// refreshes them.
pub fn rows(inventory: &[InventoryItem]) -> Markup {
    html! {
        @for item in inventory {
            tr { td { (item.name) } td { (item.quantity) } }
        }
    }
}

/// What the refresh shows when the rows cannot be read; it asks again by itself.
pub fn rows_unavailable() -> Markup {
    html! {
        tr { td colspan="2" role="alert" { "Couldn't load the inventory. Trying again shortly." } }
    }
}

fn stock_form(inventory: &[InventoryItem], form: &StockForm) -> Markup {
    html! {
        form class="stock-form" method="post" action="/inventory"
             hx-post="/inventory" hx-target="#inventory" hx-swap="outerHTML" hx-disable="find button" {
            h3 class="title is-6" { "Update stock" }
            div class="field" {
                label class="label" for="stock-item" { "Item" }
                div class="control" {
                    div class=(if form.item_error.is_some() { "select is-danger" } else { "select" }) {
                        select id="stock-item" name="item"
                               aria-describedby=[form.item_error.map(|_| "stock-item-error")] {
                            @for item in inventory {
                                option value=(item.id) selected[form.item == item.id.to_string()] {
                                    (item.name)
                                }
                            }
                        }
                    }
                }
                @if let Some(error) = form.item_error {
                    p id="stock-item-error" class="help is-danger" { (error) }
                }
            }
            div class="field" {
                label class="label" for="stock-quantity" { "Quantity" }
                div class="control" {
                    input id="stock-quantity" name="quantity" type="text" inputmode="numeric"
                          autocomplete="off" value=(form.quantity)
                          class=(if form.quantity_error.is_some() { "input is-danger" } else { "input" })
                          aria-invalid=[form.quantity_error.map(|_| "true")]
                          aria-describedby=[form.quantity_error.map(|_| "stock-quantity-error")];
                }
                @if let Some(error) = form.quantity_error {
                    p id="stock-quantity-error" class="help is-danger" { (error) }
                }
            }
            div class="field" {
                div class="control" {
                    button id="stock-save" class="button is-primary" type="submit" { "Save quantity" }
                }
            }
            @match form.notice {
                Some(Notice::Saved) => p class="help is-success" role="status" { "Saved." },
                Some(Notice::Failed(message)) => p class="help is-danger" role="alert" { (message) },
                None => {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> Vec<InventoryItem> {
        vec![
            InventoryItem {
                id: 1,
                name: "Notebooks".into(),
                quantity: 12,
            },
            InventoryItem {
                id: 2,
                name: "Pencils".into(),
                quantity: 48,
            },
        ]
    }

    #[test]
    fn valid_input_becomes_an_id_and_a_quantity() {
        let mut form = StockForm::new("2".into(), " 30 ".into());
        assert_eq!(form.validate(), Some((2, 30)));
        assert!(!form.has_errors());
    }

    #[test]
    fn each_invalid_field_gets_its_own_error() {
        for quantity in ["", "many", "-1", "10000", "1.5"] {
            let mut form = StockForm::new("2".into(), quantity.into());
            assert_eq!(form.validate(), None, "{quantity:?}");
            assert!(form.item_error.is_none());
            assert!(form.quantity_error.is_some(), "{quantity:?}");
        }
        let mut form = StockForm::new(String::new(), "5".into());
        assert_eq!(form.validate(), None);
        assert!(form.item_error.is_some());
        assert!(form.quantity_error.is_none());
    }

    #[test]
    fn a_rejected_form_keeps_what_was_typed_beside_its_error() {
        let mut form = StockForm::new("2".into(), "many".into());
        form.validate();
        let html = render(&inventory(), &form).into_string();
        assert!(html.contains(r#"<option value="2" selected>Pencils</option>"#));
        assert!(html.contains(r#"value="many""#));
        assert!(html.contains(r#"aria-invalid="true""#));
        assert!(html.contains(
            r#"<p id="stock-quantity-error" class="help is-danger">Enter a whole number from 0 to 9999.</p>"#
        ));
        assert!(!html.contains("stock-item-error"));
    }

    #[test]
    fn the_table_refreshes_its_rows_and_the_form_replaces_the_section() {
        let html = render(&inventory(), &StockForm::default()).into_string();
        assert!(
            html.contains(r#"<tbody hx-get="/fragments/inventory-rows" hx-trigger="every 5s">"#)
        );
        assert!(html.contains(&rows(&inventory()).into_string()));
        assert!(html.contains(r#"<form class="stock-form" method="post" action="/inventory""#));
        assert!(
            html.contains(r##"hx-post="/inventory" hx-target="#inventory" hx-swap="outerHTML""##)
        );
    }
}
