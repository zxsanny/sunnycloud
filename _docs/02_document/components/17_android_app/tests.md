# Test Specification — 17 Android App

Component tests complement the system-level scenarios (`02_document/tests/`) and reference them by ID. Tiers: **Unit (Rust)** = `mobile_backup` with a fake `AssetSource` (Kotlin-side callback trait) and, at the client trait boundary only, a fake `SyncApi`; **IT-EMU** = release-signed build on the API 34+ emulator (KVM host, userdebug image for `adb root` kill loops) against the real server container with real PostgreSQL, driven by Appium UiAutomator2 and `adb`; **IT-DEV** = physical Pixel, Samsung, Xiaomi (`H-ANDROID`); a run without phones is `NOT-RUN(HW-DEVICE)`, never PASS. Extra emulator images API 30 and 33 cover the minSdk 30 and foreground-service paths. Interfaces used are only those in the description (`BackupCore`, `AssetSource`, `TokenVault`, `AuthReturn`, `TlsTrustBridge`, the Kotlin entry points). Mobile durations come from config mocks (`android.catchup_interval` driven through WorkManager's test driver).

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|-----------------------------|----------|----------|
| AC-9 | Photo uploads in 60 s (Pixel); Samsung/Xiaomi p95 <= 5 min | PT-17-1, IT-17-14 | Covered; Samsung/Xiaomi published not pass/fail; system FT-P-29, FT-P-30, NFT-PERF-12 (H-ANDROID) |
| AC-14 | Camera backup never writes into the vault [v1.2] | — | v1.2: deferred, covered by system FT-P-45 when v1.2 is under test; nothing to test in v1.0 (no vault code) |
| AC-21 | Revoked token: refresh refused clears the vault | IT-17-4 | Covered; system NFT-SEC-08 |
| AC-46 | Reinstall: 0 media bytes; edited photo linked | IT-17-12 | Covered; system FT-P-32, NFT-PERF-22, NFT-RES-28 |
| AC-52 | One implementation; start <= 10 s; <= 150 MB; 10 kills | IT-17-13, IT-17-18, PT-17-2, PT-17-3 | Covered; system FT-P-34, NFT-PERF-23, NFT-RES-27, NFT-RES-LIM-03 |
| AC-53 | "N photos visible, tap to allow more"; picks backed up | IT-17-8, AT-17-2 | Covered; system FT-P-35 |
| AC-54 | Plain-HTTP sign-in refused | IT-17-6, ST-17-3, AT-17-4 | Covered; system FT-N-26, NFT-SEC-12 |
| AC-58 | Refresh single-flight (client) | IT-17-4 | Covered; system NFT-SEC-10, NFT-RES-33 |
| AC-64 | `UpgradeRequired` pauses with the server text | IT-17-15 | Covered; system FT-N-27 |
| AC-76 | Mobile code single use; client discards code and verifier | IT-17-7 | Covered (client side); server rule owned by 01 |
| AC-78 | Foreground and background upload one asset once | IT-17-1, IT-17-2, IT-17-3 | Covered; system FT-P-76 (EMU) |
| RESTRICT-33 | Token excluded from platform backups | IT-17-5, ST-17-1 | Covered; system FT-N-36, NFT-SEC-44 |

## Blackbox Tests

### IT-17-1: One run at a time (Rust unit)
- **Summary**: A second `run_backup` while one runs returns `Skipped(AlreadyRunning)`.
- **Traces to**: AC-78
- **Input**: two `run_backup` calls started within 1 ms on the same process with a 100-asset fake `AssetSource`; a third call after the first ends; a run killed mid-way (process restart).
- **Expected**: one call returns `Completed`/`Partial`, the other `Skipped(AlreadyRunning)`; the third call after completion runs; after a process death the mutex is free (no lease to expire) and the next run starts; the skipped caller makes no `SyncApi` call.
- **Max time / Deps**: 10 s; fake `AssetSource`.

### IT-17-2: Foreground and background at once yield one file per asset
- **Summary**: `AppOpenKick` and `BackupWorker` started together create exactly one server file per asset.
- **Traces to**: AC-78
- **Input**: 100 new photos (DS-21 subset) in MediaStore; start `AppOpenKick` and enqueue `BackupWorker` within 100 ms; repeat 10 times with fresh assets; Kotlin side must enqueue only the unique work `backup` (`KEEP`), `AppOpenKick` raises the "rerun after" flag.
- **Expected**: server shows exactly 100 files per round (listing count), 0 duplicates, 0 conflict copies; a second `backup` work request while one runs is dropped (WorkManager query shows 1 running); a rerun after the flag finds 0 new assets and uploads 0 bytes. System FT-P-76.
- **Max time / Deps**: 5 min; emulator, server.

### IT-17-3: Idempotent upload keeps working without the mutex and unique-work layers
- **Summary**: Layer 3 alone (derived `client_key`, stored `commit_id`) gives one file when two uploaders race.
- **Traces to**: AC-78
- **Input**: two separate processes (the mutex is per process) run `mobile_backup` on the same 100-asset fixture against the real server, each with its own checkpoint DB; batch variant with 300 assets of 1 KiB to 1 MiB.
- **Expected**: 100 server files (not 200); `client_key = derive_client_key(device_id, asset_id, file_hash)` identical in both processes; the loser gets the same session or `NothingToSend{revision}`; batch `commit_id` is random UUIDv7, written to the `batch` table before the first `stage_small`, and never equal to an asset `client_key`; a repeated batch gets the stored response.
- **Max time / Deps**: 5 min; emulator (two processes via `am instrument` or a native test binary), server.

### IT-17-4: Token lifecycle with the 15-minute fixture
- **Summary**: Refresh is single-flight, the new refresh token is stored before use, and rejection signs the user out.
- **Traces to**: AC-58, AC-21
- **Input**: fake-clock server fixture (NFT-RES-33): access 15 min, rotating refresh; a 40-minute run with 4 concurrent block PUTs; kill the app between the server's rotation and the vault write, restart within 10 s and again after 40 s; `TokenRejected` from the server.
- **Expected**: one refresh per expiry across all callers, no lost block or batch; `on_rotated` returns only after `TokenVault.save` completed (timestamp spy) and no request uses the new access token earlier; restart within the 30 s grace works with the old refresh token; restart after 40 s ends in sign-in shown (accepted risk 4), vault cleared; the Flutter layer receives no token (channel payload inspection, IT-17-19).
- **Max time / Deps**: 45 min; emulator, server fixture with test clock.

### IT-17-5: Token excluded from platform backups
- **Summary**: The Keystore-wrapped token lives in `noBackupFilesDir` and a backup archive holds no token.
- **Traces to**: RESTRICT-33
- **Input**: signed-in app; `adb shell bmgr`/`adb backup` full backup and device-transfer extraction; restore onto a clean emulator.
- **Expected**: manifest has `allowBackup="false"` and data-extraction rules excluding the store and the checkpoint auth fields; archive contains 0 occurrences of the access/refresh sentinels; the restored app starts signed out. System FT-N-36.
- **Max time / Deps**: 10 min; emulator.

### IT-17-6: Plain-HTTP address refused
- **Summary**: A release build refuses `http://` and sends nothing.
- **Traces to**: AC-54
- **Input**: enter `http://sunnycloud.test:8080` on the server-address screen of the release build; a debug build with the debug-only flag.
- **Expected**: clear error, 0 cleartext requests (tcpdump on the emulator network), no code or token ever in a cleartext request; debug flag build allows it; BLOCKED sub-case: `BackupCore` has no method to set or validate the server address (only `exchange_code(code, code_verifier, device label)`), so the refusal point inside Rust (`ConnectError::InsecureScheme` of 13) cannot be asserted at the `BackupCore` boundary. System NFT-SEC-12.
- **Max time / Deps**: 5 min; emulator.

### IT-17-7: Sign-in return and PKCE
- **Summary**: `AuthReturn` validates input, passes code and verifier once and clears them.
- **Traces to**: AC-76
- **Input**: `sunnycloud://auth?code=C1&state=S1` with matching state; wrong state; missing code; a second delivery of the same intent; an `http://` base address.
- **Expected**: matching state -> `exchange_code(C1, verifier, label)` called once, tokens stored via `TokenVault`; wrong or missing state/code -> rejected before any network call; the second delivery is ignored (the verifier was cleared), and a manually replayed code gets `InvalidCode` shown; the code is not in logs or process arguments.
- **Max time / Deps**: 5 min; emulator, mock-idp, server.

### IT-17-8: Limited photo access
- **Summary**: The app shows the visible count and backs up newly picked photos on the next run.
- **Traces to**: AC-53
- **Input**: API 34+ emulator, grant "Select photos" with 20 of 300 photos; then pick 10 more older photos (token below the cursor); then revoke to Denied; then full access.
- **Expected**: banner text exactly `20 photos visible, tap to allow more` (no "of 300"; no library total); tap opens the system picker; after picking, the next run's visible-id diff queues the 10 older photos as `new` and uploads them (30 files on the server); `Denied` -> `Paused(PermissionDenied)` with explanation; access level and visible count are re-read at the start of every run; a changed count triggers the diff. System FT-P-35.
- **Max time / Deps**: 10 min; emulator API 34+.

### IT-17-9: Generation catch-up heals a missed trigger
- **Summary**: `CatchUpWorker` finds assets after the stored `ChangeToken` and runs the visible-id diff.
- **Traces to**: AC-9, AC-53
- **Input**: disable `MediaTriggerWorker`; add 5 photos; one with `IS_PENDING=1`, one `IS_TRASHED=1`; edit one photo twice within one change-token tick; advance WorkManager time by `android.catchup_interval`.
- **Expected**: the catch-up run uploads the 5 visible photos; pending and trashed rows are filtered (re-included after they become final); the twice-edited photo yields one upload of the latest hash; the stored `ChangeToken` advances; the diff runs on every catch-up run; `current_token()` is opaque to Rust (round-trips bytes).
- **Max time / Deps**: 10 min; emulator.

### IT-17-10: GPS original access
- **Summary**: The original is opened with `setRequireOriginal` so Exif location is not redacted.
- **Traces to**: AC-46
- **Input**: a JPEG with Exif GPS in MediaStore; run 1 with `ACCESS_MEDIA_LOCATION` granted together with the photo permission; run 2 with it denied.
- **Expected**: run 1: server file BLAKE3 equals the device original and Exif GPS is present when downloaded through the API; run 2: the upload still succeeds with the location-redacted copy; the app "says so in status" — BLOCKED for that assertion: `BackupStatus` has no field for it (`{visible, uploaded, pending, paused, skipped, last_error, access_level}`).
- **Max time / Deps**: 5 min; emulator.

### IT-17-11: Camera folder and collision suffix
- **Summary**: Assets go to `Camera Uploads/<device label>/` and name collisions get the asset-id suffix.
- **Traces to**: AC-46
- **Input**: two assets both named `IMG_0001.jpg` (asset ids 101 in DCIM and 202 in Pictures); a third `IMG_0002.jpg`; delete the server folder before the next run (`ParentGone`); reinstall and rebuild with both names present.
- **Expected**: folder created through `create_folder` and its node id kept in `cursor`; the first processed asset keeps `IMG_0001.jpg`, the other becomes `IMG_0001 (202).jpg` (or `(101)` by processing order); 0 server-named conflict copies; after folder removal the app recreates it and retries once; after reinstall the rebuilt listing keeps both names and uploads nothing.
- **Max time / Deps**: 5 min; emulator, server.

### IT-17-12: Reinstall and edited photo
- **Summary**: A rebuilt checkpoint uploads 0 media bytes; an edited photo links to its original.
- **Traces to**: AC-46
- **Input**: DS-21 (5,000 assets, 100 edited with the same MediaStore id but new hash); uninstall, reinstall, sign in; fallback leg with `client_meta` unavailable (hash all assets + `lookup_revisions` 1,000 per request); the same photo also uploaded by a second device.
- **Expected**: proxy log 0 media bytes for the 5,000; `rebuild_checkpoint` returns `RebuildReport`; fallback issues 5 lookup requests; the edited photo (same asset id, new hash) uploads as a new file with `linked_from` set; a new asset id uploads as an unlinked file; the second device's copy is not looked for (two blobs, per accepted deferral). System NFT-PERF-22, NFT-RES-28.
- **Max time / Deps**: 30 min; emulator, server.

### IT-17-13: First sync survives forced kills
- **Summary**: 10 `kill -9` loops over 50,000 assets cause no duplicates and little re-hashing.
- **Traces to**: AC-52
- **Input**: DS-21 scaled to 50,000 assets; rooted emulator kills the app process at random times (10 kills); `adb dumpsys meminfo` samples.
- **Expected**: at most 1% of assets re-hashed (hash saved after hashing); rows become `uploaded` only after the commit ack, a kill between ack and write is healed by the idempotent upload; 0 duplicate server files; final server count 50,000; `session` rows resume large uploads. System NFT-RES-27.
- **Max time / Deps**: 4 h; emulator, `blobs-large` not needed.

### IT-17-14: Camera backup latency per device (gate S9)
- **Summary**: New photos arrive on time on real phones.
- **Traces to**: AC-9
- **Input**: battery exemption on, Wi-Fi, 30 photos taken at 2-minute spacing per device (Pixel, Samsung, Xiaomi), `MediaTriggerWorker` path and a run with the trigger killed by the OEM (catch-up only).
- **Expected**: Pixel: every photo on the server <= 60 s from capture; Samsung and Xiaomi: p95 <= 5 min, numbers published per device (not a pass gate); catch-up heals a killed trigger within `android.catchup_interval` + 5 min.
- **Max time / Deps**: 2 h per device; H-ANDROID; otherwise `NOT-RUN(HW-DEVICE)`. See PT-17-1.

### IT-17-15: Wire errors map to checkpoint states and pauses
- **Summary**: Each error from 13 leaves the loop running or pauses as specified, and counts add up.
- **Traces to**: AC-64
- **Input**: scripted proxy errors per asset: 507, `SizeTooLarge`, `InvalidName`, `AccountNotActive`, `ParentNotFound`, 426; an asset deleted between listing and upload; metered network; no network.
- **Expected**: 507 -> `Paused(QuotaExceeded)` retried at the next trigger; `SizeTooLarge` -> `skipped_size`; `InvalidName` -> `skipped_name`; deleted asset -> `skipped_gone`; `AccountNotActive` -> `Paused(AccountNotActive)`; `ParentNotFound` -> folder recreated, retry once; 426 -> `Paused(UpgradeRequired)` with the server text; the first three do not stop the loop; `visible = uploaded + pending + skipped{gone,name,size,failed}` at every `status`; metered -> `Paused(MeteredNetwork)` unless the setting allows it.
- **Max time / Deps**: 10 min; emulator, proxy faults.

### IT-17-16: Entry points and large first sync job
- **Summary**: Each entry point only calls `run_backup`; the long first sync uses the right host mechanism per Android version.
- **Traces to**: AC-52
- **Input**: API 34 and API 30/33 emulators; start a first sync of 5,000 photos from the status screen; static scan of Kotlin and Dart sources.
- **Expected**: API 34: a user-initiated data-transfer job with notification; API 30 to 33: a foreground service of type dataSync started by the user's tap with the same notification; no always-on foreground service for automatic backup; Kotlin/Dart sources contain 0 upload-protocol calls (`/api/v1/uploads`, `/staged-uploads`, `commits:batch`) and 0 second uploader; one `.so` per ABI with 16 KB alignment (CI check).
- **Max time / Deps**: 15 min; emulators, CI.

### IT-17-17: Sign-out and TLS trust
- **Summary**: `sign_out` clears the vault even offline; TLS uses the platform verifier.
- **Traces to**: AC-21
- **Input**: `sign_out` with the server unreachable; `TlsTrustBridge` against a publicly trusted certificate; private-CA server.
- **Expected**: `DELETE /api/v1/devices/me` attempted, `TokenVault` cleared regardless, sign-in screen shown; the public cert connects in every process (worker and UI); a private-CA server (CA installed on the emulator as a user CA) connects, `ServerCheck.trust = UserAdded` and the persistent server-screen warning is shown; with the CA removed the connection is refused.
- **Max time / Deps**: 5 min; emulator.

### IT-17-18: Background start latency and memory
- **Summary**: A background trigger starts uploading within 10 s with at most 150 MB.
- **Traces to**: AC-52
- **Input**: app not running, add one 5 MB photo; sample time to first byte at the proxy and `dumpsys meminfo` PSS every second during a 2 GiB upload.
- **Expected**: first byte <= 10 s after the trigger; no Flutter engine started (process list/logcat); memory <= 150 MB; library loaded once, DB opened lazily.
- **Max time / Deps**: 10 min; emulator (functional), H-ANDROID for the numbers.

### IT-17-19: Status UI contents
- **Summary**: The Flutter screens show state faithfully without seeing tokens.
- **Traces to**: AC-53
- **Input**: scripted `BackupStatus` through the EventChannel: uploaded 10, pending 5, paused `QuotaExceeded`, skipped {gone 1, name 2, size 1, failed 0}, `last_error` with correlation id.
- **Expected**: progress, pause reason, coverage banner, skipped counts with reasons and last error with correlation id all rendered; settings (Wi-Fi only, charging only, buckets) persisted and passed to constraints; channel payloads contain no field named or valued like a token (sentinel check); battery/OEM help links per vendor present.
- **Max time / Deps**: 5 min; emulator, Flutter integration test.

## Performance Tests

### PT-17-1: Capture-to-server latency (gate S9)
- **Summary**: Pixel within 60 s; others measured and published.
- **Traces to**: AC-9
- **Load**: IT-17-14 protocol, 30 photos per device, Wi-Fi 100 Mbit/s.
- **Targets / failure**: Pixel max <= 60 s / above 60 s; Samsung and Xiaomi p95 <= 5 min / above 5 min is a published finding, not a release block unless the owner decides; `NOT-RUN(HW-DEVICE)` without phones. System NFT-PERF-12.

### PT-17-2: Memory during first sync
- **Summary**: RSS/PSS stays bounded for 50,000 assets.
- **Traces to**: AC-52
- **Load**: IT-17-13 run, `dumpsys meminfo` every 10 s.
- **Targets / failure**: peak <= 120 MB / above 150 MB; no upward trend above 10 MB per hour. System NFT-RES-LIM-03.

### PT-17-3: Rate of a 50,000-asset first sync
- **Summary**: The first sync is not slowed by checkpoint writes.
- **Traces to**: AC-52
- **Load**: 50,000 assets of 100 KB to 1 MB over `shape-wan`, `android.hash_ahead` 8.
- **Targets / failure**: throughput >= 80% of the link goodput / below 50%; checkpoint transactions per second <= 200 / above 1,000. Targets proposed by this spec; the AC only bounds kills and memory.

## Security Tests

### ST-17-1: Tokens exist only in `TokenVault`
- **Summary**: No token in Dart, logs, intents, backups or process listings.
- **Traces to**: RESTRICT-33
- **Attack vector**: backup extraction, logcat, `ps`, intent extras, Flutter channel capture, app-private file scan.
- **Procedure**: sign in with sentinel tokens (mock server); dump all of the above; scan the Keystore-wrapped file for plaintext sentinel.
- **Pass**: 0 plaintext occurrences outside the encrypted `TokenVault` file; backup archive empty of the sentinel. **Fail**: any occurrence.

### ST-17-2: Deep-link hardening
- **Summary**: Hostile intents cannot start a sign-in or leak a code.
- **Traces to**: AC-76
- **Attack vector**: `sunnycloud://auth` with foreign `state`, oversized code, extra parameters, a second app registering the same scheme, `adb shell am start` with `http` data.
- **Procedure**: fire each intent while signed out and signed in.
- **Pass**: all rejected before any network call; the existing session untouched; no crash. **Fail**: any exchange attempt with a foreign state.

### ST-17-3: Cleartext and downgrade
- **Summary**: No request, with or without a token, goes over plaintext.
- **Traces to**: AC-54
- **Attack vector**: server address `http://`, redirect from https to http, TLS 1.2-only server.
- **Procedure**: run sign-in and a backup against each variant, capture all traffic.
- **Pass**: 0 cleartext bytes containing tokens or codes; TLS 1.2-only server rejected. **Fail**: any cleartext token.

### ST-17-4: Logs and diagnostics
- **Summary**: Logs hold no file names or tokens; the diagnostics export needs confirmation.
- **Traces to**: RESTRICT-33
- **Attack vector**: ring-buffer and Logcat inspection after IT-17-15 and IT-17-11.
- **Procedure**: use sentinel file names and tokens; export diagnostics once with confirmation declined, once accepted.
- **Pass**: 0 sentinel names/tokens; asset ids only at DEBUG; export happens only after confirmation. **Fail**: any hit or export without confirmation.

## Acceptance Tests

### AT-17-1: Sign in and first backup
- **Summary**: A user signs in and the first photo reaches the server.
- **Traces to**: AC-9, AC-54
- **Preconditions**: release build, server with mock-idp, battery exemption granted, Wi-Fi.
- **Steps**: (1) enter the HTTPS address and sign in through the browser: status screen shows signed in; (2) take a photo: it appears in `Camera Uploads/<label>/` within 60 s (Pixel); (3) status shows uploaded count increased by 1.

### AT-17-2: Limited access
- **Summary**: The coverage banner and picks work.
- **Traces to**: AC-53
- **Preconditions**: API 34+, 20 of 300 photos shared.
- **Steps**: (1) open the app: `20 photos visible, tap to allow more`; (2) tap, pick 10 more: the next run uploads them; (3) no total library count is shown.

### AT-17-3: Reinstall
- **Summary**: A reinstalled app does not upload again.
- **Traces to**: AC-46
- **Preconditions**: 5,000 backed-up assets.
- **Steps**: (1) uninstall and reinstall, sign in: the checkpoint is rebuilt; (2) wait for idle: 0 media bytes sent; (3) edit one photo: it uploads as a new linked file.

### AT-17-4: Plain HTTP refused
- **Summary**: The app protects credentials.
- **Traces to**: AC-54
- **Preconditions**: release build.
- **Steps**: (1) enter an `http://` address: refused with a clear message; (2) enter the `https://` address: sign-in proceeds.

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-21 | 5,000 unique JPEG/short MP4 assets, 100 edited, scalable to 50,000 | `gen_phone.py` | 1 GB (50,000: 8 GB) |
| GPS fixture | JPEG with Exif GPS | repository fixture | < 1 MB |
| Token fixture | 15-minute access and rotating refresh server with test clock | NFT-RES-33 fixture | n/a |

**Setup**: `tests/run.sh --profile android-emu`; Appium server, `adb push` of assets and `content call`/media scan; account via invite token; short WorkManager periods through the test driver; real devices through the lab controller with Wi-Fi adb.
**Teardown**: uninstall the app, clear MediaStore test assets, purge the account through `e2e-ctl`, reset the emulator snapshot after kill loops.
**Isolation**: one emulator snapshot, one account and one device label per test; fakes (`AssetSource`, `SyncApi`) only in the Rust unit tests; every IT runs against the real server container with real PostgreSQL; kill loops and test-clock runs are group `X`.
