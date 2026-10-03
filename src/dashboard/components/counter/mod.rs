use maud::{Markup, html};

/// What the line under the button says about the last click.
pub enum Status {
    Idle,
    Saved,
    Failed(&'static str),
}

/// Without JavaScript the form posts and the server redirects back to the
/// page. With htmx the same POST returns this section, which replaces itself.
pub fn render(value: i64, status: Status) -> Markup {
    html! {
        section id="counter" class="box" aria-labelledby="counter-heading" {
            h2 id="counter-heading" class="title is-5" { "Saved counter" }
            output id="counter-value" aria-labelledby="counter-heading" { (value) }
            // One request at a time: the button is disabled until the reply.
            // Its id lets htmx return focus to it in the section that replaces this one.
            form method="post" action="/increment"
                 hx-post="/increment" hx-target="#counter" hx-swap="outerHTML" hx-disable="find button" {
                button id="counter-increment" class="button is-primary" type="submit" { "Increment counter" }
            }
            @match status {
                Status::Idle => p id="counter-status" class="help" role="status" {
                    "Each click saves one increment."
                },
                Status::Saved => p id="counter-status" class="help is-success" role="status" { "Saved." },
                Status::Failed(message) => p id="counter-status" class="help is-danger" role="alert" {
                    (message)
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_posts_with_or_without_htmx_and_replaces_its_section() {
        let html = render(7, Status::Idle).into_string();
        assert!(html.contains(
            r#"<output id="counter-value" aria-labelledby="counter-heading">7</output>"#
        ));
        assert!(html.contains(r#"<form method="post" action="/increment""#));
        assert!(
            html.contains(r##"hx-post="/increment" hx-target="#counter" hx-swap="outerHTML""##)
        );
    }

    #[test]
    fn a_failed_write_is_announced_in_place_of_the_hint() {
        let html = render(7, Status::Failed("Could not confirm the update.")).into_string();
        assert!(html.contains(r#"role="alert">Could not confirm the update.</p>"#));
        assert!(!html.contains("Each click saves one increment."));
    }
}
