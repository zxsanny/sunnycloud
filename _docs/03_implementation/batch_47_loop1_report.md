# Batch Report

**Batch**: 47
**Tasks**: AZ-3708_tree_folder_mutations (done), AZ-3834_csrf_idor_matrices (done), AZ-3908_token_store_device_login (partly, stays in todo), AZ-3747_blobstore_perf_security (partly, stays in todo); new follow-up AZ-4231
**Date**: 2026-11-12

Four parallel workers on disjoint crates (`02_tree`, `11_http_api` tests, `desktop/daemon`, `04_blobstore`); only the daemon worker changed `Cargo.lock`. The owner chose option A of the complexity budget for the four five-point tasks (AZ-3708 recursive cycle check with no new table; AZ-3747 security suite now, performance informational; AZ-3834 generators in 11's test suite; AZ-3916 stage-and-tell for updates, next batch). The parent unblocked the tree crate (workspace dependencies, `TreeServiceError::AccountNotActive` in contracts with 11's mapping), ran the review, the gates and the commit.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3708_tree_folder_mutations | Done | `PgTreeService::{create_folder, update_node, delete_node}` (inherent methods; the `TreeService` trait comes with AZ-3716 once the path and revision queries exist). Guard first, stamp last, notify after COMMIT; a move takes the account advisory lock and one recursive parent-chain query (30 levels under 100 ms); `run_retrying` retries a deadlock once, also inside the stamp. AC-1..AC-9 on PostgreSQL 18 with the real 05 stamp and notifier, including 200 racing create/delete pairs. Review fix: an empty update or a same-name rename is a no-op with no write and no stamp. 71 integration tests |
| AZ-3834_csrf_idor_matrices | Done | Generators read the OpenAPI document: 161 CSRF cases (23 operations x 7 header sets), 69 controls, 34 GET/HEAD reads with the provider callback exempted by name, 49 two-account pairs plus 11 body-account cases, 14 admin cases, and an uncovered-operation check that fails CI. One runner for the stub server or a base URL (the real-stack run is `#[ignore]`d for E-21/c20). No operation leaked and no CSRF case failed |
| AZ-3908_token_store_device_login | **Partly, stays in todo** | Keyring 4.2.0 store and the Linux file fallback with the one WARN line, measured contexts in isolated `dbus-run-session` homes; the RFC 8628 login state machine (5/5/10/10 s poll gaps after `slow_down`), denial, expiry, `http://` refused, revocation and logout. Open: `expires_at` (sync-core's grant has no `expires_in`), the end-to-end login (wire mismatch) and the locked-keyring sign-out (needs a fallible `bearer()` in sync-core) all move to AZ-4231 |
| AZ-3747_blobstore_perf_security | **Partly, stays in todo** | Security suite ST-04-1..7 passes (stolen disk on a plain directory, keys, active attacks, traversal with an inotify path recorder, cross-account and roles, key hygiene); the performance suite PT-04-1..4 is built and ran once as information (gate `NOT-RUN(HW-REF)`). Open: ST-04-8 (below), the ST-04-1 whole-image scan (needs a loop-mounted XFS image) and the PT gates on REF-SRV. The pre-existing clippy lint in `gc_sweeper` is fixed (`Probe::new()`) |
| AZ-4231_device_login_wire (new) | Spec written, Jira AZ-4231 | sync-core follows the server's grant and poll answers, the server adds `verification_uri` and keeps `expires_in`, and a host can report `CredentialUnavailable` so an unreadable keyring pauses sync instead of signing out (owner decisions 2026-11-12) |

## Security finding (owner decision: fix in the next batch)

`write_block` still re-seals a block index written again with different bytes under the same nonce: the nonce reuse the 2026-11-11 rule forbids (identical bytes accepted after decrypt-and-compare, different bytes refused with `BlockConflict`). `contracts::BlobError` has no `BlockConflict`. A torn, undecryptable region after a crash needs a careful rule. ST-04-8 waits for this change.

## Informational performance numbers (AZ-3747; Ryzen 9 3900XT, ext4 on NVMe, release build; not a gate result)

PT-04-1 4 KiB range reads p95 0.23 ms warm, 0.43 ms cold; PT-04-2 5 GiB write 1549 MB/s, read 527 MB/s; PT-04-3 400 writers at 120 MB/s offered for 60 s: `await_durable` p50 1.4 ms, p95 4.4 ms, p99 6.0 ms; PT-04-4 writes keep 95.4% of baseline under scrub.

## Code Review Verdict: PASS_WITH_WARNINGS

Two findings, owner-approved handling: the empty `update_node` no-op (fixed); the locked-keyring sign-out (moved into AZ-4231, because `CredentialProvider::bearer()` cannot fail and sync-core's `renew` stops authentication after a 401).

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, wasm build and the four check scripts exit 0; workspace tests 2919 passed, 8 failed, 19 ignored. All eight failures were the crate-graph check (`server/tests/crate_graph.rs`) rejecting the tree crate's dev-dependency on `sunnycloud-sync-feed` (the AZ-3708 spec asks for the real 05 stamp in the tree tests, ADR-015's check forbade every component edge). Owner decision 2026-11-12: dev-dependencies between components are allowed; normal and build edges stay forbidden. The check and ADR-015 were changed, a test for the accepted dev edge added; the crate-graph tests then passed 11 of 11, fmt and the server clippy are clean.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | The dependency table listed AZ-3708 as ready, but the tree crate had no name, clock or id dependency and contracts had no `AccountNotActive` on `TreeServiceError` | AZ-3708 | Parent added the workspace dependencies (lockfile names only) and the contract variant with 11's mapping | clear |
| 2 | Renaming or moving a tombstoned node answers `NotFound`; create and move check only the direct parent for a tombstone (the cascade handles ancestors) | AZ-3708 | As implemented | unclear |
| 3 | OpenAPI gaps: `x-blake3` (put block), `cursor` (changes), `user_code` (device-grant lookup) are plain strings without a pattern, so a request built from the schema is refused | AZ-3834 | Named known gaps in the generators; follow-up: add patterns to the route registry | unclear |
| 4 | Uniform but non-404 answers for foreign ids: upload ids 410, commit/delete with another account's base revision 409, `commits:batch` 200 with a failed entry, `DELETE /sessions/{id}` 204, `GET /shares?node_id=` empty 200, staged keys 200 | AZ-3834 AC-4 | Listed in `UNIFORM_NOT_404`; each answers the same as a random id | unclear |
| 5 | `contracts::MasterKey` does not zeroize on drop | AZ-3747 AC-9 | Contracts owner follow-up | unclear |
| 6 | Key 1 (the master key file) can never leave the ring; AT-04-1 step 2 cannot be expressed as specified | AZ-3747 | Tested with key 2 removed | unclear |
| 7 | No `SyncApi` call returns the account id the token store keys by | AZ-3908 | `Session` takes it as input; the caller has no source yet | unclear |
| 8 | `LoginState` and `ControlError` have no store-failure variant | AZ-3908 | A failed save ends signed-out with an ERROR log | unclear |
| 9 | The 39 lockfile packages of `keyring` 4.2.0 (zbus, secret-service, async runtime pieces) | AZ-3908 | As required by the 14 description | clear |

## Commit

`[AZ-3708] [AZ-3834] [AZ-3908] [AZ-3747] Tree mutations, CSRF/IDOR matrices, desktop token store, blobstore suites` — Loop: 1

## Next Batch

The block-rewrite rule (contracts `BlockConflict`, `write_block`, ST-04-8, owner: next batch), AZ-3916 (updater, stage-and-tell) and AZ-4231 (device login wire).
