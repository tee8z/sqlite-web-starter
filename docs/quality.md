# sqlite-web-starter engineering quality guide

Apply these conventions to new and changed code. Explain exceptions in review.
See the [README](../README.md) for architecture and [operations](operations.md)
for configuration, deployment, and durability limits.

## Design and types

- Start with concrete types and inherent `impl` blocks. Use a closure to replace
  one operation.
- Add a project trait for a current production substitution boundary or shared
  behavior required by a common algorithm. Testing alone is insufficient.
- Use standard and framework traits for Rust interoperability.
- Pass dependencies through constructors. Keep operation order, ownership,
  state changes, and failure behavior explicit.
- Expose fields on plain records. Keep fields that protect invariants private.
  Do not add trivial getters, setters, or convenience `Deref` implementations.
- Use enums with typed payloads for alternatives. Add newtypes when validation,
  units, or ambiguity require them. Match enums exhaustively.

## Module boundaries

- Keep this application in one crate. Add modules when responsibilities require
  separation; avoid speculative service or repository layers.
- Keep `main.rs` small and process composition in [startup.rs](../src/startup.rs).
- Keep connections, SQL, and write commands in [database.rs](../src/database.rs).
  HTTP handlers use `Database` operations without access to its connections.
- Keep HTTP routing and error mapping in [routes.rs](../src/routes.rs).
- Keep Maud page composition in [dashboard/pages.rs](../src/dashboard/pages.rs)
  and the shared HTML shell in [dashboard/layouts.rs](../src/dashboard/layouts.rs).
- Give each component its own folder under
  [dashboard/components/](../src/dashboard/components/), with `mod.rs` beside its JavaScript and CSS files. Use
  [dashboard/styles.css](../src/dashboard/styles.css) for shared page styles.
- Add sibling `.css` and `.js` files without a central asset list. Follow the
  [asset build conventions](operations.md#configure-the-application) for ordering and script boundaries.
- Keep items private by default. Expose only what callers need, without widening
  production visibility for tests.
- Import project items at module scope. Use short names or explicit aliases at
  call sites instead of function-local `crate::...` paths.

## Database and lifecycle

- Preserve one writable connection, a bounded command queue, and a bounded
  read-only, query-only pool for normal application traffic.
- Reply with success only after commit. Keep admission rejection distinct from
  an unknown outcome after admission. Never retry uncertain writes automatically.
- Preserve accepted writes when callers disconnect. Keep transactions short and
  free of network or other unbounded work.
- Keep direct writers and artificial transaction delays confined to the
  [contention lab](../src/database/contention.rs) and its temporary databases.
- Add numbered migrations without changing applied files. Preserve existing
  data when initializing or reopening a database.
- Keep task supervision and shutdown ordering explicit in `startup`.
  Treat unexpected HTTP or writer task completion as application failure.
- When Litestream is enabled, complete its [initial sync](../src/replication.rs)
  before serving HTTP.
- On shutdown, disable readiness, drain HTTP, close and drain writes, then close
  SQLite. See [shutdown details](operations.md#replacement-and-durability).
- Bound startup and shutdown waits. Cancel and await owned tasks.
  Use explicit coordination for ordering; elapsed time alone cannot establish readiness.
- Preserve one live owner per replica prefix. A successful write confirms a
  local commit; asynchronous replication does not guarantee remote durability.

## Validation and errors

- Validate configuration and external input before use. Bound queues, concurrent
  work, and waits at their entry points.
- Use typed errors when callers need different actions, as with `WriteError`.
  Retain `sqlx::Error` for database reads and contextual `anyhow` errors for process failures.
- Map errors explicitly to HTTP status codes and safe messages. Log internal
  details with `tracing`; do not expose database errors or credentials to clients.
- Do not panic on external input. Use safe Rust by default; explain each required
  `unsafe` block with a preceding `SAFETY:` comment.

## Tests and documentation

- Name tests after observable behavior. Keep focused tests beside the owning
  module; use real SQLite databases in temporary directories.
- Test affected guarantees: commit before reply, queue rejection, dropped replies,
  read-only enforcement, migration preservation, and shutdown drain.
- Call routers directly for response checks. Use sockets for lifecycle behavior
  and port `0` for test listeners.
- Coordinate concurrent tests with channels, barriers, or notifications. Bound
  real waits with timeouts and clean up spawned tasks and temporary resources.
- Keep tests independent of developer databases and cloud accounts. Use the
  container smoke test for packaged assets, shutdown, and restoration.
- Keep the counter form usable without JavaScript. Test asset URLs, served bytes,
  and cache headers when changing the frontend build or delivery path.
- Add tests that prove behavior or reproduce failures. Avoid tests that only
  exercise lines and abstractions added solely for mocking.
- Document invariants and surprising decisions. Keep one maintained explanation
  per topic and link to it instead of repeating procedures.

## Dependencies and checks

- Add dependencies only for current needs. Enable only required features and
  keep resolved versions in `Cargo.lock`.
- Use [rust-toolchain.toml](../rust-toolchain.toml) and the [justfile](../justfile).
  Explain narrow lint exceptions with `#[expect(lint, reason = "...")]`.

Run relevant [focused checks](operations.md#build-and-validate) while editing.
Run the full local gate before submitting code, build, or deployment changes:

```sh
just local-ci
```

This gate checks formatting, Clippy, Rust tests, release helper tests, Helm
validation, and the container smoke test. It requires Docker and Helm.
For documentation-only changes, verify claims, commands, and relative links.
Do not claim an audit or coverage gate that the repository does not configure.
