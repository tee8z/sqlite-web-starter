#!/usr/bin/env bash

# Exercise CI polling without GitHub access or Git commits.
set -euo pipefail

wait_script="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)/ci/wait_for_ci.sh"
command -v jq >/dev/null || { echo 'Missing test dependency: jq' >&2; exit 1; }
real_sleep=$(command -v sleep)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir "$scratch/mock-bin"
export WAIT_TEST_DIR="$scratch" WAIT_TEST_REAL_SLEEP="$real_sleep"
export GH_TOKEN=test-token GH_REPO=example/repository
export WAIT_TEST_SHA=0123456789abcdef0123456789abcdef01234567 WAIT_TEST_BRANCH=trunk
export CI_WAIT_TIMEOUT_SECONDS=10 CI_WAIT_POLL_SECONDS=1
export PATH="$scratch/mock-bin:$PATH"

cat > "$scratch/mock-bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ $# -eq 14 && "$1" == run && "$2" == list && "$3" == --workflow && "$4" == ci.yaml &&
    "$5" == --event && "$6" == push && "$7" == --commit && "$8" == "$WAIT_TEST_SHA" &&
    "$9" == --branch && "${10}" == "$WAIT_TEST_BRANCH" && "${11}" == --json &&
    "${12}" == databaseId,status,conclusion,url,headSha,headBranch,event &&
    "${13}" == --limit && "${14}" == 1 ]] || exit 90
count=$(< "$WAIT_TEST_DIR/calls")
count=$((count + 1))
echo "$count" > "$WAIT_TEST_DIR/calls"
if [[ "$WAIT_TEST_CASE" == api-error ]]; then
    echo 'HTTP 403: API denied' >&2
    exit 1
fi
if [[ -f "$WAIT_TEST_DIR/response-$count" ]]; then
    cat "$WAIT_TEST_DIR/response-$count"
else
    cat "$WAIT_TEST_DIR/response-last"
fi
EOF

cat > "$scratch/mock-bin/sleep" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "$1" >> "$WAIT_TEST_DIR/sleeps"
if [[ "$WAIT_TEST_CASE" == timeout ]]; then
    exec "$WAIT_TEST_REAL_SLEEP" "$@"
fi
EOF
chmod +x "$scratch/mock-bin/gh" "$scratch/mock-bin/sleep"

checks=0
fixture() {
    export WAIT_TEST_CASE="$1" CI_WAIT_TIMEOUT_SECONDS=10 CI_WAIT_POLL_SECONDS=1
    rm -f "$scratch"/response-*
    echo 0 > "$scratch/calls"
    : > "$scratch/sleeps"
}
run_json() {
    local status="$1" conclusion="$2"
    jq -n --arg sha "$WAIT_TEST_SHA" --arg branch "$WAIT_TEST_BRANCH" \
        --arg status "$status" --arg conclusion "$conclusion" '
        [{databaseId: 123, status: $status, conclusion: $conclusion,
          url: "https://github.com/example/repository/actions/runs/123",
          headSha: $sha, headBranch: $branch, event: "push"}]'
}
fail() {
    echo "FAIL: $*" >&2
    cat "$scratch/output" >&2
    exit 1
}
run_wait() {
    bash "$wait_script" "$@" > "$scratch/output" 2>&1
}
expect_success() {
    run_wait "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH" || fail 'Expected success'
    [[ $(< "$scratch/output") == *'CI passed'* ]] || fail 'Missing successful CI confirmation'
}
expect_failure() {
    local message="$1"
    shift
    if run_wait "$@"; then
        fail 'Expected failure'
    fi
    [[ $(< "$scratch/output") == *"$message"* ]] || fail "Expected error containing: $message"
}
pass() {
    checks=$((checks + 1))
    echo "PASS: $*"
}

fixture success
run_json completed success > "$scratch/response-last"
expect_success
[[ $(< "$scratch/calls") == 1 && ! -s "$scratch/sleeps" ]] || fail 'Successful CI should not poll again'
pass 'completed successful CI proceeds immediately'

fixture pending
run_json in_progress '' > "$scratch/response-1"
run_json completed success > "$scratch/response-last"
expect_success
[[ $(< "$scratch/calls") == 2 && $(< "$scratch/sleeps") == 1 ]] || fail 'Expected one pending poll'
pass 'running CI waits before success'

fixture missing
echo '[]' > "$scratch/response-1"
run_json queued '' > "$scratch/response-2"
run_json completed success > "$scratch/response-last"
expect_success
[[ $(< "$scratch/calls") == 3 ]] || fail 'Missing and queued CI must both be polled'
pass 'missing run and queued run wait for success'

for conclusion in failure cancelled timed_out action_required neutral skipped stale startup_failure; do
    fixture "$conclusion"
    run_json completed "$conclusion" > "$scratch/response-last"
    expect_failure "concluded $conclusion" "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
    [[ $(< "$scratch/calls") == 1 && ! -s "$scratch/sleeps" ]] || fail 'Unsuccessful CI must fail immediately'
done
pass 'every unsuccessful completed conclusion blocks promotion'

for field in headSha headBranch event; do
    fixture wrong-identity
    run_json completed success | jq --arg field "$field" '.[0][$field] = "wrong"' > "$scratch/response-last"
    expect_failure 'unexpected CI run data' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
done
pass 'wrong commit, branch, and event cannot authorize promotion'

for response in 'not JSON' '{}' '[{}]' '[null]' '[{}, {}]'; do
    fixture malformed
    echo "$response" > "$scratch/response-last"
    expect_failure 'unexpected CI run data' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
done
for mutation in '.[0].databaseId = null' '.[0].url = ""' '.[0].status = "unknown"' \
    '.[0].conclusion = "unknown"' '.[0].status = "in_progress"'; do
    fixture malformed
    run_json completed success | jq "$mutation" > "$scratch/response-last"
    expect_failure 'unexpected CI run data' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
done
pass 'malformed responses, identities, statuses, and conclusions fail'

fixture api-error
expect_failure 'Could not read CI runs' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
[[ $(< "$scratch/calls") == 1 && ! -s "$scratch/sleeps" ]] || fail 'API errors must fail immediately'
pass 'API errors stop polling'

fixture timeout
export CI_WAIT_TIMEOUT_SECONDS=1
echo '[]' > "$scratch/response-last"
expect_failure 'Timed out after 1 seconds' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
[[ $(< "$scratch/calls") == 1 && $(< "$scratch/sleeps") == 1 ]] || fail 'Timeout should stop after the bounded delay'
pass 'missing CI times out within the configured bound'

fixture invalid-input
expect_failure 'Usage:'
expect_failure 'full, lowercase commit SHA' abc "$WAIT_TEST_BRANCH"
expect_failure 'default branch' "$WAIT_TEST_SHA" ''
for value in 0 3601 -1 1.5 01 invalid 999999999999999999999; do
    export CI_WAIT_TIMEOUT_SECONDS="$value"
    expect_failure 'CI_WAIT_TIMEOUT_SECONDS' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
done
export CI_WAIT_TIMEOUT_SECONDS=10
for value in 0 61 -1 1.5 01 invalid 11; do
    export CI_WAIT_POLL_SECONDS="$value"
    expect_failure 'CI_WAIT_POLL_SECONDS' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
done
export CI_WAIT_POLL_SECONDS=1
GH_TOKEN='' expect_failure 'Set GH_TOKEN and GH_REPO' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
GH_REPO='' expect_failure 'Set GH_TOKEN and GH_REPO' "$WAIT_TEST_SHA" "$WAIT_TEST_BRANCH"
[[ $(< "$scratch/calls") == 0 ]] || fail 'Invalid inputs must not call GitHub'
pass 'arguments, credentials, timeout, and poll intervals are validated before API access'

echo "$checks CI wait test groups passed."
