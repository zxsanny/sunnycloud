# sc_config: typed, validated settings loader

**Task**: AZ-3763_sc_config
**Name**: sc_config loader, key table, durations, secrets, validation, client reload
**Description**: One loader that binds defaults, a config file, `SUNNYCLOUD_*` variables and secret files into immutable typed sections, refusing to start with every problem listed at once
**Complexity**: 5 points
**Dependencies**: AZ-3652_initial_structure
**Component**: shared/config (helper 01)
**Tracker**: AZ-3763
**Epic**: AZ-3623 (E-01)
**Ref**: E-01/c01

## Problem

Components need about 150 tunables (retention, TTLs, leases, limits). Without one owner a key gets two spellings, a secret leaks into a log and a typo in an environment variable silently does nothing. Tests depend on short durations, while a below-floor value in production must refuse to start (RESTRICT-32, FT-P-17).

## Outcome

- Every binary (`sunnycloud`, `sunnycloud-worker`, `decoder-runner`) and client gets its typed section from one closed key table, or a start failure that lists every problem.
- Durations use one code path: production defaults and floors, short test values (`environment = test`).
- Secrets come from files, are printed as `***` everywhere and never reach logs or errors.
- Clients can apply a settings change atomically or keep the old settings.

## Scope

### Included
- Binding order: compiled defaults, config file, environment, secret files; later wins.
- The closed key table, row for row as helper 01 (all groups), with default, type, range, secret flag and reading binaries; section types generated from it.
- Unknown `SUNNYCLOUD_*` variables refuse start with every offender listed; `SUNNYCLOUD_VERSION` is tolerated.
- Duration settings (`2s`, `5m`, `7d`) with floors and ceilings that bind only in production; `ForbiddenInRelease` below a floor.
- `Secret<T>`; secrets from `SUNNYCLOUD_<KEY>_FILE` or the default secrets folder; `MissingSecret` for an absent required secret.
- Cross-field validation hooks per section (retention 0 to 90 days, at least one sign-in provider, `public_url` https outside development).
- Release-fixed values (updater source and keys, minimum API family, HTTPS-required flag) that env or file cannot override in release builds.
- `reload_client_settings` with an atomic swap; the previous settings stay active on error.
- Logging of the active `environment` at load (helper risk).
- Crate documentation of how a component adds a key row.

### Excluded
- Key rows a later component adds (each component adds its own rows).
- Settings window UI (E-25), daemon settings-file location (E-24), server runtime reload (v1.0 restarts).

## Acceptance Criteria

**AC-1: Out-of-range values refuse start (AC-30)**
Given `environment = production` and `retention = 91d`
When a binary loads its configuration
Then it exits non-zero and the error names `retention` with its allowed range 0 to 90 days (FT-P-17)

**AC-2: All problems at once**
Given a config file with 5 invalid keys
When `load` runs
Then one `Invalid` error carries exactly 5 key problems

**AC-3: Unknown variables refused**
Given `SUNNYCLOUD_TYPO=1` and `SUNNYCLOUD_VERSION=1.0.0` in the environment
When `load` runs
Then start fails naming `SUNNYCLOUD_TYPO` as unknown, and `SUNNYCLOUD_VERSION` is not reported

**AC-4: Secrets never printed (NFT-SEC-38)**
Given a planted secret value in `db_api_url` through `SUNNYCLOUD_DB_API_URL_FILE`
When the section is loaded, formatted with `Debug`, logged and an error about a missing second secret is produced
Then the planted value appears 0 times in the output, and the missing secret yields `MissingSecret` naming its key

**AC-5: Floors only in production**
Given `upload_session_ttl = 10s`
When `load` runs with `environment = test` and with `environment = production`
Then the test load succeeds and the production load fails with `ForbiddenInRelease`

**AC-6: Release-fixed values (AC-49)**
Given a release build and an environment variable and a file entry that set the updater source and keys
When the client settings load
Then the compiled values are used and the override is ignored (NFT-SEC-33 step 3)

**AC-7: Client reload is atomic (AC-16)**
Given active client settings and a new settings document with one invalid value
When `reload_client_settings` runs, then runs again with a valid document
Then the first call returns `Invalid` and the previous settings stay active, and the second call makes the new settings visible to the next reader (FT-P-49)

**AC-8: No provider configured (FT-N-25)**
Given a server configuration with no sign-in provider
When the API section loads
Then start fails with a problem naming the missing provider settings

**AC-9: One name per key**
Given the key table
When the table test runs
Then every row has exactly one environment name (`SUNNYCLOUD_` + upper-cased key) and no two rows share a key

## Non-Functional Requirements

**Performance**
- Loading the full table takes under 50 ms.

**Reliability**
- `load` never panics on malformed input (property test over random environment maps).

**Security**
- No global state; no I/O except reading the named files; dependencies `serde`, `serde_json`, `thiserror` only.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Each duration floor and ceiling in production versus test | Range errors only in production |
| AC-2 | Collection of several problems | All listed in one error |
| AC-3 | Unknown-key listing with several typos | Every offender named |
| AC-4 | `Debug` and `Display` of `Secret`; `_FILE` and default secret path | `***`; both paths read |
| AC-7 | Atomic reload with a concurrent reader | Reader sees old or new, never a mix |
| AC-9 | Table row uniqueness | 0 duplicates, 1 env name each |
| — | Property test: random environment maps | No panic |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Server with `retention=91d` | FT-P-17 | Refuses start, names the key | — |
| AC-8 | No provider configured | FT-N-25 | Refuses start, names the provider setting | — |
| AC-4 | Full suite with planted secrets | NFT-SEC-38 (config part) | 0 secret hits in logs and errors | Security |
| AC-6 | Release desktop build with override | NFT-SEC-33 step 3 | Override ignored | — |
| AC-7 | Desktop settings change | FT-P-49 | Applied without restart | — |

## Constraints

- Keys are flat snake case; the env name has no alias; helper 01's table is authoritative and must match row for row.
- Compose-only variables (`PUBLIC_HOST`, `BLOB_PATH`, `WORKER_CPUS`, `WORKER_MEM`) are never read by a binary.

## Risks & Mitigation

**Risk 1: About 150 keys at the 5-point cap (E-01 risk 1)**
- *Risk*: hand-written sections and tests per key overrun the task.
- *Mitigation*: rows are data; sections are generated; tests are generic over the table.

**Risk 2: Components forget their key rows**
- *Risk*: a component reads a key that is not in the table.
- *Mitigation*: unknown-key refusal makes the component's own tests fail.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Keys used by components but missing from the table: `desktop.wake_gap` (also not flat snake case), the sign-in and device-approval limit numbers, `max_inflight_blocks_per_account`. | E-01 owner with E-24, E-08, E-14 | open | Medium |
| data_model §1 spells several keys differently (`retention_days`, `tombstone_days`, `job_max_attempts`, `device_user_code_ttl`, `orphan_age`, `max_staged_per_account`); helper 01's table wins and the data model needs a doc fix. | Data-model owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_config/sc_config.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
