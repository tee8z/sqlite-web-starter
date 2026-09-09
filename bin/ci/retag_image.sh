#!/usr/bin/env bash

set -euo pipefail

# The ECR repository must enforce immutable release tags to prevent concurrent
# promotions from overwriting one another between the lookup and put requests.

if [[ $# -ne 3 ]]; then
    echo 'Usage: retag_image.sh ECR_REPOSITORY SOURCE_SHA VERSION' >&2
    exit 1
fi

repository="$1"
source_sha="$2"
version="$3"

if [[ -z "$repository" || ! "$source_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo 'An ECR repository and a full, lowercase commit SHA are required.' >&2
    exit 1
fi

if [[ ! "$version" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
    echo 'The release version must be a stable tag such as v1.2.3.' >&2
    exit 1
fi

# Print one validated image. Return 2 only for an explicit ImageNotFound;
# authorization errors and unexpected responses must never imply a missing tag.
lookup_image() {
    local tag="$1"
    local response state

    if ! response=$(aws ecr batch-get-image \
        --repository-name "$repository" \
        --image-ids "imageTag=$tag" \
        --output json); then
        echo "Failed to read ECR image $repository:$tag." >&2
        return 1
    fi

    if ! state=$(jq -er --arg tag "$tag" '
        def valid_image:
            (.imageId.imageTag == $tag)
            and (.imageId.imageDigest | type == "string" and test("^sha256:[0-9a-f]{64}$"))
            and (.imageManifest | type == "string" and length > 0
                and (try (fromjson | type == "object" and (.schemaVersion | type == "number")) catch false))
            and (.imageManifestMediaType == null
                or (.imageManifestMediaType | type == "string" and length > 0));
        if type != "object" then "invalid"
        elif ((.images // []) | type) != "array"
            or ((.failures // []) | type) != "array" then "invalid"
        elif ((.images // []) | length) == 1
            and ((.failures // []) | length) == 0
            and (.images[0] | valid_image) then "found"
        elif ((.images // []) | length) == 0
            and ((.failures // []) | length) == 1
            and .failures[0].failureCode == "ImageNotFound"
            and .failures[0].imageId.imageTag == $tag then "missing"
        else "invalid"
        end
    ' <<< "$response"); then
        echo "Invalid ECR response for $repository:$tag." >&2
        return 1
    fi

    case "$state" in
        found) jq -c '.images[0]' <<< "$response" ;;
        missing) return 2 ;;
        *)
            echo "ECR returned an unexpected image or failure for $repository:$tag." >&2
            return 1
            ;;
    esac
}

if source_image=$(lookup_image "$source_sha"); then
    source_digest=$(jq -r '.imageId.imageDigest' <<< "$source_image")
else
    status=$?
    if [[ "$status" -eq 2 ]]; then
        echo "Source image $repository:$source_sha is missing. Wait for that commit's CI image build to succeed before releasing." >&2
    fi
    exit 1
fi

if existing_image=$(lookup_image "$version"); then
    existing_digest=$(jq -r '.imageId.imageDigest' <<< "$existing_image")
    if [[ "$existing_digest" == "$source_digest" ]]; then
        echo "$repository:$version already points to $source_digest; nothing to change."
        exit 0
    fi
    echo "Refusing to overwrite $repository:$version: $existing_digest differs from source $source_digest." >&2
    exit 1
else
    status=$?
    if [[ "$status" -ne 2 ]]; then
        exit 1
    fi
fi

manifest=$(jq -r '.imageManifest' <<< "$source_image")
media_type=$(jq -r '.imageManifestMediaType // empty' <<< "$source_image")
put_args=(
    ecr put-image
    --repository-name "$repository"
    --image-tag "$version"
    --image-manifest "$manifest"
    --image-digest "$source_digest"
    --output json
)
if [[ -n "$media_type" ]]; then
    put_args+=(--image-manifest-media-type "$media_type")
fi

if put_response=$(aws "${put_args[@]}"); then
    if ! jq -e --arg digest "$source_digest" --arg version "$version" '
        .image.imageId.imageDigest == $digest and .image.imageId.imageTag == $version
    ' <<< "$put_response" >/dev/null; then
        echo "Invalid ECR response while creating $repository:$version." >&2
        exit 1
    fi
    echo "Promoted $repository:$source_sha to $repository:$version ($source_digest)."
    exit 0
fi

# An immutable repository can reject a simultaneous put of the same version.
# Accept it only if a fresh lookup proves that the intended digest was published.
if existing_image=$(lookup_image "$version"); then
    existing_digest=$(jq -r '.imageId.imageDigest' <<< "$existing_image")
    if [[ "$existing_digest" == "$source_digest" ]]; then
        echo "$repository:$version now points to $source_digest; promotion is complete."
        exit 0
    fi
fi

echo "Failed to promote $repository:$source_sha to $repository:$version." >&2
exit 1
