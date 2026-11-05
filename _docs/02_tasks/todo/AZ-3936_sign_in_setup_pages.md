# Sign-in, invite and first-run setup pages

**Task**: AZ-3936_sign_in_setup_pages
**Name**: Sign-in and first-run pages
**Description**: The signed-out pages: sign-in with the providers the API lists, the invite and outcome states, the two-step first-run page with the setup token, and the open-redirect guard on return paths
**Complexity**: 3 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3933_setup_page_spec_spike, AZ-3950_playwright_web_harness, AZ-3684_provider_sign_in_callbacks, AZ-3685_invite_redemption_account_creation, AZ-3688_setup_token_invites_operator_ops, AZ-3828_identity_admin_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3936
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c07a

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.

## Problem

Users sign in only with the providers the operator enabled (AC-19). The first person needs the one-time setup token (AC-23); later people need an invite that works once and never by email match. Refusals (not invited, deactivated) must be clear and must create nothing. The pages handle a `return` parameter, which must never send the user to another host.

Split note: row 7 of the epic (`Auth pages: sign-in, setup first-run, account, device and session list`, 3 points) grew with the approved mockups (`ui_mockups/web-sign-in.html`, `web-account.html`): three sign-in outcome states, a two-step first run, and an account page with profile, storage, devices, sessions, provider linking, sign-out and deletion, about 6 points. This task keeps the signed-out pages; E-26/c07b keeps the account page.

## Outcome

- The sign-in page shows exactly the providers the API returns and completes a cookie-session sign-in.
- Invite and setup tokens reach the server only in the login-attempt body.
- Each refusal shows its mockup text and leaves no account behind.
- Return paths are relative only.

## Scope

### Included
- Sign-in page (mockup state "Sign in"): provider buttons from the providers list in API order, the line "Your administrator chose these sign-in options", start of a login attempt with provider, relative return path, client kind web and optional invite or setup token, then navigation to the returned authorization URL.
- Invite state ("Invite link"), "Not invited" and "Deactivated" states from the SPA sign-in error route.
- First-run page per the E-26/c04 paragraph (states "First run", "Token error", "First run: sign in"), shown while `setup_required` is true.
- Open-redirect guard on `return` (relative single-slash paths only); a signed-in user opening sign-in goes to the files page.

### Excluded
- The account page (E-26/c07b); provider and invite logic (E-15); Facebook sign-in (out of scope); Apple (v1.1, shown only if the API lists it).

## Acceptance Criteria

**AC-1: Providers come from the API (AC-19, FT-P-62, FT-P-73 step 1)**
Given the operator enabled GitHub and generic OIDC (Google) only
When the sign-in page loads
Then it shows exactly those two buttons in the API's order and no Apple or Facebook option; with one provider enabled it shows one

**AC-2: Sign-in round trip**
Given mock-idp and an existing account
When the user picks a provider
Then the page starts a login attempt with a relative return path, navigates to the returned authorization URL, and after the callback lands on the return path signed in with a cookie session that JavaScript cannot read

**AC-3: Invite works once (AC-23, FT-N-21)**
Given an invite link
When it is opened and the user signs in, and the same link is opened again by another identity
Then the first visit shows the "Invite link" state, the invite token appears in no request URL (network log) and only in the login-attempt body, the account is created; the second ends in the "Not invited" state with no account created

**AC-4: Not invited and deactivated (FT-N-20)**
Given an unknown identity without invite, and an identity whose account is deactivated
When each completes the provider step
Then the page shows the "Not invited" or the "Deactivated" text and no session exists

**AC-5: First run (AC-23, FT-P-64, IT-16-12)**
Given an empty database (`setup_required` true)
When any signed-out route is opened, a wrong token is entered, then the right token followed by sign-in
Then the first-run page appears; the wrong token shows the "Token error" text and creates no account; the right token creates the first admin; afterwards the first-run page cannot be reached

**AC-6: Open-redirect guard (ST-16-4)**
Given `return` values `https://evil.test`, `//evil.test`, `/\evil.test`, `javascript:alert(1)` and `/files/x`
When the user signs in with each
Then the first four land on `/` and the last on `/files/x`; no navigation leaves the origin except to the authorization URL returned by the API

**AC-7: Tokens are not kept (ST-16-2 page half)**
Given a setup token and an invite token used on these pages
When browser storage and the address bar are inspected after the attempt starts
Then neither token is in `localStorage`, `sessionStorage`, IndexedDB or Cache Storage, and the setup token was never in a URL

## Non-Functional Requirements

**Compatibility**
- Works with storage blocked (no preference needed to sign in).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Return-path guard over the hostile list and valid relative paths | Only relative single-slash paths pass |
| AC-1 | Provider list rendering from an API answer of 1, 2 and 3 providers | Exactly the returned providers, in order |

## Blackbox Tests

Playwright against the real stack with mock-idp (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Provider subsets configured | FT-P-62; FT-P-73 step 1 | Exactly the configured providers | AC-19 |
| AC-2 | Existing account | Sign-in round trip | Signed in at the return path | AC-58 |
| AC-3, AC-4 | Invite; unknown identity; deactivated account | FT-N-20, FT-N-21 | States shown; account counts as expected | AC-23 |
| AC-5 | Empty database | IT-16-12; FT-P-64 | First admin created once | AC-23 |
| AC-6 | Hostile `return` values | ST-16-4 (redirect half) | Lands on `/` | AC-12 |
| AC-7 | Tokens entered | Storage and URL inspection | 0 tokens found | ST-16-2 |

## Constraints

- Tokens travel only in request bodies (01 §3); the cookie is set by the server and never read.

## Risks & Mitigation

**Risk 1: Error route codes drift**
- *Risk*: the server's error route codes change and the page shows a generic text.
- *Mitigation*: unknown codes fall back to a neutral "sign-in did not complete" text with the request id; the codes are fixed by the flagged owner item.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The invite link format is not specified (`ui_mockups/DESIGN.md` §9 #6; mockup placeholder `/invite#sc_inv_…`); AC-3 needs its route and where the token sits | E-15 and E-17 owners | open | Medium |
| The SPA sign-in error route and its code values for "not invited" and "deactivated" are not named in 01 or 11 (E-21/c12 redirects "as 01 decides") | E-15 and E-21 owners | open | Medium |
| When the setup token is checked is open; E-26/c04 records it | E-26/c04, E-15 | open | Low |
| Provider buttons must use each provider's official mark per its brand guidelines (DESIGN §5); the self-hosted assets need a licence check | E-26 owner | open | Low |
