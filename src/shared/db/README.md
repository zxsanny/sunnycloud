# sc_db

The only crate that owns PostgreSQL connections, transaction shape, tenant scope, the migrator and the schema version check. Contracts: `_docs/02_document/contracts/shared_db/sc_db_access.md` and `sc_db_migrations.md`.

## Migrator

`Migrator::run_pending` and `Migrator::revert` change the schema and refuse a `Db` that is not `DbRole::Migrator` (`MigrationError::WrongRole`); the one-shot `migrate` step is the only caller. `Migrator::status` and `Migrator::check_version` are read-only and work as either role; `serve` starts only on `Ok(SchemaState::Current)`. The files are embedded from `src/migrations/` at compile time (`build.rs` rebuilds the crate when the folder changes).

- Each migration runs in one transaction together with its bookkeeping row, under sqlx's session advisory lock. A failure returns `MigrationFailed { version, .. }`, leaves the earlier migrations applied and closes the locking connection.
- An applied file whose content changed is `ChecksumMismatch` (nothing is applied); a database with an applied version the binary lacks is `UnknownNewer`.
- `revert(db, steps)` runs the `down` of the newest `steps` applied migrations, one transaction each. A `down` that raises `irreversible: restore from backup` (SQLSTATE `P0001`) is reported as `IrreversibleMigration`; that migration stays applied.
- `Db::health` never fails and never waits longer than `db_acquire_timeout`; `Db::stats` is a synchronous snapshot (pool, per-`op` unit-of-work durations, last seen pending count).

## Writing a migration

1. **A reversible pair, always.** `src/migrations/YYYYMMDDHHMMSS_<verb>_<object>.up.sql` and `.down.sql` (sqlx does not mix pair and single files). `<object>` is the owner's domain word (`module-layout.md` section Migrations, for example `accounts_base`) followed by at least one more word: `create_accounts_base_table`. Pick a timestamp after every existing file.
2. **Applied files never change.** A change is a new pair. Additive by default (data_model 13.1): new tables, new nullable columns or columns with constant defaults, new indexes, new `CHECK` values. Dropping or renaming a column, changing a type or tightening a constraint is a contract step in a later release than the expand step.
3. **`down` undoes the `up`, including grants.** `scripts/ci/check-migrations.sh` runs up, down, up on an empty database and the second `up` must succeed on what `down` left.
4. **Irreversible changes** (a data rewrite, a drop in a contract release) get a header comment naming the owner and the reason, and a `down` that raises the marker: `DO $$ BEGIN RAISE EXCEPTION 'irreversible: restore from backup'; END $$;`. Reviewers treat each as an exception.
5. **Grants to `sunnycloud` live in the same file as the table** and are the only source of its privileges. The API role owns nothing. Roles come from `deploy/postgres/initdb.d/10-roles.sh`, never from a migration.
6. **No `now()`, no `DEFAULT now()`** in SQL (`scripts/ci/check-no-sql-now.sh`): ids and timestamps come from the application. Row-level policies read the tenant with `NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid`; the plain `::uuid` cast raises `22P02` on a reused connection whose setting was reset to the empty string.
7. **Tenant tables reference `accounts (id) ON DELETE RESTRICT`.** A delete that the reference blocks is SQLSTATE `23001`, which `DbError` maps to `ForeignKeyViolation` like `23503`.
8. **`-- no-transaction`** (first line of the file; for `CREATE INDEX CONCURRENTLY`) runs the file outside a transaction and without the bookkeeping row in the same transaction. Such a file must be idempotent (`IF NOT EXISTS`, cleanup of an invalid index) and is listed in the release notes. Verified against sqlx 0.9.0 (`tests/interrupted.rs`): on PostgreSQL sqlx never writes `success = false`, so an interrupted no-transaction file is **not** blocked as dirty. It leaves no bookkeeping row, `check_version` reports `Pending`, and the next `run_pending` runs the whole file again, which is why idempotence is required.

## Tests

Integration tests need a PostgreSQL 18 superuser URL in `DATABASE_URL` and create their own databases (cloned from a template that holds the two roles of ADR-014):

```bash
docker run -d --name sc-pg18 -e POSTGRES_PASSWORD=postgres -p 127.0.0.1::5432 postgres:18-trixie
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:<port>/postgres cargo test -p sc_db --locked
```

Migration tests build the "binary with one more file", a changed file or a failing file from the embedded set plus seeded pairs (`src/tests/seeded.rs`); the embedded files themselves are applied unchanged.
