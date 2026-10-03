# Integration tests

The top-level `tests/` directory contains API integration tests that exercise the application through its public library and HTTP router.
Keep unit tests beside their implementation in `src/`, inside a `#[cfg(test)]` module.
For example, [startup.rs](../src/startup.rs) contains private startup and shutdown tests inline.

```text
tests/
  README.md
  api/
    main.rs
    helpers.rs
    health_check.rs
    counter.rs
    inventory.rs
    page.rs
```

[api/main.rs](api/main.rs) declares the test modules. Cargo discovers this directory as the `api` integration test target.
The suite covers health checks, committed counter updates, form redirects, HTML asset responses,
and the fragments htmx gets from the same URLs.

[api/helpers.rs](api/helpers.rs) creates an isolated temporary SQLite database, starts its writer task, and constructs the real application router.
Requests run through Axum in process, so the suite needs no bound port or external services.
`app.htmx` and `app.htmx_form` send what htmx sends; `app.form` posts what a browser posts without JavaScript.
Call `app.shutdown().await` after each test to stop the writer before removing its temporary database.

Add new API cases to the relevant module, or create a module under `api/` and declare it in `api/main.rs`.
Reuse the shared helper for database setup, requests, and cleanup.

Run commands from the repository root:

```sh
cargo test --locked --test api          # API integration tests
cargo test --locked --lib               # Unit tests in library modules
cargo test --locked --lib startup::tests # Startup unit tests only
cargo test --locked --all-features      # Full Rust test suite
```
