# Batch 127, loop 1: AZ-3847 media integration suite (AC-1 to AC-4 met; AC-5 in part)

**Batch**: 127
**Tasks**: AZ-3847_media_integration_suite
**Date**: 2026-11-26

## Starting point

A read-only map, made before this batch, found every IT-08 case already in its port form: Rust against the real 08 crate on PostgreSQL 18. The gaps were:

- **Wire form on the real 08 code.** The 11 crate's worker tests run over doubles. The server pipeline tests covered only a few statuses.
- **AC-2.** No test compared all six credentials, with headers, against the real `WorkerAuth`.
- **AC-3.** No test ran `decide` on real probes.
- **Port-form checks.** IT-08-3 had no bounded-count check. IT-08-5 had no 200 ms claim bound.
- **AC-5.** There was no coverage gate on 08.
- **IT-08-19.** It was still marked BLOCKED in the docs, although AZ-3840 AC-7 had resolved it.

## What was built

- **Harness H1, wire form.** It runs on the real `sunnycloud serve` binary (`--features test-clock`), on PostgreSQL 18 and a blob volume, with the real 02, 03, 04, 08 and 11, and files uploaded through 03. It extends the existing `pipeline_support` harness; there is no second harness.
  - `src/server/tests/pipeline_media_wire.rs` (5 tests):
    - **IT-08-4**: interactive first, then bulk (`import_hint`) by age; lanes never cross; 204 with no body when empty; `attempt` 1 and `lease_ms` equal to `job_lease`; the row shows `running`, 1 attempt and `leased_by`.
    - **IT-08-6**: after the test clock passes the lease, w2 gets attempt 2. Every call from w1 answers 409 `lease_lost` with no job data: lease, output, failure, result, release. So does a call from a worker that never held the job. The new holder extends with `lease_ms`.
    - **IT-08-8**: release answers 204, the row goes back to `queued` with 0 attempts, and the job is claimable again. `result` with `skipped_compliant` or `obsolete` answers 204 and leaves no derived row. A non-holder gets 409.
    - **IT-08-16 / AC-2**: six credentials on `claims`, `input` and `status`: no header, empty bearer, wrong secret, user session, device token, session cookie. All 18 answers are identical once `X-Request-Id`, `Date` and the body's `request_id` (which repeats that header) are removed. A valid worker on a job it does not hold gets 409. No refused credential appears in the log.
    - **IT-08-20**: `seen_at` is the API clock (one hour ahead), whatever the request's `Date` says.
  - `src/server/tests/pipeline_media_outputs.rs` (5 tests):
    - **IT-08-9**: preview then thumb answers 204; the job stays `running` until the last output, then becomes `succeeded` with `derived`; the transcode job closes the same way. The rows carry kind, type, width and height. No blob file starts with a WebP or MP4 signature. The blobs are `committed` and quota `used_bytes` is unchanged.
    - **IT-08-10**: a repeat answers 204 then 200. Of 8 concurrent posts, exactly one answers 204 and seven 200. After a retry, preview answers 200 and thumb 204. There is one row per content and kind. Every surplus blob sits in `blob_gc` with reason `derived_removed`, due now, with its file in place. After a restart with GC every second, all of them are gone.
    - **IT-08-11**: a 65 KiB thumb answers 413. A PNG for thumb, text labelled `video/mp4` and an MP4 for thumb answer 422 `output_type_rejected`. `transcode` posted to a stills job answers 422 `output_kind_rejected`. `..%2Fx` and `unknown` answer 404 or 422. An expired lease answers 409. Job rows, derived rows, blob files and committed blobs are all unchanged.
    - **IT-08-12**: the input is the exact plaintext with its `Content-Length` and `Cache-Control: no-store`. `Range` is ignored: 200, full body, no `Content-Range`. The block hash equals the claim's `content_hash`. The input still serves after the file is overwritten (retained revision). The wrong holder gets 409.
    - **Collected revision**: 404 `revision_obsolete` on both input and output, no derived row, then `result` `obsolete` closes the job with its row kept.
  - Helpers added in `pipeline_support/media.rs`: `claim`, `put_output_as`, `mp4_bytes`, `commit_file`, `claim_kind`, `derived_rows`, `account_number`, `surplus_blobs`, `problem_code`, `volume_files`. `WEBP` is now public.
  - CI: both binaries are added to the `test-clock` server-test step in `ci.yml`. They compile to nothing without the feature.
- **AC-3 / IT-08-17 real probes** (`src/decoder/tests/container/it_policy_probes.rs`, worker-written, run by the parent). Ten clips are generated on the host and probed by the release decoder image; `decide` runs on each probe. DS-07 decides `SkipCompliant`. The six near-miss clips (10.5 Mbit/s, 1920x1088, High 10, HLG BT.2020, HEVC, MP3) decide `Transcode`. The index-at-end clip decides `Remux`. Both controls (9.5 Mbit/s, portrait 1080x1920) decide `SkipCompliant`. For each clip, the test puts the missed property back and confirms the probe then decides `SkipCompliant`, so that property alone caused the decision. The clip makers are in `decoder/tests/common/generated.rs`. The clips' ffprobe facts are identical under CI's ffmpeg 6.1.1, checked in the `noble-decoder-env` image.
- **Port-form additions in the 08 crate**:
  - IT-08-3: the queued-count probe is bounded by `LIMIT $2`, and threshold 5 binds 6 (`effect_plan.rs`).
  - IT-08-5: no claim call over 200 ms with 20 concurrent claimers (`queue_claim.rs`). This held under llvm-cov instrumentation too.
  - IT-08-11 (a): a body longer or shorter than its declared length is refused as `OutputTooLarge` and nothing is kept (`accept_refuse.rs`). This covers the `SizeMismatch` branch, which was uncovered before.
- **Docs**: IT-08-19 and G-08-1 are marked resolved (`08_media_jobs/tests.md`, `tests/spec-gaps.md`).
- **AC-5 (part)**: `components/08_media_jobs/src/worker_auth.rs` is now in `scripts/ci/critical-paths.txt`, at 100% measured.

## AZ-3847 acceptance

| AC | Status |
|---|---|
| AC-1 wire form of the worker operations | Met: `pipeline_media_wire.rs`, `pipeline_media_outputs.rs` |
| AC-2 uniform 403 | Met: IT-08-16 test |
| AC-3 policy on real probes | Met: IT-08-17 container case |
| AC-4 every IT-08 case passes, no fake store | Met: port forms (08 crate, 125 tests) plus the wire forms above |
| AC-5 coverage | **In part.** Worker auth is gated at 90%. accept_output, claim and the crate are not gated: the CI gate's JSON summary reads 46.8%, 49.4% and 72.1%, while the merged per-line report reads 75.2%, 74.2% and 88.1%. Leftover `2026-11-26_az-3847-ac5.md` |

AZ-3847 stays in `todo/` with AC-5 open. Its Jira ticket stays In Progress.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3847_media_integration_suite | Partial (AC-5) | 2 new server test files, server harness, 1 new container test file plus clip makers, 3 08 test files, CI, critical paths, 2 docs | wire 10/10 (three runs of part 2), IT-08-17 1/1, 08 crate 125 (155 with unit tests) | AC-5 |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the diff against the spec, the contract and the harness patterns:

- Every AC traces to the tests named above.
- No production code changed.
- One test bug was found and fixed: the first IT-08-10 version counted surplus blobs while GC ran every second, and failed once.
- The 403 comparison removes only what differs per request.
- `pipeline_media_outputs.rs` reached 526 lines and was split: its helpers moved into the support module, leaving 437 lines.
- Warning: AC-5 is open (the leftover above).

## Test Suite

- `cargo test -p sunnycloud-server --features test-clock --test pipeline_media_wire --test pipeline_media_outputs`: 10 passed. Part 2 ran green three times in a row.
- `DECODER_IMAGE=sunnycloud-decoder:e2e cargo test -p decoder-runner --test container -- --ignored it_08_17`: 1 passed (5.8 s).
- `cargo llvm-cov nextest -p media-api`: 155 passed.
- CI-parity: PASS. Clippy with warnings as errors on `decoder-runner`, `media-api` and `sunnycloud-server` with all targets, with and without `test-clock`. `rustfmt --check` clean on all nine changed Rust files. `cargo test -p media-api -p decoder-runner`: all binaries green (08 `media` 125 passed; decoder host-process suites green; the 48 container cases are `#[ignore]` there and run in the `decoder-suite` CI job).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Over-cap outputs get two code names. A declared `Content-Length` over the kind's cap is refused by the HTTP body-limit layer as 413 `size_too_large`, before 08 reads a byte. 08's own `output_too_large` is for a stream longer than its declared length | IT-08-11 (a); `worker_job_protocol.md` open point 5 (code names for 413 and 422) | Both are 413 and keep nothing. The test asserts what the wire sends. The owner names the codes under open point 5 | unclear (naming only; no guarantee changes) |
| 2 | A "lying `Content-Length` smaller than the stream" has no wire form: HTTP/1.1 frames the body by `Content-Length`, so the extra bytes are a next request | IT-08-11 (a) | Tested at the port: both directions are refused | clear |
| 3 | H1's `job_lease` 2 s is below what the settings accept (minimum 5 s) | 08 tests.md H1 | The wire form uses 5 s and the test clock | clear |
| 4 | A "foreign revision" job cannot exist: the job's foreign key is `(revision_id, account_id)` | IT-08-12 | Nothing to test on the wire | clear |
| 5 | The CI coverage gate's JSON summary reads about half the merged line coverage for 08 | AZ-3847 AC-5 | Leftover with two fixes; the recommendation is to gate on merged lines | clear |
| 6 | Worker-token rotation is not repeated on the wire (it needs a restart with `worker_api_token_next`) | IT-08-16 | The port form `worker_rotation.rs` covers both tokens and the removal | clear |

## Commit

`[AZ-3847] Media worker routes over HTTP on the real binary; real-probe policy` — Loop: 1

## Next Batch

AZ-3878 (worker integration suite on H2) needs stack pieces that do not exist yet. A design choice between them goes to the operator first; see `_docs/OPERATOR_TODO.md`. The next ready item from the queue runs meanwhile.
