# 12 Operator CLI

## 1. High-Level Overview

**Purpose**: The `sunnycloud` binary crate: the composition root, the janitor scheduler, the server start/stop sequence and the operator commands (run on the server host with `docker compose exec|run api sunnycloud ...`). Command handling is a thin delivery layer: parse arguments, build the needed services, call **one** business operation per command, print the typed result, map it to an exit code. One responsibility: *wire the components together and translate operator intent into calls on the owning components*.

**Architectural pattern**: Command-per-module delivery layer (clap) over a composition root with lazy service construction. No SQL, no blob file I/O, no crypto and no business rules in this crate (grep tests enforce: no `sqlx`, no `blobs/` paths). Janitor loops are timers around `run_cycle` calls and hold no logic.

**Boundary decisions (decisions 16, 17)**: the `sunnycloud` binary crate owns `main()`, the clap command tree, the **composition root** (`AppContext`: constructor injection of every business component behind its `contracts` port) and the **janitor loops**. It is the only crate that depends on every component crate. 11_http_api is a library that receives `ApiServices` (port objects) and runs the listeners; no component depends on 11 or on 12.

**Crate dependencies**: every component crate (to construct its implementations), 11_http_api (`HttpServer`), `contracts`, helpers 01 config, 02 error, 03 clock, 04 audit, 07 db (`Migrator`, `DbTx`, pool, system scope), 08 ratelimit (constructs the `AbuseLimiter` once).
**Downstream consumers**: the operator, CI restore drill (AC-65), install guide (AC-22).

**Scope in**: argument parsing, confirmation guards, output (text and `--json`), exit codes, secret printing, doctor aggregation, progress display; composition root; scheduler; start-up and shutdown order; config binding and validation.
**Scope out**: every operation's logic and its audit row (the owning service writes `admin_action` with `Actor::Operator`), HTTP handlers and listeners (11), what each janitor cycle does (owners).

## 2. Ports (declared in `contracts`)

**Provides**
| Port | Methods | Consumers |
|---|---|---|
| `Janitor` registry (`Scheduler`) | registers owners' `Janitor`s and calls `run_cycle(now)` on their interval | internal |
| `ReadinessCheck` (`scheduler_alive`) | `/readyz` item | 11 |
| `DoctorCheck` registry | collects the checks implemented by owners | `doctor` command |

**Requires** (one business call per command; the owner implements, 12 injects)
| Port | Provider | Used by command |
|---|---|---|
| `OperatorIdentityOps`, `InviteService` | 01_identity | `admin create`, `admin reset`, `admin setup-token`, `admin invite ...` |
| `AdminService` (`Actor::Operator`), `AccountDeletionService`, `DevSeed` | 07_admin | `admin promote\|demote`, `admin user ...`, `admin dev-seed` |
| `QuotaAdmin` | 07a_quota | `admin user set-quota`, doctor drift check |
| `TreeRestore` | 02_tree | `admin restore` |
| `EpochAdmin` (`bump_epoch`, `marker_status`) | 05_sync_feed | `admin epoch-bump`; `serve` start check |
| `MasterKeyring`, `Scrubber` (`start_run`, `run_step`, `finish_run`, `status`), `GcSweeper`, `EpochMarker` | 04_blobstore | `key-check`, `admin keycheck-init`, `admin rewrap`, `scrub ...`, `admin gc-run` |
| `MediaAdmin` (`requeue_dead`, `stats`) | 08_media_jobs | `admin media-requeue`, `admin media-stats` |
| `AccountLifecycleHook` (list), `BlobOwnerCleanup` (list) | 02, 03, 05, 06, 07a, 08, 20 / 02, 08 | registered late by 12 (breaks wiring cycles); the identity list goes through `register_hooks` on the identity service (set once, only 12 calls it); driven by 07's `DeletionPurger` and 04's GC |
| `AuditReader` | helper 04 | `admin audit export` |
| `Migrator` (`run_pending`, `revert`, `status`, `check_version`) | helper 07 db | `migrate ...`, `serve` start check |
| `Janitor` (list) | 01, 02, 03, 04, 07, 08, helper 04 | scheduler |
| `DoctorCheck` (list) | 01, 02, 04, 05, 07a, 08, 11 and helpers | `doctor` |
| `HttpServer` | 11_http_api | `serve`, `healthcheck`, `openapi` |

## 3. Command surface (final names; supersede the provisional names in the deployment docs and `architecture.md`)

| Command | Business call (one) | Result and exit |
|---|---|---|
| `admin create [--note] [--ttl]` | `OperatorIdentityOps::create_admin_invite` | prints the one-time admin invite token **once** and the sign-in hint; 0 |
| `admin reset <account>` | `OperatorIdentityOps::reset_admin` | sessions of that admin revoked, fresh invite printed once (also for the only active admin, G-12-3); `NotFound` -> 2 |
| `admin promote\|demote <account>` | `AdminService::update_user` (role) | the last-admin rule is 07's; `LastAdmin` -> 2 |
| `admin setup-token` | `OperatorIdentityOps::ensure_setup_token` | only while zero accounts exist: replaces the open token, prints it once; otherwise `SetupNotAvailable` -> 2 |
| `admin invite create\|list\|revoke` | `InviteService` (system scope) | token once on create |
| `admin user list\|set-quota\|delete` | `AdminService` / `AccountDeletionService` | delete needs `--yes`; uses the normal deletion path (AC-27) |
| `admin epoch-bump [--yes]` | `EpochAdmin::bump_epoch(now)`, which also writes the `.epoch` marker through `EpochMarker`; then the `restore`-tagged doctor checks | prints the new epoch and the reconcile findings (rows without blobs, orphans); 0 success, 1 error |
| `admin restore --account --path --at <RFC 3339 UTC timestamp> [--dry-run]` | `TreeRestore::restore_revision` (publishes through the normal commit path) | `Restored{revision, placement}`; `placement` is `Original`, or `Restored` when the original folder is deleted (02 puts the file into a "Restored" folder at the root); `BeyondRetention`, `NotFound` -> 2 |
| `admin rewrap --new-key-file [--yes]` | `MasterKeyring::rewrap_batch` looped until 0 rows remain | progress on stderr; old key must be kept (message) |
| `admin keycheck-init` | `MasterKeyring::init_key_check` | first unwraps a sample of existing `blobs.wrapped_key` with the candidate key and writes `.keycheck` only if every sample unwraps; `KeyCheckSampleFailed` -> 2 |
| `key-check` | `MasterKeyring::load_and_verify` | 0 match; `FileMissing`, `WrongLength`, `KeyCheckMismatch`, `KeyCheckAbsentButBlobsExist` -> 2; never prints key bytes |
| `admin gc-run` | `GcSweeper::run_cycle` once (drains until nothing is due or the cycle budget ends) | counts; 0 |
| `scrub run [--full] [--rate MiB/s]` | `Scrubber::start_run(mode, rate)` (`--full` is `Full`, otherwise `Due`; `--rate` is the `rate` argument, default `scrub_max_rate`), then `run_step(&run)` looped until `StepResult.done`, then `finish_run(&run)` | live progress; findings listed; **2 when any mismatch or missing blob** (restore drill expects 0); the summary is written by 04 as an audit event and metrics (decision 6). A CLI run cannot see the API's activity gauge, so `--rate` is its only throttle |
| `scrub status` | `Scrubber::status` | oldest `last_scrubbed_at`, unscrubbed count, open `scrub_mismatch` findings, suspect set (derived from `blobs.last_scrubbed_at` and audit events; no table); 0 |
| `admin audit export --since` | `AuditReader::export` | JSON lines to stdout, ids only |
| `admin media-requeue [--account] [--kind]` | `MediaAdmin::requeue_dead` | requeues dead jobs after a policy fix; count; 0 |
| `admin media-stats` | `MediaAdmin::stats` | queue depth per lane and status; 0 |
| `admin dev-seed` | `DevSeed::seed` | refused when `environment = production` |
| `migrate [run]` | `Migrator::run_pending` (uses `db_migrator_url`) | see section 4 |
| `migrate status` | `Migrator::status` (either role; the API role can read the migrations table) | applied, pending, dirty, unknown-newer; newer than binary -> 2 |
| `migrate revert [--steps N] --yes` | `Migrator::revert` (uses `db_migrator_url`) | CI and recovery only |
| `doctor [--pre-flight] [--json] [--backup-interval D] [--attest-backup]` | every registered `DoctorCheck` (`--backup-interval` is a duration such as `24h`) | 0 ok, 1 warnings only, 2 any failure (a missing blob is a failure) |
| `serve`, `healthcheck`, `openapi`, `version` | composition root + `HttpServer` / local | `serve` runs section 5 and exits non-zero when the schema is not current or the database epoch is below the `.epoch` marker; healthcheck probes `127.0.0.1:9090/readyz`; `openapi` prints the document used by CI diffs |

**Exit codes**: SIGINT during `serve` exits 130 after graceful shutdown; 0 success; 1 warnings only (doctor); 2 operation failed, refused by the owner, or findings; 3 usage or configuration error (including a destructive command without `--yes` on a non-TTY). `--json` prints `{result, code, data}` with the owner's stable error code.

## 4. `migrate` and rollback behaviour (interrupted upgrade rolls back and the server refuses to start)
- `migrate` is a one-shot step that runs before the API starts (S4-2): a separate Compose step that must complete successfully before `api` starts. It connects with `db_migrator_url` (role `sunnycloud_migrator`, schema owner, the only role with DDL). The API role `sunnycloud` has no DDL and the API never migrates itself; `db_migrator_url` is not mounted in the API container.
- One transaction per migration (PostgreSQL DDL is transactional) under sqlx's advisory lock, so two concurrent `migrate` runs cannot interleave. Failure or kill: that migration rolls back, earlier ones stay applied, exit 2, output names the failed version and error; the next `migrate` retries from it. Applied files are immutable (checksum mismatch -> 2).
- A database that has a version unknown to the binary (newer) -> 2; `-- no-transaction` migrations are idempotent and `status` reports a half-done one as `dirty`.
- **`serve` checks, it does not migrate**: `Migrator::check_version` must return `Current`; `Pending` or `UnknownNewer` refuses the start with a message naming `sunnycloud migrate`. `FT-N-39`, `NFT-RES-14`.
- `revert` runs `down` files in reverse, each in its own transaction; a `down` that raises `irreversible: restore from backup` surfaces as typed `IrreversibleMigration` and stops. Because migrations are expand/contract, an image rollback normally needs no `revert`.

## 5. Composition root, scheduler, start-up and shutdown

**Start (`serve`)**: bind typed config and validate (retention 0..90, at least one provider, TTL ranges, `public_url`; unknown `SUNNYCLOUD_*` keys refuse) -> connect with `db_api_url` (helper 07 verifies the role) -> **schema version check**: `check_version` is `Current` or the process exits non-zero (no migrations are run here) -> master key check (04) -> **epoch marker check**: `EpochAdmin::marker_status` reports whether the database epoch is below the `.epoch` marker on the blob volume; if so the process refuses to start and prints `admin epoch-bump` (a restore without a bump would let devices that are ahead keep a stale cursor) -> construct components behind their ports (lifecycle hooks, `DoctorCheck`s, `HealthContributor`s, `ReadinessCheck`s registered here) -> register the late lists (lifecycle hooks, including `register_hooks` on identity, `BlobOwnerCleanup`s, janitors, checks) -> build `ApiServices` and `HttpServer` (listeners: public `:8080`, internal `:8081` for workers, ops `:9090`) -> start the scheduler -> `ready`. The lists are set before any listener accepts a request, so a sign-in never sees an unset hook list. One API process (decision 12).

**Scheduler**: runs named `Janitor`s (`name()`, interval from config, `run_cycle(now)`). The loops contain no logic. 12 constructs the scheduler and the `AbuseLimiter` rate limiter and hands them to 11 (11 constructs neither). Backlog cycles (GC, the account-erase purger) drain until nothing is due, within their time budget. A failed cycle is logged at ERROR with a metric and the loop continues (documented best-effort survival); each cycle claims rows with `SKIP LOCKED` or leases, so no advisory-lock leader is used (a lock would hold a session across the cycle's I/O). `admin gc-run` and `scrub run` call the same `run_cycle` from the CLI process.

| Task | Owner |
|---|---|
| session reaper, receipt janitor, staged-upload expiry | 03_transfer |
| GC sweeper, orphan sweep, scrub cycle (`ScrubJanitor`) | 04_blobstore |
| tombstone purge, folder cascade | 02_tree |
| account-deletion purger | 07_admin |
| identity janitors (expired grants, oauth states, old sessions) | 01_identity |
| job reaper, derived janitor | 08_media_jobs |
| audit retention | helper 04 |

**Shutdown (SIGTERM, `SUNNYCLOUD_SHUTDOWN_GRACE` 25s)**: readiness 503 -> `HttpServer` stops accepting, wakes long-polls and drains connections -> stop the scheduler -> flush upload progress checkpoints (03) -> final group sync (04) -> close pool. Unacked blocks are simply re-sent by clients.

## 6. Doctor (aggregation only)
`DoctorCheck { name, tags (restore, pre-flight), run(ctx) -> Vec<Finding{level Ok|Warn|Fail, message, remedy}> }` is a port **defined in `contracts`, implemented by the owners**. Checks: config (helper 01 validate), public URL/TLS 1.3/HTTP/2/proxy body probe (11), database version, API role attributes (`sunnycloud`: not superuser, no `BYPASSRLS`, not the schema owner; helper 07) and pending migrations through `check_version` (the migrator URL is not mounted in the API container, so doctor does not use it), master key and key-check (04), blob volume and free-space floor (04), providers and redirect URIs (01), setup state (01), decoder self-check, image age and worker presence from `worker_status` (08; the supervisor reports it over the worker API, there is no other heartbeat transport), retention vs `--backup-interval` (04), clock skew (helper 03), quota drift by `QuotaAdmin::recompute` (07a, AC-74), missing blobs per revision via `BlobInventory::find_missing` mapped to paths (04 + 02, AC-73), epoch head report (05) and the **`.epoch` marker check** (decision 5: the marker on the blob volume is written at every bump and only ever rises; a database epoch below it means the bump after a restore is missing -> Fail; `serve` refuses to start on the same condition). Read-only, no secrets in output, `--json` identical content. `--pre-flight` prints the exact redirect URIs to register and checks config, key length and the volume before the stack starts (AC-22).

## 7. Data owned, side effects
No tables, no files. Side effects are the owners' (invite rows, epoch rows, GC rows, audit rows, the `.epoch` marker). The CLI itself writes stdout/stderr; **secrets go to stdout through a `SecretOut` writer that bypasses the logger**, once, and are never stored or echoed by `--json` logs. Long commands install a SIGINT handler that cancels cooperatively through the owner's `CancellationToken`.

## 8. Configuration
Reads the same typed sections as the server (helper 01) but builds services **lazily**, so `migrate`, `admin setup-token` and `key-check` do not require what they do not use (for example `migrate` works with a missing master key). Extra keys: `cli_confirm_required` (default on), `cli_progress_interval` (2s); `scrub run` takes its default rate from `scrub_max_rate`. Secrets by file: `db_api_url`, `db_migrator_url` (only the `migrate` step), `master_key_file`, the `--new-key-file` argument. Key names are those of helper 01; there is no worker database secret. Scheduler intervals are owned by their components. Durations from config only.

## 9. Security notes
Authority is host access to the Compose project (no network listener of its own, no login). Commands run in database system scope only through the single system repository module of helper 07 (CI grep test). Tokens are issued by the owners (256-bit, only SHA-256 stored), printed once, never logged; `doctor` and `key-check` never print key material or secrets; `audit export` has ids only. Destructive commands need `--yes` or an interactive confirmation naming the target. Every state-changing command's owner records `admin_action` (command name, ids, no secrets).

## 10. ACs implemented
| AC | Release | Part |
|---|---|---|
| AC-23 | v1.0 | `admin create/reset/promote`, `admin setup-token` (create or reset admins without the web) |
| AC-30 | v1.0 | `admin restore` by path and time, `admin gc-run` |
| AC-41 | v1.0 | `admin epoch-bump` in the restore procedure, `.epoch` marker check in doctor and in `serve` |
| AC-43 | v1.0 | `admin rewrap`, `admin keycheck-init` (sample unwrap first), `key-check` |
| AC-24 | v1.0 | `scrub run/status` |
| AC-65 | v1.0 | `migrate`, restore drill commands, `scrub run --full` exit code |
| AC-22 | v1.0 | `doctor --pre-flight` redirect URIs, `doctor` exit 0 as install check |
**Interaction ACs folded in**: **AC-73** [v1.0]: `doctor` lists every revision whose blob is missing (path, account, revision id) and exits 2 or 1 by policy while listings stay error-free; **AC-72** [v1.0]: `key-check` and `doctor` fail on a master key that does not match the stored check, and start-up order puts the key check before any listener; **AC-80** [v1.0]: `scrub run` is safe beside a live API because 04 skips non-`committed` rows and re-checks state before reporting; **AC-74** [v1.0]: doctor's quota recompute reports drift; **AC-68/AC-80**: the scheduler cycles are the only janitors.

## 11. Tests
FT-P-64, FT-P-65, FT-N-20, FT-N-21 (setup token, admin, invites), FT-P-16, FT-P-17, FT-P-18, FT-N-35 (retention, restore, clock), FT-P-43, NFT-SEC-26 (scrub), FT-P-69, NFT-PERF-30 (install, doctor), FT-P-71, FT-P-72, FT-N-33, FT-N-39, NFT-RES-07, NFT-RES-10, NFT-RES-13, NFT-RES-14 (restore, upgrade, interrupted migration), FT-P-41 (rewrap), NFT-SEC-38 (no secrets in output/logs). Unit: argument grammar, exit-code mapping table, `--yes` guard, secret writer, lazy construction, scheduler survives a failing cycle, start-up order (config, schema version, key check, epoch marker, listeners, scheduler), `serve` refuses on a pending migration and on a database epoch below the marker, `keycheck-init` refuses a wrong key, `scrub run --full --rate` calls `start_run(Full, rate)`, restore into a deleted folder reports placement `Restored`, doctor `.epoch` check and worker-status levels. Each command has one test with a fake owner port asserting the **single** call.

## 12. Component acceptance
Every command in the table runs against the Compose test stack with the documented exit codes; the CI restore drill (dump, wipe, restore, `admin epoch-bump`, `scrub run --full` exit 0, `doctor`) passes end to end; skipping the epoch bump makes `doctor` fail and `serve` refuse to start; a secret appears once on stdout and nowhere in logs, JSON output or audit rows; the crate contains no SQL and no business rule (grep test green).

## 13. Dependency graph
**Implement after**: `contracts` and the owners' port implementations (01, 02, 04, 05, 07, 07a, 08, helper 07) and 11 (`HttpServer`). 09 is not a dependency: the supervisor is a separate binary. **Parallel with**: 14 to 19. **Blocks**: the CI restore drill and the install guide.

## 14. Logging
Human text to stdout, progress and diagnostics to stderr; structured JSON logs (`component=cli`, `command`, `exit_code`, duration) only when `SUNNYCLOUD_LOG=json`; never arguments that are secrets or paths of user files. `serve` logs the start-up steps and scheduler cycle failures.

## 15. Risks and open points
**Defaults chosen at Step 5 gap pass**: `admin epoch-bump` exits 0 success, 1 error; `doctor` exits 0 ok, 1 warnings only, 2 any failure (missing blobs fail); SIGINT during `serve` exits 130 after graceful shutdown; `--at` is an RFC 3339 UTC timestamp; `--backup-interval` is a duration such as `24h`.
2. `admin reset` semantics depend on 01 (identity is `(provider, id)`, so a lost provider login cannot be re-linked; reset issues a new admin invite and the old account is demoted by the operator).
3. The composition root grows with every component: wiring only, no logic; the grep test and review rule keep it so.
