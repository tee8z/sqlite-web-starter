use maud::{Markup, html};

pub fn render() -> Markup {
    html! {
        section id="contention-lab" aria-labelledby="contention-heading" {
            h2 id="contention-heading" { "Why put writes through a channel?" }
            p {
                "SQLite allows one writer at a time, even in WAL mode. Start 12 write calls together and compare independent connections with a single writer behind a channel."
            }
            div class="lab-settings" {
                span { "12 simultaneous calls per path" }
                span { "75 ms transaction hold" }
                span { "0 ms busy timeout" }
            }
            button id="run-contention" type="button" disabled aria-describedby="contention-status" {
                "Run 12 concurrent writes"
            }
            p id="contention-status" role="status" {
                "Enable JavaScript to run the comparison."
            }
            div id="contention-results" hidden {
                div class="path-comparison" {
                    @for (path, title, description) in [
                        ("direct", "Independent writers", "12 calls → 12 SQLite connections"),
                        ("queued", "Channel + one writer", "12 calls → channel → 1 SQLite connection"),
                    ] {
                        article class="path-card" data-path=(path) aria-labelledby=(format!("{path}-heading")) {
                            h3 id=(format!("{path}-heading")) { (title) }
                            p class="path-description" { (description) }
                            dl class="path-stats" {
                                div { dt { "Committed" } dd id=(format!("{path}-committed")) {} }
                                div { dt { "SQLITE_BUSY" } dd id=(format!("{path}-busy")) {} }
                                div { dt { "Final counter" } dd id=(format!("{path}-value")) {} }
                            }
                            p class="path-duration" id=(format!("{path}-duration")) {}
                            ol class="request-timings" id=(format!("{path}-requests")) aria-label=(format!("{title}: each request's result and elapsed time")) {}
                        }
                    }
                }
                p class="timing-note" id="timing-note" {}
                p class="lab-takeaway" id="contention-takeaway" {}
            }
            details class="lab-explanation" {
                summary { "What this comparison shows" }
                p {
                    "One HTTP request starts concurrent tasks on the server for each path. Each run uses two fresh, temporary WAL databases starting at zero, so your saved counter is unchanged."
                }
                p {
                    "The lab holds each write transaction for 75 ms and sets the busy timeout to zero on both paths so lock contention is easy to see. Independent writers can return SQLITE_BUSY while another holds the write lock. The channel lets calls wait for one writer to commit them in turn. Request times include that wait."
                }
                p {
                    "SQLite busy waits or retries are other ways to handle competing writers. A channel coordinates writes in this process; it cannot prevent contention from another process. This demonstrates coordination, not a speed benchmark. The saved counter uses a 5-second busy timeout and no artificial delay."
                }
            }
        }
    }
}
