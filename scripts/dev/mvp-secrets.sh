#!/usr/bin/env bash
# Creates secrets-dev/ for the MVP stack (gitignored). Idempotent: an existing file is never touched.
# Never prints a secret value. Run again after editing nothing; edit the PLACEHOLDER files by hand.
#
# Modes: directory 700; files 600, except the three that the Postgres container reads as uid 999 (644; the
# directory mode keeps other host users out).
# Google sign-in: put the client id into oidc_client_id and the client secret into provider_oidc_client_secret.
# ACME_EMAIL=<address> in the environment of the first run fills acme_email; otherwise it is a placeholder and
# Caddy registers its ACME account without an email.
set -euo pipefail
# shellcheck source=scripts/dev/mvp-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/mvp-lib.sh"

DB_PORT="${MVP_DB_PORT:-5532}"
umask 077
mkdir -p "$MVP_SECRETS"
chmod 700 "$MVP_SECRETS"
mkdir -p "$MVP_DATA/blobs" "$MVP_RUN" "$MVP_DATA/scratch-interactive" "$MVP_DATA/scratch-general"

created=()
# make_secret NAME MODE GENERATOR...  (the generator writes the value on stdout)
make_secret() {
  local name="$1" mode="$2"
  shift 2
  local path="$MVP_SECRETS/$name"
  if [ -e "$path" ]; then
    return 0
  fi
  "$@" > "$path"
  chmod "$mode" "$path"
  created+=("$name")
}

hex() { openssl rand -hex "$1"; }
text() { printf '%s' "$1"; }

make_secret postgres_password 644 hex 24
# Database URL of role $1 with a fresh random password; the init script of the db container takes the role
# passwords from these two files.
db_url() { printf 'postgres://%s:%s@127.0.0.1:%s/sunnycloud' "$1" "$(openssl rand -hex 24)" "$DB_PORT"; }
make_secret db_migrator_url 644 db_url sunnycloud_migrator
make_secret db_api_url 644 db_url sunnycloud
make_secret master_key 600 openssl rand 32
# The key table's `master_key_file` is a secret whose text is the PATH of the 32-byte key file (04 keyring).
make_secret master_key_file 600 text "$MVP_SECRETS/master_key"
make_secret worker_api_token 600 hex 24
# `share_grant_key` is read as UTF-8 text and must be at least 32 bytes: 64 hex characters.
make_secret share_grant_key 600 hex 32
make_secret provider_oidc_client_secret 600 text "${PLACEHOLDER_PREFIX}_google_client_secret_goes_here"
make_secret oidc_client_id 600 text "${PLACEHOLDER_PREFIX}_google_client_id_goes_here"
make_secret acme_email 600 text "${ACME_EMAIL:-${PLACEHOLDER_PREFIX}_acme_email_goes_here}"
# Non-secret host-process settings, editable copy of the committed example.
make_secret mvp.env 600 cat "$REPO_ROOT/deploy/mvp/mvp.env.example"

if [ "${#created[@]}" -eq 0 ]; then
  log "secrets-dev is complete; nothing created"
else
  log "created in $MVP_SECRETS: ${created[*]}"
fi
for name in provider_oidc_client_secret oidc_client_id acme_email; do
  if is_placeholder "$MVP_SECRETS/$name"; then
    log "placeholder still in place: $name"
  fi
done
