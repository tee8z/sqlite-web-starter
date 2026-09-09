#!/usr/bin/env bash

set -euo pipefail

die() {
    echo "Error: $*" >&2
    exit 1
}

[[ $# -eq 2 ]] || die 'Usage: wait_for_ci.sh SOURCE_SHA DEFAULT_BRANCH'
source_sha="$1"
default_branch="$2"
[[ "$source_sha" =~ ^[0-9a-f]{40}$ && -n "$default_branch" && "$default_branch" != -* ]] ||
    die 'A full, lowercase commit SHA and a default branch are required.'
[[ -n "${GH_TOKEN:-}" && -n "${GH_REPO:-}" ]] || die 'Set GH_TOKEN and GH_REPO before waiting for CI.'

wait_timeout="${CI_WAIT_TIMEOUT_SECONDS:-3600}"
poll_seconds="${CI_WAIT_POLL_SECONDS:-15}"
[[ "$wait_timeout" =~ ^[1-9][0-9]{0,3}$ ]] && ((wait_timeout <= 3600)) ||
    die 'CI_WAIT_TIMEOUT_SECONDS must be an integer from 1 to 3600.'
[[ "$poll_seconds" =~ ^[1-9][0-9]?$ ]] && ((poll_seconds <= 60 && poll_seconds <= wait_timeout)) ||
    die 'CI_WAIT_POLL_SECONDS must be an integer from 1 to 60, no greater than the timeout.'
for dependency in gh jq sleep; do
    command -v "$dependency" >/dev/null || die "Required command not found: $dependency"
done

deadline=$((SECONDS + wait_timeout))
while ((SECONDS < deadline)); do
    if ! response=$(gh run list \
        --workflow ci.yaml \
        --event push \
        --commit "$source_sha" \
        --branch "$default_branch" \
        --json databaseId,status,conclusion,url,headSha,headBranch,event \
        --limit 1); then
        die "Could not read CI runs for $source_sha on $default_branch."
    fi

    # Check identity even though gh applies the same filters. Unexpected data
    # must not authorize promotion of an image from a different commit or event.
    if ! state=$(jq -er --arg sha "$source_sha" --arg branch "$default_branch" '
        if type != "array" then error("expected an array")
        elif length == 0 then "missing"
        elif length != 1 then error("expected at most one run")
        else .[0] |
            if type != "object"
                or .headSha != $sha or .headBranch != $branch or .event != "push"
                or (.databaseId | type != "number" or . <= 0 or . != floor)
                or (.url | type != "string" or length == 0)
            then error("invalid CI run identity")
            elif .status == "completed" then
                if .conclusion == "success" then "success"
                elif (.conclusion | IN("failure", "cancelled", "timed_out", "action_required",
                    "neutral", "skipped", "stale", "startup_failure")) then "failed"
                else error("invalid completed conclusion") end
            elif (.status | IN("queued", "in_progress", "pending", "requested", "waiting"))
                and (.conclusion == null or .conclusion == "") then "pending"
            else error("invalid CI run status") end
        end
    ' <<< "$response"); then
        die 'GitHub returned malformed or unexpected CI run data.'
    fi

    case "$state" in
        success)
            echo "CI passed for $source_sha on $default_branch."
            exit 0
            ;;
        failed)
            conclusion=$(jq -r '.[0].conclusion' <<< "$response")
            run_url=$(jq -r '.[0].url' <<< "$response")
            die "CI for $source_sha concluded $conclusion: $run_url"
            ;;
        missing) echo "Waiting for CI to start for $source_sha on $default_branch." ;;
        pending) echo "Waiting for CI to finish for $source_sha on $default_branch." ;;
    esac

    remaining=$((deadline - SECONDS))
    ((remaining > 0)) || break
    delay="$poll_seconds"
    ((delay <= remaining)) || delay="$remaining"
    sleep "$delay"
done

die "Timed out after $wait_timeout seconds waiting for CI for $source_sha on $default_branch."
