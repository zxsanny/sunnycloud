# Test Specification — 09 Worker Supervisor

Scope: the `sunnycloud-worker` process. 09 provides and requires no `contracts` ports; its interfaces are `WorkerApi` (HTTP client of the worker listener `:8081`, routes defined by 08), `DecoderClient` (Unix socket, `decoder_protocol` v1), `Scratch`, `TranscodePolicy`/`poster_offset_ms` (pure, from `media-shared`), `Clock`/`OffsetClock`, and the ops port `:9090` (`/healthz`, `/readyz`, `/metrics`). Tests observe the supervisor only through those boundaries: the real API on `:8081` (job rows read with the harness superuser connection), the decoder socket, the scratch directory, the ops port and the container. System scenarios are cited by ID, not restated.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-10 | Image-only interactive slot, one fetch for thumb and preview, ready within 5 s; bulk 10,000 in 15 min | IT-09-2, IT-09-9, PT-09-1; sys FT-P-20, FT-P-21, NFT-PERF-13, NFT-PERF-14 | Covered by this file (flow, slots, overhead) and system scenarios (latency) |
| AC-11 | Policy applied: compliant never re-encoded, index-at-end remuxed; throughput scales | IT-09-3, IT-09-4, PT-09-2, AT-09-1; sys FT-P-25, NFT-PERF-17 | Covered by this file and system scenarios |
| AC-25 | No creds to decoder; no DB login for the worker; self-check gate; hostile output handling | IT-09-1, IT-09-12, IT-09-13, ST-09-1, ST-09-2, ST-09-3; sys NFT-SEC-28, NFT-SEC-29, NFT-RES-LIM-04, NFT-SEC-46 | Covered by this file and system scenarios |
| AC-28 | NVENC only when enabled and probed; failures rerun on CPU (v1.1) | IT-09-16; sys FT-N-08, FT-P-28 | Covered by this file with a fake decoder; real GPU part is sys-only (H-GPU, v1.1) |
| AC-44 | Output compliance check keeps derived size in budget | IT-09-5; sys FT-P-26, NFT-PERF-20 | Covered by this file and system scenarios |
| AC-45 | Tier-2 watchdog, single CPU retry | IT-09-6, IT-09-10, IT-09-11; sys FT-N-06, NFT-RES-19, NFT-PERF-21 | Covered by this file and system scenarios |
| AC-71 | Kill of the supervisor leaves one derived per key and empty scratch | IT-09-14, AT-09-2; sys NFT-RES-36, NFT-RES-23 | Covered by this file and sys NFT-RES-36 |
| AC-72 | Hash-verified fetch (worker side) | IT-09-7; sys FT-N-43 | Covered by this file and sys FT-N-43 |
| AC-79 | Client side: worker secret and URL only; 403 stops claiming | IT-09-10, ST-09-1; sys NFT-SEC-50 | Covered by this file and sys NFT-SEC-50 |

Not testable at 09's boundary: AC-63 (image rebuild cadence and CVEs) is a CI gate; the graph test that the image has no `sqlx`, no database URL and no media library is IT-09-1 (image content) plus a CI crate-graph job.

## Blackbox Tests

Harness `H2`: Compose test stack (profile `core`, test clock on) with real PostgreSQL 18, real API (08 + 11), real supervisor image; the decoder is either `FD` (a fake decoder written for these tests, listening on the real Unix socket with the real `decoder_protocol`, scripted per case and recording every request) or `RD` (the real `decoder-interactive`/`decoder-general` containers with real vips/ffmpeg). FD is used only to script faults and to check protocol-level behaviour; every case that claims a media outcome uses RD. Config for tests: `job_lease=6s` (heartbeat 2 s), `worker_claim_poll_interval=100ms`, `worker_status_interval=1s`, `decoder_silence_timeout=1s`, `decoder_cancel_grace=500ms`, `photo_job_timeout=5s` unless a case says otherwise, `worker_shutdown_grace=3s`.

### IT-09-1: Start-up gate and self-check
**Summary**: the supervisor sweeps scratch, handshakes, runs `self_check` and takes jobs only when everything passes.
**Traces to**: AC-25
**Input data**: scratch roots pre-filled with leftover job trees and a stray file; FD returns `hello` OK and a `SelfCheckReport` with all items ok; then a run where one item (`no_network`) reports `ok = false`; then a run where the image is inspected with `docker inspect`/`ctl inspect`.
**Expected result**: leftovers deleted before the first `claim`; with all ok: `/readyz` 200, `worker_selfcheck_ok 1`, claims start; with one failed item: 0 claim requests reach the API (API access log), `/readyz` 503, `worker_selfcheck_ok 0`, next `PUT /worker-jobs/status` carries `decoder_ok = false`; image inspection: no `db_*` env, no secret except `worker_api_token`, no blob mount, no `ffmpeg`/`vips` binaries, no `sqlx` symbols in the binary (CI graph test result referenced).
**Max execution time**: 30 s
**Dependencies**: API, FD, ctl inspect

### IT-09-2: Photo job, one fetch two renders
**Summary**: a `stills` job fetches the input once and posts `preview` then `thumb`.
**Traces to**: AC-10
**Input data**: DS-03 JPEG uploaded; RD interactive decoder; API access log and FD/RD request log.
**Expected result**: exactly 1 `GET input`; render requests `max_edge 1440` then `max_edge 320`; `PUT outputs/preview` then `outputs/thumb`; job `succeeded` outcome `derived`; scratch dir for the job removed; `jobs_total{kind=stills,result=ok}` incremented; heartbeat PUTs appear while the job runs and stop after it.
**Max execution time**: 20 s
**Dependencies**: API, RD

### IT-09-3: Video poster and transcode decisions
**Summary**: the supervisor computes the poster offset and applies `TranscodePolicy::decide` to the probe.
**Traces to**: AC-11, AC-10
**Input data**: FD returns scripted probes: 9 s clip, 60 s clip (poster); DS-07 compliant probe; DS-35 moov-at-end probe; DS-35 10.5 Mbit/s probe; 1080x1920 portrait compliant probe.
**Expected result**: poster `render_still{max_edge 320, poster_at_ms}` with 900 for the 9 s clip and 1,000 for the 60 s clip, posted as `outputs/thumb`; compliant (and portrait) probe -> no `transcode` call, `PUT result {skipped_compliant}`, 0 output PUTs; moov-at-end -> `transcode` with `Plan::Remux`; 10.5 Mbit/s -> `transcode` with a `Transcode` plan (`encoder Cpu`, `faststart`, `tone_map false`); `Plan` JSON contains only enum fields, never a string argument.
**Max execution time**: 20 s
**Dependencies**: API, FD

### IT-09-4: Real remux and skip on real media
**Summary**: the same decisions with the real decoder on real clips.
**Traces to**: AC-11
**Input data**: DS-07, DS-35 moov-at-end clip, DS-35 portrait control; RD general decoder.
**Expected result**: DS-07 and portrait control close `skipped_compliant` with 0 derived rows and 0 bytes posted; the moov-at-end clip yields one `transcode` derived file whose packets hash equals the input's (checked by the harness with ffprobe on the downloaded derived file) and `moov` first; metric `jobs_total{result=remuxed}` = 1.
**Max execution time**: 120 s
**Dependencies**: API, RD, DS-07, DS-35

### IT-09-5: Output compliance check
**Summary**: a non-compliant decoder output fails the job once and does not loop.
**Traces to**: AC-44, AC-11
**Input data**: FD answers `transcode` with `probe_of_output` of 12 Mbit/s H.264 (non-compliant) and a file `out/result.mp4`.
**Expected result**: no `PUT outputs/transcode`; `fail(Permanent)` with code `OutputInvalid`; job `dead` after one attempt; FD saw exactly 1 `transcode` call (no CPU rerun for `OutputInvalid`).
**Max execution time**: 15 s
**Dependencies**: API, FD

### IT-09-6: Retry inside the claim
**Summary**: `Stalled` and `EncoderFailed` rerun once on CPU; permanent codes close at once.
**Traces to**: AC-45, AC-28
**Input data**: FD scripts per job: (a) `Stalled` then ok; (b) `Stalled` twice; (c) `EncoderFailed{exit_code: 1}` with `Plan.encoder = Nvenc` then ok; (d) `Remux` fails then ok; (e) `Unsupported`; (f) `Corrupt`; (g) `LimitExceeded`; (h) socket closed mid-job; (i) fetch 5xx.
**Expected result**: (a) second `transcode` has a fresh `out/` and `encoder Cpu`, job `succeeded`; (b) `fail(Permanent)`, job `dead`; (c) rerun with `Cpu`; (d) rerun as CPU `Transcode`, not `Remux`; (e)-(g) `fail(Permanent)` -> `dead` at first attempt, `attempts = 1`; (h) and (i) `fail(Retry)` -> `queued` with backoff; every rerun uses `out/` emptied beforehand.
**Max execution time**: 60 s
**Dependencies**: API, FD

### IT-09-7: Input fetch verification and limits
**Summary**: the fetch hashes while streaming and refuses mismatches and oversize input before the decoder sees it.
**Traces to**: AC-72, AC-25
**Input data**: (a) job whose `content_hash` differs from the stored revision (harness edits the job row); (b) 2 MiB photo with `worker_max_input_photo=1MiB`; (c) a body that grows past its `Content-Length`; (d) a clean photo; (e) input larger than free `statvfs` space of the scratch mount (small tmpfs).
**Expected result**: (a) `HashMismatch`, `fail(Retry)`, FD saw 0 requests, scratch removed; (b) refused from the declared length before the first byte is written (scratch `source` size 0), `fail(Permanent)` `TooLarge`; (c) stream aborted at the limit; (d) `Content-Length`-exact `source`, mode 0440, group `decoder_gid`, BLAKE3 per 2 MiB block rolled up equals `content_hash`; (e) job refused before download; the failure disposition is not specified (gap G-09-2), the test asserts only that nothing is posted and the job is not left `running`.
**Max execution time**: 30 s
**Dependencies**: API, FD

### IT-09-8: Gone revision closes the job as obsolete
**Summary**: a 404 `revision_obsolete` on input closes the job without retry.
**Traces to**: AC-71
**Input data**: photo job; the harness deletes the revision and advances the clock past retention before the fetch.
**Expected result**: `PUT result {obsolete}`; job `succeeded` outcome `obsolete`; no decoder call; no retry; scratch removed; `jobs_total{result=obsolete}` = 1.
**Max execution time**: 15 s
**Dependencies**: API, test clock

### IT-09-9: Slots and lanes
**Summary**: the interactive slot claims only `image`; the general slot alternates lanes.
**Traces to**: AC-10
**Input data**: 20 queued `poster`/`transcode` jobs and 20 `stills` jobs; one supervisor with two FD sockets (interactive, general), each FD answering after 300 ms.
**Expected result**: the interactive slot's claim requests all have `lane=image`; the general slot's lane sequence alternates `video, image, video, image, ...` while both lanes have work; both lanes progress (first 10 claims of the general slot contain at least 4 of each lane); at most one job per slot at a time; a priority 0 `stills` job enqueued behind bulk is claimed by the next free image-capable slot.
**Max execution time**: 30 s
**Dependencies**: API, FD x2

### IT-09-10: Lease heartbeat, 409 and 403
**Summary**: the supervisor heartbeats at `lease_ms / 3`, stops quietly on 409, and stops claiming on 403.
**Traces to**: AC-45, AC-79
**Input data**: FD job lasting 8 s; case A: harness expires the lease (clock advance) mid-job; case B: harness replaces the API's worker token mid-run so the next call returns 403.
**Expected result**: heartbeats `PUT /lease` every 2 s +/- 500 ms; case A: after the 409 the supervisor sends `cancel` to the decoder within 1 s, posts no output, no `result`, no `failure` for that job (API log), removes scratch, keeps polling with the old worker id; case B: first 403 marks the worker not ready (`/readyz` 503), 0 further `claims` requests until restart, status PUT also stops, log has no token value.
**Max execution time**: 40 s
**Dependencies**: API, FD, test clock

### IT-09-11: Two-tier watchdog
**Summary**: a silent runner is cancelled and the slot marked unhealthy; the wall limit is enforced here too.
**Traces to**: AC-45
**Input data**: FD stops sending any frame for 3 s (`decoder_silence_timeout=1s`); another FD keeps sending `Alive` but never answers a photo render (`photo_job_timeout=2s`); a third FD answers `Stalled` (tier 1 handled by the decoder, 10).
**Expected result**: silent case: `cancel` at about 1 s, after `decoder_cancel_grace` (500 ms) the slot is unhealthy (no further claims for that slot, `/readyz` 503) and `fail` is transient (job `queued`); wall case: `cancel` at <= 2.5 s, same outcome; stalled case: handled as in IT-09-6.
**Max execution time**: 40 s
**Dependencies**: API, FD

### IT-09-12: Output files are hostile input
**Summary**: `out/` reads reject symlinks, hardlinks, FIFOs, devices and oversize files without blocking.
**Traces to**: AC-25
**Input data**: FD (acting as a compromised decoder) writes `out/result.webp` as: (a) symlink to `/run/secrets/worker_api_token`; (b) a hard link (`nlink = 2`) to a file in `in/`; (c) a FIFO; (d) a sparse file larger than the cap; (e) a regular file with the right size and WebP bytes (control); (f) a directory.
**Expected result**: (a)-(d) and (f) rejected at `fstat`/open without reading content; no `PUT outputs/*` carries any byte of the secret; the FIFO case returns within 1 s (no blocked slot); next job on the slot still runs; (e) posted. The failure code for a rejected output is not named in the description (gap G-09-3); the test asserts only that the job does not succeed from the rejected file and ends `queued` or `dead`.
**Max execution time**: 30 s
**Dependencies**: API, FD

### IT-09-13: Hostile frames
**Summary**: oversize, malformed and out-of-range frames never crash the supervisor.
**Traces to**: AC-25
**Input data**: FD sends a frame of 64 KiB + 1; a frame with an unknown JSON field; `width = 0`, `width = 4294967295`, negative `duration_ms`; unknown media type enum; invalid UTF-8; a frame that announces a length and then stalls.
**Expected result**: each treated as a protocol violation of the decoder: slot unhealthy, `fail(Retry)` for the current job, reconnect with backoff and `hello`/`self_check` repeated before claiming; supervisor process stays up (container start time unchanged), memory growth under 10 MiB.
**Max execution time**: 40 s
**Dependencies**: API, FD

### IT-09-14: Kill the supervisor at each step
**Summary**: a SIGKILL at fetch, probe, decode or post leaves a clean end state after restart.
**Traces to**: AC-71
**Input data**: RD with DS-05 base clip (transcode) and DS-03 photo; `ctl` kills the supervisor container at: during `GET input`, during `transcode`, during `PUT outputs`; then starts it; lease 6 s.
**Expected result**: after restart `sweep_all` empties both scratch roots before the first claim; the job is reclaimed after lease expiry (`attempts = 2`); end state: job `succeeded`, exactly 1 `derived` row and 1 derived blob per `(account, content_hash, kind, params_version)`, 0 files in scratch, no running ffmpeg process in the decoder container; the killed run's decoder is cancelled by socket loss within 2 s (CPU idle).
**Max execution time**: 180 s total
**Dependencies**: API, RD, ctl

### IT-09-15: Reconnect after a runner exit
**Summary**: after the decoder runner exits, the supervisor reconnects and repeats `hello` and `self_check` before claiming.
**Traces to**: AC-25
**Input data**: RD interactive decoder; ctl kills the runner while idle; FD variant closes the socket and refuses connections for 3 s.
**Expected result**: during the outage no claim for that slot; request order on the new connection is `hello`, `self_check`, then the first `claim`; reconnect attempts back off (intervals non-decreasing); `worker_decoder_restarts_total` increments by 1.
**Max execution time**: 60 s
**Dependencies**: API, RD or FD, ctl

### IT-09-16: Encoder choice [v1.1]
**Summary**: `Nvenc` is planned only when `nvenc = on` and the decoder reports a successful probe.
**Traces to**: AC-28
**Input data**: FD `hello` with `nvenc_probe_ok` true and false; config `nvenc` off and on; non-compliant probe.
**Expected result**: only (`on`, probe ok) gives `Plan.encoder = Nvenc`; others `Cpu`; `EncoderFailed` after `Nvenc` -> CPU rerun (IT-09-6c).
**Max execution time**: 15 s
**Dependencies**: API, FD

### IT-09-17: Shutdown and status
**Summary**: SIGTERM drains within the grace period; status reports decoder health and image age.
**Traces to**: AC-45, AC-71
**Input data**: FD job of 8 s with `worker_shutdown_grace=3s`; another job of 1 s; FD `build_timestamp` 20 days before the test clock.
**Expected result**: after SIGTERM: no new `claims`; the 1 s job completes and posts; the 8 s job gets `cancel` at 3 s and `DELETE /lease`; job `queued` with `attempts` decremented; exit code 0 within 5 s; status PUT body has `decoder_ok = true` and `image_age_days = 20` every second (`worker_status_interval`), API `worker_status.seen_at` equals API clock time.
**Max execution time**: 30 s
**Dependencies**: API, FD

### IT-09-18: Test clock (test build only)
**Summary**: `OffsetClock` polls the API offset in the test build, moves only `image_age_days`, and is absent from release.
**Traces to**: AC-71
**Input data**: test image with FD `build_timestamp` = start time minus 20 days and offset 0; a long FD job running (30 s, `photo_job_timeout` default) with heartbeats; then `POST /test/clock {"advance_seconds": 3600}` (1 hour) and later `{"advance_seconds": 259200}` (3 days) through ctl; release image of the same version.
**Expected result**: test supervisor requests `GET /internal/v1/test/clock-offset` with the worker bearer (API log, at most 1 per s); before any advance the status PUT body has `image_age_days = 20`; after the 1-hour advance it is still 20; within `worker_status_interval` (1 s) plus one poll after the 3-day advance it is 23 and stays 23; across both advances the running job is not disturbed: heartbeats keep their 2 s +/- 500 ms cadence, no `cancel`, no `decoder_kills_total`, `/readyz` stays 200, and log line timestamps stay within 2 s of host time (not offset); release supervisor makes 0 requests to that route, and the binary has no `OffsetClock` symbol (string check). **Pass criterion**: `image_age_days` shifts by exactly the advanced whole days and no other supervisor timing changes.
**Max execution time**: 60 s
**Dependencies**: API test image, FD, ctl

## Performance Tests

### PT-09-1: Supervisor overhead on photo jobs
**Summary**: fetch, handshake and two posts cost little beside the decoder.
**Traces to**: AC-10
**Load scenario**:
- 2,000 `stills` jobs of DS-03 size, FD answering each render in 50 ms, one supervisor, API local

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Supervisor time per job above FD time | 40 ms | 150 ms |
| Jobs per second (FD at 50 ms/render) | 6 | 3 |
| Supervisor RSS | 64 MiB | 128 MiB |

**Resource limits**:
- CPU of the supervisor under 50% of one core. Targets are proposed (the budget is 15 min for 10,000 photos with 2 slots); system timing is sys NFT-PERF-14 (`C-REF`).

### PT-09-2: Two supervisors scale
**Summary**: a second supervisor/decoder pair raises throughput.
**Traces to**: AC-11
**Load scenario**:
- 400 jobs, FD taking 200 ms each, run with 1 then 2 supervisors on the same queue

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Throughput ratio 2 vs 1 | 1.9 | 1.7 |
| Duplicate claims | 0 | 0 |

**Resource limits**:
- The 1.7x bound with real transcodes is sys NFT-PERF-17 (REF-CPU4).

### PT-09-3: Streaming memory
**Summary**: a large input does not buffer in memory.
**Traces to**: AC-25
**Load scenario**:
- DS-25 1 GiB dense file as a video input, `worker_max_input_video` default, one supervisor

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Peak RSS | 64 MiB | 128 MiB |
| Hash equals manifest | yes | no |

**Resource limits**:
- Container `mem_limit` as in Compose.

## Security Tests

### ST-09-1: Secret handling
**Summary**: the worker secret appears nowhere but the Authorization header.
**Traces to**: AC-79, AC-25
**Attack vector**: log, metric, status or error-body disclosure.
**Test procedure**: run IT-09-2, IT-09-10 (case B) and IT-09-13 with `worker_api_token` set to a known 48-character marker; grep supervisor logs, `/metrics`, `/readyz` bodies, the status PUT body and API-side log lines.
**Expected behavior**: no occurrence outside the header; no `Debug` print of the token.
**Pass criteria**: 0 matches.
**Fail criteria**: any match.

### ST-09-2: No path to the database or blobs
**Summary**: the container has no credential, secret or mount beyond the worker token.
**Traces to**: AC-25
**Attack vector**: a compromised supervisor looking for data access.
**Test procedure**: `ctl inspect` of `worker-supervisor`: env, mounts, secrets, capabilities, networks; attempt `psql` is not possible (no client), so a TCP connect to `db:5432` plus a startup message with user `sunnycloud` and an empty password.
**Expected behavior**: env has no `db_*`/`SUNNYCLOUD_DB_*`; only `worker_api_token` under `/run/secrets`; no blob mount; TCP reachable by network design but authentication fails for any role; no Docker socket.
**Pass criteria**: all checks hold.
**Fail criteria**: any credential or mount found.

### ST-09-3: Decoder output cannot exfiltrate files
**Summary**: a malicious `result.webp` cannot cause upload of host files.
**Traces to**: AC-25
**Attack vector**: symlink, hardlink, `/proc/self/fd` tricks, TOCTOU swap between `fstat` and reopen.
**Test procedure**: IT-09-12 cases plus a FD that swaps `out/result.webp` between a regular file and a symlink 10,000 times while the supervisor reads it.
**Expected behavior**: the posted body is always either the regular file's bytes or nothing; never bytes of the symlink target.
**Pass criteria**: 0 posts containing the secret marker in 100 runs.
**Fail criteria**: one.

## Acceptance Tests

### AT-09-1: Compliant clip leaves no output; index-at-end clip is remuxed
**Summary**: component acceptance for the policy outcome.
**Traces to**: AC-11
**Preconditions**:
- Compose `core` stack with real worker and decoders; DS-07 and DS-35 moov-at-end clip uploaded.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Upload DS-07 | transcode job closes `skipped_compliant`; `GET /nodes/{id}/transcode` 404; 0 derived transcode bytes |
| 2 | Upload the moov-at-end clip | derived transcode exists, streams identical to the original, index first |

### AT-09-2: Kill-and-recover with two supervisors
**Summary**: no loss and no duplicates; scratch empty.
**Traces to**: AC-71
**Preconditions**:
- Two supervisor/decoder pairs; 20 transcode jobs from DS-08 subset.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Kill one supervisor at random during decode | the other keeps running |
| 2 | Restart it, wait for all jobs | every job `succeeded` or `dead` with a hostile-class code; exactly 1 derived blob per key |
| 3 | List both scratch volumes | empty |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-03 | JPEG/HEIC photos, orientations 3/6/8 | `gen_photos.py` | about 450 MB |
| DS-05 base | 30 s clip-1080-sdr base file | `gen_video.py` | about 60 MB |
| DS-07, DS-35 | compliant clip, edge clips and controls | `gen_video.py` | about 300 MB |
| DS-08 subset | 20 mixed clips | `gen_video.py mixed` | about 800 MB |
| DS-25 | 1 GiB dense file | `gen_bigfile.py` | 1 GiB |
| FD scripts | JSON scripts of frames/probes per case | written with the tests | < 1 MB |

**Setup procedure**:
1. Start the `core` stack with the test image; seed an account through an invite; take the golden DB snapshot.
2. Per test: restore the snapshot (or use a fresh account), empty both scratch volumes, set the clock offset to 0; for FD cases start FD on the decoder socket volume instead of the decoder container.

**Teardown procedure**:
1. Stop FD, reset the clock, `ctl` account purge; leak check of blob volume bytes and scratch directories (must be empty).

**Data isolation strategy**: an account per test, unique content (a seeded 8-byte marker appended to each file) so the unique job key never collides across tests; DB reset from the golden snapshot for exclusive groups (kills, clock). The queue, API and Postgres are always real; FD replaces only the decoder, and only in cases that script faults.

## Spec gaps found

- G-09-1: resolved 2026-11-11 (AZ-3867, agent decision): the loop type is the implementer's choice; the slot boundaries tests rely on are listed in 09 section 3 item 2 (one job per slot, lane mask, own heartbeat, own socket and scratch, observable only through API, socket, scratch and ops port).
- G-09-2: the outcome when an input does not fit the scratch free space (`statvfs` refusal) is not named (permanent `TooLarge` or transient).
- G-09-3: the `DecodeError`/failure code used when an output file fails the safe-open check is not named.
- G-09-4: resolved 2026-11-11 (AZ-3867, agent decision): only `image_age_days` uses the offset clock; durations stay monotonic and log timestamps stay on system time (09 section 2, IT-09-18).
