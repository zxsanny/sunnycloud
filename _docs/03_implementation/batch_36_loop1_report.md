# Batch Report

**Batch**: 36
**Tasks**: AZ-3828_identity_admin_handlers (partly done: 25 of 26 operations)
**Date**: 2026-11-11

One worker on `11_http_api`, approach A (the batch 34 pattern), as the owner chose before batch 35. No shared crate was edited; `contracts` and `Cargo.lock` are unchanged.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3828_identity_admin_handlers | **Partly done, stays in `todo/`**: 25 operations registered (auth 5, identity link attempt 1, sessions 3, devices 6, account 3, admin 7), each with one port call. `GET /api/v1/device-grants?user_code=` is not registered (item 1, blocked) | `sunnycloud-http-api`: new `registry/signin/`, `sessions/`, `devices/`, `account/`, `admin/`; `registry/request.rs` (`Anonymous` extractor, `query_pairs`, `cookie_value`, `host`, `is_cookie_octet`, `seconds`), `registry/validate.rs` (success 302 allowed), `problem.rs` (generic extras, `ApiProblem::device_poll`), `auth/format.rs` (new `CredentialFormat.binding_cookie`), `accept.rs`, `registry/sharing/grant.rs` (shared cookie helper), `registry/mod.rs`; `api_v1_surface.md` 1.6.0, `problem_details.md` 1.0.5, `http_server.md` 1.5.0 | 42 new tests (41 in `tests/it/identity/` plus unit tests in `signin/cookie.rs`); 81 unit + 1 + 319 `it` (default features), 81 + 1 + 321 `it` with `test-clock` | Items 1-15 |

## Code Review Verdict: PASS_WITH_WARNINGS

No blocking findings. No production `unwrap`, `expect`, `let _ =` or empty catch arm (the four `expect` calls are in the test module of `signin/cookie.rs`); the largest new file is 467 lines. A redaction test proves no secret of a sign-in, device grant or invite reaches a log line or a metric. Existing tests changed because the production route set grew: `registry/support.rs` (the sample surface is now `production_routes()`), `registry/declarations.rs` (the "unknown success status" case uses 301), `auth/credentials.rs` and `harness/mod.rs` (the new binding cookie). Warnings: a new required config field (item 4), cookies not literally from a DTO (item 3), and the blocked lookup (item 1).

CI-parity (all run in this session, `CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, `cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids`, `scripts/ci/check-{pinning,no-sql-now,db-gates,audit-sql}.sh`: all exit 0. Not run: `check-migrations.sh` (needs `sqlx-cli`) and `cargo deny check` (known failure RUSTSEC-2026-0459).

## Test Suite

- Total: 2766 (workspace, default features, real PostgreSQL 18)
- Passed: 2757
- Failed: 0
- Skipped/ignored: 9

## Owner decisions

Waiting for the owner: item 1 (blocks the last operation of AZ-3828) and items 2-12.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | **Blocked.** `DeviceGrantService::lookup_for_approval` takes `&sc_ids::UserCode`, which only `new_user_code()` can build; there is no constructor from the text a person types. Missing: something like `UserCode::from_presented(String)` in `src/shared/ids_hashing/src/token.rs`, with 01 deciding case and hyphen normalisation. The lookup parts of AC-4 (client IP) and AC-5 (429) are untested until it exists. `PortError` already maps `RateLimited` to 429 | AZ-3828 scope, AC-4, AC-5 | Not registered, no stand-in; needs an owner-approved change outside this crate | unclear |
| 2 | `complete_link` has no route in 01's table, so a provider link started by `POST /identity-link-attempts` cannot complete over HTTP | flagged concern | Not registered; surface open point 34; a design gap for the E-15 owner | unclear |
| 3 | There is no cookie DTO: 01 returns `SessionTokens` and a `BrowserBinding` value and excludes cookie attributes, so AC-3's "exactly as the DTO" cannot be literal | AC-3 | Cookie is `<configured __Host- name>=<value>; Secure; HttpOnly; SameSite=Lax; Path=/`, the session cookie adds `Max-Age=<expires_in>`, no `Domain`; logout does not clear it; open point 36 | unclear |
| 4 | The binding cookie name is defined nowhere | AC-2 | New required config field `credentials.binding_cookie` (`__Host-` prefix, must differ from the session cookie); 12 must supply it (`cargo check -p sunnycloud-server --all-targets` is clean) | unclear |
| 5 | RFC 8628 poll answers have no `sc_error` rows | flagged concern, AC-4 | 400 problem body with `code` and an extra `error` member, both `authorization_pending`, `slow_down`, `access_denied` or `expired_token`; open point 41 | unclear |
| 6 | `DELETE /devices/me`: `DeviceOnly` or `SessionOrDevice` | flagged concern | Kept `DeviceOnly`; a session gets 403 at the door; open point 2 | unclear |
| 7 | `GET /api/v1/setup` is not in 01's table | flagged concern | Not registered; `list_providers` carries `setup_required`; open point 8 | unclear |
| 8 | `GET /accounts/me` and its quota: session only (11 section 8) or `SessionOrDevice` (07) | flagged concern | `SessionOrDevice` for both, per the task; the deletion is `SessionOnly`; open point 37 | unclear |
| 9 | How tokens are delivered after code exchange and refresh | flagged concern | Mobile code exchange and refresh answer `{access_token, refresh_token, expires_in}` in the body; the web callback sets only the session cookie, so a browser gets no refresh token; refresh is `Public` with the token in the body, so CSRF does not apply; open points 16 and 38 | unclear |
| 10 | SPA error route and mobile code redirect | AC-2 | `UntrustedState`, `TrustedFailure{code}` and `InvitationRequired` redirect (302) to `/login?error=<reason>`; a `MobileCode` outcome redirects to `return_to` with `code=` added before any fragment; `ProviderUnavailable` stays a problem body; open point 39 | unclear |
| 11 | `InvalidCode` on the mobile code exchange has no `sc_error` row (`PortError` would answer 500) | 01 | The handler answers 400 `invalid_request` (field `code`) itself; `PortError` untouched | unclear |
| 12 | Success statuses beyond the AC list; list defaults | AC-1 | Link attempt, device grant start and invite create answer 200; the admin user list defaults to offset 0 and limit 1000; an invite `note` is required; open point 40 | unclear |
| 13 | Callback query parameters | AC-2 | Form-decoded (`+` is a space), passed in arrival order; a bad escape is 400; a missing binding cookie goes to 01 as an empty binding | clear |
| 14 | The registry validator refused success 302 | AC-1 | Allowed now; a 302 answer may not declare a body schema | clear |
| 15 | Admin target ids (user, invite) in the path | AC-1 | The thing acted on, not the principal's account; the actor is always `Actor::Admin(principal)` | clear |
| 16 | Jira still unreachable this session (hook timeout); AZ-3828 stays `To Do` | tracker gate | Same leftover pattern as batch 35: `_docs/_process_leftovers/2026-11-11_az-3828-tracker-transitions.md` | clear |
| 17 | Items open from batches 18-35 stay open | earlier batches | | unclear |

## Commit

`[AZ-3828] Identity, session, device, account and admin handlers (25 of 26 operations)` — Loop: 1

## Next Batch

Recompute ready tasks: 33 of 252. Besides AZ-3828 (its last operation is blocked by item 1) and AZ-3893 (Windows only), all 31 are decision or research spikes (AZ-3681, 3705, 3721, 3722, 3731, 3732, 3787, 3799, 3849-3851, 3867, 3881-3887, 3913, 3921, 3930, 3933, 3934, 3973, 3985-3988, 4004, 4005) that need an owner decision. Most other tasks wait on AZ-3828, AZ-3893 or those spikes. 252 tasks left.
