# Provider clients for GitHub and generic OIDC, sign-in begin and complete, negative callbacks

**Task**: AZ-3684_provider_sign_in_callbacks
**Name**: Provider sign-in and callback validation
**Description**: Sign a known identity in through GitHub or any operator-configured OIDC provider (Google included), with hashed one-time state bound to the browser, and reject the negative callback cases of AC-56 without creating anything
**Complexity**: 5 points
**Dependencies**: AZ-3682_identity_crate_migration_config, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base; notes: E-31/c13 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 01 identity
**Tracker**: AZ-3684
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c04

## Problem

There are no local passwords: identity is `(provider, provider user id)` (RESTRICT-17, ADR-008). Users sign in through operator-configured providers. The callback is the main attack surface (AC-56): replayed or foreign state, wrong tokens, hostile return paths, forged hosts, and a provider that points the server at internal addresses (NFT-SEC-30).

## Outcome

- `SignInService::begin` and `complete` work for `github` and `oidc` provider kinds; Google signs in as a generic OIDC provider (AC-19).
- A known identity gets a new session (access token for `access_token_ttl`, refresh token for `refresh_token_ttl`).
- Negative callback cases 1 to 11 of AC-56 end in their documented errors with no row created; case 12 (wrong code verifier) belongs to E-15/c09.
- Provider calls respect the 10 s timeout, follow no redirects and never reach link-local or metadata addresses.

## Scope

### Included
- `ProviderClient` for `github` (OAuth 2: code exchange, user profile, email list with the per-address `verified` flag) and `oidc` (discovery from the configured issuer, id-token signature, issuer, audience and nonce checks).
- `begin(provider, return_to, client_kind, invite/setup token?, code_challenge, host)`: unknown provider, non-relative `return_to` and a host other than the `public_url` host are refused; `code_challenge` is required for mobile client kinds; `redirect_uri` is built from `public_url`, never from the request host; a state row is stored with `state_hash = SHA-256(state ‖ browser_binding)`, the nonce, the PKCE verifier, the relative return path, the client kind, the hash of any invite or setup token and any code challenge; result `AuthorizationRedirect{url, browser_binding}`.
- `complete(provider, params, browser_binding, host)`: consume the state row by delete-returning (a replay finds nothing); exchange and validate with the provider; look up the identity by `(provider, provider_user_id)`, never by email; for a known identity issue a session and update `last_login_at`; for an unknown identity without an invite or setup token answer `InvitationRequired` and create nothing.
- Outbound rules: only configured issuers and the endpoints they advertise; HTTPS only; redirects never followed; link-local and cloud-metadata destinations refused, checked on the resolved address at connect time; `provider_timeout` (10 s).

### Excluded
- Invite and setup-token redemption and account creation (E-15/c05, E-15/c08).
- The mobile one-time code outcome and its exchange (E-15/c09); provider linking and `list_providers` (E-15/c14).
- Abuse counting, `RateLimited` and audit events (E-15/c11).
- Cookies, CSRF, the 302 to the SPA error route (E-21); Sign in with Apple (v1.1; the `ProviderClient` trait must not need to change for it).

## Acceptance Criteria

**AC-1: Sign in with each configured provider (AC-19)**
Given mock-idp configured as a `github` provider and as an `oidc` provider, and an existing identity for each
When `begin` then `complete` run with the `valid` scenario, once more with the same provider user id but a changed email
Then each returns a session whose access token authenticates as the identity's account, and the changed email still reaches the same account with 0 new accounts

**AC-2: Begin refuses bad input and builds the redirect from the public URL (AC-54, AC-56)**
Given `public_url = https://sunnycloud.test`
When `begin` is called with provider `nosuch`; with `return_to` set to each of `https://evil.test/`, `//evil.test`, `/\evil.test`, `/x\r\nSet-Cookie: a=b` and the empty string; with host `evil.test`; with a mobile client kind and no `code_challenge`; and with valid input
Then the results are `UnknownProvider`, `InvalidReturnPath` for each hostile path, `HostMismatch`, an invalid-request error, and an `AuthorizationRedirect` whose URL starts with the provider's authorize endpoint and carries `redirect_uri=https://sunnycloud.test/api/v1/auth/providers/{key}/callback`; the stored row holds only `SHA-256(state ‖ browser_binding)`, never the plaintext state

**AC-3: Negative callbacks 1 to 11 (AC-56)**
Given one `begin` per case and mock-idp scenarios from DS-20
When `complete` runs with: missing state; replayed state; foreign state (other browser binding); expired state (clock past `oauth_state_ttl`); wrong provider key; wrong nonce; wrong audience; wrong issuer; absolute `return_to` at begin; foreign Host at complete; reused one-time provider code
Then cases 1 to 5 return `UntrustedState`, 6 to 8 and 11 return `TrustedFailure{code}`, case 9 returns `InvalidReturnPath`, case 10 returns `HostMismatch`; row counts of `sessions`, `accounts`, `identities` and `login_codes` are unchanged; cases 1 to 5 consume no state row but their own, and the second call of case 2 finds no row

**AC-4: Provider timeout leaves nothing behind (NFT-RES-24)**
Given a provider that stalls longer than `provider_timeout` (a short configured value in the test) and an existing session of another user
When `complete` runs
Then it returns `ProviderUnavailable` within the timeout plus 1 s, no row is created, and the existing session still authenticates

**AC-5: Outbound requests stay on configured issuers (NFT-SEC-30, component part)**
Given a discovery document that advertises endpoints at `169.254.169.254`, `[fe80::1]` and the cloud-metadata hostname, an issuer endpoint that answers 302 to a canary, a hostname that resolves to a metadata address at connect time, and a `file://` endpoint
When `complete` runs for each
Then the sign-in fails without a session, the canary and metadata addresses receive 0 requests, and no redirect is followed; a configured issuer on a private LAN address (10.x) still signs in

**AC-6: No secret at rest**
Given a completed sign-in
When every column of `oauth_states` and `sessions` is read with the migrator role
Then no plaintext state, provider code, invite token, access token or refresh token appears; tokens have a type prefix and only their SHA-256 is stored

## Non-Functional Requirements

**Reliability**
- A provider outage never creates a partial account or touches existing sessions (AC-21, NFT-RES-32).

**Compatibility**
- v1.0 provider kinds are `github` and `oidc`; Apple (v1.1) is added without changing the `ProviderClient` trait.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | `return_to` validator over the hostile list | Only relative single-slash paths without control characters pass |
| AC-2 | `redirect_uri` builder | Built from `public_url` only |
| AC-2 | State hash | Equals SHA-256(state ‖ binding); differs for another binding |
| AC-5 | Destination filter | Link-local, metadata and non-HTTPS refused; private LAN allowed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | mock-idp as github and oidc, known identities | Sign-in, then changed email | Session per provider; same account | FT-P-62 |
| AC-2 | `public_url` https | IT-01-4 | Errors per input; redirect from `public_url`; only hash stored | FT-N-26, NFT-SEC-12 |
| AC-3 | DS-20 scenarios | IT-01-5 cases 1-11 | Documented errors; row counts unchanged | FT-N-23, NFT-SEC-03 |
| AC-4 | Stalling provider | `complete` | `ProviderUnavailable` within timeout + 1 s; no rows; old session works | NFT-RES-24, NFT-RES-32, FT-N-30 |
| AC-5 | Hostile discovery, redirects, rebinding | `complete` | 0 canary hits; no redirect; LAN issuer works | NFT-SEC-30 |
| AC-6 | Completed sign-in | Column dump | No plaintext secret | ST-01-5 |

## Runtime Completeness

- **Named capability**: OIDC client (`openidconnect` 4.1.0: discovery, JWKS, id-token signature, issuer, audience, nonce), GitHub OAuth 2 (`oauth2` 5.0.0), `jsonwebtoken`, and the workspace-pinned `reqwest` with redirects off.
- **Production code that must exist**: real HTTPS exchanges with the configured issuers and full id-token validation for `oidc`; the GitHub profile and verified-email calls for `github`.
- **Allowed external stubs**: mock-idp (E-31/c13) as the provider in tests; the provider is an external service.
- **Unacceptable substitutes**: a built-in fake `ProviderClient` in production code, a test flag that skips signature, nonce or audience checks, or canned identities inside the crate.

## Constraints

- Identity is `(provider, provider_user_id)`; email is never a lookup key (RESTRICT-17).
- The invite or setup token travels in the request body and in the state row as a hash, never in a URL and never to the provider.

## Risks & Mitigation

**Risk 1: Provider quirks per issuer**
- *Risk*: real OIDC issuers differ from mock-idp in claims or discovery.
- *Mitigation*: mock-idp scenarios cover the claims the checks read; the system scenario FT-P-62 runs both kinds.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The error variant for a sign-in blocked by the outbound rules (metadata address, redirect, non-HTTPS endpoint) is not named in 01; NFT-SEC-30 only asks for a generic error | E-15 owner | open | Low |
| The connect-time address check (DNS rebinding, NFT-SEC-30 step 3) is required by the system scenario but not stated in 01's description | E-15 owner | open | Low |
| IT-01-4 lists `/maps` among the hostile `return_to` values, but it is a relative path and no rule in 01 makes it invalid; it is left out of AC-2 until the rule (for example "no SPA-reserved prefix") is stated | E-15 owner | open | Low |
