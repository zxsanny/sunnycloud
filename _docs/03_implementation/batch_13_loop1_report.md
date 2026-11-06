# Batch Report

**Batch**: 13
**Tasks**: AZ-3684_provider_sign_in_callbacks, AZ-3891_local_index_store, AZ-3966_token_lifecycle, AZ-3801_crate_skeleton_accept_loop
**Date**: 2026-11-06

Four parallel workers on disjoint crates (`components/01_identity`, `desktop/daemon`, `sync-core`, `components/11_http_api`). Owner decisions in this session: `rusqlite` pinned to `=0.39.0` instead of `=0.41.1` (see #1 below), and the dated ignore of RUSTSEC-2023-0071 in `deny.toml` kept (#2). Parent edits: the `rusqlite` pin in `src/Cargo.toml`, the wiring of `local_index` into `desktop/daemon` (the worker left it unwired because the old pin did not resolve). Shared files touched: `Cargo.toml` (rusqlite pin, `openidconnect` and `oauth2` without default features), `Cargo.lock`, `deny.toml`, the crates' own manifests, `sync-core` tests (`request_secrets.rs`, `support/route.rs`).

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3684_provider_sign_in_callbacks | Done | `sunnycloud-identity`: `IdentitySignIn` (`begin`, `complete`), GitHub and OIDC clients, guarded outbound HTTP client, 4 statements added to `auth_repo.rs`; in-process ES256 provider for tests | 18 unit + 31 integration | Mobile completion fails closed (E-15/c09); HTTPS handshake untested; `RUSTSEC-2023-0071` ignored |
| AZ-3891_local_index_store | Done | `sunnycloud-daemon::local_index` (SQLite schema, forward-only migrations, store, `CheckpointStore`, rebuild, typed errors) | 27 new (the daemon crate: 57 unit tests) | 5 GiB resume, CPU and the token scan of AC-4 and AC-8 not run; pin 0.39.0; deviations from data model 11 |
| AZ-3966_token_lifecycle | Done | `sync-core`: single-flight refresh on 401, persist-before-use, device grant, mobile code exchange, sign-out; `SyncApi` +5 methods | 16 new; `request_secrets.rs` edited (one case moved) | AC-8 drives block PUTs through the request path (no upload executor yet); wire shapes assumed |
| AZ-3801_crate_skeleton_accept_loop | Done | `sunnycloud-http-api`: `HttpServer`, `ApiServices`, `HttpConfig`, hand-built accept loop (h1 and h2c on one port, connection cap, h2 tuning), Mode S stub-services harness | 5 unit + 18 integration | PT-11-1 not run; h2 preface drip not covered by the header timeout |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 8 findings, none blocking: the outbound guard misses Azure WireServer, Alibaba, Oracle and NAT64-wrapped metadata addresses; the h1 header timeout does not cover an h2 preface drip; a mobile callback consumes the state and then fails with a 500; the persist of rotated tokens has no timeout under the refresh gate; the accept-fault backoff is a hardcoded 1 s; `rebuild` drops pairs and settings; token routes and bodies are assumptions; the outbound HTTPS path is untested.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as in earlier batches). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo test -p sc_observability` without the `db` feature, the dependency-graph check of the worker and decoder, the four wasm32 builds, `cargo deny check advisories bans licenses sources`; in the repo root `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`. All 15 steps RC 0.

## Test Suite

- Total: 1294 (workspace, default features, real PostgreSQL 18)
- Passed: 1292
- Failed: 0
- Skipped: 2 (AZ-3892 AC-7 casefold true branch; AZ-3888 AC-7 UI driver)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `rusqlite =0.41.1` does not resolve with `sqlx 0.9`: both link the native `sqlite3` library, and `sqlx-sqlite` (in the lock whatever the features) needs `libsqlite3-sys <0.38` while 0.41.1 needs `^0.39`; 0.41.1 also needs Rust 1.89 against the workspace `rust-version = "1.88"` (the 14 description says 1.90). Owner decision: pin `=0.39.0` | AZ-3891, 14 description | Update the 14 description; revisit when `sqlx` allows the newer library | clear |
| 2 | `RUSTSEC-2023-0071` (`rsa`, via `openidconnect`): ignored in `deny.toml` until 2027-02-04. The crate only verifies id-token signatures with public keys; the advisory concerns private-key timing. Owner decision: keep | AZ-3684, research F33 | Review the ignore before 2027-02-04 | clear |
| 3 | Sign-in: no "invalid request" variant on `SignInError` (mapped to `TrustedFailure{InvalidRequest}`); mobile `complete` returns an unexpected error until the one-time code (E-15/c09); session `label` is the client kind and `created_ip` is NULL (no user agent or address passed); `sc_ids::new_token` issues no type prefix (open point 1 of `sc_ids.md`), so the "type prefix" part of AC-6 is not run | AZ-3684 | Contracts decision; E-15/c09 | unclear |
| 4 | Outbound rules: loopback and private LAN allowed (spec lists only link-local and metadata); other cloud metadata addresses (Azure 168.63.129.16, Alibaba 100.100.100.200, Oracle 192.0.0.192) not yet refused | AZ-3684 | Follow-up: extend `address_refused` | unclear |
| 5 | Local index deviations from data model 11: `upload_checkpoint` keyed by `local_key`, no `hash_state` (the blocklist is the hash state), `batch` without `pair_id` and `created_at`, cursor is one row, `disk_path` column; one writer under a mutex, not a thread; `file_id` NOT NULL as text `volume:index`; `rebuild` deletes pairs and settings; the 11 MB figure of 14 section 4 is 32 MB for 100,000 entries | AZ-3891 | Update data model 11 and 14 | unclear |
| 6 | Token lifecycle wire shapes assumed: `POST /api/v1/auth/refresh-tokens {refresh_token}`, mobile code body `{code, code_verifier}` with raw code `invalid_code` (no `sc_error` row), device grant bodies, `DevicePoll` tagged body (no RFC 8628 problem-code mapping), sign-out route per credential kind (`/devices/me` or `/sessions/current`); `refresh()` returns `()` | AZ-3966, `sync_api.md` open points 3 and 4 | Pin in `api_v1_surface.md` | unclear |
| 7 | Client behaviour: a revoked session, a device token refused, or a pair the host cannot store stops all work (sticky) until a new sign-in; persist has two attempts and no timeout | AZ-3966 | Record in `sync_api.md` | unclear |
| 8 | HTTP API skeleton: `bind` and `BoundServer` added; `HttpServerError::Metrics`; `ApiServices.readiness` is a `Vec`; `offset_clock` under `test-clock`; `openapi()` is a minimal document; the public listener address has no config key (contract open point 1); listeners serve an empty router until the route tasks | AZ-3801 | Update `http_server.md` | unclear |
| 9 | Carried from earlier batches and still open: tree and batch body shapes, `EditedSince.current`, provider list format, AC-20 numbers, audit-retention interval key, `task` log field, Retry-After cap, image checks never run | batches 2-12 | See their reports | unclear |

## Commit

`[AZ-3684] [AZ-3891] [AZ-3966] [AZ-3801] provider sign-in, local index store, token lifecycle, HTTP API skeleton` — Loop: 1

## Next Batch

Ready now: AZ-3687 device grant exact-once, AZ-3749 quota skeleton (adds a migration), AZ-3723 blobstore skeleton, AZ-3964 verified download, AZ-3965 feed client, AZ-3961 sync hash file (all `sync-core`, one at a time), AZ-3870 and AZ-3871 worker, AZ-3893 platform fs windows.
