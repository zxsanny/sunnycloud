#!/usr/bin/env bash
# Creates the two application roles of ADR-014 on the server DATABASE_URL points at, when they are missing.
# The first migration (accounts_base) grants to `sunnycloud`, so a plain PostgreSQL service needs the roles
# before any migration runs; deploy/postgres/initdb.d/10-roles.sh does the same for the Compose database.
# Same attributes as that script, no password (CI roles are never logged into). Idempotent.
# Needs psql and a superuser (or CREATEROLE) DATABASE_URL.
set -euo pipefail

tag="[ensure-db-roles]"
: "${DATABASE_URL:?DATABASE_URL must point at a PostgreSQL 18 server}"
command -v psql >/dev/null || { echo "$tag FAILED: psql not found (postgresql-client)" >&2; exit 2; }

echo "$tag creating sunnycloud_migrator and sunnycloud when missing"
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 <<'SQL'
DO $$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'sunnycloud_migrator') THEN
    CREATE ROLE sunnycloud_migrator LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'sunnycloud') THEN
    CREATE ROLE sunnycloud LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION;
  END IF;
END
$$;
SQL
echo "$tag OK"
