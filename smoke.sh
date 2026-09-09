#!/usr/bin/env bash
# Exercise browser-form/API writes and graceful restore using disposable Docker data.
set -euo pipefail

image=${1:-sqlite-web-starter:local}
prefix="sqlite-web-starter-$$-$RANDOM"
replica="$prefix-replica"
copy="$prefix-copy"
temporary=$(mktemp -d)
containers=()
volumes=()

cleanup() {
    status=$?
    trap - EXIT
    if (( status != 0 )); then
        for container in "${containers[@]}"; do
            if docker inspect "$container" >/dev/null 2>&1; then
                printf '\n--- %s ---\n' "$container" >&2
                docker logs "$container" >&2 || true
            fi
        done
    fi
    for container in "${containers[@]}"; do
        docker rm -f "$container" >/dev/null 2>&1 || true
    done
    for volume in "${volumes[@]}"; do
        docker volume rm "$volume" >/dev/null 2>&1 || true
    done
    rm -rf "$temporary"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

for command in docker curl sort diff seq; do
    command -v "$command" >/dev/null || fail "missing command: $command"
done

# Startup's explicit sync initializes Litestream and publishes the baseline.
# Disable background WAL capture in this test, so even the replica monitor's
# immediate first iteration cannot upload later writes. Close must capture and
# upload them. Production keeps its normal background database monitoring.
cat >"$temporary/litestream.yml" <<'CONFIG'
socket:
  enabled: true
  path: ${LITESTREAM_SOCKET}
  permissions: 0600
shutdown-sync-timeout: 45s
shutdown-sync-interval: 1s
dbs:
  - path: ${SQLITE_PATH}
    monitor-interval: 0s
    replica:
      url: ${LITESTREAM_REPLICA_URL}
      sync-interval: 1h
CONFIG
chmod 644 "$temporary/litestream.yml"

request() {
    curl --fail --silent --show-error --max-time 10 "$@"
}

counter() {
    local body pattern='^[[:space:]]*\{[[:space:]]*"value"[[:space:]]*:[[:space:]]*([0-9]+)[[:space:]]*\}[[:space:]]*$'
    body=$(request "$@")
    [[ $body =~ $pattern ]] || fail "invalid counter response: $body"
    printf '%s\n' "${BASH_REMATCH[1]}"
}

check_counter() {
    local expected=$1 actual
    actual=$(counter "$base/counter")
    [[ $actual == "$expected" ]] || fail "expected counter $expected, got $actual"
}

check_page() {
    local expected=$1 page=$2 item
    local pattern="id=\"counter-value\"[^>]*>[[:space:]]*$expected[[:space:]]*<"
    [[ $page =~ $pattern ]] || fail "page did not render counter $expected: $page"
    [[ $page == *'id="sample-data"'* ]] || fail "page is missing the inventory table"
    for item in '<td>Notebooks</td><td>12</td>' '<td>Pencils</td><td>48</td>' '<td>Mugs</td><td>6</td>'; do
        [[ $page == *"$item"* ]] || fail "page is missing inventory row: $item"
    done
    [[ $page == *'<form method="post" action="/increment">'* ]] || fail "page is missing the increment form"
    [[ $page == *'>Increment counter</button>'* ]] || fail "page is missing the increment button"
}

check_asset() {
    local url=$1 expected_type=$2 header value
    local content_type='' cache_control='' nosniff=''
    request --dump-header "$temporary/asset-headers" \
        --output "$temporary/asset-body" "$base$url"
    while IFS=: read -r header value; do
        value=${value# }
        value=${value%$'\r'}
        case $header in
            [Cc][Oo][Nn][Tt][Ee][Nn][Tt]-[Tt][Yy][Pp][Ee]) content_type=$value ;;
            [Cc][Aa][Cc][Hh][Ee]-[Cc][Oo][Nn][Tt][Rr][Oo][Ll]) cache_control=$value ;;
            [Xx]-[Cc][Oo][Nn][Tt][Ee][Nn][Tt]-[Tt][Yy][Pp][Ee]-[Oo][Pp][Tt][Ii][Oo][Nn][Ss]) nosniff=$value ;;
        esac
    done <"$temporary/asset-headers"
    [[ $content_type == "$expected_type" ]] || fail "unexpected Content-Type for $url: $content_type"
    [[ $cache_control == 'public, max-age=31536000, immutable' ]] || fail "missing immutable caching for $url"
    [[ $nosniff == nosniff ]] || fail "missing nosniff for $url"
    [[ -s $temporary/asset-body ]] || fail "empty asset: $url"
}

check_assets() {
    local page=$1 css_url js_url
    local css_pattern='href="(/assets/site\.[a-f0-9]{64}\.css)"'
    local js_pattern='src="(/assets/site\.[a-f0-9]{64}\.js)"'
    [[ $page =~ $css_pattern ]] || fail "page is missing a hashed stylesheet URL"
    css_url=${BASH_REMATCH[1]}
    [[ $page =~ $js_pattern ]] || fail "page is missing a hashed JavaScript URL"
    js_url=${BASH_REMATCH[1]}
    check_asset "$css_url" 'text/css; charset=utf-8'
    check_asset "$js_url" 'text/javascript; charset=utf-8'
}

prepare_volume() {
    local volume=$1 helper="$prefix-prepare-${#volumes[@]}"
    volumes+=("$volume")
    docker volume create "$volume" >/dev/null
    containers+=("$helper")
    docker run --rm --name "$helper" --user 0 -v "$volume:/replica" \
        --entrypoint chown "$image" 10001:10001 /replica
}

# Sets name and base in the calling shell, so cleanup can track every container.
start() {
    local phase=$1 volume=$2 deadline
    name="$prefix-$phase"
    containers+=("$name")
    docker run -d --name "$name" \
        --tmpfs /data:uid=10001,gid=10001,mode=0700 \
        -v "$volume:/replica" \
        -v "$temporary/litestream.yml:/etc/litestream.yml:ro" \
        -e LITESTREAM_REPLICA_URL=file:///replica \
        -p 127.0.0.1::3000 "$image" >/dev/null
    base="http://$(docker port "$name" 3000/tcp)"
    deadline=$((SECONDS + 60))
    while (( SECONDS < deadline )); do
        if curl --fail --silent --max-time 2 "$base/ready" >/dev/null; then
            request "$base/healthy" >/dev/null
            return
        fi
        [[ $(docker inspect --format '{{.State.Running}}' "$name") == true ]] || break
        sleep 0.1
    done
    fail "$name did not become ready"
}

stop() {
    local container=$1 code
    docker stop --time 90 "$container" >/dev/null
    code=$(docker inspect --format '{{.State.ExitCode}}' "$container")
    [[ $code == 0 ]] || fail "$container exited with code $code"
}

prepare_volume "$replica"
printf 'Checking fresh startup followed immediately by a write and SIGTERM...\n'
start rapid "$replica"
[[ $(counter --request POST "$base/counter") == 1 ]] || fail "fresh counter did not increment to 1"
stop "$name"

printf 'Checking fresh-disk restore, Maud HTML/assets, form submission, and concurrent writes...\n'
start owner "$replica"
owner=$name
owner_base=$base
check_counter 1
page=$(request "$base/")
check_page 1 "$page"
check_assets "$page"
# curl follows the 303 using GET when POST is selected with --data.
check_page 2 "$(request --location --data '' "$base/increment")"

# Keep eight requests in flight per batch, and verify every committed reply is unique.
for batch in 0 1 2 3; do
    workers=()
    for index in 0 1 2 3 4 5 6 7; do
        counter --request POST "$base/counter" >"$temporary/value-$batch-$index" &
        workers+=("$!")
    done
    failed=false
    for worker in "${workers[@]}"; do
        wait "$worker" || failed=true
    done
    [[ $failed == false ]] || fail "a concurrent write failed"
done
diff -u <(seq 3 34) <(sort -n "$temporary"/value-*)
expected=$(counter --request POST "$base/counter")
[[ $expected == 35 ]] || fail "expected final committed counter 35, got $expected"

printf 'Proving committed writes are absent from the replica before shutdown...\n'
prepare_volume "$copy"
helper="$prefix-copy-replica"
containers+=("$helper")
# The probe gets its own replica prefix; it cannot modify the live owner's replica.
docker run --rm --name "$helper" --user 0 \
    -v "$replica:/source:ro" -v "$copy:/replica" \
    --entrypoint cp "$image" -a /source/. /replica/
start probe "$copy"
check_counter 1
stop "$name"

printf 'Checking SIGTERM final sync and restore into another empty database directory...\n'
base=$owner_base
check_counter "$expected"
stop "$owner"
start restored "$replica"
check_counter "$expected"
check_page "$expected" "$(request "$base/")"
check_page "$((expected + 1))" "$(request --location --data '' "$base/increment")"
check_counter "$((expected + 1))"
check_page "$((expected + 1))" "$(request "$base/")"
stop "$name"
printf 'PASS: rapid shutdown, embedded CSS/JS, HTML/form/API writes, and deterministic final-sync restore of %s commits\n' "$expected"
