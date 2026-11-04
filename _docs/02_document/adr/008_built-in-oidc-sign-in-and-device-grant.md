# ADR-008: Built-in OAuth/OIDC sign-in, server-owned sessions and device grant

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Users sign in with social or enterprise identity, never with a local password, and the operator of a self-hosted install must reach a working server in 30 minutes (AC-22) with one provider ready. The desktop daemon has no browser of its own and must obtain a limited credential; Apple has no device flow, so a provider-neutral way to approve a headless client is needed. Registration must not become an account-takeover path: email claims from providers are mutable or unverified (the nOAuth class, `b_security.md` F4 and S7 to S9). The first admin has no one to invite them.

- Acceptance criteria addressed: AC-19 (provider subset, at least one; iOS rule), AC-20 (abuse limits), AC-21 (revocation within 5 s; existing sessions work without internet), AC-22 (30-minute install), AC-23 (setup token, invites, no email match), AC-54, AC-55 (unverified email never creates or links an account), AC-56 (12 negative OAuth callbacks), AC-57 (device approval page and scope limits), AC-58 (CSRF, refresh rotation, replay revokes the family), AC-75 (two concurrent redemptions create one account), AC-76 (device code exchanged exactly once)
- Restrictions addressed: RESTRICT-01 and RESTRICT-02 (GitHub and generic OIDC in v1.0, Apple in v1.1), RESTRICT-04 (Facebook out of scope), RESTRICT-17 (identity is (provider, provider user id), never email; no local passwords), RESTRICT-18 (setup token, `sunnycloud admin`, invites or allowlist), RESTRICT-19 (own opaque sessions, own RFC 8628 grant, device tokens sync-only), RESTRICT-20 (no internet means no new sign-ins), RESTRICT-21 (iOS rule from App Store guideline 4.8), RESTRICT-33 (Android device token excluded from backups)
- Risks addressed: R05 (mobile token lifecycle and refresh rotation), R25 (Linux fallback token file), R18 (store policy, Apple sign-in)
- Research source: `_docs/01_solution/solution.md` § Component: Identity and sessions

## Decision

We will make the sunnycloud API itself the OAuth/OIDC relying party (`openidconnect 4.1.0`, `oauth2 5.0.0`, `jsonwebtoken`), key accounts by `(provider, provider_user_id)`, issue its own opaque hashed sessions, and run its own RFC 8628 device grant for desktop daemons.

**Providers.** v1.0 ships one generic OIDC client (Google, Authelia, authentik, Keycloak, Pocket ID and others are configured through it) and GitHub (OAuth2 plus `/user` and `/user/emails`). v1.1 adds Sign in with Apple (hand-written ES256 client secret, no scopes, no PKCE). Facebook is cut. The operator registers their own provider apps and the API refuses to start without at least one provider. Provider calls time out after 10 s, follow no redirects and block link-local and metadata ranges; the `redirect_uri` is built from `public_url`, never from the Host header. The iOS app shows other providers only when the server also has Sign in with Apple enabled.

**Accounts and registration.** `identities` has `UNIQUE (provider, provider_user_id)`; an account is never found by email. An unknown identity can create an account only with an invite or the setup token in its login state, redeemed with a single atomic `UPDATE ... WHERE redeemed_at IS NULL`. An operator email allowlist is matched only at invite redemption and only if the provider reports the email as verified. The first admin uses the setup token (an `invites` row of kind `setup`, valid only while `accounts` is empty, printed once at start) and `sunnycloud admin setup-token|create|reset|promote|demote`; `reset` re-binds an existing admin through an operator invite with `target_account_id`.

**Sessions and devices.** Web and mobile sign-ins create a `sessions` row: 256-bit random tokens stored as SHA-256 only, a 15-minute access token and 30-day refresh token, rotation under a row lock, and a 30 s grace in which the previous refresh hash is accepted without a new revocation; a replay after the grace revokes the family. Mobile uses the system browser with PKCE and a one-time hashed code (`login_codes`) returned through the custom scheme. Desktop daemons use the device grant: hashed device code, user code valid at most 10 min, an approval page showing requester and approver IP and location, wrong attempts limited per session, account and IP, and a sync-only device token stored hashed. Device tokens cannot approve devices, create shares, delete accounts or link providers.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| External IdP or broker (Keycloak, Authentik, Zitadel, Dex) as the required login service | Dex has no Facebook or Apple connector and Google is alpha; Keycloak needs about 1 to 1.5 GB RAM per small production pod. Any broker adds a second product, database and admin console to the 30-minute install (AC-22). Self-hosters who already run one can use it through the generic OIDC client, so nothing is lost. An embedded broker stays the documented fallback if provider quirks dominate maintenance. |
| Local passwords | Excluded by RESTRICT-17 and the out-of-scope list: it would add password storage, reset flow, breach handling and credential-stuffing defence to a service that otherwise stores no user secrets; the AC-20 abuse limits cover only OAuth, device-approval and share-password failures. |
| Link or create accounts by email | Email claims are mutable or unverified at some providers (Apple relay, GitHub per-address flag, Facebook); 2026 auto-link-by-email takeovers are documented. AC-55 requires 0 accounts created or linked from an unverified email in the mock test, and AC-23 forbids email match for registration. |
| Per-provider device flows for desktop, or loopback redirect | Apple has no device flow, Google and Facebook device flows need special client types, so four code paths with one missing; loopback needs a browser on the same machine and fails on headless or SSH hosts. A server-owned RFC 8628 grant is the only uniform option. |
| Generic OIDC only (Immich model) | Smallest code, but GitHub is OAuth2 not OIDC and RESTRICT-01 names GitHub for v1.0. |

## Consequences

### Positive

- One component handles sign-in, sessions and devices, with no extra service for the operator; Google works through the same generic client as self-hosted IdPs.
- Revocation is under our control: a revoked session or device stops syncing within 5 s, including live long-polls (AC-21), and existing sessions keep working with the provider offline.
- Identity keyed by `(provider, sub)` removes the email-takeover class; AC-55 and AC-23 are enforced structurally, not by convention.
- Device tokens carry sync scope only (they get 403 on approve-device, create-share, delete-account and link-provider), limiting damage from a stolen laptop token.
- Refresh rotation with a 30 s grace lets four concurrent refreshes yield one successor (AC-58), which protects mobile background uploads (R05).

### Negative

- We own security-sensitive code: the RFC 8628 grant, Apple client-secret JWT, state and nonce handling and the 12 negative-callback tests have no mature crate to lean on. Live provider flows are unproven until spike S8.
- `oauth2` is nearly two years old and `jsonwebtoken` 11.x is a fresh breaking release; dependency floors must be watched.
- Operators must register apps with each provider; an air-gapped install cannot sign anyone new in (RESTRICT-20).
- Device tokens do not rotate in v1.0 (revocable only), and the Linux fallback is a 0600 plaintext file when no Secret Service exists (R25, accepted).
- The iOS rule (Apple required before other providers show) makes the "any subset" operator choice platform-dependent; Apple costs US$99/year and extra setup, and arrives only in v1.1.

### Neutral / Open

- Authorization stays per account in the API; this ADR changes no tree permission. Worker authentication is separate (`WorkerAuthenticator`, internal listener) and uses no user session.
- Setup-token printing to the log is a documented exception to the no-secrets-in-logs rule.
- An enterprise-SSO demand beyond generic OIDC (LDAP) would be met by documenting a broker upstream, not by changing this design.

## Evidence

- `_docs/02_document/components/01_identity/description.md` § 1. High-Level Overview; § 5. Implementation Details; § 6. Configuration keys; § 7. Security notes
- `_docs/02_document/system-flows.md` § Flow F1: First-run bootstrap; § Flow F2: Sign-in through OIDC or GitHub; § Flow F3: Desktop device-code login and approval
- `_docs/02_document/data_model.md` § 4.2 `identities`; § 4.3 `sessions`; § 4.5 `device_grants`; § 4.6 `invites`; § 4.8 `login_codes`
- `_docs/02_document/architecture.md` § 7. Security Architecture (Authentication); § 9. Confirmed Design Answers (2026-11-04), answers 6 and 7 (mobile sign-in; per-worker token, superseded in part by `canonical-names.md` § Owner decisions at Step 4)
- `_docs/02_document/canonical-names.md` § Technical defaults applied at Step 4 (`reset_admin`, mobile sign-in session, device grant yields a sync-only token)
- `_docs/00_research/04_reasoning_chain.md` § Dimension 7: Auth
- `_docs/00_research/raw/security_clients.md` § 2. Desktop login mechanism (options table); `_docs/00_research/raw/b_ops_licence_scope.md` F9; `_docs/00_research/raw/b_security.md` F4
- `_docs/00_research/02_fact_cards/mve_crypto_auth_mobile.md` § MVE — openidconnect 4.1.0 + oauth2 5.0.0; § MVE — RFC 8628 device authorization grant; § Findings that change the plan (items 6, 7, 10)
- `_docs/00_research/06_component_fit_matrix.md` § Decisions (D2); § Round 2 changes (Auth providers row)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R05, R25)
- `_docs/00_problem/restrictions.md` § Authentication; `_docs/02_document/tests/restriction-ids.md` (RESTRICT-17 to RESTRICT-21)

## Notes

`architecture.md` § 8 lists this decision as "ADR-003" in its short index; the file number here follows the Step 4.5 plan. `restrictions.md` is the source for the requirement set; the Android device token backup exclusion (RESTRICT-33) is an Android client detail not repeated here. Re-evaluate the embedded-broker fallback if provider-specific bugs dominate maintenance after v1.0, and re-check the iOS provider rule when App Store guidelines change.
