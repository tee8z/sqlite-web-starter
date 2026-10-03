use maud::{DOCTYPE, Markup, html};

use super::{assets, components::tabs};

pub(super) struct PageConfig {
    pub title: &'static str,
    pub current_page: CurrentPage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CurrentPage {
    Counter,
    Inventory,
}

pub(super) fn base(config: &PageConfig, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (config.title) }
                link rel="stylesheet" href=(assets::BULMA_CSS.url);
                link rel="stylesheet" href=(assets::SITE_CSS.url);
                script src=(assets::HTMX_JS.url) defer {}
                script src=(assets::SITE_JS.url) defer {}
            }
            body {
                section class="section" {
                    div class="container is-max-desktop" {
                        h1 class="title" { "SQLite Web Starter" }
                        p class="subtitle" { "Small server-rendered pages with data saved in SQLite." }
                        (tabs::render(config.current_page, false))
                        main id="main-content" {
                            (content)
                        }
                        footer class="site-footer" { "Every form here also works with JavaScript disabled." }
                    }
                }
            }
        }
    }
}

/// What htmx swaps into `#main-content` when a tab opens a page: the page's
/// title and content, and the tabs out of band so the current one stays marked.
pub(super) fn page_fragment(config: &PageConfig, content: Markup) -> Markup {
    html! {
        title { (config.title) }
        (tabs::render(config.current_page, true))
        (content)
    }
}
