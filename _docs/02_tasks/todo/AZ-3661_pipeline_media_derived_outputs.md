# Media pipeline stage 2: supervisor and decoder to served derived files (F10)

**Task**: AZ-3661_pipeline_media_derived_outputs
**Name**: Media pipeline stage 2
**Description**: The real supervisor and decoders turn queued jobs into encrypted derived files that listings and the thumbnail and transcode routes serve, end to end across the API, worker and decoder containers
**Complexity**: 5 points
**Dependencies**: AZ-3741_docker_compose_skeletons, AZ-3830_share_media_handlers, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3792_janitor_scheduler, AZ-3842_derived_store_accept_input, AZ-3844_thumbnail_states_derived_opener, AZ-3845_media_janitors_erase_hook, AZ-3846_media_admin_health_doctor, AZ-3718_tree_thumb_state_listing, AZ-3755_transfer_download, AZ-3726_blob_writer, AZ-3729_blob_registry_gc_queue, AZ-3869_worker_api_client, AZ-3870_worker_scratch, AZ-3871_worker_decoder_client, AZ-3872_worker_slot_loop, AZ-3873_worker_heartbeat_watchdog, AZ-3874_worker_status_shutdown_main, AZ-3876_worker_policy_step, AZ-3877_worker_nvenc_preference, AZ-3853_decoder_runner_socket_loop, AZ-3855_decoder_containment_kill_all, AZ-3856_decoder_safe_open_watchdog, AZ-3857_decoder_image_executor, AZ-3858_decoder_probe, AZ-3859_decoder_video_poster, AZ-3860_decoder_video_executor, AZ-3863_decoder_media_build_stages, AZ-3864_decoder_image_build_checks, AZ-3660_pipeline_media_queue_worker_api, AZ-3652_initial_structure; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3661
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p09

## Problem

The second half of F10 crosses three processes: the supervisor (09) claims and fetches over `:8081`, drives a decoder (10) over its socket, and posts outputs that 08 stores as derived blobs through 04; 02 and 11 then expose thumbnail states and derived content. E-20 tests its loop against a fake decoder and E-19 tests the runner alone. No planned task runs the real API, supervisor and decoders together and proves that a commit ends as a served thumbnail or transcode, or that a supervisor crash leaves exactly one result.

## Outcome

- A photo or video committed through the real `serve` process becomes the expected derived files, produced by the real supervisor and decoder containers and served by the real routes.
- Every component operation of this stage has a production caller on the API side, and the supervisor's slot loop runs against the real listener and decoder socket.
- At least one integration test runs the stage in the Compose test stack, including a supervisor kill.

## Scope

### Included
- Flow order and seams (`system-flows.md` F10 steps 2-7):
  1. Supervisor slot loop (E-20/c06) → `WorkerApi` claim and input fetch over `:8081` → `Scratch` → `DecoderClient` → decoder runner (interactive or general container, `network_mode: none`).
  2. Decoder probe and render (`vipsthumbnail` 320 px WebP and 1440 px preview; video probe) → supervisor applies the policy from `media-shared` (skip compliant, remux, transcode, poster).
  3. Output post → `WorkerJobs::accept_output` → `DerivedStore::accept_output` → `BlobWriter` and `BlobRegistry` (encrypted derived blob) → derived row → job succeeded; heartbeat → `record_status` → `worker_status`.
  4. Listing → `ThumbnailStateSource::states` (02 enrichment) and the thumbnail-states route; thumbnail and transcode routes → `DerivedOpener::{open_derived, open_derived_for}` → `DownloadService::open_blob_content`.
  5. Scheduler (12) → `DerivedJanitor::run_cycle` removes surplus derived blobs.
- The end-to-end integration test of the steps above in the test stack (api, db, worker-supervisor, decoder-interactive, decoder-general).

### Excluded
- NVENC (v1.1), the queue and listener stage (E-22/p08), decoder hardening internals (E-19), web thumbnail polling (E-26/p01).

## Acceptance Criteria

AC-1..AC-5 are local to this task. AC-71 keeps its number from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Photo to served thumbnail**
Given the test stack and a signed-in device
When it commits a JPEG with orientation 6
Then the real supervisor and the interactive decoder produce a 320 px WebP and a 1440 px preview with swapped dimensions, the listing shows the thumbnail state ready, and the thumbnail route serves the WebP

**AC-2: Video policy end to end**
Given a non-compliant clip, a compliant clip with its index first and a compliant clip with its index at the end
When each is committed
Then the first gets an H.264 High 1080p AAC MP4 with faststart and a poster, the second closes `skipped_compliant` with no output, and the third gets a lossless remux; the transcode route serves the produced file

**AC-3: Hostile input is contained**
Given an SVG named `.png` and a truncated HEIC
When each is committed
Then the jobs fail, the originals stay downloadable unchanged, and the decoder container exits after the hostile-class outcome and is restarted by Compose

**AC-4: Worker status reaches the operator**
Given a running supervisor
When `doctor` runs
Then the worker presence, image age and decoder probe from `worker_status` are reported

**AC-5: Every stage operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `WorkerJobs::accept_output` (internal route), `DerivedStore::accept_output`, `ThumbnailStateSource::states` (02 listing and 11), `DerivedOpener::{open_derived, open_derived_for}` and `DownloadService::open_blob_content` (thumbnail and transcode routes), a scheduler registration of `DerivedJanitor`, and the supervisor's `main` starting its slot loop with the real `WorkerApi` and `DecoderClient`

### Interaction criteria (Step 1.8)

**AC-71: Supervisor crash leaves exactly one result** (R4)
Given a photo job and a video job
When the supervisor container is killed with SIGKILL during the input fetch, during decoding and during the output post (one run each), restarted, and the test clock passes the lease
Then after the retry exactly one derived row exists per (account, content hash, parameters), no surplus derived blob remains after the derived janitor runs, and the supervisor's scratch directories are empty

## Non-Functional Requirements

**Performance**
- Thumbnail within 5 s at p95 for interactive uploads (AC-10); measured in E-31/c44.

**Reliability**
- The decoder has no network, no credentials and no blob mount; the supervisor has no database login (checked by inspecting the stack).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Static caller check over non-test sources (server and worker) | Each named operation has a production caller; supervisor `main` starts the slot loop with real clients |

## Blackbox Tests

Integration through the real binaries and containers; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Test stack, JPEG orientation 6 | Commit, list, fetch thumbnail | WebP 320 and preview 1440, swapped, state ready | AC-10 |
| AC-2 | Three clips | Commit each | Transcode + poster; skip; remux | AC-11 |
| AC-3 | Hostile SVG, truncated HEIC | Commit | Job failed, original intact, decoder restarted | AC-25, AC-45 |
| AC-4 | Running supervisor | `doctor` | Worker status reported | |
| AC-71 | Photo and video jobs, test clock | SIGKILL at fetch, decode, post | One derived per key, no surplus, empty scratch | NFT-RES-36, FT-N-49 |

## Constraints

- Owner: component 12 (`src/server/`): API-side bindings for this stage and its end-to-end tests, run in the Compose test stack. No SQL or business rule in 12. The supervisor and decoder images come from E-20 and E-19 builds; this task changes neither crate.
- No fake decoder and no fake supervisor: both are the release images of the commit under test (test-clock feature on).
- Downstream: E-31/c26 (FT-P-20..27), E-31/c31 (FT-N-06, FT-N-07) and E-31/c41 (NFT-RES-36) should depend on this task.

## Risks & Mitigation

**Risk 1: Kill-and-retry duplicates outputs**
- *Risk*: a late post from the killed attempt and the retry both store a derived blob.
- *Mitigation*: AC-71 counts rows and blobs after the derived janitor.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-20 has no explicit child that composes the supervisor `main` (config, `WorkerApi`, `Scratch`, `DecoderClient`, slots, heartbeat); this task assumes E-20/c06 includes it | E-20 owner, confirm at Step 2 | open | Medium |
| Slot-loop boundaries and scratch sizes are still a spike (E-20/c01, G-09-1, G-09-4) | E-20 | open | Low |
