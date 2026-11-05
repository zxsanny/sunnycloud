# CI check scripts

CI YAML only calls these scripts (`_docs/04_deploy/ci_cd_pipeline.md`, Principles). Run them locally the same way.

## One-time setup: sqlx-cli

The command-line tool must match `sqlx = "0.9"` (workspace `src/Cargo.toml`). Pinned: `0.9.0`.

```bash
cargo install sqlx-cli --locked --version 0.9.0 --no-default-features --features rustls,postgres
sqlx --version        # sqlx-cli 0.9.0
cargo sqlx --version  # sqlx-cli-sqlx 0.9.0
```

Both scripts refuse to run on any other minor version. Both need a PostgreSQL 18 reachable through `DATABASE_URL`:

```bash
docker run -d --name sunnycloud-pg18 -e POSTGRES_PASSWORD=dev -p 127.0.0.1:5432:5432 postgres:18
export DATABASE_URL=postgres://postgres:dev@127.0.0.1:5432/postgres
```

## `check-migrations.sh`: up, down, up

Applies every migration to an empty database, reverts them all (newest first, one at a time), applies them again, and prints each step. It exits non-zero and names the version when an `up` or `down` file fails, when a file is not part of a reversible `YYYYMMDDHHMMSS_<verb>_<object>.up.sql` / `.down.sql` pair, or when the database already has applied migrations. With zero migrations it prints `0 migrations applied, reverted and re-applied`.

```bash
DATABASE_URL=... scripts/ci/check-migrations.sh
MIGRATIONS_DIR=/path/to/dir DATABASE_URL=... scripts/ci/check-migrations.sh   # default: src/migrations
```

The database is left in the fully migrated state. Drop and recreate it before the next run.

## `check-sqlx-prepare.sh`: offline query data is current

Runs `cargo sqlx prepare --check --workspace` in `src/` and fails when the committed `src/.sqlx/` data does not match the compile-time checked queries. The database behind `DATABASE_URL` must already have all migrations applied (run `check-migrations.sh` first, or `sqlx migrate run --source src/migrations`). To refresh the data after changing a query:

```bash
cd src && cargo sqlx prepare --workspace   # then commit src/.sqlx
```

Builds without a database use `SQLX_OFFLINE=true`, which reads `src/.sqlx/`. `WORKSPACE_DIR` overrides the workspace directory (used by the tests).

## Tests of the scripts

```bash
scripts/ci/tests/test-no-quiet-flags.sh                     # no database needed
DATABASE_URL=... scripts/ci/tests/test-check-migrations.sh        # needs CREATE DATABASE rights
DATABASE_URL=... scripts/ci/tests/test-check-sqlx-prepare.sh      # builds a scratch workspace under /tmp
```

The database tests seed their files only in temporary directories and create and drop their own databases.
