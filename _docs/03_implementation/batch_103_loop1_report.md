# Batch 103 (loop 1): additive server fields for the account, device approval and admin pages

Owner decision 2026-11-21: one server change for the gaps the three web pages reported (batch_100 to 102). Only the
changes with no open owner decision were made; the others are listed under "Still open".

| Operation | New fields (all additive, contract 1.11.0) |
|---|---|
| `GET /api/v1/sessions` item | `client_kind` (web, android, ios), `ip` (string or null: the sign-in address; null for sessions created before it was recorded), `current` (true for the caller's own session) |
| `GET /api/v1/device-grants?user_code=` | `platform` (linux, windows, macos) from `device_grants.platform` |
| `GET /api/v1/admin/users` item | `display_name` |
| `POST /api/v1/admin/invites` | `expires_at` next to `token` (`create_invite` returns `CreatedInvite{token, expires_at}`) |

`sessions.created_ip` existed in the schema but was never written: `INSERT_SESSION` now writes it (web sign-in and the mobile code exchange pass the request address). No migration.

## Verification

| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` | clean (re-run by the parent) |
| nextest over identity, admin, http-api, contracts, server (worker) | 1274 run, 1268 passed; 5 `pipeline_db_restore` tests need `pg_dump`, not installed on this host (NOT RUN, environment); 1 `pipeline_realtime` load flake passes alone (14/14) |
| `scripts/ci/check-it-suite.sh`, `check-db-gates.sh`, `check-audit-sql.sh`, `check-no-sql-now.sh` | OK |
| sqlx offline data | not needed: runtime `sqlx::query` strings, no `query!` macros |
| `bun run gen:api --refresh`, web typecheck, lint, tests | snapshot adds only the new properties; web suite green at the time (638 tests) |

New tests: `sign_in_complete::a_web_sign_in_records_the_request_address_and_the_session_list_reports_it`,
`mobile_code::the_exchange_records_the_address_of_the_app_and_the_list_reports_it`,
`session_manage::a_session_created_before_the_address_was_recorded_lists_no_address`, http `sessions::the_session_list_marks_only_the_callers_own_session_as_current_and_shows_no_address_for_an_old_one`,
`device_approval::the_view_carries_the_platform_the_device_asked_with`, `listing::the_display_name_of_the_account_is_carried_to_its_row`,
`invites::a_created_member_invite_reports_when_it_expires`.

## Code Review Verdict

Not run: pending the batch review of the loop.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The new response fields are `required` in the OpenAPI schemas, so they are required in the generated TypeScript types (`ip` is required but nullable) | api_v1_surface.md versioning | accept: a new required response field is non-breaking for clients | clear |
| 2 | A session never has client kind `desktop`: desktop clients use device tokens | AZ-3937 AC-2 | the account page lists desktop devices from `GET /devices` and phone sessions from `GET /sessions` | clear |

## Still open (need an owner decision before code)

1. Provider link completion (open point 34): does the callback choose `complete_link` when a session cookie is present, or a second route; how `IdentityTaken` shows (a new error code or a redirect with `?error=`); whether the link request takes a `return_to`.
2. Admin users: expose `email`? It is stored only when the email allowlist is on and is mostly null.
3. Invites: the invite URL format (DESIGN section 9 item 6), and whether "revoked" must stay visible (needs a soft delete and a migration; today revoke deletes the row).
4. Share default and maximum expiry (`share_default_ttl`, `share_max_ttl`): in the public `server-info`, or in the sharing answers.
5. Default quota value: placement (an admin list field, or per-user `effective_limit`).

## Commit

`[AZ-3937] [AZ-3944] [AZ-3946] Server: sessions list carries client kind, address and current marker; approval carries platform; admin users carry display name; invite create carries expiry` — Loop: 1
