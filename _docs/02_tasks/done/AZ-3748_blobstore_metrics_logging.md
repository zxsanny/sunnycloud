# Blobstore metrics and logging

**Task**: AZ-3748_blobstore_metrics_logging
**Name**: Blobstore metrics and logs
**Description**: The blob I/O metrics (`block_fsync_duration_seconds`, `blob_read_duration_seconds`, `blob_free_bytes`, `blob_free_floor_bytes`) and the blobstore log levels, with ids only and no per-chunk or per-block lines
**Complexity**: 2 points
**Dependencies**: AZ-3725_master_keyring, AZ-3727_group_commit_barrier, AZ-3728_blob_reader, AZ-3730_gc_sweeper, AZ-3743_orphan_sweeper, AZ-3745_scrubber, AZ-3746_blob_maintenance_ports, AZ-3786_sc_observability
**Component**: 04 blobstore
**Tracker**: AZ-3748
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c15

## Problem

Operators need to see slow syncs (the ext4 stall risk), read latency and free space before the floor is hit, and logs must help without leaking names, paths or key material or flooding with one line per chunk.

## Outcome

- Metrics in the shared registry (prefix `sunnycloud_`): `block_fsync_duration_seconds` (one observation per group flush), `blob_read_duration_seconds`, `blob_free_bytes`, `blob_free_floor_bytes`.
- Log levels as the description's logging table (§14): ERROR for a corrupt chunk on read, a key unwrap failure, an I/O error on write and an aborted sweep; WARN for the free-space floor hit, an orphan found in report mode, a GC unlink of a missing file and a scrub paused too long; INFO for scrub, GC and orphan cycle start and summary, a key rotation batch and the passed startup key check.
- JSON on stdout; fields blob id and account id only.

## Scope

### Included
- Registering and updating the four metrics; a check over the whole crate that every listed log event exists at its level and that no other blobstore line carries names, paths or key material.

### Excluded
- Scrub metrics (set by `finish_run`, E-10/c12); the `/metrics` route (E-09, E-21/c16).

## Acceptance Criteria

**AC-1: Flush histogram counts group flushes**
Given 100 concurrent block writes over 10 files
When they are all durable
Then the `block_fsync_duration_seconds` count rose by the number of group flushes the barrier hook counted (at most 25)

**AC-2: Free-space gauges**
Given a volume and `blob_free_reserve = 8 MiB`
When the health probe runs
Then `blob_free_bytes` equals the probe's `free_bytes` and `blob_free_floor_bytes` equals 8 MiB

**AC-3: Read duration recorded**
Given 10 opens of a blob
When the metrics text is scraped
Then `blob_read_duration_seconds` holds observations from those reads

**AC-4: Log levels**
Given each event of the logging table (corrupt chunk, unwrap failure, write I/O error, aborted sweep, floor hit, orphan in report mode, missing file at GC unlink, cycle start and summary, rotation batch, key check passed)
When it is triggered
Then exactly one JSON line at the listed level is written, with blob and account ids only

**AC-5: No per-chunk or per-block lines**
Given a 64 MiB upload and its download
When the log output is counted
Then the number of blobstore lines is the same as for a 2 MiB file (no line per block or chunk)

## Non-Functional Requirements

**Performance**
- Metric updates add no lock on the block-write or read hot path beyond the registry's own.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Log field set of each event | Ids only; no file name, no key bytes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 100 writes, 10 files | IT-04-5 (c) metric assertion | Histogram count = flushes | |
| AC-2 | Volume with 8 MiB reserve | Probe and scrape | Gauges equal probe and setting | |
| AC-3 | 10 opens | Scrape | Observations present | |
| AC-4 | Each logging event | Log capture | One line at its level, ids only | NFT-SEC-38 |
| AC-5 | 64 MiB and 2 MiB transfers | Log line count | Equal counts | |

## Constraints

- The observability helper (E-09) owns the registry and JSON format; this task adds the blobstore series and events only.

## Risks & Mitigation

**Risk 1: Metric names drift from the alert rules**
- *Risk*: the ext4 stall alert watches a different name.
- *Mitigation*: names exactly as the description lists them.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| What one `blob_read_duration_seconds` observation covers (one open, one chunk read, one whole stream) is not stated | Epic owner | open | Low |
