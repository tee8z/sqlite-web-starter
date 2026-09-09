# Operations

Run commands from the repository root. See the [README](../README.md) for database channels and architecture.

## Configure the application

| Environment variable | Default | Purpose |
| --- | --- | --- |
| `SQLITE_PATH` | `./data/demo.sqlite`; `/data/demo.sqlite` in Docker | Local database path |
| `PORT` | `3000` | HTTP listen port |
| `RUST_LOG` | `info` | Filter compact `tracing` output on stderr |
| `RUST_LOG_STYLE` | `auto`; `always` for `just run` | Automatic, forced, or disabled color (`auto`, `always`, `never`) |
| `SHUTDOWN_TIMEOUT_SECONDS` | `25` | Application drain budget |
| `LITESTREAM_REPLICA_URL` | Required in Docker | Replica destination |
| `LITESTREAM_SOCKET` | `/data/litestream.sock` in Docker; unset locally | Startup sync control socket |
| `LITESTREAM_STARTUP_TIMEOUT_SECONDS` | `60` | Initial sync deadline |
| `LITESTREAM_SYNC_INTERVAL` | `1s` in Docker | Periodic replica upload interval |

`just run` defaults to colored logs, including IDE consoles. Redirected output will also contain ANSI color codes.
Use `RUST_LOG_STYLE=never just run` for plain output, or `RUST_LOG_STYLE=auto just run` to detect terminals.
Direct `cargo run` uses automatic color; a nonempty `NO_COLOR` disables color in that mode.
Use `RUST_LOG=sqlite_web_starter=debug` to filter application logs.

Local `cargo run --locked` does not start Litestream. Leave `LITESTREAM_SOCKET` unset to run without the startup sync barrier.
The [entrypoint](../entrypoint.sh) restores missing databases before starting Rust under Litestream 0.5.16.
An absent replica permits first boot; other restore errors prevent startup.
Remove `-if-replica-exists` if missing backups must block startup.

With `LITESTREAM_SOCKET` set, the [startup barrier](../src/replication.rs) requests `litestream sync -wait -json` through the private socket after migrations.
HTTP starts after a nonzero local transaction ID reaches the replica.
The barrier establishes Litestream database initialization before serving writes; a timeout prevents serving.

Add numbered SQL files to [migrations](../migrations). Keep applied files unchanged so SQLx can validate their checksums.

Edit each component's browser assets beside its `mod.rs` in its own [src/dashboard/components/](../src/dashboard/components/) folder.
For example, [counter/](../src/dashboard/components/counter/) contains `mod.rs`, `counter.js`, and `counter.css`.
Use [styles.css](../src/dashboard/styles.css) for shared page styles, [pages.rs](../src/dashboard/pages.rs) for page composition, and [layouts.rs](../src/dashboard/layouts.rs) for the HTML shell.
[build.rs](../build.rs) uses [Lightning CSS](https://docs.rs/lightningcss/latest/lightningcss/stylesheet/struct.StyleSheet.html) and [minify-js](https://docs.rs/minify-js/0.6.0/minify_js/) during normal Cargo builds.
It discovers `.css` and `.js` files recursively under `src/dashboard/`, so new sibling assets need no central registration.
The CSS bundle starts with `styles.css`; remaining CSS files and all JavaScript files follow sorted path order.
Adding or editing an asset triggers a rebuild. Cargo writes minified bundles and a generated Rust manifest into `OUT_DIR`; the executable embeds those bytes.
The same pipeline runs for development and release builds, without Node.js, npm, or a separate frontend command.

Keep each JavaScript file inside a standalone immediately invoked function expression (IIFE).
The build inserts separators between JavaScript files; the page loads their bundle as a classic deferred script.
The build does not resolve JavaScript modules or CSS imports. Add build and route support before introducing imports, fonts, images, or other static assets.

Each `/assets/site.<sha256>.css` or `.js` URL hashes the minified bytes served by [assets.rs](../src/dashboard/assets.rs).
The asset responses allow immutable caching for one year. HTML uses `Cache-Control: no-cache` to select the current asset URLs after deployment.
Unknown asset hashes return HTTP 404. The executable serves only the embedded minified bundles.
The [Dockerfile](../Dockerfile) copies dashboard sources with `src/`; no separate frontend directory is required.
The final container needs no frontend tools or asset directory at runtime.

The counter script submits one request at a time and displays the returned value.
On an uncertain response, it asks the user to reload before retrying; it never retries writes automatically.
The normal `/increment` form remains available when JavaScript is disabled.

## Deploy with Helm

Create the `staging` or `prod` namespace first. Publish an image, then replace the placeholders below.
Use separate replica prefixes and Secrets for each environment.
The [environment overlays](../chart/values.staging.yaml) initially select an `unreleased` image; [production](../chart/values.prod.yaml) follows the release PR.
Helm automatically loads [shared defaults](../chart/values.yaml); use `-f chart/values.staging.yaml` or `-f chart/values.prod.yaml` for environment settings.

```sh
DEPLOY_ENV=staging # Use prod for production.
kubectl --namespace "$DEPLOY_ENV" create secret generic "sqlite-web-starter-replica-$DEPLOY_ENV" \
  --from-literal="replica-url=s3://YOUR_BUCKET/sqlite-web-starter/$DEPLOY_ENV"
helm upgrade --install sqlite-web-starter ./chart --namespace "$DEPLOY_ENV" \
  -f "chart/values.$DEPLOY_ENV.yaml" \
  --set image.repository=YOUR_REGISTRY/sqlite-web-starter \
  --set-string image.tag=YOUR_TAG \
  --set region=YOUR_REGION \
  --set serviceAccountName=YOUR_SERVICE_ACCOUNT \
  --set internalIngressRoute.enabled=false
kubectl --namespace "$DEPLOY_ENV" rollout status statefulset/sqlite-web-starter
kubectl --namespace "$DEPLOY_ENV" port-forward service/sqlite-web-starter-http 3000:3000
```

Staging uses a published commit SHA. For production, omit image overrides after the release PR updates the values file.
See [release setup](../.github/workflows/README.md) for ECR publishing and version tags.

Give the service account access to its bucket prefix through workload identity.
Alternatively, set `credentialsSecretName` to a Secret containing `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and optional `AWS_SESSION_TOKEN`.
The chart does not create cloud credentials or identity bindings.

By default, the chart creates a ServiceAccount named after the release. Set `serviceAccount.annotations` to configure your provider's workload identity binding.
To use an existing account, set `serviceAccount.create=false` and `serviceAccount.name`.
The `serviceAccountName` override in the example takes precedence and skips account creation.

Both overlays enable Traefik ingress with example hostnames. The command above disables ingress for port forwarding.
For hostname access, configure a private Traefik controller, its custom resource definitions, private networking, DNS, and a certificate.
The chart selects the `traefik-internal` ingress class and `websecure` entrypoint. Configure these values for your controller.
The class name alone does not restrict network access, and the application has no built-in authentication.

Replace the hostname in the selected environment file and configure an existing TLS Secret:

```yaml
internalIngressRoute:
  enabled: true
  domain: sqlite-web-starter.staging.internal.example.com
  tls:
    enabled: true
    secretName: internal-demo-tls
  # Optional existing authentication middleware:
  # middlewares:
  #   - name: internal-auth
```

Use your production hostname in `values.prod.yaml`. Remove `--set internalIngressRoute.enabled=false` from the installation command to enable hostname access.
The TLS Secret and any referenced middleware must exist in the release namespace for this example.
With an empty `tls.secretName`, the route uses the controller's default certificate configuration.
If an upstream load balancer terminates TLS, set `internalIngressRoute.tls.enabled=false` and choose a plaintext `entryPoint`.
See the [Traefik IngressRoute reference](https://doc.traefik.io/traefik/reference/routing-configuration/kubernetes/crd/http/ingressroute/) for controller and TLS configuration.

The route forwards the entire hostname to the `<release>-http` ClusterIP Service on `service.port`, which defaults to `3000`.
The HTML page, `/increment` form action, and redirects use root-relative paths. Give this application a dedicated hostname without prefix stripping.
The original `<release>` headless Service provides StatefulSet identity; both Services select the same pod.

The environment overlays select `linux/amd64` nodes because CI publishes that image architecture. Local chart defaults leave node selection unconstrained.
Use `nodeSelector`, `affinity`, and `tolerations` for your cluster's scheduling requirements.
Set `imagePullSecrets` for private registry credentials and `podAnnotations` for integrations that inspect pod metadata.

The [chart defaults](../chart/values.yaml) use one StatefulSet owner and a disk-backed `emptyDir`, without a persistent volume.
The StatefulSet is always rendered, including when ingress is disabled. There is no Deployment or autoscaling option.
The database survives container restarts within a pod. Pod replacement removes the local files and requires restoration from the object-store replica.
SQLite storage is limited to 2Gi; ephemeral storage requests are 2Gi and limits are 3Gi.
Allow space for WAL growth, restore files, and logs. The startup probe allows ten minutes for restoration.

## Replacement and durability

SIGTERM reaches Rust through Litestream. Rust disables readiness, drains HTTP, closes write admission, drains accepted commands, and closes SQLite.
After Rust exits successfully, Litestream attempts its final replica sync. Default budgets are 25 seconds for Rust, 45 for final sync, and 90 for pod termination.
A Rust shutdown failure can skip this sync; see [Litestream's signal handling](https://github.com/benbjohnson/litestream/blob/v0.5.16/cmd/litestream/main.go#L172-L197).

A successful counter write response confirms a local commit. Readiness checks database access and writer availability; it does not certify replica health.
Node loss, forced termination, or replica outages can lose unreplicated writes.
Litestream 0.5.16 can log final sync failures without a failing exit status. Inspect shutdown logs and verify restoration.

Keep one owner per replica prefix. Stop or fence a partitioned owner before replacing it; the chart supplies no distributed ownership lock.
Replacement interrupts service while the new pod restores.

After incrementing the counter, test normal replacement:

```sh
kubectl --namespace "$DEPLOY_ENV" rollout restart statefulset/sqlite-web-starter
kubectl --namespace "$DEPLOY_ENV" rollout status statefulset/sqlite-web-starter
```

Restart port forwarding and read `/counter`. Successful final replication preserves the previous value.

## Build and validate

Use the pinned Rust toolchain and [justfile](../justfile):

| Command | Check |
| --- | --- |
| `just build` / `just run` | Build or run locally |
| `just fmt` / `just lint` | Apply formatting / check formatting and Clippy |
| `just test` / `just test-release` | Rust / isolated release tests |
| `just helm` | Lint and render all chart environments |
| `just docker` / `just smoke` | Build image / build and verify container restoration |
| `just local-ci` | Lint, tests, chart checks, and container smoke test |

Install just, rustup, Bash, Helm 3, Docker, Git, jq, OpenSSH tools, curl, and standard shell utilities for `just local-ci`.
Release tests use disposable signing keys and repositories, without cloud access.

The HTTP integration suite starts at [tests/api/main.rs](../tests/api/main.rs). See [test conventions](../tests/README.md) for the directory layout.
Its [shared helper](../tests/api/helpers.rs) gives each test an isolated SQLite database, a writer task, and the real application router.
Requests run through Axum in process, so these tests need no bound port or external services.
Each test shuts down its writer before removing temporary files.

Keep unit tests beside their implementation in `src/`; reserve `tests/` for API integration tests.
[startup.rs](../src/startup.rs) contains private startup and shutdown checks inline, including unexpected HTTP and writer task failures.
Its `#[cfg(test)]` module keeps the checks together with their implementation and provides access to private runtime internals.

Run either group directly:

```sh
cargo test --locked --test api
cargo test --locked --lib startup::tests
```

The [smoke test](../smoke.sh) checks the packaged CSS/JavaScript responses and verifies committed writes survive SIGTERM and restore into a fresh container.
It disables periodic capture to exercise final sync, then removes its containers and replica volume.
Run `./smoke.sh IMAGE` to test an existing image. This checks neither Kubernetes scheduling nor S3 permissions.
