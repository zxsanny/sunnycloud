# Android sign-in and token lifecycle pipeline (F2 step 7, F16 step 7)

**Task**: AZ-3672_pipeline_android_sign_in
**Name**: Android sign-in pipeline
**Description**: The Android app signs in through the system browser and a one-time code, keeps its rotating tokens in the token vault and refreshes them single-flight, end to end from Flutter through Kotlin and the shared Rust core to the real server
**Complexity**: 3 points
**Dependencies**: AZ-3985_android_uniffi_device_spike, AZ-3989_mobile_backup_checkpoint_db, AZ-3991_backup_core_surface, AZ-3992_android_auth_token_vault, AZ-3996_tls_trust_bridge, AZ-3999_flutter_backup_ui, AZ-4000_flutter_channel_bridge, AZ-4002_native_library_ci, AZ-4003_android_emulator_harness, AZ-3966_token_lifecycle, AZ-3968_sync_uniffi_surface, AZ-3689_mobile_code_identity_janitor, AZ-3664_pipeline_sign_in, AZ-3652_initial_structure
**Component**: 17 android_app (Flutter + Kotlin app, `mobile_backup` crate)
**Tracker**: AZ-3672
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/p01

## Problem

Mobile sign-in crosses Flutter (button and status), Kotlin (custom tab, `AuthReturn`, `TokenVault`, `TlsTrustBridge`), the `mobile_backup` crate through UniFFI, sync-core's token lifecycle and the server's mobile-code exchange. Each piece has an E-27, E-23 or E-15 child; none drives a real sign-in on an emulator against the real server, and none proves the refresh rules across the worker and the app.

## Outcome

- A user signs in on the emulator through the system browser and the real server; the tokens live only in the token vault and refresh correctly while backups run.
- Every component operation of this flow has a production caller on the app side.
- At least one test runs the flow on the emulator against the real server stack with mock-idp.

## Scope

### Included
- Flow order and seams:
  1. Flutter server screen → MethodChannel → Kotlin → `BackupCore::set_server` (https only unless the debug flag; server-info check; user-added CA through `TlsTrustBridge`).
  2. Flutter sign-in → Kotlin opens the system browser with PKCE → server login (mobile client kind) → mock-idp → callback → one-time code through `sunnycloud://auth`.
  3. Kotlin `AuthReturn` → `BackupCore::exchange_code(code, verifier, device label)` → `SyncApi::exchange_mobile_code` → server → session tokens → `TokenVault::save`; code and verifier cleared.
  4. During runs: 401 → sync-core single-flight `refresh` → `CredentialProvider::on_rotated` persists the new refresh token before use.
  5. Family revoked → refresh fails → backup stops with a sign-in message; `BackupCore::sign_out` → server revoke → vault cleared.
  6. Status to Flutter over the EventChannel, without token fields.
- The emulator test of the steps above.

### Excluded
- Camera backup itself (E-27/p02), server-side sign-in (E-22/p02), iOS (E-28/p01).

## Acceptance Criteria

AC-1..AC-5 are local to this task. No interaction risk of `interaction-risks.md` lies on this flow.

### Wiring and flow criteria

**AC-1: Sign-in on the emulator**
Given the emulator with the release-signed debug-flag-off build and the real server with mock-idp
When the user enters the https server address and signs in through the system browser
Then the app shows signed in, the token vault holds the tokens, and no token appears in Dart state, intents, logs or EventChannel payloads

**AC-2: Single-flight refresh with persist-before-use**
Given a signed-in app and the test clock past the 15-minute access lifetime
When the background worker and the foreground app call the server at the same moment
Then exactly one refresh request reaches the server, and the new refresh token is in the vault before the new access token is used

**AC-3: Revoked family stops the backup**
Given a signed-in app
When its session family is revoked on the web and the next backup run starts
Then the run stops with the sign-in message and the vault is cleared

**AC-4: Address and trust rules**
Given an `http://` address and an https server with a user-added CA
When each is entered
Then the http address is refused, and the user-CA server is accepted with the persistent user-CA warning

**AC-5: Every flow operation has a production caller**
Given the app sources (Dart, Kotlin, Rust)
When the wiring check searches non-test code
Then it finds `AuthReturn` calling `BackupCore::exchange_code`, Kotlin calling `BackupCore::{set_server, refresh, sign_out}`, the `CredentialProvider` over `TokenVault` with `on_rotated`, and no token field in the EventChannel schema

## Non-Functional Requirements

**Compatibility**
- Android emulator (profile `android-emu`) in CI; Samsung and Xiaomi devices in the lab.

**Reliability**
- A kill between the refresh response and the vault write never loses the family when the restart is inside the 30 s reuse grace.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Caller check over Dart, Kotlin and Rust sources | Each named call has a production caller; no token in the channel schema |

## Blackbox Tests

Appium on the emulator against the real stack; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Emulator, real server, mock-idp | Sign in | Signed in; tokens only in the vault | AC-54 |
| AC-2 | Signed in, test clock past 15 min | Concurrent worker and app calls | One refresh; persist before use | |
| AC-3 | Signed in | Revoke family, run | Stops with sign-in message, vault cleared | AC-21 |
| AC-4 | http address; user-CA server | Enter address | Refused; accepted with warning | AC-54 |

## Constraints

- Owner: component 17 (`src/mobile/` and `src/mobile_backup/`): the sign-in composition across Flutter, Kotlin and Rust, and this flow's emulator tests.
- No fake of sync-core or the server; only the identity provider is mocked.
- Downstream: E-31/c27 (mobile scenarios) and E-31/c34 should depend on this task.

## Risks & Mitigation

**Risk 1: Token reaches Dart**
- *Risk*: a status payload or a MethodChannel reply carries a token.
- *Mitigation*: AC-1 and AC-5 inspect payloads and the schema.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Single-library UniFFI build on device (NDK r28, 16 KB alignment, JNA) is still a spike (E-27/c01) | E-27 | open | Medium |
