# Test Specification — 08 Media Jobs

Scope: ports `CommitEffect` (`MediaEnqueueEffect`), `WorkerAuthenticator`, `WorkerJobs` (`claim`, `extend_lease`, `release`, `complete_skipped`, `fail`, `open_input`, `accept_output`, `record_status`), `ThumbnailStateSource`, `DerivedOpener`, `MediaAdmin`, `AccountLifecycleHook`, `BlobOwnerCleanup`, the two `Janitor`s (`JobReaper`, `DerivedJanitor`), `DoctorCheck`/`HealthContributor`, and the pure `media-shared::policy` and `types`. The internal HTTP routes under `/api/v1/worker-jobs/**` on `:8081` are the wire form of `WorkerJobs`; both forms are exercised. System scenarios are cited by ID, not restated.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-10 | Thumbnail within 5 s (reserved lane), orientation, bulk 10,000 in 15 min | IT-08-1, IT-08-3, IT-08-4, PT-08-1, PT-08-2; sys FT-P-20, FT-P-21, FT-P-22, NFT-PERF-13, NFT-PERF-14 | Covered by this file (planning, lanes, priority) and system scenarios (latency) |
| AC-11 | Compliance rule, remux rule, never re-encode a compliant clip | IT-08-8, IT-08-17, IT-08-18; sys FT-P-23, FT-P-24, FT-P-25, NFT-PERF-15..17 | Covered by this file (policy functions on real probes) and system scenarios |
| AC-25 | Worker endpoints expose no secrets; decoder cannot reach DB or blobs (API side) | IT-08-16, ST-08-1, ST-08-2, ST-08-3; sys FT-N-07, NFT-SEC-28, NFT-SEC-29, NFT-RES-LIM-04 | Covered by this file and system scenarios |
| AC-28 | NVENC fallback uses the same output spec (v1.1) | IT-08-17 (spec unchanged by encoder); sys FT-N-08, FT-P-28 | Covered for the output spec; GPU behaviour is sys-only (H-GPU, v1.1) |
| AC-43 | Derived blobs encrypted like all blobs | IT-08-9; sys FT-P-40 | Covered by this file and sys FT-P-40 |
| AC-44 | Derived at most 20% of original (policy numbers) | IT-08-17; sys FT-P-26, NFT-PERF-20 | Covered by this file (constants snapshot) and system scenarios (ratio) |
| AC-45 | Attempts, lease and retry rule (API side) | IT-08-5, IT-08-6, IT-08-7, IT-08-8, IT-08-19, IT-08-20; sys FT-N-06, NFT-RES-19 | Covered by this file and system scenarios; one case BLOCKED (IT-08-19) |
| AC-71 | One derived file per (account, content hash, params) after any retry; owner | IT-08-2, IT-08-6, IT-08-10, IT-08-11, IT-08-14, AT-08-2; sys NFT-RES-36, NFT-RES-23 | Covered by this file and sys NFT-RES-36 |
| AC-72 | Worker fetch equals plaintext; wrong key stops start (worker side) | IT-08-12, PT-08-3; sys FT-N-43 | Covered by this file (input bytes) and sys FT-N-43 (1 GiB, key mismatch start) |
| AC-79 | Worker endpoints reject user and device tokens (403), absent on public route (owner of auth) | IT-08-16, ST-08-1; sys NFT-SEC-50 | Covered by this file (auth) and sys NFT-SEC-50; public route 404 is tested in 11 (IT-11-9) |
| AC-27 | Account erase removes jobs and derived rows | IT-08-15; sys FT-P-19, NFT-SEC-27 | Covered by this file and system scenarios |
| AC-47 | Thumbnails never SVG; no fallback to the original | IT-08-13; sys NFT-SEC-19 | Covered by this file and sys NFT-SEC-19 |
| AC-59 | Media routes in the two-account matrix | ST-08-4; sys NFT-SEC-13 | Covered by this file (port level) and sys NFT-SEC-13 |

AC-77 is not implemented here (derived shares follow the node through `open_derived_for`; 06/11 own it). AC-63 image CVEs are CI. No AC is without a test; one sub-case is BLOCKED (IT-08-19).

## Blackbox Tests

Harness `H1`: Rust integration binary with real PostgreSQL 18 (roles `sunnycloud_migrator`, `sunnycloud`), real 02, 03, 04 (temp XFS blob dir, per-run master key) and the 08 `media-api` crate, HTTP checks through a real `HttpServer` on `:8081`. Config: `job_lease=2s`, `job_attempts=3`, `job_retry_backoff=200ms`, `bulk_queue_threshold=5`, `job_terminal_ttl=3s`, derived caps `thumb=64KiB`, `preview=256KiB`, `transcode=2MiB` for cap tests, `worker_status_interval=1s`. Clock: injectable test clock. "Worker" in these tests is the test client holding `worker_api_token`; no supervisor is involved except AT-08-2.

### IT-08-1: Job planning matrix
**Summary**: `MediaEnqueueEffect::on_entry` plans jobs per media type inside the commit transaction and issues no I/O.
**Traces to**: AC-10, AC-11
**Input data**: published entries for JPEG, PNG, WebP, GIF, HEIC, MP4, MOV, MKV, SVG, `application/octet-stream`, a zero-byte file, and a vault file (`vault = true` with a JPEG type); each in its own transaction.
**Expected result**: photo types create one `stills` job (lane `image`, outputs `thumb` + `preview`); MP4/MOV/MKV create one `poster` job and one `transcode` job (lane `video`); SVG, unknown, zero-byte and vault create none; `on_batch` creates none; the unique key `(account, kind, content_hash, params_version)` holds; the effect makes at most the indexed probes (statement count from `pg_stat_statements`: 2 SELECT probes + 1 INSERT per job); `supported_kinds_for(media_type)` returns `[stills]` for photos, `[poster, transcode]` for video and `[]` for the rest, equal to what the planner enqueues.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL, 02 types

### IT-08-2: Idempotent enqueue and skip when derived exists
**Summary**: re-planning the same content creates no second job and no job when all outputs exist.
**Traces to**: AC-71
**Input data**: same entry planned twice; a second entry, same content hash, other revision; an entry whose `derived` row for `thumb` exists but `preview` does not; an entry with both rows present.
**Expected result**: 1 job after the first two plans; partial case enqueues the `stills` job; the full case enqueues nothing; changing `params_version` in the planner enqueues a new job for the same content.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-08-3: Bulk priority rules
**Summary**: `priority = 1` for `import_hint` or when the account has more than `bulk_queue_threshold` queued jobs.
**Traces to**: AC-10
**Input data**: entries with `import_hint = true`; an account with 5 queued jobs then a 6th and 7th entry (threshold 5); another account with 1 queued job.
**Expected result**: import entries priority 1; the 6th enqueue (6 queued before it is more than 5) gets priority 1, the 5th gets 0; the other account's job priority 0; the counting probe uses `LIMIT 6` (statement text check via `pg_stat_statements`).
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-08-4: Claim ordering and lane isolation
**Summary**: `claim(lane, worker_id)` orders by priority then `available_at` and never crosses lanes.
**Traces to**: AC-10
**Input data**: 10 bulk `stills` jobs (priority 1, earlier `available_at`), then 1 interactive `stills` job (priority 0, later), 3 `transcode` and 2 `poster` jobs; claims on lane `image` then `video`.
**Expected result**: the first `image` claim returns the interactive job despite later `available_at`; next `image` claims return bulk jobs in `available_at` order; `video` claims return only `poster`/`transcode`; empty lane returns `None` (HTTP 204, no body); each claim sets `running`, `attempts = 1`, `leased_by`, and `lease_ms` equals `job_lease`.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-08-5: Claim concurrency
**Summary**: no job is claimed twice; claims do not block on each other.
**Traces to**: AC-45
**Input data**: 500 `image` jobs; 20 concurrent claimers loop until 204.
**Expected result**: 500 distinct job ids returned in total, 0 duplicates; no claim call longer than 200 ms; every row `running` with one `leased_by`.
**Max execution time**: 20 s
**Dependencies**: PostgreSQL

### IT-08-6: Lease expiry makes a job claimable again
**Summary**: a running job with an expired lease is reclaimed; the old holder is refused with `LeaseLost`.
**Traces to**: AC-71, AC-45
**Input data**: worker W1 claims job J; test clock advances `job_lease + 1 s`; W2 claims; then W1 calls `extend_lease`, `accept_output`, `fail`, `complete_skipped`, `release` for J.
**Expected result**: W2 receives J with `attempt = 2`; every W1 call returns `LeaseLost` (HTTP 409 `lease_lost`); W2's `extend_lease` returns `Extended` with `lease_ms`; a third worker that never held J also gets 409 and no job data.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL, test clock

### IT-08-7: Failure dispositions and backoff
**Summary**: `fail` applies retry, backoff and the dead rule on the API clock.
**Traces to**: AC-45
**Input data**: job A `fail(Retry)` once; job B `fail(Retry)` three times (claiming between); job C `fail(Permanent)`; job D with `last_error` of 2,000 characters.
**Expected result**: A returns `Requeued`, `available_at = now + 200 ms` (doubling to 400 ms on the second failure), `claim` returns None before then and the job after; B is `Dead` at the third failure (`attempts >= 3`); C is `Dead` at the first; `last_error` stored length <= 500; `jobs_dead_total` increments for B and C.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL, metrics

### IT-08-8: Release and complete_skipped
**Summary**: graceful release restores the attempt; skipped outcomes close without output.
**Traces to**: AC-45, AC-11
**Input data**: claim J1 then `release`; claim J2 then `complete_skipped(skipped_compliant)`; claim J3 then `complete_skipped(obsolete)`.
**Expected result**: J1 `queued`, `attempts` back to 0, claimable immediately; J2 `succeeded` outcome `skipped_compliant`, 0 `derived` rows; J3 `succeeded` outcome `obsolete`; both calls by a non-holder return `LeaseLost`.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-08-9: accept_output stores an encrypted derived file and closes the job
**Summary**: valid outputs are stored via 04, registered, and the job succeeds after the last expected output.
**Traces to**: AC-71, AC-43
**Input data**: `stills` job J claimed; `PUT outputs/preview` (WebP 1440 px from DS-03), then `PUT outputs/thumb` (WebP 320 px); headers `X-Width`, `X-Height`; then a `transcode` job with an MP4 (DS-07 slice).
**Expected result**: first PUT 204, job still `running`; second PUT 204, job `succeeded` outcome `derived`; two `derived` rows with the given kind, media type, width and height; blob files on the volume (harness reads raw bytes through ctl-style access) do not begin with a WebP (`RIFF....WEBP`) or MP4 (`ftyp` at offset 4) signature; `blobs` rows `committed`; quota `used_bytes` unchanged.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL, 04, 07a

### IT-08-10: accept_output retry and race produce one derived file
**Summary**: a repeated or concurrent post yields `AlreadyPresent` and schedules the surplus blob for removal.
**Traces to**: AC-71
**Input data**: post the same `thumb` twice sequentially; then 8 concurrent posts of the same `thumb` for a fresh job; then a `stills` retry where `preview` is stored and `thumb` is not.
**Expected result**: sequential: 204 then 200 (`AlreadyPresent`); concurrent: exactly 1 of 8 returns 204, 7 return 200; one `derived` row per `(account, content_hash, kind, params_version)`; each surplus blob is in `blob_gc` with `Due::Now`; after 04's GC pass 0 unreferenced derived blob files remain; partial retry returns 200 for `preview` and 204 for `thumb`, job `succeeded` after the second.
**Max execution time**: 15 s
**Dependencies**: PostgreSQL, 04 GC

### IT-08-11: accept_output rejections
**Summary**: untrusted worker output is validated by lease, revision, size, type and kind.
**Traces to**: AC-71, AC-25
**Input data**: (a) body of 65 KiB for `thumb` (cap 64 KiB) and another with a lying `Content-Length` smaller than the stream; (b) a PNG for `thumb`, a text file labelled `Content-Type: video/mp4` for `transcode`, an MP4 for `thumb`; (c) `transcode` posted to a `stills` job; (d) a `kind` of `../x` and `unknown`; (e) revision deleted and retention passed; (f) expired lease.
**Expected result**: (a) 413 and no bytes kept (blob dir unchanged, no `derived` row); (b) 422 `OutputTypeRejected` based on sniffed bytes, whatever the header says; (c) 422 `OutputKindRejected`; (d) 422 or 404, no path effect; (e) 404 `revision_obsolete`; (f) 409 `lease_lost`; in all cases job state unchanged, no orphan row in `blobs` with state `committed`.
**Max execution time**: 15 s
**Dependencies**: PostgreSQL, 04

### IT-08-12: open_input returns the plaintext original
**Summary**: the worker input equals the uploaded bytes through the normal decrypt path.
**Traces to**: AC-72, AC-71
**Input data**: DS-03 photo (4 MB) and DS-07 clip (150 MB) uploaded through 03; job claimed; `GET input`; also a retained non-current revision (file overwritten after job creation, retention 7 d) and a revision whose retention has expired (test clock).
**Expected result**: `Content-Length` equals the file size exactly; BLAKE3 of the body equals the manifest `content_hash`; retained non-current (superseded or deleted but still retained) revision still served; collected (retention expired) or foreign revision 404 `revision_obsolete`; wrong holder 409; response has `Cache-Control: no-store`; a request with `Range: bytes=0-99` is ignored: 200, the full body and exact `Content-Length` (G-08-4).
**Max execution time**: 20 s
**Dependencies**: PostgreSQL, 02, 03, 04

### IT-08-13: Derived serving never falls back
**Summary**: `open_derived` and `open_derived_for` return the derived `BlobRef`, `NotReady` or `NotFound`, never the original.
**Traces to**: AC-47
**Input data**: photo with finished thumb; photo whose job is queued; SVG file; vault file; video without transcode (compliant); another account's node id.
**Expected result**: ready photo returns a `BlobRef` that reads back a WebP; queued photo `NotReady` (HTTP 404 `thumbnail_not_ready`); SVG and vault `NotFound` (`thumbnail_unavailable`), never an SVG body; compliant video `transcode` request `NotFound`; foreign account node `NotFound`; `open_derived_for(live_file, thumb)` equals `open_derived` for the same live file.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL, 02, 04

### IT-08-14: Thumbnail states
**Summary**: `states(account, hashes)` reports `Ready`, `Pending`, `Failed`, `None` in two batched probes.
**Traces to**: AC-10
**Input data**: 1,000 content hashes: 400 with `thumb`, 300 queued/running, 100 dead, 200 SVG/unsupported.
**Expected result**: map matches the seeded states exactly; `pg_stat_statements` shows 2 statements for the call (no per-hash queries); a vault file hash gives `None`; unknown hash gives `None`.
**Max execution time**: 3 s
**Dependencies**: PostgreSQL

### IT-08-15: Account erase hook and janitors
**Summary**: `erase_batch` drains jobs then derived rows with `GcQueue Due::Now`; janitors behave on schedule.
**Traces to**: AC-27, AC-71
**Input data**: account with 120 jobs (all statuses) and 200 derived rows; `erase_batch(account, 50)` until `remaining = 0`; separately: derived rows whose content has no remaining revision and others with a revision; `JobReaper` with terminal jobs older and newer than `job_terminal_ttl`; stale and fresh `worker_status` rows; `BlobOwnerCleanup::remove_owner_rows(tx, blob_id)` for a derived blob.
**Expected result**: each call deletes at most 50 rows; `remaining` decreases to 0; the account's derived blobs are in `blob_gc` `Due::Now`; other accounts untouched; `DerivedJanitor` removes only orphan-content rows (and queues GC); `JobReaper` deletes terminal rows older than TTL and `worker_status` not seen for TTL, leaves recent ones; `remove_owner_rows` deletes the derived row that owns the blob.
**Max execution time**: 20 s
**Dependencies**: PostgreSQL, 04

### IT-08-16: Worker authentication is uniform
**Summary**: every failure cause gives an identical 403; rotation works.
**Traces to**: AC-79, AC-25
**Input data**: requests to `claims`, `input`, `status` with: no header; `Authorization: Bearer` empty; wrong secret; a valid user session token; a valid device token; a session cookie; then `worker_api_token_next` configured and used; then the old token removed.
**Expected result**: all failures answer 403 with the same status, body and header set (compare after removing `X-Request-Id` and `Date`); a valid secret with a job it does not hold gets 409, never 403; during rotation both tokens work; after removal the old token gets 403; auth failures increment a counter and the log has no secret.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL, 11 listener

### IT-08-17: Policy decisions on real probes
**Summary**: `is_compliant` and `decide` follow the confirmed rule on probes produced by real ffprobe.
**Traces to**: AC-11, AC-44, AC-28
**Input data**: `Probe` JSON produced by the real decoder (`10` `probe`) for DS-07 (compliant), DS-35 seven edge clips (10.5 Mbit/s; 1920x1088; High 10; moov at end; HLG BT.2020; HEVC; MP3) and two controls (9.5 Mbit/s; portrait 1080x1920); synthetic boundary probes at exactly 10,000,000 bit/s, 1920x1080 and 1080x1920, profile Baseline/Main/High, no audio.
**Expected result**: DS-07 `SkipCompliant`; each of the six near-miss clips `Transcode(Plan)`; moov-at-end clip `Remux` (no parameters); both controls `SkipCompliant`; boundary values compliant (<= semantics, decimal Mbit/s: gap G-08-2); no audio is compliant; rotation metadata ignored; output spec constants of `Params[v1]` equal edge 320 and 1440, WebP, H.264 High, short side <= 1080, long <= 1920, AAC, faststart; with `encoder_pref = Nvenc` the plan changes only `encoder`, never the rest.
**Max execution time**: 60 s (probe of 9 clips)
**Dependencies**: decoder image (10), DS-07, DS-35

### IT-08-18: Poster offset
**Summary**: `poster_offset_ms(duration_ms)` follows the 1 s / 10% rule.
**Traces to**: AC-10
**Input data**: durations 0, 500, 5,000, 9,999, 10,000, 10,001, 600,000 ms.
**Expected result**: 0 -> 0, 500 -> 50, 5,000 -> 500, 9,999 -> 999, 10,000 -> 1,000, 10,001 -> 1,000, 600,000 -> 1,000; never above the duration.
**Max execution time**: 1 s
**Dependencies**: none (pure)

### IT-08-19: Claim of an exhausted job with an expired lease (BLOCKED)
**Summary**: whether `claim` may take a `running` row whose lease expired and whose `attempts` already equals `job_attempts` before `JobReaper` marks it `dead`.
**Traces to**: AC-45
**Input data**: job claimed 3 times with the lease expiring each time; the reaper is paused; a fourth `claim`.
**Expected result**: BLOCKED. The description says the reaper marks "expired lease and exhausted attempts" rows `dead` but the `claim` statement does not state it excludes them, so the fourth attempt may run. Required: the claim predicate must exclude rows with `attempts >= job_attempts` (expected result then: `None`, row `dead` after the next reap).
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-08-20: Worker status, doctor levels, requeue and stats
**Summary**: `record_status` upserts; `DoctorCheck` levels and `MediaAdmin` behave as specified.
**Traces to**: AC-45, AC-63
**Input data**: no `worker_status` row; a row with `decoder_ok = true, image_age_days = 3`; `image_age_days = 15`; `decoder_ok = false`; `seen_at` older than 3 intervals with and without queued jobs; 4 dead jobs (2 accounts, 2 kinds): `requeue_dead(account A)`, `requeue_dead(kind transcode)`, `stats()`.
**Expected result**: no row -> Warn "no worker" (Fail if jobs queued or dead exist); healthy row -> Ok; age 15 -> Warn; `decoder_ok = false` -> Fail; `seen_at` set by the API clock regardless of any client time; `requeue_dead` returns the count of matching rows and sets them `queued` with attempts reset; `stats()` returns counts per lane and status equal to a harness query.
**Max execution time**: 10 s
**Dependencies**: PostgreSQL, test clock

## Performance Tests

### PT-08-1: Claim throughput with a deep queue
**Summary**: the index-only claim stays fast with 100,000 queued rows.
**Traces to**: AC-10
**Load scenario**:
- Concurrent users: 50 claim loops (2 lanes)
- Request rate: closed loop with 204 polls allowed
- Duration: 3 minutes
- Ramp-up: 10 s

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Claim latency p95 | 5 ms | 20 ms |
| Claim latency p99 | 10 ms | 50 ms |
| Throughput | 1,000 claims/s | 300 claims/s |
| Error rate | 0% | 0% |

**Resource limits**:
- Database connections: 20; CPU of db host under 50%. Targets are proposed (the description gives none; two slots poll about 4 req/s), baseline replaced after the first run.

### PT-08-2: Interactive job passes a 10,000-job bulk backlog
**Summary**: an interactive job is the next claim regardless of backlog.
**Traces to**: AC-10
**Load scenario**:
- 10,000 bulk `stills` jobs queued (DS-04 hashes), 1 interactive job enqueued every 2 s for 60 s, 4 claim loops draining

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Interactive wait until claimed | 1 claim round (<= 500 ms) | 2 s |
| Bulk jobs claimed before an interactive job enqueued earlier | 0 | 0 |

**Resource limits**:
- The 5 s end-to-end thumbnail bound is measured by sys NFT-PERF-13/14, not here.

### PT-08-3: Input streaming rate
**Summary**: 1 GiB input streams at disk/decrypt speed with flat memory.
**Traces to**: AC-72
**Load scenario**:
- One 1 GiB dense file (DS-25), one worker `GET input`, on loopback (no WAN shaping)

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Throughput | 112 MB/s or more | 90 MB/s |
| API RSS growth | under 64 MiB | 200 MiB |
| BLAKE3 equals manifest | yes | no |

**Resource limits**:
- C-REF hardware only for the throughput bound; elsewhere `NOT-RUN(HW-REF)` and only the hash and RSS checks run. Floor taken from the AC-2 rate; same decrypt path as downloads.

## Security Tests

### ST-08-1: Worker credential confusion
**Summary**: no user or device credential reaches a worker route, and the worker secret reaches nothing else.
**Traces to**: AC-79, AC-25
**Attack vector**: a stolen device token or admin session used on `:8081`; the worker secret used on public routes.
**Test procedure**:
1. Call all 7 worker routes with an admin session token, a device token, a session cookie.
2. Call `GET /api/v1/accounts/me` and `GET /api/v1/admin/users` on the public listener with the worker bearer.
**Expected behavior**: step 1 uniform 403; step 2 401 (not a principal).
**Pass criteria**: 0 successful calls; responses of step 1 identical to the no-header response.
**Fail criteria**: any 2xx, or a difference that tells the causes apart.

### ST-08-2: Job-scoped authorization
**Summary**: an authenticated worker can act only on jobs it currently holds.
**Traces to**: AC-25, AC-71
**Attack vector**: a hostile or buggy supervisor reading other jobs' inputs or writing other jobs' outputs.
**Test procedure**: worker W2 (valid secret, own `X-Worker-Id`) calls `input`, `outputs/thumb`, `result`, `failure`, `lease` on a job leased by W1; and with W1's `X-Worker-Id` header spoofed while using its own connection.
**Expected behavior**: 409 `lease_lost`, no bytes of the input, no state change. A spoofed `X-Worker-Id` of the real holder is accepted by design (one shared secret, no per-worker auth; documented accepted risk).
**Pass criteria**: non-holder with its own id never gets data.
**Fail criteria**: any input byte or state change for a non-holder.

### ST-08-3: Hostile output content
**Summary**: sniffing and size caps hold against crafted outputs.
**Traces to**: AC-25
**Attack vector**: polyglot files, HTML/SVG labelled as WebP, oversized streams, decompression-style padding, chunked body with no length.
**Test procedure**: post to `thumb`: an SVG with script, an HTML page prefixed with `RIFF`-like bytes (valid first 12 bytes only), a 10 MiB zero stream via chunked encoding, a WebP followed by 1 MiB of trailing data; `Content-Type` always `image/webp`.
**Expected behavior**: 422 or 413 as applicable; the trailing-data WebP is rejected or accepted only if the cap and the dimension bound hold (recorded); nothing stored on rejection.
**Pass criteria**: 0 stored files for rejected posts; stream stopped at the cap (bytes read by the server <= cap + 1 buffer).
**Fail criteria**: SVG/HTML stored, or the full oversized stream consumed.

### ST-08-4: Cross-account thumbnails
**Summary**: user thumbnail and transcode routes only serve own nodes.
**Traces to**: AC-59
**Attack vector**: node id of another account on `/nodes/{id}/thumbnail`, `/transcode`, `thumbnail-states`.
**Test procedure**: account B requests A's node (ready thumbnail) on each route; `thumbnail-states` with A's node ids from B; body of 1,001 ids.
**Expected behavior**: 404 for foreign ids on the node routes; `thumbnail-states` omits foreign and unknown ids from the answer (no state leaked); 422 `batch_too_large` for 1,001 ids (G-08-3).
**Pass criteria**: 0 successful cross-account reads.
**Fail criteria**: any derived byte or state of A visible to B.

## Acceptance Tests

### AT-08-1: Interactive thumbnail while a bulk import runs
**Summary**: component-level view of the AC-10 flow with the real supervisor and decoders.
**Traces to**: AC-10
**Preconditions**:
- Compose `core` profile with the worker and both decoders; DS-04 subset of 500 photos queued as bulk; DS-03 photos.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Upload 500 DS-04 photos with `import_hint` | jobs priority 1 |
| 2 | Upload 10 DS-03 photos one by one | `thumbnail-states` shows `Ready` for each; no bulk job claimed before them |
| 3 | `GET /nodes/{id}/thumbnail?size=small` | 200 WebP, 320 px long edge, dimensions swapped for orientation 6 and 8 |

The 5 s p95 and 15 min bulk bound are sys NFT-PERF-13 and NFT-PERF-14 (`C-REF`).

### AT-08-2: Kill the supervisor mid-job
**Summary**: exactly one derived row per key after the retry, scratch empty.
**Traces to**: AC-71
**Preconditions**:
- Compose stack; job lease 10 s; DS-05 base clip uploaded; `e2e-ctl` kill action.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Wait for the transcode to run, `docker kill worker-supervisor`, then start it | lease expires after at most 10 s, job `queued` or reclaimed |
| 2 | Wait for job success | `attempts = 2`, one `derived` row of kind `transcode` for the content hash |
| 3 | ctl probe: derived blobs and scratch | exactly 1 derived blob file, scratch directory empty |

Full variants with kills at fetch, decode and post: sys NFT-RES-36.

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-03 | 100 JPEG, 20 HEIC, orientations 3/6/8 | `gen_photos.py` | about 450 MB |
| DS-04 subset | 500 bulk JPEG variants | `gen_photos_bulk.py --count 500` | about 1.5 GB |
| DS-07, DS-35 | compliant clip; 7 edge clips and 2 controls | `gen_video.py` | about 300 MB |
| DS-05 base | 30 s base clip of clip-1080-sdr | `gen_video.py clip-1080-sdr` base file | about 60 MB |
| DS-25 | 1 GiB dense file | `gen_bigfile.py --size 1GiB` | 1 GiB |
| factories | jobs in each state, derived rows, worker_status rows | harness factories through the real queue functions | small |

**Setup procedure**:
1. PostgreSQL 18 (pinned digest) in a container; `sunnycloud migrate` into `sc_template`; per test `CREATE DATABASE t_<id> TEMPLATE sc_template`, the SUT as role `sunnycloud`.
2. Per-test blob directory on XFS and a fresh master key; test clock at 0; jobs created through the real commit effect where possible, direct row inserts by the harness superuser only for states that cannot be reached quickly (dead, exhausted).
3. Real ffprobe/ffmpeg/libvips come from the decoder image (IT-08-17, AT-08-1/2); no codec is stubbed.

**Teardown procedure**:
1. Drop the database and blob directory; reset the clock offset; leak check that `jobs` and `derived` counts of surviving accounts equal their pre-test values.

**Data isolation strategy**: database per test from a template, blob directory per test, accounts per test; claim tests use unique accounts so lanes are not shared. No in-memory queue or fake store is used anywhere: the claim, `SKIP LOCKED`, unique-key and partial-index behaviour is the object under test.

## Spec gaps found

- G-08-1 (BLOCKED, IT-08-19): `claim` eligibility for expired-lease rows whose attempts are exhausted.
- G-08-2: whether "10 Mbit/s" is 10,000,000 bit/s (decimal) and whether the bound is inclusive.
- G-08-3: resolved 2026-11-07: foreign and unknown ids are omitted from the `thumbnail-states` answer; a body over 1,000 ids is 422 `batch_too_large`.
- G-08-4: resolved 2026-11-07: `Range` on `GET input` is ignored (always 200, full body).
