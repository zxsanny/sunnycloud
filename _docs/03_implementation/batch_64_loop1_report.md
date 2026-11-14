# Batch Report

**Batch**: 64
**Tasks**: AZ-3720_tree_integration_pass (done), AZ-3756_transfer_janitors_reservations (done), AZ-3758_transfer_security_suite (done after two 04 fixes)
**Date**: 2026-11-14

Two parallel workers: 02 (AZ-3720) and 03 (AZ-3756 then AZ-3758). AZ-3758's suite exposed two 04 limits; the owner decided both on 2026-11-14 and a third worker fixed them in 04: read-ahead starts at one chunk and widens only while the consumer keeps pace (AC-3), and the free-space floor check plus file allocation are serialised in the writer (AC-5). The parent allowlisted 03's `janitor_repo.rs` (the reaper lists the accounts with expired rows before any account is known), moved 02's system-scope test helper into `tests/tree/real_account_repo.rs` (allowlisted, the repository-module rule), moved 03's two floor tests into their own binary `tests/volume_floor/` (they measure the shared temporary volume, which other tests of the transfer binary write to in parallel; one parallel run had failed), updated the stale known-failure comments, reviewed, ran the gate and committed. Owner decisions for the next batch are recorded in AZ-3699, AZ-3818, AZ-3822 and AZ-3927.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3720_tree_integration_pass | Done | 02's guard, effect-order, no-dangling-blob, erase and thumbnail cases re-run with the real 01 guard, real 03 effects (through `commit_upload` / `commit_batch`), real 08 media effect, `DerivedStore` and thumbnail source, real 04/05/07a. AC-1..AC-6 PASS. Accounts are created with 01's exact insert plus the real `initialize` hooks (01 creates accounts only inside a provider sign-in). Dev-dependencies: workspace crates plus `bytes` and `futures-util` (already in the lock, declared as in 03 and 08) to build `ByteStream` bodies |
| AZ-3756_transfer_janitors_reservations | Done | `SessionReaper` (account advisory lock, `DELETE … RETURNING`, release exactly the sum, remove blob rows; files discarded and entries evicted after COMMIT; `upload_sessions_total{result=expired}`), `ReceiptJanitor` (`commit_receipt_ttl`, batches with `SKIP LOCKED`), `IdleFlush` (checkpoint and evict idle entries), `ReservationSource`. One expiry rule (`session_repo::is_expired`), also applied at the key: an expired row under a `client_key` or `staged_key` is reaped by the next `start_upload` / `stage_small` (settles the batch 60 warning). `abort_upload` takes the account advisory lock first (removes the batch 61 abort/commit deadlock). AC-1..AC-6 PASS incl. 500 reaper-vs-commit rounds with one winner each |
| AZ-3758_transfer_security_suite | Done | IDOR, protocol abuse, Range abuse, hostile content types, quota/disk DoS at 03's boundary. AC-4 found a GIF89a/HTML polyglot served inline: 03's sniffer now types a binary head carrying markup as `text/html`. AC-3 (1000 abusive range streams on 1 GiB within 64 MiB) and AC-5 (floor under concurrent starts) pass after the 04 fixes |
| 04 fixes (owner, 2026-11-14) | Done | Reader: first chunk verified before `open`; no read-ahead until it is taken; window 1, doubling only when the consumer finds nothing ready, capped at `blob_read_ahead_chunks`. 1000 streams: heap +66.1 MB, RSS +61.4 MB (limit 67.1 MB). Writer: one `Mutex` per `FileBlobWriter` over floor check, folder creation, file create, header and `fallocate`; new `tests/writer_floor` (10 concurrent creates: 3 admitted, 7 `FreeSpaceFloor`, never below the floor; fails 3 of 3 without the lock) |

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3557 passed, 0 failed, 23 ignored (incl. `range_memory` and `volume_floor`). `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no sqlx macros, no migration in this batch.

Warnings (recorded):
- AC-3 passes with 1.4% headroom (one chunk per stream is the floor while the first chunk is verified before `open`).
- The floor lock holds within one writer instance; another writer or process on the same volume can still cross the floor (documented in `blob-writer.md`). Without preallocation (`write_complete` always) the floor check is a snapshot only.
- Lazy load vs abort/reap race (batch 59) and two concurrent `commit_upload` of one upload (batch 61) remain for the race suite.
- `declared_size = 0` on the large path answers `Unexpected` (Low flag, recorded).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | 03's reaper needs a cross-account listing | AZ-3756 | `janitor_repo.rs` allowlisted | clear |
| 2 | Reaper vs `publish` lock order | AZ-3756 | Advisory lock first everywhere | clear |
| 3 | GIF/HTML polyglot served inline | AZ-3758 AC-4 | Sniffer fix + unit test | clear |
| 4 | Read-ahead held ~1.1 MB per stream | AZ-3758 AC-3 | Owner: grow from one chunk (04) | clear |
| 5 | Concurrent starts crossed the floor | AZ-3758 AC-5 | Owner: serialise in 04 | clear |
| 6 | Floor tests share the runner's volume | AZ-3758 | Own test binary `volume_floor` | clear |
| 7 | 01 creates accounts only through a provider sign-in | AZ-3720 | 01's insert + real hooks; recorded in the spec | clear |

## Commit

`[AZ-3720] [AZ-3756] [AZ-3758] Tree real pass, transfer janitors, security` — Loop: 1

## Next Batch

AZ-3759 (03), AZ-3699 (06), AZ-3818 (07), AZ-3822 (07), AZ-3927 (02, 03, 06) with the owner's decisions; split by file ownership.
