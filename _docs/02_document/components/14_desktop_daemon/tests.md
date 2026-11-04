# Test Specification — 14 Desktop Daemon

Component tests complement the system-level scenarios (`02_document/tests/`) and reference them by ID. Tiers: **Unit** = daemon modules with a fake `SyncApi` (client trait boundary), fake `PlatformFs`, `ManualClock` and a config mock with short durations (`desktop.settle` 400 ms, `desktop.lock_retry_initial` 50 ms, `desktop.lock_retry_max` 200 ms; production durations are never slept); **IT** = real daemon binary against the real server container with real PostgreSQL on a real file system per OS (ext4 and XFS in `desktop-linux-a/-b`, NTFS in WIN-VM; env `C`, `H-LIN`, `H-WIN`). Only `DaemonControl`, `LocalIndexStore`, `PlatformFs`, `TokenStore` and the 13 host traits are used.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|-----------------------------|----------|----------|
| AC-3 | Resume re-sends <= 10 MiB (checkpoint rows) | IT-14-19 | Covered; system FT-P-01, NFT-RES-01 |
| AC-5 | Device B holds the verified file within 5 s | IT-14-3 | Covered; system FT-P-07, NFT-PERF-08 (C-REF) |
| AC-7 | Never rename an unverified download | ST-14-4 | Covered; system FT-N-02 |
| AC-8 | Stale base is a conflict copy; edit beats delete | IT-14-3, IT-14-8 | Covered; system FT-P-10, FT-P-12, FT-P-77 |
| AC-15 | 100,000 files: idle <= 1% CPU, <= 100 MB, scan <= 60 s, hashing <= 25% | PT-14-1, PT-14-2 | Covered; system NFT-PERF-24, NFT-RES-LIM-06 |
| AC-16 | Changed setting takes effect within 5 s | IT-14-15, PT-14-4 | Covered (daemon half); system FT-P-49, NFT-PERF-26 |
| AC-18 | No echo loop; upload after 2 s of stillness | IT-14-1, IT-14-2 | Covered; system FT-P-51, FT-P-52 |
| AC-21 | Revoked device token stops sync | IT-14-21 | Covered; system NFT-SEC-08, FT-N-30 |
| AC-26 | Syncs with no tray; Windows logon task | IT-14-20 | Covered; system FT-P-53 (H-LIN, H-WIN) |
| AC-29 | Missing/empty root or 200 deletions pause deletes | IT-14-5, AT-14-3 | Covered; system FT-N-17, FT-N-18, FT-P-59, NFT-RES-11, NFT-RES-12 |
| AC-31 | 1,000 files, 200 collide: 0 lost, 100 copies | IT-14-11, AT-14-2 | Covered; system FT-P-54, NFT-RES-18 |
| AC-32 | Evicted placeholders: 0 hydration, 100 skipped | IT-14-9 | Covered; system FT-P-55, NFT-RES-17 (H-WIN) |
| AC-33 | Locked file retried without spin, uploaded <= 10 s | IT-14-10, PT-14-3 | Covered; system FT-P-56, NFT-RES-15 (H-WIN) |
| AC-34 | Save-in-place: 1 revision, same ID | IT-14-8, AT-14-1 | Covered; system FT-P-57 |
| AC-35 | Convergence within 60 s after 30-minute suspend | IT-14-12, AT-14-3 | Covered; system FT-P-58, NFT-RES-16 |
| AC-37 | Hostile names: 0 writes outside root, 0 overwrites | IT-14-14, ST-14-1, ST-14-5 | Covered (ST-14-5 second-collision rule BLOCKED); system FT-N-19, NFT-SEC-31 |
| AC-41 | Resync after restore; local-only up, server-only down | IT-14-13 | Covered; system FT-P-13, FT-P-14, NFT-RES-07 |
| AC-42 | Case-insensitive roots; case-only change is a rename | IT-14-7 | Covered; system FT-P-15, FT-N-05 |
| AC-48 | IPC reachable only by own user; no token in schema | IT-14-16, IT-14-18, ST-14-2 | Covered; system NFT-SEC-32, NFT-SEC-34 |
| AC-49 | Updater rejects bad builds; fixed source | IT-14-17, ST-14-3 | Covered; system NFT-SEC-33 |
| AC-54 | HTTPS required for server address and login | IT-14-15, IT-14-23 | Covered; system NFT-SEC-12 |
| AC-66 | Include/exclude rules; excluded path never touched | IT-14-4 | Covered; system FT-P-60 |
| AC-67 | macOS access denied reported [v1.1] | IT-14-24 | BLOCKED — `DaemonStatus` has no field for it (see IT-14-24); system FT-P-75 |
| AC-69 | Feed rows applied idempotently, cursor saved after apply | IT-14-6, IT-14-13 | Covered; system NFT-RES-35 |
| AC-70 | Directory move while another device edits inside | IT-14-22 | Covered; system FT-N-42 |

## Blackbox Tests

### IT-14-1: Settler waits for stillness
- **Summary**: A file still being written uploads only after `desktop.settle` of unchanged `(size, mtime)`.
- **Traces to**: AC-18
- **Input**: unit with `desktop.settle` 400 ms: hints for `a.bin` at 0, 100, 200, 300 ms with growing size, then silence. IT: DS-27 slow writer (2 GiB in 16 MiB steps, 1 s pauses) with the production 2 s.
- **Expected**: unit: one `upload` at >= 700 ms after the first hint; IT: first upload byte at the proxy >= last write + 2.0 s, 0 uploads before the writer ends.
- **Max time / Deps**: unit 2 s, IT 5 min; fake `SyncApi` / server.

### IT-14-2: Echo suppression
- **Summary**: A downloaded file is recorded `synced` before the atomic rename so its hint is dropped; a real edit is still uploaded.
- **Traces to**: AC-18
- **Input**: remote change of a 1 MiB file (DS-23) downloaded by daemon A; then (a) nothing; (b) user rewrites it with different bytes, same size, mtime reset by `utimensat` to the stored value.
- **Expected**: (a) 0 upload requests for the path (proxy log); (b) hash mismatch against the stored `(file_id, size, mtime_ns, content_hash)` -> uploaded with the downloaded revision as `base_revision`; entry is `synced` before `rename_over_in` (spy order).
- **Max time / Deps**: 30 s; server, spy `PlatformFs`.

### IT-14-3: ChangeApplier dirty-path rule, remote delete to OS trash
- **Summary**: A feed row never overwrites, moves or deletes a dirty, conflict, delete-held or in-flight path; a clean remote delete goes to the trash.
- **Traces to**: AC-5, AC-8
- **Input**: entries p1 dirty, p2 conflict, p3 delete_held, p4 upload in flight (`upload_checkpoint` row), p5 clean; feed updates for p1 to p4, deletes for p5 and p1; a second p5 delete with `trash` failing (`TrashUnavailable`).
- **Expected**: p1 to p4 untouched on disk; the upload carries the entry's `base_revision` and only after its result is stored is the pending row re-read from the cursor and applied; p5 appears in the freedesktop trash (info file plus data) with 0 unlink calls; on `TrashUnavailable` the file stays, the entry is marked and reported, 0 unlink calls; the cursor does not pass a pending row until its upload result is stored.
- **Max time / Deps**: 20 s; LocalIndex, fake `SyncApi`; Windows recycle bin on WIN-VM (H-WIN).

### IT-14-4: Include and exclude rules
- **Summary**: `RuleMatcher` follows the gitignore grammar; an excluded path is never uploaded, downloaded or deleted.
- **Traces to**: AC-66
- **Input**: rules `*.log`, `build/`, `/root-only.txt`, `!keep.log`, `docs/**/draft?.md`; paths a.log, keep.log, build/x, src/build/x, root-only.txt, sub/root-only.txt, docs/a/b/draft1.md, docs/draft12.md; invalid rule text with the bad line 3.
- **Expected**: ignored: a.log, build/x, src/build/x, root-only.txt, docs/a/b/draft1.md; kept: keep.log, sub/root-only.txt, docs/draft12.md; excluded server file: 0 downloads; excluded local file removed on server: 0 deletes; default excludes (`~$*`, `.~lock.*#`, `*.tmp`, swap files, daemon temp prefix) hold; `add_pair` -> `RulesInvalid{line: 3}`.
- **Max time / Deps**: 10 s; fake `SyncApi`. UI half: 15 AT-15-4.

### IT-14-5: DeleteGuard and root sentinel
- **Summary**: 199 deletes pass; 200, or a missing/unmounted/empty root, hold the pair and nothing is sent until the user decides.
- **Traces to**: AC-29
- **Input**: production `desktop.mass_delete_threshold` 200. Cases: 199 local deletes; 200 local (a directory with 200 descendants counts individually); 200 remote; root renamed; sentinel id mismatch (volume swapped); root empty while index is not; restart; then `resolve_hold`.
- **Expected**: 199 propagate, no hold; 200 either direction -> `hold_mass_delete`, 0 `delete_node` calls, 0 local deletions, `hold_detail` survives restart; root cases -> `root_missing`, 0 deletes; new files still upload during a hold; `ConfirmDeletes` -> `delete_node` with base revisions; `RestoreFromServer` -> all 200 re-downloaded, hashes equal; no hold -> `NoSuchHold`.
- **Max time / Deps**: 2 min; server, `e2e-ctl` unmount (Linux), VHD dismount (H-WIN).

### IT-14-6: One feed loop per account, cursor saved after apply
- **Summary**: Two pairs share one long-poll and one cursor; a kill between apply and save re-applies idempotently.
- **Traces to**: AC-69
- **Input**: account with pairs P1, P2; 500 feed rows, 40 touching both pairs (moves between pairs); kill after page 3 is applied and before `set_cursor`.
- **Expected**: at most 1 `poll_changes` in flight; `meta` holds 1 cursor row for the account; after restart page 3 is re-applied with no duplicate file, no extra upload, identical final trees; a node moved between pairs is a delete in one and a download in the other.
- **Max time / Deps**: 60 s; fake `SyncApi`, server; system NFT-RES-35.

### IT-14-7: Case-insensitive roots and directory rename
- **Summary**: The `is_case_insensitive` probe drives `name_key` comparison; a directory rename is one `move_node`.
- **Traces to**: AC-42
- **Input**: roots on ext4 and on a case-folding mount (ext4 casefold, NTFS in WIN-VM). Rename `Report.txt` to `report.txt`; local `A.txt` and `a.txt` on the case-sensitive root; rename directory `d` (2,000 files) to `e`; directory ids unstable (fake `file_id` none) with identical content and with partly changed content.
- **Expected**: probe result stored on the pair; case-insensitive: `move_node` with the new name, same node id, no second entry; case-sensitive: neither file lost, server resolves one with a conflict copy (FT-N-05); stable ids: 1 `move_node`, 0 uploads, 0 deletes; unstable ids: full match of `(size, mtime, hash)` multiset -> 1 `move_node`; partial -> create plus delete counted by DeleteGuard (2,000 deletes hold the pair).
- **Max time / Deps**: 60 s; real file systems per OS; spy `SyncApi`.

### IT-14-8: Save-in-place from editors
- **Summary**: Rename-over, delete-and-recreate and backup-swap saves produce one revision on the same file ID.
- **Traces to**: AC-34, AC-8
- **Input**: DS-15 scripts (vim `-es`, `keepassxc-cli`, LibreOffice UNO, JetBrains replay) on Linux; Word and others on WIN-VM; an existing share link.
- **Expected**: per editor: 1 new revision, same file ID, 0 conflict copies, 0 `delete_node`, share link opens; decided from disk, not event kind.
- **Max time / Deps**: 20 s per editor; server; LibreOffice, JetBrains, Word are H-LIN/H-WIN.

### IT-14-9: Placeholders and cloud-managed roots
- **Summary**: Placeholder entries are skipped, never opened, hydrated or deleted.
- **Traces to**: AC-32
- **Input**: unit with fake `is_placeholder`; IT (H-WIN) DS-14 (400 hydrated, 100 dehydrated).
- **Expected**: 100 `skipped_placeholder` entries in `list_skipped` with reason; 0 `open_for_read` on them; Cloud Files callback log 0 hydrated bytes; 0 delete requests; `add_pair` inside a cloud-managed folder -> `CloudManagedRoot`.
- **Max time / Deps**: 60 s; cfapi test provider (H-WIN).

### IT-14-10: Locked file retry
- **Summary**: `Locked` retries use backoff timers; the file is never reported deleted.
- **Traces to**: AC-33
- **Input**: unit: `open_for_read` returns `Locked` 5 times then succeeds (50 to 200 ms config mock). IT (H-WIN): DS-28 exclusive lock for 60 s. Linux locks are advisory: "not applicable", not pass.
- **Expected**: unit: gaps 50, 100, 200, 200, 200 ms (+/- 20%) from timers, 0 `delete_node`; IT: CPU <= 1%, upload <= 10 s after release, entry never `delete_held`.
- **Max time / Deps**: unit 3 s, IT 120 s; `ManualClock`; WIN-VM.

### IT-14-11: First-sync merge (also after restore)
- **Summary**: Connecting a folder with collisions loses nothing and re-uploads no identical bytes.
- **Traces to**: AC-31
- **Input**: DS-13 (1,000 local files; server has 100 identical and 100 different paths); daemon killed at 25%, 50%, 75%.
- **Expected**: 0 lost; 0 bytes uploaded for the 100 identical (proxy log); exactly 100 server-named conflict copies; the local original replaced only after the upload ack; nothing deleted; each kill resumes from the index to the same end state; case-colliding local names on a case-sensitive root lose neither file.
- **Max time / Deps**: 10 min; server, `e2e-ctl` kill.

### IT-14-12: Suspend, wake and rescans
- **Summary**: A clock gap above `desktop.wake_gap` (`Resumed`) triggers a safety rescan and convergence.
- **Traces to**: AC-35
- **Input**: unit: `ManualClock` jumps 30 min; a watcher error; `desktop.rescan_interval` elapsed. IT: DS-23 (500 files changed locally and on device B) across a real 30-minute guest suspend.
- **Expected**: one rescan per trigger (start, interval, `Resumed`, watcher error); devices equal within 60 s after wake; inotify limit hit -> pair falls back to rescan-only.
- **Max time / Deps**: unit 5 s, IT 40 min; LIN-VM `dompmsuspend`, Windows sleep (a paused VM is weaker evidence and is labelled).

### IT-14-13: Feed resync order
- **Summary**: `ResyncRequired` runs head cursor first, then the merge; the cursor moves only after the merge.
- **Traces to**: AC-41, AC-69
- **Input**: DB restored from an older backup (`e2e-ctl`); a device offline 91 days (test clock); fake variants with 410 and 422.
- **Expected**: request order `head_cursor` then `list_children` pages; local-only files upload, server-only download, deletions only via the guard; convergence within 60 s.
- **Max time / Deps**: 3 min; server, test clock (group `X`).

### IT-14-14: Hostile and unrepresentable names
- **Summary**: The 40-case corpus per OS causes no write outside the root and no overwrite.
- **Traces to**: AC-37
- **Input**: DS-12 for the running OS, created on the server through the API.
- **Expected**: 0 files outside the root (tree diff of the parent); sentinel file hashes unchanged; unrepresentable names saved `%XX`-escaped, state `name_escaped`, `list_escaped` returns `{original_name, saved_name, rel_path}`; symlinks, junctions, sockets, FIFOs appear in `list_skipped`; hard links are independent files.
- **Max time / Deps**: 5 min; server; reserved names, ADS, junctions on WIN-VM (H-WIN).

### IT-14-15: DaemonControl call matrix
- **Summary**: Every method returns its documented output or typed error.
- **Traces to**: AC-16, AC-54
- **Input**: JSON-RPC calls: `set_server("http://x")`; unreachable host; server with `min_client_version` above the client; `set_server` while signed in; `list_server_folders` with None, valid, unknown, signed out; `add_pair` with both node id and path, nested root, unwritable root, same server folder twice, unknown node, `create_if_missing` path; `update_pair`/`remove_pair`/`pause`/`resume` with an unknown id; `set_settings` with bandwidth -1; `login_start` without server; `resolve_hold` with no hold; `logout` with the server down; `subscribe` while pausing.
- **Expected**: `HttpsRequired`; `ServerUnreachable`; `UpgradeRequired{message}`; `SignedIn`; folder list / `UnknownFolder` / `NotSignedIn`; invalid spec, `RootNested`, `RootInvalid`, `AlreadyPaired`, `UnknownFolder`; `UnknownPair`; `SettingInvalid{key}`; `ServerNotSet`; `NoSuchHold`; `login_start` response never contains a token; `logout` deletes the local token and says the server copy still needs revoking; `set_settings` echoes the applied value and a bandwidth cap is visible on a transfer within 1 s; `status` has `server_url, account, pairs[], skipped_count, escaped_count, holds[], update_available`; `StatusEvent` arrives on pause and resume.
- **Max time / Deps**: 30 s; server for success paths.

### IT-14-16: IPC endpoint ownership
- **Summary**: Only the daemon's user can connect.
- **Traces to**: AC-48
- **Input**: second OS user (Linux container; Windows second user and a remote client in WIN-VM); a pre-created pipe or socket at the endpoint path.
- **Expected**: Linux socket mode 0600 in a 0700 dir under `$XDG_RUNTIME_DIR`, other user denied; Windows pipe grants only the current SID, remote client refused; pre-created pipe -> the daemon refuses to start the endpoint and reports it (`FILE_FLAG_FIRST_PIPE_INSTANCE`) while sync continues.
- **Max time / Deps**: 60 s; WIN-VM for pipes; system NFT-SEC-32.

### IT-14-17: Updater verification matrix
- **Summary**: 8 of 8 bad builds are rejected, good ones accepted.
- **Traces to**: AC-49
- **Input**: black-box against `mock-update-server` with DS-31 builds (the description names no updater interface; observables are `DaemonStatus.update_available`, the staged payload and the serial in `meta`). Good: active key, v1.0.1, serial 7, expires +30 d, matching SHA-256; recovery-key manifest. Bad: unsigned; unpinned key; expired (clock past `expires`); replayed serial after 7 accepted; same version; lower version; altered payload (digest mismatch); truncated payload. Also `SUNNYCLOUD_*` and settings-file overrides of source and keys.
- **Expected**: good: `update_available` true, payload staged, serial stored; each bad build: not staged, no restart, WARN names the failed check; only the fixed update host is contacted (0 requests to the sync server); overrides change nothing.
- **Max time / Deps**: 2 min; `mock-update-server`, test-key daemon build.

### IT-14-18: Token store
- **Summary**: The token lives only in the keychain or a 0600 file.
- **Traces to**: AC-48
- **Input**: Linux with and without a Secret Service; a pre-created 0644 token file.
- **Expected**: keyring entry `sunnycloud/<account id>` when available; else `UserFileTokenStore` file 0600 in a 0700 dir under `$XDG_STATE_HOME` with one WARN; the 0644 file -> `PermissionTooOpen`, not read; token string absent from IPC traffic, logs, `/proc/<pid>/cmdline`, `/proc/<pid>/environ`.
- **Max time / Deps**: 30 s; system FT-P-61, NFT-SEC-34.

### IT-14-19: Upload checkpoints and LocalIndex errors
- **Summary**: `upload_checkpoint` rows make a 5 GiB upload resumable; index damage is reported.
- **Traces to**: AC-3
- **Input**: DS-09 killed at 40% (`sync.hash_checkpoint_blocks` 32); then a corrupted SQLite file; then `user_version` above the daemon's.
- **Expected**: restart re-sends <= 10 MiB and hashing CPU after restart < 5% of the first run; corruption -> `IndexCorrupt`, rebuild offered (rescan plus first-sync merge, nothing deleted); newer version -> `IndexNewerThanDaemon`, refuse and offer rebuild.
- **Max time / Deps**: 10 min; server, `e2e-ctl` kill.

### IT-14-20: Tray and autostart
- **Summary**: The daemon syncs without a tray host and installs a correct autostart entry.
- **Traces to**: AC-26
- **Input**: LIN-VM stock GNOME (no AppIndicator); WIN-VM standard user.
- **Expected**: tray failure logged, process alive, a new file reaches the server within 10 s; user unit has `WantedBy=default.target`; Windows logon task for the current user, `InteractiveToken`, registered without a prompt; autostart off removes unit/task.
- **Max time / Deps**: 10 min; H-LIN, H-WIN.

### IT-14-21: Revoked device token
- **Summary**: Revocation stops sync and shows signed-out.
- **Traces to**: AC-21
- **Input**: device revoked via the API while the feed loop is held; separately the identity provider made unreachable.
- **Expected**: loop ends within 5 s (`Unauthorized` from 13), `login_status` is `SignedOut`, uploads stop, pairs and index kept; provider down alone does not interrupt sync.
- **Max time / Deps**: 30 s; server.

### IT-14-22: Folder move while another device edits inside
- **Summary**: Node ids come from the feed so the edit uploads under the same node.
- **Traces to**: AC-70
- **Input**: device A moves `d` to `x/d`; device B (daemon offline until after the move) edits `d/f.txt`.
- **Expected**: B keeps `node_id` for `f.txt`, uploads with its base revision; final `x/d/f.txt` has the edit, same file ID, 0 conflict copies. System FT-N-42.
- **Max time / Deps**: 30 s; server, two daemons.

### IT-14-23: Device login
- **Summary**: The RFC 8628 client honours the interval and `slow_down`.
- **Traces to**: AC-54
- **Input**: fake grant server: interval 5 s (config mock), one `slow_down`, then approval; also Denied and Expired; `http://` URL.
- **Expected**: poll gaps >= interval, +5 s after `slow_down`; `login_start` returns `{verification_url, user_code, expires_at}` only; approval stores the token via `TokenStore`; `HttpsRequired` for `http://`.
- **Max time / Deps**: 20 s; fake grant server.

### IT-14-24: macOS access denied [v1.1]
- **Summary**: Denial is reported within 10 s and the folder resumes within 10 s after the grant.
- **Traces to**: AC-67
- **Expected**: BLOCKED — per-pair states (`active | paused | hold_mass_delete | root_missing`) and `DaemonStatus` have no value for a TCC denial and `desktop.macos_permission_probe` has no named reporting field. Needed: a pair state or hold kind for access denied in `DaemonStatus` so 15 can show it. System scenario FT-P-75 stays H-MAC.
- **Max time / Deps**: n/a; MAC, v1.1.

## Performance Tests

### PT-14-1: 100,000 files, idle and first scan
- **Summary**: Idle CPU, memory and scan time with DS-11.
- **Traces to**: AC-15
- **Load**: 1 daemon, 100,000 files (1 to 16 KiB, depth 6), `fs.inotify.max_user_watches` >= 524288; cold start, scan, then 10 min idle.
- **Targets / failure**: first scan <= 30 s / above 60 s; idle CPU <= 0.5% / above 1%; resident <= 80 MB / above 100 MB. Settings window excluded. Env C-REF (Linux), H-WIN; system NFT-PERF-24, NFT-RES-LIM-06.

### PT-14-2: Hashing throttle
- **Summary**: Initial hashing uses at most 25% of one core while the user is active.
- **Traces to**: AC-15
- **Load**: 20 GiB of small files hashed while a UI-activity simulator runs; sample daemon thread CPU.
- **Targets / failure**: <= 25% of one core / above 30%; low process priority / normal.

### PT-14-3: Locked-file CPU
- **Summary**: A file locked for 60 s costs at most 1% CPU.
- **Traces to**: AC-33
- **Load**: DS-28 on WIN-VM, 60 s lock.
- **Targets / failure**: CPU <= 1% / above 1%; retries bounded by backoff to `lock_retry_max`. System NFT-RES-LIM-07.

### PT-14-4: IPC apply latency
- **Summary**: `set_settings` round trip and time to effect.
- **Traces to**: AC-16
- **Load**: 100 sequential `set_settings` while a transfer runs.
- **Targets / failure**: response p95 <= 200 ms / above 1 s; value effective <= 1 s / above 5 s. System NFT-PERF-26.

## Security Tests

### ST-14-1: Traversal and symlink races
- **Summary**: Directory-handle writes cannot be redirected.
- **Traces to**: AC-37
- **Attack vector**: server name `../x`; a subdirectory swapped for a symlink to `/tmp/outside` between check and write; symlink inside the path; Windows junction and ADS names.
- **Procedure**: create the names via the API; run a swapper thread 10,000 times during downloads; diff the parent directory.
- **Pass**: 0 files outside the root; `EscapesRoot` reported (`openat2` `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`, `NtCreateFile` handle-relative). **Fail**: 1 or more.

### ST-14-2: IPC schema and secrets
- **Summary**: No IPC method can return a token.
- **Traces to**: AC-48
- **Attack vector**: schema inspection; calling every method on a signed-in daemon.
- **Procedure**: walk `ipc_schema` types, reject any field typed `Token`, `Secret`, `Credential` or named `token`, `secret`, `password`; record all responses and `subscribe` events and grep for the token value.
- **Pass**: 0 matching types, token absent. **Fail**: any hit.

### ST-14-3: Updater downgrade and key confusion
- **Summary**: Replay, downgrade and wrong-key attacks fail.
- **Traces to**: AC-49
- **Attack vector**: replayed older valid manifest; recovery-key manifest with a lower version; a key listed in the manifest itself; manifest URL redirected to another host.
- **Procedure**: serve each from `mock-update-server`; inspect staged files and `meta`.
- **Pass**: 0 staged payloads, serial unchanged, keys never fetched, redirect not followed. **Fail**: any staged payload.

### ST-14-4: Unverified download never lands
- **Summary**: A hash mismatch never replaces a local file.
- **Traces to**: AC-7
- **Attack vector**: flipped block or final hash; kill between temp write and rename.
- **Procedure**: fake `SyncApi.download` returns `HashMismatch`; second case kills after the temp write.
- **Pass**: destination hash unchanged, entry not `synced`, temp file removed (stale ones with the daemon prefix cleaned at start). **Fail**: a renamed file with a wrong hash.

### ST-14-5: Escape names cannot collide
- **Summary**: `%XX` escaping never overwrites a real file named like the escaped form.
- **Traces to**: AC-37
- **Attack vector**: the server holds `a:b` and a real `a%3Ab`.
- **Procedure**: sync both to Linux and Windows roots.
- **Pass**: both files survive and the escaped one is in `list_escaped`. **Fail**: one overwritten. BLOCKED for the Windows reserved-name marker (`%5F`) and the ` (2)` suffix: open owner items in `canonical-names.md`.

## Acceptance Tests

### AT-14-1: Save-in-place in five editors
- **Summary**: Editing does not create copies.
- **Traces to**: AC-34
- **Preconditions**: paired folder, one file with a share link.
- **Steps**: (1) save from vim, JetBrains, Word, LibreOffice, KeePassXC in turn: each syncs within 10 s; (2) server shows 1 new revision per save, same file ID, 0 conflict copies, 0 deletes; (3) the share link still opens.

### AT-14-2: Connect a folder with collisions
- **Summary**: First sync is safe.
- **Traces to**: AC-31
- **Preconditions**: DS-13 on disk, server pre-seeded.
- **Steps**: (1) `add_pair` with local root and server folder; (2) wait for idle: 1,000 local files present, 100 conflict copies, 0 identical bytes uploaded.

### AT-14-3: Mass delete held; suspend and wake
- **Summary**: Destructive mistakes are stopped and devices converge after sleep.
- **Traces to**: AC-29, AC-35
- **Preconditions**: 300 synced files (DS-23), two devices.
- **Steps**: (1) delete 200 files locally: hold in `status.holds[]`, server unchanged; (2) `resolve_hold(RestoreFromServer)`: 200 files return, hashes equal; (3) suspend 30 min with 500 changes on device B: all devices converge within 60 s after wake.

## Test Data Management

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-11 | 100,000 small files | `gen_tree.py` | 0.5 GB |
| DS-12 | 40 hostile names per OS | `gen_names.py` | < 1 MB |
| DS-13, 14, 15, 23, 27, 28 | collisions, evicted, editors, boundary files, slow writer, lock script | test-data.md generators | < 5 GB |
| DS-31 | update builds (8 bad, 2 good) | `tests/mocks/update` | 200 MB |
| DS-09 | 5 GiB for checkpoints | `gen_bigfile.py` | 5 GiB |

**Setup**: `tests/run.sh --profile desktop` for Linux containers; the lab controller reverts WIN-VM/LIN-VM snapshots for `H-*`; one account per test via invite token; the daemon signs in with the device grant against `mock-idp`.
**Teardown**: stop the daemon, delete its state dir and sync root, purge the account through `e2e-ctl`; restore VM snapshots after suspend, unmount and lock tests.
**Isolation**: one account, sync root and LocalIndex per test; fake `SyncApi`/`PlatformFs` only in unit tests at the trait boundary; integration tests use the real server container with real PostgreSQL and a real file system per OS; suspend, unmount, test-clock and kill tests are group `X`.
