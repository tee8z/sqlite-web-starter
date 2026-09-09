use maud::{DOCTYPE, Markup, html};

use super::assets;

pub(super) fn base(content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "SQLite Web Starter" }
                link rel="stylesheet" href=(assets::CSS_URL);
                script src=(assets::JS_URL) defer {}
            }
            body {
                main {
                    h1 { "SQLite Web Starter" }
                    p { "A small server-rendered page with data saved in SQLite." }
                    (content)
                    footer { "The counter also works with JavaScript disabled." }
                }
            }
        }
    }
}
