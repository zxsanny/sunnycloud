# Server-side Sign in with Apple provider [v1.1]

**Task**: AZ-4024_sign_in_with_apple_provider
**Name**: Sign in with Apple provider
**Description**: Add the `apple` provider kind: authorization with no scopes and `response_mode=query`, no PKCE towards Apple, a code exchange authenticated by an ES256 client-secret JWT that the server builds from the `.p8` key itself, and id-token validation against Apple's keys, all behind the unchanged `ProviderClient` trait of AZ-3684
**Complexity**: 5 points
**Dependencies**: AZ-3684_provider_sign_in_callbacks, AZ-3682_identity_crate_migration_config, AZ-3685_invite_redemption_account_creation, AZ-3689_mobile_code_identity_janitor, AZ-3694_provider_linking_and_list, AZ-3691_identity_abuse_limits_audit; notes: E-31/c13 (test epic, Step 9: mock-idp Apple module)
**Component**: 01 identity [v1.1]
**Tracker**: AZ-4024
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/n02

## Dependencies

### Document Dependencies
- ADR-008 (built-in OIDC sign-in; "v1.1 adds Sign in with Apple (hand-written ES256 client secret, no scopes, no PKCE)").
- `_docs/00_research/02_fact_cards/mve_crypto_auth_mobile.md` — "MVE — openidconnect 4.1.0 + oauth2 5.0.0": Apple row and the Apple client-secret, form_post, PKCE and ID-token notes.
- `_docs/02_document/system-flows.md` F2 and the "Sign in with Apple [v1.1]" integration row.
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `SignInService`, `ProviderList`.

## Problem

AC-19 makes Sign in with Apple a v1.1 provider, and the iOS app may show Google or GitHub only when the server has Apple enabled (App Store guideline 4.8, RESTRICT-21). No task adds it on the server: AZ-3684 builds `github` and `oidc` and requires that "Apple (v1.1) is added without changing the `ProviderClient` trait"; the iOS sign-in task AZ-3978 flags the gap. Apple differs from plain OIDC: the client secret is a short-lived ES256 JWT signed with the operator's `.p8` key, scopes force a `form_post` callback, and PKCE is not documented, so the flow is a confidential client protected by `state`, `nonce` and the secret (fact card).

## Outcome

- A provider of kind `apple` signs a known identity `(apple, sub)` in through the same `begin` and `complete` paths as the other kinds, and redeems invites and the setup token the same way.
- The authorize URL carries `client_id` (the Services ID), `redirect_uri` from `public_url`, `response_type=code`, `response_mode=query`, `state` and `nonce`, no `scope` and no `code_challenge`.
- The token request uses `client_secret_post` with an ES256 JWT: `iss` = the 10-character Team ID, `sub` = the client id, `exp` at most 15,777,000 s ahead; the server regenerates it itself before it expires, so the operator has no six-month manual step (`environment_strategy.md`).
- The id token is verified with Apple's RS256 keys: issuer `https://appleid.apple.com`, audience = client id, nonce, expiry; the identity is the `sub`; an unverified or relay email is treated as no email (AC-55).
- The `ProviderClient` trait of AZ-3684 is unchanged; `apple` is one more implementation selected by kind, with the same timeout and outbound rules.
- `list_providers` lists the Apple provider, so the iOS rule (AZ-3978) can see that it is enabled; the doctor providers check (E-15/n01) prints its redirect URI.

## Scope

### Included
- The `apple` provider kind in configuration and validation (`facebook` stays refused, FT-N-28), the `apple_p8` file secret (`containerization.md`).
- The Apple `ProviderClient`: authorize URL, client-secret JWT and its renewal, code exchange, id-token validation, mapping to `(provider, provider_user_id)`.
- The callback negative cases that apply to Apple ("Same as OIDC", system-flows integration row).
- The redirect-URI line for Apple in the providers doctor check.

### Excluded
- The iOS app's sign-in and provider rule (AZ-3978); the mobile one-time code and its PKCE between app and server (AZ-3689; unchanged, it is not the provider leg).
- A `form_post` callback route and the first-login `user` JSON (not needed with no scopes, fact card).
- Apple token revocation (guideline 5.1.1(v); see Flagged concerns).

## Acceptance Criteria

**AC-1: Sign in with Apple (AC-19, FT-P-63 step 2)**
Given mock-idp's Apple module configured as provider key `apple`, and an existing identity `(apple, sub-1)`
When `begin` runs for a web client and for a mobile client kind with a `code_challenge`, and `complete` runs with the module's valid callback
Then both return a session for the identity's account; the authorize URL has `response_mode=query`, `state`, `nonce` and the `public_url` redirect URI, and has no `scope` and no `code_challenge` parameter; the mobile run ends in the one-time-code outcome of AZ-3689

**AC-2: Client-secret JWT (fact card "Apple client secret")**
Given the test team key, Team ID and client id
When the code exchange runs, and again after the clock moves past the secret's renewal point
Then the token request is a form POST with `client_id`, `client_secret`, `code`, `grant_type` and `redirect_uri`; the secret is an ES256 JWT that the mock validates against the test team key, with `iss` = the Team ID, `sub` = the client id and `exp` no more than 15,777,000 s after `iat`; the second exchange uses a newly generated secret; the `.p8` key bytes and the JWT appear in no log, audit row or error

**AC-3: Id token checks and negative callbacks (AC-56, IT-01-5 cases 1-11 with Apple)**
Given one `begin` per case with the Apple module
When `complete` runs with: missing, replayed, foreign and expired state; wrong nonce; wrong audience; wrong issuer; a bad RS256 signature; a reused code
Then the state cases return `UntrustedState`, the token and id-token cases return `TrustedFailure{code}`, and the row counts of `sessions`, `accounts`, `identities` and `login_codes` are unchanged

**AC-4: Email is never a key (AC-55, NFT-SEC-02 Apple-like case)**
Given the Apple module returning a relay address with `email_verified=false`, an allowlist that contains that address, and a victim account with the same address
When an unknown Apple identity signs in without an invite, and then redeems a valid invite
Then the first creates nothing, the second creates a separate account, and no identity is linked to the victim

**AC-5: Trait unchanged and provider listed (AZ-3684 constraint, AC-19)**
Given the v1.0 `ProviderClient` trait definition and a server configured with `apple` and `gh`
When the v1.1 build is compared and `list_providers` runs
Then the trait has no added, removed or changed method, and the list contains both providers in configured order with a field from which a client can tell that Sign in with Apple is enabled

**AC-6: Configuration (FT-N-25, FT-N-28)**
Given, in turn, an `apple` provider with its `apple_p8` secret, Team ID and client id; the same without the secret file; and a `facebook` provider
When the server starts
Then the first starts and `doctor --pre-flight` prints `{public_url}/api/v1/auth/providers/apple/callback`; the second and third refuse to start with a message naming the provider key

**AC-7: Provider outage and outbound rules (NFT-RES-24, NFT-SEC-30)**
Given the Apple module stalling beyond `provider_timeout`, and a key-set URL that redirects to a canary
When `complete` runs for each
Then the first returns `ProviderUnavailable` within the timeout plus 1 s with no row created, and the second fails with 0 requests to the canary

## Non-Functional Requirements

**Compatibility**
- v1.1 only (RESTRICT-02); the v1.0 build keeps `github` and `oidc` (see Flagged concerns on how `apple` is kept out of v1.0).
- Pins of the workspace: `openidconnect 4.1.0`, `jsonwebtoken 11.1.0` with one crypto backend feature.

**Reliability**
- A failed exchange never creates a partial account or touches existing sessions (AC-21, NFT-RES-32).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Authorize URL builder for `apple` | No `scope`, no `code_challenge`; `response_mode=query`; redirect from `public_url` |
| AC-2 | Client-secret JWT builder and renewal | ES256; claims as stated; `exp` <= 15,777,000 s; renewed before expiry |
| AC-3 | Id-token validator with Apple claims | Each wrong claim rejected |
| AC-6 | Config validation | `apple` with secret accepted; missing secret and `facebook` refused |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | mock-idp Apple module | Web and mobile sign-in (FT-P-63 step 2) | Sessions; URL shape | AC-19 |
| AC-2 | Test team key | Exchange and renewal | Mock accepts JWT; new secret after renewal; 0 key bytes logged | NFT-SEC-38 |
| AC-3 | Apple negative scenarios | IT-01-5 cases with Apple | Documented errors; counts unchanged | AC-56, FT-N-23 |
| AC-4 | Relay email, allowlist | NFT-SEC-02 Apple-like case | No account by email; no link | AC-55 |
| AC-5 | `apple` + `gh` | `GET /api/v1/auth/providers` | Both listed; Apple recognisable | FT-P-63 step 1 |
| AC-6 | Three configs | Server start; `doctor --pre-flight` | Start / refuse; Apple redirect URI printed | FT-N-25, FT-N-28, AC-22 |
| AC-7 | Stalling module; redirecting key set | `complete` | `ProviderUnavailable`; 0 canary hits | NFT-RES-24, NFT-SEC-30 |

## Runtime Completeness

- **Named capability**: Sign in with Apple (ADR-008, AC-19 v1.1): ES256 client-secret JWT, code exchange, RS256 id-token validation against Apple's keys.
- **Production code that must exist**: the Apple `ProviderClient` making real HTTPS calls to the configured Apple endpoints, and the real JWT signing with the operator's `.p8` key.
- **Allowed external stubs**: mock-idp's Apple module in tests; live Apple only in the S8 smoke (`H-LIVE`, test-data row 9).
- **Unacceptable substitutes**: treating Apple as a plain `oidc` provider with a static secret pasted by the operator, skipping signature, nonce or audience checks, a test flag in production code, or a change to the `ProviderClient` trait.

## Constraints

- Identity is `(apple, sub)`; the `sub` is stable per developer team, so changing the Team ID changes every user's identity (fact card).
- No scopes, so no `form_post` route (fact card).

## Risks & Mitigation

**Risk 1: Claims of the client secret not confirmed**
- *Risk*: the fact card could not confirm `aud = https://appleid.apple.com` and the `kid` header from Apple's page; a wrong value fails every live exchange.
- *Mitigation*: the S8 live smoke with a real Apple account (`H-LIVE`) confirms them before release; see Flagged concerns.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `aud` claim (`https://appleid.apple.com`) and the `kid` header of the client secret are not in the extracted text of Apple's page (fact card); they must be confirmed by the S8 live smoke before the ACs are final | E-15 owner / S8 | open | Medium |
| The configuration keys for Apple are not listed: 01 §6 has `providers[]` (key, kind, issuer, client id, secret file) and `containerization.md` the `apple_p8` secret, but no Team ID or key id field; this task needs both | E-15 / E-01 owners | open | Medium |
| `ProviderInfo`'s field list is open (part 1 open point 1); AZ-3978 needs a field that says a provider is Sign in with Apple (kind, not key) | E-00 / E-15 owners | open | Medium |
| How `apple` stays out of v1.0 builds (refused kind, or a build flag) is not stated; AZ-3682 AC refuses kinds other than `github` and `oidc` | Owner (ADR-016) | open | Low |
| Guideline 5.1.1(v) asks for a way to revoke social credentials from the app; whether the server must call Apple's revoke endpoint at account deletion is not stated (fact card "Revoke tokens") | Owner | open | Medium |
| AZ-3978 (iOS sign-in) and AZ-3674 (iOS pipeline) should depend on this task once it has a key | Parent (Step 4) | open | Low |
