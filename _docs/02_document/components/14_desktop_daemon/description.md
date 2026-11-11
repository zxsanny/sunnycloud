# 14_desktop_daemon

## 1. High-Level Overview

**Purpose**: A per-user background process that keeps local folders (sync pairs) identical to server folders, safely: it turns filesystem hints into verified uploads, feed events into local writes, and refuses to propagate destructive mistakes.

**Single responsibility**: the *local side* of sync on Windows and Linux (macOS v1.1). It decides what to send and what to write. It does not implement the wire protocol (13_sync_core), the settings UI (15) or any server logic.

**Architectural pattern**: one process, one tokio runtime, one `FeedLoop` per account (one long-poll, one cursor, fan-out to pairs by ancestry), one actor per pair (`PairWorker`) for local changes, all sharing an `Arc<dyn SyncApi>`; the OS watcher is a hint source, the LocalIndex plus the disk is the truth. Tray and IPC are thin adapters over a `DaemonControl` trait. Two crates: the daemon binary and `ipc_schema`, the IPC schema crate (request/response types, method names, errors) that 14 owns and 15 consumes; 15 depends on nothing else of 14.

**Upstream dependencies**: 13_sync_core (`SyncApi` and the host traits it requires, defined in the client crate `sync-core`, section 2a), helpers 01 config, 02 error, 03 clock, 05 ids/hashing, 06 names; server API (11) at runtime through 13.
**Downstream consumers**: 15_desktop_settings (JSON-RPC), the OS (autostart units), the user (tray).

### Scope
In: server address setting, watcher hints, own 2 s settle, rescans, echo suppression, rename-over and directory-rename tracking, LocalIndex, mass-delete guard and root sentinel, first-sync merge, placeholder skipping, lock retry, include/exclude rules, pairs, safe local writes (remote deletes go to the OS trash), device login (RFC 8628 client), token storage, local IPC server, tray, autostart installers, update check with signature verification.
Out: HTTP framing, hashing loops, lanes, resume, long-poll mechanics, token refresh (13); settings screens and dialogs (15); files-on-demand placeholders (out of product scope); vault (v1.2, not on desktop in v1.0); sharing UI (web).

## 2. Internal Interfaces

Modules are private to the crate; only these traits cross a boundary (so tests can fake the OS and the network).

### Interface: DaemonControl (called by the IPC server and the tray)

| Method | Input | Output | Async | Error Types |
|--------|-------|--------|-------|-------------|
| `status` | none | `DaemonStatus { server_url, account, pairs[], skipped_count, escaped_count, holds[], update_available, access_denied, watcher_degraded }`; `access_denied` = macOS permission denial (AC-67, v1.1), `watcher_degraded` = a pair fell back to rescan-only; both are shown in the tray tooltip and in 15 | Yes | none |
| `set_server` | `url` | `()`; stored in `meta`, validated by `ServerUrl` and a `GET /api/v1/server-info` probe (so `UpgradeRequired` shows before sign-in); changing it while signed in is refused | Yes | `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}`, `SignedIn` |
| `list_server_folders` | `parent: Option<NodeId>` (none = account root) | `[FolderEntry { node_id, name }]` (folders only, paged) for the pair picker | Yes | `NotSignedIn`, `ServerUnreachable`, `UnknownFolder` |
| `list_pairs` / `add_pair` | `PairSpec { server_node_id` or `server_path` + `create_if_missing, local_root, rules_text }` (exactly one of node id and path) | `PairId` | Yes | `NotSignedIn`, `RootInvalid`, `RootNested`, `RulesInvalid{line}`, `CloudManagedRoot`, `AlreadyPaired`, `UnknownFolder` |
| `update_pair` / `remove_pair` / `pause` / `resume` | `PairId`, partial spec | `()` | Yes | `UnknownPair`, `RulesInvalid` |
| `list_skipped` / `list_escaped` | optional `PairId`, page | `[SkippedEntry { rel_path, reason }]` (placeholders, symlinks, special files) / `[EscapedEntry { original_name, saved_name, rel_path }]` | Yes | `UnknownPair` |
| `get_settings` / `set_settings` | `SettingsPatch` (bandwidth limit, hash throttle, autostart, tray on/off) | applied `Settings` | Yes | `SettingInvalid{key}` |
| `resolve_hold` | `PairId`, `HoldDecision::{ConfirmDeletes, RestoreFromServer}` | `()` | Yes | `NoSuchHold` |
| `login_start` | device name | `LoginPrompt { verification_url, user_code, expires_at }` (never a token) | Yes | `ServerNotSet`, `ServerUnreachable`, `HttpsRequired` |
| `login_status` / `logout` | none | `LoginState::{Pending, Approved, Denied, Expired, SignedOut}`; `logout` calls `SyncApi::sign_out` (`DELETE /api/v1/devices/me`), then deletes the token from `TokenStore` even if the server is unreachable (the result then says the server copy still needs revoking) | Yes | none |
| `subscribe` | none | stream of `StatusEvent` | Yes | none |

### Interface: LocalIndexStore (rusqlite, one writer thread)
`get_entry`, `upsert_entries(batch)`, `entries_in_state`, `find_by_file_id`, `find_by_node`, `set_pair_state`, `set_cursor` (one per account), `meta_get/set`, `rebuild`, and the `CheckpointStore` side: `upload_checkpoint(pair_id, rel_path, session_id, commit_id, file_hash, hash_state, blocklist)` saved at session start and after every `hash_checkpoint_blocks` blocks, read on restart, deleted on commit ack (a 5 GiB upload resumes without re-hashing and re-sends at most 10 MiB, AC-3), plus the batch records of 13 (`save_batch`, `load_batches`, `clear_batch`). Errors: `IndexCorrupt` (offer rebuild), `IndexNewerThanDaemon` (refuse, offer rebuild), `Io`.

### Interface: PlatformFs (per-OS implementations behind one trait)
`open_root(path) -> RootHandle`, `is_case_insensitive(root) -> bool` (probe at `add_pair`: create a temp file with a lower-case random name in the root, open it by the upper-cased name, delete it; result stored on the pair), `create_new_in(dir, name) -> TempFile`, `rename_over_in(dir, temp, name)`, `trash(dir, name)` (move to the OS trash or recycle bin), `stat_no_follow`, `file_id` (files and directories), `is_placeholder`, `open_for_read` (shared-read, reports `Locked`), `walk(dir)` (no symlink following). Errors: `Locked`, `Placeholder`, `EscapesRoot`, `NameNotRepresentable`, `TrashUnavailable`, `PermissionDenied`, `Io`.
Linux: `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` (fallback `openat` + `O_NOFOLLOW` per component); trash per the freedesktop trash spec. Windows: handle-relative `NtCreateFile` with reparse-point checks; trash through the shell recycle-bin API. macOS (v1.1): `openat` + `O_NOFOLLOW`, `SF_DATALESS` test, Finder trash.

### Interface: TokenStore
`load(account) -> Option<Token>`, `save`, `delete`. `KeyringTokenStore` (keyring 4.2.0 v1 mode, service `sunnycloud`, user = account id) is primary. `UserFileTokenStore` is used on Linux when no Secret Service answers: file 0600 in a 0700 directory under `$XDG_STATE_HOME`, one WARN line when first used. Errors: `StoreUnavailable`, `PermissionTooOpen` (refuses to read a group/world-readable file).

### Sync client contract (confirmed by 13: trait `SyncApi`, implemented by `HttpSyncClient`; section 2 of 13)
Same operations and typed results as 13, no HTTP types; tests fake the trait. Naming differences to read through: 13's `CommitOutcome::ConflictCopy` is what this document calls a conflict (the server picks the copy name, also for a new file that collides; the daemon never chooses one); `NameConflict`, `EditedSince`, `QuotaExceeded`, `SizeTooLarge`, `SessionCapReached`, `InvalidName` are typed errors of `upload`; `hash_file` and `upload` take a `FileSource` (a `PlatformFs` handle), not a path; `upload` takes the `CheckpointStore` (LocalIndex). Used here: `hash_file`, `upload`, `stage_small` + `batch_commit`, `list_children`, `root_node`, `download` (to a `DownloadSink` on a temp handle), `delete_node`, `create_folder`, `move_node` (no base revision), `poll_changes`, `head_cursor`, `device_grant_request`/`device_grant_poll`, `sign_out`. `lookup_revisions` and `server_has_hashes` are not used: content equality at first sync comes from the `content_hash` in server listings.

## 2a. Ports

`contracts` is server-only, so the client-side traits are defined in the client crate `sync-core` (13); across a process boundary the schema named in "Defined in" applies. Same format as 13.

| Direction | Port | Defined in | Counterpart |
|---|---|---|---|
| Provides | `DaemonControl` (JSON-RPC 2.0 over the local endpoint) | `ipc_schema` (14's own crate; process boundary) | 15_desktop_settings; the only surface 15 uses |
| Requires | `SyncApi` | `sync-core` | 13 (`HttpSyncClient`) |
| Provides | `CredentialProvider` (over `TokenStore`) | `sync-core` | consumed by 13 |
| Provides | `FileSource` (over `PlatformFs`) | `sync-core` | consumed by 13 |
| Provides | `DownloadSink` (verified blocks only) | `sync-core` | consumed by 13 |
| Provides | `CheckpointStore` (over `LocalIndexStore`: per-file `upload_checkpoint` rows and batch records) | `sync-core` | consumed by 13 |
| Provides | `ProgressSink` | `sync-core` | consumed by 13 |

## 3. External API
None served. The daemon is a client of the public API. It exposes only the local JSON-RPC endpoint (section 5).

## 4. Data Access Patterns

**Owned data**: `LocalIndex` (SQLite WAL, `synchronous=NORMAL`, local user data dir even if the root is on a share; schema in data model section 11): `meta` (server URL, account, the single feed cursor), `pair` (carries `case_insensitive` from the probe), `entry`, `upload_checkpoint`, and the 13 batch records. It is an index, never truth: lost index means rescan plus first-sync merge, nothing deleted. Forward-only migrations through `PRAGMA user_version`. No tokens in it.

| Query | Frequency | Hot | Index |
|-------|-----------|-----|-------|
| entry by `(pair_id, rel_path)` per watcher hint | High | Yes | PK |
| entries by `file_id` (rename-over) | Medium | Yes | `(pair_id, file_id)` |
| entries by `node_id` (apply feed row) | High | Yes | `(pair_id, node_id)` |
| non-synced entries | Low | No | partial `(pair_id, state)` |
| `upload_checkpoint` by `(pair_id, rel_path)` | per hashed block batch | No | PK |

Storage: 100,000 rows, about 11 MB file, 7 MB resident (measured on Linux). Growth with file count only.

## 5. Implementation Details

**State**: per-pair state machine `active | paused | hold_mass_delete | root_missing`; per-entry `synced | dirty | delete_held | conflict | skipped_placeholder | name_escaped`. Settings live in `meta` (the daemon owns them; the settings window is stateless).

**Pipeline (F9)**
1. *Watcher* (`notify` 8.2.1 `RecommendedWatcher`, recursive) emits paths only; no event kind is trusted. Overflow/missing events are expected: a `Scanner` safety rescan (walk + stat, about 0.3 s per 100k files warm) runs at start, every `desktop.rescan_interval`, after `Resumed` (suspend/wake) and on watcher error. If the OS watch limit is hit (Linux inotify), the pair falls back to rescan-only and reports it.
2. *RuleMatcher* (`ignore` crate gitignore grammar: `*`, `**`, `?`, leading `/`, trailing `/`, `!`; AC-66) filters before anything else. An excluded path is never uploaded, downloaded or deleted. Default excludes: `~$*`, `.~lock.*#`, `*.tmp`, editor swap files, the daemon's own temp prefix.
3. *Settler* keeps a dirty path until `(size, mtime)` is unchanged for `desktop.settle` (2 s, own check; debouncer-full does not do it). Locked/unreadable files (`Locked`) retry with exponential backoff from `desktop.lock_retry_initial` to `desktop.lock_retry_max`, using timers not spinning; they are never reported deleted.
4. *Decide from disk, not from event kind*: path exists and mapped in LocalIndex means update of that node, even when an editor replaced it by rename-over (new file ID); the upload carries the entry's base revision, so one new revision lands on the same file ID with no delete and no conflict copy (AC-34). A known `file_id` appearing at a new path while the old path is gone is a `move_node`. **Directory rename or move**: a directory whose `file_id` is known and whose old path is gone is one `move_node` of the directory node (children are not re-sent); where directory ids are not stable (some network and FAT filesystems), the fallback matches the vanished and the new directory by the multiset of `(size, mtime, hash)` of their files and moves only on a full match, otherwise it falls back to create plus delete, which the DeleteGuard counts. **Case-insensitive roots** (probe at `add_pair`): `rel_path` components are compared by `name_key` (06_helper_names), so a case-only change is a rename (`move_node` with the new name) and two paths differing only in case cannot coexist in the index. A path gone after settle is a delete candidate.
5. *Echo suppression*: before the atomic rename of a downloaded file, the entry is written `synced` with `(file_id, size, mtime_ns, content_hash)` of the temp file (mtime set from the revision first). A later hint matching that record is dropped (AC-18); a mismatch is a real change.
6. *DeleteGuard*: counts deletions per batch (descendants of a deleted directory counted individually) in both directions. Root missing, unmounted (sentinel file/id mismatch) or empty while the index is not, or `desktop.mass_delete_threshold` (200) or more deletes pending, moves the pair to a hold state; nothing is sent or applied; `hold_detail` persists across restarts. `RestoreFromServer` re-downloads; `ConfirmDeletes` sends deletes with base revisions. 199 pass without a prompt. New files keep uploading during a hold.
7. *ChangeApplier*: the `FeedLoop` applies `poll_changes` pages idempotently; the cursor (one per account) is saved only after applying. A row belongs to a pair when its `node_id` is an entry of that pair, or its `parent_id` is an entry (or the root node) of that pair; one row can touch several pairs and each applies it independently, so moving a node between pairs is a delete in one and a download in the other. `ResyncRequired` (410, or `invalid_cursor` mapped by 13) runs the merge below. **Dirty-path rule**: a feed row never overwrites, moves or deletes a path whose entry is `dirty`, `conflict`, `delete_held` or has an upload in flight (`upload_checkpoint` row). For such a path the local change goes first: the upload carries the entry's `base_revision`; the server then decides (conflict copy, or revives a deleted file when the edit beats the delete), the upload result updates the entry and only then is the pending feed row (re-read from the cursor) applied. A remote delete of a path that is not dirty moves the local file to the OS trash or recycle bin (`PlatformFs::trash`); if that fails (`TrashUnavailable`) the file stays, the entry is marked and reported, and nothing is unlinked. Downloads write to a temp file inside the root through `PlatformFs` directory handles, verify the file hash, then rename. Hostile or unrepresentable names (06_helper_names `portable_check`) are saved under a `%XX`-escaped name (`escape_name` of 06_helper_names), marked `name_escaped`, counted and reported with the original name (`list_escaped`); the daemon never writes outside the root and never overwrites an unrelated file. Symlinks, junctions, sockets and FIFOs are skipped and counted (`list_skipped`); hard links are independent files.
8. *FirstSyncMerger* (also used after restore, AC-41): enumerate both trees by `name_key` (listings carry `content_hash` for files); local-only uploads; server-only downloads; equal hash links in LocalIndex with 0 bytes sent (compared against the listing's `content_hash`, no extra call); different content: upload the local file as a new file at that name, the server answers with a conflict copy it names itself, then download the server version of the original; the upload is acknowledged before the local path is replaced (the dirty-path rule), and the copy returns through the feed; case-colliding local names on a case-sensitive filesystem lose neither file. Nothing is deleted. Interrupted merges resume from the index (NFT-RES-18).
9. *Placeholders*: `PlatformFs::is_placeholder` (Windows Cloud Files/OFFLINE/RECALL attributes, macOS dataless flag, v1.1) is tested before any open; such entries are `skipped_placeholder`, counted, never read, hydrated or deleted. Adding a root inside a detected cloud-managed folder is refused (`CloudManagedRoot`).
10. *Hashing budget*: initial hashing is throttled to `desktop.hash_cpu_share` (25% of one core while the user is active), runs at low priority, and the idle loop has no periodic work faster than the rescan timer (idle CPU at most 1%, AC-15).

**Device login**: `login_start` calls the grant through 13, opens the system browser, polls at the server interval and honours `slow_down`; on approval the token goes to `TokenStore` only. HTTPS is required unless an explicit developer flag is compiled in.

**Local IPC (AC-48)**: JSON-RPC 2.0 over `interprocess` local socket. Linux: socket mode 0600 inside a 0700 directory in `$XDG_RUNTIME_DIR`; macOS: own 0700 directory; Windows: named pipe with a security descriptor granting only the current user SID, remote clients rejected, `FILE_FLAG_FIRST_PIPE_INSTANCE` so a pre-created pipe makes the daemon refuse to start the endpoint and report it. The schema (`ipc_schema` crate, shared with 15) contains no method that returns a token, secret or credential; a test walks the schema. Requests apply immediately; the daemon confirms the applied value in the response (AC-16 5 s budget).

**Tray**: `tray-icon` 0.26.1 (ksni backend on Linux). Missing tray host (GNOME) or D-Bus failure is logged and non-fatal (AC-26). macOS needs the tray on the main thread (v1.1). Menu: status, pause/resume, open settings (spawns 15), open folder, pending holds, quit.

**Autostart (written directly, no `auto-launch` crate)**: Linux systemd user unit `WantedBy=default.target`; Windows Task Scheduler logon task for the current user, `InteractiveToken`, created from XML/COM (a service cannot own a tray); macOS LaunchAgent via `SMAppService` in a signed bundle (v1.1).

**Updater (AC-49)**: the update source (the project's GitHub Releases) and **two Ed25519 public keys** (active and recovery) are compiled in (01_helper_config release-fixed values); env and settings-file overrides are ignored. The manifest is signed offline (or in a manual job), never by an automatic CI secret. It carries `version`, per-OS entries with a SHA-256 of the payload, a monotonic `serial` and an `expires` time (exact layout and the signing tool: ADR). Flow: fetch the manifest from the fixed source, verify its signature against either pinned key, check `expires` against the clock, check `serial` is greater than the last accepted one (kept in `meta`) and `version` is strictly greater than the installed one, download the payload named for this OS, check its SHA-256 against the signed manifest, stage, apply on restart. Rejects: unsigned or unpinned-key manifest, expired manifest, serial not greater, lower or same version, payload whose digest does not match the signed manifest, truncated download. A manifest signed by the recovery key is accepted (key rotation); the app never fetches keys. Never contacts the sync server for updates.

**Key dependencies**: notify 8.2.1, ignore, rusqlite 0.41.1 (bundled), keyring 4.2.0, tray-icon 0.26.1, interprocess 2.4.7, tokio 1.54.1+, 13_sync_core. Toolchain at least Rust 1.90 (workspace pin).

**Error handling**: expected outcomes are typed (`UploadOutcome`, `ApplyOutcome`); network errors keep work queued in LocalIndex and back off; a server 4xx on a name marks the entry and reports it in the tray; no silent catch. Unexpected errors are logged with context and the pair worker restarts with backoff; the process does not exit for one pair.

## 6. Extensions and Helpers
01_helper_config (settings validation, durations), 02_helper_error (`ApiError` parsing), 03_helper_clock (`ManualClock` in tests; `Resumed` detection), 05_helper_ids_hashing, 06_helper_names (portability and escaping).

## 7. Caveats and Edge Cases
- notify 8.2.1 does not signal Windows buffer overflow: rescans are mandatory on every OS, not an optimisation.
- Four lanes are four HTTP/2 streams on one connection by default (13 decides).
- Network-share roots: no inotify events; the pair is rescan-only with a warning; the LocalIndex stays local.
- Sparse files above 10 GiB logical size warn; POSIX syncs only the executable bit.
- Antivirus stalls and exclusive locks look alike; both use the same backoff path.
- Race: a user edit during download of the same path: the post-rename hash/mtime mismatch makes the next hint a real change, uploaded with the downloaded revision as base (conflict copy if stale, never a silent overwrite). An edit that lands before the download starts is protected by the dirty-path rule.

## 8. Dependency Graph
Must be implemented after: 13_sync_core (SyncApi), helpers 01-03, 05, 06. Can run in parallel with: 15 (once the `ipc_schema` crate exists), 16, 17. Blocks: 15 (runtime), packaging/signing tasks.

## 9. Logging
Structured JSON to a rotating file in the user state dir (and stderr when attached). ERROR: index corruption, endpoint refused, unrecoverable pair failure. WARN: keychain fallback used (once), watch limit reached, name escaped, placeholder skipped, hold entered. INFO: pair added/removed, login approved, update staged. No paths of user files above DEBUG, never tokens or user codes.

## 10. Configuration keys (all validated by 01_helper_config)
`desktop.settle` 2s; `desktop.rescan_interval` 15m; `desktop.wake_gap` 60s; `desktop.lock_retry_initial` 500ms; `desktop.lock_retry_max` 4s; `desktop.mass_delete_threshold` 200; `desktop.hash_cpu_share` 0.25; `desktop.longpoll_wait` 30s (server-capped); `desktop.update_check_interval` 24h; `desktop.sparse_warn_bytes` 10GiB; `desktop.macos_permission_probe` 10s (v1.1). Update URL and key are build-time constants, not keys. Environment names follow helper 01: `SUNNYCLOUD_` + the key upper-cased with `.` replaced by `_` (`desktop.settle` is `SUNNYCLOUD_DESKTOP_SETTLE`).

## 11. Security notes
Token only in the keychain or the 0600 file; never in IPC, logs, process arguments or environment (NFT-SEC-34). Downloads are hash-verified before rename. Device token is sync-only on the server (cannot approve devices or create shares); `logout` revokes it with `DELETE /api/v1/devices/me`. LocalIndex holds no secrets. IPC endpoint rules above. Updater rules above (offline-signed manifest, two pinned keys, expiry, serial).

## 12. Acceptance criteria implemented
AC-15, AC-18, AC-26, AC-29, AC-31, AC-32, AC-33, AC-34, AC-35, AC-37 (client half), AC-48, AC-49, AC-66 [v1.0]; AC-67 [v1.1]. Shared with 13 and the server: AC-3, AC-5, AC-7, AC-8, AC-41 (hash-verified transfer is in 13; the daemon never renames an unverified download into place).
**Interaction ACs folded in**: none are owned by this component in `interaction-risks.md`. It participates in AC-70 (folder move while another device edits inside it): the daemon applies a `move` feed event as a rename of the directory handle's entry and keeps node IDs from the feed, so an edit inside the moved folder is uploaded under the same node. It participates in AC-41/AC-69 by applying feed rows idempotently and saving the cursor only after applying.

## 13. Test scenarios
FT-P-07, FT-P-08, FT-P-10, FT-P-12, FT-P-51 to FT-P-61, FT-P-77 (feed row against a dirty path), FT-P-75 (v1.1), FT-N-05, FT-N-17, FT-N-18, FT-N-19, NFT-PERF-24, NFT-PERF-26, NFT-RES-11, NFT-RES-12, NFT-RES-15 to NFT-RES-18, NFT-RES-25, NFT-RES-LIM-06, NFT-RES-LIM-07, NFT-SEC-31 to NFT-SEC-34.

## 14. Component acceptance
- 40-case hostile corpus per OS: 0 writes outside root, 0 overwrites.
- 100,000 files: first scan at most 60 s, idle at most 1% CPU, at most 100 MB resident.
- 199 deletes propagate, 200 are held in both directions; hold survives restart.
- Save-in-place by five editors: one revision, same file ID, 0 conflict copies.
- IPC schema contains 0 token-returning methods; other-user connect refused.
- Updater rejects 8 of 8 bad builds (unsigned, unpinned key, expired, replayed serial, lower or same version, altered payload, truncated); a recovery-key manifest is accepted.

## 15. Risks
0. Default chosen at Step 5 gap pass: `DaemonStatus` gains `access_denied` (macOS permission denial, AC-67, v1.1) and `watcher_degraded` (rescan-only fallback), surfaced in the tray tooltip and settings.
1. Windows/macOS CPU and memory at 100k files unmeasured (Verify, gate S2).
2. Linux headless token fallback is a file (owner-confirmed by restrictions); durable keyring behaviour in systemd user context unverified.
3. Standard-user logon-task registration without a prompt unverified on Windows.
4. Resolved 2026-11-11 (AZ-3887, owner confirmed the layout): update manifest layout, serial storage in `meta`, the offline signing tool (`update-sign`, built by E-24/c18) and the release runbook step are in ADR-017 Notes. Decided earlier: offline signing, two pinned Ed25519 keys (active, recovery), `version`, per-OS SHA-256, `serial`, `expires`.
5. Exact `SyncApi` surface depends on 13; mismatch is the largest integration risk.
