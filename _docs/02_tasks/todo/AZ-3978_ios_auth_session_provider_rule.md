# iOS sign-in through an authentication session and the provider-list rule

**Task**: AZ-3978_ios_auth_session_provider_rule
**Name**: iOS sign-in and provider rule
**Description**: Sign in on iOS through `ASWebAuthenticationSession` with a PKCE one-time code handed to `BackupCore::exchange_code`, show Google and GitHub only when the server reports Sign in with Apple enabled, and refuse plain HTTP
**Complexity**: 3 points
**Dependencies**: AZ-3975_ios_uniffi_static_library, AZ-3977_ios_keychain_credentials, AZ-3991_backup_core_surface, AZ-3992_android_auth_token_vault, AZ-3689_mobile_code_identity_janitor, AZ-3694_provider_linking_and_list, AZ-4024_sign_in_with_apple_provider
**Component**: 18 ios_app [v1.1] (Swift platform layer)
**Tracker**: AZ-3978
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — `GET /api/v1/auth/providers`, `POST /api/v1/auth/login-attempts`, `POST /api/v1/auth/mobile-code-exchanges`.
- `_docs/02_document/components/17_android_app/description.md` §2 (`exchange_code`, `set_server`, `AuthReturn`).
- `_docs/02_document/components/18_ios_app/description.md` §1, §3, §5.

## Problem

Apple has no device flow, so mobile sign-in uses the system browser and a one-time code (ADR-008). On iOS the system browser is `ASWebAuthenticationSession`, and the code returns through a universal link or custom scheme. The App Store requires Sign in with Apple whenever Google or GitHub sign-in is offered (guideline 4.8, RESTRICT-21, AC-19), so the iOS app must read the server's provider list and hide Google and GitHub when Apple is not enabled. Plain HTTP must be refused before any code or token is sent (AC-54).

## Outcome

- A user signs in through the authentication session; the one-time code and PKCE verifier reach `BackupCore::exchange_code` and are then discarded.
- The sign-in screen follows the provider list read from the API, never a hard-coded list.
- Without Sign in with Apple on the server, no Google or GitHub button is shown, and a message says sign-in is unavailable on iOS for this server.
- An `http://` server address is refused with a clear error and no cleartext request.

## Scope

### Included
- Reading providers from `GET /api/v1/auth/providers` and the button rule for cases (a) to (d) of IT-18-1.
- Starting the authentication session with PKCE, receiving the return (universal link or custom scheme), and handing code and verifier to `BackupCore::exchange_code`.
- Address check through the shared `set_server` (https only; the debug-only flag exists only in debug builds).
- Tests IT-18-1, IT-18-7 and ST-18-2.

### Excluded
- Token storage (E-28/c05), refresh and sign-out logic (E-27/c08, E-23/c13).
- Server-side providers and the one-time code (E-15/c09, E-15/c14).
- Flutter screens beyond passing the provider list and results over the channel (E-28/c08).

## Acceptance Criteria

**AC-1: Provider matrix (AC-19)**
Given the server providers list (a) Apple + GitHub + Google OIDC, (b) GitHub only, (c) Apple only, (d) Google only
When the iOS sign-in screen opens for each server
Then (a) shows all three buttons, (b) and (d) show no Google or GitHub button and the message that sign-in is unavailable on iOS for this server, and (c) shows Apple only

**AC-2: List read from the API**
Given a server whose provider list changes from Apple + GitHub to GitHub only
When the sign-in screen is reopened
Then the buttons follow the new list without an app update

**AC-3: Sign-in with a one-time code**
Given the real server with the Apple-shaped mock provider
When the user signs in through the authentication session
Then the return delivers a code that is exchanged once through `BackupCore::exchange_code` with its PKCE verifier, the app is signed in, and code and verifier are not kept after the exchange

**AC-4: Plain HTTP refused (AC-54)**
Given a release build and the address `http://sunnycloud.test`
When the user tries to sign in
Then a clear error is shown and 0 cleartext requests leave the device (no code or token sent)

**AC-5: No downgrade on redirect**
Given an https server that redirects to an http address
When sign-in and a backup run against it
Then 0 cleartext bytes carry a token or a code

## Non-Functional Requirements

**Compatibility**
- Simulator is enough for AC-1, AC-2 and AC-4 (IT-18-7); a test server uses a publicly trusted certificate because iOS ignores user-installed CAs.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Button rule for the four provider lists | Buttons and message as in AC-1 |
| AC-3 | Code and verifier lifetime after exchange | Both discarded |

## Blackbox Tests

Appium XCUITest on the simulator or DEV-IPHONE against the real server stack with mock-idp (harness E-28/c11a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Lists (a)-(d) | Sign-in screen | Matrix as specified | IT-18-1, FT-N-34, FT-P-63 |
| AC-2 | List changed on server | Reopen screen | Follows the new list | IT-18-1 |
| AC-3 | Apple-shaped mock provider | Sign in | Signed in; one exchange | FT-P-63 |
| AC-4 | `http://` address, release build | Sign in | Error; 0 cleartext requests | IT-18-7, FT-N-26, NFT-SEC-12 |
| AC-5 | https to http redirect | Sign-in and backup with packet capture | 0 cleartext token or code bytes | ST-18-2 |

## Constraints

- No tokens or codes in Dart, logs or URLs kept after the exchange (18 §9).
- Mobile sign-in yields a `sessions` row with a rotating refresh token (`canonical-names.md`).

## Risks & Mitigation

**Risk 1: Guideline 4.8 wording changes**
- *Risk*: Apple changes which providers require Sign in with Apple.
- *Mitigation*: the rule is one function covered by AC-1's matrix; a change touches one place.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No task adds server-side Sign in with Apple: E-15 covers GitHub and generic OIDC for v1.0, and the v1.1 follow-up named in E-28's assumptions is not planned | Owner / E-15 | open | Medium |
| How the app recognises "Google" among generic OIDC providers in the list, and whether other generic OIDC providers follow the same hide rule, is not stated | Owner | open | Low |
| Sign-out for a mobile session: 13 names `DELETE /api/v1/devices/me`, which the API surface lists as `DeviceOnly`; a session would get 403 (same concern as E-23/c13) | E-23 / E-15 owners | open | Low |
