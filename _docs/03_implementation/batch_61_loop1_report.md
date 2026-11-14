# Batch Report

**Batch**: 61
**Tasks**: AZ-3738_transfer_commit_upload (done), AZ-3698_share_lookup_open_uniform (done), AZ-3715_tree_restore (done), AZ-3719_tree_security_perf_suites (done; PT rows `NOT-RUN(HW-REF)`)
**Date**: 2026-11-14

Owner decisions 2026-11-14 (complexity budget, option A for all four): 03 has its own head sniffer; unknown share tokens take fixed dummy reads and Argon2 runs on the blocking pool; a path restore picks the most recently tombstoned node; tree performance rows stay `NOT-RUN(HW-REF)`. Also decided: AZ-3819 gets a read method on 01's account admin port (batch 62); gate S12 = measure Safari on the owner Mac (kit `_docs/00_research/s12_download_spike/`, leftover `2026-11-14_az-3931-safari-mac-run.md`).

Three parallel workers: 03, 06, and 02 (AZ-3715 then AZ-3719). The parent removed `EditedSince` from `CommitServiceError` and `EntryError` as `commit_service.md` assigns to this task (contracts part 2 1.2.0; 11's mapping, route error list, gate double and tests; 03's dead `TransferError::EditedSince`), added the reviewed allowlist line for 06's token lookup, reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3738_transfer_commit_upload | Done | `commit_upload` (inherent on `PgUploadService`; the `CommitService` impl lands with `commit_batch`, E-14/c09): blocklist check (count, missing -> `NeedBlocks`, hash), roll-up from block hashes, `read_head` <= 64 KiB (asserted: 0 full reads), own sniffer, target choice, one `publish` with `BlobCommitEffect` (locks the open row, marks the blob committed, keeps the row as the committed-key record; `NoChange` drops the unused blob), metrics `commits_total{kind,result}`, `commit_duration_seconds{kind}`. AC-1..AC-6 and AC-6b (record expiry; removal is AZ-3756's reaper) on real 02, 04, 05, 07a |
| AZ-3698_share_lookup_open_uniform | Done | One system-scope read by `token_hash` (`lookup_repo.rs`, the crate's only `run_system`), liveness decided in code, then `resolve_live_file` + `get_node` always run (live target or a fixed nil dummy). The five NotFound cases and three malformed tokens run the same 3 statements (AC-4). `Available` has exactly four fields. Argon2 hashing in share create moved to `spawn_blocking`. `open` is inherent; the `PublicShareService` impl lands with `verify_password` and the authorize methods (E-16/c05, c06) |
| AZ-3715_tree_restore | Done | `PgTreeRestore` (operator, system scope): retention check, then one transaction — account lock, resolve (live child else most recently tombstoned per part), lock node, pick revision, placement (`Original` only when every ancestor is live, else root folder `Restored`), `reinstate_in_tx` (its `FileMissing` and `BeyondRetention` map straight through), re-point the existing revision row, usage delta, stamp last, notify. AC-1..AC-6, each refusal compared by a full row snapshot |
| AZ-3719_tree_security_perf_suites | Done | ST-02-1 (20 calls, B gets for A's ids exactly what random ids give; A unchanged), ST-02-2 (RLS: 0 rows of A, `DISABLE ROW LEVEL SECURITY` denied 42501, janitor sees both), ST-02-3 (hostile names verbatim, traversal `NotFound`, crafted cursors refused), AT-02-1 (exact conflict-copy name). PT-02-1, PT-02-3, PT-02-4 batch row written and `#[ignore = "NOT-RUN(HW-REF)"]` |

## Parent changes

- `contracts`: `EditedSince` removed from `CommitServiceError` and `EntryError`; part 2 1.2.0.
- 11: `CommitServiceError`/`EntryError` mapping, commit route error list (no `edited_since`), the IDOR gate double (an unknown or foreign base revision is ignored, so the commit answers 200 for A's id and for a random one alike), error-case and port-table tests, datapath commit tests.
- 03: dead `TransferError::EditedSince` and its test row removed.
- `scripts/ci/run-system-allowlist.txt`: `src/components/06_sharing/src/lookup_repo.rs` — the visitor token lookup must read `share_links` by `token_hash` before any account is known (AZ-3698, one statement on the unique index).

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3, with the 06 allowlist line), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked` on PostgreSQL 18: 3458 passed, 1 failed, 23 ignored. The one failure was 03's error-table count test (21 -> 20 after the parent removed `TransferError::EditedSince`); fixed and re-run green with fmt and 03 clippy. 11: 344 passed after the contract change. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no sqlx macros, no migration in this batch.

Warnings (recorded, not fixed):
- 06: a live link whose file is under a tombstoned folder with the cascade pending runs 02's ancestor walk (5 statements) yet answers `NotFound`; other NotFound cases run 3. Uniform work here needs a 02 change (always run the walk). Owner decision.
- 03: two concurrent commits of one upload: the second hits 02's unique `revisions.blob_id` and answers `Unexpected` (race suite E-14/c11).
- 03: abort racing commit after 02's insert can deadlock until PostgreSQL cancels one side (E-14/c11).
- 02: a root *file* named `Restored`, or a user's racing `create_folder("Restored")`, fails a Restored-placement restore as `Unexpected`.
- 02: in AC-3 the replaced revision already queued `deleted` keeps that reason (04 merge keeps the earliest due); `replaced` is checked where the node is live (AC-5).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | `EditedSince` still in the commit enums | AZ-3738, commit_service.md | Removed by the parent (above) | clear |
| 2 | Unknown or foreign `base_revision` on commit | AZ-3738 | Ignored -> `Target::New` (as `start_upload`); 11's IDOR expectation now 200 | unclear: owner confirm |
| 3 | Abort or reap before the effect: 02 answers `BlocklistMismatch` (blob row gone) | AZ-3738 | Mapped to `UploadNotFound` for `commit_upload`; recorded in the contract | clear |
| 4 | `commits_total` result values | AZ-3738 | `committed`, `refused`, `failed`; recorded | clear |
| 5 | 06 needs a `run_system` call site | AZ-3698 | Allowlisted by the parent with the reason above | clear |
| 6 | Pending-cascade timing difference | AZ-3698 | Warning above | unclear |
| 7 | `FileMissing` detection | AZ-3715 flag (Low) | Existing `ReinstateOutcome::FileMissing` from 04 | clear |
| 8 | Path restore through tombstoned folders | AZ-3715 flag (Medium) | Owner rule: live child, else most recently tombstoned, per part; in `tree_service.md` 1.2.0 | clear |
| 9 | `Restored` name collisions | AZ-3715 | Warning above; no rule exists | unclear |
| 10 | `admin restore` wiring needs the real `BlobRegistry` and blobstore retention | AZ-3715 | E-22/c10 | clear |
| 11 | PT-02-3 targets need calibration on REF-SRV | AZ-3719 flag | Stays open | unclear |

## Commit

`[AZ-3738] [AZ-3698] [AZ-3715] [AZ-3719] Commit upload, share open, restore, suites` — Loop: 1

## Next Batch

AZ-3819 with the new read method on 01's account admin port; then recompute the ready tasks.
