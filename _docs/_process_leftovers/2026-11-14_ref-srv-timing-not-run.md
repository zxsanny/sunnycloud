# Reference-server timing rows not run (REF-SRV)

**Timestamp**: 2026-11-14
**Blocked action**: run the timing halves of the server performance suites on the reference server (`tests/environment.md`, env `C-REF`) and record the results
**Reason**: REF-SRV does not exist yet; its model and availability are owner items (performance gaps G1, G11; E-31/c01). Timing measured on the developer workstation is never reported as a pass.

## Rows waiting (all `#[ignore = "NOT-RUN(HW-REF)…"]` in the tree)

| Suite | Tests | Task |
|---|---|---|
| 02 tree | PT-02-1 (`reader_listing`), PT-02-3 (`perf_publish`), PT-02-4 cascade and batch rows | AZ-3719 |
| 03 transfer | PT-03-1..4 (`tests/perf/`; harness never executed), PT-03-5 download half (`tests/download_memory.rs`, release only) | AZ-3759 |
| 06 sharing | PT-06-1..3 timing halves (`tests/share_perf/`, print per-metric verdicts and the host line) | AZ-3703, AZ-3704 |
| 04 blobstore | perf ×4 and `reader_gib` (release-only) | earlier 04 tasks |
| 12 operator CLI | PT-12-1 (command latencies with DS-29 500 accounts and the DS-11 tree of 100,000 files; api under 1 GiB during `doctor`), PT-12-2 (10 idle minutes, 20 accounts: CPU, queries per janitor cycle, cycle p95, scheduler connections). Not written yet: two `NOT-RUN(HW-REF)` markers in `tests/consumer/suites/operator_cli/test_operator_cli.py`; they need the `perf` profile of `tests/run.sh` and the DS-29/DS-11 generators | AZ-3815 |

## Replay

On REF-SRV: `cargo test --release -p <crate> --test <binary> -- --ignored --nocapture` for each row; record host line, numbers and verdicts in the task specs and the component `tests.md`; calibrate PT-02-3 targets; then delete this leftover.
