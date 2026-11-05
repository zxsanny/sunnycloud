# `BackupCore` UniFFI surface: `set_server`, `status`, `cancel` and wire-error mapping

**Task**: AZ-3991_backup_core_surface
**Name**: BackupCore surface and error mapping
**Description**: Export `BackupCore` through UniFFI with `set_server` (https only, server-info check), `status` from the checkpoint, `cancel`, and the mapping of every sync-core error to an asset skip or a run pause so the loop keeps going and the counts always add up
**Complexity**: 5 points
**Dependencies**: AZ-3990_run_backup_loop, AZ-3958_sync_typed_errors, AZ-3959_http_sync_client_connect, AZ-3967_sync_host_traits_foundation, AZ-3968_sync_uniffi_surface, AZ-3742_test_harness_skeleton
**Component**: 17 android_app (crate `mobile_backup`)
**Tracker**: AZ-3991
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `HttpSyncClient::connect`, `ConnectError`, typed errors and the wire-code table, `cancel()` (E-23/c06, c07, c14).
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — the `uniffi` feature, derives and foreign traits (E-23/c15).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — produced by this task.

## Problem

Kotlin and Swift can only reach the backup through one exported object, and every value that crosses must be a typed record or enum (ADR-013). AC-54 requires the app to refuse plain-HTTP addresses before any request; the address must be checked against `server-info` before sign-in (spec-gaps §1, `set_server`). Server answers such as 507, 413, a rejected name, an inactive account or 426 must each end in the right asset state or pause, without stopping the loop for per-asset errors, and the status must always satisfy `visible = uploaded + pending + skipped` (AC-64, FT-N-50).

## Outcome

- `BackupCore` exported through UniFFI with the methods and types of `backup_core.md`; Kotlin bindings generated without manual edits.
- `set_server(url) -> ServerCheck` with https-only (debug-only feature allows http), reachability and `GET /api/v1/server-info` through sync-core.
- `status()` read from the checkpoint with counts, skipped counts by reason, pause reason, last error with correlation id, access level and `location_redacted`.
- `cancel()` stops the run between blocks and cancels in-flight sync-core work.
- The wire-error table of `backup_core.md` applied inside `run_backup`.

## Scope

### Included
- UniFFI exports of `BackupCore`, `RunContext`, `BackupOutcome`, `PauseReason`, `BackupStatus`, `AccessLevel`, `ServerCheck`, `BackupError`, the `AssetSource` foreign trait.
- Construction from host-supplied inputs (recorded in `backup_core.md` open point 1).
- `set_server`: scheme check before any socket, `server-info` validation through sync-core, typed `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}`; the `trust` field (filled by E-27/c12).
- `status`: synchronous read; the sum rule; `last_error` with message and correlation id; `location_redacted` set when an original could only be read location-redacted.
- Mapping: 507 → `Paused(QuotaExceeded)`; `SizeTooLarge` → `skipped_size`; `InvalidName` → `skipped_name`; `AssetGone` → `skipped_gone`; `AccountNotActive` → `Paused(AccountNotActive)`; 426 → `Paused(UpgradeRequired)` with the server text; network policy not met → `Paused(NoNetwork)` or `Paused(MeteredNetwork)`; `Denied` access → `Paused(PermissionDenied)`; `TokenRejected` returned as an error.
- `cancel()` flag and sync-core `cancel()`.

### Excluded
- `ParentNotFound` and `ParentGone` recovery (E-27/c13).
- `exchange_code`, `refresh`, `sign_out` (E-27/c08); `rebuild_checkpoint` (E-27/c14).
- The server-address screen and its emulator check (E-27/c15).
- The Kotlin binding build in CI and the drift check (E-27/c18).

## Acceptance Criteria

**AC-1: Plain HTTP refused before any socket (AC-54, IT-17-6 at the `BackupCore` boundary)**
Given a release-profile build
When `set_server("http://sunnycloud.test:8080")` is called
Then it returns `HttpsRequired`, 0 sockets are opened, and a debug-profile build with the plain-HTTP feature accepts the same address

**AC-2: Server check**
Given an https server that answers `server-info`, one that is unreachable, and one whose `min_client_version` is above the app's version
When `set_server` is called for each
Then the first returns a `ServerCheck`, the second `ServerUnreachable`, the third `UpgradeRequired` with the server text

**AC-3: Wire errors map to states and pauses (IT-17-15, AC-64)**
Given a run over assets whose uploads meet, one each, 507, `SizeTooLarge`, `InvalidName`, an asset deleted between listing and upload, `AccountNotActive` and 426, plus a run on a metered network with Wi-Fi only on
When each case is processed
Then 507 gives `Paused(QuotaExceeded)` and is retried on the next run; `SizeTooLarge`, `InvalidName` and the deleted asset give `skipped_size`, `skipped_name` and `skipped_gone` and the loop continues with the next asset; `AccountNotActive` gives `Paused(AccountNotActive)`; 426 gives `Paused(UpgradeRequired)` with the server text in `last_error`; the metered run gives `Paused(MeteredNetwork)`

**AC-4: Counts always add up (FT-N-50, component part)**
Given 100 assets of which 5 vanish before upload, followed by a quota that is full after 10 more uploads
When `status` is read after every asset of both runs
Then `visible = uploaded + pending + skipped.gone + skipped.name + skipped.size + skipped.failed` every time, `skipped.gone` is 5, and after the quota is raised the next run uploads the rest

**AC-5: Cancel**
Given a run with three uploads in flight
When `cancel()` is called
Then the run returns `Partial{remaining}` within 1 s of the in-flight operations ending `Cancelled`, and nothing is sent afterwards

**AC-6: No token crosses the surface**
Given a signed-in core with sentinel tokens
When every method's return value, `status`, every error value and the log output are captured during a run
Then the sentinels occur 0 times

## Non-Functional Requirements

**Compatibility**
- UniFFI 0.32.2 (workspace lockfile); Kotlin bindings generated from the Rust sources only.

**Reliability**
- No error is dropped: every failure is a typed outcome or ends in `last_error` with a correlation id.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Scheme check in release and debug profiles | `HttpsRequired` with 0 sockets; debug accepts |
| AC-3 | Mapping table with a fake `SyncApi` returning each typed error and a fake source raising `AssetGone` | Each row of the table |
| AC-4 | Sum rule after every asset over a mixed run | Holds every time |
| AC-5 | Cancel with three operations in flight | `Partial` after `Cancelled`; nothing sent after |
| AC-6 | Sentinel scan of outputs and logs | 0 matches |

## Blackbox Tests

Native test binary of `mobile_backup` with sync-core against the real server container behind the test proxy (fault scripts per asset; account quota set through `e2e-ctl`); the emulator leg of the same faults is AZ-3673 AC-4 and system scenario FT-N-50.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Real server; unreachable address; high `min_client_version` | `set_server` | `ServerCheck`; `ServerUnreachable`; `UpgradeRequired` | AC-64 |
| AC-3 | Proxy faults 507, 413, name rejection, 403 inactive, 426; deleted asset | Run | Table outcomes; loop continues on per-asset errors | IT-17-15, AC-64 |
| AC-4 | 100 assets, 5 vanish; quota full, then raised | Two runs | 95 then all remaining uploaded; counts add up | FT-N-50 |

## Constraints

- Errors map in exactly one table here; sync-core keeps the one wire-code table (`sync_api.md`).
- Kotlin is the only caller; Flutter never calls Rust (S4-4).

## Risks & Mitigation

**Risk 1: Unmapped error stops backup silently**
- *Risk*: a new sync-core error has no row and the loop ends with no visible reason.
- *Mitigation*: an unmapped error ends the asset as `failed` with `last_error` and a correlation id (see Flagged concerns for the open rule).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| When an asset moves to `failed` and whether it is retried later is not stated in 17 | E-27 owner | open | Medium |
| 13's `FreeSpaceFloor` (507, server disk floor) has no `PauseReason`; 17 lists only `QuotaExceeded` | E-27 / E-23 owners | open | Medium |
| `PauseReason` values `LimitedAccess`, `ServerRejectedName` and `SignedOut` have no stated trigger | E-27 owner | open | Low |
| `BackupCore` construction inputs are not stated in 17 §2; this task records them in `backup_core.md` | E-27 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/17_android_app/backup_core.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
