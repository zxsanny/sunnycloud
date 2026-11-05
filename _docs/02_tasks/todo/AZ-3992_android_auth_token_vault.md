# Android auth: `exchange_code`, `refresh`, `sign_out`, `TokenVault` and `AuthReturn`

**Task**: AZ-3992_android_auth_token_vault
**Name**: Android auth and token vault
**Description**: Sign the phone in with a one-time code and PKCE through the system browser, keep the 15-minute access token and the rotating refresh token only in a Keystore-wrapped store excluded from backups, persist a rotated pair before use, and sign out even when the server is unreachable
**Complexity**: 5 points
**Dependencies**: AZ-3991_backup_core_surface, AZ-4000_flutter_channel_bridge, AZ-4002_native_library_ci, AZ-4003_android_emulator_harness, AZ-3966_token_lifecycle, AZ-3689_mobile_code_identity_janitor
**Component**: 17 android_app (Kotlin `TokenVault`, `AuthReturn`; `mobile_backup` auth methods)
**Tracker**: AZ-3992
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `exchange_mobile_code`, `refresh`, `sign_out` (E-23/c13); open point 4 (sign-out route).
- `_docs/02_document/contracts/13_sync_core/host_traits.md` — `CredentialProvider`, `on_rotated` (E-23/c14).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `exchange_code`, `refresh`, `sign_out` (this task contributes them).
- ADR-008 (sign-in design); `components/17_android_app/description.md` §2 (`TokenVault`, `AuthReturn`), §5 (Token lifecycle), §11.

## Problem

A mobile session has a 15-minute access token and a rotating refresh token (ADR-008). If the new refresh token is used before it is stored and the process dies, the user is signed out (R05). Tokens must never reach Dart, logs, intents or platform backups (RESTRICT-33, FT-N-36). The one-time code must be used once and its PKCE verifier discarded (AC-76, client side); a hostile deep link must not start a sign-in. The row was planned at 3 points; it also has to carry the backup-exclusion tests IT-17-5 and ST-17-1, which no other row owns, so it is re-estimated at 5.

## Outcome

- `BackupCore::{exchange_code, refresh, sign_out}` over sync-core's token calls; `TokenRejected` clears the store and shows sign-in.
- A Kotlin `TokenVault` (`save`, `load`, `clear`) with a Keystore-wrapped value in app-private no-backup storage; manifest backup and data-extraction rules exclude it and the checkpoint's auth fields.
- The `CredentialProvider` over `TokenVault`: `bearer()` returns the stored pair; `on_rotated` returns only after `save` completed.
- `AuthReturn`: opens the system browser with PKCE and `state`, receives `sunnycloud://auth?code&state`, validates before any network call, passes code and verifier once, then clears them.

## Scope

### Included
- Code verifier and `state` generation, system-browser launch, the custom-scheme (or App Link) return.
- Input validation of the return: `state` match, code size limit, extra parameters ignored; refusal of an `http://` base address outside the debug build.
- `exchange_code(code, code_verifier, device label)`; tokens stored through `TokenVault`.
- `refresh()` exposed for a refresh before a run after a long idle; normal refresh on 401 stays in sync-core.
- `sign_out()`: server revoke attempted, `TokenVault` cleared regardless.
- `allowBackup="false"`, backup and data-extraction rules; the restored app starts signed out.
- Tests IT-17-4, IT-17-5, IT-17-7, ST-17-1, ST-17-2 and the sign-out leg of IT-17-17.

### Excluded
- Single-flight refresh, retry and persist ordering inside sync-core (E-23/c13).
- The server's mobile-code exchange and rotation rules (E-15/c06, E-15/c09).
- Sign-in and status screens (E-27/c15); TLS trust (E-27/c12).

## Acceptance Criteria

**AC-1: Return validated before any network call (IT-17-7, ST-17-2)**
Given a pending sign-in with state `S1`
When `sunnycloud://auth` arrives with code `C1` and state `S1`, with a foreign state, with no code, with an oversized code, with extra parameters, and a second time with `C1`
Then only the first leads to exactly one `exchange_code(C1, verifier, label)`; the others are rejected before any request; the second delivery is ignored because the verifier was cleared; a manually replayed code is shown as `InvalidCode`; the code appears in no log and no process argument

**AC-2: Persist before use (IT-17-4, AC-58 client side)**
Given the 15-minute token fixture with a rotating refresh token and a 40-minute run with 4 concurrent block uploads
When the access token expires during the run
Then exactly one refresh happens per expiry, no block or batch is lost, `on_rotated` returns only after `TokenVault.save` completed (timestamp spy), and no request carries the new access token earlier

**AC-3: Kill during rotation (IT-17-4, R05)**
Given the fixture and a kill between the server's rotation and the vault write
When the app restarts within 10 s, and in a second round after 40 s
Then the first restart continues with the stored refresh token, and the second shows sign-in with the vault cleared (accepted risk 4)

**AC-4: Rejected refresh signs out (AC-21)**
Given a session whose family was revoked on the server
When the next refresh is refused
Then `TokenRejected` is returned, the vault is cleared and the app reports signed out

**AC-5: Tokens excluded from backups (IT-17-5, RESTRICT-33)**
Given a signed-in app
When a full backup and a device-transfer extraction are taken and restored onto a clean emulator
Then the manifest has `allowBackup="false"` and rules excluding the token store and the checkpoint auth fields, the archives contain 0 occurrences of the access and refresh sentinels, and the restored app starts signed out

**AC-6: Tokens exist only in the vault (ST-17-1)**
Given sign-in with sentinel tokens from the mock server
When logcat, process listings, intent extras, app-private files and Flutter channel traffic are captured
Then the sentinels occur 0 times outside the encrypted `TokenVault` file, and that file holds no plaintext sentinel

**AC-7: Sign-out offline (IT-17-17 sign-out leg)**
Given a signed-in app and an unreachable server
When the user signs out
Then the revoke is attempted, the vault is cleared regardless, and the sign-in screen is shown

## Non-Functional Requirements

**Reliability**
- A kill between the refresh response and the vault write never loses the family if the restart is within the server's 30 s reuse grace.

**Compatibility**
- minSdk 30; Android Keystore available on every supported device.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Return validation cases | One exchange; others rejected before any request |
| AC-2 | `on_rotated` over a vault with a slow `save` | Returns only after `save`; no early use |
| AC-4 | `TokenRejected` path | Vault cleared; signed out |

## Blackbox Tests

Appium and `adb` on the API 34+ emulator against the real server with mock-idp and the test clock (harness E-27/c19).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Pending sign-in | Six deep-link variants | One exchange; rest rejected | IT-17-7, ST-17-2, AC-76 |
| AC-2 | Token fixture, 40-minute run | Expiries during uploads | One refresh per expiry; no lost block | IT-17-4, NFT-RES-33 |
| AC-3 | Kill at rotation | Restart at 10 s and 40 s | Continues; then sign-in | IT-17-4 |
| AC-5 | Signed in | Backup, extraction, restore | 0 sentinels; signed out | IT-17-5, FT-N-36, NFT-SEC-44 |
| AC-6 | Sentinel tokens | Capture all channels | 0 occurrences outside the vault | ST-17-1 |
| AC-7 | Server unreachable | Sign out | Vault cleared | IT-17-17 |

## Constraints

- Flutter never receives a token; the channel schema has no token field (`status_channel.md`).
- Single-flight refresh is sync-core's; this task never refreshes in parallel to it.

## Runtime Completeness

- **Capability**: Android Keystore-wrapped token storage in no-backup app storage.
- **Production code that must exist**: `TokenVault` over the Keystore and the `CredentialProvider` bridge used by every run.
- **Allowed stubs**: the mock identity provider and the test clock on the server side.
- **Unacceptable substitutes**: plain shared preferences, an unencrypted file, an in-memory store in a release build.

## Risks & Mitigation

**Risk 1: Mobile sign-out route refused**
- *Risk*: `DELETE /api/v1/devices/me` is `DeviceOnly`, so a mobile session gets 403 and the server-side session stays alive.
- *Mitigation*: AC-7 clears the vault regardless; the route question is flagged.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `sign_out` uses `DELETE /api/v1/devices/me`, which is `DeviceOnly`; a mobile session would get 403 (`sync_api.md` open point 4); the mobile revoke route needs an owner answer | Owner / E-15, E-21, E-27 | open | Medium |
| The code size limit for the deep-link check is not stated | E-27 / E-15 owners | open | Low |
