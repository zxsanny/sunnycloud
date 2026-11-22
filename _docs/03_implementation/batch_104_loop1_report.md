# Batch 104 (loop 1): owner decisions of 2026-11-22 on the server gaps of batch 103

| Change | Result (contract api_v1_surface 1.12.0, server_info 1.1.0, admin_service 1.5.0) |
|---|---|
| Link completion (open point 34) | `GET /auth/providers/{provider}/callback` stays `Public` and authenticates the session cookie inside its handler (same `Authenticator` port and `SessionOnly` scope). A live cookie calls `complete_link`: `Linked` (idempotent repeat too) is 302 `/account?linked=1`; `IdentityTaken` is 302 `/account?error=identity_taken`; `TrustedFailure`, `InvitationRequired` are 302 `/account?error=<reason>`; `ProviderUnavailable`, `RateLimited`, `HostMismatch` keep the port's answer. No cookie, or a stale one: plain sign-in as before. `UntrustedState` from `complete_link` falls back to the plain sign-in (the parent added this: a state that is not a link state of this account is looked up by another hash and consumes nothing, so a web session beside a mobile sign-in still signs in) |
| Invite URL | `POST /admin/invites` answers `{token, expires_at, url}`, `url` = `<public_url>/invite#<token>` built in the delivery layer; revoke stays a hard delete |
| Share limits | `GET /server-info` gains `share_default_ttl`, `share_max_ttl` (whole seconds, like `longpoll_max_wait`) |
| Default quota | `GET /admin/users` answers `{items, default_quota_bytes}`; `list_users` returns `AdminUserPage{items, default_quota_bytes}` and 07a `QuotaAdmin` gains `default_quota_bytes()` (larger edit than the field: about 15 files of doubles and call sites, because a second port method broke the one-port-call-per-handler rule) |

## Verification

| Check | Result |
|---|---|
| `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings` | clean |
| nextest over http-api, contracts, admin, quota, identity, server, sync-core-proto (worker) | 1419 run; the 5 `pipeline_db_restore` tests need `pg_dump` (not installed here): NOT RUN; `it_07_11_cycles_stop_at_the_budget` failed once under host load (load average 32 from other users) and passed alone |
| http-api crate after the parent's fallback | 504 run, 504 passed |
| `scripts/ci/check-it-suite.sh`, db-gates, audit-sql, no-sql-now | OK |
| OpenAPI snapshot and web types regenerated | adds only `url`, `default_quota_bytes`, `share_default_ttl`, `share_max_ttl` and the callback summary |
| web typecheck, lint, tests | clean; one loaded run failed and a rerun alone passed (639 tests); the failing test of the loaded run was not recorded |

New tests: five in `identity/link.rs` (live cookie links as that account; refusal redirects; `ProviderUnavailable` keeps the port answer; stale cookie is a plain sign-in; failing session check reaches neither port) plus `a_session_cookie_beside_a_sign_in_state_still_finishes_the_sign_in`; `the_invite_url_is_built_from_the_configured_origin_with_one_trailing_slash_trimmed`; the server-info field-list tests (ten fields) and a sub-second-remainder test; `a_page_carries_the_installation_default_limit_that_07a_holds_even_when_it_is_empty`; `the_port_names_the_installation_default_it_was_configured_with`.

## Code Review Verdict

Not run: pending the batch review of the loop.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The route's owner port-method string stays `SignInService::complete` because the "GET never mutates" exemption is keyed on it | api_v1_surface open point 3 | none | clear |
| 2 | The web pages do not yet use any field of batches 103 and 104 | AZ-3937, AZ-3944, AZ-3946, AZ-3943 | a web batch wires them in and completes the partial acceptance criteria | clear |

## Commit

`[AZ-3937] [AZ-3944] [AZ-3946] [AZ-3943] Server: link completion in the provider callback, invite URL, share limits in server-info, default quota on the admin user list` — Loop: 1
