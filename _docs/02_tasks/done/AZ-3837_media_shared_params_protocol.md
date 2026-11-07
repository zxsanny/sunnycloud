# media-shared: job kinds, Params v1, output specs and the worker job protocol

**Task**: AZ-3837_media_shared_params_protocol
**Name**: media-shared kinds, params and worker protocol
**Description**: Fill the 08-owned part of `media-shared` with the job-kind table, the version 1 output parameters and the accepted output types, freeze the worker listener protocol that the supervisor calls and the API serves, and keep the API-side copies of the job DTOs equal on the wire
**Complexity**: 3 points
**Dependencies**: AZ-3770_contracts_part3, AZ-3771_media_shared_skeleton, AZ-3826_media_gaps_spike, AZ-3652_initial_structure
**Component**: 08 media_jobs (`media-shared`, modules `types` and `policy`)
**Tracker**: AZ-3837
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — the type skeleton this task fills (E-00/c06).
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — the API-side copies of the job DTOs, open point 2 (E-00/c05).

## Problem

E-00/c06 creates the shared types, their JSON round-trip tests and the crate-graph test. What 08 still owes the supervisor (09) and the API routes (11) is the content: which outputs each job kind must produce and on which lane, the version 1 output settings, which media types an output may have, and one frozen description of the worker listener routes, bodies and status codes. The same DTOs exist twice (in `media-shared` for 09 and in `contracts` for 11) and can drift.

## Outcome

- Job-kind table: `stills` on lane `image` produces `thumb` and `preview`; `poster` on lane `video` produces `thumb`; `transcode` on lane `video` produces `transcode`.
- `Params` for `params_version` 1: `thumb` 320 px and `preview` 1440 px long edge, WebP, orientation applied, a separate HEIC profile; transcode H.264 High, short side at most 1080 and long side at most 1920, AAC with the native encoder, MP4 with the index first, HDR tone-mapped to SDR.
- Output specs: `thumb` and `preview` accept WebP or JPEG, `transcode` accepts MP4 (always sniffed by the API, never trusted from the worker).
- The `JobSpec` type the planner returns.
- The worker job protocol contract, consumed by E-20 and E-21.
- A wire test proving each `media-shared` DTO and its `contracts` copy serialise to the same JSON.

## Scope

### Included
- The kind table, `Params[1]`, output specs and `JobSpec` in `media-shared`.
- The contract file `contracts/08_media_jobs/worker_job_protocol.md`.
- The wire-equality test (run from 08's crate, which may import both).

### Excluded
- Policy functions (E-18/c03); the type skeleton (E-00/c06); `decoder_protocol` (E-19/c04).
- The routes themselves (E-21/c15) and the client (E-20/c03).

## Acceptance Criteria

**AC-1: Kind table**
Given each job kind
When its lane and expected outputs are read
Then `stills` is `image` with `thumb` and `preview`, `poster` is `video` with `thumb`, and `transcode` is `video` with `transcode`, matching the generated `lane` column of `data_model.md` §9.1

**AC-2: Params v1 snapshot (AC-44)**
Given `Params` for version 1
When the snapshot test runs
Then edges are 320 and 1440, stills are WebP, video is H.264 High with short side at most 1080, long side at most 1920, AAC and faststart; changing any value without raising `params_version` fails the test

**AC-3: Accepted output types**
Given the output spec of each derived kind
When a media type is checked against it
Then `thumb` and `preview` accept only WebP and JPEG, and `transcode` accepts only MP4

**AC-4: Wire copies equal**
Given one value of `ClaimedJob`, `WorkerStatus`, the lane, the skip outcome, the disposition and the failure result
When the `media-shared` value and the `contracts` copy are serialised
Then the JSON is byte-for-byte equal

**AC-5: Protocol frozen**
Given 08 §5 and the answers of E-18/c01
When the contract is written
Then it lists all 8 worker operations (claim, lease extend, release, input, output, result, failure, status) with method, path, headers, body, success status and every error status and code, and E-20/c03 and E-21/c15 name it under their document dependencies

**AC-6: No new crate edges**
Given the crate graph
When the crate-graph test runs
Then `media-shared` still has no sqlx, I/O, `contracts` or component dependency

## Non-Functional Requirements

**Compatibility**
- serde-level dependencies only in `media-shared`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Kind table | As listed |
| AC-2 | `Params[1]` snapshot | Equal; change without bump fails |
| AC-3 | Output-type check per kind | Only listed types accepted |
| AC-4 | Wire equality of the two DTO copies | Identical JSON |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | `Params[1]` | IT-08-17 constants part | Values as listed | FT-P-26, NFT-PERF-20 |
| AC-6 | Workspace | Crate-graph test (AZ-3652) | No forbidden edge | |
| AC-5 | Contract file | Review with E-20 and E-21 owners | All routes and statuses present | NFT-SEC-50 |

## Constraints

- `media-shared` keeps no behaviour that needs I/O; the per-kind size caps are API settings (E-18/c04), not part of `Params`.
- Any field change in these types needs the same change in the `contracts` copy in the same task (skeleton contract versioning rule).

## Risks & Mitigation

**Risk 1: Copy drift between 09 and 11**
- *Risk*: a field renamed on one side breaks the supervisor silently.
- *Mitigation*: AC-4 runs in CI.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| CRF and preset values are not given; 08 §7 says they are chosen to meet derived storage at most 20% of original bytes and tuned in NFT-PERF-20 | E-18 owner | open | Low |
| `JobSpec` is named in 08 §1 without a field list | E-18 owner | open | Low |
| `content_hash` is lowercase hex, 64 characters, and `Range` on input is ignored (decided 2026-11-07, E-18/c01) | E-18 owner | resolved | Low |

## Contract

This task produces `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
