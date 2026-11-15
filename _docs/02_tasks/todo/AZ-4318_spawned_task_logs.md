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

- Every task `serve` spawns logs through the command's subscriber.
- No process-global default that would break in-process tests running several commands with separate sinks.

## Acceptance Criteria

**AC-1: Spawned tasks log**
Given `serve` with `log = json`
When a request handler fails with an internal error and a janitor cycle fails
Then each failure produces one JSON log line on the binary's output (real binary, PostgreSQL 18)

**AC-2: Separate sinks stay separate**
Given two in-process command runs with their own sinks
When both log from spawned tasks
Then each line reaches only its own run's sink

## Constraints

- No silent drop: a log line that cannot be written follows the existing sink's error rule.
- Never log secrets or PII (existing redaction tests stay green).
