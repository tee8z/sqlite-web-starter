use maud::{Markup, html};

pub fn render(value: i64) -> Markup {
    html! {
        section aria-labelledby="counter-heading" {
            h2 id="counter-heading" { "Saved counter" }
            output id="counter-value" aria-labelledby="counter-heading" aria-live="polite" { (value) }
            form method="post" action="/increment" {
                button type="submit" { "Increment counter" }
            }
            p id="counter-status" role="status" { "Each click saves one increment." }
        }
    }
}
