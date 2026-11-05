# iOS photo backup pipeline [v1.1] (F16 iOS, F2 step 7 iOS)

**Task**: AZ-3674_pipeline_ios_photo_backup
**Name**: iOS photo backup pipeline
**Description**: The iOS app signs in through an authentication session and runs the shared Rust backup loop from every iOS trigger over PhotoKit, uploading to the real server without duplicates, end to end on a device or simulator
**Complexity**: 5 points
**Dependencies**: AZ-3973_ios_ac78_sibling_spike, AZ-3974_ios_live_photo_form_spike, AZ-3975_ios_uniffi_static_library, AZ-3976_ios_photokit_asset_source, AZ-3977_ios_keychain_credentials, AZ-3978_ios_auth_session_provider_rule, AZ-3979_ios_background_scheduling, AZ-3980_ios_flutter_ui, AZ-3981_ios_icloud_optimise_opt_in, AZ-3982_ios_reinstall_rebuild, AZ-3983_ios_xcuitest_harness, AZ-3984_ios_memory_acceptance, AZ-3990_run_backup_loop, AZ-3991_backup_core_surface, AZ-3966_token_lifecycle, AZ-3968_sync_uniffi_surface, AZ-3664_pipeline_sign_in, AZ-3655_pipeline_large_upload, AZ-3656_pipeline_small_upload_batch, AZ-3652_initial_structure, AZ-4024_sign_in_with_apple_provider
**Component**: 18 ios_app (Swift layer of the shared Flutter project)
**Tracker**: AZ-3674
**Epic**: AZ-3641 (E-28)
**Ref**: E-28/p01

## Problem

The iOS app reuses the Rust uploader from E-27 and replaces the platform side: `ASWebAuthenticationSession` sign-in, a Keychain credential store, a PhotoKit `AssetSource`, and best-effort triggers (app open, foreground observer, `BGProcessingTask`, `BGAppRefreshTask`) that must call `cancel()` on expiry. Each has an E-28 child; none composes them into a running app against the real server.

## Outcome

- On an iPhone or simulator a user signs in and new photos reach the real server from every iOS trigger, with edited renders linked to their originals and no duplicate across expiry, kill and reinstall.
- Every component operation of the iOS flow has a production caller on the Swift side.
- At least one test runs the flow on iOS against the real server stack.

## Scope

### Included
- Flow order and seams:
  1. Flutter sign-in → Swift `ASWebAuthenticationSession` (PKCE) → server login → mock-idp → one-time code → `BackupCore::exchange_code` → Keychain `CredentialProvider` (persist before use on rotation).
  2. Triggers: app open, foreground observer, `BGProcessingTask`, `BGAppRefreshTask` → `BackupCore::run_backup` with the PhotoKit `AssetSource` (change tokens, `group_id` for edited renders, access levels, `open_read`).
  3. Task expiry handler → `BackupCore::cancel()`; the next run resumes from the checkpoint.
  4. iCloud-only assets raise `NeedsDownload`; downloaded only with the opt-in (Wi-Fi and charging gating).
  5. Uploads through sync-core (F4, F5) to the server; reinstall rebuild through `lookup_revisions`.
  6. Status → Swift → EventChannel → Flutter.
- The iOS test of the steps above.

### Excluded
- Android (E-27/p01, p02), the shared Rust loop's internals (E-27/c06), vault (v1.2).

## Acceptance Criteria

AC-1..AC-5 are local to this task. No interaction AC is attached yet: the iOS sibling of AC-78 is still to be written by the owner spike E-28/c01 (see Flagged concerns).

### Wiring and flow criteria

**AC-1: Sign-in on iOS**
Given the app on a simulator and the real server with mock-idp
When the user signs in through the authentication session
Then the app shows signed in, the Keychain holds the tokens with "after first unlock, this device only", and no token appears in Dart or logs

**AC-2: Photos back up from each trigger**
Given 10 new photos, one of them edited
When backup runs on app open, from the foreground observer and from a simulated `BGProcessingTask`
Then all photos are on the server, the edited render is linked to its original through `group_id`, and each run went through `run_backup`

**AC-3: Expiry and kill resume without duplicates**
Given a background run of 200 photos
When the task expires mid-run (expiry handler fires) and later the app is killed during a foreground run
Then `cancel()` is called on expiry, the following runs complete with exactly 200 server files, and nothing is uploaded twice

**AC-4: iCloud-only assets and reinstall**
Given iCloud-only photos with the opt-in off, then on with Wi-Fi and charging; and a fully backed-up device that is reinstalled
When backup runs
Then the opt-in-off run skips them with `NeedsDownload`, the opt-in run downloads and uploads them, and after the reinstall 0 bytes are re-sent

**AC-5: Every flow operation has a production caller**
Given the iOS sources (Swift) and the shared Rust crate
When the wiring check searches non-test code
Then every Swift trigger calls `BackupCore::run_backup`, the expiry handler calls `cancel()`, the authentication-session callback calls `exchange_code`, and the PhotoKit `AssetSource` methods are called from Rust

## Non-Functional Requirements

**Performance**
- Best effort only (iOS gives no background guarantee); memory within the PT-18-1 budget.

**Compatibility**
- Simulator in the lab CI; a physical iPhone for background tasks (lab, NOT-RUN when absent).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Caller check over Swift and Rust sources | Each named call has a production caller |

## Blackbox Tests

Appium XCUITest against the real stack; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Simulator, real server, mock-idp | Sign in | Signed in; Keychain only | |
| AC-2 | 10 photos, one edited | App open, foreground, BG task | All on server, edit linked | |
| AC-3 | 200 photos | Expire mid-run; kill | `cancel()` called; 200 files, no duplicates | |
| AC-4 | iCloud-only photos; reinstall | Opt-in off and on; reinstall | Skip, then upload; 0 bytes re-sent | |

## Constraints

- Owner: component 18 (`src/mobile/ios/`): trigger and sign-in composition on the Swift side and this flow's iOS tests. Changes to the shared Flutter `lib/` go through an E-27 task.
- No fake of sync-core or the server; PhotoKit is the simulator's or device's real library.
- Downstream: E-31/c48 ([v1.1] iOS scenarios) should depend on this task.

## Risks & Mitigation

**Risk 1: Background time runs out before a block finishes**
- *Risk*: iOS kills the task before `cancel()` takes effect, leaving a half-sent batch.
- *Mitigation*: the saved commit id makes the replay idempotent; AC-3 checks the file count.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AC-78 has no iOS sibling yet; E-28/c01 (owner) must add it, then it attaches here as an interaction AC | Owner via E-28/c01 | open | Low |
| Live Photo stored form and checkpoint columns are still a spike (E-28/c02) | E-28 | open | Low |
