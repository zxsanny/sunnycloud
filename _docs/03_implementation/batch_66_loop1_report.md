# Batch Report

**Batch**: 66
**Tasks**: AZ-3701_share_abuse_limits (done), AZ-3757_transfer_erase_and_crash_cases (done), AZ-3762_transfer_docs (done)
**Date**: 2026-11-14

Two parallel workers: 06 (AZ-3701) and 03 (AZ-3757 then AZ-3762). AZ-3927 (vault membership across 02, 03 and 06) was moved to the next batch so these could run in parallel. The parent passed the new limiter to `PgShareService::new` in 07's dev-seed test (06's constructor gained it), converted 03's receipt-janitor delete to the materialised form (see discovery 1), reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3701_share_abuse_limits | Done | `sc_ratelimit` counters in 06: invalid tokens per IP (checked before the lookup on `open`, `authorize_content` and `verify_password`; a limited IP gets `RateLimited` on every token so the limit confirms nothing; valid tokens never count) and wrong passwords per link (a token reserved before the PHC read and Argon2, cleared on the right password, so 50 parallel attempts from 50 IPs run exactly 10 checks). One best-effort `abuse_trip` audit event and one WARN per trip, no token or password. AC-1..AC-5 PASS; AZ-3698 constant-work tests still pass |
| AZ-3757_transfer_erase_and_crash_cases | Done | `PgTransferLifecycle`: `erase_batch` deletes sessions, staged rows, committed-key records and receipts in bounded account-scoped transactions, queuing open rows' blobs `AccountDeleted` `Due::Now`, no reservation release (the account's quota goes with the account), entries evicted after COMMIT. AC-1 (30 sessions, 200 staged, 5,000 receipts: `remaining` falls by exactly 100 per call), AC-2, AC-68 (crash before commit; failure after file write), AC-69 (effects before the stamp, no blob I/O in the transaction), AC-70 (commit after the parent moved; parent tombstoned) PASS |
| AZ-3762_transfer_docs | Done | `contracts/03_transfer/client_protocol.md`: the 20 transfer error codes with status, routes, meaning and client action (CI test parses the catalogue against `TransferError`), client retry rules, the one-API-process rule, codes per route vs 11's registry. 03 docs corrected (`edited_since` removed from §4, staged effect name, erase settles no reservation) |

## Parent changes

- 07 `tests/dev_seed/world.rs`: the limiter is shared by sign-in and the share service.
- 03 `janitor_repo.rs`: `DELETE_OLD_RECEIPTS` uses `WITH picked AS MATERIALIZED (…) DELETE … USING picked`.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3616 passed, 0 failed, 28 ignored. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no sqlx macros, no migration in this batch.

Warnings (recorded):
- `UploadError::ClientKeyMismatch` has no wire code: 11 answers it 500 `internal` (E-21/E-14).
- `UploadError::Overloaded` carries no delay, so 11 sends 503 without `Retry-After` against `problem_details.md` (contracts change or a default in 11).
- 11's route error lists miss `account_not_active` on `DELETE /uploads/{id}` and `account_not_active` / `invalid_request` on the block route (E-21/c13).
- `sc_ratelimit` has no refund, so "only wrong passwords count" is approximated by reserve + clear-on-success.
- The share abuse WARN line carries an unkeyed SHA-256 prefix of the IP (a pseudonym, reversible over IPv4); the audit row keeps the raw IP as 01 does.
- FT-P-67 step 2 still says "<= 10 MiB" (AC-50 is 16 MiB).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | `DELETE … WHERE id IN (SELECT … ORDER BY … LIMIT n FOR UPDATE)` deleted 231 rows with LIMIT 100 (equal sort keys let a rescan pick another set); it also affected the reaper | AZ-3757 | 03's session and receipt deletes now materialise the picked set (as 08's); the rest of the codebase checked: only 20's vault delete uses `IN (…LIMIT)`, ordered by a unique id with at most one row per account | clear |
| 2 | `verify_password` with an unknown token counts on the per-IP counter | AZ-3701 flag (Low) | Proposal: yes (stricter); awaits E-16 owner | unclear |
| 3 | Committed-key record's blob belongs to its revision | AZ-3757 | Not queued by 03; 02's erase owns it | clear |
| 4 | Composition must register `PgTransferLifecycle` | AZ-3757 | AZ-3789 / E-22 | clear |
| 5 | Path for client/operator protocol docs | AZ-3762 flag (Low) | Proposal `contracts/<component>/client_protocol.md`; operator rules stay in `deployment/` | unclear |
| 6 | Release compose file does not exist | AZ-3762 AC-3 | Reported to E-00 | clear |

## Commit

`[AZ-3701] [AZ-3757] [AZ-3762] Share abuse limits, transfer erase, client docs` — Loop: 1

## Next Batch

AZ-3927 (02, 03, 06; owner: one task, `vault_file_too_large` 413), then the cumulative review of batches 64-67.
