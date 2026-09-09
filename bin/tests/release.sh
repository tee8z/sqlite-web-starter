#!/usr/bin/env bash

# Exercise the release helper with disposable Git remotes and a mock AWS CLI.
set -euo pipefail

release_script="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)/release.sh"
for dependency in git jq ssh-keygen; do
    command -v "$dependency" >/dev/null || { echo "Missing test dependency: $dependency" >&2; exit 1; }
done

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir "$scratch/mock-bin" "$scratch/hooks"
export GIT_CONFIG_GLOBAL="$scratch/gitconfig" GIT_CONFIG_NOSYSTEM=1
ssh-keygen -q -t ed25519 -N '' -f "$scratch/signing-key"
git config --global user.name 'Release Helper Test'
git config --global user.email 'release-test@example.invalid'
git config --global gpg.format ssh
git config --global user.signingkey "$scratch/signing-key"
git config --global commit.gpgsign true
git config --global tag.gpgsign false
git config --global core.hooksPath "$scratch/hooks"
export AWS_CALL_FILE="$scratch/aws-call"
export PATH="$scratch/mock-bin:$PATH"
unset AWS_REGION ECR_REPOSITORY

cat > "$scratch/mock-bin/aws" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$AWS_CALL_FILE"
[[ $# -eq 10 && "$1" == ecr && "$2" == batch-get-image &&
   "$3" == --region && "$4" == "${AWS_REGION:-us-west-2}" &&
   "$5" == --repository-name && "$6" == "${ECR_REPOSITORY:-sqlite-web-starter}" &&
   "$7" == --image-ids && "$8" == "imageTag=$AWS_EXPECTED_SHA" &&
   "$9" == --output && "${10}" == json ]] || exit 90
case "$AWS_MOCK_MODE" in
    found) jq -n --arg tag "$AWS_EXPECTED_SHA" '{images: [{imageId: {imageTag: $tag}}], failures: []}' ;;
    missing) echo '{"images":[],"failures":[{"failureCode":"ImageNotFound"}]}' ;;
    denied) echo 'AccessDeniedException: test denial' >&2; exit 1 ;;
    malformed) echo 'not json' ;;
    wrong-tag) echo '{"images":[{"imageId":{"imageTag":"other"}}],"failures":[]}' ;;
    mixed) jq -n --arg tag "$AWS_EXPECTED_SHA" '{images: [{imageId: {imageTag: $tag}}], failures: [{failureCode: "Denied"}]}' ;;
    *) exit 91 ;;
esac
EOF
chmod +x "$scratch/mock-bin/aws"

fixtures=0
checks=0
signed_commit() {
    git -C "$work" status --short >/dev/null
    git -C "$work" commit -S --allow-empty --quiet --message 'test: add release fixture'
}
fixture() {
    fixtures=$((fixtures + 1))
    remote="$scratch/origin-$fixtures.git"
    work="$scratch/work-$fixtures"
    git init --bare --initial-branch=trunk --quiet "$remote"
    git init --initial-branch=trunk --quiet "$work"
    git -C "$work" remote add origin "$remote"
    signed_commit
    git -C "$work" push --quiet origin trunk
    commit=$(git -C "$work" rev-parse HEAD)
    export AWS_EXPECTED_SHA="$commit" AWS_MOCK_MODE=found
}
fail() {
    echo "FAIL: $*" >&2
    cat "$scratch/output" >&2
    exit 1
}
run_release() {
    : > "$AWS_CALL_FILE"
    (cd "$work" && "$release_script" "$@") > "$scratch/output" 2>&1
}
expect_success() {
    run_release "$@" || fail "Expected success: $*"
}
expect_failure() {
    local message="$1"
    shift
    if run_release "$@"; then
        fail "Expected failure: $*"
    fi
    [[ $(< "$scratch/output") == *"$message"* ]] || fail "Expected error containing: $message"
}
assert_no_tag() {
    [[ -z $(git -C "$work" tag --list "$1") ]] || fail "Created local tag $1 unexpectedly"
    [[ -z $(git ls-remote --tags "$remote" "refs/tags/$1") ]] || fail "Pushed tag $1 unexpectedly"
}
pass() {
    checks=$((checks + 1))
    echo "PASS: $*"
}

fixture
expect_success --help
expect_failure 'Usage:'
expect_failure 'Usage:' v0.1.0
expect_failure 'Usage:' v0.1.0 HEAD --execute extra
expect_failure 'Unknown option' v0.1.0 HEAD --wrong
expect_failure 'explicit commit' v0.1.0 ''
expect_failure 'explicit commit' v0.1.0 --execute
for version in '' v1.0 v01.0.0 v1.0.0-rc.1 v1.0.0+build; do
    expect_failure 'stable version' "$version" "$commit"
done
[[ ! -s "$AWS_CALL_FILE" ]] || fail 'Invalid arguments reached AWS'
pass 'help and invalid arguments'

expect_success 0.1.0 "$commit"
[[ $(< "$scratch/output") == *"Preview passed"* ]] || fail 'Missing preview output'
assert_no_tag v0.1.0
pass 'first release accepts bare version and preview creates no tags'

git -C "$work" tag v0.1.0
expect_failure 'already exists' v0.1.0 "$commit" --execute
[[ ! -s "$AWS_CALL_FILE" ]] || fail 'Duplicate tag reached AWS'
pass 'local duplicate is rejected before ECR lookup'

fixture
git -C "$work" tag v0.1.0
git -C "$work" push --quiet origin refs/tags/v0.1.0
git -C "$work" tag --delete v0.1.0 >/dev/null
expect_failure 'already exists' v0.1.0 "$commit" --execute
[[ -z $(git -C "$work" tag --list) ]] || fail 'Remote tag was fetched'
pass 'remote-only duplicate is rejected without fetching tags'

fixture
git -C "$work" tag v1.10.0
git -C "$work" push --quiet origin refs/tags/v1.10.0
git -C "$work" tag --delete v1.10.0 >/dev/null
expect_failure 'later than' v1.9.0 "$commit" --execute
assert_no_tag v1.9.0
pass 'remote stable versions are compared numerically'

fixture
git -C "$work" tag v1.10.0
git -C "$work" tag v99.0.0-rc.1
git -C "$work" tag v98.0.0+build
git -C "$work" tag unrelated
expect_failure 'later than' v1.9.0 "$commit"
expect_success v1.11.0 "$commit"
pass 'local versions are enforced and prereleases are filtered before sorting'

fixture
git -C "$work" tag v99.0.0-rc.1
expect_success v0.1.0 "$commit"
pass 'prerelease-only history permits the first stable release'

fixture
expect_failure 'Could not resolve commit' v0.1.0 nonexistent-ref --execute
[[ ! -s "$AWS_CALL_FILE" ]] || fail 'Invalid commit reached AWS'
assert_no_tag v0.1.0
git -C "$work" switch --quiet --create unmerged
signed_commit
off_branch=$(git -C "$work" rev-parse HEAD)
expect_failure 'not reachable' v0.1.0 "$off_branch" --execute
[[ ! -s "$AWS_CALL_FILE" ]] || fail 'Unmerged commit reached AWS'
assert_no_tag v0.1.0
pass 'invalid and unmerged commits are rejected before ECR lookup'

fixture
signed_commit
new_commit=$(git -C "$work" rev-parse HEAD)
git -C "$work" push --quiet origin trunk
git -C "$work" update-ref refs/remotes/origin/trunk "$commit"
export AWS_EXPECTED_SHA="$new_commit"
expect_success v0.1.0 origin/trunk
[[ $(git -C "$work" rev-parse origin/trunk) == "$new_commit" ]] || fail 'Default branch was not refreshed'
pass 'default branch is discovered and refreshed before resolving a ref'

fixture
for mode in missing denied malformed wrong-tag mixed; do
    export AWS_MOCK_MODE="$mode"
    expect_failure 'ECR' v0.1.0 "$commit" --execute
    assert_no_tag v0.1.0
done
pass 'missing, unauthorized, malformed, mismatched, and mixed ECR responses fail safely'

export AWS_MOCK_MODE=found AWS_REGION=us-east-1 ECR_REPOSITORY=custom/repository
expect_success v0.1.0 "$commit"
assert_no_tag v0.1.0
unset AWS_REGION ECR_REPOSITORY
pass 'ECR region and repository overrides are passed to AWS'

fixture
selected="$commit"
signed_commit
git -C "$work" push --quiet origin trunk
git -C "$work" tag --annotate unrelated --message unrelated "$selected"
git -C "$work" config push.followTags true
expect_success v0.1.0 "$selected" --execute
[[ $(git -C "$work" rev-parse 'v0.1.0^{commit}') == "$selected" ]] || fail 'Tagged HEAD instead of the selected commit'
[[ $(git -C "$work" cat-file -t refs/tags/v0.1.0) == tag ]] || fail 'Release tag is not annotated'
[[ $(git --git-dir="$remote" rev-parse 'v0.1.0^{commit}') == "$selected" ]] || fail 'Remote release tag targets the wrong commit'
[[ -z $(git ls-remote --tags "$remote" refs/tags/unrelated) ]] || fail 'Pushed an unrelated tag'
pass 'execute publishes only an annotated tag at the exact selected commit'

fixture
selected="$commit"
signed_commit
git -C "$work" push --quiet origin trunk
git clone --quiet --depth 1 "file://$remote" "$scratch/shallow"
work="$scratch/shallow"
expect_success v0.1.0 "$selected"
[[ $(git -C "$work" rev-parse --is-shallow-repository) == false ]] || fail 'Shallow history was not refreshed'
assert_no_tag v0.1.0
pass 'shallow clone can verify an older default-branch commit'

printf '%s release-helper checks passed. No AWS services were contacted.\n' "$checks"
