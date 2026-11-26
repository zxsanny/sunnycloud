# `doctor` fails `media_worker` for up to 30 s after every worker start

**Timestamp**: 2026-11-26
**Found by**: AZ-3815 stack suite (`tests/consumer/suites/operator_cli/`), batch 124
**Blocked action**: none; the suites wait for a healthy report (`stackkit.wait_worker_ok`). This is a product follow-up.

## What happens

The supervisor reports to the API every `worker_status_interval` (30 s, `src/worker/src/status.rs`). Its first report after
a start comes before the decoder slots pass their gate (`hello` + `self_check`), so `worker_status.decoder_ok` is false
until the next report. During that window `doctor` reports `media_worker: 1 of 1 media workers report a failing decoder
self-check` (Fail, exit 2), although both decoders are ready a few seconds after the start.

A restored database keeps the `worker_status` row of its dump until the worker reports again, which it cannot while
`serve` is down; the restore drill (AT-12-2) therefore dumps only after a healthy report.

## Proposed handling

The supervisor sends a status report as soon as a slot's state changes (gate passed, slot marked unhealthy), on top of
the interval. This does not change a guarantee: `decoder_ok` keeps its meaning and only becomes current sooner. Owner:
E-24 worker (`status.rs`, `decoder_client/slot.rs`). When it lands, drop the waits of `wait_worker_ok` that only cover
this window (keep the restore drill's).
