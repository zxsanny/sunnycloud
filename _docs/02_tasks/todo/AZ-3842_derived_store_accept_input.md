# Derived outputs and worker input: accept_output and open_input

**Task**: AZ-3842_derived_store_accept_input
**Name**: Derived store and worker input
**Description**: Accept a worker's output as an encrypted derived blob after checking lease, revision, size and sniffed type, exactly once per key, and stream the decrypted original to the job's holder
**Complexity**: 5 points
**Dependencies**: AZ-3837_media_shared_params_protocol, AZ-3839_media_tables_config, AZ-3840_job_queue, AZ-3826_media_gaps_spike, AZ-3707_tree_reader, AZ-3726_blob_writer, AZ-3728_blob_reader, AZ-3729_blob_registry_gc_queue, AZ-3730_gc_sweeper
**Component**: 08 media_jobs
**Tracker**: AZ-3842
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — output kinds, accepted types, statuses (E-18/c02).
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `get_revision`, `is_current_or_retained` (E-13/c03).
- `_docs/02_document/contracts/04_blobstore/blob-writer.md` — `write_complete` (E-10/c06).
- `_docs/02_document/contracts/04_blobstore/blob-reader.md` — `open` (E-10/c08).
- `_docs/02_document/contracts/04_blobstore/blob-registry-gc.md` — `register_in_tx`, `GcQueue`, `BlobCatalog` (E-10/c09).

## Problem

Worker output is untrusted: it may be oversized, mistyped, for the wrong kind, or posted twice after a lost response or a crash (AC-25, AC-71). It must be stored encrypted like every blob (AC-43), and the job may close only when every expected output exists. The worker's input must be exactly the original plaintext through the same decrypt path as downloads (AC-72, worker side).

## Outcome

- `accept_output(job_id, worker_id, kind, media_type, width?, height?, duration_ms?, body, length)` returns `Accepted` or `AlreadyPresent`, or `LeaseLost`, `RevisionObsolete`, `OutputTooLarge`, `OutputTypeRejected`, `OutputKindRejected`, `Storage`.
- Order: lease check; revision still current or retained; stream the body through 04's writer with the kind's size cap and no transaction open; sniff the head (WebP or JPEG for `thumb` and `preview`, MP4 for `transcode`); then one short transaction that registers the blob as committed, inserts the derived row if absent and, when this was the last expected output, marks the job `succeeded` with outcome `derived`.
- If the derived row already existed, the new blob is queued for removal with `Due::Now` and the answer is `AlreadyPresent`; a refused output keeps no bytes.
- `open_input(job_id, worker_id)` checks the lease and the revision and returns 04's decrypted stream of the whole original with its exact length, or `RevisionObsolete`.

## Scope

### Included
- Both operations, the per-kind caps, the sniffer, idempotency and surplus GC.
- A lossless remux result is accepted like a transcode; a `stills` job posts `preview` and `thumb` as two outputs.

### Excluded
- The routes and status mapping (E-21/c15); worker authentication (E-18/c08); the supervisor's own hash check (E-20/c03); the end-to-end kill test (AZ-3661 AC-71).

## Acceptance Criteria

AC-1..AC-4 are local to this task. AC-71 and AC-72 keep their numbers from `acceptance_criteria.md` (component parts kept here by `interaction-risks.md`; the end-to-end parts are AZ-3661 and AZ-3660).

**AC-1: Encrypted derived output closes the job (AC-43)**
Given a claimed `stills` job and a claimed `transcode` job
When the holder posts a 1440 px WebP `preview`, then a 320 px WebP `thumb`, and an MP4 for the transcode
Then the first post is `Accepted` with the job still running, the second is `Accepted` and the job is `succeeded` with outcome `derived`, the transcode job closes the same way, the derived rows carry kind, media type, width and height, the stored files do not begin with a WebP or MP4 signature, the blob rows are committed, and the account's quota usage is unchanged

**AC-2: Untrusted output is refused (AC-25)**
Given a claimed job and caps of 64 KiB for `thumb`
When the holder posts a 65 KiB `thumb`, a body longer than its declared length, a PNG as `thumb`, a text file labelled `video/mp4` as `transcode`, an MP4 as `thumb`, a `transcode` to a `stills` job, an output after the revision was deleted and its retention passed, and an output after the lease expired
Then the answers are `OutputTooLarge`, `OutputTooLarge`, `OutputTypeRejected` (decided by sniffed bytes whatever the header), `OutputTypeRejected`, `OutputTypeRejected`, `OutputKindRejected`, `RevisionObsolete` and `LeaseLost`; the job state is unchanged, no bytes are kept and no committed blob row is left behind

**AC-3: No transaction during streaming**
Given a slow 2 MiB `transcode` body
When it is accepted
Then no database transaction of 08 is open while the body streams, and registration and the derived row happen in one short transaction

**AC-4: Crash after the file write**
Given a fault injected between the file write and the row transaction
When the holder retries the post
Then the first attempt left no derived row (its file is left to 04's orphan sweep) and the retry is `Accepted`

### Interaction criteria

**AC-71: Exactly one derived file per key (R4)**
Given a claimed job
When the same `thumb` is posted twice in a row, 8 identical posts race on a fresh job, and a `stills` retry finds `preview` stored and `thumb` missing
Then the repeat answers `AlreadyPresent`; of the 8 racers exactly 1 is `Accepted` and 7 are `AlreadyPresent`; one derived row exists per (account, content hash, kind, params version); each surplus blob is queued with `Due::Now` and no unreferenced derived file remains after 04's GC; the partial retry answers `AlreadyPresent` for `preview` and `Accepted` for `thumb` and the job then succeeds

**AC-72: Worker input equals the plaintext (worker side)**
Given a 4 MB DS-03 photo and a 150 MB DS-07 clip committed through 03, each with a claimed job
When the holder opens the input
Then the length equals the file size exactly and the BLAKE3 of the bytes equals the stored content hash; a revision past retention or deleted and collected answers `RevisionObsolete`; a non-holder gets `LeaseLost`

## Non-Functional Requirements

**Performance**
- Input streaming at least 112 MB/s with API memory growth under 64 MiB (PT-08-3 in E-18/c13).

**Reliability**
- Coverage at least 90% on `accept_output` (critical path).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Sniffer on WebP, JPEG, MP4, PNG, SVG, text heads | Only the kind's types pass |
| AC-1 | "Last expected output" rule per job kind | Close only when all outputs exist |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stills and transcode jobs | IT-08-9 | Accepted; job succeeded; ciphertext on disk | FT-P-40 |
| AC-71 | Repeat, 8-way race, partial stills | IT-08-10 | One row per key; surplus GC | NFT-RES-36, NFT-RES-23 |
| AC-2 | Cap 64 KiB; hostile posts | IT-08-11 | Typed refusals; nothing kept | NFT-SEC-28 |
| AC-72 | DS-03, DS-07 via 03 | IT-08-12 | Exact length; hash equal; obsolete; non-holder refused | FT-N-43 |
| AC-3 | Slow 2 MiB body | Transaction sampling | None during stream | |
| AC-4 | Fault after write | Retry post | No row first; retry accepted | NFT-RES-36 |

## Runtime Completeness

- Named capability: encrypted derived storage and the worker input stream (ADR-006, ADR-009).
- Production code that must exist: real streaming through 04's writer and reader, the real sniffer and the single short transaction.
- Unacceptable substitutes: trusting the worker's `Content-Type` or length, buffering the whole body in memory, or a plaintext derived file.

## Constraints

- The worker never chooses blob ids or paths; derived blobs are under the job's account and do not count against quota.

## Risks & Mitigation

**Risk 1: Hostile oversized output**
- *Risk*: a supervisor streams gigabytes into a `thumb`.
- *Mitigation*: the cap is enforced while streaming (AC-2).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Whether a superseded or deleted revision that is still retained is served or answered `revision_obsolete` conflicts between 08 §4 and IT-08-12 (served) and FT-N-49, data model §9.1, F10 and 09's tests (obsolete); decided by E-18/c01. AC-72 follows 08 §4 until then | E-18 owner | open | Medium |
| 08 §9 says output dimensions are bounded, but no bound is given | E-18 owner | open | Low |
