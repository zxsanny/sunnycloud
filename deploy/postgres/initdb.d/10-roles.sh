#!/usr/bin/env bash
# Creates the two application roles of ADR-014 on an empty data directory (the postgres image runs
# initdb.d only then). Roles only: grants to `sunnycloud` ship in migrations.
#
# Secret choice (open question of AZ-3741, decided here): the role passwords are read from the
# `db_migrator_url` and `db_api_url` secret files, the same files the migrate and api services use to
# log in, so a role password exists in exactly one place. The password is the (percent-decoded) userinfo
# password of each URL; the URL user must be the role it is meant for. The db service therefore mounts
# these two files besides `postgres_password`.
#
# Idempotent: an existing role is left untouched (a password is never rewritten); the schema owner
# change and the revokes are repeatable.
set -euo pipefail

SECRETS_DIR="${SUNNYCLOUD_SECRETS_DIR:-/run/secrets}"

# Prints the percent-decoded password of the URL in file $1 after checking that its user is $2.
password_from_url_file() {
  local file="$1" role="$2" url userinfo user pw
  [ -r "$file" ] || { echo "10-roles.sh: secret file $file is missing or unreadable" >&2; return 1; }
  url="$(tr -d '\r\n' < "$file")"
  case "$url" in
    postgres://*|postgresql://*) ;;
    *) echo "10-roles.sh: $file is not a postgres:// URL" >&2; return 1 ;;
  esac
  userinfo="${url#*://}"
  userinfo="${userinfo%%@*}"
  case "$userinfo" in
    *:*) ;;
    *) echo "10-roles.sh: $file has no password in its URL" >&2; return 1 ;;
  esac
  user="${userinfo%%:*}"
  pw="${userinfo#*:}"
  if [ "$user" != "$role" ]; then
    echo "10-roles.sh: $file names user '$user', expected '$role'" >&2
    return 1
  fi
  [ -n "$pw" ] || { echo "10-roles.sh: $file has an empty password" >&2; return 1; }
  printf '%b' "${pw//%/\\x}"
}

MIGRATOR_PASSWORD="$(password_from_url_file "$SECRETS_DIR/db_migrator_url" sunnycloud_migrator)"
API_PASSWORD="$(password_from_url_file "$SECRETS_DIR/db_api_url" sunnycloud)"
export MIGRATOR_PASSWORD API_PASSWORD

# Passwords reach psql through the environment (\getenv), never through argv or the SQL on stdin of a shell.
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<'SQL'
\getenv migrator_pw MIGRATOR_PASSWORD
\getenv api_pw API_PASSWORD

SELECT format('CREATE ROLE sunnycloud_migrator LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %L', :'migrator_pw')
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'sunnycloud_migrator') \gexec

SELECT format('CREATE ROLE sunnycloud LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION PASSWORD %L', :'api_pw')
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'sunnycloud') \gexec

-- The application schema is `public` of the application database; the migrator owns it and all DDL.
-- `sunnycloud` keeps USAGE through PUBLIC and gets no CREATE.
ALTER SCHEMA public OWNER TO sunnycloud_migrator;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
SQL
