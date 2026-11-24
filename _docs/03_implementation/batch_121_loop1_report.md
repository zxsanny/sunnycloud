# Batch Report

**Batch**: 121
**Tasks**: AZ-3661 media pipeline stage 2 (AC-1 to AC-5, AC-71), archived with two legs not run (see "Not covered").
**Date**: 2026-11-24

## What changed

- **Two defects found by the first real run of supervisor and decoder together, both fixed:**
  1. The supervisor names a job folder by its own path (`/scratch-interactive/<job>`); the decoder mounted the same volume at `/scratch` and refused every job ("job_dir is not an absolute path inside the scratch root"). `deploy/compose/compose.dev.yml` and `tests/compose.test.yml` now mount each decoder's scratch volume at the supervisor's path and set `SUNNYCLOUD_DECODER_SCRATCH_ROOT` to it. Configuration only.
  2. The supervisor created the job folder `0750`; the decoder makes and removes its own `work/` inside it and failed (`cannot make work/: Permission denied`, job `OutputInvalid`). `src/worker/src/scratch/mod.rs`: `JOB_DIR_MODE` is now `0o770` (group `decoder_gid`, no world bits); `src/worker/tests/scratch.rs` and `09_worker_supervisor/description.md` follow. Decision recorded: the decoder is the untrusted party, but the supervisor opens its output beneath the job folder with no symlinks and removes the tree without following links, so group write on the folder adds no path the supervisor trusts.
- `tests/consumer/suites/media/test_media_pipeline.py` (9 tests, 8 run): AC-1 JPEG with orientation 6 (stored 2400x1800) gets a 240x320 thumbnail and a 1080x1440 preview, state `ready`, served as WebP. AC-2 a 12 Mbit/s 1080p clip is transcoded (H.264 High, AAC, index first, smaller than the input) with a poster; a compliant clip with its index first closes `skipped_compliant` and has no transcode (404); a compliant clip with its index last is remuxed (index moved to the front, same picture and profile, `mdat` the same size, so not re-encoded). AC-3 a truncated HEIC fails its job (`dead`, state `failed`), the original downloads byte for byte, the decoder containers keep their start time and the next photo works; an SVG named `.png` gets no job at all (the content is sniffed, an SVG is not an image type the planner knows). AC-4 `doctor` reports the `media_worker` check as ok (worker present, decoder probe ok, image age current).
- `tests/consumer/suites/media/test_worker_crash.py` (AC-71, 4 tests, 2 run): both decoders are frozen (`docker pause`), a photo or a clip is committed, the supervisor is killed with SIGKILL once a job is running in a decoder, the decoders are thawed, the supervisor is started, the test clock moves 30 s past the 10 s lease. Then: the most attempts of any job is 2, exactly one derived row per kind was added, no blob is left that no revision or derived row refers to (after the derived janitor, 5 s here), both scratch volumes are empty.
- `src/server/tests/wiring_flows.rs` (AC-5): production callers of `WorkerJobs::accept_output`, `DerivedStore::accept_output`, `ThumbnailStateSource::states` (02 and 11), `DerivedOpener::open_derived` and `open_derived_for`, `open_blob_content`, the derived janitor on the scheduler, the supervisor's `WorkerApiClient`, and `main` starting the worker which builds `SlotLoop` and `Slot`; 7 of 7 pass.
- Harness: `tests/tools/gen_media.py` (photo, clips and the two hostile files from the host's ffmpeg, each clip checked with ffprobe for bitrate, profile, colour tags and index position; the decoder image's ffmpeg is a minimal build without synthetic sources); ctl action `inspect` (container state, start time, restart count; scratch volume listing), `process-kill` gained `pause` and `unpause` for the decoders, `db-probe` queries `media_jobs`, `job_attempts`, `derived_rows`, `blob_refs`, `worker_status`; `tests/run.sh` makes the media files, takes `E2E_PYTEST_ARGS`, passes the commit time as the decoder's build timestamp (without it the image age was 20781 days) and has a `rerun` command (consumer against a kept stack); the stack config sets `derived_janitor_interval` 5 s, `job_reap_interval` 2 s, `job_retry_backoff` 1 s; consumer `mediakit.py` reads WebP size and MP4 facts from bytes.

## Code Review Verdict: PASS (own review)

Product change is two lines of mode and path configuration with their tests; the rest is harness and tests.

## Test Suite

- `tests/run.sh --profile core`: PASS=75 FAIL=0 NOT-RUN=4 (the sentinel and the three below), exit 0.
- CI-parity: `cargo fmt --all --check`, clippy `-D warnings` on `sunnycloud-worker` and `sunnycloud-server` (all targets), `cargo nextest -p sunnycloud-worker` 211 of 211, `wiring_flows` 7 of 7; ctl unit tests 29 OK.

## Not covered (leftover `2026-11-24_az-3661-not-run-legs.md`)

- AC-3 "the decoder exits after a hostile-class outcome and Compose restarts it": no file the API accepts produces a hostile-class exit (a corrupt file is a clean `Corrupt`; the container keeps serving, which the suite checks). The decoder's own suite owns it (IT-10-14, IT-10-15) and needs the decoder test image. NOT-RUN.
- AC-71 kills during the input fetch and during the output post: both phases last milliseconds on small files and the stack has no hold on them. NOT-RUN, the kill inside the decoder runs.
- Seen once: `worker_status.decoder_ok` read false about 20 s after a worker start and true 30 s later (the status interval). The suite waits for it before `doctor`. Question for E-20, in the leftover.
- The 5 s p95 thumbnail time (AC-10) is E-31/c44, as the spec says.

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| AC-3 "SVG named .png: the jobs fail" | The planner never enqueues an SVG (sniffed content); the test asserts no job, original intact, no thumbnail | clear |
| AC-2 "transcode route serves the produced file" for a skipped clip | The route answers 404 for a clip that needs no transcode | clear |
| Poster jobs produce the `thumb` kind only (no `preview`) | Asserted as such (clip: poster served at `size=small`) | clear |

## Commit

`[AZ-3661] Media pipeline on the compose stack: photo and clip outputs, hostile input, supervisor kill in the decoder; scratch path and job folder mode fixed` - Loop: 1

## Next: AZ-3813 and AZ-3815 (CLI suites), AZ-3835 AC-1 and AC-4, then AZ-3847/3878 suites on the stack
