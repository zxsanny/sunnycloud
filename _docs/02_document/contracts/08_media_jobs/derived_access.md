# Contract: ThumbnailStateSource and DerivedOpener (derived files for listings and routes)

**Component**: 08_media_jobs
**Producer task**: E-18/c09 — `_docs/02_tasks/todo/B05-20_thumbnail_states_derived_opener.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-13/c14 (listing enrichment), E-13/c16 (integration pass), E-21/c14 (thumbnail, transcode and thumbnail-states handlers; share thumbnail); in batch E-18/c12, E-18/c13; pipeline AZ-3661
**Version**: 1.0.0
**Status**: draft (open point 1 waits for E-18/c01)
**Last Updated**: 2026-11-04

## Purpose

How listings learn thumbnail state and how the thumbnail and transcode routes find the derived blob of a file. Signatures are in `contracts` part 3.

## Shape

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-------------------------|--------|-----------|
| `ThumbnailStateSource::states` | `(account, &[FileHash]) -> map hash -> ThumbState` | none | async; 2 statements per call |
| `DerivedOpener::open_derived` | `(account, node_id, DerivedKind) -> BlobRef` | `NotReady`, `NotFound` | async |
| `DerivedOpener::open_derived_for` | `(&LiveFile, DerivedKind) -> BlobRef` | `NotReady`, `NotFound` | async; no node lookup |

### State per hash

| State | When |
|-------|------|
| `Ready` | a `thumb` derived row exists for (account, hash) at the current params version |
| `Pending` | no row, and a job producing `thumb` for the hash is queued or running |
| `Failed` | no row, and that job is dead |
| `None` | no job is planned for the type (unsupported, SVG, vault) or the hash is unknown to the account |

### Routes owned by 08 (routed by 11; session or device auth)

| Method | Path | Maps to | Answers |
|--------|------|---------|---------|
| GET, HEAD | `/api/v1/nodes/{node_id}/thumbnail?size=small\|preview` | `open_derived(kind = thumb \| preview)` | 200 WebP or JPEG; `NotReady` 404 `thumbnail_not_ready`; `NotFound` 404 `thumbnail_unavailable`; never SVG, never the original |
| GET, HEAD | `/api/v1/nodes/{node_id}/transcode` | `open_derived(kind = transcode)` | 200 MP4 with Range (served by 03); 404 when none (compliant clips are served as the original through the content route) |
| POST | `/api/v1/thumbnail-states` | `states` | at most 1,000 ids in the body: more is 422 `batch_too_large`; ids of another account and unknown ids are omitted from the answer (no state leaked) |

The share-link thumbnail: 06 `authorize_derived` checks the token and returns the `LiveFile`; 11 calls `open_derived_for`.

## Invariants

- Never a fallback: a file without a derived row is `NotReady` or `NotFound`, never its original bytes; SVG and vault files have no derived rows.
- Another account's node or hash yields `NotFound` or no state; nothing of another account is visible.
- `states` makes one batched probe of the derived key and one of the job key, with no join into the listing query.
- 08 does not stream; 11 passes the `BlobRef` to 03's `DownloadService`.
- Derived output belongs to content within an account: copies and restored revisions reuse it.

## Non-Goals

- Not covered: share-token checks (06), Range and conditional requests (03).

## Versioning Rules

- **Breaking changes** (a state or error removed or changing meaning, a fallback added) require a new major version and notice to E-13 and E-21.
- **Non-breaking additions** (a new derived kind) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| states | 1,000 seeded hashes | exact map; 2 statements | IT-08-14 |
| no-fallback | SVG, vault, queued photo, compliant video | `NotFound`, `NotFound`, `NotReady`, `NotFound` | IT-08-13 |
| share-path | same live file through both methods | equal results | IT-08-13 |
| cross-account | B asks for A's node and hashes | `NotFound`; ids omitted from `thumbnail-states`, no state | ST-08-4 |
| over-limit | `thumbnail-states` body of 1,001 ids | 422 `batch_too_large` | ST-08-4 |

## Open points

1. G-08-3: closed 2026-11-07 (owner decision): foreign and unknown ids are omitted from the answer; a body over 1,000 ids is 422 `batch_too_large` (same family as the commit batch cap; 400 `invalid_request` was the rejected alternative).
2. The route takes node ids; `states` takes hashes; the node-to-hash step (11 through `TreeReader`) is not stated.
3. `BlobRef` carries no media type; the derived row's type must reach 03 (contracts part 3 open point 6).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.0.1 | 2026-11-07 | Closed open point 1: foreign and unknown ids omitted; over 1,000 ids 422 `batch_too_large` | AZ-3826 |
