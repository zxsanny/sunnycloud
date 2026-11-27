# Code Review Report

**Batch**: 135, AZ-3878 AC-6 | **Date**: 2026-11-27 | **Verdict**: PASS_WITH_WARNINGS

Scope: `test_it_09_14_kill.py`, `h2cases.py` hold helpers, e2e-ctl `cpu-limit` and its unit test, actions.json and the
stub self-test count, environment.md CPU limit row, README, the D4 record, the AZ-3661 leftover note.

## Findings

| # | Severity | Category | File:Line | Title |
|---|----------|----------|-----------|-------|
| F1 | Low | Maintainability | tests/consumer/suites/worker/test_it_09_14_kill.py | the decode leg's window rests on half a CPU making a 1 s clip's transcode last seconds; a much faster runner could shrink it again |

**F1** (Low / Maintainability): the leg fails loudly ("no transcode ffmpeg was seen") rather than passing for the wrong
reason, and the quota is a named constant.

## Phases

- Spec compliance: every IT-09-14 expected result is asserted (sweep before claim, attempt 2, `succeeded`, one derived
  row per kind and no unreferenced derived blob, empty scratch, no ffmpeg, the killed run's decoder cancelled within
  2 s); "already present" is exercised by the post leg.
- Security quick-scan: `cpu-limit` takes a decoder from a fixed set and a bounded number; the quota is restored by the
  case. No new parameter reaches SQL; the argv list holds only the formatted number.
- Cross-task: one hold client (`h2cases`), one decoder freeze (`process-kill pause`), one clock move (`advance_clock`).
