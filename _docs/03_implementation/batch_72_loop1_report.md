# Batch Report

**Batch**: 72
**Tasks**: AZ-3789_composition_root (done; running the real `serve` binary moved to AZ-3791)
**Date**: 2026-11-15

The last part of AZ-3789 after its blockers (AZ-4309, AZ-4310, AZ-4311) landed in batch 71. One worker on the server crate and `sc_config`; the parent reviewed, rebuilt the test database after a host restart, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3789_composition_root | Done | `root::api_services` assembles 11's `ApiServices` from the same lazy builders every other consumer uses: one instance per port (01 authenticator, sign-in, sessions, device grants, invites; 02 tree and reader; 03 uploads, commits, downloads; 05 change feed with the feed shutdown token; 06 shares and public shares; 07 admin, accounts, account deletion; 08 thumbnails, derived, worker auth, worker jobs; 20 vault only with feature `vault`), plus readiness, metrics and clock. The account hook list is built first. New keys `identity_janitor_interval` (5m) and `audit_retention_interval` (1h) schedule 01's `IdentityMaintenance` and 12's `AuditRetentionJanitor`; the janitor list is 03, 04, 02, 07, 01, 08, audit retention. AC-1..AC-6 PASS in process (`api_services.rs`, `composition_root.rs`) |

## Moved to AZ-3791

Running the real `serve` binary (AC-1 through the binary, the `serve` half of AC-5) needs `HttpServer::new` with an `HttpConfig` whose listener address and cookie names had no key. `serve` itself, its start order and listeners are AZ-3791's scope. Owner decisions 2026-11-15, recorded in AZ-3791:
- keys `public_addr` (`0.0.0.0:8080`), `session_cookie` (`__Host-session`), `binding_cookie` (`__Host-signin-binding`);
- the two shutdown steps with no port are added inside AZ-3791: 03 saves every upload's unsaved progress now; 04 waits for every open sync group.

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, server clippy with `--features vault`, `check-no-sql-now`, `check-db-gates`, `check-audit-sql`, `build-wasm` exit 0; `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`). `cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18: 3714 passed, 4 failed, 31 ignored; the 4 failures were all in 05's `feed` binary with "too many clients already" (environment, see below); after raising `max_connections` to 400 the binary passed 91 of 91. Total 3718 passed, 0 failed, 31 ignored.

Environment (recorded):
- A host restart removed the `sunnycloud-pg18` container (not stopped: gone). Recreated from `scripts/ci/README.md` on port 57432; the old container ran with `max_connections=400`, which the README did not say. The README command now carries `-c max_connections=400`.
- The session scratchpad was cleared by the restart; the gate script was rebuilt.

Warnings (recorded):
- The server's `test-clock` feature does not turn on 11's `test-clock`, so the `-test` api image would get no `OffsetClock` and no `:9091` listener; `test_clock_addr` has no key (http_server open point 4); `--all-features` would not compile the server. Belongs to the `-test` image task.
- `update_url` has no key; `HttpServer::new` refuses a `min_client_version` without one (`server_info.md` open point 2).
- 11's OpenAPI document hardcodes `__Host-session` while the name comes from config; consistent with the owner's default, but a changed key would not reach the document.

## Commit

`[AZ-3789] Composition root: API services and janitor keys` — Loop: 1

## Next Batch

AZ-3791 `serve` start and stop sequence (owner decisions above), alone: it touches the server crate, `sc_config`, 03 and 04.
