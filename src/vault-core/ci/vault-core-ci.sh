#!/usr/bin/env bash
# v1.2 vault-core matrix legs (AZ-4007 AC-2, AC-3, AC-5). Run by the `vault-core` job of .github/workflows/ci.yml;
# the same commands run on a laptop. No v1.0 or v1.1 release build calls this script (ADR-016, FT-P-73).
# usage: vault-core-ci.sh LEG     LEG: native | wasm | uniffi | swift | licences
#   native    clippy and verbose tests (default features, then every native feature); crate-graph and
#             randomness checks are tests of the crate (tests/crate_graph.rs, tests/randomness.rs)
#   wasm      wasm32-unknown-unknown build (no getrandom: tests/crate_graph.rs) and wasm-pack test --node
#   uniffi    Kotlin bindings from the cdylib, compiled and run on a JVM against the native library
#   swift     Swift bindings smoke on macOS; elsewhere prints NOT-RUN(H-MAC), never PASS
#   licences  cargo deny licence allowlist for the iOS build graph (aarch64-apple-ios, mobile features)
# Downloads are verified against pinned SHA-256 values (published upstream for kotlin-compiler, JNA (SHA-1
# cross-checked) and wasm-bindgen; wasm-pack publishes none, so its value was recorded at pin time).
# VAULT_TOOLS_DIR   download and install directory (default: src/target/vault-ci-tools)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
src_dir="$repo_root/src"
crate_dir="$src_dir/vault-core"
tools_dir="${VAULT_TOOLS_DIR:-$src_dir/target/vault-ci-tools}"
native_features="passphrase-policy,os-rng,uniffi"

WASM_PACK_VERSION=0.15.1
WASM_BINDGEN_VERSION=0.2.130
# Per host: release target triple and the SHA-256 of its wasm-pack and wasm-bindgen archives. wasm-bindgen's
# values are its published .sha256sum files; wasm-pack publishes none, so those were recorded at pin time.
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    host_triple=x86_64-unknown-linux-musl
    WASM_PACK_SHA256=410101009739b9d01c7d5affaaad6e9c9a24b4cba6791a4805db8e89b9bf7af2
    WASM_BINDGEN_SHA256=9687f3bb551b9f66f37ed32a40d7913a00f7962db653054e56857bc306c3227f ;;
  Darwin-arm64)
    host_triple=aarch64-apple-darwin
    WASM_PACK_SHA256=bcaf2a418ae39c874b072ea122fec58e11970862cbd8fd4eca2f00a17f76c536
    WASM_BINDGEN_SHA256=b90ddb9357f584b1af897d58ad77e4cce2b48fc17e2a967bc249a87f72809444 ;;
  Darwin-x86_64)
    host_triple=x86_64-apple-darwin
    WASM_PACK_SHA256=9387715ad345a1fe58751d60c80f49d9139957540728bafd0668d005a37cef62
    WASM_BINDGEN_SHA256=eb209c115b7cb95832423c1aa3f4d06d6c53f63ef74fc4b6fb79ef0b2eea5f10 ;;
  *)
    host_triple=
    WASM_PACK_SHA256=
    WASM_BINDGEN_SHA256= ;;
esac
if [ "$(uname -s)" = "Darwin" ]; then lib_ext=dylib; else lib_ext=so; fi
KOTLIN_VERSION=2.4.21
KOTLIN_SHA256=7cc140e76daf416a0424a5557f99afd7ca89ebd463ea1101261e3e01c33e75cd
JNA_VERSION=5.19.1
JNA_SHA256=4fb141dd8ef6b0585ffceea4bc49602fbc6312fa977e2c488794ea3e6aafecae

leg="${1:-}"
tag="[vault-core-ci:${leg:-?}]"
fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# fetch URL SHA256 FILE -> downloads once into $tools_dir and verifies the checksum on every run
fetch() {
  local url="$1" sha256="$2" file="$tools_dir/$3"
  mkdir -p "$tools_dir"
  if [ ! -f "$file" ]; then
    echo "$tag downloading $url"
    curl --fail --show-error --location --retry 6 --retry-delay 10 --no-progress-meter --output "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  sha256_matches "$sha256" "$file" || { rm -f "$file"; fail "$file does not match the pinned SHA-256 $sha256"; }
}

# sha256_matches SHA256 FILE -> Linux has sha256sum, macOS has shasum
sha256_matches() {
  if command -v sha256sum >/dev/null; then
    echo "$1  $2" | sha256sum --check -
  else
    echo "$1  $2" | shasum -a 256 --check -
  fi
}

# untar ARCHIVE DIR BINARY -> extracts the verified archive into $tools_dir/DIR unless DIR/BINARY is already
# there. The binary is the test, not the directory: a cache restore can leave DIR empty.
untar() {
  if [ ! -x "$tools_dir/$2/$3" ]; then
    rm -rf "${tools_dir:?}/$2"
    tar -xzf "$tools_dir/$1" -C "$tools_dir"
  fi
  [ -x "$tools_dir/$2/$3" ] || fail "$1 did not provide $2/$3"
}

build_cdylib() {
  echo "$tag cargo rustc -p vault-core --lib --features uniffi --crate-type cdylib --locked"
  cargo rustc -p vault-core --lib --locked --features uniffi --crate-type cdylib
}

# bindings LANGUAGE LIBRARY OUT_DIR
bindings() {
  echo "$tag uniffi-bindgen generate --language $1"
  rm -rf "$3"
  cargo run --locked -p vault-core --features uniffi-cli --bin uniffi-bindgen -- \
    generate --library "$2" --language "$1" --out-dir "$3" --no-format
}

leg_native() {
  echo "$tag step 1/3: cargo clippy (every native feature) -D warnings"
  cargo clippy -p vault-core --all-targets --locked --features "$native_features" -- -D warnings
  echo "$tag step 2/3: cargo test -v (default features: core tree, crate-graph, randomness, contract checks)"
  cargo test -v -p vault-core --locked --no-fail-fast
  echo "$tag step 3/3: cargo test -v (features $native_features)"
  cargo test -v -p vault-core --locked --no-fail-fast --features "$native_features"
}

leg_wasm() {
  echo "$tag step 1/3: cargo build --target wasm32-unknown-unknown (default, wasm, then wasm,passphrase-policy)"
  cargo build -v -p vault-core --locked --target wasm32-unknown-unknown
  cargo build -v -p vault-core --locked --target wasm32-unknown-unknown --features wasm
  cargo build -v -p vault-core --locked --target wasm32-unknown-unknown --features wasm,passphrase-policy
  echo "$tag step 2/3: pinned wasm-pack $WASM_PACK_VERSION and wasm-bindgen $WASM_BINDGEN_VERSION"
  [ -n "$host_triple" ] || fail "no pinned wasm-pack and wasm-bindgen for $(uname -s) $(uname -m)"
  local pack="wasm-pack-v$WASM_PACK_VERSION-$host_triple"
  local bindgen="wasm-bindgen-$WASM_BINDGEN_VERSION-$host_triple"
  fetch "https://github.com/wasm-bindgen/wasm-pack/releases/download/v$WASM_PACK_VERSION/$pack.tar.gz" "$WASM_PACK_SHA256" "$pack.tar.gz"
  fetch "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$WASM_BINDGEN_VERSION/$bindgen.tar.gz" "$WASM_BINDGEN_SHA256" "$bindgen.tar.gz"
  untar "$pack.tar.gz" "$pack" wasm-pack
  untar "$bindgen.tar.gz" "$bindgen" wasm-bindgen
  export PATH="$tools_dir/$bindgen:$tools_dir/$pack:$PATH"
  wasm-pack --version
  wasm-bindgen --version
  echo "$tag step 3/3: wasm-pack test --node (feature wasm)"
  (cd "$crate_dir" && wasm-pack test --node -- --locked --features wasm)
}

leg_uniffi() {
  echo "$tag step 1/4: cdylib with the uniffi feature"
  build_cdylib
  local lib="$src_dir/target/debug/libvault_core.$lib_ext" out="$src_dir/target/vault-uniffi/kotlin"
  echo "$tag step 2/4: Kotlin bindings"
  bindings kotlin "$lib" "$out"
  echo "$tag step 3/4: pinned Kotlin $KOTLIN_VERSION compiler and JNA $JNA_VERSION"
  fetch "https://github.com/JetBrains/kotlin/releases/download/v$KOTLIN_VERSION/kotlin-compiler-$KOTLIN_VERSION.zip" "$KOTLIN_SHA256" "kotlin-compiler-$KOTLIN_VERSION.zip"
  fetch "https://repo1.maven.org/maven2/net/java/dev/jna/jna/$JNA_VERSION/jna-$JNA_VERSION.jar" "$JNA_SHA256" "jna-$JNA_VERSION.jar"
  local kotlin_home="$tools_dir/kotlin-$KOTLIN_VERSION" jna="$tools_dir/jna-$JNA_VERSION.jar"
  # The compiler is the test, not the directory: a cache restore can leave kotlin_home empty.
  if [ ! -x "$kotlin_home/kotlinc/bin/kotlinc" ]; then
    rm -rf "$kotlin_home"
    mkdir -p "$kotlin_home"
    (cd "$kotlin_home" && unzip -q "$tools_dir/kotlin-compiler-$KOTLIN_VERSION.zip")
  fi
  [ -x "$kotlin_home/kotlinc/bin/kotlinc" ] || fail "the Kotlin compiler archive did not provide kotlinc/bin/kotlinc"
  java -version
  echo "$tag step 4/4: compile the bindings with the smoke test and run it on the JVM"
  "$kotlin_home/kotlinc/bin/kotlinc" -include-runtime -cp "$jna" -d "$out/smoke.jar" \
    "$out/uniffi/vault_core/vault_core.kt" "$crate_dir/tests/uniffi/kotlin/Smoke.kt"
  java -Djna.library.path="$(dirname "$lib")" -cp "$out/smoke.jar:$jna" uniffi.vault_core.SmokeKt
}

leg_swift() {
  if [ "$(uname -s)" != "Darwin" ] || ! command -v swiftc >/dev/null; then
    echo "$tag NOT-RUN(H-MAC): the Swift leg needs the owner Mac (macOS with swiftc); it blocks only the v1.2 release gate"
    return 0
  fi
  echo "$tag step 1/3: cdylib with the uniffi feature"
  build_cdylib
  local lib="$src_dir/target/debug/libvault_core.dylib" out="$src_dir/target/vault-uniffi/swift"
  echo "$tag step 2/3: Swift bindings"
  bindings swift "$lib" "$out"
  echo "$tag step 3/3: compile the bindings with the smoke test and run it"
  swiftc -module-name vault_core -I "$out" -Xcc -fmodule-map-file="$out/vault_coreFFI.modulemap" \
    -L "$(dirname "$lib")" -lvault_core -o "$out/smoke" "$out/vault_core.swift" "$crate_dir/tests/uniffi/swift/main.swift"
  DYLD_LIBRARY_PATH="$(dirname "$lib")" "$out/smoke"
}

leg_licences() {
  echo "$tag cargo deny check licenses: vault-core as sole root, aarch64-apple-ios, features $native_features, no dev-dependencies"
  cargo deny --manifest-path "$crate_dir/Cargo.toml" --config "$src_dir/deny.toml" \
    --target aarch64-apple-ios --features "$native_features" --exclude-dev check licenses
}

cd "$src_dir"
case "$leg" in
  native) leg_native ;;
  wasm) leg_wasm ;;
  uniffi) leg_uniffi ;;
  swift) leg_swift ;;
  licences) leg_licences ;;
  *) echo "usage: vault-core-ci.sh native|wasm|uniffi|swift|licences" >&2; exit 2 ;;
esac
echo "$tag OK"
