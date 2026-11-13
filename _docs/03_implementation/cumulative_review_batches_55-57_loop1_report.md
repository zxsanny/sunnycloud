# Cumulative Review — batches 55 to 57 (loop 1)

**Date**: 2026-11-13
**Range**: `2c87798..9ba4de7` (batch 55 `8a5be14`, batch 56 `182556d`, batch 57 `9ba4de7`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review with every finding fixed, decided by the owner or recorded. This pass looks across the three batches for drift that no single batch review would see. 229 files changed, about 14,000 lines added; most in 03 transfer (start, put_block), 02 tree (lookups, usage deltas, thumb state), 06 sharing (create, list, revoke), 08 media jobs (thumbnail states, derived store), 01 identity (security suite, grant binding) and the shared token helper.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds (`crate_graph` test green). New normal edges are helpers only (03 and 06 on `sc_clock`, `sc_db`, `sc_audit`, `sc_ids`, `sc_names`, `sc_observability`); component edges are dev-only (03 on 02, 04, 07a; 06 on 02; 08 on 04) |
| Shared contracts changed | Seven, each approved by the parent and documented: `DerivedOpener::open_derived_for` and `DerivedBlob` (55), `CreatedShare` Debug redaction (55), `NodeView.thumb_state` (56), `BlobRegistry` takes the account and `BlobWriter::create_file` takes the size (56, breaking, callers updated in the batch), `RevisionView.blob_id` (57) |
| Migrations | Two, both additive and parent-written, each checked up, down, up on a copy of the test template: `upload_sessions.committed_revision_id` (55), `device_grants.looked_up_by_account_id` (57) |
| Lockfile growth | No new external package in the three batches (`argon2` 0.6.0 was already locked through 01) |
| System-scope allowlist | Unchanged |
| Secrets | Share tokens and identity tokens now carry a type prefix and are stored only as hashes; `CreatedShare` no longer prints its URL path; the identity suite scans 700 tokens of 7 kinds in every table and log line |
| One canonical path | One token helper for every kind (`sc_ids::new_token`, prefix from `TokenKind`) and one unprefixed generator for one-time sign-in values; one free-space floor function used by writer, health and doctor; one delta table for every publish outcome |
| File length | No source or test file over 500 lines; `job_repo.rs` (495), `contracts/tests/blobstore.rs` (485), `03 tests/transfer/progress.rs` (484) are close to the cap |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | The boundary `AccountGuard` test double is now copied four times (02, 03, 05, 08 suites) and the statement capture five times; batch 55 added 03's copy | AZ-4250 extended to 03's double; scheduled in batch 58 with no other worker in those test suites | Medium (maintainability) |
| 2 | A tombstoned vault root makes the tombstone purge fail on every run (FK with no delete rule); AZ-3923 creates the first vaults | Owner decision before AZ-3923 starts | High |
| 3 | After a restart `put_block` cannot rebuild a session's write handle (no key port for an `uploading` blob) | AZ-3736 spec | Medium |
| 4 | Services built in batches 55-57 have no production wiring yet (`PgUploadService`, `PgShareService`, `DerivedOpener`, `ThumbnailStateSource`, `DerivedStore`) | AZ-3789 composition root | Low (expected order) |
| 5 | 11 does not yet refuse `declared_size <= 0` and a bad `client_key` as `invalid_request`, and does not show `thumb_state` in listings | `api_v1_surface.md` open items | Low |
| 6 | Argon2 runs inline on the async worker in share create; the visitor `verify_password` path will run it per request | AZ-3698 (share open) must move it to a blocking pool | Low (performance) |
| 7 | `find_revisions` pages by offset; the port has no cursor | `tree_service.md` open item | Low |

## Open findings carried forward

| # | Finding | Status |
|---|---|---|
| 1 | OpenAPI parameters without a pattern | Open, route registry follow-up |
| 2 | `contracts::MasterKey` does not zeroize on drop | Open, contracts owner |
| 3 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 4 | `max_vault_header_bytes` default outside `sc_config` | Open, Low |
| 5 | 07a lock-wait test timing-flaky under load | Open, Low |
| 6 | Generated Kotlin and Swift bindings print token fields | Open, E-27/E-28 |
| 7 | Android JNI code and UniFFI CI legs never run | Leftover `2026-11-13_az-3968-android-legs.md` |
