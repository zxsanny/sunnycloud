# Cumulative Review — batches 48 to 51 (loop 1)

**Date**: 2026-11-13
**Range**: `2637188..9894dac` (batch 48 `a22b88b`, batch 49 `93b0003`, batch 50 `7e138c9`, batch 51 `9894dac`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review with every finding fixed, decided by the owner or recorded. This pass looks across the four batches for drift no single batch review would see. 115 files changed, about 10,400 lines added; most in the desktop daemon (updater, token store), 02 tree (publish, erase), 08 (enqueue effect) and 11's identity tests.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds for normal and build edges. New component dev edges: tree on quota, blobstore and vault service; feed on tree and quota; media on tree, quota and feed. All allowed since 2026-11-12; the checker's helper bug (a faked normal edge hidden by a real dev edge) was fixed in batch 50 |
| Shared contracts changed | `BlobError::BlockConflict` (batch 48), `SessionTokens` field names (batch 50, `sync_core_proto.md` 1.1.0). No other contract type changed; `TreeCommit`, `CommitEffect`, `AccountLifecycleHook`, `BlobOwnerCleanup` were implemented as written |
| Lockfile growth | No new external package in the range (24 lockfile lines, all workspace path entries). `rcgen` 0.14 and `tokio-rustls` 0.26 in 11's tests match sync-core's declarations |
| One canonical path | Production code: one route hook for the mobile code refusal (`send_anonymous_reading`), one retry wrapper (`run_retrying_each`, used by publish and erase), one enqueue path (`enqueue_in_tx`). Test code: see finding 1 |
| SQL placement | All new SQL is in repository modules (`publish_repo.rs`, `lifecycle_repo.rs`, `vault_repo.rs`, `derived_repo.rs`, `job_repo.rs`); `check-db-gates` passes |
| File length | No source or test file over 500 lines |
| Feature gates absent from release | `test-update-trust` (daemon) is a `compile_error!` outside debug builds; the release trust file pins no key until the owner provides them (leftover `2026-11-12_desktop-updater-release-keys.md`) |
| Gate script | Batch 50 found the local gate script reported success after a failed step; it now fails the run |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | Test support duplicated across crates: five copies of the sqlx statement capture (`statements.rs` in 01, 02, 05 feed, 05 via_tree, 08) and three copies of the boundary-backed `AccountGuard` double (02, 05, 08). A change to the statement event shape or the guard's rule must be made in each | Follow-up task: move both into the shared test helper `sc_testdb` (or a sibling test-support crate) and delete the copies | Medium (maintainability) |
| 2 | `zeroize` is declared as `"1.9"` in the daemon and `{ version = "1.8", default-features = false, features = ["alloc"] }` in 04 and vault-core; the lockfile resolves one version (1.9.1) | Align the declarations in the same follow-up | Low |
| 3 | `erase_batch` retries a deadlock only when it surfaces as `DbError`; one inside `GcQueue::schedule_in_tx` fails the batch until the purger's next tick | Recorded (batch 51) | Low |
| 4 | The 07a lock-wait test can measure a few microseconds below its 400 ms floor under the full parallel run | Recorded (batch 51) | Low |

## Open findings carried forward (batches 45 to 47)

| # | Finding | Status |
|---|---|---|
| 1 | Block rewrite with different bytes under one nonce | Fixed in batch 48 (`BlockConflict`) |
| 2 | Device login wire mismatch, locked keyring sign-out | Fixed in batch 49 (AZ-4231) |
| 3 | OpenAPI parameters without a pattern (`x-blake3`, `cursor`, `user_code`) | Open, route registry follow-up |
| 4 | `contracts::MasterKey` does not zeroize on drop | Open, contracts owner |
| 5 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 6 | `max_vault_header_bytes` default outside `sc_config`'s key table | Open, Low |
