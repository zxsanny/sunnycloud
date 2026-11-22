#!/usr/bin/env bash
# Container-level decoder suite (H3, AZ-3865): builds the `test-stubs` variant on a decoder image that already
# exists, then runs the cases of src/decoder/tests/container with `--ignored` against both images. Nothing is pushed.
# The variant adds only a Rust build of the runner and the stub plus one thin layer: no second media build.
# usage: decoder-suite.sh RELEASE_IMAGE [NEXTEST_ARG...]    e.g. decoder-suite.sh sunnycloud-decoder:ci
# DECODER_TEST_IMAGE   tag of the variant (default RELEASE_IMAGE-test, like the -test images of build-images.sh)
# DECODER_SUITE_THREADS  cases run in parallel (default 2; each starts its own container)
# Needs docker, cargo-nextest and the host reference tools (scripts/ci/install-tools.sh media-tools).
# Every container the suite starts carries the label sunnycloud-suite=h3 and is removed at the end, also on failure.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tag="[decoder-suite]"
image="${1:?usage: decoder-suite.sh RELEASE_IMAGE [NEXTEST_ARG...]}"
shift
test_image="${DECODER_TEST_IMAGE:-$image-test}"
threads="${DECODER_SUITE_THREADS:-2}"
cd "$repo_root"

remove_suite_containers() {
  local leftover
  leftover="$(docker ps --all --format '{{.ID}}' --filter label=sunnycloud-suite=h3)"
  if [ -n "$leftover" ]; then
    echo "$tag removing containers of the suite: $leftover"
    # shellcheck disable=SC2086 # ids are hex words
    docker rm --force --volumes $leftover >/dev/null
  fi
}
trap remove_suite_containers EXIT

echo "$tag building $test_image on $image"
docker build --file deploy/docker/decoder.Dockerfile --target test-stubs \
  --build-arg FEATURES=test-freeze --build-arg "TEST_STUBS_BASE=$image" --tag "$test_image" .

cd src
export DECODER_IMAGE="$image" DECODER_TEST_IMAGE="$test_image"
echo "$tag running the cases against $image and $test_image"
cargo nextest run --locked -p decoder-runner --run-ignored only -E 'binary(container)' \
  --no-fail-fast --test-threads "$threads" --success-output immediate --failure-output immediate "$@"
echo "$tag OK: every case passed (values recorded in src/target/tmp/decoder-suite-records.txt)"
