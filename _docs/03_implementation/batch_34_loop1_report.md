# Batch Report

**Batch**: 34
**Tasks**: AZ-3830_share_media_handlers
**Date**: 2026-11-11

One worker on `11_http_api` (the owner picked AZ-3830 from the three remaining ready tasks of that crate). No shared crate was edited by the worker.

Addendum (same day): the owner approved the `contracts` change for item 1, so the parent added `ShareDerived{account_id, file}` as the result of `PublicShareService::authorize_derived` and registered the share-thumbnail route (7 new tests in `tests/it/sharing/share_thumbnail.rs`, two rows in the registry test); AC-5 is covered.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3830_share_media_handlers | Done; the share-thumbnail route (AC-5) was added after the batch with the owner-approved `contracts` change | `sunnycloud-http-api`: `registry/sharing/` (7 route rows: owner create, list, revoke; visitor open, password verification, content), `registry/media/` (5 rows: node thumbnail, transcode, thumbnail-states), `registry/request.rs` (shared extractors), `content/request.rs` (`download_options`, `starts_download`), `timestamp.rs`, `ApiProblem::code` and `::not_found`; `api_v1_surface.md` 1.4.0, `content_serving_headers.md` 1.1.0, `problem_details.md` 1.0.3, `http_server.md` 1.4.1 | 75 unit + 237 `it` (default features), 75 unit + 239 `it` with `test-clock`; new `tests/it/sharing/` (owner, uniform, password, range, media, redaction, registry) | Items 1-8 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the new code (the `expect` calls are in test modules); no file over 500 lines. The worker handlers and `caller.rs` now use the shared request helpers (a small refactor, no behaviour change). Two existing tests were changed because the production route set grew (`registry/support.rs`, `csrf/reads.rs`). Warnings: thumbnail-states takes content hashes, not node ids (item 2); the derived media type is a guess (item 6).

CI-parity: all gates passed except `cargo deny` (item 9). In `src/` with `CARGO_INCREMENTAL=0`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. Not run: `check-migrations.sh` (needs `sqlx-cli`). The first full test run had one failure (`serve_refuses_a_retention_above_ninety_days_naming_the_range`, item 8); it passed alone and the whole test step was re-run green.

## Test Suite

- Total: 2678 (workspace, default features, real PostgreSQL 18)
- Passed: 2669
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

None asked this batch. Approved after the batch: the `ShareDerived` change (item 1). Waiting for the owner: items 2-7 and 9.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | Closed after the batch (owner approval): `PublicShareService::authorize_derived` returns `ShareDerived{account_id, file}` (`contracts` part 3 1.1.0, `derived_access.md` 1.0.2, `api_v1_surface.md` 1.4.1) so 11 has the owning account for 08 and 03; `GET` and `HEAD /api/v1/public-shares/{token}/thumbnail?size=small\|preview` (default `small`, other values 400) call 06, then 08 `open_derived_for`, then 03 `open_blob_content(account_id, blob, image/webp)`, with no cross-origin header. 06 does not implement the port yet, and 06's own route table still lacks the row | AZ-3830 AC-5 | Done | clear |
| 2 | `thumbnail-states` takes content hashes, not node ids: no port maps many node ids to hashes in one call (`TreeReader::get_node` is one node per call, which would break "one port call per handler"). Body `{content_hashes: [64 hex]}`, answer `{states: {hash: ready\|pending\|failed}}`; more than 1,000 hashes is 422 `batch_too_large`, a non-hash is 400; a hash with no state (no job planned, unknown, or another account's) is left out. Moving to node ids needs a bulk node-id method in a port and is a breaking wire change before any client ships | AZ-3830 flagged concern, G-08-3 | Worker's reading | unclear |
| 3 | Grant cookie: `__Secure-share-grant=<expires_at in microseconds>.<value>; HttpOnly; Secure; SameSite=Lax; Path=/api/v1/public-shares/{token}; Max-Age=<remaining>`, no `Domain`; the expiry is carried in the value because 06 wants the whole `AccessGrant` back; a missing or malformed cookie counts as no grant. Password route is `.../password-verifications` (06's name) and answers 204 with no body | AZ-3830 flagged concerns | Worker's reading; `api_v1_surface.md` open points 22-28 | unclear |
| 4 | CORS: `Access-Control-Allow-Origin: *` only on the 200, 206 and 304 of `GET` and `HEAD` share content; errors, thumbnail and all other routes carry none; no `Access-Control-Expose-Headers`; an `OPTIONS` is 405 | AZ-3830 AC-6 | Worker's reading | unclear |
| 5 | `Range`: `ByteRange` has two ends, so it cannot hold a suffix or open-ended range. `bytes=a-b` is passed as is; `bytes=a-` becomes `end = u64::MAX`, which needs 03 to clamp (resumable clients depend on it); suffix ranges, several ranges and other units are ignored and the whole content is served. Owners of E-14 and E-21 extend the type or confirm the clamp | AZ-3830 AC-4 | Worker's reading | unclear |
| 6 | Derived media type: `BlobRef` and `DerivedOpener` carry none, so 11 passes `image/webp` for thumb and preview and `video/mp4` for transcode. Wrong for JPEG stills if 08 accepts them | AZ-3830 AC-6 | Worker's reading | unclear |
| 7 | Timestamps are RFC 3339 UTC with microseconds, formatted in a new `timestamp.rs` because `sc_observability`'s formatter is crate-private (a duplicate). Share bodies: `{node_id, expires_in (seconds), password}` in; `{id, token, url_path, expires_at, has_password}` out; `{items: [...]}` for the listing. `HEAD` on share content is not a download start (`starts_download` false), so a probe writes no audit event. Wiring for AZ-3789: the real 06, 08 and 03 implementations, 06 must accept the grant as `{value, expires_at}`, and `ClientIp` and `trusted_proxies` must be configured | AZ-3830 | Worker's reading | unclear |
| 8 | The first full test run failed `serve_refuses_a_retention_above_ninety_days_naming_the_range` (`server/tests/config_binary.rs:121`, "a listener opened before the refusal"). The test probes two addresses while the binary starts; this machine has other processes listening on ephemeral ports, which is the likely cause (not confirmed). It passed alone (9 of 9) and the full test step passed on a re-run. The crate is untouched by this batch | gate | Reported, no change | unclear |
| 9 | `cargo deny check` fails with RUSTSEC-2026-0459 (`libsqlite3-sys` through `rusqlite` in `sunnycloud-daemon`); reported in batches 31 to 33 | gate | Owner decision | unclear |
| 10 | Items open from batches 18-33 stay open | earlier batches | | unclear |

## Commit

`[AZ-3830] Share and media handlers` — Loop: 1

## Next Batch

Recompute ready tasks. Still only `11_http_api` tasks (one at a time): AZ-3828 identity admin handlers and AZ-3829 tree, transfer and feed handlers (5 points each, which need the complexity-budget question first). AZ-3893 is Windows only. 253 tasks left.
