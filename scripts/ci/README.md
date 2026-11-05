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

Before step 5 the script runs `ensure-db-roles.sh`: the first migration (`accounts_base`) grants to the role `sunnycloud`, and a plain PostgreSQL service has no application roles (the Compose database gets them from `deploy/postgres/initdb.d/10-roles.sh`). `ensure-db-roles.sh` creates `sunnycloud_migrator` and `sunnycloud` when missing, with the attributes of that init script and no password; it needs `psql` and a superuser `DATABASE_URL`. `rust-lint.sh` calls it before its own `sqlx migrate run`.

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

## PR gate: `.github/workflows/ci.yml` and its scripts (AZ-3761)

The workflow only checks out, caches and calls these scripts, each under `watchdog.sh`. Jobs: `rust-lint`, `rust-test`, `pg18-migrations`, `supply-chain`, `images`; triggers: pull request to `dev`, push to `dev`.

| Script | Job | What it does |
|--------|-----|--------------|
| `setup-rust.sh` | all cargo jobs | `rustup toolchain install` from `src/rust-toolchain.toml`, verifies the pinned channel |
| `install-tools.sh TOOL...` | all cargo jobs | pinned `cargo-deny`, `cargo-llvm-cov`, `gitleaks` (SHA-256 verified downloads), `cargo-audit`, `sqlx-cli` (`cargo install --locked --version`); the versions live in that script |
| `rust-lint.sh` | `rust-lint` | `cargo fmt --check`, clippy `-D warnings`, `sqlx migrate run`, `check-sqlx-prepare.sh` (needs `DATABASE_URL`) |
| `check-coverage.sh` | `rust-test` | `cargo llvm-cov test --workspace --locked` (lists every test), then 75% overall and 90% per entry of `critical-paths.txt` (starts empty); prints the measured values |
| `check-migrations.sh` | `pg18-migrations` | see above |
| `check-pinning.sh [ROOT]` | `supply-chain` | `uses:` = 40-hex SHA + `# vX.Y.Z`, images by digest, default `permissions: contents: read`, no fork checkout under `pull_request_target` |
| `check-licence.sh` | `supply-chain` | `LICENSE` matches `licence.sha256`; `COPYING.iOS` checked once it exists |
| `audit.sh` | `supply-chain` | validates the dated ignores in `src/deny.toml` (`added`, `expires`, `why`; not expired; `added` at most 90 days old), passes the same ids to `cargo audit --deny warnings`, runs `cargo deny check advisories bans licenses sources`. `DENY_TOML` and `AUDIT_TODAY` override the file and the date |
| `build-wasm.sh` | `supply-chain` | wasm32 build of `sync-core-wasm` and `sc_ids` |
| `secret-scan.sh` | `supply-chain` | gitleaks over the full history with `gitleaks.toml` (built-in rules plus a task-id allowlist); findings name the file, values are redacted; a shallow clone is refused |
| `build-images.sh` | `images` | builds api, worker, decoder and the `-test` api and worker images, then runs `deploy/tests/check-images.sh`; never pushes |
| `watchdog.sh [--name N] -- CMD...` | every long step | heartbeat every 30 s (`WATCHDOG_TICK`), STALLED and abort of the whole process group after 120 s without output (`WATCHDOG_STALL`), exit 124 |

Tests: `tests/run-script-tests.sh` (run by `supply-chain`) covers the pinning, audit, coverage-threshold, watchdog (fast), secret-scan and quiet-flag checks. Run separately: `tests/test-watchdog-realtime.sh` (about 2.5 minutes, default 30 s / 120 s) and `tests/test-check-coverage-real.sh` (needs `cargo-llvm-cov`).

Advisory ignore format in `src/deny.toml`: `{ id = "RUSTSEC-YYYY-NNNN", reason = "added=YYYY-MM-DD; expires=YYYY-MM-DD; why=..." }`.
