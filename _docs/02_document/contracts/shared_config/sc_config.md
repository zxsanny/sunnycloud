# Contract: sc_config — typed, validated settings

**Component**: shared/config (crate `sc_config`)
**Producer task**: E-01/c01 — `_docs/02_tasks/todo/B01-07_sc_config.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-06/c01a, E-06/c01b, E-07/c01, E-08/c01, E-09/c01; other batches: every server component, 09, 10, 13, 14, 15 (endpoint path), `mobile_backup`
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

One loader that turns compiled defaults, a config file, `SUNNYCLOUD_*` environment variables and secret files into immutable typed sections, refusing to start with every problem listed at once. The key table of helper 01 is the single list of keys for the three binaries.

Sources: `common-helpers/01_helper_config.md` (key table authoritative), `epics/E-01_cross-cutting-configuration.md`.

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `load` | `load::<T: ConfigSection>(sources: &ConfigSources) -> Result<Arc<T>, ConfigError>` | `ConfigError::{Invalid(Vec<KeyProblem>), MissingSecret(key), Unreadable(path, io_kind)}` | sync (reads the named files only) |
| `ConfigSection::validate` | `fn validate(&self, env: Environment) -> Vec<KeyProblem>` | none (collects) | sync |
| `DurationSetting::get` | `fn get(&self) -> Duration` | none (validated at load) | sync |
| `Secret<T>` | `Secret::expose(&self) -> &T`; `Debug`/`Display` print `***` | none | sync |
| `reload_client_settings` | `(json: &str) -> Result<Arc<T>, ConfigError>`; on success swaps one `Arc` atomically, on error the previous settings stay active | `ConfigError::Invalid` | sync |

| Type | Shape |
|---|---|
| `ConfigSources` | `env: map of variables, files: config file paths, secrets_dir (default /run/secrets), environment: Environment` |
| `Environment` | `Production` (default), `Development`, `Test` |
| `KeyProblem` | `key, reason: OutOfRange{min, max} \| WrongType \| Unknown \| MissingRequired \| ForbiddenInRelease` |
| `KeySpec` | `key, type, default, secret: bool, duration_range: Option<(floor, ceiling)>, read_by: {api, worker, decoder, clients}` (one row per key of helper 01) |

Rules carried by the contract:
- Binding order: defaults, config file, environment, secret files; later wins.
- Environment name = `SUNNYCLOUD_` + key upper-cased; no alias; any `SUNNYCLOUD_*` variable not in the table refuses start (all offenders listed), except the tolerated `SUNNYCLOUD_VERSION`.
- A secret key is read from `SUNNYCLOUD_<KEY>_FILE`, else `<secrets_dir>/<key>`; its value never reaches logs, errors or `Debug`.
- Durations are written `2s`, `5m`, `7d` (units `s`, `m`, `h`, `d`; `ms` where the table uses it); floors and ceilings bind only when `environment = production`.
- Release-fixed values (updater source and its two Ed25519 public keys, minimum API family, HTTPS-required flag) cannot be overridden by env or file in release builds.
- Section structs are generated from the key table, so a second spelling of a key cannot exist; a component that adds a key adds its table row in the same change.

## Invariants

- No global state; the crate depends on `serde`, `serde_json` and `thiserror` only (no sqlx, no tokio).
- `load` never panics on malformed input and reports every problem of one start in one error.
- A `Secret` value never appears in any formatted output of the crate.

## Non-Goals

- Not covered: the key rows that later components add; server-side runtime reload (v1.0 restarts); the settings window and the daemon's settings file location (E-25, E-24).

## Versioning Rules

- Adding a key row is a minor change owned by the component that reads it. Renaming or removing a key, or changing a default's unit, is breaking (operators' `.env` files depend on it).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| out-of-range | `retention=91d`, production | `Invalid` naming `retention`, `OutOfRange{0, 90d}` | AC-30, FT-P-17 |
| five-problems | file with 5 bad keys | one `Invalid` with 5 `KeyProblem`s | E-01 AC 2 |
| unknown-var | `SUNNYCLOUD_TYPO=1` | `Invalid` with `Unknown` for that name; `SUNNYCLOUD_VERSION` ignored | |
| test-floor | `upload_session_ttl=10s` with `environment=test` / `production` | accepted / `ForbiddenInRelease` | |
| secret-debug | `Debug` of a section holding `db_api_url` | `***`, no value | NFT-SEC-38 |
| reload-atomic | failing `reload_client_settings` | previous `Arc` still returned | AC-16 |

## Open points (flagged in the producer task)

1. Keys used by components but missing from the helper 01 table: `desktop.wake_gap` (helper 03; also not flat snake case), the sign-in and device-approval limit numbers (compiled AC-20 defaults), `max_inflight_blocks_per_account` (03).
2. Spelling drift against the authoritative table: data_model §1 uses `retention_days`, `tombstone_days`, `job_max_attempts`, `device_user_code_ttl`, `orphan_age`, `max_staged_per_account`; the table has `retention`, `tombstone_horizon`, `job_attempts`, `device_code_ttl`, `orphan_sweep_age`, `max_staged`. The table wins; the docs need a fix by their owners.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
