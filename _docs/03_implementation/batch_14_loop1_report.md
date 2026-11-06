# Batch Report

**Batch**: 14
**Tasks**: AZ-3686_session_refresh_rotation, AZ-3832_ops_endpoints_readiness, AZ-3903_delete_guard_holds, AZ-3964_verified_download
**Date**: 2026-11-06

Four parallel workers on disjoint crates (`components/01_identity`, `components/11_http_api`, `desktop/daemon`, `sync-core`). Parent edit: the config key `desktop_mass_delete_threshold` (Count, default 200, range 1 to 1,000,000) in `shared/config/src/table.rs` and its row in helper 01, because AZ-3903 stopped at its absence. No allowlist, advisory or schema change in this batch.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3686_session_refresh_rotation | **Partly done** | `sunnycloud-identity`: `IdentitySessions` (`SessionService`: refresh rotation under the row lock, reuse detection, revoke, revoke_all, list, touch), `session_repo.rs`, one system-scope lookup added to `auth_repo.rs` | 15 integration + 5 unit | A refresh with the previous token inside the grace window answers `Unauthenticated` (the successor's plaintext is not stored); the old access token does not keep working until its TTL; no index on `sessions.prev_refresh_hash`. Spec stays in `todo/`, ticket stays `In Progress` |
| AZ-3832_ops_endpoints_readiness | Done | `sunnycloud-http-api`: ops listener on `ops_addr` with the shared `ops_router`, readiness list from `ApiServices`, `Liveness::shut_down` before the accept loops stop, HTTP metric families (`HttpMetrics`) | 27 integration + 5 unit | AC-4: per-route series proven at label-type level only until the route layer (E-21/c05) exists |
| AZ-3903_delete_guard_holds | Done | `sunnycloud-daemon::delete_guard` (counting per direction, hold persisted in `pair.state` and `hold_detail`, root sentinel and root checks, resolve); `entries_under` and `has_entries` added to `local_index` | 38 new (daemon lib: 95) | AC-5 upload during a hold not run end to end; threshold is a constructor argument until the daemon wiring passes the new key |
| AZ-3964_verified_download | Done | `sync-core`: `download` (one ranged GET per 2 MiB block, block verified before the sink, whole file rolled up, resume at a block boundary), `Verified`, `SyncApi::download` | 17 new | Strict 206 with exact `Content-Range`; `finish(false)` after any failure; 5 GiB case scaled to 16 blocks |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 8 findings, none blocking: the grace-path retry cannot be answered with the successor (sync-core then stops all work); the unauthenticated refresh route seq-scans `sessions` through the missing index; refresh expiry slides with no absolute cap; the old access token dies at rotation; a resumed download trusts the blocklist for the earlier blocks; `finish(false)` on cancel may make a host drop the partial; the root sentinel lives in the synced folder; the ops listener shares the public connection cap.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as in earlier batches). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo test -p sc_observability` without the `db` feature, the dependency-graph check of the worker and decoder, the four wasm32 builds, `cargo deny check advisories bans licenses sources`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All 15 steps RC 0.

## Test Suite

- Total: 1379 (workspace, default features, real PostgreSQL 18)
- Passed: 1377
- Failed: 0
- Skipped: 2 (AZ-3892 AC-7 casefold true branch; AZ-3888 AC-7 UI driver)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Refresh grace path: the data model and the spec say a replay of the previous token inside 30 s "creates nothing", but R05 and sync-core (AZ-3966, AC-4) need the lost-response retry to receive the successor, and only its hash is stored. Options: accept the sign-out; re-mint (breaks "one successor" of AC-2/AC-58); or make the successor recoverable: a column holding the successor pair encrypted under a key derived from the presented previous token (no server key), cleared after the grace, plus the index of #3 (one migration, an AEAD dependency) | AZ-3686, R05, NFT-RES-33 | **Owner decision**; AZ-3686 waits for it | unclear |
| 2 | AC-2 clause "the old access token keeps working until its TTL" cannot hold: `sessions` has one `token_hash` and rotation replaces it (a `prev_token_hash` column and an authenticator change would be needed) | AZ-3686 | Relax the clause (the cache TTL bound applies; sync-core repeats a 401) or add the column with the migration of #1 | unclear |
| 3 | Partial index `CREATE INDEX sessions_prev_refresh_hash_idx ON sessions (prev_refresh_hash) WHERE prev_refresh_hash IS NOT NULL` is needed; every unknown token takes the lookup | AZ-3686 | Migration with #1 | clear |
| 4 | `SessionInfo` has no `client_kind` or `created_ip` (the spec's `list` mentions both); `list` returns `id`, `label`, `created_at`, `last_seen_at` | AZ-3686 | Contracts decision | unclear |
| 5 | Refresh expiry slides on each rotation with no absolute session lifetime | AZ-3686 | Owner decision on a cap | unclear |
| 6 | Ops listener: `Listener::Ops`, `BoundServer::ops_addr()`, `HttpServer::http_metrics()` added; shares the global connection cap; per-check readiness timeout is the `sc_observability` default of 2 s (no config key); request-family labels `route` and `status` class are the worker's reading; `prometheus-parse` drops label values with `}` | AZ-3832 | Update `http_server.md`; move the 1000-id cardinality check to E-21/c05 | unclear |
| 7 | Delete guard: counts files per direction (directories not counted); hold stored as submitted paths; `resolve_hold` on a `root_missing` pair is `RootHold`; `RestoreFromServer` returns both lists to the applier (E-24/c12b decides what restore means); scanners and uploaders must skip `.sunnycloud-root-id`; no `list_pairs` in the index | AZ-3903 | Update 14 description and tests.md | unclear |
| 8 | Download: `finish(false)` on every failure including `Cancelled`; strict 206 with exact `Content-Range`; `resume_from` beyond the file is a `ProtocolViolation`; no progress callback; earlier blocks of a resume are trusted from the blocklist | AZ-3964 | Confirm with 14 (applier) and `content_serving_headers.md` | unclear |
| 9 | Carried from earlier batches and still open: provider-linked wire shapes, `EditedSince.current`, audit-retention interval key, `task` log field, mobile sign-in completion, image checks never run | batches 2-13 | See their reports | unclear |

## Commit

`[AZ-3686] [AZ-3832] [AZ-3903] [AZ-3964] session refresh rotation, ops listener, delete guard, verified download` — Loop: 1

## Next Batch

Ready now: AZ-3687 device grant exact-once, AZ-3749 quota skeleton (adds a migration), AZ-3723 blobstore skeleton, AZ-3961 sync hash file and AZ-3965 feed client (`sync-core`, one at a time), AZ-3870 and AZ-3871 worker, AZ-3893 platform fs windows; AZ-3686 completes after the decision in #1.

## Follow-up: AZ-3686 grace-path retry made recoverable (owner decision)

A refresh that repeats the previous token inside `refresh_reuse_grace` (a lost response) now gets the same successor pair back instead of `Unauthenticated`; nothing is created and nothing is revoked, so sync-core's lost-response retry (AZ-3966 AC-4) is consistent with the server and R05 holds. How: migration `20261106100000_add_identity_session_successor` adds `sessions.successor_enc` (bytea) and the partial index `sessions_prev_refresh_hash_idx`. At rotation the successor access and refresh tokens are sealed with ChaCha20-Poly1305 (workspace pin `=0.11.0`) under a key derived as SHA-256 of a domain label and the previous refresh token, with the session id as associated data; the previous token is 256 random bits that the database holds only as a hash, so the database alone cannot open the ciphertext, and a key encrypts exactly one message, so the fixed nonce is never reused. The ciphertext is replaced by the next rotation and cleared on revocation. A row rotated before the column existed (no ciphertext) still answers `Unauthenticated`. Four concurrent refreshes now all receive the same one successor (AC-58 read literally). The data model section 4.3 is updated. Side effects: `sc_db` migration tests derive their version lists from `EMBEDDED` (the next migration will not break them); new transitive crates `chacha20poly1305`, `poly1305`, `cipher`, `inout`, `universal-hash`; `deny.toml` unchanged.

Findings 1 (grace retry) and 2 (index) of the review are closed. Still open for AZ-3686: AC-2 clause "the old access token keeps working until its TTL" (rotation replaces the single `token_hash`; honouring it needs a `prev_token_hash` column and an authenticator change, or the clause is relaxed), `SessionInfo` without `client_kind` and `created_ip`, and the sliding refresh expiry without an absolute cap. The spec stays in `todo/`, the ticket stays `In Progress`.

Gate: all 15 steps RC 0; 1383 passed, 0 failed, 2 ignored.

Commit: `[AZ-3686] sealed successor for the refresh grace retry, session index` — Loop: 1

## Closure: AZ-3686 (owner decision, 2026-11-06)

The AC-2 clause "the old access token keeps working until its TTL" is relaxed: rotation replaces the session's single access hash, so the old access token stops authenticating within `session_cache_ttl` after a rotation, and a client that gets a 401 repeats its request with the new token (sync-core does). No `prev_token_hash` column is added. AZ-3686 is closed with this deviation; the spec records it. Still open as owner questions, not blocking: `SessionInfo` has no `client_kind` or `created_ip`, and the refresh expiry slides with no absolute cap. AZ-3686 is `In Testing`.

Commit: `[AZ-3686] relax AC-2 old-access-token clause, close` — Loop: 1
