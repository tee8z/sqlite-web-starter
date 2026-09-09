#!/bin/sh
set -eu

# Shared by the daemon configuration and the Rust startup sync barrier.
export LITESTREAM_SOCKET="${LITESTREAM_SOCKET:-/data/litestream.sock}"
export LITESTREAM_SYNC_INTERVAL="${LITESTREAM_SYNC_INTERVAL:-1s}"

# Existing files survive container restarts; replacement pods restore from replica.
litestream restore -if-db-not-exists -if-replica-exists -integrity-check quick "$SQLITE_PATH"
# Tini is PID 1; Litestream waits for the Rust child before its final replica sync.
exec litestream replicate -exec /usr/local/bin/sqlite-web-starter
