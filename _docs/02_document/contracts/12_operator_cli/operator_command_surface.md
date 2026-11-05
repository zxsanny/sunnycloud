# Contract: operator command surface of the `sunnycloud` binary

**Component**: 12_operator_cli
**Producer task**: B08-02_cli_skeleton (E-22/c02) for the streams, the `--json` envelope, the exit codes and the confirmation rule; each command row is implemented by the task named in its row
**Consumer tasks**: this batch: E-22/c03, E-22/c04, E-22/c05, E-22/c07..c13, E-22/c14a, E-22/c15, E-22/c16, E-22/c17; other batches: the E-22 pipelines p01..p14 (they run the commands), E-31 system tests (FT-P-64, FT-P-65, FT-P-78, FT-N-25, FT-N-39, FT-N-46, FT-P-69, FT-P-71, FT-P-72, NFT-RES-13, NFT-RES-14, NFT-RES-37 and others), E-00/c08 (`e2e-ctl exec-operator`), E-00 Compose and deployment docs (`migrate` step, `healthcheck`)
**Version**: 1.0.0
**Status**: draft (freezes when the open points below are answered)
**Last Updated**: 2026-11-04

## Purpose

What an operator, a script, the Compose file and the test harness can rely on when they run `sunnycloud <command>`: the command names and arguments, the output streams, the `--json` envelope, the exit codes and the confirmation rule. The business result of each command belongs to the owning component's contract; this file covers only how 12 presents it. Sources: `components/12_operator_cli/description.md` §3, §4, §7, §8, §14, `canonical-names.md` (Operator commands), `tests/spec-gaps.md` (gap-pass defaults G-12-2, G-12-5, G-12-6, G-12-7).

## Shape

### Invocation

- Running stack: `docker compose exec api sunnycloud <command>`. Stopped API (restore drill, `doctor --pre-flight`, `admin epoch-bump` after a restore): `docker compose run --rm api sunnycloud <command>`.
- `migrate` runs as the one-shot `migrate` Compose step, the only container where `db_migrator_url` is mounted.
- `--json` is accepted by every command. Destructive commands accept `--yes`.

### Streams

| Stream | Carries |
|--------|---------|
| stdout | the result as text, or exactly one JSON object with `--json`; a one-time secret, printed once |
| stderr | usage text, progress lines (every `cli_progress_interval`, default 2s), diagnostics |
| structured log (only with `log = json`) | one line per command: `component=cli`, `command`, `exit_code`, duration; never a secret argument or the path of a user file |

### `--json` envelope

| Field | Type | Required | Description | Constraints |
|-------|------|----------|-------------|-------------|
| `result` | string | yes | outcome class of the command | value set not yet fixed (open point 1) |
| `code` | string | yes | on a refusal or failure, the owner's stable error code from helper 02's code table, for example `last_admin` | value on success not yet fixed (open point 1) |
| `data` | object | yes | the same facts the text mode prints | carries no secret except under open point 2 |

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | `doctor`: warnings only. `admin epoch-bump`: the bump returned an error (gap-pass default G-12-5) |
| 2 | operation failed, refused by the owner, or findings: `scrub run` with a mismatch or a missing blob, `doctor` with any Fail, `migrate` failure, checksum mismatch or unknown-newer database |
| 3 | usage or configuration error, including a destructive command without `--yes` on a non-TTY |
| 130 | SIGINT during `serve`, after the graceful shutdown |
| non-zero | `serve` start refusal (configuration, database role, schema version, master key, epoch marker); `healthcheck` when `/readyz` is not 200 |

### Confirmation rule

Destructive commands: `admin user delete`, `admin rewrap`, `admin epoch-bump`, `migrate revert`. Without `--yes`, a TTY gets a confirmation that names the target and proceeds only on a confirming answer; a non-TTY exits 3 and no business call is made. `cli_confirm_required` (default on) is the config key of this rule.

### Commands

| Command | Arguments | Business call (one) | Output | Exit | Task |
|---------|-----------|---------------------|--------|------|------|
| `serve` | none | composition root + `HttpServer` | start-up log lines | refusal non-zero; SIGTERM 0; SIGINT 130 | E-22/c05 |
| `migrate [run]` | none | `Migrator::run_pending` (`db_migrator_url`) | applied versions | 0; failure, checksum mismatch, unknown-newer 2; missing migrator secret 3 | E-22/c07 |
| `migrate status` | none | `Migrator::status` (either role) | applied, pending, dirty, unknown-newer | 0; unknown-newer 2 | E-22/c07 |
| `migrate revert` | `[--steps N] --yes` | `Migrator::revert` | reverted versions | 0; `IrreversibleMigration` 2; no `--yes` on non-TTY 3 | E-22/c07 |
| `admin setup-token` | none | `OperatorIdentityOps::ensure_setup_token` | token once and a sign-in hint | 0; `SetupNotAvailable` 2 with nothing on stdout | E-22/c08 |
| `admin create` | `[--note] [--ttl]` | `OperatorIdentityOps::create_admin_invite` | invite token once and a sign-in hint | 0 | E-22/c08 |
| `admin reset` | `<account>` | `OperatorIdentityOps::reset_admin` | fresh invite once | 0; `NotFound` 2; sole admin per G-12-3 | E-22/c08 |
| `admin promote`, `admin demote` | `<account>` | `AdminService::update_user` (role, `Actor::Operator`) | new role | 0; `LastAdmin` 2 | E-22/c08 |
| `admin invite create\|list\|revoke` | `create [--note]`, `list`, `revoke <id>` | `InviteService` | token once on `create`; never on `list` | 0 | E-22/c08 |
| `admin user list` | none | `AdminService::list_users` | account, role, state, last login, used, limit; no token | 0 | E-22/c09 |
| `admin user set-quota` | `<account> <size>` | `AdminService::set_quota` | new limit | 0 | E-22/c09 |
| `admin user delete` | `<account> --yes` | `AccountDeletionService::request` | deletion deadline | 0; `LastAdmin` 2; no `--yes` on non-TTY 3 | E-22/c09 |
| `admin dev-seed` | none | `DevSeed::seed` | seed summary | 0; refused when `environment = production` 2 | E-22/c09 |
| `admin audit export` | `--since <time>` | `AuditReader::export` (paged) | JSON lines, ids only | 0 | E-22/c09 |
| `admin epoch-bump` | `[--yes]` | `EpochAdmin::bump_epoch(now)`, then the `restore`-tagged doctor checks | new epoch and reconcile findings | 0; error 1; no `--yes` on non-TTY 3 | E-22/c10 |
| `admin restore` | `--account --path --at <RFC 3339 UTC> [--dry-run]` | `TreeRestore::restore_revision` | `Restored{revision, placement: Original \| Restored}` | 0; `BeyondRetention`, `NotFound` 2 | E-22/c10 |
| `admin gc-run` | none | `GcSweeper::run_cycle` once | counts | 0 | E-22/c10 |
| `key-check` | none | `MasterKeyring::load_and_verify` | match, never key bytes | 0; `FileMissing`, `WrongLength`, `KeyCheckMismatch`, `KeyCheckAbsentButBlobsExist` 2 | E-22/c11 |
| `admin keycheck-init` | none | `MasterKeyring::init_key_check` | key check written | 0; `KeyCheckSampleFailed` 2 | E-22/c11 |
| `admin rewrap` | `--new-key-file <path> [--yes]` | `MasterKeyring::rewrap_batch` looped until 0 rows | progress on stderr; "keep the old key" | 0; no `--yes` on non-TTY 3 | E-22/c11 |
| `scrub run` | `[--full] [--rate MiB/s]` | `Scrubber::start_run`, `run_step` until done, `finish_run` | live progress, findings | 0; any mismatch or missing blob 2 | E-22/c12 |
| `scrub status` | none | `Scrubber::status` | oldest scrub time, unscrubbed count, open findings | 0 | E-22/c12 |
| `admin media-requeue` | `[--account] [--kind]` | `MediaAdmin::requeue_dead` | count | 0 | E-22/c13 |
| `admin media-stats` | none | `MediaAdmin::stats` | queue depth per lane and status | 0 | E-22/c13 |
| `doctor` | `[--pre-flight] [--json] [--backup-interval <duration, e.g. 24h>] [--attest-backup]` | every registered `DoctorCheck` | findings: check, level, message, remedy | 0 ok; 1 warnings only; 2 any Fail | E-22/c14a |
| `healthcheck` | none | probe of `127.0.0.1:9090/readyz` | none required | 0 on 200; non-zero otherwise | E-22/c15 |
| `openapi` | none | `HttpServer` route registry | the OpenAPI document | 0 | E-22/c15 |
| `version` | none | local | semver and git commit | 0 | E-22/c15 |

Names not in this table (`key rewrap`, `media requeue`, a top-level `restore`) are usage errors (exit 3).

## Invariants

- Every command makes exactly one business operation call on its owning port; a looped command repeats the same operation (`admin rewrap`, `scrub run` steps, `admin audit export` and `admin user list` pages).
- A usage error or a refused confirmation makes no business call and changes no state.
- A one-time secret appears exactly once, on stdout, and never in stderr, log lines, audit rows or another command's output.
- Text mode and `--json` carry the same facts.
- No command except `serve` opens a network listener.
- `doctor` and `key-check` never print key material or secrets; `admin audit export` prints ids only.

## Non-Goals

- Not covered: the business result, errors and side effects of each operation (owner contracts, for example `01_identity/operator_identity_ops.md`, `04_blobstore/epoch-marker-and-health.md`).
- Not covered: which configuration sections each command binds (E-22/c04) and the `serve` start order (E-22/c05).
- Not covered: the HTTP API (`11_http_api`).

## Versioning Rules

- **Breaking changes** (a renamed or removed command or option, a changed exit code for an outcome, a changed `--json` field) require a new major version and notice to E-31, E-00 and the pipelines.
- **Non-breaking additions** (a new command, a new optional option, a new `data` field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| unknown-command | `sunnycloud key rewrap` | exit 3, usage on stderr, no business call | IT-12-1 |
| missing-confirmation | `admin user delete <id>` with stdin not a TTY, no `--yes` | exit 3, no business call | IT-12-1, ST-12-2 |
| owner-refusal | `admin demote <last admin> --json` | exit 2, one JSON object with `code = last_admin` | IT-12-1, IT-12-3 |
| doctor-levels | checks returning Ok; one Warn; one Fail | exit 0; 1; 2 | IT-12-1, IT-12-15 |
| secret-once | `admin create` with `log = json`, stdout and stderr captured apart | token once on stdout, 0 times elsewhere | IT-12-19, ST-12-1 |
| setup-unavailable | `admin setup-token` with one account | exit 2, `SetupNotAvailable`, nothing on stdout | FT-P-78 |

## Open points (flagged in the producer task)

1. The value set of `result` and the `code` on success are not stated.
2. With `--json`, whether a one-time secret is inside `data` (once) or printed outside the object is not stated.
3. Stable codes for operator-only outcomes (`SetupNotAvailable`, `BeyondRetention`, `KeyCheckMismatch`, `KeyCheckSampleFailed`, `FileMissing`, `WrongLength`, `KeyCheckAbsentButBlobsExist`, `IrreversibleMigration`, checksum mismatch, unknown-newer) are not in helper 02's code table.
4. The exit code when SIGINT cancels a long command other than `serve` is not stated (G-12-7 covers `serve` only).
5. The exact non-zero code (2 or 3) of each `serve` refusal kind is not stated (IT-12-6 accepts either).
6. `admin create --note --ttl` has no matching parameters in `OperatorIdentityOps::create_admin_invite()`.
7. The accepted format of `admin audit export --since` is not stated.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B08) |
