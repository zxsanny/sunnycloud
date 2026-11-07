# 08 Media Jobs

## 1. High-Level Overview

**Purpose**: API-side owner of all media work: the `jobs` queue and its SQL, the `derived` table, job planning at commit (lanes, interactive vs bulk), the worker-facing API on the internal listener (claim, heartbeat, input, output, result, failure, worker status) with bearer-secret auth, worker health for `doctor`, and serving thumbnails and transcodes to users. The worker supervisor (09) talks to this component over HTTP only; it has no database login (S4-1).

**Architectural Pattern**: Queue-in-Postgres (`SKIP LOCKED`) inside the API process. The `JobQueue` repository runs in system scope (a claim has no account yet; `jobs` and `worker_status` are system-scope tables, helper 07).

**Single responsibility**: "Decide what derived files should exist for a revision, track their production, and accept the result safely." It runs no decoder and mounts no blobs; process execution is 09 and 10.

**Shared crate `media-shared`** (workspace member; no sqlx, no queue module, no I/O, no credentials):
- `types`: `JobKind`, `JobSpec`, `Params`, `OutputSpec`, `Probe`, `Plan`, the worker-API DTOs (`ClaimedJob`, `JobFailure`, `WorkerStatus`).
- `policy`: `TranscodePolicy`, `is_compliant(&Probe)`, `poster_offset_ms(duration_ms)`, `supported_kinds_for(media_type)`. Applied by 09, not by the decoder.
- `decoder_protocol`: wire types for the supervisor-decoder socket. Owned by 10, linked by 09 and 10.
The API-side crate `media-api` holds everything else in this document, including the queue SQL.

## 2. Ports (declared in `contracts`)

**Provides**
| Port | Methods | Consumers |
|---|---|---|
| `CommitEffect` (impl `MediaEnqueueEffect`) | `on_entry(tx, &PublishedEntry)`, `on_batch(tx, &[PublishedEntry])` | 02 (supplied per `publish` call by 03; runs inside 02's commit transaction) |
| `ThumbnailStateSource` | `states(account, [content_hash])` | 02 (listing), 11 (`thumbnail-states`) |
| `DerivedOpener` | `open_derived(account, node, kind)` and `open_derived_for(live_file, kind)`, both return a `BlobRef` (defined in 04) | 11 (thumbnail and transcode routes; 11 streams the `BlobRef` through 03's `DownloadService`), 11 for the public share thumbnail after 06 authorised the token |
| `WorkerAuthenticator` | `authenticate(bearer)` | 11 (internal listener) |
| `WorkerJobs` | `claim`, `extend_lease`, `release`, `complete_skipped`, `fail`, `open_input`, `accept_output`, `record_status` | 11 (internal routes) |
| `AccountLifecycleHook` | `initialize(tx, account)` (no-op), `erase_batch(account, limit) -> EraseProgress{remaining}`: deletes the account's `jobs`, then its `derived` rows with `GcQueue Due::Now` | 07 `DeletionPurger` drives it; registered by 12 |
| `MediaAdmin` | `requeue_dead(filter)`, `stats()` | 12 (`admin media-requeue`, `admin media-stats`) |
| `BlobOwnerCleanup` (derived rows) | `remove_owner_rows(tx, blob_id)` | 04 (GC) |
| `Janitor` x2 (`JobReaper`, `DerivedJanitor`) | `name()`, `run_cycle(now)` | 12 scheduler |
| `HealthContributor`, `DoctorCheck` | dead jobs, queue depth, worker status (section 4) | 07, 12 |

**Requires**
| Port | Provider | Used for |
|---|---|---|
| `TreeReader` (merged, includes the old `NodeResolver`) | 02_tree | `resolve_live_file(account, node) -> LiveFile{kind, revision, blob_ref}` for `open_derived`; `get_revision` and `is_current_or_retained(account, revision)` for `open_input` and `accept_output` |
| `BlobReader` | 04_blobstore | decrypted original for `open_input` |
| `BlobWriter`, `BlobRegistry`, `GcQueue`, `BlobCatalog` | 04_blobstore | store and register derived blobs, schedule removal, `get_ref` for the derived `BlobRef` |

Helpers: db, clock, config, error, audit. `media-shared` is the only crate shared with 09 and 10.

## 3. Scope

**In**: job planning and enqueue, lanes and priority, claim/lease/retry/dead rules, reaper and terminal purge, derived storage and idempotent result acceptance, thumbnail state for listings, derived serving, transcode policy and output specs, worker token auth, worker status, worker-facing services, requeue and stats.
**Out**: decoder sandbox, ffmpeg/vips invocation, watchdog, scratch files (09, 10); encryption and file I/O (04); download streaming and Range (03); which revision is current (02); quota (derived does not count, 07a); scheduling (12); HTTP (11); share-token authorisation (06).

## 4. Internal Interfaces

### CommitEffect implementation (`MediaEnqueueEffect`)
Implements `on_entry` (plan and enqueue for one published entry) and `on_batch` (no-op). Runs inside 02's commit transaction (after node and quota locks, before the counter bump). Input `CommitContext{account_id, revision_id, content_hash, media_type, size, import_hint, vault}`. `vault` is set by 02/03 for vault files; the planner returns nothing for them (08 has no dependency on 20). `JobPlanner::plan` (pure) returns specs for supported types only:

| Media | Job kind | Lane | Outputs (`derived` kinds) |
|---|---|---|---|
| photo | `stills` | `image` | `thumb` 320 px and `preview` 1440 px, one job, one fetch |
| video | `poster` | `video` | `thumb` (poster frame) |
| video | `transcode` | `video` | `transcode` |
| vault file, SVG, unknown, unsupported | none | | |

For each spec skip when every output already has a `derived` row for `(account, content_hash, kind, params_version)`; else `INSERT ... ON CONFLICT DO NOTHING` on the job unique key. `priority` = 1 (bulk) when `import_hint` is set (initial import or camera-roll first sync) or the account already has more than `bulk_queue_threshold` queued jobs (counted with `LIMIT threshold+1` on the queued-jobs index); otherwise 0. The effect issues no I/O and no read beyond those indexed probes.

### JobQueue (repository inside `media-api`, system scope)
| Method | Input | Output | Notes |
|---|---|---|---|
| `enqueue_in_tx` | `tx`, `JobSpec`, priority | `Enqueued \| AlreadyExists` | idempotent |
| `claim` | `lane`, `worker_id` | `Option<ClaimedJob{job_id, kind, content_hash, params_version, attempt, lease_ms}>` (`content_hash` is lowercase hex, 64 characters, as everywhere on the HTTP surface) | one statement: `SKIP LOCKED`, `ORDER BY priority, available_at`, sets `running`, `attempts + 1`, `available_at = now + job_lease`, `leased_by`. Any priority may be claimed by any slot: ordering by priority already serves interactive jobs first. Before handing out work, `claim` skips and marks `dead` any job whose lease expired and whose attempts are exhausted (same rule as `reap`, so a claim never hands out an exhausted job) |
| `extend_lease` | `job_id`, `worker_id` | `Extended \| LeaseLost` | heartbeat while a decoder runs |
| `complete_skipped` | `job_id`, `worker_id`, outcome `skipped_compliant \| obsolete` | `()` | `succeeded` with that outcome, no derived row |
| `fail` | `job_id`, `worker_id`, short error code, `Disposition::Retry \| Permanent` | `Requeued \| Dead` | backoff `job_retry_backoff` doubling is applied here, on the API clock; bounded `last_error` (500 chars); `dead` when `Permanent` or `attempts >= job_attempts` |
| `release` | `job_id`, `worker_id` | `()` | graceful shutdown: back to `queued`, `attempts - 1` |
| `reap` | `now` | `ReapStats` | marks `running` rows with expired lease and exhausted attempts `dead`; increments `job_leases_expired_total` |
| `purge_terminal` | `now - job_terminal_ttl` | count | batched deletes |
| `requeue_dead` | filter (account, kind) | count | used by `MediaAdmin::requeue_dead` after a policy fix |

A running job whose lease expired is simply claimable again; that is the retry path (AC-71). `succeeded` with outcome `derived` is written only by `DerivedStore::accept_output`, once every expected output exists; never by the supervisor. The `jobs.outcome` values are `derived`, `skipped_compliant`, `obsolete`. Job-scoped calls check that the job is `running`, `leased_by` the calling worker and the lease unexpired; otherwise the result is `LeaseLost` (HTTP 409 `lease_lost`).

### DerivedStore (behind `WorkerJobs::accept_output`)
| Method | Input | Output | Errors |
|---|---|---|---|
| `accept_output` | `job_id`, `worker_id`, `kind` (`thumb`, `preview` or `transcode`; must be an expected output of the job kind), `media_type`, `width?`, `height?`, `duration_ms?`, `body: ByteStream`, `length` | `Accepted \| AlreadyPresent` | `LeaseLost`, `RevisionObsolete`, `OutputTooLarge`, `OutputTypeRejected`, `OutputKindRejected`, `Storage` |

Sequence: validate the lease, then `TreeReader::is_current_or_retained` for the job's revision; stream the body through `BlobWriter::write_complete` (size capped per kind, file synced, no tx open); sniff the head and require the expected type (WebP or JPEG for `thumb`/`preview`, MP4 for `transcode`; the worker is never trusted on type); then one short tx: `register_in_tx(committed)`, insert `derived` (`ON CONFLICT DO NOTHING`), and, when this was the last expected output, mark the job `succeeded` with outcome `derived`. If the derived row already existed (retry after a lost response) the new blob is scheduled with `GcQueue Due::Now` and the answer is `AlreadyPresent`, so exactly one derived file exists per key (AC-71). A crash after the file write leaves an orphan swept by 04. A lossless remux result is accepted like a transcode. A `stills` job posts `preview` and `thumb` as two outputs; a retry after a partial run gets `AlreadyPresent` for the one already stored.

`open_input(job_id, worker_id)`: validates the lease, resolves the job's revision (`TreeReader::get_revision`, `is_current_or_retained`), takes the `BlobRef` from `BlobCatalog::get_ref` and returns `BlobReader::open` with no range (same decrypt path as downloads). A revision is obsolete only when it is collected (no longer `is_current_or_retained`, owner E-13 in 02) or foreign to the account: that is `RevisionObsolete` (404) and the supervisor closes the job `obsolete`. A superseded or deleted revision that is still retained is served and the job is processed (decided 2026-11-07).

### Derived queries
- `ThumbnailStateSource::states(account_id, &[content_hash]) -> Map<hash, Ready \| Pending \| Failed \| None>`: one batched probe on `derived` PK (`thumb`) and one on the job unique key (both by index, no join into the listing query; NFT-PERF-10 must include these two probes). `None` when the planner yields no job for the type.
- `open_derived(account_id, node_id, kind) -> BlobRef | NotReady | NotFound`: `TreeReader::resolve_live_file`, then the derived row for the revision's `content_hash`, then `BlobCatalog::get_ref`. `open_derived_for(live_file, kind)` is the same without the node lookup; it is used for share links, where 06 (`PublicShareService::authorize_derived`) has already checked the token and returned the `LiveFile`. 08 does not authorise visitors and does not stream; 11 passes the `BlobRef` to 03's `DownloadService`. SVG or any type without a derived row never falls back to the original. Vault files have no derived rows.

### WorkerAuthenticator
`authenticate(bearer: &str) -> Result<WorkerIdentity, WorkerAuthError>`: SHA-256 of the presented secret compared in constant time with the loaded digests of `worker_api_token` and, during rotation, `worker_api_token_next`. Failure is `Forbidden` for every cause (no header, wrong secret, user session token, device token) so nothing distinguishes them.

### Worker status
`record_status(worker_id, WorkerStatus{decoder_ok, image_age_days})` upserts `worker_status(worker_id PK, seen_at, image_age_days, decoder_ok)`; `seen_at` is the API clock. The supervisor sends it every `worker_status_interval` (30 s). 08's `DoctorCheck` reads the table: no row seen within three intervals is Warn ("no worker"; Fail when jobs are queued or dead); `decoder_ok = false` is Fail; `image_age_days` above 14 is Warn (weekly rebuild rule, AC-63). `JobReaper` deletes rows not seen for `job_terminal_ttl`. This is the only worker-level heartbeat; there is no direct transport from the supervisor to the CLI.

### Maintenance (bounded `run_cycle`, scheduled by 12)
`JobReaper` (reap + purge terminal jobs + purge stale `worker_status`); `DerivedJanitor` (derived rows whose `(account_id, content_hash)` has no remaining revision: delete the row and `GcQueue Due::Now`; runs after retention purge); `BlobOwnerCleanup::remove_owner_rows` for derived rows hit by GC (called by 04).

## 5. External API Specification

### Internal worker resources (internal listener `:8081` only; never in the public router, never routed by Caddy)
All carry `Authorization: Bearer <worker_api_token>` and `X-Worker-Id`. `:8081` is plaintext on the internal network of one Compose host; remote workers are out of v1.0.

| Endpoint | Method | Description |
|---|---|---|
| `/api/v1/worker-jobs/claims` | POST | Body `{lane}` (`image` or `video`). 200 `ClaimedJob`, or 204 when nothing is due |
| `/api/v1/worker-jobs/{job_id}/lease` | PUT, DELETE | PUT is the heartbeat: 200 `{lease_ms}`. DELETE is `release` (graceful shutdown): 204. 409 `lease_lost` when the job is not running for this worker |
| `/api/v1/worker-jobs/{job_id}/input` | GET | Decrypted original via `BlobReader`; `Range` is ignored: always 200 with the full decrypted body and exact `Content-Length` (G-08-4, decided 2026-11-07); 404 `revision_obsolete` only for a collected or foreign revision (the supervisor then closes the job `obsolete`); 409 `lease_lost` |
| `/api/v1/worker-jobs/{job_id}/outputs/{kind}` | PUT | One output per `kind` (`thumb`, `preview`, `transcode`): `Content-Type`, optional `X-Width`, `X-Height`, `X-Duration-Ms`; 204 on `Accepted`, 200 on `AlreadyPresent`, 413 over the kind cap, 422 on a type the sniffer rejects or a kind the job does not produce, 409 `lease_lost`, 404 `revision_obsolete` |
| `/api/v1/worker-jobs/{job_id}/result` | PUT | Body `{outcome: skipped_compliant \| obsolete}`: closes the job without output. 204 |
| `/api/v1/worker-jobs/{job_id}/failure` | POST | Body `{code, disposition: retry \| permanent}`. 200 `{result: requeued \| dead}` |
| `/api/v1/worker-jobs/status` | PUT | Body `WorkerStatus{decoder_ok, image_age_days}`. 204 |

Authentication failure of any kind is a uniform 403. An authenticated worker that does not hold the job gets 409, so the supervisor needs no secret-versus-lease disambiguation. Responses carry `Cache-Control: no-store`; no job or account data is returned beyond the bytes and the `ClaimedJob` fields. The test-image clock endpoint on the same listener is owned by 11.

### Public resources owned here
| Endpoint | Method | Auth | Description |
|---|---|---|---|
| `/api/v1/nodes/{node_id}/thumbnail?size=small\|preview` | GET, HEAD | session or device | 200 WebP/JPEG; `404 thumbnail_not_ready` or `thumbnail_unavailable`; never SVG |
| `/api/v1/nodes/{node_id}/transcode` | GET, HEAD | session or device | Range-capable MP4; `404` when none (compliant clips serve the original via content) |
| `/api/v1/thumbnail-states` | POST (query body of node ids) | session or device | Batch state for the web poll every 2 s; body capped at 1,000 ids: more is 422 `batch_too_large` (same family as the commit batch cap; 400 `invalid_request` was the rejected alternative). Ids of another account and unknown ids are omitted from the answer, so no state is leaked (G-08-3, decided 2026-11-07) |

The share-link thumbnail route is 06's and 11's: 06 authorises the token, 11 calls `open_derived_for` with the returned `LiveFile`.

## 6. Data Owned and Access

| Table | Owner | Hot queries | Index |
|---|---|---|---|
| `jobs` | 08 (system scope, no tenant policy) | claim per lane, enqueue upsert, reap, bulk-threshold count | `jobs_claim_idx (lane, priority, available_at) WHERE status IN ('queued','running')`, unique `(account_id, kind, content_hash, params_version)`, `jobs (account_id) WHERE status = 'queued'` (decision 11) |
| `derived` | 08 | PK probe by content, janitor anti-join with `revisions (account_id, content_hash)` | PK, `blob_id` unique |
| `worker_status` | 08 (system scope) | upsert by `worker_id`, read by doctor | PK |

Decision 11 adds the queued-jobs index and the outcome value `obsolete`. Table tuning for churn is in `data_model.md` section 9. Estimates: 500 users, 40 photos/day each: about 20 k jobs/day (one `stills` job per photo, not two), 14-day terminal retention about 280 k rows (about 60 MB); `derived` about 2 rows per photo.

**Lanes**: `lane` is generated: `image` for `stills`, `video` for `poster` and `transcode`. Priority 0 interactive, 1 bulk. `decoder-interactive` (image-only) claims the `image` lane; `decoder-general` claims `video` and `image`, rotating the first lane per claim. Because claim orders by priority, an interactive photo is taken by the next free image-capable decoder; a 10,000-photo bulk import cannot starve it, and the worst wait is one short photo job on each decoder (AC-10).

## 7. Transcode Policy and Output Specs (`media-shared::policy`)

Constants versioned by `params_version`; changing any number bumps the version (no operator-tunable output settings). The supervisor applies the policy to the decoder's probe; the decoder only probes.
- **Compliant**: H.264 High profile or lower, video bitrate <= 10 Mbit/s (decimal: 10,000,000 bit/s, inclusive), AAC (or no) audio, SDR BT.709, MP4 with the index first, and the **orientation-independent bound**: long side <= 1920 and short side <= 1080 (rotation metadata does not matter). `is_compliant(&Probe)` is pure.
- **Decision** `decide(probe, caps, encoder_pref) -> SkipCompliant | Remux | Transcode(Plan)`:
  - all compliance axes hold -> `SkipCompliant` (never re-encoded);
  - everything holds except the index is at the end -> `Remux`: lossless copy of the first video and audio streams with the index moved to the front (`faststart`), no re-encode;
  - otherwise `Transcode`: H.264 High, short side <= 1080 and long side <= 1920 (orientation independent), AAC MP4 with `faststart`, HDR tone-mapped to SDR, native AAC encoder. CRF and preset chosen to meet derived storage <= 20% of original bytes (AC-44, tuned in NFT-PERF-20).
- The same `is_compliant` is applied to the probe of every `Remux` or `Transcode` output.
- **Photos**: `thumb` 320 px and `preview` 1440 px WebP, orientation applied, separate HEIC profile. Dimension swap for orientations 5 to 8. Both come from one `stills` job.
- **Poster frame** (video `poster` job, derived kind `thumb`): `poster_offset_ms(duration_ms)` = 1,000 ms, or 10% of the duration for clips shorter than 10 s. The supervisor (09) computes it in `media-shared` from the probe and passes it to the decoder.
- GPU (v1.1) is a worker-local encoder choice with the same output spec and `params_version`.

## 8. Configuration (keys and env names as in helper 01)

| Key (env) | Default | Notes |
|---|---|---|
| `SUNNYCLOUD_JOB_LEASE` | 5m | test 10s; returned to the worker as `lease_ms` |
| `SUNNYCLOUD_JOB_ATTEMPTS` | 3 | |
| `SUNNYCLOUD_JOB_RETRY_BACKOFF` | 30s base, doubling | applied by `fail` |
| `SUNNYCLOUD_BULK_QUEUE_THRESHOLD` | 200 | confirmed answer 3 |
| `SUNNYCLOUD_JOB_TERMINAL_TTL` | 14d | |
| `SUNNYCLOUD_JOB_REAP_INTERVAL` / `SUNNYCLOUD_DERIVED_JANITOR_INTERVAL` | 1m / 1h | |
| `SUNNYCLOUD_WORKER_API_TOKEN` (file) / `SUNNYCLOUD_WORKER_API_TOKEN_NEXT` (file, optional) | secret files | the second one only while rotating |
| `SUNNYCLOUD_INTERNAL_ADDR` | `0.0.0.0:8081` | second listener, backend network only |
| `SUNNYCLOUD_DERIVED_MAX_THUMB` / `..._PREVIEW` / `..._TRANSCODE` | 5MiB / 20MiB / 10GiB | per-kind caps on `outputs/{kind}`; the transcode cap equals `worker_max_input_video` because a remux is as large as its input |

Stall kill, per-job timeouts, input limits and scratch sizes are 09/10 config.

## 9. Security Notes

- Internal routes exist only on the internal listener and are absent from the public router (public path answers 404, AC-79). Authentication is the worker bearer only; a cookie, session token or device token is `403` there. Caddy has no route to port 8081.
- Job-scoped authorization: the job must be `running`, `leased_by` the calling `X-Worker-Id`, lease not expired; an authenticated worker that fails this gets 409 `lease_lost`.
- Secrets: only SHA-256 digests held in memory after load; constant-time comparison; never logged. The token is not an account credential; the account comes from the job row. One token for all supervisors; per-worker revocation would need a table and is not planned.
- Worker output is untrusted: type sniffed, size capped while streaming, dimensions bounded, stored encrypted by 04; the worker never chooses blob ids or paths. A hostile supervisor can post at most the expected output kinds of jobs it holds.
- Decrypted input is streamed, never persisted by the API. The supervisor holds no master key and no database credential; the decoder holds nothing (AC-25).
- Derived blobs sit under the same account and are removed on account deletion.

## 10. ACs Implemented

| AC | Release | Part |
|---|---|---|
| AC-10 | v1.0 | Interactive/bulk priority, lanes, one fetch per photo |
| AC-11 | v1.0 | Compliance rule, remux rule and output spec; no re-encode of compliant clips |
| AC-25 (API side) | v1.0 | Worker endpoints expose no secrets; decoder cannot reach DB or blobs; workers have no DB |
| AC-28 | v1.1 | Same output spec for GPU fallback |
| AC-43 | v1.0 | Derived blobs encrypted by 04 |
| AC-44 | v1.0 | Policy numbers tuned to <= 20% |
| AC-45 (API side) | v1.0 | Attempts, lease and retry rule (stall kill is 09) |

**Interaction ACs folded in**
- **AC-71**: unique job key + unique `derived` PK + atomic `accept_output` + `AlreadyPresent` give one derived file per `(account, content hash, params)`; scratch cleanup is a 09 requirement. Killing the supervisor mid-job only expires a lease.
- **AC-79**: second listener, worker-only auth, uniform 403, absent from public router; the test through the public proxy route returns 404.
- **AC-72** (worker side): input is the 04 reader's plaintext; a wrong master key already stopped startup.
- **AC-77** is not here; derived shares follow the node through `open_derived_for`.

## 11. Tests

FT-P-20..27, FT-P-28 (v1.1), FT-N-06, FT-N-07, FT-N-08, NFT-PERF-13..21, NFT-RES-19, NFT-RES-23, NFT-RES-31 (v1.1), NFT-SEC-28, NFT-SEC-29, NFT-RES-LIM-04, NFT-RES-LIM-09, FT-P-39 (thumbnail types), NFT-SEC-19 (thumbnail never SVG). Step 5 scenarios: AC-71 (kill supervisor mid-transcode, exactly one derived, scratch empty), AC-79 (user and device tokens 403 on 8081, 404 via the public proxy). Unit: planner matrix per type including the vault flag, claim ordering and lane isolation, `accept_output` retry race and the partial `stills` case, `is_compliant` table including 1080x1920 portrait and the `Remux` decision, poster offset (9 s clip, 10 s clip), worker auth uniformity, 409 for a non-holder, `obsolete` outcome and queued-jobs count, worker status doctor levels, account erase hook drains jobs and derived rows.

## 12. Acceptance for the Component

Exactly one derived row per key after any crash point; interactive thumbnails p95 <= 5 s while a 10,000-photo bulk import runs; no compliant clip re-encoded and an index-at-end clip with compliant codecs is remuxed without re-encoding; worker endpoints reachable only with a valid worker secret for a job the caller leases; 100% of unit tests on claim, lease and accept paths pass; coverage 90% on `accept_output`, `claim`, auth (critical).

## 13. Risks
**Defaults chosen at Step 5 gap pass**: `claim` skips and marks dead any job with an expired lease and exhausted attempts before handing out work; the 10 Mbit/s bitrate bound is decimal (10,000,000 bit/s) and inclusive.


| Risk | Mitigation |
|---|---|
| Listing latency from thumbnail-state probes | batched PK probes; NFT-PERF-10 includes them; fall back to the separate `thumbnail-states` poll only if the budget breaks |
| Hostile or buggy supervisor posts oversized output | per-kind cap enforced while streaming |
| Lost response after accept | idempotent `AlreadyPresent` |
| Dead jobs for hostile files stay unresolved for 14 d | metric `jobs_dead_total`; `media-requeue` operator command |
| Contract drift between API, supervisor and decoder | single `media-shared` crate; same workspace version; DTOs in one place |
| Claim polling over HTTP (about 4 requests/s for two slots) | index-only claim statement; 204 with no body; revisit only if measured |
| A video poster waits behind a running transcode (one `video` slot per decoder-general) | accepted for v1.0; a second supervisor pair adds a slot; tracked with gate S4/S5 |
| Policy change re-triggers all jobs | `params_version` bump re-derives lazily only on new commits; `media-requeue` for dead jobs |

## 14. Dependency Graph

After: `contracts`, 04_blobstore, 03_transfer, 02_tree (ports), helpers. Parallel with: 09_worker_supervisor (shared crate), 07, 07a. Blocks: 09, 10 (shared crate), 11 routes, 12 `media-requeue`.

## 15. Logging

ERROR: output accept failed after retries, job reaped dead. WARN: lease lost, output rejected by sniffer or cap, auth failure on the internal listener (count only, no secret). INFO: job claimed/succeeded/dead (ids, kind, attempt, duration), reaper summary. Metrics: `jobs_queued`, `jobs_running`, `job_leases_expired_total`, `jobs_dead_total`, `job_duration_seconds{kind}`, `derived_bytes_total{kind}`.
