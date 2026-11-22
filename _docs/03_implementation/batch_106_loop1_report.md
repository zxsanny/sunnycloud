# Batch 106 (loop 1): a password-protected share link opens after the password (AZ-3943 server gap 1)

The web side of AZ-3943 found that `GET /api/v1/public-shares/{token}` answered 401 `password_required` for every live
password link, even after a correct password and the grant cookie (leftover 2026-11-21_az-3943-share-open-points.md).

**Root cause.** `PublicShareService::open` had no grant parameter. The handler in 11 parsed the grant cookie into
`visitor.grant` (the content routes use it) and dropped it, so 06 `open_link`/`decide` answered `PasswordRequired` for
every live password link.

**Fix.** `open(token, grant: Option<&AccessGrant>, client_ip)` (contracts part3 1.6.0). 06 `open_link` accepts the grant
with the same `GrantKey::accepts` the content path uses; `decide(link, target, granted)` answers `PasswordRequired` only
for a protected link without a valid grant of its own (an expired, altered, empty, oversized or other link's grant counts
as absent); a link without a password ignores grants; NotFound handling and counting are unchanged. 11 passes
`visitor.grant`. The wire shape and OpenAPI are unchanged (api_v1_surface 1.12.1).

## Verification

| Check | Result |
|---|---|
| fmt, clippy `--workspace --all-targets -D warnings` | clean (re-run by the parent) |
| sharing and http-api tests (parent re-run) | 643 passed, 4 skipped |
| contracts, http-api (worker) | 598 passed; sharing, admin, server: the 5 `pipeline_db_restore` tests need `pg_dump` (not installed here): NOT RUN |
| `check-it-suite.sh` (20 cases), db-gates, audit-sql, no-sql-now | OK |

Tests added: 06 unit `a_valid_grant_opens_a_password_link_and_no_grant_does_not`; 06 real Postgres `open_answers_a_password_link_with_the_file_only_for_a_valid_grant_of_its_own` (no grant, another link's grant, tampered, expired, valid) and `open_of_a_link_without_a_password_ignores_a_stray_grant`; 11 IT `the_page_of_a_password_link_opens_with_the_cookie_the_verification_set` (open 401 with no file data, verify 204 and cookie, open with the cookie 200, altered cookie 401; the grant value is never printed).

## Code Review Verdict

Not run: pending the batch review of the loop.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | A public-link download is always `SavedUnverified{NoServerHash}`: `ShareOpen` carries no content hash | AZ-3943 AC-6, AZ-3942 | owner decides whether an unlocked visitor may see the hash | unclear |
| 2 | The web page for password links (AC-8) can now be proven end to end; the Playwright row is NOT RUN | AZ-3943 AC-8 | run it with the next acceptance stack | clear |

## Commit

`[AZ-3943] Server: a password link opens with the grant cookie its verification set` — Loop: 1
