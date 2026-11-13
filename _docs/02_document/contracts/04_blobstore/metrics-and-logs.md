# Contract: metrics-and-logs

**Component**: 04_blobstore
**Producer task**: E-10/c15 — `_docs/02_tasks/todo/AZ-3748_blobstore_metrics_logging.md`
**Consumer tasks**: E-09 observability (the shared registry and the JSON format), E-21/c16 (`/metrics` route and the composition root that wires the handle), E-21 alert rules (names below)
**Version**: 1.0.0
**Status**: draft (frozen when E-10/c15 is accepted)
**Last Updated**: 2026-11-08

## Purpose

The blob I/O series and the log events of 04. Names, types and the logging levels are those of `deployment/observability.md` and of the description's § 14; this contract states how 04 feeds them. The scrub series are `ScrubMetrics` (E-10/c12) and are not repeated. Source: `components/04_blobstore/src/metrics.rs`.

## Shape

`BlobMetrics::register(&Metrics, Arc<dyn Clock>) -> Result<Arc<BlobMetrics>, ObsError>` registers the four families below (`Duplicate` when the registry already holds them). Each owner takes the handle with a builder; a process without a registry omits it and records nothing.

| Series (exported with the prefix `sunnycloud_`) | Type | Labels | Fed by | One observation or update is |
|---|---|---|---|---|
| `block_fsync_duration_seconds` | histogram | none | `SyncBarrier::with_metrics` | one group flush (the `fdatasync` of the group's files), failed flushes included; the coalesce wait is not counted |
| `blob_read_duration_seconds` | histogram | `kind` = `range` (`open` with a byte range) or `stream` (`open` of the whole blob) | `FileBlobReader::with_metrics` | one successful `open`: from the call to the return of the stream, i.e. until the first chunk of the range is read and authenticated. Not the time the consumer takes to drain the stream. A failed `open` and `read_head` add nothing |
| `blob_free_bytes` | gauge | none | `BlobHealth::with_metrics` | set by each `probe` to the probe's `free_bytes` |
| `blob_free_floor_bytes` | gauge | none | `BlobHealth::with_metrics` | set by each `probe` to the free-space floor (the smaller of `blob_free_reserve` and `blob_free_reserve_percent` of the volume) |

- The gauges change only when `probe` runs; the caller that exposes `/metrics` (E-21/c16) must probe on a schedule or before a scrape.
- Durations are read from the injected clock's monotonic time; the histogram buckets are the registry's one set.
- A metric update takes no lock of its own on the write or read path; the labelled read histogram takes the registry's read lock on its child map.

## Log events

JSON on stdout through `sc_observability`. A line carries `err` (the error and its causes, whose faults hold the blob id, the account id when known, and the failure class) and, for a cycle summary, `duration_ms`. No other field, no file name, no path, no key material, and no line per block or chunk.

| Level | Event | Message |
|---|---|---|
| ERROR | corrupt chunk on read (once per failed read) | `blob corrupt on read` |
| ERROR | key unwrap failure | `data key unwrap failed` |
| ERROR | I/O error on a write job (block write, file creation) | `blob write failed` |
| ERROR | failed group sync (one line per failed flush, not per waiter) | `blob group sync failed` |
| ERROR | aborted sweep | `gc sweep aborted`, `orphan sweep aborted`, `scrub cycle failed` |
| WARN | free-space floor hit on file creation | `blob volume at its free-space floor` |
| WARN | orphan found in report mode | `orphan blob file` |
| WARN | GC unlink of a file already missing | `gc blob file already missing` |
| WARN | scrub paused until the cycle budget ended | `scrub paused until the cycle budget ended` |
| INFO | GC, orphan and scrub cycle start and summary | `gc|orphan|scrub cycle started`, `... cycle finished` |
| INFO | key rotation batch | `rewrap batch finished` |
| INFO | startup key check passed | `startup key check passed` |

The crate test `tests/log_events` fails when a log call is added, removed or given a field outside this table.

## Non-Goals

- Not covered: the scrub series, `blob_reserved_bytes`, the `/metrics` route, the log format and redaction (E-09).

## Versioning Rules

- **Breaking changes** (a metric renamed or relabelled, a level changed, what one observation covers) require a new major version and notice to E-21 and the alert-rule owners.
- **Non-breaking additions** (a new series, a new supporting log line) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| flush-count | 100 writes over 10 files, hook counts flushes | histogram count = flushes (at most 25) | `tests/metrics/flush.rs` |
| flush-failed | one failed flush | count 1 | |
| free-gauges | reserve 8 MiB, probe | gauges = probe's `free_bytes` and 8 MiB | `tests/metrics/volume.rs` |
| read-count | 10 opens (5 with a range) and one failed open | `range` 5, `stream` 5 | `tests/metrics/read.rs` |
| io-events | corrupt read, failed block write, failed sync, floor, aborted GC sweep | one blobstore line each, at its level, with the blob id | `tests/blob_logs` |
| no-per-block | 2 MiB and 64 MiB read | equal line counts | `tests/blob_logs` |
| table | whole crate | every table event present, no other line | `tests/log_events` |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-08 | Initial contract (AZ-3748) | implement |
