use maud::{Markup, html};

use crate::dashboard::layouts::CurrentPage;

/// The pages, as tabs. Each is a plain link that htmx upgrades: it fetches
/// the page's content into `#main-content` and puts the URL in the address
/// bar. That response re-sends the tabs `out_of_band`, so htmx replaces these
/// too and the current page stays marked.
pub fn render(current: CurrentPage, out_of_band: bool) -> Markup {
    html! {
        nav id="site-tabs" class="tabs is-boxed" aria-label="Pages"
            hx-swap-oob=[out_of_band.then_some("true")] {
            ul {
                (tab(current, CurrentPage::Counter, "/", "Counter"))
                (tab(current, CurrentPage::Inventory, "/inventory", "Inventory"))
            }
        }
    }
}

fn tab(current: CurrentPage, page: CurrentPage, href: &str, label: &str) -> Markup {
    let active = current == page;
    html! {
        li class=[active.then_some("is-active")] {
            // htmx 4 attributes are not inherited: each link names its own target.
            a href=(href) hx-get=(href) hx-target="#main-content" hx-push-url="true"
              aria-current=[active.then_some("page")] {
                (label)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_page_is_marked_and_every_tab_is_a_link() {
        let html = render(CurrentPage::Inventory, false).into_string();
        assert!(html.contains(
            r##"<li class="is-active"><a href="/inventory" hx-get="/inventory" hx-target="#main-content" hx-push-url="true" aria-current="page">Inventory</a>"##
        ));
        assert!(html.contains(r#"<li><a href="/" hx-get="/""#));
        assert!(!html.contains("hx-swap-oob"));
    }

    #[test]
    fn a_fragment_replaces_the_tabs_out_of_band() {
        let html = render(CurrentPage::Counter, true).into_string();
        assert!(html.contains(r#"id="site-tabs""#));
        assert!(html.contains(r#"hx-swap-oob="true""#));
    }
}
