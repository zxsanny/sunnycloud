#!/usr/bin/env bash
# Sign-in proof without a browser: setup token, then an invite for bob, each through the mock IdP with a cookie
# jar, plus the WebSocket upgrade through the one-origin web server. Needs a FRESH stack (the setup token works
# once per database): scripts/dev/acceptance-down.sh && scripts/dev/acceptance-up.sh. Prints no token or cookie value.
set -euo pipefail
# shellcheck source=acceptance-lib.sh
# shellcheck source=acceptance-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/acceptance-lib.sh"

JAR_DIR="$(mktemp -d)"
trap 'rm -rf -- "$JAR_DIR"' EXIT
CLI="$REPO_ROOT/scripts/dev/acceptance-cli.sh"
pass() { printf 'PASS  %s\n' "$*"; }
fail() { printf 'FAIL  %s\n' "$*" >&2; exit 1; }

curl_ok() { curl -sS --fail-with-body --max-time 20 "$@"; }

[ "$(curl_ok "$ACC_ORIGIN/api/v1/auth/providers" | jq -r .setup_required)" = true ] \
  || fail "the stack already has an account: run acceptance-down.sh and acceptance-up.sh for a fresh database"

# sign_in JAR BODY-JSON: begin, follow the mock IdP and the callback; leaves the session cookie in JAR.
sign_in() {
  local jar="$1" body="$2" authorization_url callback_url status
  authorization_url="$(curl_ok -c "$jar" -b "$jar" -H "Origin: $ACC_ORIGIN" -H 'content-type: application/json' \
    -d "$body" "$ACC_ORIGIN/api/v1/auth/login-attempts" | jq -r .authorization_url)"
  case "$authorization_url" in "$ACC_ISSUER/authorize?"*) ;; *) fail "authorization_url does not point at the mock IdP" ;; esac
  callback_url="$(curl_ok -o /dev/null -w '%{redirect_url}' -c "$jar" -b "$jar" "$authorization_url")"
  case "$callback_url" in "$ACC_REDIRECT_URI?code="*) ;; *) fail "the mock IdP did not redirect to the callback" ;; esac
  status="$(curl -sS -o /dev/null -w '%{http_code} %{redirect_url}' -c "$jar" -b "$jar" "$callback_url")"
  [ "$status" = "302 $ACC_ORIGIN/" ] || fail "callback answered '$status', expected a 302 to /"
}

token_of() { "$CLI" --json "$@" | jq -r .data.token; }

# 1. First-run setup with the setup token: alice becomes the admin.
alice_jar="$JAR_DIR/alice"
setup_token="$(token_of admin setup-token)"
sign_in "$alice_jar" "$(jq -nc --arg t "$setup_token" '{provider:"oidc",return_to:"/",client_kind:"web",setup_token:$t}')"
unset setup_token
alice="$(curl_ok -b "$alice_jar" "$ACC_ORIGIN/api/v1/accounts/me")"
[ "$(jq -r .role <<< "$alice")" = admin ] || fail "alice is not an admin: $alice"
[ "$(jq -r .display_name <<< "$alice")" = "Alice Admin" ] || fail "display name was not taken from the id token"
pass "setup token: alice signed in through the mock IdP and /accounts/me says role=admin"
[ "$(curl_ok "$ACC_ORIGIN/api/v1/auth/providers" | jq -r .setup_required)" = false ] || fail "setup_required is still true"
pass "setup_required is false afterwards"

# 2. The WebSocket upgrade through the web server, with and without the session.
cookie="$(awk -F'\t' '$6 == "__Host-session" {print $6 "=" $7}' "$alice_jar")"
ACC_COOKIE="$cookie" "$BUN" "$REPO_ROOT/tests/e2e/scripts/ws-probe.ts" "$ACC_ORIGIN" || fail "no frame from /api/v1/realtime with a session"
pass "WebSocket /api/v1/realtime upgrades through the web server and sends its first frame"
"$BUN" "$REPO_ROOT/tests/e2e/scripts/ws-probe.ts" "$ACC_ORIGIN" expect-refused || fail "/api/v1/realtime accepted a socket without a session"
pass "WebSocket without a session is refused"
unset cookie

# 3. An invite for bob.
alice_id="$(jq -r .id <<< "$alice")"
invite_token="$(token_of admin invite --admin "$alice_id" create)"
curl_ok -X POST -d '{"user":"bob"}' "$ACC_ISSUER/__control/user" > /dev/null
bob_jar="$JAR_DIR/bob"
sign_in "$bob_jar" "$(jq -nc --arg t "$invite_token" '{provider:"oidc",return_to:"/",client_kind:"web",invite_token:$t}')"
unset invite_token
curl_ok -X POST -d '{"user":"alice"}' "$ACC_ISSUER/__control/user" > /dev/null
bob="$(curl_ok -b "$bob_jar" "$ACC_ORIGIN/api/v1/accounts/me")"
[ "$(jq -r .role <<< "$bob")" = user ] || fail "bob is not a user: $bob"
[ "$(jq -r .display_name <<< "$bob")" = "Bob Builder" ] || fail "bob's display name is wrong"
[ "$(jq -r .id <<< "$bob")" != "$alice_id" ] || fail "bob has alice's account"
pass "invite: bob signed in and /accounts/me says role=user, a different account"
echo "all checks passed"
