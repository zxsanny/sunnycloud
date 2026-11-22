# Batch 107 (loop 1): AZ-4024 server-side Sign in with Apple provider (roadmap section 7, item 3)

| Task | Status |
|------|--------|
| AZ-4024 | IMPLEMENTED and tested against an in-process mock Apple; NOT final: the live smoke (S8) must confirm the client secret's `aud`/`kid` and the RS256 path, AC-4 step 2 conflicts with an existing owner rule, and Apple token revocation at account deletion is not built. Spec stays in `todo/`, Jira stays To Do |

**Design (decisions of the parent, ADR-008).** The ES256 client-secret JWT is built by hand with `ring` (already a dependency; moved from dev-dependency to dependency of 01; no new crate, `Cargo.lock` unchanged, `cargo deny check` ok): header `{alg: ES256, kid: <key id>}`, claims `iss` = Team ID, `iat`, `exp` (5 minutes), `aud` = `https://appleid.apple.com`, `sub` = the Services ID. It is built fresh for every exchange, so there is no renewal state. The id token is verified with the shared `openidconnect` code (issuer, audience, nonce, expiry; identity = `sub`); that code moved from `oidc.rs` into `provider/openid.rs` so both clients share it. `openidconnect` gains the `accept-string-booleans` feature (Apple sends `email_verified` as text). The authorization URL has `response_mode=query`, `state`, `nonce` and no `scope` or `code_challenge`. The `ProviderClient` trait is unchanged.

**Config (all four or none).** `apple_client_id`, `apple_team_id`, `apple_key_id`, and the file secret `apple_p8` (`SUNNYCLOUD_APPLE_P8_FILE` or `secrets/apple_p8`). A partial set names the missing key; an unparsable `.p8` refuses start naming `providers[i].client_secret`; `apple` is accepted in every build; `facebook` stays refused. `GET /api/v1/auth/providers` items are `{key, display_name, kind}` (api_v1_surface 1.13.0, part1 2.3.0); doctor prints `{public_url}/api/v1/auth/providers/apple/callback`.

## Verification (re-run by the parent)

| Check | Result |
|---|---|
| fmt, clippy `--workspace --all-targets -D warnings`, `cargo deny check` | clean |
| nextest identity, sc_config, http-api, contracts | 966 passed, 1 skipped |
| worker: server and identity | 596 passed; the 5 `pipeline_db_restore` tests need `pg_dump` (not installed): NOT RUN |
| `check-it-suite.sh`, db-gates, audit-sql, no-sql-now, pinning | OK |
| web snapshot and types (`kind` added), web typecheck, lint, tests | green (676 tests) |

Tests per AC: AC-1 web and mobile sign-in and the URL builder; AC-2 client-secret post, new secret each exchange, a scan that the key file and the JWT appear in no log, audit row or error; AC-3 state cases, wrong nonce, audience, issuer, bad signature, reused code, row counts unchanged; AC-4 (see below); AC-5 the provider list names Apple by kind; AC-6 five config tests, three sc_config tests, the doctor line; AC-7 a stalling Apple within the timeout, a redirecting key-set URL with 0 canary hits.

## Code Review Verdict

Not run: pending the batch review of the loop. The Blackbox tier (mock-idp Apple module, E-31, Step 9) is NOT BUILT; the live smoke S8 needs a real Apple account.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | AC-4 step 2 (a valid invite redeemed with a relay email that an allowlist contains) cannot succeed: with an allowlist set, an invite is refused unless the email is verified, non-relay and on the list (`email_allowed`, owner decision 2026-11-13). The worker split the case: with an allowlist, relay and unverified addresses create and link nothing, invite or not; without an allowlist, an invite creates a separate account with no email stored and nothing linked to the victim | AZ-4024 AC-4 | the owner confirms the rule stays and AC-4 is reworded, or changes the rule | unclear (owner) |
| 2 | The in-process mock signs id tokens with ES256 because an RSA key cannot be generated without a new dependency; the signature check is `openidconnect`'s, but the live RS256 path, Apple's real discovery document and a real `.p8` loading in `ring` are covered only by S8 | AZ-4024 Risk 1 | S8 live smoke before release | clear |
| 3 | Apple token revocation at account deletion (guideline 5.1.1(v)) is not built | AZ-4024 flagged concern | owner decision | unclear |
| 4 | AZ-3978 and AZ-3674 should depend on AZ-4024 | AZ-4024 flagged concern | update the dependencies table at the next decompose touch | clear |
| 5 | No `apple_issuer` key: only tests override the issuer, as for `github` | config | none | clear |

## Commit

`[AZ-4024] Server: Sign in with Apple provider with a hand-built ES256 client secret, id-token checks and the provider kind in the list` — Loop: 1
