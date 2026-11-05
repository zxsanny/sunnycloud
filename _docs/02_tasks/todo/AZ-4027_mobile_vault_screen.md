# Mobile vault screen in the shared Flutter shell [v1.2]

**Task**: AZ-4027_mobile_vault_screen
**Name**: Mobile vault screen
**Description**: A v1.2 vault screen in the shared Flutter app shell: create a vault with the passphrase rules and a one-time recovery-key panel, unlock with the passphrase or recovery key, list, upload and download vault files, show the limit and tampering errors, and lock, with every crypto step done by the vault core through the Kotlin layer
**Complexity**: 5 points
**Dependencies**: AZ-4026_mobile_vault_operations, AZ-3999_flutter_backup_ui, AZ-4000_flutter_channel_bridge, AZ-3990_run_backup_loop, AZ-4003_android_emulator_harness; notes: new task `N-06_mobile_vault_operations` the parent resolves its key
**Component**: 17 android_app (Flutter UI in the shared Dart app) [v1.2]
**Tracker**: AZ-4027
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/n03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/17_android_app/backup_core.md` — v1.2 vault section written by E-27/n02.
- `_docs/02_document/contracts/17_android_app/status_channel.md` — how Flutter sends commands and receives results (E-27/c16).
- `_docs/02_document/ui_mockups/DESIGN.md` — tokens and Flutter theming row (vault screens are "Not drawn").

## Problem

The UniFFI surface of the vault core has no caller on the phone (AZ-4014, AZ-4002 and AZ-3675 flag it), and F17 lists mobile among its clients. The web vault page (AZ-3954) is the model: the user creates, unlocks, uses and locks a vault, and every rule (passphrase policy, size limit, tamper rejection) comes from the core, never from a copy in the page. On the phone the screen lives in the shared shell of AZ-3999, so 18 (iOS) can reuse it; Flutter only shows state and sends commands (ADR-013).

## Outcome

- In a v1.2 build, the app shell shows a vault entry; v1.0 and v1.1 builds show none.
- Create: passphrase entry with the core's reasons shown, a generated-passphrase option, the recovery key shown once in a panel and gone after it closes.
- Unlock with passphrase or recovery key; a wrong passphrase shows a clear error.
- The vault folder's files listed; a picked file uploaded; a listed file downloaded and opened; errors for the size limit and for tampered data named in plain words.
- Lock returns the screen to the locked state and leaves no passphrase, recovery key or key in the app.

## Scope

### Included
- The vault screens and their states (no vault, locked, unlocked, uploading, downloading, error), the commands to the Kotlin layer, light and dark themes from the design tokens.
- Widget tests and emulator tests (AT-style) against the real v1.2 stack.

### Excluded
- The vault operations themselves (E-27/n02); passphrase change (not in the mobile list); share links for vault files (none in v1.2, owner decision 2026-11-05); camera backup into a vault (never).
- The iOS host side of these commands (E-28; see Flagged concerns).

## Acceptance Criteria

**AC-1: Passphrase rules shown, not re-implemented (AC-14, FT-N-37)**
Given a signed-in account without a vault
When the user enters an 11-character passphrase, a 12-character passphrase of zxcvbn score 3, a 12-character passphrase of score 4, and then picks a generated passphrase
Then the first shows the 12-character minimum, the second the strength reason, the last two create a vault, and the recovery key is shown once in a panel and appears nowhere after the panel closes

**AC-2: Unlock (AC-14)**
Given an existing vault
When the user unlocks with the right passphrase, with the recovery key, and with a wrong passphrase
Then the first two reach the unlocked state, and the third shows a clear error and stays locked

**AC-3: Upload and download (AC-13, FT-P-44 steps 1-3)**
Given an unlocked vault
When the user uploads a 10 MiB file and then downloads and opens it from the list
Then the file appears in the list, the opened file's hash equals the original, and the server holds only ciphertext of it

**AC-4: Limit and tamper messages (AC-14, FT-P-45 steps 1-2, NFT-SEC-39 step 4)**
Given the vault limit of 100 MiB and a vault file the test server has tampered with
When the user uploads a 101 MiB file and downloads the tampered file
Then the first shows a message naming the 100 MiB limit, the second shows a clear error, and no file is offered as opened or saved

**AC-5: Lock leaves nothing behind (AC-14, NFT-RES-LIM-08 step 2)**
Given an unlocked vault with a known test key and a typed passphrase
When the user locks it and a heap dump of the app process (Dart and native) is searched
Then the screen shows the locked state, 0 matches of the key bytes, the passphrase or the recovery key are found, and the next action needs a new unlock

**AC-6: Camera backup stays out (AC-14, FT-P-45 step 3)**
Given a vault on the device and camera backup on
When a new camera photo is taken and backup runs
Then the photo is stored outside any vault, the vault file count is unchanged, and the vault screen offers no backup-target option

**AC-7: Release gating (ADR-016)**
Given the v1.0, v1.1 and v1.2 builds
When the app shell opens
Then only the v1.2 build shows the vault entry

## Non-Functional Requirements

**Compatibility**
- Shared Dart code only; platform calls go through the channel (ADR-013, S4-4).

**Reliability**
- The screen never holds content in Dart: upload and download progress come from the Kotlin layer.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Screen shows the reasons the core returns | No passphrase rule in Dart |
| AC-1 | Recovery-key panel | Shown once; cleared on close |
| AC-4 | Error states for limit and tamper results | Message per result; no "opened" state |
| AC-7 | Shell with and without the vault feature | Entry only in v1.2 |

## Blackbox Tests

Emulator against the real v1.2 stack (harness E-27/c19).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Four passphrases | FT-N-37 (Android UI leg) | Reasons shown; 2 created | AC-14 |
| AC-2 | Existing vault | Unlock three ways | Unlocked, unlocked, error | AC-14 |
| AC-3 | 10 MiB file | FT-P-44 steps 1-3 through the screen | Listed; equal hash; ciphertext on server | AC-13 |
| AC-4 | 101 MiB file; tampered file | FT-P-45 steps 1-2; NFT-SEC-39 step 4 (Android) | Limit message; tamper error | AC-13, AC-14 |
| AC-5 | Known key, typed passphrase | NFT-RES-LIM-08 step 2 (Android) | 0 matches; locked | AC-14 |
| AC-6 | Vault and camera backup | FT-P-45 step 3 | Photo outside vault; count unchanged | AC-14 |

## Runtime Completeness

- **Named capability**: the mobile vault client of F17.
- **Production code that must exist**: the screens calling the real vault operations of E-27/n02 over the channel.
- **Allowed external stubs**: a scripted channel in widget tests only.
- **Unacceptable substitutes**: Dart crypto or a Dart copy of the passphrase rule, a test-only screen, keys or passphrases kept in Dart after lock.

## Constraints

- Flutter only displays state and sends commands; no key, token or content crosses into Dart except the passphrase the user types, which is sent once and cleared.

## Risks & Mitigation

**Risk 1: Passphrase in the Dart heap**
- *Risk*: the passphrase is typed in a Flutter field, so a copy may stay in the Dart heap after it is sent.
- *Mitigation*: AC-5 scans the Dart heap too; see Flagged concerns.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No vault screens are drawn (DESIGN §8 "Not drawn: vault page"); mobile screens must be drawn and approved before implementation | Owner (design) | open | Medium |
| Where a picked file comes from (system file picker, share sheet) and where an opened file goes on Android are not stated; file browsing and download on Android are out of v1.0 by owner decision | Owner | open | Medium |
| The passphrase typed in Flutter crosses the channel and may stay in the Dart heap; neither 17 nor 19 states a rule for it, and Dart strings cannot be wiped | Owner / reviewer | open | Medium |
| Whether the vault locks itself when the app goes to the background or after a timeout is not stated | Owner | open | Low |
| iOS (18) reuses the shared shell, but its Swift side of these commands and the iOS library with the vault core are E-28 work that has no task | E-28 owner | open | Medium |
| One or many vaults per account (E-30/c01) changes the screen layout | E-30/c01 | open | Medium |
| AZ-3675 (vault pipeline) should list E-27/n01, E-27/n02 and this task once they have keys | Parent (Step 4) | open | Low |
