# Test Specification — 18 iOS App [v1.1, stub-level]

Release v1.1: none of these tests run for v1.0. They are written at the level the stub description allows and are expanded when 18 is planned. Component tests complement the system-level scenarios (`02_document/tests/`) and reference them by ID. Tiers: **Unit (Rust)** = shared `mobile_backup` tests of 17 (not repeated here); **IT-IOS** = Appium XCUITest on a real iPhone (`H-IOS`, DEV-IPHONE) and, for non-background flows, the simulator, against the real server container with real PostgreSQL; the Mac and phone are owner hardware, so a run without them is `NOT-RUN(H-IOS)`, never PASS. Interfaces used are only those in the description (`BackupCore`, `AssetSource`, the Keychain token store, `CredentialProvider`, `CheckpointStore`).

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|-----------------------------|----------|----------|
| AC-9 | iOS camera backup best effort; foreground catch-up completes the backlog [v1.1] | IT-18-3, IT-18-4, AT-18-1 | Covered for the foreground part; the background part has no time guarantee, so it is recorded not pass/fail; system FT-P-31 |
| AC-19 | iOS shows Google/GitHub sign-in only when Sign in with Apple is enabled [v1.1] | IT-18-1, AT-18-2 | Covered; system FT-N-34, FT-P-63 |
| AC-46 | iOS reinstall uploads 0 media bytes [v1.1] | IT-18-6 | Covered; system NFT-RES-28 (iOS leg); Live Photo pairing deferred (IT-18-10) |
| AC-52 | One implementation; bounded memory; kills; foreground/background [v1.1] | IT-18-4, IT-18-9, PT-18-1 | Covered; system FT-P-34 equivalent, NFT-RES-27, NFT-RES-28, NFT-RES-LIM-03 |
| AC-53 | Limited library: "N photos visible, tap to allow more" [v1.1] | IT-18-2, AT-18-3 | Covered; system FT-P-35 |
| AC-54 | Plain-HTTP sign-in refused [v1.1] | IT-18-7, ST-18-2 | Covered; system FT-N-26, NFT-SEC-12 |
| AC-78 sibling | Foreground and background upload one asset once on iOS | IT-18-9 | BLOCKED — the description says "a v1.1 sibling criterion is proposed" but none exists in `acceptance_criteria.md`; the test is written against the shared mutex and key rules and gets an AC id when the sibling is added |
| RESTRICT-33 (iOS keychain check) | Token not synced to iCloud, not in backups | ST-18-1 | Covered; system NFT-SEC-44 (Keychain check) |

## Blackbox Tests

### IT-18-1: Sign-in provider list follows Sign in with Apple
- **Summary**: Google and GitHub buttons appear only when the server reports Sign in with Apple enabled.
- **Traces to**: AC-19
- **Input**: server providers list (a) Apple + GitHub + Google OIDC; (b) GitHub only; (c) Apple only; (d) Google only.
- **Expected**: (a) all three buttons; (b) and (d) no Google/GitHub button and a message that sign-in is unavailable on iOS for this server; (c) Apple only; the list is read from the API, not hard-coded. System FT-N-34, FT-P-63.
- **Max time / Deps**: 5 min; server with mock-idp (Apple-shaped provider); DEV-IPHONE or simulator.

### IT-18-2: Limited library banner
- **Summary**: Limited authorization shows the coverage banner and picks are backed up on the next run.
- **Traces to**: AC-53
- **Input**: authorize "Selected Photos" with 20 of 300 assets; pick 10 more; switch to Denied; then Full.
- **Expected**: `20 photos visible, tap to allow more` (same wording as 17, no library total); tap opens the limited-library picker; the next run's `list_visible_ids` diff queues the 10 picks and uploads them (30 server files); `Denied` -> `Paused(PermissionDenied)`; authorization re-read at the start of every run. System FT-P-35.
- **Max time / Deps**: 10 min; DEV-IPHONE.

### IT-18-3: Foreground catch-up progress
- **Summary**: The foreground run shows visible, resumable progress with an ETA.
- **Traces to**: AC-9
- **Input**: 1,000 assets (DS-21 subset) pending; app open in the foreground for the whole run; then lock the screen halfway and reopen.
- **Expected**: UI shows "backing up n of N" (N = visible assets) with the permission level and an ETA; after the lock/suspend the run resumes from the checkpoint losing at most one block; the backlog completes while the app stays in the foreground; no claim that it completes in the background.
- **Max time / Deps**: 30 min; DEV-IPHONE. System FT-P-31.

### IT-18-4: Background tasks are best effort and resumable
- **Summary**: `BGProcessingTask` and `BGAppRefreshTask` runs resume from the checkpoint; no timing is asserted.
- **Traces to**: AC-9, AC-52
- **Input**: schedule both tasks, trigger them with the Xcode `_simulateLaunchForTaskWithIdentifier` debugger command; expire the task window mid-block.
- **Expected**: the Rust loop runs only inside the granted window; on expiry the run stops cleanly (`cancel()`), the checkpoint is consistent and the next run continues; at most one block is lost; arrival times are recorded for the report but never asserted (RESTRICT-27).
- **Max time / Deps**: 15 min; DEV-IPHONE with a debug build for task triggering.

### IT-18-5: Rotating-token persistence on iOS
- **Summary**: `on_rotated` over the Keychain returns only after the new pair is stored.
- **Traces to**: AC-52
- **Input**: the 15-minute fixture (NFT-RES-33); kill the app between the server's rotation and the Keychain write; restart within and after the 30 s grace.
- **Expected**: same results as IT-17-4: one refresh per expiry, persist-before-use, old refresh token works inside the grace, sign-in shown after it.
- **Max time / Deps**: 45 min; DEV-IPHONE, fixture with test clock.

### IT-18-6: Reinstall rebuild
- **Summary**: A reinstall uploads no media again.
- **Traces to**: AC-46
- **Input**: 2,000 backed-up assets; delete and reinstall; sign in.
- **Expected**: the checkpoint is rebuilt from the listing of `Camera Uploads/<device label>/` and, as fallback, hashing plus batched `lookup_revisions` (1,000 per request); 0 media bytes in the proxy log; the collision suffix from the asset id is kept for equal display names. System NFT-RES-28.
- **Max time / Deps**: 20 min; DEV-IPHONE.

### IT-18-7: Plain HTTP refused
- **Summary**: The iOS app refuses `http://` sign-in by default.
- **Traces to**: AC-54
- **Input**: enter an `http://` server address in the release build.
- **Expected**: clear error, 0 cleartext requests (proxy/packet capture), no code or token sent. System FT-N-26.
- **Max time / Deps**: 5 min; simulator is enough.

### IT-18-8: iCloud-optimised assets
- **Summary**: iCloud-only assets raise `NeedsDownload` and are fetched only with the opt-in.
- **Traces to**: AC-9
- **Input**: 20 iCloud-only assets; opt-in off (default), then on with Wi-Fi and charging, then on without charging.
- **Expected**: opt-in off: the 20 are listed as pending with a reason, 0 downloads from iCloud; on with Wi-Fi and charging: they are downloaded and uploaded; without charging: waiting; the visible count equals uploaded + pending + skipped.
- **Max time / Deps**: 20 min; DEV-IPHONE with iCloud account (owner).

### IT-18-9: One run at a time and idempotent keys on iOS
- **Summary**: The shared mutex and derived `client_key` give one server file when foreground and a background task overlap.
- **Traces to**: AC-52
- **Input**: start a foreground run while a `BGProcessingTask` is triggered, 100 new assets.
- **Expected**: the second `run_backup` returns `Skipped(AlreadyRunning)`; 100 server files after both finish; the per-asset `client_key` equals `derive_client_key(device_id, asset_id, file_hash)`. Becomes the iOS leg of AC-78 when the sibling criterion exists (see traceability).
- **Max time / Deps**: 10 min; DEV-IPHONE.

### IT-18-10: Edited renders and Live Photo pairing
- **Summary**: Original and edited renders are linked by `group_id`; Live Photo pairing is deferred.
- **Traces to**: AC-46
- **Input**: an edited photo (original + render) and a Live Photo.
- **Expected**: edited pair: two resources linked by `group_id`, uploaded as linked entries. Live Photo: BLOCKED — FT-P-33 and its fixture are deferred to v1.1 planning; the expected stored form (one asset with a linked still and video) has no confirmed API or checkpoint columns beyond `content_identifier`.
- **Max time / Deps**: 10 min; DEV-IPHONE.

## Performance Tests

### PT-18-1: Memory during a background run
- **Summary**: The uploader stays within the mobile memory bound on iOS.
- **Traces to**: AC-52
- **Load**: foreground first sync of 5,000 assets and one granted background window; memory sampled with Instruments (footprint) every 5 s.
- **Targets / failure**: footprint <= 120 MB / above 150 MB (AC-52 bound; the iOS extension ceiling is stricter and tested when the extension exists). System NFT-RES-LIM-03.

## Security Tests

### ST-18-1: Keychain handling
- **Summary**: The token is stored with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` and not synced.
- **Traces to**: RESTRICT-33
- **Attack vector**: iCloud Keychain sync, encrypted device backup restore to another device, log inspection.
- **Procedure**: inspect the Keychain item attributes on a jailbroken/test device or with a debug probe; restore an encrypted backup to a second device; grep logs for the sentinel token.
- **Pass**: accessibility attribute as stated, `kSecAttrSynchronizable` false, restored device signed out, 0 token occurrences in logs. **Fail**: any other result. System NFT-SEC-44 (Keychain check).

### ST-18-2: Cleartext and downgrade
- **Summary**: No plaintext credentials on the wire.
- **Traces to**: AC-54
- **Attack vector**: `http://` address, https-to-http redirect.
- **Procedure**: run sign-in and a backup against each variant, capture traffic.
- **Pass**: 0 cleartext bytes with tokens or codes. **Fail**: any.

## Acceptance Tests

### AT-18-1: Foreground backup of a backlog
- **Summary**: A user can bring the library up to date by keeping the app open.
- **Traces to**: AC-9
- **Preconditions**: signed in, 500 pending assets.
- **Steps**: (1) open the app: the progress with ETA appears; (2) keep it open: the count reaches 500 of 500; (3) the server holds 500 files in `Camera Uploads/<label>/`.

### AT-18-2: Provider rule
- **Summary**: The sign-in screen follows the server's Apple setting.
- **Traces to**: AC-19
- **Preconditions**: two servers, one with Sign in with Apple enabled.
- **Steps**: (1) connect to the server with Apple: Apple, Google and GitHub offered; (2) connect to the server without Apple: no Google or GitHub button.

### AT-18-3: Limited access
- **Summary**: The coverage banner is shown and acted on.
- **Traces to**: AC-53
- **Preconditions**: "Selected Photos" authorization with 20 photos.
- **Steps**: (1) the banner reads `20 photos visible, tap to allow more`; (2) tap and pick 10 more; (3) the next run uploads them.

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-21 | phone fixtures (unique JPEG/MP4, edited pairs) | `gen_phone.py`, imported into Photos with Xcode/`simctl addmedia` | 1 GB |
| Token fixture | 15-minute access, rotating refresh, test clock | NFT-RES-33 fixture | n/a |

**Setup**: owner Mac with Xcode and an iPhone; the `lan` profile with a publicly trusted certificate (iOS ignores user CAs); Appium XCUITest; account via invite token; Apple-shaped provider from `mock-idp`.
**Teardown**: delete the app and test photos, purge the account through `e2e-ctl`.
**Isolation**: one account and device label per test; background-task triggers use the debug command, never wall-clock waiting; all integration runs use the real server container with real PostgreSQL, and any fake is limited to the Rust unit tests at the client trait boundary.
