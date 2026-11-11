# Spec gaps index (Step 5 gap pass)

Consolidated from the `Spec gaps found` / `Gaps` lists and BLOCKED markers in `components/*/tests.md` and `tests/*.md`. Component descriptions were patched for the items in section 1 (each marked "default chosen at Step 5 gap pass" in that file's risks section); tests.md files were not edited, so they still show those items as gaps until the test writers rerun. Gap IDs are the tests' own; where a tests.md references a gap id without defining it (G-03-3, G-05-2, G-05-3, G-06-2, G-06-3) the text is taken from the referencing test.

Resolve during: **decompose** = settle in the task spec/contract; **implement** = settle while building (test infrastructure, benchmarks, test data); **owner** = owner decision or provision needed first.

## 1. Resolved at the gap pass

| Component | Resolution | Tests unblocked/affected | Fix |
|---|---|---|---|
| 02_tree | `resolve_live_file`/node lookups: file under a tombstoned ancestor is Gone at once (parent-chain walk, bounded by depth), independent of the 30 s cascade | G-06-1; IT-06-6, IT-06-7, AT-06-3; AC-12, AC-77 (FT-N-44) | 1 |
| 02_tree | `create_folder`, `update_node`, `delete_node` call `AccountGuard::ensure_active` | IT-02-10 | 1 |
| 02_tree | Owner of the `NewRevision.blob_id` account check: 02 `publish` via 04 `BlobCatalog`/`BlobRef`; existing errors `BlocklistMismatch`/`NotFound` | ST-02-4 | 1 |
| 12_operator_cli | G-12-2: `--at` RFC 3339 UTC; `--backup-interval` duration such as `24h` | IT-12-4, IT-12-15 | 2 |
| 12_operator_cli | G-12-5: `admin epoch-bump` exits 0 success, 1 error | IT-12-5 | 2 |
| 12_operator_cli | G-12-6: `doctor` 0 ok, 1 warnings only, 2 any failure (missing blobs fail) | IT-12-15, AC-73 | 2 |
| 12_operator_cli | G-12-7: SIGINT during `serve` exits 130 after graceful shutdown | IT-12-19 | 2 |
| 07_admin | G-07-1: hook and cleanup lists passed to the `DeletionPurger` constructor by the composition root | IT-07-6, IT-07-9, AT-07-2 | 3 |
| 07_admin | G-07-2 (resolved 2026-11-07, AZ-3814): per-step claim, no lease column; each purger step takes `SELECT ... FOR UPDATE SKIP LOCKED` on the `deleting` account row through the proposed `AccountAdmin::lock_deleting(tx, account) -> bool` in its own short transaction, so no claim is held across a cycle's idle time. Open risk: the hooks run on their own connections and a hook inserting a row with a foreign key to `accounts` needs `FOR KEY SHARE`, which `FOR UPDATE` blocks; `FOR NO KEY UPDATE` avoids it (owner asked; E-17/c06 confirms in IT-07-9) | IT-07-12, IT-07-9 | 3 |
| 07_admin / 11_http_api | G-07-3 / G-11-5: `LastAdmin` is HTTP 409 | IT-07-3 to IT-07-5, AT-07-2 | 3 |
| 07a_quota | G-07a-1: `InvalidLimit` bounds 0 <= limit <= 2^53 bytes (0 = no new data) | IT-07a-6 | 3 |
| 07a_quota | G-07a-2: `views` omits an unknown id | IT-07a-12, IT-07a-13 | 3 |
| 07a_quota | G-07a-3: not a gap (11 maps `QuotaExceeded` to 507; 07a owns the nothing-reserved guarantee) | - | 3 |
| 11_http_api | G-11-4: `/readyz` is 503 when any contributor is degraded | IT-11-17 | 4 |
| 11_http_api | G-11-2: missing `X-Sunnycloud-Client` accepted only from a browser session; native clients must send it; malformed is 400 | IT-11-7, IT-11-8 | 4 |
| 11_http_api | G-11-3: worker listener `:8081` serves only worker routes and test-clock read (else 404); public listener never serves worker routes | IT-11-9, ST-11-4 | 4 |
| 14_desktop_daemon | `DaemonStatus` gains `access_denied` (AC-67, v1.1) and `watcher_degraded` | IT-14-24; AC-67 (FT-P-75) | 5 |
| 15_desktop_settings | Settings surfaces `access_denied`/`watcher_degraded`; `DaemonClient` table includes `subscribe` and `list_pairs` | IT-15-14, IT-15-9; AC-67 | 5 |
| 17_android_app | `BackupCore::set_server(url) -> ServerCheck` (https, reachability, `server-info`, AC-54) | IT-17-6 | 6 |
| 17_android_app | `BackupStatus.location_redacted: bool` | IT-17-10 | 6 |
| 13_sync_core | `on_rotated` persist failure: retry once, then `TokenRejected`, app signs out | IT-13-9 | 7 |
| 08_media_jobs | G-08-1: `claim` skips and marks dead expired-lease jobs with exhausted attempts | IT-08-19 (BLOCKED), AT-08-2 | 8 |
| 08_media_jobs | G-08-2: 10 Mbit/s is decimal 10,000,000 bit/s, inclusive | IT-08-17 | 8 |
| 09_worker_supervisor | G-09-2: input too large for scratch -> `input_too_large` (permanent) | IT-09-7 | 9 |
| 09_worker_supervisor | G-09-3: output failing safe-open -> `output_rejected` (permanent) | IT-09-12 | 9 |
| 10_decoder | G-10-2: descendant found and killed at job end is hostile-class (runner exits) | IT-10-14, IT-10-16 | 9 |
| 10_decoder | G-10-5: SVG renamed `.png` is `Unsupported` | IT-10-16, IT-10-20, ST-10-3 | 9 |
| 10_decoder | G-10-7: `DecodeError` gains `ProtocolViolation` | IT-10-3, IT-10-16, ST-10-2 | 9 |
| 05_sync_feed | G-05-2 (resolved 2026-11-07, AZ-3773): the 5th concurrent long-poll of a principal (`max_holds_per_principal` = 4) is refused at once with the new typed variant `ChangeFeedError::TooManyHolds{retry_after}`, sent as the existing `rate_limited` (429) with `Retry-After`; no new wire code or status-table row; the variant is a proposed additive `contracts` change; Retry-After 5 s is proposed, not agreed | IT-05-6, ST-05-3 | spike |
| 05_sync_feed | G-05-3 (resolved 2026-11-07, AZ-3773): `raise_purge_horizon` never decreases the horizon (`GREATEST`); a lower seq leaves it unchanged and is not an error; a reset is `bump_epoch` | IT-05-12 | spike |
| 06_sharing | G-06-2 (resolved 2026-11-07, AZ-3695): a correct password while the per-link counter is limited gets `RateLimited{retry_after}`; accepting it would let an attacker keep guessing at no cost; trade-off: ten wrong guesses lock the link for the window, legitimate viewers included | IT-06-8, ST-06-6, NFT-SEC-05, NFT-SEC-06 | spike |
| 06_sharing | G-06-3 (resolved 2026-11-07, AZ-3695): `expires_at > now` is live; `expires_at <= now` is `NotFound`; time from the injectable clock | IT-06-7 | spike |
| 06_sharing | IT-06-12 note (resolved 2026-11-07, AZ-3695): `ShareConfig` validation in 06 is the single entry point; it refuses a missing, unreadable, empty or under-32-byte `share_grant_key`, naming the key; `serve` (12) calls it before binding the listener; E-01's loader checks only that the file exists and is readable | IT-06-12 | spike |
| 07_admin | G-S6 (resolved 2026-11-07, AZ-3814): a non-admin calling an admin-only operation gets 403 `forbidden`; another account's id gets 404 | ST-07-1, NFT-SEC-13 | spike |
| 07_admin | Deletion confirmation (resolved 2026-11-07, AZ-3814; inferred from ST-07-2's cases): the caller's own account id as lowercase hyphenated UUID text, exact equality, no trim, no case fold; `ConfirmationMismatch` otherwise | ST-07-2 | spike |
| 07_admin | Operator deletion (resolved 2026-11-07, AZ-3814): separate proposed `AccountDeletionService::request_as_operator(account)`, no confirmation (`--yes` is the confirmation), last-admin guard kept; `request` stays session-only; unknown or already-deleting account behaviour not specified | IT-07-3, IT-07-8 | spike |
| 08_media_jobs | G-08-3 (resolved 2026-11-07, AZ-3826): `thumbnail-states` omits ids of another account and unknown ids; a body over 1,000 ids is 422 `batch_too_large` (400 `invalid_request` rejected); the node-id to content-hash mapping stays open | ST-08-4, AT-08-1 | spike |
| 08_media_jobs | G-08-4 (resolved 2026-11-07, AZ-3826): worker `GET input` ignores `Range` and always answers 200 with the full body and exact `Content-Length` | IT-08-12 | spike |
| 08_media_jobs | Obsolete rule (resolved 2026-11-07, AZ-3826): a job's revision is obsolete only when collected (no longer `is_current_or_retained`, owner 02) or foreign; a superseded or deleted revision still retained is served and processed; FT-N-49, F10 and data model 9.1 reworded | FT-N-49, IT-08-12 | spike |
| 08_media_jobs | Hash encoding (resolved 2026-11-07, AZ-3826): `ClaimedJob.content_hash` is lowercase hex, 64 characters, as in the HTTP API surface | media-shared open point 1 | spike |
| 11_http_api | G-11-1 (resolved 2026-11-07, AZ-3800): `need_blocks` and `edited_since` are both 409 (412 is for `If-Match` only); `batch_too_large` is 422 for more entries than `batch_max_entries`; a body over the 4 MiB `Batch` class is a separate 413 from the HTTP layer; F5, FT-N-48 and AZ-3656 changed from 413 to 422 | IT-11-1, FT-N-48, AZ-3655, AZ-3656, AZ-3659, AZ-3810 | spike |

## 2. Open gaps by component

| ID | Gap | Affected tests | Owner decision? | Resolve during |
|---|---|---|---|---|
| **01_identity** | | | | |
| IT-01-17 note | RESOLVED 2026-11-11 (AZ-3681): `register_hooks` is an inherent set-once method of the identity service, called only by 12 before the listeners accept requests; unset means account creation fails closed (01 description, AccountLifecycleHook) | IT-01-17 | No | resolved |
| **03_transfer** | | | | |
| G-03-1 | RESOLVED 2026-11-11 (AZ-3731, owner): AC-3 now reads "at most 16 MiB" (4 lanes x 2 MiB in flight + one 8 MiB checkpoint interval); the limits stay (`max_lanes` 4, `checkpoint_bytes` 8 MiB, `checkpoint_interval` 1 s), so PT-03-1 throughput and the checkpoint WAL cost are unchanged. Options weighed (figures are estimates from the design, not measurements): checkpoint every block = worst case 10 MiB, 4x checkpoint writes (about +40% WAL for a 50 GiB file at line rate); 3 lanes + 4 MiB checkpoint = 10 MiB, 2x writes (+20%), send window 6 MiB (about 60 MB/s at 100 ms RTT vs 80); 2 lanes + 4 MiB = 8 MiB, window 4 MiB (about 40 MB/s at 100 ms RTT, misses AC-2 on WAN). E-14/c06 asserts 16 MiB. Still to change to 16 MiB where the figure is a derived threshold: AC-50 and its web rows (decision needed), E-23, E-26, E-24, 13, 16 and 03-adjacent test text, FT-P-01, NFT-PERF-05, NFT-RES-01, NFT-RES-29, results_report rows 1 and 50, ADR-004, system-flows | IT-03-4, PT-03-5, AT-03-1; AC-3 (FT-P-01, NFT-PERF-05, NFT-RES-01) | Yes (answered) | resolved |
| G-03-3 | RESOLVED 2026-11-11 (AZ-3731, owner): the commit answer per `PublishOutcome` and the removal of `EditedSince` from commit are written in `contracts/03_transfer/commit_service.md` (frozen): stale base = `node_id` is the file with the new content and `conflict_copy` the sibling holding the older content; name taken = `node_id` is the new file and `conflict_copy` repeats its own id; commit never answers edited-since. E-14/c08 removes the `EditedSince` variants from `CommitError` and `EntryError` in `contracts` | IT-03-7 | No | resolved |
| **04_blobstore** | | | | |
| ST-04-8 | RESOLVED 2026-11-11 (AZ-3721, owner): a second `write_block` of one index accepts identical bytes only (compare after decrypt) and refuses different bytes with `BlockConflict`; procedure and pass/fail criteria are in 04 `tests.md`; E-10/c06 builds the rule and the `BlockConflict` variant, E-10/c14 the test; 03 maps the error to its own `BlockConflict` (E-14 owner informed, no other change to 03) | ST-04-8 (03 ST-03-2 covers caller side) | Yes (answered) | resolved |
| **05_sync_feed** | | | | |
| **06_sharing** | | | | |
| **08_media_jobs** | | | | |
| **09_worker_supervisor** | | | | |
| G-09-1 | Supervisor slot-loop type is not named; tests use boundaries only | IT-09-9, IT-09-11 to IT-09-15, PT-09-1 | No | implement |
| G-09-4 | Observable effect of `OffsetClock` inside the supervisor not stated (only that the offset is shared) | IT-09-18 | No | decompose |
| **10_decoder** | | | | |
| G-10-1 | No config key or build target to substitute `vipsthumbnail`/`ffmpeg` in a test image (tests assume a `test-stubs` variant) | AT-10-2 and stub-based ITs | No | implement |
| G-10-3 | DS-05/06 are generated as 10-minute loops; the 30 s base files must be exposed in the manifest | IT-10-7, IT-10-8, IT-10-10, IT-10-13, AT-10-2 | No | implement |
| G-10-4 | BLOCKED: no way to freeze only the main loop to test the self-abort watchdog (SIGSTOP stops every thread) | IT-10-18 | No | decompose |
| G-10-6 | BLOCKED: poster-frame HDR (HLG) tolerance is an open owner item | IT-10-7 (HLG part), IT-10-10 | Yes | owner |
| **11_http_api** | | | | |
| **12_operator_cli** | | | | |
| G-12-1 | BLOCKED: no test-only migration source for failing, killed and irreversible migrations (release binary embeds its migrations) | IT-12-11; NFT-RES-13, NFT-RES-14 | No | decompose |
| G-12-3 | RESOLVED 2026-11-11 (AZ-3681, owner): reset of the sole active admin is allowed; no `LastAdmin` applies, no refusal error | IT-12-2, AT-12-2 | Yes (answered) | resolved |
| **13_sync_core** | | | | |
| IT-13-16 (B) | RESOLVED 2026-11-11 (AZ-3732): 03 answers `NothingToSend{revision}` for a live file at (parent, name) with the same current hash, matched by `name_key` (`contracts/03_transfer/upload_service.md`); the test belongs to E-23 (sync_core, component 13) | IT-13-16, IT-13-14 (AC-46) | No | resolved |
| **14_desktop_daemon** | | | | |
| ST-14-5 | Windows reserved-name marker (`%5F`) and ` (2)` suffix are still open owner items, but they do not block the server: checked 2026-11-11 (AZ-3705) that both are client-side (what a client does with a name its operating system cannot hold, E-05 epic); the server accepts every name except the short reject list of IT-02-1 (`CON`, `a:b`, a `%` all accepted), `name_key` is a pure function of the name, and the server's own conflict-copy suffix is separate. No server rule changes against E-13/c04. ST-14-5 stays BLOCKED on the client wording only | ST-14-5, AC-37 (FT-N-19, NFT-SEC-31) | Yes | owner (client wording) |
| **16_web_app** | | | | |
| IT-16-7 | `TooManyFiles` named but the limit that raises it is not stated | IT-16-7 | No | decompose |
| IT-16-8 | Firefox/Safari download verified up to 2 GiB: mechanism fixed by spike S12; no expected behaviour writable yet | IT-16-8, AC-7 (FT-P-68, NFT-SEC-21) | No | implement |
| IT-16-12 | Setup-token first-run page: route, fields and messages not specified (AC-23); only server behaviour is tested | IT-16-12, AC-23 | No | decompose |
| G-AS-1 | Web admin "Delete account…" for another user: 16 §5 lists user deletion, but 07 exposes only `PATCH {role, state}` for another account (no delete operation); add the operation or drop the menu item (`ui_mockups/DESIGN.md` §9 #1) | web admin task; FT-N-46 unaffected | Yes | owner |
| G-AS-2 | Desktop bandwidth limit has no unit (15 §5; tests use "5000"); the mockup shows KB/s (`ui_mockups/DESIGN.md` §9 #7) | FT-P-82 step 3, IT-15 settings tests | Yes | owner |
| **17_android_app** | | | | |
| IT-17-17 | Private-CA leg: description says user-added CAs work (`rustls-platform-verifier`), `environment.md` says release builds ignore user-installed CAs; intended behaviour undecided | IT-17-17 | Resolved | Step 5 owner decision |
| **18_ios_app** | | | | |
| IT-18-9 | AC-78 sibling for iOS (foreground and background upload one asset once): the v1.1 criterion does not exist | IT-18-9 | Yes | owner |
| IT-18-10 | Live Photo expected stored form (one asset with linked still and video) has no confirmed API/checkpoint columns; FT-P-33 deferred to v1.1 | IT-18-10, FT-P-33 | No | implement |
| **19_vault_core** | | | | |
| IT-19-5 | Memory ceilings for WASM and mobile Argon2 left to the S7 benchmark | IT-19-5 | No | implement |
| IT-19-7 | Envelope length vs the `client_meta` CHECK bound is not stated in `data_model.md` | IT-19-7 | No | decompose |
| IT-19-11 | Parent-folder swap (AC-60, restrictions): the spec binds vault and file but not the parent; either AC wording changes or the parent joins the AAD | IT-19-11, AC-60 | Resolved | Step 5 owner decision |
| **20_vault_service** | | | | |
| Vault membership | How 02, 03, 06 learn that a node lies under a vault root (flag, size limit, share refusal); moves in/out of a vault and an already-shared plaintext file undefined | IT-20-4, IT-20-5, AT-20-1 | Yes | owner |
| Header cap | `max_vault_header_bytes` has no default value | IT-20-1, AT-20-1 | No | decompose |
| Vaults per account | One or many vaults per account undecided (data model A6) | IT-20-7 | Yes | owner |
| **system/blackbox** | | | | |
| G-1 | RESOLVED 2026-11-11 (AZ-3705, owner): one name, stored as sent. `name_key` is NFC, Unicode full case folding, NFC (as built in `sc_names`), so an NFC and an NFD spelling of one name are the same key and the second is a `NameConflict`; the stored name keeps the exact spelling of the first writer; `name_key_version` stays the migration path if the fold ever changes | FT-P-15, AC-42 | Yes (answered) | resolved |
| G-2 | Exact error text/substring for out-of-range retention, no provider configured, upgrade must be re-run | FT-P-17, FT-N-25, FT-N-39 | No | decompose |
| G-5 | Audio-only clips in clip-mixed-100: transcode to AAC m4a, pass through, or no derivative | FT-P-27, AC-45 | Yes | owner |
| G-7 | Time bound for iOS foreground catch-up of a 100-photo backlog | FT-P-31, AC-9 | Yes | owner |
| G-9 | Response shape for a password-protected link opened without a password | FT-P-37, AC-12 | No | decompose |
| G-10 | Inline eligibility decided by declared MIME, sniffing or file name | FT-P-39, FT-N-11, AC-47 | Yes | owner |
| G-12 | Required text of the Linux fallback warning and token file location | FT-P-61, RESTRICT-19 | No | decompose |
| G-15 | OpenAPI document URL, provider-list endpoint, operation names for absence checks | FT-P-70, FT-P-73, FT-N-28 | No | decompose |
| G-19 | Upgrade message substring, client-version mechanism and minimum version value | FT-N-27, AC-64 | No | decompose |
| G-21 | GC cycle trigger for "removed in next cycle"; behaviour above 50 GiB (AC-4) | FT-P-17 | No | decompose |
| **system/performance** | | | | |
| G1, G11 | Reference server model and owner-held hardware (GPU, phones, WIN-WEAK, MAC) not yet named; scenarios report `NOT-RUN(HW-REF)` | PERF-01.., 12, 18, 19, 24, 25, 29 | Yes | owner |
| G2-G10, G12, G13 | Non-blocking test parameters/assumptions: AC-1 rates, 1 GbE margin, AC-18 settle inside 5 s, ext4 limit, listing "server time" definition, Samsung/Xiaomi limit, HEIC threshold, fallback limit, reinstall and vault timing, scripted-operator timing | PERF-01, 02, 03, 08, 09, 10, 12, 13, 19, 22, 29, 30 | Yes | owner |
| **system/resource-limit** | | | | |
| G1 | Session cap C and free-space floor FL have no defaults (same as G-R9) | RES-LIM-01, NFT-RES-05 | Yes | decompose |
| G2-G4, G6, G7 | Test definitions: mobile RSS vs PSS, 10 s averaging windows and kill allowance, "user active"/idle windows; owner-held hardware | RES-LIM-03, 04, 06, 08, 09 | No | implement |
| **system/resilience** | | | | |
| G-R3, G-R15 | Many-small-files test data set (100 multi-block files) to add to data_parameters; restriction IDs for new bullets missing | NFT-RES-03 | No | implement |
| G-R4 | Device offline > 90 days: locally unchanged file deleted by another device not specified | NFT-RES-08, AC-41 | Yes | owner |
| G-R5 | "One batch" window of AC-29 undefined (test uses a 5 s burst) | NFT-RES-12, AC-29 | Yes | owner |
| G-R9 | Session cap and free-space floor defaults not fixed (see resource-limit G1) | NFT-RES-05 | Yes | decompose |
| G-R12 | `resync_required` for cursor above head / unknown epoch is in research but not in an AC | NFT-RES-07 | No | decompose |
| G-R13, G-R18 | Hardware named in environment.md must exist (WIN-VM, LIN-VM, DEV-PIXEL, MAC); power-loss durability beyond `kill -9` is a proposed extension | NFT-RES-15, 16, 17, 27, NFT-RES-03 (variant B) | Yes | owner |
| **system/security** | | | | |
| G-S5 | TLS 1.2 refusal not stated (test asserts 1.3 negotiated, <= 1.1 refused) | NFT-SEC-11 | No | decompose |
| G-S10, G-S12 | 40 hostile-name cases per OS only named by class; restriction IDs for new bullets missing | NFT-SEC-31 | No | implement |
| G-S13 | Out of scope: vault freshness against a malicious server (caveat only); per-link download connection caps not an AC | none | Yes | owner |

## 3. Counts (open entries; grouped entries such as "G2-G10" count once)

| Resolve during | Component-level | System-level | Total |
|---|---|---|---|
| decompose | 11 | 10 | 21 |
| implement | 6 | 3 | 9 |
| owner | 9 | 10 | 19 |
| **Total** | 26 | 23 | 49 |

Owner decision needed: 24 of 59 (G-AS-1 and G-AS-2 added by the architecture-sync pass of 2026-11-04). Resolved at this pass: 28 items (27 of them numbered gaps/BLOCKED tests). Resolved by the owner on 2026-11-07 through spikes AZ-3773, AZ-3800, AZ-3826, AZ-3695 and AZ-3814: 13 items (the rows tagged "spike" in section 1; 8 of them were open gaps in section 2, so the open counts above fell by 8).

## 4. Notes

- Owner-decision items overlap with the owner list in `canonical-names.md` (vault share links, Windows reserved-name marker, poster-frame SSIM tolerance, WASM trim, external review of blob encryption, signing accounts) and with the proposed defaults in `tests/gap-decisions.md`.
- G-07-2 decided 2026-11-07 (AZ-3814): per-step claim, no lease column; see section 1 for the deadlock risk of `FOR UPDATE` against foreign-key checks of the hooks.
- Making `/readyz` 503 on any degraded contributor means 04's suspect-blob state also turns it 503; revisit if that is too strict (11 risks).
