# Batch 122, loop 1: AZ-3813 CLI integration suite (partly done, stays in todo)

## What was built

- `src/server/tests/cli_suite.rs`: the cross-command cases. IT-12-1 matrix over every command of the contract (success and `--json` envelope, owner refusal exit 2 with the owner's code, doctor 0/1/2, usage errors exit 3 with empty stdout, the four destructive commands with and without `--yes`). The single-business-call check reads the contract's own command table: a command with no proof row, or a row naming a test that does not exist, fails (and a unit test shows the check fails on a table with a missing row). The one-time secret test runs four commands in text and `--json` with `log = json`: each token once on its own stdout, never in stderr, log lines or another command's output. The coverage-map check reads `tests.md` and `it12_coverage_map.md`.
- `src/server/tests/it12_coverage_map.md`: 70 rows assign every expected result of IT-12-1..19 to one place (suite, a command task, a pipeline, an owner task, E-31) with a status. One IT-12-11 part is `blocked` (feature `test-migrations` does not exist: G-12-1), two IT-12-18 parts are `known failure`.
- `src/server/tests/it_cases.txt` and CI job `cli-suite`: `scripts/ci/check-it-suite.sh` (now with `IT_SUITE_PACKAGE`) runs the 19 cases with their budgets and the missing-case check. Local run: 19 cases PASS, the slowest 7.8 s (IT-12-8).
- Coverage gate: `critical-paths.txt` lists `exit.rs` (100%), `guard.rs` (93.5%), `secret.rs` (100%), `output.rs` (98.6%) and `serve/start.rs` (91.1%) at 90%. The start-up order was at 86% (three refusal paths untested): three unit tests added in `serve/start.rs`. The test helpers that spawn the binary with a cleared environment now pass `LLVM_PROFILE_FILE` on, otherwise the binary's own coverage was never counted (`serve/http.rs` read 0%).
- `COMMANDS` table moved from `command_tree.rs` to `support/mod.rs` so both files share it.

## AC status

AC-1, AC-2, AC-3, AC-6, AC-7 done. AC-4 is met by the existing `rewrap_sigint.rs` and `scrub_sigint.rs` (stop within 5 s, `finish_run` once, exit 130 recorded); they are listed in IT-12-19 of the case map. AC-5 is NOT met: `migrate` and `openapi` succeed with no provider and no worker token, `admin setup-token` and `key-check` do not (they need the whole server configuration). Two honest tests exist and are `#[ignore]`d with the reason. Leftover: `_process_leftovers/2026-11-24_az-3813-lazy-config.md`. AZ-3813 stays in `todo`.

## Checks

`cargo fmt --check` clean, clippy `-D warnings` on `sunnycloud-server` clean, server tests 331 passed. Six failed here: five `pipeline_db_restore` tests need `pg_dump` (not installed on this host; CI installs it) and `config_binary::serve_lists_three_problems_of_three_sections_in_one_refusal` failed once under the instrumented parallel run (a port its prober dials was taken by another test; it passes 5 of 5 alone). CI script tests for pinning, no-quiet and the IT runner pass.

Next: AZ-3815 (CLI acceptance, performance, security), AZ-3835 AC-1 and AC-4, the media and worker suites.
