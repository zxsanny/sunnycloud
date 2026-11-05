# `scrub run|status`

**Task**: AZ-3798_scrub_commands
**Name**: Scrub commands
**Description**: Run a scrub from the CLI with a mode and a rate, show live progress, exit 2 on any mismatch or missing blob, and print the scrub status
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3745_scrubber
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3798
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `Scrubber` (`start_run`, `run_step`, `finish_run`, `status`).
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — command rows (E-22/c02).

## Problem

The restore drill expects `scrub run --full` to exit 0 on a good backup and 2 on any corruption (AC-24, AC-65). A CLI run cannot see the API's activity gauge, so its rate argument is its only throttle. An operator needs progress on a long run, a clean stop on Ctrl-C, and a quick status view.

## Outcome

- `scrub run [--full] [--rate MiB/s]`: one `Scrubber::start_run(mode, rate)` call (`--full` is `Full`, otherwise `Due`; `--rate` defaults to `scrub_max_rate`), then `run_step` repeated until `done`, then one `finish_run`; live progress on stderr every `cli_progress_interval`; exit 2 when any step reported a mismatch or a missing blob, else 0.
- SIGINT stops the step loop and still calls `finish_run`.
- `scrub status`: one `Scrubber::status` call; prints the oldest `last_scrubbed_at`, the unscrubbed count, the open findings and the latest summary; exit 0.
- An invalid `--rate` (zero, negative, not a number) exits 3 with no call.

Narrowed against the plan: scrub against real corruption, with uploads and GC running beside it (AC-80), is AZ-3667 (E-22/p14); the rate cap itself is E-10/c12. This task owns the command side.

## Scope

### Included
- Both commands: parsing, the call sequence, progress, Ctrl-C handling, output and exit mapping.

### Excluded
- Verification, re-checks, audit events and metrics (04, E-10/c12), the scheduled scrub (E-22/c06 runs 04's `ScrubJanitor`).

## Acceptance Criteria

**AC-1: Mode and rate (IT-12-14)**
Given `scrub run --full --rate 5`, and plain `scrub run` with `scrub_max_rate` configured
When each runs against a recording `Scrubber`
Then the first calls `start_run(Full, 5 MiB/s)`, the second `start_run(Due, scrub_max_rate)`, each then calls `run_step` until `done` and `finish_run` exactly once

**AC-2: Exit code from findings (IT-12-14, FT-P-43, NFT-SEC-26)**
Given step results with no mismatch and no missing blob, then results with one mismatch, then results with one missing blob
When `scrub run` runs for each
Then the exit codes are 0, 2 and 2, and the checked, mismatched and missing counts are printed

**AC-3: Live progress**
Given steps that take longer than `cli_progress_interval` (500 ms in tests)
When `scrub run` runs
Then a progress line appears on stderr at least every 500 ms

**AC-4: Ctrl-C closes the run (IT-12-19)**
Given a run in progress
When SIGINT arrives
Then no new step starts after the running one returns, `finish_run` is called once, and the process ends within 5 s

**AC-5: Status (IT-12-14)**
Given a recording `status` answer
When `scrub status` runs, in text and in `--json`
Then the oldest scrub time, the unscrubbed count, the open findings and the latest summary are printed with the same facts in both modes, and the exit code is 0

**AC-6: Invalid rate**
Given `--rate 0`, `--rate -1` and `--rate fast`
When `scrub run` runs
Then each exits 3 with no `start_run` call

## Non-Functional Requirements

**Performance**
- `scrub status`: target 2 s, failure above 10 s (PT-12-1, measured in E-22/c17).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Mode and rate mapping | `Full`/`Due`; default rate |
| AC-2 | Exit from step results | 0 / 2 / 2 |
| AC-4 | Cancellation path | `finish_run` once after SIGINT |
| AC-6 | Rate parsing | Exit 3 on bad values |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Harness, scripted `Scrubber` steps | `scrub run`, `--full --rate 5` (IT-12-14) | Calls as stated; exit 0 / 2 | AC-24, FT-P-43 |
| AC-3 | Harness, slow steps | Progress lines | At least every 500 ms | |
| AC-4 | Harness, blocking step | SIGINT (IT-12-19) | `finish_run` once; ends within 5 s | |
| AC-5 | Harness, recording `status` | `scrub status` (IT-12-14) | Facts printed; exit 0 | |

The owner-effect parts of IT-12-14 (a flipped bit found with its blob id, `--rate 5` taking 3.5 s to 6 s for 20 MiB, no false finding beside 100 uploads and GC, summary in the audit log and metrics) are proven by E-10/c12 and AZ-3667 (E-22/p14) AC-2, AC-3 and AC-80.

## Constraints

- One business operation sequence per run; no blob I/O in 12.

## Risks & Mitigation

**Risk 1: An interrupted run leaves no summary**
- *Risk*: Ctrl-C kills the process before `finish_run`.
- *Mitigation*: AC-4 requires `finish_run` after SIGINT.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-12-14 expects findings listed with the blob id and path, but `StepResult` carries counts only and `Scrubber` has no per-blob findings output; this task prints the counts | E-10 owner | open | Medium |
| 12 §3 lists a "suspect set" in `scrub status`; E-10/c12's `status` returns the oldest time, unscrubbed count, latest summary and the count of unavailable blobs | E-10 / E-22 owners | open | Low |
| Exit code of a run cancelled by SIGINT is not stated (G-12-7 covers `serve` only) | E-22 owner | open | Low |
