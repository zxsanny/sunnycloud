# Batch Report

**Batch**: 37
**Tasks**: AZ-3828_identity_admin_handlers (last operation; the task is now complete, 26 of 26)
**Date**: 2026-11-11

The owner approved the one change outside `11_http_api` that batch 36 item 1 asked for: `UserCode::from_presented` in `sc_ids`. Nothing else outside `11_http_api` and `sc_ids` was edited; `contracts` and `Cargo.lock` are unchanged.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3828_identity_admin_handlers | **Done**: `GET /api/v1/device-grants?user_code=` registered (session only), one `lookup_for_approval` call with the principal, the parsed code and the client address | `sc_ids`: `UserCode::from_presented(&str) -> Option<UserCode>` in `token.rs`; `sunnycloud-http-api`: `registry/devices/` (`lookup` handler, `approval` wire and schema, route row), `registry/request.rs` (`User` carries `client_ip`); contracts `api_v1_surface.md` 1.7.0 (Open point 35 closed), `sc_ids.md` 1.1.0 | 2 new `sc_ids` tests, 3 new `it` tests (forwarded address and normalised code, device / missing / malformed refusals, 404 and 429 mapping) | Items 1-3 |

## Code Review Verdict: PASS

No findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm in the new code. A code that cannot exist is a 400 for `user_code` at the door and never reaches the port; a missing parameter is a 400 too.

CI-parity (all run in this session, `CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`: all exit 0. Also `cargo test -p sunnycloud-http-api --features test-clock`: 81 + 1 + 324 passed. Not run: `check-migrations.sh` (needs `sqlx-cli`) and `cargo deny check` (known failure RUSTSEC-2026-0459).

## Test Suite

- Total: 2771 (workspace, default features, real PostgreSQL 18)
- Passed: 2762
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

Closed: batch 36 item 1. Still waiting for the owner: batch 36 items 2-12 (item 2 is the missing `complete_link` route), batch 35 items 2-11, and batches 29-34.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | `UserCode::from_presented` ignores case, hyphens and spaces and accepts exactly 8 base-20 characters; 01 stores the code upper case without a hyphen, so the normalised text matches the stored one | AC-4 | Owner-approved; `None` for anything else | clear |
| 2 | The answer body (`grant_id`, `expires_at`, `device`, both addresses and locations, `mismatch`) mirrors `ApprovalView`; the requester's address is shown on purpose, the approval page compares it with the approver's | AC-4 | Documented in `api_v1_surface.md` 1.7.0 | unclear |
| 3 | `User` now needs the client address layer (`ClientIp`); a stack without it answers a missing-layer 500 for every authenticated route | tree of extractors | All production stacks have the layer; tests green | clear |

## Commit

`[AZ-3828] Device approval lookup and UserCode::from_presented` — Loop: 1

## Next Batch

Recompute ready tasks: AZ-3828 is done. What is left that is ready: AZ-3893 (Windows only) and the decision spikes (AZ-3681, 3705, 3721, 3722, 3731, 3732, 3787, 3799, 3849-3851, 3867, 3881-3887, 3913, 3921, 3930, 3933, 3934, 3973, 3985-3988, 4004, 4005), which need an owner decision. 251 tasks left.
