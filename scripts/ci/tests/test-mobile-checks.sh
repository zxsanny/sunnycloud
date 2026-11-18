#!/usr/bin/env bash
# Seeded failures of the mobile gates (AZ-4002 unit tests): each check must pass on the real input and fail on a
# seeded one. Needs the NDK, cargo and the built jniLibs of scripts/ci/mobile-build-native.sh; seeds only in temporary
# directories (a scratch copy of src/ for the dependency seed), never in the working tree.
# usage: test-mobile-checks.sh
set -euo pipefail

tag="[test-mobile-checks]"
tests_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ci_dir="$(cd "$tests_dir/.." && pwd)"
source "$ci_dir/mobile-common.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# expect_fail NAME REASON CMD... -> the command must exit non-zero and its output must hold REASON, so a gate that
# breaks for another cause (a failed build, a missing tool) does not count as a seeded failure caught
expect_fail() {
  local name="$1" reason="$2" output
  shift 2
  if output="$("$@" 2>&1)"; then
    echo "$output"
    fail "$name: the check passed on the seeded input"
  fi
  echo "$output" | tail -4
  echo "$output" | grep -F -- "$reason" >/dev/null || fail "$name: the check failed, but not for the seeded reason ($reason)"
  echo "$tag $name: failed as expected ($reason)"
}

echo "$tag === AC-2 alignment: real libraries pass, a 4 KB library fails"
"$ci_dir/mobile-check-alignment.sh" "$jni_libs_dir"
bin="$(ndk_bin)"
mkdir -p "$work/align"
printf 'int seeded(void) { return 4; }\n' > "$work/align/seed.c"
for page in 16384 4096; do
  "$bin/armv7a-linux-androideabi${ANDROID_API}-clang" -shared -fPIC -Wl,-z,max-page-size=$page \
    -o "$work/align/lib-$page.so" "$work/align/seed.c"
done
"$ci_dir/mobile-check-alignment.sh" "$work/align/lib-16384.so"
expect_fail "4 KB library" "LOAD align: 0x1000" "$ci_dir/mobile-check-alignment.sh" "$work/align/lib-4096.so"
expect_fail "a directory holding a 4 KB library" "1 of 4 libraries" "$ci_dir/mobile-check-alignment.sh" "$jni_libs_dir" "$work/align/lib-4096.so"
mkdir "$work/empty"
expect_fail "no library at all" "no .so file found" "$ci_dir/mobile-check-alignment.sh" "$work/empty"

echo "$tag === AC-3 bindings drift: committed bindings pass, a changed BackupCore method fails"
"$ci_dir/mobile-check-bindings.sh"
cp -r "$bindings_dir" "$work/bindings"
before="$(grep -c 'fun `status`' "$work/bindings/mobile_backup/mobile_backup.kt")"
[ "$before" -ge 1 ] || fail "the committed bindings have no BackupCore status method to seed"
sed -i 's/fun `status`/fun `statusSeeded`/' "$work/bindings/mobile_backup/mobile_backup.kt"
BINDINGS_DIR="$work/bindings" expect_fail "renamed BackupCore.status" "statusSeeded" "$ci_dir/mobile-check-bindings.sh"
rm -rf "$work/bindings"
cp -r "$bindings_dir" "$work/bindings"
rm "$work/bindings/sync_core_proto/sync_core_proto.kt"
BINDINGS_DIR="$work/bindings" expect_fail "missing generated file" "Only in" "$ci_dir/mobile-check-bindings.sh"

echo "$tag === AC-4 no vault code: the real graph passes, a seeded vault-core dependency fails"
"$ci_dir/mobile-check-no-vault.sh"
mkdir "$work/src"
(cd "$repo_root" && git ls-files -z --cached --others --exclude-standard src | grep -z -v '^src/mobile/' | xargs -0 cp --parents -t "$work")
sed -i 's|^uuid = { workspace = true }|&\nvault-core = { path = "../vault-core" }|' "$work/src/mobile_backup/Cargo.toml"
grep -F 'vault-core = { path' "$work/src/mobile_backup/Cargo.toml" >/dev/null || fail "the seeded dependency was not added"
(cd "$work/src" && cargo update --offline --workspace)
SRC_DIR="$work/src" expect_fail "mobile_backup depending on vault-core" "links vault-core" "$ci_dir/mobile-check-no-vault.sh"

echo "$tag OK: every gate passed its real input and failed its seeded one"
