# Sign-in, session and invite pipeline (F2)

**Task**: AZ-3664_pipeline_sign_in
**Name**: Sign-in pipeline
**Description**: Browser and mobile sign-in through OIDC or GitHub creates accounts from invites, initializes every per-account row and issues sessions, end to end against the real components
**Complexity**: 5 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3784_sc_audit, AZ-3785_sc_ratelimit, AZ-3801_crate_skeleton_accept_loop, AZ-3806_auth_extraction, AZ-3808_csrf_origin_check, AZ-3828_identity_admin_handlers, AZ-3789_composition_root, AZ-3790_per_command_config, AZ-3791_serve_start_stop, AZ-3794_identity_operator_commands, AZ-3683_authenticator_scopes_cache, AZ-3684_provider_sign_in_callbacks, AZ-3685_invite_redemption_account_creation, AZ-3686_session_refresh_rotation, AZ-3689_mobile_code_identity_janitor, AZ-3691_identity_abuse_limits_audit, AZ-3694_provider_linking_and_list, AZ-3751_quota_admin_lifecycle, AZ-3778_feed_horizon_lifecycle, AZ-3652_initial_structure; notes: E-31/c13 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3664
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p02

## Problem

Sign-in crosses the delivery layer (11), identity (01), the account lifecycle hooks of other components (07a quota row, 05 feed row), the shared abuse limiter and the audit log. Each is tested alone; the hook list and the limiter are only joined in the composition root. No planned task proves that a real callback through the real router creates a complete account, or that the cookie, CSRF and refresh rules hold through the real stack.

## Outcome

- A sign-in through the real `serve` process creates the account, its identity and every per-account row registered by 12, and returns a working session (cookie for the browser, one-time code then tokens for mobile).
- Every component operation of flow F2 has a production caller.
- At least one integration test runs F2 through the real binaries over HTTP with only the provider mocked.

## Scope

### Included
- Flow order and seams (`system-flows.md` F2 steps 1-7):
  1. Login route (relative return path, Host equals the public URL) → `SignInService::begin` → state row bound to a browser cookie → redirect to mock-idp.
  2. Callback route → `SignInService::complete` → token exchange and validation (OIDC: signature, issuer, audience, nonce; GitHub: `/user`, `/user/emails`).
  3. Unknown identity with an invite → invite redeemed and account created in one transaction → `AccountLifecycleHook::initialize` for every hook in the list registered by 12 (07a quota row, 05 feed row; others no-op).
  4. `SessionService` issues opaque tokens → cookie → redirect; `SessionService::refresh` rotation; `Authenticator::authenticate` and `require` on later requests; CSRF origin check on cookie-authenticated state changes.
  5. Mobile: `begin` with mobile client kind and code challenge → `complete` returns a one-time code → `exchange_mobile_code` → session tokens.
  6. Link-provider (session-only) through `begin_link` / `complete_link`.
  7. The single `AbuseLimiter` built by 12 counts failed steps (429 with Retry-After); sign-in events go to the audit log with ids only.
- `admin invite create` and the admin invite route as the producers of invites for the test.
- The end-to-end integration test of the steps above.

### Excluded
- Sign in with Apple (v1.1), the first-admin setup token (E-22/p01), the device grant (E-24/p01), the mobile app side (E-27/p01), provider-client internals (E-15).

## Acceptance Criteria

AC-1..AC-6 are local to this task. AC-75 keeps its number from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Invite sign-in creates a complete account**
Given an operator invite and mock-idp answering `valid` for a new identity
When the user signs in through the login and callback routes of the real server
Then one account and one identity exist, every registered lifecycle hook initialized its row in the same transaction (quota row and feed row present), and the session cookie authenticates the account route

**AC-2: Rejections through the real router**
Given mock-idp scripted with the 12 negative callback cases, `unverified-victim-email` and a provider slower than 10 s
When each sign-in is attempted
Then each case returns non-2xx with no session, no account and no off-site redirect; an unverified email is ignored; the slow provider fails the sign-in within the timeout while an existing session keeps working

**AC-3: Session rules hold end to end**
Given a signed-in browser
When four refreshes with the same refresh token run concurrently, the old token is replayed after the 30 s grace (test clock), and a cookie POST is sent without `Origin` and `Sec-Fetch-Site`
Then exactly one successor is issued, the replay revokes the token family within 5 s, and the POST gets 403

**AC-4: Mobile one-time code**
Given a mobile sign-in started with a code challenge
When the callback completes and the code is exchanged with the verifier, then exchanged a second time
Then the first exchange returns session tokens and the second is refused

**AC-5: Shared limiter and audit**
Given one client IP
When 11 sign-in steps fail within a minute
Then the 11th gets 429 with Retry-After, and the audit log holds the sign-in events with ids only

**AC-6: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `SignInService::{begin, complete, exchange_mobile_code, begin_link, complete_link, list_providers}`, `SessionService::{refresh, revoke}`, `Authenticator::{authenticate, require}`, `InviteService` create, `AccountLifecycleHook::initialize` (through the list registered by 12), and exactly one `AbuseLimiter` construction (in 12)

### Interaction criteria (Step 1.8)

**AC-75: One invite, two concurrent redemptions, one account** (R8)
Given one unused invite and two browsers signing in with two different provider identities
When both callbacks reach the real server at the same moment (repeated 20 times with fresh invites)
Then each time exactly one account exists with exactly one quota row and one feed row, the other sign-in is refused, and it leaves no account, identity, quota or feed row behind

## Non-Functional Requirements

**Performance**
- Callback to session within 1 s at p95 with a local provider (excluding provider time).

**Reliability**
- Provider outage never affects existing sessions (AC-21).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over non-test sources | Each named operation has a production caller; one `AbuseLimiter` construction |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | One invite, mock-idp `valid` | Browser sign-in | Account, identity, quota row, feed row; cookie works | |
| AC-2 | mock-idp negative cases, unverified email, slow provider | Each sign-in | Non-2xx, no session/account/off-site redirect; old session works | AC-21 |
| AC-3 | Signed-in browser, test clock | Concurrent refresh, replay after grace, cookie POST without origin | One successor; family revoked within 5 s; 403 | |
| AC-4 | Mobile begin with code challenge | Exchange twice | Tokens once, second refused | |
| AC-5 | One IP | 11 failed steps in a minute | 429 with Retry-After; audit ids only | |
| AC-75 | One invite, two identities | Two concurrent callbacks, 20 rounds | One account, one quota row, one feed row; loser leaves no rows | NFT-SEC-48 |

## Constraints

- Owner: component 12 (`src/server/`): wiring of the identity services, hook list and limiter for this flow, and the flow's end-to-end tests among 12's integration tests. No SQL, blob I/O or business rule in 12; a missing behaviour goes back to the component's task.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 over HTTP; only the provider is mocked (mock-idp); no sunnycloud component is faked.
- The core redemption transaction and its concurrency suite stay in E-15/c05; AC-75 here checks the cross-component outcome with the real hook list.
- Downstream: E-31/c30, E-31/c32, E-31/c34 (FT-P-62, FT-P-63, FT-N-21..23, FT-N-26, FT-N-28..30) and E-31/c38 (NFT-SEC-48) should depend on this task.

## Risks & Mitigation

**Risk 1: Hook initialization outside the account transaction**
- *Risk*: the composition root runs hooks after commit, so a lost race leaves orphan quota or feed rows.
- *Mitigation*: AC-75 counts rows of every hook after each concurrent round.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `register_hooks` name in 01's interface is still a spike (E-15/c01) | E-15 | open | Low |
| Sign in with Apple is v1.1 and has no E-15 child yet | E-15 / owner | open | Low |
