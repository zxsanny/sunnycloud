# Contract: worker job protocol (worker listener `:8081`) and media-shared job tables

**Component**: 08_media_jobs (`media-shared` modules `types` and `policy` data; routes served by 11)
**Producer task**: E-18/c02 — `_docs/02_tasks/todo/B05-13_media_shared_params_protocol.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-20/c03 (`WorkerApi` client), E-20/c06 (slot loop), E-20/c07 (heartbeat), E-20/c08 (status report), E-21/c07 (worker route auth), E-21/c15 (internal listener router); in batch E-18/c03, c05, c07, c08, c12, c13; pipelines AZ-3660, AZ-3661
**Version**: 1.0.0
**Status**: draft (open points 1-3 wait for E-18/c01)
**Last Updated**: 2026-11-04

## Purpose

The one description of how the worker supervisor (09) talks to the API (08 behind 11) and of the job tables both sides share. The supervisor has no database login (S4-1): every queue change goes through these routes. Type shapes are in `_docs/02_document/contracts/media-shared/skeleton_types.md`; this file fixes the routes, statuses and the per-version values.

## Shape

### Job kinds

| Job kind | Lane | Expected outputs (derived kinds) | Source |
|----------|------|----------------------------------|--------|
| `stills` | `image` | `preview`, `thumb` (one job, one input fetch) | photo |
| `poster` | `video` | `thumb` (poster frame) | video |
| `transcode` | `video` | `transcode` (a lossless remux is posted as `transcode`) | video |

The interactive decoder claims `image` only; the general decoder claims `video` and `image`, rotating the first lane per claim.

### Output parameters, `params_version` 1

| Output | Value |
|--------|-------|
| `thumb` | 320 px long edge, WebP, orientation applied (dimensions swapped for orientations 5 to 8), separate HEIC profile |
| `preview` | 1440 px long edge, WebP, as `thumb` |
| poster `thumb` | frame at `poster_offset_ms` (see `media_policy.md`), WebP 320 px |
| `transcode` | H.264 High, short side at most 1080 and long side at most 1920 (orientation independent), AAC (native encoder), MP4 with the index first (faststart), HDR tone-mapped to SDR; CRF and preset: open point 4 |

Changing any value bumps `params_version`. GPU (v1.1) uses the same values.

### Accepted output types (sniffed by the API, never trusted from the worker)

| Derived kind | Accepted media types | Size cap (API setting) |
|--------------|----------------------|------------------------|
| `thumb` | image/webp, image/jpeg | `derived_max_thumb` (5 MiB) |
| `preview` | image/webp, image/jpeg | `derived_max_preview` (20 MiB) |
| `transcode` | video/mp4 | `derived_max_transcode` (10 GiB) |

### Routes (internal listener only)

Every request carries `Authorization: Bearer <worker_api_token>` and `X-Worker-Id`. Every response carries `Cache-Control: no-store`. Error bodies are 11's problem body with the code shown.

| Operation | Method and path | Request | Success | Errors |
|-----------|-----------------|---------|---------|--------|
| claim | POST `/api/v1/worker-jobs/claims` | `{lane}` | 200 `ClaimedJob`; 204 no body when nothing is due | 403 |
| extend lease | PUT `/api/v1/worker-jobs/{job_id}/lease` | — | 200 `{lease_ms}` | 409 `lease_lost`, 403 |
| release | DELETE `/api/v1/worker-jobs/{job_id}/lease` | — | 204 | 409 `lease_lost`, 403 |
| input | GET `/api/v1/worker-jobs/{job_id}/input` | — (a `Range` header is ignored) | 200, whole plaintext, exact `Content-Length` | 404 `revision_obsolete`, 409 `lease_lost`, 403 |
| output | PUT `/api/v1/worker-jobs/{job_id}/outputs/{kind}` | body; `Content-Type`; optional `X-Width`, `X-Height`, `X-Duration-Ms` | 204 accepted; 200 already present | 413 over the cap, 422 type or kind rejected, 409 `lease_lost`, 404 `revision_obsolete`, 403 |
| result | PUT `/api/v1/worker-jobs/{job_id}/result` | `{outcome: skipped_compliant \| obsolete}` | 204 | 409 `lease_lost`, 403 |
| failure | POST `/api/v1/worker-jobs/{job_id}/failure` | `{code, disposition: retry \| permanent}` | 200 `{result: requeued \| dead}` | 409 `lease_lost`, 403 |
| status | PUT `/api/v1/worker-jobs/status` | `{decoder_ok, image_age_days}` | 204 | 403 |

`{kind}` is `thumb`, `preview` or `transcode`; anything else is refused without any path effect.

## Invariants

- 403 means the secret is wrong. It is identical (status, body, headers other than `X-Request-Id` and `Date`) for every cause: no header, empty bearer, wrong secret, a user session, a device token, a cookie. The supervisor then stops claiming.
- An authenticated worker that does not hold the job (not `running`, leased by another, or lease expired) gets 409 `lease_lost` and no job data; it stops quietly and posts nothing more for that job.
- 404 `revision_obsolete` on input or output: the supervisor closes the job with outcome `obsolete` through `result`. A revision is obsolete only when it is collected (no longer `TreeReader::is_current_or_retained`, owner E-13 in 02) or foreign; a superseded or deleted revision still retained is served and the job is processed.
- `ClaimedJob.content_hash` is lowercase hex, 64 characters, the encoding the HTTP surface uses for content hashes; 09 compares its BLAKE3 roll-up against it.
- Lease, backoff and every timestamp are the API's; the supervisor measures only local durations and heartbeats every `lease_ms / 3`.
- `succeeded` with outcome `derived` is written by the API when the last expected output is accepted, never by the supervisor.
- A second identical output answers 200 (already present); exactly one derived file exists per (account, content hash, kind, params version).
- No response carries job or account data beyond the bytes and the `ClaimedJob` fields.
- The routes exist only on the internal listener; the public listener and the proxy answer 404 for them (AC-79).
- The JSON of each DTO is the same from `media-shared` and from its `contracts` copy (wire-equality test).

## Non-Goals

- Not covered: the test-clock read route on the same listener (11), the public thumbnail routes (`derived_access.md`), the decoder socket protocol (10).

## Versioning Rules

- **Breaking changes** (a route, status or code removed or changing meaning, a DTO field renamed) require a new major version and notice to E-20 and E-21.
- **Non-breaking additions** (a new optional header, a new optional body field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| empty-claim | claim on an empty lane | 204, no body | IT-08-4 |
| uniform-403 | six bad credentials on three routes | identical 403 | IT-08-16 |
| non-holder | valid secret, job of another worker | 409 `lease_lost`, no bytes | IT-08-6, ST-08-2 |
| repeat-output | same `thumb` twice | 204 then 200 | IT-08-10 |
| obsolete-input | job whose revision is collected (retention expired) or foreign | 404 `revision_obsolete` | IT-08-12 |
| retained-input | job whose revision is superseded or deleted but still retained | 200, whole plaintext | IT-08-12 |
| range-input | `GET input` with `Range: bytes=0-99` | 200, full body, exact `Content-Length` | IT-08-12 |
| wire-copies | each DTO from both crates | equal JSON | E-18/c02 AC-4 |

## Open points

1. Closed 2026-11-07: `ClaimedJob.content_hash` is lowercase hex, 64 characters.
2. Closed 2026-11-07 (G-08-4): `Range` on the input route is ignored; the answer is always 200 with the full body.
3. Closed 2026-11-07: a superseded or deleted revision still retained is served; `revision_obsolete` only for a collected or foreign revision.
4. CRF and preset values (tuned to derived storage at most 20% of originals, NFT-PERF-20).
5. Code names for 413 and 422.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.0.1 | 2026-11-07 | Closed open points 1 to 3: hash hex; `Range` on input ignored; obsolete only when collected or foreign | AZ-3826 |
