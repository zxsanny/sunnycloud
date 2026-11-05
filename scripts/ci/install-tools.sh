#!/usr/bin/env bash
# Installs pinned CI tools into $TOOLS_BIN (default: ${CARGO_HOME:-~/.cargo}/bin, which the cargo cache restores).
# usage: install-tools.sh TOOL...      TOOL: cargo-deny cargo-audit cargo-llvm-cov sqlx-cli gitleaks
# Downloaded archives are verified against a pinned SHA-256 (also published by the upstream release); cargo
# installs use --locked and an exact version. Linux x86_64 only. To bump a tool, change its version and checksum here.
set -euo pipefail

tag="[install-tools]"
bin_dir="${TOOLS_BIN:-${CARGO_HOME:-$HOME/.cargo}/bin}"

CARGO_DENY_VERSION=0.20.2
CARGO_DENY_SHA256=9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
CARGO_LLVM_COV_VERSION=0.9.1
CARGO_LLVM_COV_SHA256=b3f68e625481fed9b16444174f3fa5ebcdbde4a1878803a35eabe2dcefcdc41a
GITLEAKS_VERSION=8.30.1
GITLEAKS_SHA256=551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb
CARGO_AUDIT_VERSION=0.22.2
SQLX_CLI_VERSION=0.9.0

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

# download_install NAME URL SHA256 MEMBER_IN_ARCHIVE
download_install() {
  local name="$1" url="$2" sha256="$3" member="$4" work
  work="$(mktemp -d)"
  echo "$tag downloading $name from $url"
  curl --fail --show-error --location --retry 3 --no-progress-meter --output "$work/archive.tar.gz" "$url"
  echo "$sha256  $work/archive.tar.gz" | sha256sum --check - || fail "$name archive does not match the pinned SHA-256 $sha256"
  mkdir "$work/x"
  tar -xzf "$work/archive.tar.gz" -C "$work/x"
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
    gitleaks)
      have="$(installed_version gitleaks version)"
      if [ "$have" = "$GITLEAKS_VERSION" ]; then echo "$tag $tool $GITLEAKS_VERSION already installed"; return; fi
      download_install gitleaks "https://github.com/gitleaks/gitleaks/releases/download/v$GITLEAKS_VERSION/gitleaks_${GITLEAKS_VERSION}_linux_x64.tar.gz" "$GITLEAKS_SHA256" gitleaks
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
    *) fail "unknown tool '$tool' (cargo-deny cargo-audit cargo-llvm-cov sqlx-cli gitleaks)" ;;
  esac
  echo "$tag $tool installed"
}

[ $# -gt 0 ] || { echo "usage: install-tools.sh TOOL..." >&2; exit 2; }
[ "$(uname -s)-$(uname -m)" = "Linux-x86_64" ] || fail "only Linux x86_64 is supported, found $(uname -s)-$(uname -m)"
for tool in "$@"; do
  install_tool "$tool"
done
echo "$tag OK: $* in $bin_dir"
