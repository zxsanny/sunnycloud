# Batch Report

**Batch**: 33
**Tasks**: AZ-3833_graceful_shutdown
**Date**: 2026-11-11

One worker on `11_http_api` (the owner picked AZ-3833 from the four ready tasks of that crate). No shared crate was edited.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3833_graceful_shutdown | Done; the long-poll half of AC-1 is covered with a stand-in (the changes route does not exist yet) | `sunnycloud-http-api`: `accept.rs` (`CloseSignals`, `AcceptLoop::close_connections(grace)`), `gate.rs` (`ConnectionGate::wait_idle`), `server.rs` (six-step shutdown in `BoundServer::serve`), `services.rs` (`ApiServices.feed_shutdown`); `http_server.md` 1.4.0 | 205 `it` + 57 unit (default features), 207 `it` + 56 unit with `test-clock`; new `tests/it/shutdown/{order,downloads}.rs` | Items 1-7 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the diff (the `expect` calls are in test modules); no file over 500 lines. One existing test was removed because idle connections no longer survive `run`, and the harness `shutdown_grace` default went from 25 s to 1 s (item 5). Warnings: the long-poll wake is exercised through worker `claim` calls held in a stub, not real feed polls (item 1); the ops listener is not scrapeable during the grace (item 2).

CI-parity: all gates passed except `cargo deny` (item 8). In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. Not run: `check-migrations.sh` (needs `sqlx-cli`). The first full run had three failures in `04_blobstore`, a crate this batch did not touch (item 7); they passed alone and the whole test step was re-run green.

## Test Suite

- Total: 2628 (workspace, default features, real PostgreSQL 18)
- Passed: 2619
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

None asked this batch. Waiting for the owner: items 2, 3, 8, and the wiring in item 6.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | AC-1 long-poll half: 11 has no changes route yet (E-21/c13), so the 200 empty page with its own cursor is not asserted. The order test holds 20 worker `claim` calls (10 over HTTP/1.1, 10 over HTTP/2) on the internal listener in a stub until `feed_shutdown` fires and answers them 204; when the changes route lands, the same rig should hold real polls and assert the 200 empty page | AZ-3833 AC-1 | Stand-in disclosed, not a pass for the real route | unclear |
| 2 | The ops listener stops accepting with the others, as the contract says, so `/readyz` and `/metrics` cannot be scraped on a new connection during the grace; a probe sees a refusal, which also reads as not ready. Keeping ops up through the grace needs a separate stop token. Recorded as `http_server.md` open point 5 | AZ-3833 | Worker's reading | unclear |
| 3 | HTTP/2 abort at grace end is a connection drop, not an explicit RST_STREAM per stream (the client sees a broken pipe); the contract allows "RST_STREAM or connection close". HTTP/1.1 ends with an incomplete body | AZ-3833 AC-2 | Worker's reading | unclear |
| 4 | The grace clock starts after the wake (step 4 of the six steps), not at the signal; steps 1 to 3 take microseconds. If the wake wins a race against a request that has not yet reached 05's hold, 05's cancelled token returns the empty page at once (per the 05 contract; not tested here) | AZ-3833 AC-3 | Worker's reading | unclear |
| 5 | Existing tests changed: removed `ops::the_shutdown_signal_turns_readyz_to_503_on_a_connection_opened_before_it` (relied on idle connections surviving `run`; the 503 is now asserted in the order test); `harness::default_config` sets `shutdown_grace` to 1 s because an abuse test leaves a raw HTTP/2 peer that never closes and would hold `run` for the whole grace; the `ConnectionGate` takes `u32` and `AcceptLoop::new` rejects `max_connections` above 32 bits | AZ-3833 | Worker's reading | clear |
| 6 | Wiring for the composition root (AZ-3789): `ApiServices.feed_shutdown` is a new required field; create one dedicated `CancellationToken`, give a clone to `PgChangeFeed::new` and put the other in `feed_shutdown`; do not cancel it at the process signal and do not share it with the scheduler, clock or anything else (`run` cancels it at step 4); pass `HttpConfig.shutdown_grace` (default 25 s); run scheduler stop, upload checkpoints, group sync and pool close after `run` returns | AZ-3833 | For AZ-3789 | clear |
| 7 | The first full test run failed three tests in `04_blobstore` (`health::free_bytes_follow_the_filesystem_and_the_floor_decides_below_floor`, `races::two_hundred_races_between_the_scrub_and_the_collector_report_nothing`, `termination::a_run_ends_while_blobs_keep_arriving_and_reports_exactly_the_damaged_ones`; the scrub ones panicked in `create_file`). They passed alone (4 of 4 and 15 of 15) and the complete test step passed on a re-run. Likely the machine under load or `/tmp` space pressure (the free-bytes test compares the filesystem free space within 1 MiB while other tests write to it); the cause is not confirmed | gate | Reported; crate untouched, no change | unclear |
| 8 | `cargo deny check` fails with RUSTSEC-2026-0459 (`libsqlite3-sys` through `rusqlite` in `sunnycloud-daemon`); reported in batch 31 and 32 | gate | Owner decision | unclear |
| 9 | Items open from batches 18-32 stay open | earlier batches | | unclear |

## Commit

`[AZ-3833] Graceful connection shutdown` — Loop: 1

## Next Batch

Recompute ready tasks. Still only `11_http_api` tasks (one at a time): AZ-3830 share and media handlers (3 points), AZ-3828 identity admin handlers and AZ-3829 tree, transfer and feed handlers (5 points each, which need the complexity-budget question first). 254 tasks left.
