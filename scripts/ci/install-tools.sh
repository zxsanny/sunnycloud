#!/usr/bin/env bash
# Installs pinned CI tools into $TOOLS_BIN (default: ${CARGO_HOME:-~/.cargo}/bin, which the cargo cache restores).
# usage: install-tools.sh TOOL...      TOOL: cargo-deny cargo-audit cargo-llvm-cov cargo-nextest sqlx-cli gitleaks bun wasm-pack pg-client-18 keyring-test-deps media-tools
# Downloaded archives are verified against a pinned SHA-256 (also published by the upstream release); cargo
# installs use --locked and an exact version. Linux x86_64 only. To bump a tool, change its version and checksum here.
# bun is the JavaScript runtime and package manager of the web app (src/web), from the release zip. wasm-pack builds
# sync-core-wasm for the browser (src/web/scripts/build-wasm.sh); the prebuilt musl release replaces a cargo install.
# pg-client-18 is the PostgreSQL 18 client (pg_dump, pg_restore, psql) from the PGDG apt repository at an exact
# package version, installed with apt (sudo when not root) into /usr/lib/postgresql/18/bin; the repository key is
# accepted only with the pinned fingerprint, and apt verifies the packages against the signed index. Ubuntu only.
# keyring-test-deps is gnome-keyring, secret-tool and dbus-run-session from the Ubuntu archive: the desktop daemon's
# token_keyring tests start a real keyring on a private session bus. Ubuntu only.
# media-tools is vipsthumbnail, vipsheader (libvips-tools) and ffmpeg, ffprobe from the Ubuntu archive: the decoder
# runner starts only when these four are on PATH, and its tests render real images and clips with them. Ubuntu only.
set -euo pipefail

tag="[install-tools]"
bin_dir="${TOOLS_BIN:-${CARGO_HOME:-$HOME/.cargo}/bin}"

CARGO_DENY_VERSION=0.20.2
CARGO_DENY_SHA256=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
CARGO_LLVM_COV_VERSION=0.9.1
CARGO_LLVM_COV_SHA256=b3f68e625481fed9b16444174f3fa5ebcdbde4a1878803a35eabe2dcefcdc41a
# cargo-nextest runs the tests of all test binaries in one pool (cargo test runs the binaries one after another).
# The upstream release publishes no checksum file; this one was computed from the downloaded archive.
CARGO_NEXTEST_VERSION=0.9.152
CARGO_NEXTEST_SHA256=d039b43c3e0958441b6ad0a39c8884d018b3d2e8fb9928f209074b1100b5e17a
GITLEAKS_VERSION=8.30.1
GITLEAKS_SHA256=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
# bun: SHA-256 of bun-linux-x64.zip, equal to the entry in SHASUMS256.txt of the release.
BUN_VERSION=1.4.5
BUN_SHA256=94559b3b528995a49452f9f2404cec74f2efd7d06e0a000a8cd214b02c3666eb
# wasm-pack: the release publishes the digest in its asset list; it was also recomputed from the downloaded archive.
WASM_PACK_VERSION=0.15.1
WASM_PACK_SHA256=410101009739b9d01c7d5affaaad6e9c9a24b4cba6791a4805db8e89b9bf7af2
CARGO_AUDIT_VERSION=0.22.2
SQLX_CLI_VERSION=0.9.0
# The exact package version is a supply-chain pin. PGDG's apt.postgresql.org index keeps only the newest few
# versions of a package (18.7, 18.6 and 18.4 on 2026-11-18) and moves the rest to apt-archive.postgresql.org, so
# this value must be bumped when a newer 18.x release has pushed it out of the index (the job then fails with
# "version not found"): read the current one from
# https://apt.postgresql.org/pub/repos/apt/dists/noble-pgdg/main/binary-amd64/Packages.gz and update it here.
PG_CLIENT_VERSION=18.7-1.pgdg24.04+2
PGDG_KEY_URL=https://www.postgresql.org/media/keys/ACCC4CF8.asc
PGDG_KEY_FINGERPRINT=B97B0AFCAA1A47F044F244A07FCC7D46ACCC4CF8
PG_CLIENT_BIN=/usr/lib/postgresql/18/bin

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# download_install NAME URL SHA256 MEMBER_IN_ARCHIVE   (a .zip URL is unzipped, anything else is a .tar.gz)
download_install() {
  local name="$1" url="$2" sha256="$3" member="$4" work
  work="$(mktemp -d)"
  echo "$tag downloading $name from $url"
  curl --fail --show-error --location --retry 6 --retry-delay 10 --no-progress-meter --output "$work/archive" "$url"
  echo "$sha256  $work/archive" | sha256sum --check - || fail "$name archive does not match the pinned SHA-256 $sha256"
  mkdir "$work/x"
  case "$url" in
    *.zip) unzip -o -d "$work/x" "$work/archive" ;;
    *) tar -xzf "$work/archive" -C "$work/x" ;;
  esac
  mkdir -p "$bin_dir"
  install -m 0755 "$work/x/$member" "$bin_dir/$name"
  rm -rf "$work"
}

# installed_version EXECUTABLE ARGS...  -> version output of the executable in $bin_dir, empty when it is absent
installed_version() {
  local exe="$bin_dir/$1"
  shift
  if [ -x "$exe" ]; then "$exe" "$@"; fi
}

# as_root CMD...  -> runs CMD as root, through sudo when this shell is not root
as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi
}

# The PGDG source with the fingerprint-checked key, then the exact client and libpq versions.
install_pg_client() {
  local work codename release keyring=/usr/share/keyrings/pgdg-archive.gpg fingerprints
  codename="$(sed -n 's/^VERSION_CODENAME=//p' /etc/os-release)"
  release="$(sed -n 's/^VERSION_ID=//p' /etc/os-release | tr -d '"')"
  [ -n "$codename" ] && [ -n "$release" ] || fail "pg-client-18: no VERSION_CODENAME or VERSION_ID in /etc/os-release"
  case "$PG_CLIENT_VERSION" in
    *"pgdg$release"*) ;;
    *) fail "pg-client-18: $PG_CLIENT_VERSION is pinned for another release than $codename" ;;
  esac
  work="$(mktemp -d)"
  curl --fail --show-error --location --retry 3 --no-progress-meter --output "$work/key.asc" "$PGDG_KEY_URL"
  fingerprints="$(gpg --show-keys --with-colons "$work/key.asc" | awk -F: '$1 == "fpr" { print $10 }')"
  [ "$fingerprints" = "$PGDG_KEY_FINGERPRINT" ] || fail "PGDG key fingerprint '$fingerprints' is not the pinned $PGDG_KEY_FINGERPRINT"
  gpg --dearmor --output "$work/pgdg.gpg" "$work/key.asc"
  as_root install -m 0644 "$work/pgdg.gpg" "$keyring"
  echo "deb [signed-by=$keyring] https://apt.postgresql.org/pub/repos/apt $codename-pgdg main" > "$work/pgdg.list"
  as_root install -m 0644 "$work/pgdg.list" /etc/apt/sources.list.d/pgdg.list
  rm -rf "$work"
  # Only the PGDG index: a full `apt-get update` also refreshes the runner's Ubuntu mirror, which hung silently past
  # the watchdog's 120 s twice (CI runs 45165398422 and 45169525185). The client's other dependencies (libssl, zlib,
  # readline, lz4, zstd) are part of the runner image already.
  local apt_net=(-o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
  as_root apt-get "${apt_net[@]}" update -o Dir::Etc::sourcelist=/etc/apt/sources.list.d/pgdg.list \
    -o Dir::Etc::sourceparts=- -o APT::Get::List-Cleanup=0
  as_root env DEBIAN_FRONTEND=noninteractive apt-get "${apt_net[@]}" install --yes --no-install-recommends \
    "postgresql-client-18=$PG_CLIENT_VERSION" "libpq5=$PG_CLIENT_VERSION"
}

install_tool() {
  local tool="$1" have
  case "$tool" in
    cargo-deny)
      have="$(installed_version cargo-deny --version)"
      if [ "$have" = "cargo-deny $CARGO_DENY_VERSION" ]; then echo "$tag $tool $CARGO_DENY_VERSION already installed"; return; fi
      local dir="cargo-deny-$CARGO_DENY_VERSION-x86_64-unknown-linux-musl"
      download_install cargo-deny "https://github.com/EmbarkStudios/cargo-deny/releases/download/$CARGO_DENY_VERSION/$dir.tar.gz" "$CARGO_DENY_SHA256" "$dir/cargo-deny"
      ;;
    cargo-llvm-cov)
      have="$(installed_version cargo-llvm-cov llvm-cov --version)"
      if [ "$have" = "cargo-llvm-cov $CARGO_LLVM_COV_VERSION" ]; then echo "$tag $tool $CARGO_LLVM_COV_VERSION already installed"; return; fi
      download_install cargo-llvm-cov "https://github.com/taiki-e/cargo-llvm-cov/releases/download/v$CARGO_LLVM_COV_VERSION/cargo-llvm-cov-x86_64-unknown-linux-gnu.tar.gz" "$CARGO_LLVM_COV_SHA256" cargo-llvm-cov
      ;;
    cargo-nextest)
      have="$(installed_version cargo-nextest nextest --version)"
      if [[ "$have" == "cargo-nextest $CARGO_NEXTEST_VERSION "* ]]; then echo "$tag $tool $CARGO_NEXTEST_VERSION already installed"; return; fi
      download_install cargo-nextest "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$CARGO_NEXTEST_VERSION/cargo-nextest-$CARGO_NEXTEST_VERSION-x86_64-unknown-linux-gnu.tar.gz" "$CARGO_NEXTEST_SHA256" cargo-nextest
      ;;
    gitleaks)
      have="$(installed_version gitleaks version)"
      if [ "$have" = "$GITLEAKS_VERSION" ]; then echo "$tag $tool $GITLEAKS_VERSION already installed"; return; fi
      download_install gitleaks "https://github.com/gitleaks/gitleaks/releases/download/v$GITLEAKS_VERSION/gitleaks_${GITLEAKS_VERSION}_linux_x64.tar.gz" "$GITLEAKS_SHA256" gitleaks
      ;;
    bun)
      have="$(installed_version bun --version)"
      if [ "$have" = "$BUN_VERSION" ]; then echo "$tag $tool $BUN_VERSION already installed"; return; fi
      download_install bun "https://github.com/oven-sh/bun/releases/download/bun-v$BUN_VERSION/bun-linux-x64.zip" "$BUN_SHA256" bun-linux-x64/bun
      ;;
    wasm-pack)
      have="$(installed_version wasm-pack --version)"
      if [ "$have" = "wasm-pack $WASM_PACK_VERSION" ]; then echo "$tag $tool $WASM_PACK_VERSION already installed"; return; fi
      local wp_dir="wasm-pack-v$WASM_PACK_VERSION-x86_64-unknown-linux-musl"
      download_install wasm-pack "https://github.com/wasm-bindgen/wasm-pack/releases/download/v$WASM_PACK_VERSION/$wp_dir.tar.gz" "$WASM_PACK_SHA256" "$wp_dir/wasm-pack"
      ;;
    cargo-audit)
      have="$(installed_version cargo-audit audit --version)"
      if [ "$have" = "cargo-audit-audit $CARGO_AUDIT_VERSION" ]; then echo "$tag $tool $CARGO_AUDIT_VERSION already installed"; return; fi
      cargo install cargo-audit --locked --version "$CARGO_AUDIT_VERSION" --root "$(dirname "$bin_dir")"
      ;;
    sqlx-cli)
      have="$(installed_version sqlx --version)"
      if [ "$have" = "sqlx-cli $SQLX_CLI_VERSION" ]; then echo "$tag $tool $SQLX_CLI_VERSION already installed"; return; fi
      cargo install sqlx-cli --locked --version "$SQLX_CLI_VERSION" --no-default-features --features rustls,postgres --root "$(dirname "$bin_dir")"
      ;;
    pg-client-18)
      have="$(dpkg-query --show --showformat='${Version}' postgresql-client-18 || true)"
      if [ "$have" = "$PG_CLIENT_VERSION" ]; then echo "$tag $tool $PG_CLIENT_VERSION already installed"; return; fi
      install_pg_client
      "$PG_CLIENT_BIN/pg_dump" --version
      echo "$tag $tool $PG_CLIENT_VERSION in $PG_CLIENT_BIN (SC_PG_TOOLS)"
      return
      ;;
    keyring-test-deps)
      if command -v gnome-keyring-daemon >/dev/null && command -v secret-tool >/dev/null && command -v dbus-run-session >/dev/null; then
        echo "$tag $tool already installed"
        return
      fi
      as_root apt-get update
      as_root env DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends \
        gnome-keyring libsecret-tools dbus
      ;;
    media-tools)
      if command -v vipsthumbnail >/dev/null && command -v vipsheader >/dev/null && command -v ffmpeg >/dev/null && command -v ffprobe >/dev/null; then
        echo "$tag $tool already installed"
        return
      fi
      as_root apt-get update
      as_root env DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends \
        libvips-tools ffmpeg libheif-plugin-libde265 libheif-plugin-x265
      vipsthumbnail --version
      ffmpeg -version | head -1
      ;;
    *) fail "unknown tool '$tool' (cargo-deny cargo-audit cargo-llvm-cov cargo-nextest sqlx-cli gitleaks bun wasm-pack pg-client-18 keyring-test-deps media-tools)" ;;
  esac
  echo "$tag $tool installed"
}

[ $# -gt 0 ] || { echo "usage: install-tools.sh TOOL..." >&2; exit 2; }
[ "$(uname -s)-$(uname -m)" = "Linux-x86_64" ] || fail "only Linux x86_64 is supported, found $(uname -s)-$(uname -m)"
for tool in "$@"; do
  install_tool "$tool"
done
echo "$tag OK: $* in $bin_dir"
