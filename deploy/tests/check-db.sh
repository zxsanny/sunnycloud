#!/usr/bin/env bash
# AC-4 and AC-5 against a running dev db container.
# usage: check-db.sh CONTAINER   (e.g. the db service of `docker compose -f deploy/compose/compose.dev.yml up -d db`)
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
if [[ $# -ne 1 ]]; then echo "usage: $0 CONTAINER" >&2; exit 2; fi
C="$1"
sql() { docker exec "$C" psql -U postgres -d sunnycloud -At -F '|' -c "$1"; }
state() { sql "select rolname,rolsuper,rolbypassrls,rolcreatedb,rolcreaterole,rolcanlogin from pg_roles where rolname like 'sunnycloud%' order by 1; select pg_get_userbyid(nspowner) from pg_namespace where nspname='public'"; }
eq() { if [[ "$2" == "$3" ]]; then pass "$1"; else fail "$1: got '$2', want '$3'"; fi; }

eq "server major version" "$(sql 'show server_version_num' | cut -c1-2)" 18
eq "roles and schema owner" "$(state)" $'sunnycloud|f|f|f|f|t\nsunnycloud_migrator|f|f|f|f|t\nsunnycloud_migrator'
eq "sunnycloud owns nothing" "$(sql "select count(*) from (select 1 from pg_class where relowner='sunnycloud'::regrole union all select 1 from pg_namespace where nspowner='sunnycloud'::regrole union all select 1 from pg_database where datdba='sunnycloud'::regrole union all select 1 from pg_proc where proowner='sunnycloud'::regrole) o")" 0
eq "sunnycloud cannot create in public" "$(sql "select has_schema_privilege('sunnycloud','public','CREATE')")" f

# second run of the script on the same data directory changes nothing
before="$(state)$(sql "select nspacl from pg_namespace where nspname='public'")$(sql "select md5(string_agg(r::text, ',' order by rolname)) from pg_roles r where rolname like 'sunnycloud%'")"
docker exec -e POSTGRES_USER=postgres -e POSTGRES_DB=sunnycloud "$C" bash /docker-entrypoint-initdb.d/10-roles.sh
after="$(state)$(sql "select nspacl from pg_namespace where nspname='public'")$(sql "select md5(string_agg(r::text, ',' order by rolname)) from pg_roles r where rolname like 'sunnycloud%'")"
eq "second script run changes nothing" "$after" "$before"

# AC-5: data volume and loopback-only publication
eq "volume mounted at /var/lib/postgresql" "$(docker inspect -f '{{range .Mounts}}{{if eq .Destination "/var/lib/postgresql"}}{{.Type}}{{end}}{{end}}' "$C")" volume
eq "published binding" "$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$C")" '{"5432/tcp":[{"HostIp":"127.0.0.1","HostPort":"5432"}]}'
finish
