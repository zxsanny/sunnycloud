# Server logs from spawned tasks reach the log sink

**Task**: AZ-4318_spawned_task_logs
**Name**: Spawned task logs
**Description**: Make every task `serve` spawns log through the command's subscriber, so request, connection and janitor log lines (errors included) are not lost
**Complexity**: 2 points
**Dependencies**: AZ-3791_serve_start_stop
**Component**: 12 operator_cli (`src/server`); helper `sc_observability` only if a spawn wrapper is needed
**Tracker**: AZ-4318
**Epic**: AZ-3647 (E-22)

## Problem

Found in AZ-3791 (batch 73). `src/server/src/run.rs` attaches the log subscriber to the command future with `WithSubscriber::with_subscriber`. A subscriber attached that way is not inherited by `tokio::spawn`ed tasks, so the HTTP request handlers, hyper connections and janitor loops of `serve` log nowhere: their errors are lost in production. The step lines of `serve` itself appear because they run on the command future.

## Outcome

- Every task `serve` spawns logs through the command's subscriber: `serve` also installs it as the process default, once (a second install in one process refuses with exit 2).
- Every other command, and every in-process run in a test, keeps the per-future subscriber and its own sink; no process-global default outside `serve`.

## Acceptance Criteria

**AC-1: Spawned tasks log**
Given `serve` with `log = json`
When a request handler fails with an internal error and a janitor cycle fails
Then each failure produces one JSON log line on the binary's output (real binary, PostgreSQL 18)

**AC-2: Other commands keep their own sink**
Given an in-process run of a command other than `serve`
When it finishes
Then no process default subscriber was installed, so runs with separate sinks stay separate

## Constraints

- Design (batch 74): a tracing subscriber attached to a future is not inherited by `tokio::spawn`; wrapping the eight production spawn sites in four crates was rejected for the one process-wide install in `run.rs` that only `serve` makes. A future in-process test of `serve` would need its own process.
- No silent drop: a log line that cannot be written follows the existing sink's error rule.
- Never log secrets or PII (existing redaction tests stay green).

## Implementation status (2026-11-15)

| AC | Status | Evidence |
|----|--------|----------|
| AC-1 | Done | `src/server/tests/serve_logs.rs::a_failing_request_and_a_failing_janitor_cycle_each_leave_a_json_log_line` (real binary: failing `POST /api/v1/uploads` gives one `unexpected failure answered as internal` JSON line, a failing session-reaper cycle gives `janitor ... cycle failed`; red without the change) |
| AC-2 | Done | `src/server/src/run.rs::tests::a_command_other_than_serve_installs_no_process_default`; the existing in-process CLI suites keep passing with separate sinks |
