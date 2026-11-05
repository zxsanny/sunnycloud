# Media performance, security and acceptance tests

**Task**: AZ-3848_media_perf_security_acceptance
**Name**: Media performance, security and acceptance
**Description**: Measure claim speed, interactive priority under a bulk backlog and input streaming; prove worker credentials, job scoping, hostile outputs and cross-account thumbnails are safe; and show an interactive thumbnail overtaking a bulk import on the real stack
**Complexity**: 5 points
**Dependencies**: AZ-3847_media_integration_suite, AZ-3806_auth_extraction, AZ-3829_tree_transfer_feed_handlers, AZ-3830_share_media_handlers, AZ-3831_internal_router_test_clock, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3872_worker_slot_loop, AZ-3853_decoder_runner_socket_loop, AZ-3857_decoder_image_executor; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 08 media_jobs
**Tracker**: AZ-3848
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — routes under attack (E-18/c02).
- `_docs/02_document/contracts/08_media_jobs/derived_access.md` — thumbnail routes (E-18/c09).

## Problem

AC-10 needs interactive thumbnails to pass a bulk backlog; AC-72 needs input streaming at disk speed with flat memory; AC-25, AC-59 and AC-79 need the worker and thumbnail surfaces to resist misuse. These need load, hostile inputs and, for AT-08-1, the real supervisor and decoders.

## Outcome

- PT-08-1 to PT-08-3, ST-08-1 to ST-08-4 and AT-08-1 recorded against their thresholds.
- AT-08-2 (kill the supervisor mid-job) is run by the media pipeline AZ-3661 (its AC-71 kills at fetch, decode and post) and is not repeated here.

## Scope

### Included
- PT-08-1..3 on H1 (PT-08-3 throughput on C-REF hardware only), ST-08-1..4 on H1 plus the public listener, AT-08-1 on the Compose `core` profile with the worker and both decoders.

### Excluded
- AT-08-2 (AZ-3661 AC-71); system NFT-PERF-13 and NFT-PERF-14 (E-31/c44).

## Acceptance Criteria

**AC-1: Claim throughput (PT-08-1)**
Given 100,000 queued jobs over two lanes
When 50 claim loops run for 3 minutes after a 10 s ramp
Then claim p95 is at most 5 ms (failure above 20 ms), p99 at most 10 ms (failure above 50 ms), throughput at least 1,000 claims/s (failure below 300), 0% errors, at most 20 database connections and database CPU under 50%

**AC-2: Interactive passes the backlog (PT-08-2, AC-10)**
Given 10,000 bulk `stills` jobs and 4 claim loops
When one interactive job is enqueued every 2 s for 60 s
Then each interactive job is claimed within one claim round (500 ms; failure above 2 s), and 0 bulk jobs are claimed before an interactive job enqueued earlier

**AC-3: Input streaming (PT-08-3, AC-72)**
Given a 1 GiB DS-25 file and one worker on loopback
When its input is fetched
Then throughput is at least 112 MB/s (failure below 90 MB/s, C-REF only, otherwise `NOT-RUN(HW-REF)`), API memory grows by less than 64 MiB (failure at 200 MiB), and the BLAKE3 equals the manifest

**AC-4: Credential confusion (ST-08-1, AC-79)**
Given an admin session token, a device token and a session cookie, and the worker secret
When the three user credentials call all worker routes on `:8081`, and the worker secret calls the account and admin user routes on the public listener
Then the first answers are 403 and identical to the no-header answer, and the second are 401

**AC-5: Job scoping (ST-08-2, AC-25)**
Given W1 holding a job and W2 authenticated with its own id
When W2 calls input, outputs, result, failure and lease on that job
Then each answers 409 `lease_lost` with no input byte and no state change

**AC-6: Hostile outputs (ST-08-3, AC-25)**
Given `Content-Type: image/webp` on every post to `thumb`
When an SVG with script, an HTML page whose first 12 bytes imitate a WebP header, a 10 MiB chunked zero stream and a WebP followed by 1 MiB of trailing data are posted
Then the first three are refused with 422 or 413, the server stops reading at the cap plus one buffer, nothing is stored for a refused post, and the trailing-data case's outcome is recorded with the cap holding

**AC-7: Cross-account thumbnails (ST-08-4, AC-59)**
Given account A with a ready thumbnail
When account B requests A's node on the thumbnail, transcode and thumbnail-states routes, and sends 1,001 ids
Then B gets 404, no state of A, and the over-long body gets the status recorded by E-18/c01

**AC-8: Interactive thumbnail during a bulk import (AT-08-1, AC-10)**
Given the Compose `core` profile with the supervisor and both decoders
When 500 DS-04 photos are uploaded with `import_hint`, then 10 DS-03 photos one by one
Then the bulk jobs have priority 1, each DS-03 photo reaches `Ready` with no bulk job claimed before it, and its small thumbnail is a 320 px WebP with dimensions swapped for orientations 6 and 8

## Non-Functional Requirements

**Performance**
- As in AC-1 to AC-3.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: test suite task | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 100,000 queued jobs | PT-08-1 | p95 5 ms; 1,000 claims/s | |
| AC-2 | 10,000 bulk jobs | PT-08-2 | One round; 0 overtakes | NFT-PERF-13, NFT-PERF-14 |
| AC-3 | DS-25 1 GiB | PT-08-3 | 112 MB/s; RSS under 64 MiB; hash equal | FT-N-43 |
| AC-4 | User credentials; worker secret | ST-08-1 | 403 uniform; 401 | NFT-SEC-50 |
| AC-5 | W1, W2 | ST-08-2 | 409; no data | NFT-SEC-29 |
| AC-6 | Hostile posts | ST-08-3 | Refused; read stopped at cap | NFT-SEC-28 |
| AC-7 | Accounts A, B | ST-08-4 | 404; nothing leaked | NFT-SEC-13 |
| AC-8 | Compose `core`, DS-04, DS-03 | AT-08-1 | Ready first; 320 px; orientation | FT-P-20, FT-P-21, FT-P-22 |

## Constraints

- PT-08-1 targets are proposed (08 sets none) and replaced by the first measured baseline.
- AC-8 uses the release images of the commit under test with the test clock; no fake supervisor or decoder.

## Risks & Mitigation

**Risk 1: No reference hardware (R03)**
- *Risk*: AC-10 timings are unproven without REF-SRV.
- *Mitigation*: timing rows report `NOT-RUN(HW-REF)` elsewhere; correctness rows always run.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AT-08-2 is left to AZ-3661 AC-71 (same kill test, more kill points); the owner may want it here too | E-18 owner | open | Low |
| AT-08-1 on the Compose stack ties this task to E-19, E-20, E-21, E-22 and E-31 | E-18 owner | open | Low |
| The reference hardware for PT-08-3 is not yet named (performance gaps G1, G11) | Owner | open | Low |
