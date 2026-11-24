# Batch Report

**Batch**: 119
**Tasks**: AZ-3662 share-link pipeline (AC-1 to AC-5 and AC-77), archived.
**Date**: 2026-11-24

## What changed

- Harness: a `Drive` client in `tests/consumer/stackkit.py` (folders, small uploads with their BLAKE3 hash, edits with a base revision, rename, move, delete; every cookie-authenticated write sends the public origin), `device_token` (the RFC 8628 grant approved by a signed-in browser), `refusal_shape` (what a refused request looks like, without id and date). The consumer image gains `blake3==1.0.11`. The compose stack sets `SUNNYCLOUD_SHARE_WRONG_PASSWORD_WINDOW=10s` (config-driven short duration). `db-probe` gains `share_link_columns` and the share events in `audit_events`.
- `tests/consumer/suites/share_links/test_share_links.py` (5 tests, a visitor with no credentials against the real routes, storage and download path):
  - AC-1: the owner creates a link (the token is in that answer only, the list shows none, the table has `token_hash` and no `token` column); the visitor reads name and size, then the bytes; `nosniff` and an attachment disposition for a text file; `share_created` and `share_accessed` audit rows whose detail keys name no secret or person.
  - AC-2: a password link answers `password_required`; ten wrong passwords are 401, the eleventh is 429 with `Retry-After`, the right password is refused inside the window too, then accepted once the limiter lets it through (204), and the grant cookie downloads the file.
  - AC-3: a valid link, an expired link (test clock), a revoked link and a random token: the three failing ones answer with identical status, headers and body; the revoked one fails within 5 s of the revocation; the valid one is unaffected.
  - AC-4: a device token, a folder target and a cookie request without `Origin` are refused (403, a typed 4xx, 403).
  - AC-77: the link serves the current content after a rename, a move and an edit in place (same node, new revision), and answers 404 within 5 s after the file is deleted.
- `src/server/tests/wiring_flows.rs` (AC-5): `ShareService::{create, list, revoke}`, `PublicShareService::{open, verify_password, authorize_content}` and the download service are called from production routes; one abuse limiter (existing check).

## Findings

- The test clock (`:9091`) moves wall-clock time only. The abuse limiter is a token bucket on the monotonic clock and does not follow it, so "once the limit window passes" cannot be simulated; the test waits the configured 10 s window in real time (polling, at most 13 s). The expired-link case does use the test clock and passes (links expire by wall-clock time).
- The small-file upload route needs the BLAKE3 hash of the file in `X-Blake3`; the OpenAPI document marks the header optional, the server answers 422 `block_hash_mismatch` without it. Documented below as a spec gap.

## Code Review Verdict: PASS (own review)

Test and harness code, one Rust test file; no product change.

## Test Suite

- `tests/run.sh --profile core`: PASS=61 FAIL=0 NOT-RUN=1 (the sentinel), exit 0, about 14 minutes with the image builds.
- CI-parity: `cargo fmt --all --check` and clippy `-D warnings` on `sunnycloud-server`; `wiring_flows` 4 of 4; ctl unit tests unchanged since the last batch (26 OK).

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| OpenAPI: `x-blake3` of `PUT /api/v1/staged-uploads/{staged_key}` is not required, but the server refuses a small upload without it (422 `block_hash_mismatch`) | Mark the header required in the route's spec in 11 (a task for 11; the harness sends it) | clear |
| AZ-3662 AC-2 "once the limit window passes" | Limiter runs on the monotonic clock; asserted with the configured 10 s window in real time | clear |

## Commit

`[AZ-3662] Share links on the compose stack: create, open, password limit, uniform refusals, follows its file; drive client in the harness` - Loop: 1

## Next: AZ-3665 account deletion on the stack (the drive client is in place); then AZ-3661 media outputs
