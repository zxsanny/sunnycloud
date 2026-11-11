# Batch Report

**Batch**: 35
**Tasks**: AZ-3829_tree_transfer_feed_handlers
**Date**: 2026-11-11

One worker on `11_http_api`. The owner answered the complexity-budget question before the batch: approach A (the batch 34 pattern: one registry row per operation, thin bind, one port call, map; no new shared helper layer) and AZ-3829 first. No shared crate was edited; `contracts` and `Cargo.lock` are unchanged.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3829_tree_transfer_feed_handlers | Done; 22 operations registered (tree 9, transfer 11, sync 2), each with one port call | `sunnycloud-http-api`: new `registry/tree/`, `registry/transfer/`, `registry/feed/`; `registry/request.rs` (`User` carries `principal` and `retained_ok`; shared `query_number`, `query_uuid`, `query_text`, `digest`, `content_length`, `byte_stream`), `json_body.rs`, `timestamp.rs` (`parse_rfc3339`), `registry/mod.rs`, `registry/worker/handlers.rs` and `registry/media/wire.rs` (now use the shared helpers, no behaviour change); `api_v1_surface.md` 1.5.0, `problem_details.md` 1.0.4, `content_serving_headers.md` 1.2.0 | 33 new `datapath::` tests (AC-1 to AC-7) in `tests/it/datapath/`; 77 unit + 277 `it` (default features), 77 unit + 279 `it` with `test-clock` | Items 1-13 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the new code; the largest new file is 416 lines. The commit handlers await nothing between the port's return and the response (AC-69). Existing tests changed because the production route set grew: `registry/support.rs` (the sample surface keeps only the two routes no group registers yet), `middleware/headers.rs` (the unknown-path probe moved to `/api/v1/not-a-route`), `auth/support.rs` (device label `laptop`), `sharing/mod.rs` (`support` is `pub mod`). Warnings: the wire field names are not fixed by any contract (item 3), and two gaps in `contracts` hurt the existing client (items 4 and 5).

CI-parity (all run in this session, `CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`: all exit 0. Not run: `check-migrations.sh` (needs `sqlx-cli`) and `cargo deny check` (known failure RUSTSEC-2026-0459, item 12).

## Test Suite

- Total: 2720 (workspace, default features, real PostgreSQL 18)
- Passed: 2711
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

Asked before the batch and answered: approach A, AZ-3829 first. Waiting for the owner: items 2-11.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | Body class of `revisions:lookup` (about 70 KB for 1,000 hashes) | flagged concern | Existing `Query` class (128 KiB), registered as not state-changing; no new class | clear |
| 2 | A malformed cursor "reaches the port's `BadCursor`", but the port takes a parsed `Cursor` | AC-6 unit row | 11 answers 422 `invalid_cursor` itself with no port call; a missing `cursor` is 400 | unclear |
| 3 | JSON field names of every body are fixed by no contract | all | Followed the DTOs in `sync_core_proto.md` where they exist, added port-only members; strict bodies (an unknown member is 400); documented in `api_v1_surface.md` 1.5.0 | unclear |
| 4 | Client DTOs differ from the ports and 03: batch entry has `parent` and `file_hash` (03: `parent_id`, no hash), `import_hint` is JSON (port: bool), `get_upload` needs `lane_limit` (not in `UploadState`), 02 names the lookup body `hashes` and the answer `hash` | AC-1, open point 29 | 11 follows 03 and the ports (a client sending `file_hash` gets 400); the lookup body uses `content_hashes` and the answer `content_hash`, as the client and `thumbnail-states` do | unclear |
| 5 | `RevisionMatch` has no `size`, but `sync_core_proto::RevisionRef` requires it, so `lookup_revisions` cannot decode the answer; `NodeView` has no `thumb_state`, although 02's route table says listing items carry it | AC-1 | Needs a field in `contracts` (owner approval): not done, no stand-in | unclear |
| 6 | Default `limit` when absent | AC-7 | 1,000 (02's `listing_default_limit`) for children and `find_revisions` | unclear |
| 7 | `GET /revisions` is described as keyset-paged, but the port takes `Page{offset, limit}` | Tree row | Query `offset` and `limit`; answer `{items}` with no `next` | unclear |
| 8 | `PATCH` with neither `name` nor `parent_id`; `client_mtime` needs a parser | Tree, commit | Passed to 02 as is; `parse_rfc3339` added (UTC `Z` only, up to 6 fractional digits) | unclear |
| 9 | Staged PUT with no `Content-Length` (empty body on HTTP/1.1 or HTTP/2); `retained_ok` on `/nodes/{id}/content` | stage_small, AC-5 | Exact body size if known, else 411; `retained_ok` is always false on the node route and true only on the revision route for an admin session | unclear |
| 10 | The shutdown test still holds worker `claim` calls as the long-poll stand-in (batch 33 item 1); the real route now exists | AZ-3833 | Not changed; graceful wake of feed polls is out of scope (c17) | unclear |
| 11 | `Principal.label` of a session principal is whatever 01 puts there (`test` in the double); the commit's `device_label` is that value | AC-4 | Used as is | unclear |
| 12 | `cargo deny check` fails with RUSTSEC-2026-0459 (`libsqlite3-sys` through `rusqlite` in `sunnycloud-daemon`); reported since batch 31 | gate | Owner decision | unclear |
| 13 | Jira is unreachable: `getJiraIssue` failed twice (hook timeout), so AZ-3829 was not transitioned | tracker gate | Leftover `_docs/_process_leftovers/2026-11-11_az-3829-tracker-transitions.md`; `Tracker: pending` | clear |
| 14 | Items open from batches 18-34 stay open | earlier batches | | unclear |

## Commit

`[AZ-3829] Tree, transfer and feed handlers` — Loop: 1

## Next Batch

Recompute ready tasks. Still only `11_http_api`: AZ-3828 identity admin handlers (5 points; the owner's approach A answer covers it). AZ-3893 is Windows only. 252 tasks left.
