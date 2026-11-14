# Batch Report

**Batch**: 63
**Tasks**: AZ-3754_transfer_commit_batch (done), AZ-3755_transfer_download (done except the suffix-range form of AC-1, owner decision), AZ-3700_share_authorize_content (done; AC-6 create/revoke part conflicts with the audit contract, owner decision)
**Date**: 2026-11-14

Owner decisions 2026-11-14 (complexity budget, option A): request hash in the order sent; no suggested name for blob downloads; no share lookup cache. Two parallel workers: 03 (AZ-3754 then AZ-3755) and 06 (AZ-3700). The parent checked the workspace build after the join (both workers touched `Cargo.lock`; workspace-crate edges only, no external package), reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3754_transfer_commit_batch | Done | `commit_batch`: checks before `publish` (no open staged row, duplicate key in the batch, blob not `uploading` or missing -> `UploadNotFound`, still stored in the receipt), one `publish` with `ReceiptEffect` (lookup under the account lock: same BLAKE3 request hash -> replay, other -> `CommitIdReused`; `on_batch` stores every outcome as JSON) and `StagedCommitEffect` (marks the blob committed and deletes the staged row; `NoChange` drops the unused blob). `impl CommitService for PgUploadService`. AC-1..AC-7 incl. 500 entries with 2 typed errors under one seq, identical resend applies nothing, two identical batches at once apply once, receipts per account |
| AZ-3755_transfer_download | Done (AC-1 suffix form open) | `open_revision_content` / `open_blob_content` on `PgUploadService` + `impl DownloadService`: one range (end clamped, start past end unsatisfiable), ETag = blob id, `If-None-Match` before `If-Range` (strong current tag only), `Inline` only for the 7 allowlisted sniffed types, first chunk verified before the answer, later `Corrupt` ends the stream with an error, missing file `ContentUnavailable` (04 marks the blob), non-current revision only with `retained_ok`, `download_bytes_total`. AC-1..AC-6 pass except `bytes=-4096`: `contracts::ByteRange` has no suffix form and 11 drops it, so it is served Full |
| AZ-3700_share_authorize_content | Done (AC-6 create/revoke open) | `authorize_content` (inherent; the trait impl waits for `verify_password` and `authorize_derived`): AZ-3698 lookup, then `resolve_live_file`; dead links read the dummy target (same statements for every NotFound); no cache, so revoke, delete, expiry and edits show on the very next call; `share_accessed` best effort with ids only when `starts_download`. Create and revoke stay fail closed (see below) |

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3520 passed, 1 failed, 23 ignored. The failure was 01 `setup_token::concurrent_setup_redemptions_leave_exactly_one_admin`; reproduced 1 in 20 under load: never a second admin, but one losing racer answered `Unexpected(Io(NotFound))` instead of `InvitationRequired`. Root cause in 04: `FileEpochMarker::write` replaces `.epoch` through the fixed temp name `.epoch.tmp`, so two concurrent writers (feed initialisation of two first sign-ins) race and the second rename finds no temp file. Fixed by the parent: writes on one marker are serialised with a `tokio::sync::Mutex` (one marker per API process; the fixed temp name stays so a stale temp is still overwritten). New test `epoch::concurrent_writes_on_one_marker_all_succeed_and_leave_a_complete_value` (32 concurrent writes) fails without the fix and passes with it; the 01 race test then passed 20 of 20 under load; 04 clippy clean. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no sqlx macros, no migration in this batch.

Owner decisions needed:
- **AZ-3755 AC-1 suffix range** (`Range: bytes=-N`): serving it as Partial needs a `contracts::ByteRange` change (a suffix variant, breaking) plus 11's parser and 03's resolution; today it is served Full. Alternatively drop the suffix case from AC-1.
- **AZ-3700 AC-6**: the spec wants create and revoke audit failures invisible; `sc_audit` (only `share_accessed`, `scrub_mismatch`, `scrub_summary` best effort), 06 §5 and E-07 say fail closed. Kept fail closed and tested.

Warnings (recorded):
- `EntryError` has no `NotFound`: an entry whose base node vanished before `publish` is `EffectFailed(not_found)`.
- `commits_total{kind=batch,result=committed}` counts a batch with failed entries as committed.
- A fully received one-block large session is indistinguishable from a staged row (pre-existing in `session_repo`).
- Commit receipts have no expiry yet (reaper task).

## Parent changes

- 04 `epoch.rs`: serialised `FileEpochMarker::write` (race found by the gate, see CI-parity) and its regression test.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | `commit_receipts` migration already existed | AZ-3754 | No migration | clear |
| 2 | `request_hash` canonical form | AZ-3754 flag (Low) | BLAKE3 over count + per-entry hashes (every field, length-prefixed), order sent, commit id excluded; owner option A | clear |
| 3 | `EntryError` lacks `NotFound` | AZ-3754 | Warning above; contracts change if wanted | unclear |
| 4 | Suffix ranges | AZ-3755 AC-1 | Owner decision above | unclear |
| 5 | `suggested_name` for blob downloads | AZ-3755 flag | Empty; 11 falls back to its own name (owner option A) | clear |
| 6 | Revision downloads need one extra `get_node` for the name | AZ-3755 | Accepted | clear |
| 7 | AC-6 vs audit contract | AZ-3700 | Owner decision above | unclear |
| 8 | No grant verification yet | AZ-3700 | A presented grant counts as absent until E-16/c05 | clear |

## Commit

`[AZ-3754] [AZ-3755] [AZ-3700] Batch commit, download, share content` — Loop: 1

## Next Batch

Cumulative review of batches 61-63, then recompute the ready tasks.
