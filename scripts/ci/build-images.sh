#!/usr/bin/env bash
# images job body: builds api, worker, decoder and the -test variants of api and worker (BuildKit). Nothing is pushed:
# no registry login, no --push; images are only loaded into the local daemon and checked by deploy/tests/check-images.sh.
# usage: build-images.sh                  build all five, then check them (local run)
#        build-images.sh build NAME FILE  build one of api, worker, decoder, api-test, worker-test and `docker save` it to FILE
#        build-images.sh check            check the five images already in the local daemon (CI loads them from the FILE of each build)
# IMAGE_PREFIX (default sunnycloud) and IMAGE_TAG (default ci) name the images: ${IMAGE_PREFIX}-api:${IMAGE_TAG}, ...-api:${IMAGE_TAG}-test
# BUILDX_CACHE_DIR   when set, a per-image local BuildKit cache is read from and written to it (needs a docker-container builder)
# VERSION, REVISION  build args for the OCI labels (defaults 0.0.0-ci and the HEAD commit)
# BUILD_TIMESTAMP    Unix seconds of the decoder build (default now): the build facts of the runner's `hello` and the
#                    yyyymmdd of the source-offer asset name come from it
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
prefix="${IMAGE_PREFIX:-sunnycloud}"
image_tag="${IMAGE_TAG:-ci}"
version="${VERSION:-0.0.0-ci}"
revision="${REVISION:-$(git -C "$repo_root" rev-parse HEAD)}"
build_timestamp="${BUILD_TIMESTAMP:-$(date +%s)}"
build_date="$(date -u -d "@$build_timestamp" +%Y%m%d)"
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

# build_named NAME: builds one image by its short name and sets $ref to its reference
build_named() {
  case "$1" in
    api) build_image api deploy/docker/api.Dockerfile "$api"; ref="$api" ;;
    worker) build_image worker deploy/docker/worker.Dockerfile "$worker"; ref="$worker" ;;
    decoder)
      build_image decoder deploy/docker/decoder.Dockerfile "$decoder" \
        --build-arg "BUILD_TIMESTAMP=$build_timestamp" --build-arg "BUILD_DATE=$build_date"
      ref="$decoder"
      ;;
    api-test) build_image api-test deploy/docker/api.Dockerfile "$api_test" --build-arg FEATURES=test-clock --build-arg TEST_CLOCK=true; ref="$api_test" ;;
    worker-test) build_image worker-test deploy/docker/worker.Dockerfile "$worker_test" --build-arg FEATURES=test-clock --build-arg TEST_CLOCK=true; ref="$worker_test" ;;
    *) echo "$tag FAILED: unknown image '$1' (api, worker, decoder, api-test, worker-test)" >&2; exit 2 ;;
  esac
}

check_all() {
  echo "$tag check users, labels, health checks and layers (deploy/tests/check-images.sh)"
  deploy/tests/check-images.sh "$api" "$worker" "$decoder" "$api_test" "$worker_test"
  echo "$tag OK: 5 images built, none pushed"
}

case "${1:-all}" in
  build)
    [ $# -eq 3 ] || { echo "$tag usage: build-images.sh build NAME FILE" >&2; exit 2; }
    build_named "$2"
    docker save --output "$3" "$ref"
    echo "$tag saved $ref to $3"
    ;;
  check) check_all ;;
  all)
    for name in api worker decoder api-test worker-test; do build_named "$name"; done
    check_all
    ;;
  *) echo "$tag usage: build-images.sh [build NAME FILE | check]" >&2; exit 2 ;;
esac
