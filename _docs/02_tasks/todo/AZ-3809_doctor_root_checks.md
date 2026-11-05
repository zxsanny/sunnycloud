# Doctor checks that only the composition root can provide

**Task**: AZ-3809_doctor_root_checks
**Name**: Doctor checks for helpers and cross-component reports
**Description**: `DoctorCheck`s for the helpers (configuration, database, clock skew), which carry no `contracts` port, and the two reports that join several components: missing blobs mapped to paths and quota drift
**Complexity**: 3 points
**Dependencies**: AZ-3807_doctor_aggregation, AZ-3763_sc_config, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3765_sc_clock, AZ-3746_blob_maintenance_ports, AZ-3716_tree_revision_lookups, AZ-3756_transfer_janitors_reservations, AZ-3752_quota_recompute, AZ-3690_account_admin_and_guard
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3809
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c14b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `DoctorCheck`, `Finding`.
- `_docs/02_document/contracts/04_blobstore/epoch-marker-and-health.md` — `BlobInventory::find_missing`.
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `describe_blobs`.
- `_docs/02_document/contracts/07a_quota/quota-admin.md` — `QuotaAdmin::recompute`.
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — `LiveUsageSource`, `ReservationSource`.
- `_docs/02_document/contracts/shared_db/sc_db_access.md`, `shared_db/sc_db_migrations.md` — role verification, `check_version`, `status`, `health`.

## Problem

Helpers stay free of `contracts` (module-layout exception 5), so the configuration, database and clock checks need a `DoctorCheck` wrapper in 12. Two reports join components that may not import each other: the missing-blob list needs 04's inventory and 02's paths (AC-73), and quota drift needs 02's live bytes, 03's reservations and 07a's comparison (AC-74). Only the composition root sees all of them.

## Outcome

- Configuration check (tag `pre-flight`): helper 01 validation problems as Fail findings naming each key, never a secret value.
- Database check: unreachable, server version other than 18, an API role that is superuser, has `BYPASSRLS` or owns the schema, and a schema that is not `Current` or has a half-applied (`dirty`) migration each give a Fail naming the remedy (`sunnycloud migrate` for the schema).
- Clock check: skew against the database server above 2 s gives a non-Ok finding.
- Missing-blob report (tag `restore`): `BlobInventory::find_missing` called until every committed blob was covered, each id mapped through `TreeReader::describe_blobs`; one Fail finding per affected revision with path, account and revision id.
- Quota drift report: for every account, `live_bytes` from `LiveUsageSource` and `reserved_bytes` from `ReservationSource` passed to `QuotaAdmin::recompute(..., repair = false)`; any drift gives a non-Ok finding with the account and the stored and computed values; nothing is changed.
- All five checks registered by E-22/c03.

## Scope

### Included
- The five checks above.

### Excluded
- Owners' checks (04 key, volume, retention; 05 epoch marker; 08 worker status; 07a has none of its own), the aggregation and exit codes (E-22/c14a), the inventory, path, usage and recompute logic (owners).

## Acceptance Criteria

**AC-1: Configuration problems (IT-12-15)**
Given a configuration with an unknown key and an out-of-range value, and a planted secret
When `doctor --pre-flight` runs
Then two Fail findings name the two keys, and the secret value appears 0 times

**AC-2: Database role and schema (IT-12-15, NFT-SEC-46)**
Given, in turn, an API role with `BYPASSRLS`, a pending migration, and a healthy database
When `doctor` runs
Then the first two give a Fail finding (role attributes; schema not current, remedy `sunnycloud migrate`), and the healthy database gives Ok

**AC-3: Clock skew**
Given a database clock 3 s ahead of the API host, and then 1 s ahead
When `doctor` runs
Then the first gives a non-Ok clock finding and the second Ok

**AC-4: Missing blobs listed with path, account and revision (AC-73, NFT-RES-37, IT-12-15)**
Given 3 revisions whose blob files were deleted from the volume
When `doctor` runs
Then exactly those 3 revisions are listed as Fail findings with path, account and revision id, and `doctor` exits 2

**AC-5: Quota drift reported, nothing changed (AC-74, NFT-RES-38, IT-12-15)**
Given one account whose stored `used_bytes` the harness raised by 1000
When `doctor` runs
Then a non-Ok finding names that account with stored and computed values, every other account is Ok, and the quota rows are unchanged afterwards

## Non-Functional Requirements

**Performance**
- With 500 accounts and 100,000 revisions the whole `doctor` stays within 60 s (failure 180 s) and the API container under 1 GiB (PT-12-1, measured in E-22/c17).

**Reliability**
- The checks only read; `recompute` is always called with `repair = false`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Mapping of helper 01 problems | One Fail per key; no secret |
| AC-2 | Mapping of database health and `check_version` | Fail per fault; Ok when healthy |
| AC-3 | Skew threshold | Above 2 s non-Ok |
| AC-4 | Paging over `find_missing` and joining with `describe_blobs` (recording ports) | Every committed blob covered; one finding per revision |
| AC-5 | Drift mapping (recording ports) | `repair = false`; finding only on drift |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Bad keys, planted secret | `doctor --pre-flight` (IT-12-15) | Two Fails; 0 secrets | NFT-SEC-38 |
| AC-2 | `BYPASSRLS` role; pending migration | `doctor` (IT-12-15) | Fail each | NFT-SEC-46 |
| AC-3 | DB clock offset 3 s and 1 s | `doctor` | Non-Ok, then Ok | |
| AC-4 | DS-22 restore state, 3 blob files deleted | `doctor` (IT-12-15) | 3 revisions listed with ids; exit 2 | AC-73, NFT-RES-37 |
| AC-5 | `used_bytes` + 1000 on one account | `doctor` (IT-12-15) | Drift finding; rows unchanged | AC-74, NFT-RES-38 |

The user-side part of AC-73 (listings 200, files open as 404 `content_unavailable`) is AZ-3666 (E-22/p13).

## Runtime Completeness

- Named capability: the `doctor` install check of AC-22 and the restore verdict of AC-65, including the redirect-URI listing of `--pre-flight` and the public URL, TLS 1.3, HTTP/2 and proxy-body probe (12 §6).
- Production code that must exist: this task's five checks, plus owner `DoctorCheck`s for providers and redirect URIs and setup state (01) and for the public URL probe (11); those two have no task yet (see Flagged concerns).
- Allowed external stubs: none in the blackbox tests; recording ports only in unit tests.
- Unacceptable substitutes: a hard-coded redirect-URI list, a check that always returns Ok, a missing-blob report without paths, or a drift check that repairs.

## Constraints

- Wiring only: each check calls owner ports or helper functions and maps their answers to findings; no SQL and no blob I/O in 12.

## Risks & Mitigation

**Risk 1: Drift check is slow on many accounts**
- *Risk*: one `recompute` per account plus two source calls exceeds the doctor budget.
- *Mitigation*: PT-12-1 measures it with 500 accounts; `views` and sources answer per account in one round trip.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `describe_blobs` returns node paths per blob id only; AC-73 needs account and revision id too | E-13 owner | open | Medium |
| E-10/c13 also provides a missing-blob `DoctorCheck` with ids only; only one of the two should be registered | E-10 / E-22 owners | open | Medium |
| No E-15 task implements the providers / redirect-URI and setup-state checks, and no E-21 task the public URL, TLS 1.3, HTTP/2 and proxy-body probe check, that 12 §6 assigns to 01 and 11; `doctor --pre-flight` (AC-22) needs the redirect URIs | E-15, E-21 owners (parent) | open | Medium |
| `find_missing(batch)` has no cursor, so how repeated calls cover every committed row is not stated (E-10/c13 flag) | E-10 owner | open | Low |
| The levels of the drift and clock findings (Warn or Fail) are not stated; the ACs require only non-Ok | E-22 owner | open | Low |
| The port that lists every account for the drift check is not named; this task assumes `AccountAdmin::list_accounts` (E-15/c10) | E-22 owner | open | Low |
