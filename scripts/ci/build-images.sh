#!/usr/bin/env bash
# images job body: builds api, worker, decoder and the -test variants of api and worker (BuildKit). Nothing is pushed:
# no registry login, no --push; images are only loaded into the local daemon and checked by deploy/tests/check-images.sh.
# IMAGE_PREFIX (default sunnycloud) and IMAGE_TAG (default ci) name the images: ${IMAGE_PREFIX}-api:${IMAGE_TAG}, ...-api:${IMAGE_TAG}-test
# BUILDX_CACHE_DIR   when set, a per-image local BuildKit cache is read from and written to it (needs a docker-container builder)
# VERSION, REVISION  build args for the OCI labels (defaults 0.0.0-ci and the HEAD commit)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
prefix="${IMAGE_PREFIX:-sunnycloud}"
image_tag="${IMAGE_TAG:-ci}"
version="${VERSION:-0.0.0-ci}"
revision="${REVISION:-$(git -C "$repo_root" rev-parse HEAD)}"
tag="[build-images]"
cd "$repo_root"

if docker buildx version; then
  build=(docker buildx build --load --progress=plain)
else
  echo "$tag buildx is not installed; using plain docker build"
  build=(docker build)
fi

# build NAME DOCKERFILE REF [EXTRA_BUILD_ARG...]
build_image() {
  local name="$1" dockerfile="$2" ref="$3"
  shift 3
  local cache=()
  if [ -n "${BUILDX_CACHE_DIR:-}" ]; then
    mkdir -p "$BUILDX_CACHE_DIR/$name"
    cache=(--cache-from "type=local,src=$BUILDX_CACHE_DIR/$name" --cache-to "type=local,dest=$BUILDX_CACHE_DIR/$name,mode=max")
  fi
  echo "$tag building $ref from $dockerfile"
  "${build[@]}" ${cache[@]+"${cache[@]}"} \
    --file "$dockerfile" --tag "$ref" \
    --build-arg "VERSION=$version" --build-arg "REVISION=$revision" "$@" .
}

api="$prefix-api:$image_tag"
worker="$prefix-worker:$image_tag"
decoder="$prefix-decoder:$image_tag"
api_test="$prefix-api:$image_tag-test"
worker_test="$prefix-worker:$image_tag-test"

echo "$tag step 1/3: release images"
build_image api deploy/docker/api.Dockerfile "$api"
build_image worker deploy/docker/worker.Dockerfile "$worker"
build_image decoder deploy/docker/decoder.Dockerfile "$decoder"

echo "$tag step 2/3: -test images (FEATURES=test-clock TEST_CLOCK=true)"
build_image api-test deploy/docker/api.Dockerfile "$api_test" --build-arg FEATURES=test-clock --build-arg TEST_CLOCK=true
build_image worker-test deploy/docker/worker.Dockerfile "$worker_test" --build-arg FEATURES=test-clock --build-arg TEST_CLOCK=true

echo "$tag step 3/3: check users, labels, health checks and layers (deploy/tests/check-images.sh)"
deploy/tests/check-images.sh "$api" "$worker" "$decoder" "$api_test" "$worker_test"
echo "$tag OK: 5 images built, none pushed"
