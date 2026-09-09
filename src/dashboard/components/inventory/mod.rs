use maud::{Markup, html};

use crate::database::InventoryItem;

pub fn render(inventory: &[InventoryItem]) -> Markup {
    html! {
        section aria-labelledby="inventory-heading" {
            h2 id="inventory-heading" { "Sample inventory" }
            table id="sample-data" {
                caption { "These sample records are read from SQLite." }
                thead { tr { th scope="col" { "Item" } th scope="col" { "Quantity" } } }
                tbody {
                    @for item in inventory {
                        tr { td { (item.name) } td { (item.quantity) } }
                    }
                }
            }
        }
    }
}
