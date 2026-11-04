# Interaction Risks

Step 2.5 of planning. Derived from the 17 flows in `system-flows.md` and the topology in `architecture.md`; names follow `canonical-names.md`. Material risks are promoted to acceptance criteria AC-68 to AC-80 in `_docs/00_problem/acceptance_criteria.md` (section "Interaction outcomes"). Tests for these ACs are added in Step 5 and appended to the system-level scenario files.

## Multi-component flows and seams

| Flow | Seam (producer → consumer: what crosses) |
|------|------------------------------------------|
| F1 Bootstrap | CLI → API/DB: setup token row; operator → first sign-in |
| F2 Sign-in | Browser → provider → API: state/nonce/code; API → DB: account + identity + invite redemption |
| F3 Device login | Daemon → API: poll with device code; browser → API: approval; API → daemon: token |
| F4 Large upload | Client → API: blocks; API → blob volume: ciphertext; API → DB: session bitmap, reserved bytes, revision, feed counter |
| F5 Small upload / batch | Client → API: single request / batch with commit id; API → DB: receipts |
| F6 Download | API ← DB (revision) + blob volume (decrypt stream) → client (Range) |
| F7 Sync | DB feed → API long-poll → devices: cursor `epoch.seq` |
| F8 Conflict | Device → API: base revision; API → DB: conflict node sharing a blob |
| F9 Desktop watcher | OS events → daemon → LocalIndex → API; API → daemon → filesystem |
| F10 Media | 02 commit → 08 job row (`jobs`, reached only through the API) → 09 supervisor claims over the worker listener (`:8081`, `/api/v1/worker-jobs/**`, no database login) → decoder (socket, scratch, `network_mode: none`) → API → DB derived |
| F11 Share link | Browser → API: token hash; API → DB node → blob stream |
| F12 GC / retention | DB delete → `blob_gc` queue → GC → blob volume; operator CLI → restore |
| F13 Account deletion | 07 `DeletionPurger` → `AccountLifecycleHook::erase_batch` of 02, 03, 05, 06, 07a, 08, 20 → `blob_gc` (`Due::Now`) → blob volume removal; 01 `finalize_deletion` last |
| F14 Restore | Operator → DB restore → `admin epoch-bump` (writes the `.epoch` marker; `serve` refuses to start without it) → devices resync |
| F15 Scrub | Scrub job → blob volume + DB hashes |
| F16 Camera backup | Android WorkManager / foreground → `mobile_backup` uploader (17, with 13) → API |
| F17 Vault (v1.2) | Client crypto core → API (ciphertext, wrapped keys); share link fragment key |

## Interaction risks

| # | Flow / seam | What can go wrong | AC |
|---|-------------|-------------------|----|
| R1 | F4 API→blob volume vs API→DB | Crash between blob write and commit leaves an orphan or, worse, a revision pointing at a missing blob | AC-68 |
| R2 | F7 commit → feed counter | Concurrent commits leave a gap or duplicate in the sequence; a device skips a change | AC-69 |
| R3 | F8 move vs edit | A folder move on one device races an edit inside it on another; the edit is lost or duplicated | AC-70 |
| R4 | F10 supervisor retry | A job retried after lease expiry creates duplicate derived files or leaves scratch data | AC-71 |
| R5 | F6/F10 encryption vs worker fetch | The worker receives wrong bytes, or a wrong master key silently decrypts to garbage | AC-72 |
| R6 | F12/F14 restore vs retention | A restored database references blobs already removed after retention; users hit errors | AC-73 |
| R7 | F4/F12 quota vs commit/delete | Concurrent commits and deletes drift the usage counter above quota | AC-74 |
| R8 | F2 invite redemption | Two concurrent redemptions create two accounts from one invite | AC-75 |
| R9 | F3 device code | A code is exchanged twice or after expiry | AC-76 |
| R10 | F11 share link vs rename/delete | A link breaks on move, or serves a deleted file | AC-77 |
| R11 | F16 foreground vs background | Both upload the same asset and create duplicates | AC-78 |
| R12 | F10 worker listener | A user or device token reaches worker-only endpoints, or the public proxy exposes them | AC-79 |
| R13 | F15 scrub vs upload/GC | The scrub reports a half-written or just-removed blob as corrupt | AC-80 |

## Already covered by existing criteria (no new AC)
- Echo loop between download and watcher: AC-18. Stale base revision and edit-beats-delete: AC-8. Mass deletes: AC-29. Revocation of live streams: AC-21. Database restore and resync: AC-41. Block durability: AC-38. Quota race at session start: AC-39. Hostile names: AC-37. Vault swap/header attacks: AC-60, AC-61.

## Deferred / accepted
- Same photo uploaded from two devices of one account stores two blobs in v1.0: cross-node content dedupe is out of scope (no commit-by-hash; the batched `revisions:lookup` exists only for reinstall rebuild); revisit if storage cost matters.
- Clock skew between client and server: no new AC; server commit order decides (AC-8).
- Provider outage during invite redemption: sign-in fails clearly (AC-21 sub-case); no partial account is created, covered by AC-75's atomicity.

## Component-owned interaction ACs for Step 3

Final component names (`component-decisions.md`); the first named component is the owner, the others contribute.

- 02 tree (commit transaction, with `TreeReader`): AC-68, AC-69, AC-70, AC-77. Contributors: 03 transfer (AC-68, AC-69, AC-70, AC-74), 05 sync_feed (AC-69), 06 sharing (AC-77).
- 07a quota: AC-74. Contributors: 02 tree (commit and delete transactions call the `QuotaLedger` port), 03 transfer (reserve and release).
- 01 identity: AC-75, AC-76.
- 04 blobstore: AC-72, AC-73, AC-80 (12 operator_cli carries the `doctor` and `key-check` parts; 02 `TreeReader::describe_blobs` serves the AC-73 check).
- 08 media_jobs: AC-71 and AC-79 (`WorkerAuthenticator` is in 08; there is no worker credential or scope in 01). Contributors: 09 worker_supervisor, 10 decoder; AC-79 on the worker listener `:8081` also touches 11 http_api (listener separation, routes absent from the public router).
- 17 android_app (through its `mobile_backup` crate): AC-78. Contributors: 13 sync_core; 18 ios_app reuses the crate in v1.1.
