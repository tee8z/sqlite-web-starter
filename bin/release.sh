#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: release.sh VERSION COMMIT [--execute]

Preview a stable release, or create and push its annotated tag with --execute.
VERSION is vX.Y.Z (a bare X.Y.Z is also accepted). COMMIT is required and may
be a commit SHA or a Git ref. It must be on origin's current default branch.

This helper is optional. To release the current default-branch commit directly:
  git tag v0.1.0
  git push origin --tags
GitHub Actions waits for CI, promotes the image, and opens the production PR.
The PR enables auto-merge unless RELEASE_AUTO_MERGE is false in repository vars.

Both modes refresh that branch, check local and remote release versions, and
verify the selected commit's full-SHA image in ECR. Preview creates no tags.

Requires Bash, Git, AWS CLI, jq, and authenticated AWS credentials for ECR reads.
ECR_REPOSITORY defaults to sqlite-web-starter; AWS_REGION to us-west-2.
Set them to match the repository's CI variables. AWS uses your current account.

Examples:
  bin/release.sh v0.1.0 abc1234
  bin/release.sh v0.1.0 abc1234 --execute
EOF
}

die() {
    echo "Error: $*" >&2
    exit 1
}

if [[ $# -eq 1 && ( "$1" == --help || "$1" == -h ) ]]; then
    usage
    exit 0
fi
if [[ $# -lt 2 || $# -gt 3 ]]; then
    usage >&2
    exit 1
fi
execute=false
if [[ $# -eq 3 ]]; then
    [[ "$3" == --execute ]] || die "Unknown option: $3"
    execute=true
fi

tag="v${1#v}"
stable_version='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
[[ "$tag" =~ $stable_version ]] || die 'Use a stable version such as v0.1.0.'
[[ -n "$2" && "$2" != -* ]] || die 'An explicit commit SHA or Git ref is required.'

for dependency in git aws jq; do
    command -v "$dependency" >/dev/null || die "Required command not found: $dependency"
done
git rev-parse --git-dir >/dev/null 2>&1 || die 'Run this command from the Git repository.'

local_tags=$(git tag --list)
remote_tags=$(git ls-remote --tags --refs origin) || die 'Could not read origin tags.'
stable_tags=()
check_tag() {
    local existing="$1"
    [[ "$existing" != "$tag" ]] || die "Tag $tag already exists locally or on origin."
    if [[ "$existing" =~ $stable_version ]]; then
        stable_tags+=("$existing")
    fi
}
while IFS= read -r existing; do
    check_tag "$existing"
done <<< "$local_tags"
while read -r _object ref; do
    check_tag "${ref#refs/tags/}"
done <<< "$remote_tags"

if [[ ${#stable_tags[@]} -gt 0 ]]; then
    # Filter stable tags before sorting; tail consumes all input under pipefail.
    latest=$(printf '%s\n' "${stable_tags[@]}" | LC_ALL=C sort -V | tail -n 1)
    newest=$(printf '%s\n' "$latest" "$tag" | LC_ALL=C sort -V | tail -n 1)
    [[ "$newest" == "$tag" ]] || die "$tag must be later than the latest stable release, $latest."
fi

remote_head=$(git ls-remote --symref origin HEAD) || die 'Could not read origin default branch.'
default_branch=''
while read -r marker ref name; do
    if [[ "$marker" == 'ref:' && "$ref" == refs/heads/* && "$name" == HEAD ]]; then
        default_branch="${ref#refs/heads/}"
    fi
done <<< "$remote_head"
[[ -n "$default_branch" ]] || die 'Origin HEAD must identify its default branch.'

fetch_args=(--no-tags)
if [[ $(git rev-parse --is-shallow-repository) == true ]]; then
    fetch_args+=(--unshallow)
fi
git fetch "${fetch_args[@]}" origin "+refs/heads/$default_branch:refs/remotes/origin/$default_branch"
commit=$(git rev-parse --verify --end-of-options "$2^{commit}") || die "Could not resolve commit: $2"
git merge-base --is-ancestor "$commit" "refs/remotes/origin/$default_branch" ||
    die "Commit $commit is not reachable from origin/$default_branch."

repository="${ECR_REPOSITORY:-sqlite-web-starter}"
region="${AWS_REGION:-us-west-2}"
response=$(AWS_PAGER='' aws ecr batch-get-image \
    --region "$region" \
    --repository-name "$repository" \
    --image-ids "imageTag=$commit" \
    --output json) || die "Could not read ECR image $repository:$commit in $region."
if ! jq -e --arg commit "$commit" '
    (.images | type == "array" and length == 1)
    and ((.failures // []) | type == "array" and length == 0)
    and .images[0].imageId.imageTag == $commit
' <<< "$response" >/dev/null; then
    die "ECR did not return the image $repository:$commit in $region. Wait for CI to publish it and check your AWS account and settings."
fi

printf 'Release: %s\nCommit: %s\nDefault branch: origin/%s\nImage: %s:%s (%s)\n' \
    "$tag" "$commit" "$default_branch" "$repository" "$commit" "$region"
if [[ "$execute" == false ]]; then
    echo "Preview passed. Add --execute to create annotated tag $tag and push it to origin."
    exit 0
fi

git tag --annotate "$tag" "$commit" --message "Release $tag"
if ! git push --no-follow-tags origin "refs/tags/$tag:refs/tags/$tag"; then
    die "Could not push $tag. The local tag remains; inspect origin before retrying."
fi
echo "Pushed $tag to origin. The release workflow will promote the image and open the production PR."
