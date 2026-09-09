set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments

default:
    @just --list

# Build the Rust executable for local development.
build:
    cargo build --locked

run:
    #!/usr/bin/env bash
    set -euo pipefail
    export RUST_LOG_STYLE="${RUST_LOG_STYLE:-always}"
    if [[ -t 0 ]]; then
        # Let Cargo own terminal signals so just preserves a graceful exit.
        set -m
        cargo run --locked &
        run_pid=$!
        # Also stop and reap Cargo if the launcher itself is terminated.
        trap 'kill -TERM -- "-$run_pid" 2>/dev/null || true; wait "$run_pid" 2>/dev/null || true' EXIT
        fg %1
    else
        exec cargo run --locked
    fi

# Apply formatting explicitly; lint and local-ci only check it.
fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

clippy:
    cargo clippy --locked --all-targets --all-features -- -D warnings

lint: fmt-check clippy

test:
    cargo test --locked --all-features

# Use isolated Git repositories and fake service CLIs; no cloud access.
test-release:
    bash bin/tests/release.sh
    bash bin/tests/wait_for_ci.sh

# Validate the default chart and both environment overlays.
helm:
    helm lint ./chart
    helm template sqlite-web-starter ./chart > /dev/null
    for environment in staging prod; do helm lint ./chart -f "chart/values.$environment.yaml"; helm template sqlite-web-starter ./chart --namespace "$environment" -f "chart/values.$environment.yaml" > /dev/null; done

docker image="sqlite-web-starter:local":
    docker build --platform linux/amd64 --tag {{ quote(image) }} .

# Build first, then verify writes and graceful restoration in the container.
smoke image="sqlite-web-starter:local": (docker image)
    bash smoke.sh {{ quote(image) }}

local-ci: lint test test-release helm smoke

# Preview a version tag for an explicit commit; add --execute to push it.
release version commit *args:
    bash bin/release.sh "$@"
