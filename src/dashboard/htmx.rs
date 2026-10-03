//! One URL serves a page and its parts: a normal load gets the whole
//! document, and an htmx request gets only the HTML it will swap in.

use axum::{
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use maud::Markup;

/// Whether htmx asked for part of the page. Going back or forward, htmx
/// re-requests the URL and replaces the whole body, so that gets the document.
pub(super) fn wants_fragment(headers: &HeaderMap) -> bool {
    headers.contains_key("hx-request") && !headers.contains_key("hx-history-restore-request")
}

/// HTML that depends on the htmx headers, so caches must key on them.
/// Revalidating lets a deployment select the current content-hashed assets.
pub(super) fn html(status: StatusCode, markup: Markup) -> Response {
    (
        status,
        [
            (header::CACHE_CONTROL, "no-cache"),
            (header::VARY, "HX-Request, HX-History-Restore-Request"),
        ],
        markup,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(names: &[&'static str]) -> HeaderMap {
        names
            .iter()
            .map(|name| (name.parse().unwrap(), HeaderValue::from_static("true")))
            .collect()
    }

    #[test]
    fn only_htmx_swaps_get_fragments() {
        assert!(!wants_fragment(&headers(&[])));
        assert!(wants_fragment(&headers(&["hx-request"])));
        assert!(!wants_fragment(&headers(&[
            "hx-request",
            "hx-history-restore-request"
        ])));
    }
}
