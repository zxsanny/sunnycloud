#!/usr/bin/env bash
# AC-3 for audit.sh on fixture deny configurations, with a fixed AUDIT_TODAY and DENY_TOML:
#   no ignores -> pass; missing fields -> fail; expired -> fail; added 91 days ago -> fail; added 89 days ago -> pass
# plus: the cargo-audit --ignore list is derived from the same entries (a stub `cargo` records its arguments).
set -euo pipefail

ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tag="[test-audit]"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
today="2026-11-05"

fail() { echo "$tag FAILED: $*" >&2; exit 1; }

days_ago() { date -u -d "$today - $1 days" +%F; }
days_ahead() { date -u -d "$today + $1 days" +%F; }

write_deny() { # file, ignore-array body
  cat >"$1" <<TOML
[advisories]
yanked = "deny"
ignore = [
$2
]
TOML
}

# run_case NAME EXPECT(pass|fail) TOML_BODY
run_case() {
  local name="$1" expect="$2" file="$work/$1.toml" rc
  write_deny "$file" "$3"
  set +e
  DENY_TOML="$file" AUDIT_TODAY="$today" AUDIT_VALIDATE_ONLY=1 "$ci_dir/audit.sh" >"$work/$name.out" 2>&1
  rc=$?
  set -e
  cat "$work/$name.out"
  if [ "$expect" = pass ] && [ "$rc" -ne 0 ]; then fail "$name: expected pass, exit $rc"; fi
  if [ "$expect" = fail ] && [ "$rc" -eq 0 ]; then fail "$name: expected fail, exit 0"; fi
  echo "$tag $name: $expect as expected (exit $rc)"
}

run_case empty pass ''
run_case missing-fields fail '  { id = "RUSTSEC-2099-0001", reason = "no dates and no why" },'
run_case plain-string fail '  "RUSTSEC-2099-0002",'
run_case missing-why fail "  { id = \"RUSTSEC-2099-0003\", reason = \"added=$(days_ago 1); expires=$(days_ahead 30); why=\" },"
run_case expired fail "  { id = \"RUSTSEC-2099-0004\", reason = \"added=$(days_ago 40); expires=$(days_ago 1); why=waiting for upstream\" },"
run_case age-91-days fail "  { id = \"RUSTSEC-2099-0005\", reason = \"added=$(days_ago 91); expires=$(days_ahead 30); why=waiting for upstream\" },"
run_case age-89-days pass "  { id = \"RUSTSEC-2099-0006\", reason = \"added=$(days_ago 89); expires=$(days_ahead 30); why=waiting for upstream\" },"
run_case age-90-days pass "  { id = \"RUSTSEC-2099-0007\", reason = \"added=$(days_ago 90); expires=$(days_ahead 30); why=waiting for upstream\" },"
run_case one-bad-of-two fail "  { id = \"RUSTSEC-2099-0008\", reason = \"added=$(days_ago 5); expires=$(days_ahead 30); why=ok\" },
  { id = \"RUSTSEC-2099-0009\", reason = \"added=$(days_ago 91); expires=$(days_ahead 30); why=old\" },"

echo "$tag derived cargo-audit ignore list comes from the same entries"
mkdir -p "$work/bin"
cat >"$work/bin/cargo" <<SH
#!/usr/bin/env bash
printf '%s\n' "\$*" >>"$work/cargo-calls.txt"
SH
chmod +x "$work/bin/cargo"
write_deny "$work/two.toml" "  { id = \"RUSTSEC-2099-0010\", reason = \"added=$(days_ago 1); expires=$(days_ahead 30); why=a\" },
  { id = \"RUSTSEC-2099-0011\", reason = \"added=$(days_ago 2); expires=$(days_ahead 30); why=b\" },"
PATH="$work/bin:$PATH" DENY_TOML="$work/two.toml" AUDIT_TODAY="$today" "$ci_dir/audit.sh"
cat "$work/cargo-calls.txt"
grep -F -- "audit --file" "$work/cargo-calls.txt" | grep -F -- "--deny warnings --ignore RUSTSEC-2099-0010 --ignore RUSTSEC-2099-0011" >/dev/null \
  || fail "cargo audit did not receive the derived --ignore list"
grep -F -- "deny --manifest-path" "$work/cargo-calls.txt" | grep -F -- "--config $work/two.toml check" >/dev/null \
  || fail "cargo deny did not receive the DENY_TOML configuration"

echo "$tag an invalid configuration never reaches the cargo tools"
rm -f "$work/cargo-calls.txt"
set +e
PATH="$work/bin:$PATH" DENY_TOML="$work/expired.toml" AUDIT_TODAY="$today" "$ci_dir/audit.sh" >/dev/null 2>&1
rc=$?
set -e
[ "$rc" -eq 1 ] || fail "expected exit 1 for the expired configuration, got $rc"
[ ! -e "$work/cargo-calls.txt" ] || fail "cargo ran although the ignores were invalid"
echo "$tag OK"
