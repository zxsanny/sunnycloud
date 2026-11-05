# Keychain token store with persist-before-use

**Task**: AZ-3977_ios_keychain_credentials
**Name**: Keychain credential store
**Description**: The Swift Keychain store behind the shared core's token-store callback, seen by Rust as `CredentialProvider`: this-device-only, never synced, and written before a rotated access token is used
**Complexity**: 2 points
**Dependencies**: AZ-3975_ios_uniffi_static_library, AZ-3992_android_auth_token_vault, AZ-3966_token_lifecycle
**Component**: 18 ios_app [v1.1] (Swift platform layer)
**Tracker**: AZ-3977
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CredentialProvider` (`bearer`, `on_rotated`), produced by E-23/c14.
- `_docs/02_document/components/17_android_app/description.md` §2 (`TokenVault`), §5 (token lifecycle).
- `_docs/02_document/components/18_ios_app/description.md` §2 (token store row).

## Problem

Mobile sessions use a 15-minute access token and a rotating refresh token (ADR-008). The shared core refreshes single-flight and hands the new pair to `CredentialProvider::on_rotated`; it uses the new access token only after that call returns. On iOS the pair must live in the Keychain, readable after the first unlock, only on this device, and never synced to iCloud or restored onto another phone (RESTRICT-33). If `on_rotated` returns before the Keychain write, a kill loses the new refresh token and the account is signed out after the 30 s grace.

## Outcome

- The access and refresh tokens are stored in one Keychain item with "after first unlock, this device only" accessibility and synchronization off.
- `on_rotated` returns only after the Keychain write has completed.
- A restored encrypted backup on another device has no token and shows sign-in.
- No token or file name appears in logs.

## Scope

### Included
- The Swift token store implementing the shared core's token-store callback (save, load, clear), which Rust sees as `CredentialProvider` (`bearer()` returns the stored pair, `on_rotated(access, refresh)` returns after the write).
- Keychain attributes: `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, `kSecAttrSynchronizable` false.
- Clearing on sign-out and on a rejected refresh.
- Tests IT-18-5 and ST-18-1.

### Excluded
- The refresh logic, single-flight and retry rules (E-23/c13) and the shared `exchange_code`, `refresh`, `sign_out` calls (E-27/c08).
- The sign-in screen and authentication session (E-28/c06).

## Acceptance Criteria

**AC-1: Stored this-device-only, not synced (RESTRICT-33)**
Given a signed-in app
When the Keychain item is inspected with a debug probe
Then its accessibility is `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` and `kSecAttrSynchronizable` is false

**AC-2: Persist before use**
Given the 15-minute token fixture and a rotation in progress
When the shared core calls `on_rotated(access, refresh)`
Then the call returns only after the new pair is in the Keychain, and no request carries the new access token before it returns

**AC-3: Kill between rotation and write**
Given the app killed after the server rotated the token and before the Keychain write
When the app restarts within the 30 s grace, and in a second run after it
Then within the grace the old refresh token still works and one refresh per expiry follows; after the grace the app shows sign-in

**AC-4: Restored backup is signed out**
Given an encrypted device backup restored to a second device
When the app starts there
Then no token is found and the sign-in screen is shown

**AC-5: No tokens in logs**
Given sentinel tokens in the fixture
When all logs of the runs above are searched
Then the sentinels occur 0 times

## Non-Functional Requirements

**Reliability**
- A failed Keychain write makes `on_rotated` report failure; the core then retries once and otherwise treats the refresh as rejected and the app signs out (Step 5 gap default, 13 §12).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | `on_rotated` with a Keychain write that is slowed | Returns only after the write completes |
| AC-5 | Debug text of the stored credential | Placeholder only, no token value |

## Blackbox Tests

Appium XCUITest on DEV-IPHONE (H-IOS) against the real server stack with the token fixture (harness E-28/c11a); no device means `NOT-RUN(H-IOS)`.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Signed in | Keychain attributes | ThisDeviceOnly; not synchronizable | ST-18-1, NFT-SEC-44 |
| AC-2 | 15-minute fixture | Rotation | Write before first use; one refresh per expiry | IT-18-5, NFT-RES-33 |
| AC-3 | Kill before write | Restart in and after grace | Old token works in grace; sign-in after | IT-18-5, NFT-RES-33 |
| AC-4 | Encrypted backup restored elsewhere | App start | Signed out | ST-18-1, NFT-SEC-44 |
| AC-5 | Sentinel tokens | Log search | 0 occurrences | ST-18-1 |

## Constraints

- Keychain only; Flutter never sees a token (18 §9, same as 17).
- Same `CredentialProvider` semantics as 17's `TokenVault` bridge; no iOS-only token rule.

## Risks & Mitigation

**Risk 1: Keychain unavailable before first unlock**
- *Risk*: a background task right after reboot cannot read the item.
- *Mitigation*: the behaviour is not specified; it is a flagged concern to settle with the owner before implementation, and the token is never copied to a weaker store as a workaround.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| RESTRICT-33's text names the Android device token; the iOS Keychain check is its v1.1 counterpart in NFT-SEC-44 and has no separate restriction id | Owner | open | Low |
| What a run does when the Keychain item cannot be read before the first unlock is not stated in 18 | E-28 | open | Low |
