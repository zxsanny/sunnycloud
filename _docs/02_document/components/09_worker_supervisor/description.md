# 09 Worker Supervisor

## 1. High-Level Overview

**Purpose**: The trusted `sunnycloud-worker` process. It claims media jobs from the API over HTTP under a lease, fetches the decrypted original from the API's worker listener into a scratch file, probes it through the decoder, applies the transcode policy from `media-shared`, hands the work to a decoder over a Unix socket, enforces limits and the stall watchdog, posts the outputs back to the API and closes the job. One responsibility: *drive one job at a time per decoder slot from claim to terminal state, without ever parsing media bytes itself*.

**Architectural pattern**: Per-slot work loop (`claim -> fetch -> probe -> decide -> decode -> post -> close`) over three traits (`WorkerApi`, `DecoderClient`, `Scratch`) plus the pure `TranscodePolicy` function. Constructor injection, no statics.

**Holds**: the worker bearer secret (`worker_api_token`) and nothing else secret. **Does not hold**: any database credential (S4-1: no `sunnycloud_worker` role, no `db_worker_url`, no sqlx), master key, blob mount, provider secrets, any media library (no ffmpeg/libvips in this image).

**Crate dependencies**: `media-shared` (types, DTOs, `policy`, `decoder_protocol` client side) and helpers (config, error, clock, ids_hashing `hash_block`/`rollup`). It depends on `media-shared` only, not on 08 or any other component crate, and on no `contracts` port: it is a leaf process whose peers are the API's worker listener and the decoder sockets. It has no `sqlx` dependency (crate graph test).

**Downstream consumers**: none. Observed by `sunnycloud doctor` through the status it sends to the API (decoder self-check result and image age, stored by 08 in `worker_status`) and by `/readyz` on the worker's ops port. Remote workers on other hosts are out of v1.0: the supervisor runs on the same Compose host as the API and `:8081` is plaintext on the internal network.

**Scope in**: claim/heartbeat/close logic as a client of the worker API; input fetch with hash verification; scratch lifecycle and start-up sweep; self-check gate; probing and applying the transcode policy; supervision of the decoder (silence timeout, wall limit, cancel, reconnect after a runner exit); GPU/CPU fallback; output upload; worker status; metrics; graceful shutdown; the test-build clock (section 2).
**Scope out**: any media parsing or encoding (10), queue SQL, enqueue, derived rows, output sniffing and encryption, reaper (08), container limits (Compose), authentication of the secret (08 `WorkerAuthenticator`, served by 11 on `:8081`).

## 2. Ports

09 provides and requires no `contracts` ports. Interfaces it uses:

| Interface | Peer | Provided by |
|---|---|---|
| `WorkerApi` (HTTP to `:8081`) | API worker listener | server side: 08 `WorkerJobs` behind 11 |
| `DecoderClient` (Unix socket) | 10_decoder | `media-shared::decoder_protocol` (owned by 10) |
| `TranscodePolicy`, `poster_offset_ms` (pure) | none | `media-shared::policy` |
| `Clock` | helper 03 | `SystemClock`; in test builds `OffsetClock` |

### WorkerApi (HTTP to the worker listener `:8081`, backend network, no TLS inside Compose; routes are defined by 08)
Every request carries the bearer secret and `X-Worker-Id`.

| Operation | Request | Result | Errors |
|---|---|---|---|
| `claim(lane)` | `POST /api/v1/worker-jobs/claims` | `Option<ClaimedJob{job_id, kind (stills\|poster\|transcode), content_hash, params_version, attempt, lease_ms}>` (204: nothing due) | `Transient` |
| `heartbeat(job)` | `PUT /api/v1/worker-jobs/{job_id}/lease` every `lease_ms / 3` | `Extended` | `LeaseLost` (409) |
| `release(job)` | `DELETE /api/v1/worker-jobs/{job_id}/lease` | `()` | |
| `fetch_input(job)` | `GET /api/v1/worker-jobs/{job_id}/input` | bytes streamed into `in/source`; BLAKE3 per 2 MiB block computed on the fly, rolled up with helper 05 and compared with `ClaimedJob.content_hash`; `Content-Length` checked against the input limit before the first byte, and enforced while streaming | `Gone` (404: close as `obsolete`), `LeaseLost` (409), `Forbidden` (403: bad secret), `HashMismatch`, `TooLarge`, `Transient` |
| `post_output(job, kind)` | `PUT /api/v1/worker-jobs/{job_id}/outputs/{kind}` with `Content-Type`, `X-Width`, `X-Height`, `X-Duration-Ms` | 204 accepted, 200 already present (both success) | `LeaseLost`, `Gone`, `TooLarge` (413), `TypeRejected` (422), `Transient` |
| `complete(job, outcome)` | `PUT /api/v1/worker-jobs/{job_id}/result`, `skipped_compliant \| obsolete` | `()` | `LeaseLost` |
| `fail(job, code, Retry\|Permanent)` | `POST /api/v1/worker-jobs/{job_id}/failure` | `Requeued \| Dead`; `Permanent` goes straight to `dead` so hostile or unsupported media does not burn three attempts; backoff is applied by the API | `LeaseLost` |
| `report_status()` | `PUT /api/v1/worker-jobs/status` every `worker_status_interval` with `{decoder_ok, image_age_days}` | `()` | `Transient` (logged, retried at the next interval) |

`403` means the secret is wrong: the worker marks itself not ready and stops claiming. `409 lease_lost` means another worker owns the job or the lease expired: stop quietly and post nothing more for that job. `succeeded` with outcome `derived` is written by the server when the last expected output is accepted. All backoff and lease time is the API's; the supervisor measures only local durations.

### DecoderClient (Unix socket, `decoder_protocol` v1)
`hello`, `self_check`, `probe`, `render_still`, `transcode`, `cancel`. Returns the typed `DecodeError` of 10 (never strings). Frames are length-prefixed JSON, max 64 KiB, `deny_unknown_fields`, every number range-checked: the decoder is untrusted. The runner may exit after a job (10: hostile-class outcome or watchdog abort); the client then reconnects with backoff and repeats `hello` and `self_check` before the slot claims again.

### TranscodePolicy (pure, from `media-shared::policy`; applied here)
`decide(probe, caps: DecoderCapabilities, encoder_pref) -> SkipCompliant | Remux | Transcode(Plan)`. Compliant (decisions 2, 3, 14): H.264 High or lower, long side <= 1920 and short side <= 1080 (orientation independent), <= 10 Mbit/s, AAC or no audio, MP4 with `moov` first, SDR BT.709; all must hold -> `SkipCompliant`. Everything compliant except `moov` at the end -> `Remux` (lossless: copy streams, index moved to the front). Otherwise `Transcode(Plan)`. `Plan` is an enum-only record (`encoder Cpu|Nvenc`, `scale_to_short_edge`, `tone_map`, `audio`, `faststart`; `Remux` carries no parameters): the decoder never gets free-form ffmpeg arguments. `Nvenc` only when `nvenc = on` and `caps.nvenc_probe_ok` [v1.1]. The same `is_compliant` is applied to the probe of the decoder's *output*; a non-compliant output fails the job (`OutputInvalid`) so a re-run cannot loop.

### Scratch
`create(job_id) -> ScratchDir` with `in/` (supervisor writes `source`, mode 0440, group `decoder_gid`) and `out/` (decoder writes one fixed name `result.<ext>` per call; the supervisor posts and deletes it before the next render of a `stills` job); `remove()`/`Drop` delete the tree; `sweep_all()` at start-up deletes everything (a supervisor/decoder pair owns its scratch exclusively; AC-71). Supervisor and decoder share one numeric group (`decoder_gid`, set in Compose) for the socket and the scratch tree, so no world-readable mode is needed.
**Reads of `out/` are hostile-input reads**: resolve with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` and `O_PATH | O_NOFOLLOW`, `fstat` that the object is a regular file with `st_nlink == 1` and `st_size <= cap`, and only then reopen it through `/proc/self/fd/N` with `O_RDONLY | O_NONBLOCK`. A FIFO, device or symlink planted by a compromised decoder is rejected at `fstat` (an `open` on a FIFO would block the slot), and the supervisor cannot be made to upload `/run/secrets/*` as a "thumbnail".

### Test clock (test images only, feature `test-clock`)
`OffsetClock` replaces `SystemClock`: it polls `GET /internal/v1/test/clock-offset` on the worker listener (worker bearer, at most once per second, helper 03) and adds the returned offset to wall time, so the supervisor and the API share one offset. Release builds do not contain it.
**Effect inside the supervisor (decision, AZ-3867)**: exactly one supervisor output reads wall time through `Clock::now()`: `image_age_days` in the status report, `floor((Clock::now() - decoder build_timestamp) / 86,400 s)`. Advancing the test clock by N whole days therefore raises the reported `image_age_days` by N within one `worker_status_interval` plus one poll; an advance of less than the remainder of the current day leaves it unchanged. Nothing else moves: heartbeat cadence (`lease_ms / 3`), claim polling, `decoder_silence_timeout`, `decoder_cancel_grace`, `decoder_stall_kill`, the wall limits, `worker_shutdown_grace` and the reconnect backoff are local durations on the monotonic clock, which is never offset (helper 03), so an advance never trips a watchdog or wakes a sleeper early. Lease expiry, backoff and retention are the API's and follow the offset there. Log line timestamps stay on system time (no timer hook for a test-only effect). The offset is one in-memory value inside `OffsetClock`, zero after a restart; nothing is persisted (section 4).

## 3. Behaviour (job flow)
1. **Start-up**: load config, `sweep_all`, connect to every slot's decoder, `hello` (protocol, capabilities, build timestamp), `self_check` (no route to api/db/internet, no secret env, no blob mount, uid != 0, no capabilities, read-only root). Any failed item: no jobs are taken, `worker_selfcheck_ok 0`, `/readyz` 503, and the next status report says `decoder_ok = false`.
2. **Slots**: two decoder containers per supervisor (decision 13), one slot each. `decoder-interactive`: lane `image` only, memory-backed scratch (512 MiB). `decoder-general`: lanes `video` and `image`, first lane rotated each claim so videos and bulk photos both progress, **disk-backed scratch** (24 GiB, see section 6). Any priority may be claimed on any slot; the API orders by priority, so interactive photos come first. More supervisor/decoder pairs scale throughput. Each slot runs its own loop and lease heartbeat (`lease_ms / 3`). The loop type (task, thread, state machine) is the implementer's choice; tests rely only on the **slot boundaries** below (G-09-1 closed by decision, AZ-3867):
   - **One job per slot**: a slot holds at most one claimed job from `claim` to its terminal call (`result`, `failure`, `release`, or a quiet stop on 409); it claims again only after that, and never while its decoder is unhealthy or reconnecting.
   - **Lane mask per slot**: the interactive slot sends only `lane=image`; the general slot alternates `video`, `image` while both have work (the lane is the `claim` body at `:8081`).
   - **Own heartbeat per slot**: `PUT .../lease` every `lease_ms / 3` while the slot's job runs and none after it ends; a 409 stops that slot's job (`cancel` to its decoder, nothing more posted) and leaves the other slot running.
   - **Own socket and scratch per slot**: one decoder connection and one scratch root per slot; one scratch tree per job, created at `claim` and removed at the end; `hello` then `self_check` before the first `claim` on every (re)connection of that slot.
   - **Observable only through** the API (claim bodies, lease PUTs, `input` GET, `outputs` PUTs, `result`/`failure`, job rows), the decoder socket (request order), the scratch directory (tree per job), and the ops port (`/readyz`, `/metrics`; the log field `slot`). Both slots live in one supervisor process (one RSS and CPU figure, PT-09-1). No test reads internal state, thread names or queue types.
   - The decision adds no state: slot state stays in memory (section 4).
3. `claim` -> `Scratch::create` -> `fetch_input` (`Gone` closes the job `obsolete`, no retry; an input too large for the slot's scratch fails with code `input_too_large`, `Permanent`) -> `probe` (photo or video, duration, streams) -> by job kind:
   - `stills` (photo): one fetch, two renders from the same `in/source`: `render_still{max_edge 1440}` then `render_still{max_edge 320}`, each posted as `outputs/preview` and `outputs/thumb`;
   - `poster` (video): `render_still{max_edge 320, poster_at_ms = poster_offset_ms(duration)}` (1 s, or 10% of the duration for clips shorter than 10 s), posted as `outputs/thumb`;
   - `transcode` (video): `TranscodePolicy::decide`; `SkipCompliant` calls `complete(skipped_compliant)` with **no output**; `Remux` or `Transcode(Plan)` calls `transcode` and posts `outputs/transcode`.
4. **Watchdog (AC-45)**, two tiers. Tier 1 is in 10: no ffmpeg progress line for `decoder_stall_kill` (90 s) kills it and answers `Stalled`. Tier 2 is here: no frame of any kind for `decoder_silence_timeout` means the runner itself is hung -> `cancel`, wait `decoder_cancel_grace`, mark the slot unhealthy (stops claiming, `/readyz` 503) and `fail` transient. The runner has its own self-abort watchdog (10), because Docker does not restart a container on a failed healthcheck; the container exits and Compose restarts it, and the supervisor (which has no Docker socket) reconnects and re-runs `hello` and `self_check`. The wall limit per kind (`photo_job_timeout` 120 s, `video_job_timeout` 40 min) is enforced by both tiers.
5. **Retry inside the claim**: `Stalled` or `EncoderFailed{exit_code}` -> rerun once on a fresh `out/` with `Plan.encoder = Cpu` (a failed `Remux` reruns once as a CPU `Transcode`). Then: permanent codes (`Unsupported`, `Corrupt`, `LimitExceeded`, `OutputInvalid`, `input_too_large`, `output_rejected`, second `Stalled`) -> `fail(Permanent)`; transient codes (decoder unreachable/killed by OOM, fetch or post `Transient`, `HashMismatch`) -> `fail(Retry)`; the API turns it into `Requeued` or `Dead` by attempts. A partly posted `stills` job is simply retried: stored outputs answer `AlreadyPresent`.
6. **Post**: open each output safely (section 2, Scratch; an output that fails the safe-open check fails the job with code `output_rejected`, `Permanent`), compute BLAKE3, `post_output` (the server's sniffer and size cap are the check). `LeaseLost`/409 means stop without posting again.
7. **Always**: scratch removed, heartbeat stopped, metrics emitted. `SIGTERM`: stop claiming, let running jobs finish for `worker_shutdown_grace`, then `cancel` + `release`.
8. **Status**: every `worker_status_interval` the supervisor reports `decoder_ok` (all slots passed `self_check` and are healthy) and `image_age_days` (from the decoders' `build_timestamp`). The API stores it (08 `worker_status`); `doctor` reads it there.

## 4. Data Owned
No persistent data and no schema. All job state changes go through the worker API. In memory only: slot states, cached decoder capabilities, unhealthy flag. Nothing to rehydrate: a restart re-claims through lease expiry.

## 5. Dependency graph
09 -> `media-shared`, helpers 01, 02, 03, 05. No cycle: 08 and 10 never import 09. **Implement after**: `media-shared` (DTOs, policy, decoder protocol) and the 08/11 worker routes (integration tests use the real API). **Parallel with**: 10 runner, 12. **Blocks**: nothing.

## 6. Configuration (keys and env names as in helper 01; durations never hard-coded; tests use short values)
| Key | Default | Notes |
|---|---|---|
| `worker_id` | hostname + random | `X-Worker-Id`, `jobs.leased_by`, `worker_status.worker_id` |
| `worker_api_url` | `http://api:8081` | the backend-network worker listener, never the public host |
| `worker_api_token` (secret file) | `/run/secrets/worker_api_token` | |
| `worker_sock_interactive`, `worker_sock_general` | `/run/decoder-interactive/decoder.sock`, `/run/decoder-general/decoder.sock` | one decoder container per slot, one job at a time each |
| `worker_scratch_interactive`, `worker_scratch_general` | `/scratch-interactive`, `/scratch-general` | the same volumes the decoders mount |
| `worker_claim_poll_interval` | 500ms | no LISTEN/NOTIFY; an empty claim is a 204 |
| `worker_status_interval` | 30s | |
| `decoder_stall_kill` | 90s | passed to the decoder per request (AC-45) |
| `decoder_silence_timeout` / `decoder_cancel_grace` | 20s / 5s | tier 2 |
| `photo_cpu`, `photo_memory`, `photo_job_timeout` | 2, 2GiB, 120s | limits sent with each request (AC-25); the decoder's own ceiling, the same keys, clamps them |
| `video_cpu`, `video_memory`, `video_job_timeout` | 4, 4GiB, 40m | |
| `worker_max_input_photo` / `worker_max_input_video` | 256MiB / 10GiB | larger input: `TooLarge`, permanent, original untouched (the original is still served; no derived file) |
| `worker_scratch_general_cap` | 24GiB | `decoder-general` mount size: 2 x `worker_max_input_video` (input plus the remux/transcode output, which is at most as large) plus a 4 GiB work margin (20 + 4 = 24). The supervisor refuses a job whose input does not fit the free space of the mount (`statvfs`). The 2x factor is a bound: a `Remux` output is about the input size, a `Transcode` output is usually smaller. Gate S5: replace the bound with the measured peak scratch use of the largest real clip and of a remux of a 10 GiB file, keep the 4 GiB margin unless it was reached, and check that one mount of this size is provisioned per supervisor/decoder pair. The value stays 24 GiB until S5 reports |
| Interactive scratch size | 512MiB tmpfs (`size=536870912`) | the one value in this spec and in `containerization.md` (the earlier `size=1g` there is withdrawn). Arithmetic: image-only slot, so `worker_max_input_photo` 256 MiB input + 2 outputs of at most 32 MiB (they are posted and deleted one at a time, and the server caps are 20 MiB preview and 5 MiB thumb, so 64 MiB is a ceiling) = 320 MiB; margin 192 MiB (60%) for the file system and a partly written output. It counts against the 2 GiB `decoder-interactive` limit: a full tmpfs leaves 1.5 GiB for vips, where 1 GiB would leave 1 GiB for no gain. The supervisor's `statvfs` check refuses an input that does not fit. Set as the tmpfs `size` of the `scratch_interactive` volume |
| `nvenc` | off | [v1.1] AC-28 |
| `worker_shutdown_grace` | 30s | |
| `worker_ops_listen` | `:9090` | `/healthz`, `/readyz`, `/metrics` |
Output quality settings (edges 320/1440, WebP Q, CRF, preset) are **not** operator config: they come from `Params[params_version]` in `media-shared`. The lease and attempt counts are the API's (08); the supervisor learns the lease from `lease_ms`.

## 7. Security notes
- Decoder output is hostile: bounded frames, strict schema, ranges on width/height/duration, media type from a fixed enum, size from `fstat`, never from the decoder's report; output files are opened without following links and are never opened blind (FIFO safe).
- Worker secret only on the backend network, `Secret<T>` wrapper, never logged; user and device tokens do not exist here (AC-79). The supervisor can reach the API only through the worker routes and cannot read any tenant table: there is no database path from this image.
- Fetch verification: BLAKE3 block hashes rolled up and compared with the claimed `content_hash`; a mismatch never reaches the decoder (AC-72, worker side).
- The scratch volume and the socket are the only shared surface with the decoder; input is read-only for it; outputs are opened without following links.
- Supervisor image is distroless with no media libraries.

## 8. Logging and metrics
JSON, `component=worker`, fields `job_id`, `kind`, `attempt`, `slot`, `err_code`; never paths or decoder stderr beyond a 2 KiB sanitised tail on WARN. Metrics: `jobs_total{kind,result}` (result includes `obsolete`, `remuxed`), `job_duration_seconds`, `decoder_kills_total{reason}`, `thumbnail_ready_seconds`, `worker_selfcheck_ok`, `worker_decoder_build_timestamp_seconds`, `worker_decoder_restarts_total`.

## 9. ACs implemented
| AC | Release | Part owned here |
|---|---|---|
| AC-10 | v1.0 | image-only interactive slot, one fetch for thumb and preview, ready within 5 s p95; bulk 10,000 in 15 min |
| AC-11 | v1.0 | policy applied: compliant never re-encoded, index-at-end remuxed; throughput scales with supervisors |
| AC-25 | v1.0 | no creds to the decoder, no database login for the worker, self-check gate, limits per kind, hostile corpus ends in job failure |
| AC-28 | v1.1 | NVENC only when enabled and probed; any failure reruns on CPU |
| AC-44 | v1.0 | output compliance check keeps derived size in budget |
| AC-45 | v1.0 | tier-2 watchdog, single CPU retry |
**Interaction ACs folded in**: **AC-71** [v1.0]: killing the supervisor mid-job leaves one derived per (account, content hash, kind, params) because (a) lease expiry re-queues, (b) the server's `accept_output` is idempotent (`AlreadyPresent`), (c) `sweep_all` at start-up and `Drop` remove scratch data; tests kill at fetch, decode and post. **AC-79** [v1.0] (client side): worker secret and worker URL only. **AC-72** [v1.0] (worker side): hash-verified fetch.

## 10. Tests
FT-P-20 to FT-P-28, FT-N-06, FT-N-07, FT-N-08, NFT-PERF-13 to NFT-PERF-21, NFT-RES-19, NFT-RES-23, NFT-RES-31, NFT-RES-LIM-04, NFT-RES-LIM-09, NFT-SEC-28, NFT-SEC-29. Unit: policy table (every non-compliant axis, portrait 1080x1920 compliant, remux case), poster offset, slot masks and lane rotation, lease-lost abort (409), 403 stops claiming, safe-open rejects symlink, hardlink, FIFO and oversize, frame limits, `OffsetClock`, reconnect after a runner exit. Integration (real API with Postgres, fake decoder on a real socket): kill the supervisor at each step, two supervisors, expired lease reclaimed, `Obsolete` close on a superseded revision, partial `stills` retry, graph test that the worker image has no `sqlx` and no database URL.

## 11. Component acceptance
Killing the supervisor at any point leaves exactly one derived blob per key after retry and an empty scratch; a hung or hostile decoder never blocks a slot beyond `decoder_stall_kill`/wall limit and never leaks a secret into an output; a compliant clip closes `skipped_compliant` with zero output bytes; an index-at-end compliant clip closes with a remuxed output; two supervisors raise throughput >= 1.7x; the worker starts with no master key, no blob mount and no database credential and refuses jobs when the self-check fails.

## 12. Risks and open points
0. **Defaults chosen at Step 5 gap pass**: input too large for scratch gives failure code `input_too_large` (permanent); an output file failing the safe-open check gives `output_rejected` (permanent); a descendant process found and killed at job end is a hostile-class outcome (10), so the runner exits and the supervisor reconnects.
1. **Reserved interactive capacity vs one job per decoder** (AC-10): resolved by two decoder containers per supervisor and an image-only interactive decoder. Slot CPU shares and the general scratch size: gates S4/S5.
2. HDR colour handling of the poster frame is implemented in 10 (`render_still`) and tuned in gate S5. The remux rule (decision 2) and the poster-frame rule (decision 3) are decided, not open; only the poster-frame tolerance goes to the Step 4 sufficiency question.
3. One shared `worker_api_token` for all supervisors rotates only by listing two secrets (08); per-worker revocation needs a table (not planned).
4. Version skew: not an issue while the worker runs from the same release on the same host. Remote workers are out of v1.0; a `supported_params_versions` claim filter is not part of v1.0.
5. `probe` runs for every job, including photos, so the decoder's `probe` must answer for images (see 10_decoder).
6. A video poster waits behind a running transcode on the one `video` slot (08 risk).
