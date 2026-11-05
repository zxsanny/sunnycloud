# Transfer performance suite and transfer metrics

**Task**: AZ-3759_transfer_perf_metrics
**Name**: Transfer performance and metrics
**Description**: Emit the transfer metrics of `observability.md` and run PT-03-1 to PT-03-5 for ingest, constant-time commit, the small-file path, Range reads and memory bounds
**Complexity**: 3 points
**Dependencies**: AZ-3735_transfer_put_block, AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3755_transfer_download, AZ-3786_sc_observability; notes: E-31/c18 (test epic E-31, decomposed at greenfield Step 9); E-31/c19 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 03_transfer
**Tracker**: AZ-3759
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md`, `commit_service.md`, `download_service.md` — the operations under measurement.

## Problem

The product promises line-rate transfers (AC-2), constant-time commits up to 50 GiB (AC-4), 10,000 small files in a minute (AC-40), fast ranges (AC-43) and bounded memory (AC-51). The server's share of each must be measured at the component boundary, and operators need the transfer metrics to see sessions, rejections and commit times in production.

## Outcome

- The transfer metrics exist with their labels and no per-account or per-file label values.
- PT-03-1 to PT-03-5 report against their targets on their environment.

## Scope

### Included
- Metrics `upload_sessions_active`, `upload_sessions_total{result}`, `upload_rejections_total{reason}`, `blocks_received_total`, `block_bytes_received_total`, `commits_total{kind, result}`, `commit_duration_seconds{kind}`, `download_bytes_total`, registered through helper E-09 (any not already emitted by the operation tasks).
- PT-03-1 to PT-03-5.

### Excluded
- `block_fsync_duration_seconds`, `blob_*` metrics (E-10/c15); `http_*` metrics (E-21); network-level NFT-PERF scenarios (E-31).

## Acceptance Criteria

**AC-1: Metrics present and low-cardinality**
Given a run with uploads, a refused start, commits of each kind and a download
When the metrics endpoint is scraped
Then each listed metric exists with its documented labels, the counters moved by the run, and no label carries an account, node, file or IP value

**AC-2: Block ingest (AC-2, AC-38)**
Given 1 session x 4 lanes and 100 sessions x 4 lanes of 2 MiB blocks for 3 minutes on XFS (C-REF)
When throughput and ack latency are measured
Then single-session throughput is at least 250 MB/s (fail 112 MB/s), ack p95 at most 150 ms (fail 400 ms), errors other than intended `Overloaded` 0%, buffered bytes within `max_buffered_block_bytes`, at most 16 DB connections

**AC-3: Constant-time commit (AC-4)**
Given commits of 1 MiB, 1 GiB and 8 GiB sparse-hybrid files, 20 each (C-LARGE)
When commit time is measured
Then p95 is at most 100 ms (fail 2 s) and the 1 MiB to 8 GiB spread is within 20% (fail 50%)

**AC-4: Small-file path (AC-40)**
Given 10,000 `stage_small` calls of 1 KiB to 1 MiB (DS-01) at concurrency 16, then 20 batches of 500 (C-REF)
When server time is measured
Then total server time is at most 25 s (fail 45 s), `stage_small` p95 at most 40 ms (fail 100 ms), batch p95 at most 1.0 s (fail 2 s)

**AC-5: Range and streaming (AC-43, AC-2)**
Given 10,000 random 4 KiB ranges from 16 clients on a 1 GiB blob (DS-25) and one 5 GiB stream (C-REF)
When latency and throughput are measured
Then range p95 (open plus first chunk) is at most 4 ms (fail 5 ms) and streaming at least 250 MB/s (fail 112 MB/s)

**AC-6: Memory bounds (AC-39, AC-51)**
Given 100 sessions declaring 50 GiB with one block each in flight, and a 5 GiB download (env C)
When memory is measured
Then the progress table is at most 80 MB (fail 100 MB) and download RSS growth at most 16 MiB (fail 64 MiB)

## Non-Functional Requirements

**Performance**
- As in AC-2 to AC-6.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Label sets | Only the documented enumerations |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mixed run | Metrics scrape | Metrics and labels as documented | |
| AC-2 | XFS, C-REF | Ingest (PT-03-1) | >= 250 MB/s; p95 150 ms | NFT-PERF-02, NFT-PERF-04 |
| AC-3 | DS-10 slices, C-LARGE | Commit (PT-03-2) | p95 100 ms; spread 20% | NFT-PERF-06 |
| AC-4 | DS-01, C-REF | Stage and batch (PT-03-3) | <= 25 s total | NFT-PERF-07 |
| AC-5 | DS-25, C-REF | Ranges and stream (PT-03-4) | p95 4 ms; >= 250 MB/s | NFT-PERF-03, NFT-PERF-11 |
| AC-6 | 100 declared sessions; 5 GiB read | Memory (PT-03-5) | <= 80 MB; <= 16 MiB | NFT-PERF-28 |

## Constraints

- C-REF and C-LARGE rows run only on their designated hosts (`tests/environment.md`); until REF-SRV exists they are reported `NOT-RUN(HW-REF)` (R03), never measured on a workstation.

## Risks & Mitigation

**Risk 1: No reference hardware (R03)**
- *Risk*: AC-2 to AC-5 cannot be measured.
- *Mitigation*: report `NOT-RUN(HW-REF)`; AC-6 runs on env C.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-03-1 may lower lanes or checkpoint sizes, which changes AC-2's throughput risk (E-14/c01) | Owner | open | Medium |
