# Blackbox Tests

Functional black-box scenarios for sunnycloud. Every scenario drives a public interface only: HTTP API `/api/v1`, web UI, Android/iOS app, desktop daemon and settings window, the sync-folder filesystem, and the operator surface (Compose files, `sunnycloud admin ...`, `admin restore`, `scrub run` and `doctor`, read access to the blob volume and a database dump as "what a stolen disk or operator sees"). Data sets are those of `_docs/00_problem/input_data/data_parameters.md`; expected numbers are those of `_docs/00_problem/input_data/expected_results/results_report.md` (RR) or the acceptance criterion itself where RR is silent. Performance-, resource-limit-, resilience- and security-focused tests (AC-1, 2, 15, 20, 21, 25, 39, 48, 49, 58, 59, 63) are owned by other documents; where an AC has both a functional and a non-functional side, only the functional side is here.

## Test conventions

- **REF-SERVER**: reference server REF-SRV (8 physical cores, 32 GB RAM, NVMe, 1 GbE) running the Compose install (API, worker, PostgreSQL 18, Caddy with request buffering off), blob volume on XFS, TLS 1.3 / HTTP/2, 4-core CPU-only worker. Encryption at rest is on unless stated.
- **Fault proxy**: sits between client and Caddy; can add 20 ms RTT, drop a connection at a byte offset, flip a bit in a request or response body, and count bytes and requests per path.
- **Clock control**: the server reads time through an injectable clock; the test image exposes a control endpoint (absent from release builds, FT-N-35) that the harness uses to advance days for retention, expiry and the 90-day horizon. Client clocks can be skewed independently.
- **oauth-mock**: mock identity provider from data_parameters.md; it can return a valid callback or any invalid callback.
- **Restriction numbering**: RESTRICT-01..31 follow `restriction-ids.md`. Two bullets added to `restrictions.md` after owner confirmation are numbered provisionally here: RESTRICT-32 = "Thumbnails and transcodes are encrypted at rest like every other blob; job leases, upload-session expiry, orphan sweep, idempotent batch commits, interrupted-upgrade rollback, provider timeout, HSTS, database-recovery time"; RESTRICT-33 = "The Android device token is excluded from backups; the vault passphrase has at least 12 characters and passes a strength check".
- **Hardware-dependent scenarios**: when the required physical devices (phones, macOS machine) are not available the result is recorded as `NOT-RUN(HW-DEVICE)`, never as pass or fail.
- **Hash**: BLAKE3 of full content unless stated. "Server order" means the order in which the server accepts commits.
- Release tags in titles: [v1.0], [v1.1], [v1.2]. A scenario tagged for a later release must be skipped, not failed, on an earlier build.

## Positive Scenarios

### FT-P-01: Resumable 5 GiB upload re-sends at most 10 MiB [v1.0]

**Summary**: A 5 GiB upload interrupted at 50% resumes and finishes with identical content.
**Traces to**: AC-3, AC-4, AC-7, RESTRICT-11
**Category**: Transfer

**Preconditions**:
- REF-SERVER running; one account; desktop or API client with fault proxy in path.

**Input data**: big-file 5 GiB (RR Transfer #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start upload; proxy drops the TCP connection when 2.5 GiB (50%) is acknowledged | Client detects failure; file not visible as committed |
| 2 | Client retries (new connection) | Session resumes; only unacknowledged blocks are sent |
| 3 | Wait for commit; fetch the file's stored hash via API | Hash equals BLAKE3 of the original |
| 4 | Compute bytes sent on the wire after the drop minus bytes still missing at the drop | Re-sent bytes at most 10 MiB |

**Expected outcome**: final BLAKE3 equals original (exact); re-sent bytes <= 10 MiB.
**Max execution time**: 300 s

### FT-P-02: 5 GiB download is hash-verified [v1.0]

**Summary**: A downloaded 5 GiB file is byte-identical to the uploaded one and the client verifies it.
**Traces to**: AC-7
**Category**: Transfer

**Preconditions**:
- big-file 5 GiB uploaded (FT-P-01).

**Input data**: big-file 5 GiB (RR Transfer #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Download via desktop daemon on a second device | File appears at final path only after verification passes |
| 2 | Hash the downloaded file | Equals original BLAKE3 |

**Expected outcome**: hash equal (exact); 0 undetected corruptions.
**Max execution time**: 300 s

### FT-P-03: 50 GiB upload and download; finish time independent of size [v1.0]

**Summary**: The maximum supported file size transfers both ways and the commit after the last block is O(1).
**Traces to**: AC-4, AC-7
**Category**: Transfer

**Preconditions**:
- REF-SERVER with >= 120 GiB free and quota >= 100 GiB for the account.

**Input data**: big-file 50 GiB (sparse-generated) and big-file 5 GiB.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the 5 GiB file; record t(last block ack) to t(committed file visible) | <= 2 s |
| 2 | Upload the 50 GiB file; record the same interval | <= 2 s |
| 3 | Download the 50 GiB file and hash it | Hash equals original |

**Expected outcome**: both finish intervals <= 2 s; the 50 GiB interval is not more than 2x the 5 GiB interval; hashes equal.
**Max execution time**: 1800 s

### FT-P-04: 10,000 small files via single-request upload and batch commit [v1.0]

**Summary**: Files up to 2 MiB go in one request each and are committed in batches.
**Traces to**: AC-40, RESTRICT-11
**Category**: Transfer

**Preconditions**:
- REF-SERVER; fault proxy adds 20 ms RTT and counts requests.

**Input data**: many-small (10,000 files of 1 KiB to 1 MiB, tree up to 5 levels) plus 3 zero-byte files.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the tree with the desktop client | Each file uses exactly 1 data request (no block-session requests) |
| 2 | Count commit requests at the proxy | Commits go through the batch commit endpoint; fewer than 10,000 commit requests |
| 3 | List the tree via API and compare hash set with source | 10,000 entries; hash set equal |
| 4 | Upload the 3 zero-byte files | Each is created through the staged path with 0 blocks; size 0, hash equals the empty-file hash; quota used unchanged |

**Expected outcome**: total time <= 60 s; 10,000 of 10,000 files present with equal hashes; 3 of 3 zero-byte files present.
**Max execution time**: 60 s

### FT-P-05: Block protocol conformance: 2 MiB blocks, 4 lanes, HTTP/2 [v1.0]

**Summary**: The wire protocol matches the documented fixed block size, lane cap and single-request threshold.
**Traces to**: RESTRICT-11, AC-40
**Category**: Protocol

**Preconditions**:
- Fault proxy records request sizes and concurrent connections per upload.

**Input data**: three files: exactly 2 MiB; 2 MiB + 1 byte; big-file 5 GiB.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the 2 MiB file | 1 data request |
| 2 | Upload the 2 MiB + 1 byte file | Block session: block 1 = 2 MiB, block 2 = 1 byte |
| 3 | Upload the 5 GiB file; sample concurrent data connections every 100 ms | Never more than 4; all blocks except the last are 2 MiB; ALPN negotiated `h2` over TLS 1.3 |

**Expected outcome**: block size 2 MiB (exact); max concurrent lanes <= 4; all three files committed with equal hashes.
**Max execution time**: 180 s

### FT-P-06: No zero-filled blocks after `kill -9` during 100 concurrent uploads [v1.0]

**Summary**: Acknowledged blocks survive a hard server kill; only fully committed files are visible and intact.
**Traces to**: AC-38, RESTRICT-09
**Category**: Durability

**Preconditions**:
- REF-SERVER; 100 concurrent upload sessions in progress.

**Input data**: many-small plus 20 files of 200 MiB each, uploaded concurrently (RR Transfer #5).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start 100 concurrent uploads; `kill -9` the API process at a random moment; repeat 5 times with different moments | Clients see connection errors |
| 2 | Restart the server; list all files and download each committed file | Every listed file downloads |
| 3 | Compare each committed file's hash with its source | Equal |
| 4 | Let clients resume the uncommitted uploads | They complete; final hashes equal |

**Expected outcome**: 0 committed files with missing or zero-filled blocks across 5 kills; no uncommitted file listed.
**Max execution time**: 600 s

### FT-P-07: Edit on device A reaches online device B within 5 s under background load [v1.0]

**Summary**: Change propagation latency holds while 100 uploads run.
**Traces to**: AC-5
**Category**: Sync

**Preconditions**:
- Devices A and B online and syncing the same folder; 100 concurrent 100 MiB uploads running from other devices.

**Input data**: one 5 MB file edited 100 times (RR Sync #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Edit the file on A (new content each time), wait 10 s between edits | A uploads |
| 2 | Record time from the file being closed on A until B holds the complete verified file (hash equals A's) | Equal on B |

**Expected outcome**: content on B equals A (exact); latency <= 5 s at p95 over 100 edits.
**Max execution time**: 1500 s

### FT-P-08: Create, rename, move, delete propagate between devices [v1.0]

**Summary**: Tree operations on A appear on B with the same structure and content.
**Traces to**: AC-5, AC-8
**Category**: Sync

**Preconditions**:
- Devices A and B online, empty shared folder.

**Input data**: 100 files from many-small (nested 3 levels).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | On A create 100 files | B has 100 files, hashes equal |
| 2 | On A rename 10 files and move 10 files to another folder | B reflects names and paths; file IDs unchanged on server |
| 3 | On A delete 10 files | B shows them removed |

**Expected outcome**: tree and hash set on B equal A after each step; each step visible on B <= 5 s at p95.
**Max execution time**: 120 s

### FT-P-09: Listing 10,000 entries is paged and complete [v1.0]

**Summary**: A large folder is returned in pages with no missing or duplicate entries.
**Traces to**: AC-6
**Category**: Listing

**Preconditions**:
- listing-10k folder uploaded.

**Input data**: listing-10k (RR Sync #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | `GET` the folder listing without page parameters | At least 2 pages (response is bounded; page token present) |
| 2 | Follow page tokens to the end | Union of entries = 10,000, 0 duplicates, 0 missing |
| 3 | Time 200 listing requests (client time minus RTT) | Server time per request |

**Expected outcome**: 10,000 unique entries; server time <= 300 ms at p95.
**Max execution time**: 60 s

### FT-P-10: Stale offline edit becomes a conflict copy [v1.0]

**Summary**: Two devices edit the same base revision; the later commit wins and the other content is kept as a conflict copy.
**Traces to**: AC-8, RESTRICT-24
**Category**: Conflicts

**Preconditions**:
- File F (1 MiB) synced to A and B at revision N; both go offline.

**Input data**: one 1 MiB file (RR Sync #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | A edits F (base rev N) and goes online; commit accepted | F is A's content |
| 2 | B edits F (base rev N), goes online later | B's commit accepted as current |
| 3 | List the folder on all devices | `F` has B's content; exactly 1 entry named "F (conflict copy YYYY-MM-DD HH-MM <device>)" (short form "name (conflict copy)" elsewhere) holds A's content; the device label is B, the committing device |

**Expected outcome**: current hash = B's content hash; conflict copy hash = A's content hash; copy count = 1; both files intact (no mixed content).
**Max execution time**: 60 s

### FT-P-11: Commit order follows server order, not client clock [v1.0]

**Summary**: A client with a wrong clock does not win by timestamp.
**Traces to**: AC-8
**Category**: Conflicts

**Preconditions**:
- Same setup as FT-P-10; clock of A set +2 h ahead, clock of B set -2 h behind.

**Input data**: one 1 MiB file.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | A (clock +2 h) commits an edit first | Accepted |
| 2 | B (clock -2 h, same base rev) commits second | B's commit is current despite older mtime |
| 3 | Repeat with clocks swapped | Second committer is again current |

**Expected outcome**: in both runs current version = later commit by server order (exact hash); 1 conflict copy per run.
**Max execution time**: 120 s

### FT-P-12: Edit beats delete [v1.0]

**Summary**: When one device deletes a file another edited, the file stays with the edit, in either commit order.
**Traces to**: AC-8, RESTRICT-24
**Category**: Conflicts

**Preconditions**:
- File F synced to A and B at revision N; both offline.

**Input data**: one file (RR Sync #4).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | A edits F and commits; B (offline) deleted F and commits later | F remains with A's content on server; B re-creates it locally |
| 2 | Reset; B deletes and commits first, then A (stale edit, base rev N) commits | F present with A's content on server and on B |

**Expected outcome**: F exists after both runs with A's content hash; 0 conflict copies; B's local file returns.
**Max execution time**: 120 s

### FT-P-13: Restored older database triggers `resync_required` and convergence [v1.0]

**Summary**: After a database restore from an older backup every device resyncs without missing changes.
**Traces to**: AC-41, RESTRICT-09
**Category**: Sync recovery

**Preconditions**:
- Two devices synced at epoch E1; backup taken; 500 more files changed afterwards; devices hold cursors newer than the backup.
- After the backup, device A created 5 files (unknown to the restored database) and deleted 5 files that the restored database still lists.

**Input data**: populated install (RR Sync #5).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Restore the database from the older backup (blobs kept), restart | Server starts with a new epoch |
| 2 | Devices poll `GET /api/v1/changes?cursor=<old>` | Response is `resync_required` |
| 3 | Wait for devices to finish full resync | The 5 local-only files are uploaded; the 5 server-only files are downloaded back to A; nothing is deleted on server or devices |

**Expected outcome**: both devices receive `resync_required` (2 of 2); 0 missed changes (hash sets equal across server and devices); converge <= 60 s; local-only files uploaded (5 of 5), server-only files downloaded (5 of 5), 0 deletions (the mass-delete guard is the only deletion path).
**Max execution time**: 120 s

### FT-P-14: Device offline 91 days does a safe full resync [v1.0]

**Summary**: Tombstones are purged after 90 days; an older device resyncs fully without data loss.
**Traces to**: AC-41, RESTRICT-09
**Category**: Sync recovery

**Preconditions**:
- Device D synced, then offline; other device deletes 10 files and edits 10 files; clock control shifts server 91 days.

**Input data**: 100 files from many-small.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Advance server clock 91 days; run tombstone purge cycle | Tombstones older than 90 days gone |
| 2 | D reconnects with its old cursor | `resync_required` |
| 3 | D completes resync | D's tree equals server's |

**Expected outcome**: the 10 deleted files are removed on D, the 10 edits are present, 0 files lost that still exist on server (exact).
**Max execution time**: 120 s

### FT-P-15: Names are case-preserving and case-insensitive; case-only rename keeps file ID [v1.0]

**Summary**: Stored names keep their case, lookups ignore case.
**Traces to**: AC-42, RESTRICT-07
**Category**: Naming

**Preconditions**:
- Empty folder.

**Input data**: tricky-names (case pair, NFC/NFD pair), file `Report.TXT`.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create `Report.TXT` | Listing shows exactly `Report.TXT` |
| 2 | Resolve path `report.txt` via API | Same file ID as step 1 |
| 3 | Rename to `report.txt` | Same file ID; listing shows `report.txt` |

**Expected outcome**: displayed case equals last stored case (exact); file ID unchanged in steps 2 and 3. NFC/NFD handling: Status BLOCKED for that sub-case (Gap G-1); other sub-cases runnable.
**Max execution time**: 10 s

### FT-P-16: Overwritten and deleted blobs are retained 7 days [v1.0]

**Summary**: A blob unreferenced by overwrite and delete is still on disk on day 6 and gone after day 7.
**Traces to**: AC-30, RESTRICT-10
**Category**: Retention

**Preconditions**:
- Default retention (7 days); clock control; operator read access to blob volume.

**Input data**: one file, overwritten once then deleted (RR Deletion #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload version 1, overwrite with version 2, delete the file; note both blob IDs on disk | Both blob files present |
| 2 | Advance clock to day 6, run GC cycle | Both blob files present; operator restore succeeds |
| 3 | Advance clock to day 8, run GC cycle | Both blob files absent |

**Expected outcome**: blob present at day 6, absent at day 8 (exact file existence).
**Max execution time**: 60 s

### FT-P-17: Retention is configurable 0 to 90 days; out-of-range rejected [v1.0]

**Summary**: Retention boundaries behave as configured.
**Traces to**: AC-30, RESTRICT-10
**Category**: Retention

**Preconditions**:
- Server restartable with a retention setting.

**Input data**: one file overwritten once per run.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Retention 0: overwrite a file, run GC cycle | Old blob removed in that cycle |
| 2 | Retention 90: overwrite, check at day 89 and day 91 | Present at day 89, absent at day 91 |
| 3 | Start the server with retention 91, then with -1 | Startup refuses both values with non-zero exit (message text: Gap G-2) |

**Expected outcome**: behavior as listed (exact file existence and exit codes).
**Max execution time**: 120 s

### FT-P-18: Operator restores a file by path and time [v1.0]

**Summary**: The operator command returns a deleted file with its original hash.
**Traces to**: AC-30
**Category**: Retention

**Preconditions**:
- File F (hash H1) uploaded at T1; overwritten at T2; deleted at T3; retention not yet expired.

**Input data**: one file (RR Deletion #4).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Run `sunnycloud admin restore --account --path --at` for F's path and time T1+1 min | Command succeeds |
| 2 | Download restored file as the owner | Hash equals H1 |
| 3 | Run restore for a time before F existed | Command reports nothing to restore; no file created |
| 4 | Delete F's original folder (retention not yet expired), then restore F again | F is recreated in a root folder named "Restored" (the original folder is gone); with the folder present, F returns to it |
| 5 | List the revisions of F's node through the API after each restore | The restore re-pointed the existing revision (same revision id, re-parented): 1 revision per stored blob, never a second revision for the same blob |

**Expected outcome**: restored hash = H1 (exact); revisions per restored blob = 1 (exact); no user-facing trash or restore control exists in web UI or API.
**Max execution time**: 30 s

### FT-P-19: Account deletion removes everything immediately [v1.0]

**Summary**: Deleting an account removes files, thumbnails, transcodes, shares and sessions completely within 15 min of the request, ignoring the retention period.
**Traces to**: AC-27, RESTRICT-10
**Category**: Account lifecycle

**Preconditions**:
- Account U with 20 files, 5 photos with thumbnails, 2 transcoded clips, 3 share links, 2 sessions; retention 7 days.

**Input data**: photos subset, clip-compliant, clip-1080-sdr (RR Deletion #5).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | U deletes the account | 2xx response |
| 2 | Poll the 3 share links (anonymously) and both session tokens until all are dead | Links 404; sessions rejected, within 15 min of step 1 |
| 3 | Within 15 min of step 1 operator checks database dump and blob volume for U's records and blob files | 0 rows, 0 blob files (originals, thumbnails, transcodes) |
| 4 | Operator restore for U's paths | Nothing restorable |
| 5 | Repeat with a second account of 2,000 files (more than one erase batch) | Everything is gone within 15 min; the deletion ran through the account erase hooks (tree, transfer, feed, sharing, quota, media) in batches until each reported nothing remaining, then the identity rows were deleted |

**Expected outcome**: 0 rows, 0 blobs, 0 working links or sessions no later than 15 min after the request (exact), with no retention delay (operator restore finds nothing).
**Max execution time**: 960 s

### FT-P-20: Thumbnail ready within 5 s of a non-bulk upload [v1.0]

**Summary**: A 12 MP JPEG gets a thumbnail and preview quickly.
**Traces to**: AC-10
**Category**: Photos

**Preconditions**:
- REF-SERVER worker idle.

**Input data**: photos (100 JPEG 12 MP) uploaded one at a time (RR Photos #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload one JPEG; poll its thumbnail URL every 250 ms | 404 until ready, then 200 `image/webp` |
| 2 | Fetch the preview rendition | 1440 px longest edge; thumbnail 320 px longest edge |
| 3 | Repeat for all 100 | Record time from upload completion to thumbnail 200 |

**Expected outcome**: thumbnail ready <= 5 s at p95 (at least 95 of 100); every thumbnail decodes as WebP with longest edge 320 px.
**Max execution time**: 300 s

### FT-P-21: Bulk import of 10,000 JPEGs completes in 15 min while interactive lane stays fast [v1.0]

**Summary**: Bulk thumbnailing finishes on the 4-core reference and does not starve a single interactive upload.
**Traces to**: AC-10
**Category**: Photos

**Preconditions**:
- 4-core reference worker; queue empty.

**Input data**: photos-bulk (10,000 JPEG 12 MP) plus 50 interactive uploads from `photos` during the import (RR Photos #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Bulk-import 10,000 JPEGs | Thumbnail jobs queued |
| 2 | During the import, upload 50 single photos spaced 15 s apart | Each gets a thumbnail |
| 3 | Count photos with a thumbnail when the bulk finishes | 10,000 of 10,000, 0 failed |

**Expected outcome**: bulk finish <= 15 min after last upload; the 50 interactive thumbnails p95 <= 5 s.
**Max execution time**: 1200 s

### FT-P-22: HEIC and EXIF-rotated photos get correctly oriented thumbnails [v1.0]

**Summary**: HEIC photos and JPEGs with EXIF orientations 5 to 8 produce thumbnails with swapped dimensions where needed that match an independent reference render.
**Traces to**: AC-10
**Category**: Photos

**Preconditions**:
- libheif >= 1.22.0 in the worker.
- Reference thumbnails rendered offline with an independent tool (ImageMagick auto-orient, then resize to 320 px longest edge).

**Input data**: photos: 20 HEIC and 10 JPEG with EXIF rotation, covering orientations 5, 6, 7 and 8 (RR "Thresholds added in the second gap round" #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the 20 HEIC and 10 rotated JPEGs | All 30 have thumbnails |
| 2 | For orientations 5-8 compare thumbnail width/height with the pixel dimensions stored in the file | Width and height swapped (thumbnail aspect ratio = displayed aspect ratio) |
| 3 | Compute SSIM of each thumbnail against its reference render | SSIM >= 0.95 |

**Expected outcome**: 30 of 30 thumbnails present, 30 of 30 with correct dimensions (exact), SSIM >= 0.95 for 30 of 30; thumbnails ready <= 5 s at p95 (HEIC has no shrink-on-load; same budget applied).
**Max execution time**: 120 s

### FT-P-23: 10-minute 1080p SDR clip transcodes in 5 min [v1.0]

**Summary**: A non-compliant 1080p clip becomes H.264 High 1080p AAC MP4 on 4 CPU cores.
**Traces to**: AC-11, RESTRICT-13
**Category**: Video

**Preconditions**:
- One worker with 4 physical cores, CPU only.

**Input data**: clip-1080-sdr (RR Video #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the clip; poll job state | Transcode job runs |
| 2 | Download the derived MP4 and `ffprobe` it | codec h264, profile High, height <= 1080, audio aac, moov before mdat (faststart), SDR BT.709 colour tags |
| 3 | Fetch the clip's thumbnail (poster frame); repeat with a 5 s excerpt of `clip-compliant` | Poster matches the source frame at 1 s (SSIM >= 0.95: proposed default, confirm in Step 4 review); for the 5 s excerpt it matches the frame at 0.5 s (10% of the duration) |

**Expected outcome**: transcode wall time <= 5 min; output duration within 0.5 s of source; playable (decodes to the end with 0 errors); poster frame taken at 1 s, or at 10% of the duration for clips under 10 s.
**Max execution time**: 420 s

### FT-P-24: 10-minute 4K HLG clip is tone-mapped to 1080p SDR in 20 min [v1.0]

**Summary**: HDR input yields SDR output with correct colour tags.
**Traces to**: AC-11
**Category**: Video

**Preconditions**:
- 4-core CPU worker.

**Input data**: clip-4k-hlg (RR Video #4).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the clip | Transcode job runs |
| 2 | `ffprobe` the derived MP4 | 1920x1080, yuv420p, color_primaries = color_transfer = color_space = bt709 |

**Expected outcome**: wall time <= 20 min; output SDR (transfer is not arib-std-b67 and not smpte2084); playable.
**Max execution time**: 1500 s

### FT-P-25: Compliant H.264 clip is never re-encoded [v1.0]

**Summary**: An already compliant MP4 (H.264 High or lower, long side <= 1920 and short side <= 1080, <= 10 Mbit/s video, AAC, SDR BT.709) is never re-encoded; with the index at the front it is served as is, with the index at the end it is losslessly remuxed. A clip just outside that definition is transcoded.
**Traces to**: AC-11, AC-44, RESTRICT-13
**Category**: Video

**Preconditions**:
- Worker idle.

**Input data**: clip-compliant (RR Video #5; 'Thresholds added' #7); variants: clip-compliant re-encoded with ffmpeg at 12 Mbit/s video; clip-compliant remuxed with the index at the end; a portrait 1080x1920 clip otherwise equal to clip-compliant (DS-35).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the clip, wait for the media pipeline to go idle | Job completes |
| 2 | List derived renditions of the file via API | 0 derived transcodes |
| 3 | Download the file | Bytes equal the original |
| 4 | Upload the 12 Mbit/s variant, wait for idle | Exactly 1 derived transcode, <= 10 Mbit/s |
| 5 | Upload the index-at-end variant, wait for idle | 0 re-encodes; one remux; the playable rendition has the index before the media data and its video and audio packets are byte-identical to the source (`ffmpeg -c copy -f md5` equal) |
| 6 | Upload the portrait 1080x1920 clip, wait for idle | 0 derived transcodes (compliant: long side 1920, short side 1080) |

**Expected outcome**: clip-compliant: derived transcode count = 0 (exact), played file = original hash; 12 Mbit/s variant: transcode count = 1; index-at-end variant: re-encodes = 0, remuxes = 1; portrait clip: transcode count = 0.
**Max execution time**: 60 s

### FT-P-26: Derived storage is at most 20% of original bytes [v1.0]

**Summary**: Thumbnails plus transcodes stay small relative to originals.
**Traces to**: AC-44
**Category**: Video

**Preconditions**:
- Empty account; pipeline idle.

**Input data**: clip-mixed-100 (RR Video #6).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload all 100 clips, wait until all jobs reach a terminal state | All jobs done or explicitly failed |
| 2 | Read per-file original and derived sizes via API | Sum available |

**Expected outcome**: sum(derived bytes) / sum(original bytes) <= 0.20.
**Max execution time**: 7200 s

### FT-P-27: Rotated and variable-frame-rate clips keep orientation and audio sync [v1.0]

**Summary**: Output orientation is upright and audio drift is within 40 ms.
**Traces to**: AC-45
**Category**: Video

**Preconditions**:
- Worker idle.

**Input data**: rotated and variable-frame-rate clips from clip-mixed-100 (RR Video #7); audio-only clips expected result: Gap G-5.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the rotated clips and VFR clips | Transcoded |
| 2 | Compare displayed width/height and a visual marker frame with the upright reference | Upright, displayed size matches |
| 3 | Measure audio sync offset using the embedded sync pulses of the test clip | Offset magnitude <= 40 ms |

**Expected outcome**: 100% of rotated outputs upright; audio drift <= 40 ms on every VFR clip.
**Max execution time**: 900 s

### FT-P-28: NVIDIA encoder is used only when enabled and probed [v1.1]

**Summary**: With the operator option on and a GPU present, the job uses NVENC and yields a valid file.
**Traces to**: AC-28, RESTRICT-02, RESTRICT-14
**Category**: Video

**Preconditions**:
- Worker with NVIDIA GPU; option off first.

**Input data**: clip-1080-sdr (RR Photos and videos #12).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Default config: transcode the clip | Job report shows CPU encoder (libx264); 0 NVENC use |
| 2 | Enable the GPU option, restart worker; worker start performs a test encode | Test encode logged as passed |
| 3 | Transcode the clip | Job report shows NVENC; output H.264 High 1080p AAC MP4 valid |

**Expected outcome**: NVENC used in step 3 only (exact); output valid (ffprobe 0 errors); NVENC wall time <= CPU wall time / 1.3 on clip-1080-sdr ('Thresholds added' #2).
**Max execution time**: 600 s

### FT-P-29: New Android photo uploads within 60 s on a Pixel [v1.0]

**Summary**: Camera backup picks up a new photo quickly on Wi-Fi.
**Traces to**: AC-9, RESTRICT-15
**Category**: Mobile backup

**Preconditions**:
- Pixel phone, Wi-Fi, battery optimization exempted for the app, backup enabled.

**Input data**: 20 new photos from `photos`, added one by one 2 min apart (RR Photos #8).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Add a photo to the camera folder | Present on server after upload |
| 2 | Record capture-to-server-visible time; repeat 20 times | Hash equals source each time |

**Expected outcome**: each photo on server <= 60 s after capture; 20 of 20 hashes equal.
**Max execution time**: 2700 s

### FT-P-30: Samsung and Xiaomi camera backup p95 is at most 5 minutes, published per device [v1.0]

**Summary**: On devices known to restrict background work, backup latency stays within the bound and is published per device.
**Traces to**: AC-9
**Category**: Mobile backup

**Preconditions**:
- Samsung and Xiaomi phones from phone-fixtures; Wi-Fi; battery optimization exemption on. Without the phones the result is `NOT-RUN(HW-DEVICE)`.

**Input data**: same 20-photo run as FT-P-29 on each device (RR "Thresholds added in the second gap round" #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Run FT-P-29 on each device (20 photos, 2 min apart) | Capture-to-server latency recorded per photo |
| 2 | Compute p95 per device | <= 5 min |
| 3 | Check the published device results in the docs | One row per device (Pixel, Samsung, Xiaomi) with the measured p95 |

**Expected outcome**: p95 <= 300 s for each of Samsung and Xiaomi; 3 of 3 devices published; 20 of 20 hashes equal per device.
**Max execution time**: 2700 s per device

### FT-P-31: iOS camera backup is best effort; foreground catch-up completes the backlog [v1.1]

**Summary**: Opening the iOS app uploads the whole backlog; no timing is promised for background uploads.
**Traces to**: AC-9, RESTRICT-27
**Category**: Mobile backup

**Preconditions**:
- iPhone with 100 photos not yet backed up; app signed in.

**Input data**: photos (100 JPEG 12 MP).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Open the app and leave it in foreground on Wi-Fi | Upload starts |
| 2 | Wait until the app shows backlog complete | Server has all photos |

**Expected outcome**: 100 of 100 photos on server, hashes equal, 0 duplicates; no latency threshold asserted for background-only operation (time bound for foreground: Gap G-7).
**Max execution time**: 1800 s

### FT-P-32: Reinstalling Android app uploads 0 bytes of media; edited photo is linked [v1.0]

**Summary**: Already-backed-up assets are recognised after reinstall.
**Traces to**: AC-46
**Category**: Mobile backup

**Preconditions**:
- Phone with 5,000 backed-up assets, including 5 photos edited on the phone after backup.

**Input data**: phone-fixtures: phone with 5,000 backed-up assets (RR Photos #9).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Uninstall and reinstall the app, sign in, enable backup | Scan runs |
| 2 | Count media bytes sent (fault proxy) | 0 bytes of media content |
| 3 | Inspect the 5 edited photos (same MediaStore asset ID, new content hash) on server | Each shows as linked to its original, not as a duplicate entry |
| 4 | Copy one photo on the phone so it gets a new MediaStore asset ID, back it up | Stored as a new file, not linked |

**Expected outcome**: 0 media bytes uploaded on reinstall; 5 of 5 edited photos linked (rule: same asset ID with new hash = linked); the copy under a new asset ID is a new, unlinked file.
**Max execution time**: 900 s

### FT-P-33: Live Photo pairing [v1.1]

**Summary**: A Live Photo is stored as one asset with a linked still and video.
**Traces to**: AC-46
**Category**: Mobile backup
**Status**: DEFERRED to v1.1 — the Live Photo fixture and the detailed expected result are written when v1.1 is planned; not a v1.0 gap.

**Preconditions**:
- iPhone with Live Photos (v1.1).

**Input data**: Live Photo fixture (to be created at v1.1 planning).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Back up a Live Photo | One asset on the server with a linked still and a linked video |

**Expected outcome**: 1 asset with 2 linked components (still, video); detailed checks at v1.1 planning.
**Max execution time**: to be set at v1.1 planning

### FT-P-34: Mobile background run starts in 10 s and survives 10 forced kills with 50,000 assets [v1.0]

**Summary**: A large first sync resumes after process kills without duplicates.
**Traces to**: AC-52, RESTRICT-15
**Category**: Mobile backup

**Preconditions**:
- Android device with a 50,000-asset library; memory sampled externally. (The "one upload implementation" part of AC-52 is a code review item, not a black-box test.)

**Input data**: phone-fixtures: library of 50,000 assets (RR Photos #10).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Add one photo; measure time to first upload byte of a background run | <= 10 s |
| 2 | Start first sync; force-kill the app process 10 times at random points; restart each time | Sync continues |
| 3 | After completion, compare server assets with device assets | 50,000 distinct files, 0 duplicates |
| 4 | Count assets hashed more than once; sample peak RSS | <= 1% re-hashed (<= 500); RSS <= 150 MB |

**Expected outcome**: as listed in steps 1, 3, 4.
**Max execution time**: 14400 s

### FT-P-35: Limited photo access shows "N photos visible, tap to allow more" and backs up newly picked photos [v1.0 Android, v1.1 iOS]

**Summary**: With partial media permission the app says how many photos it can see and offers to widen access; it does not claim a library total (Android 14 hides it).
**Traces to**: AC-53
**Category**: Mobile backup

**Preconditions**:
- Device with 100 photos; user grants access to 30 of them.

**Input data**: photos (100); phone-fixtures (RR Photos #11).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Grant access to 30 photos; open the backup screen | Text exactly "30 photos visible, tap to allow more"; no "of 100" anywhere on the screen |
| 2 | Tap the text, pick 5 more photos in the system picker; run backup | Screen shows "35 photos visible, tap to allow more" |
| 3 | Check the server | 35 photos present, hashes equal |

**Expected outcome**: label format "N photos visible, tap to allow more" exact (no total); 5 newly picked photos on server on the next run.
**Max execution time**: 300 s

### FT-P-36: Share link opens in 2 s and returns the original bytes [v1.0]

**Summary**: An anonymous visitor downloads a shared file.
**Traces to**: AC-12
**Category**: Sharing

**Preconditions**:
- Account with file F (10 MB).

**Input data**: one file (RR Sharing #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create a share link for F | Link returned |
| 2 | Open it with no cookies | HTTP 200 within 2 s |
| 3 | Hash the body | Equals original |

**Expected outcome**: HTTP 200, bytes equal original (exact), time to first byte <= 2 s.
**Max execution time**: 10 s

### FT-P-37: Share link default 7-day expiry, configurable expiry, optional password [v1.0]

**Summary**: Link lifetime and password options work as stated.
**Traces to**: AC-12
**Category**: Sharing

**Preconditions**:
- Clock control; file F.

**Input data**: one file; passwords "correct-horse" (right) and "wrong" (wrong).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create link with defaults; open at 6 d 23 h and at 7 d + 1 min | 200 then the revoked/missing response (FT-N-09) |
| 2 | Create link with expiry 1 h; open at 59 min and 61 min | 200 then the same expired response |
| 3 | Create link with password; open without, with wrong, with right password | No file bytes for first two; 200 with original bytes for third |

**Expected outcome**: expiry boundaries exact as listed; file bytes returned only with the right password (status code of the password prompt: Gap G-9).
**Max execution time**: 60 s

### FT-P-38: Share tokens are random and stored only as hashes [v1.0]

**Summary**: A database dump and logs reveal no usable token; tokens are long enough.
**Traces to**: AC-12
**Category**: Sharing

**Preconditions**:
- 1,000 share links created.

**Input data**: share tokens (RR Sharing #4).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Decode the 1,000 tokens from the URLs | Each carries >= 128 bits (>= 16 bytes decoded); 0 duplicates |
| 2 | Operator searches database dump and server logs for each token string | 0 occurrences |
| 3 | Try to open a link using the stored hash string as token | 404 (missing-link response) |

**Expected outcome**: 1,000 of 1,000 tokens >= 128 bits; 0 token strings in dump and logs; stored value not usable as token.
**Max execution time**: 120 s

### FT-P-39: Allowlisted images and videos display inline with `nosniff` [v1.0]

**Summary**: Safe media types render inline, with the protective header.
**Traces to**: AC-47
**Category**: Content serving

**Preconditions**:
- The 7 files shared and owned.

**Input data**: one file each of image/jpeg, image/png, image/gif, image/webp, image/avif, video/mp4, video/webm (synthesized from `photos` and clip-compliant with ImageMagick/ffmpeg) (RR Sharing #5; 'Thresholds added' #8).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Open each of the 5 images via share link and via authenticated content URL | `Content-Disposition` is `inline` or absent; `X-Content-Type-Options: nosniff` |
| 2 | Same for the MP4 and WebM files | Same headers; playable in a `<video>` tag |

**Expected outcome**: 14 of 14 responses (7 types x 2 routes) inline with `nosniff`; any type outside the list is an attachment (FT-N-11).
**Max execution time**: 20 s

### FT-P-40: Blob files on disk contain ciphertext only [v1.0]

**Summary**: A stolen disk without the master key yields no plaintext.
**Traces to**: AC-43, RESTRICT-07, RESTRICT-08, RESTRICT-32
**Category**: Encryption at rest

**Preconditions**:
- Encryption on; operator read access to the blob volume.

**Input data**: big-file 5 GiB containing a 64-byte known marker at every 100 MiB (RR Encryption #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the file; locate its blob on disk | Path matches `blobs/<2hex>/<2hex>/<id>` |
| 2 | Search the blob file bytes for the marker | 0 occurrences |
| 3 | Measure blob size and byte entropy of a 100 MiB sample | Size >= plaintext and <= 1.01 x plaintext; entropy >= 7.99 bits/byte |
| 4 | Download via API | Hash equals original |
| 5 | Locate the thumbnail and transcode blobs of a JPEG and of clip-1080-sdr on disk | Neither starts with a WebP (`RIFF....WEBP`) or MP4 (`ftyp`) signature; entropy >= 7.99 bits/byte |

**Expected outcome**: 0 marker hits; size and entropy within bounds; API download equal to original; derived blobs are ciphertext too.
**Max execution time**: 600 s

### FT-P-41: Master key rotation rewraps keys without rewriting blobs [v1.0]

**Summary**: After rotation blobs are unchanged on disk and still readable with the new key.
**Traces to**: AC-43
**Category**: Encryption at rest

**Preconditions**:
- 50 blobs uploaded; hashes, sizes and mtimes of blob files recorded.

**Input data**: one blob plus 49 others (RR Encryption #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Run `sunnycloud admin rewrap --new-key-file` with a new master key; restart with only the new key | Server starts |
| 2 | Re-hash blob files and compare mtimes | All 50 identical to the recorded values |
| 3 | Download all 50 files | Hashes equal originals |
| 4 | Start a server with only the old key | Cannot read content (FT-N-12 rule: no wrong-byte success) |

**Expected outcome**: 50 of 50 blob files byte- and mtime-identical; 50 of 50 downloads equal.
**Max execution time**: 120 s

### FT-P-42: Range reads of an encrypted blob return exact bytes [v1.0]

**Summary**: Random 4 KiB range reads, including chunk boundaries, are correct.
**Traces to**: AC-43
**Category**: Encryption at rest

**Preconditions**:
- One 1 GiB file uploaded; plaintext kept for comparison.

**Input data**: one 1 GiB blob (RR Encryption #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | 1,000 `Range: bytes=o-(o+4095)` reads at random offsets | 206, body equals plaintext slice |
| 2 | Reads at offsets 65,535, 65,536, 65,537 and at the last 4 KiB; one read crossing a 64 KiB boundary | 206, bytes equal |

**Expected outcome**: 1,000+ of 1,000+ reads byte-exact; 4 KiB read latency <= 5 ms at p95 ('Thresholds added' #1).
**Max execution time**: 60 s

### FT-P-43: Scrub reports a blob corrupted at rest [v1.0]

**Summary**: A single flipped bit on disk is found and named by the scheduled scrub.
**Traces to**: AC-24
**Category**: Integrity

**Preconditions**:
- 100 files uploaded; scrub previously reported clean.

**Input data**: one blob with one flipped bit (RR Auth/security #13).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Operator flips one bit in one blob file | Download of that file does not return wrong bytes silently |
| 2 | Run `sunnycloud scrub run` (or await the next scheduled scrub) | Report lists exactly that blob/file; exit code 2; the run's summary is in the audit export (`admin audit export --since`) and in the scrub metrics |
| 3 | Check the other 99 files | Not reported |

**Expected outcome**: exactly 1 mismatch reported in the next scrub (exact); 0 false positives. Transfers keeping >= 90% of baseline throughput while the scrub runs is measured by the performance tests.
**Max execution time**: 300 s

### FT-P-44: Vault upload stores ciphertext and decrypts on the client; no server derivatives [v1.2]

**Summary**: Server holds only ciphertext of a vault file; the client recovers the exact plaintext.
**Traces to**: AC-13, AC-14, RESTRICT-22, RESTRICT-23
**Category**: Vault

**Preconditions**:
- Vault created with passphrase and recovery key.

**Input data**: vault-file 100 MiB (RR Vault #1, #2); one JPEG.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the 100 MiB file into the vault | Accepted |
| 2 | Operator reads the stored bytes (also via a plain download of the server object) | Not equal to plaintext; contains no known marker |
| 3 | Download and decrypt with the right passphrase | Hash equals original |
| 4 | Upload the JPEG into the vault | 0 thumbnails and 0 transcodes generated |
| 5 | Time vault unlock (key derivation) natively and in the browser, 20 runs each | Native <= 1 s, browser <= 2 s (p95) |

**Expected outcome**: stored bytes differ from plaintext (exact); decrypted hash equals original; 0 derivatives; unlock <= 1 s native and <= 2 s browser ('Thresholds added' #6).
**Max execution time**: 300 s

### FT-P-45: Vault accepts 100 MiB, limit is configurable, camera backup stays out of the vault [v1.2]

**Summary**: The size limit works at its boundary and automatic backup never writes into a vault.
**Traces to**: AC-14
**Category**: Vault

**Preconditions**:
- Vault exists; default limit 100 MiB; Android camera backup on.

**Input data**: vault-file 100 MiB; a 51 MiB file with limit set to 50 MiB; one new camera photo.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload 100 MiB (exactly the limit) | Accepted |
| 2 | Set limit to 50 MiB; upload 51 MiB | Rejected with message naming the 50 MiB limit |
| 3 | Take a camera photo; wait for backup | Photo stored outside any vault; vault file count unchanged |

**Expected outcome**: exact limit boundary behavior; vault file count unchanged by camera backup.
**Max execution time**: 300 s

### FT-P-46: Vault empty and exact-chunk-multiple files round-trip [v1.2]

**Summary**: Edge sizes decrypt to identical content.
**Traces to**: AC-60
**Category**: Vault

**Preconditions**:
- Vault exists; chunk size read from the file header.

**Input data**: vault-file: empty file; files of exactly 1, 2 and N chunks.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload each file | Accepted |
| 2 | Download and decrypt each | Hash equals original |

**Expected outcome**: 4 of 4 files round-trip with equal hash (empty file: 0 bytes).
**Max execution time**: 120 s

### FT-P-47: Web vault page serves a strict CSP, loads no third-party hosts and leaves no key in memory after lock [v1.2]

**Summary**: The vault page is locked down and forgets its key.
**Traces to**: AC-62
**Category**: Vault

**Preconditions**:
- Vault unlocked in desktop Chrome with a test key of known bytes.

**Input data**: web vault page (RR Vault #7).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Load the vault page; read response `Content-Security-Policy` | No `unsafe-inline`, no `unsafe-eval` |
| 2 | List all hosts in the network log during unlock and a file open | Only the server origin; 0 third-party hosts |
| 3 | Lock the vault; take a heap snapshot; search for the key bytes | 0 matches |

**Expected outcome**: 0 forbidden CSP tokens; 0 third-party hosts; 0 key matches. External review is a release gate, not a test.
**Max execution time**: 120 s

### FT-P-48: Settings window opens within 1 s on the reference machines [v1.0]

**Summary**: Cold start of the settings window is fast without environment workarounds.
**Traces to**: AC-16, RESTRICT-06
**Category**: Desktop UI

**Preconditions**:
- 2019 Windows laptop with an HDD-class disk; Ubuntu LTS Wayland with NVIDIA; no workaround environment variables set (checked); daemon running; window process not running.

**Input data**: settings window (RR Desktop #5).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Launch the window from the tray or command, measure time to first interactive frame, 20 cold starts per machine (clear OS file cache between runs) | Window interactive |

**Expected outcome**: time to interactive <= 1.0 s at p95 on both machines.
**Max execution time**: 120 s

### FT-P-49: A changed setting reaches the running daemon within 5 s without restart [v1.0]

**Summary**: Setting changes are applied live.
**Traces to**: AC-16
**Category**: Desktop UI

**Preconditions**:
- Daemon running with PID recorded; two watched folders.

**Input data**: settings (RR Desktop #4): toggle "pause sync", add a watched folder, change bandwidth limit.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Toggle pause; create a file | Not uploaded after <= 5 s of the toggle |
| 2 | Add a watched folder with 10 files | Files uploaded <= 5 s after adding (scan starts) |
| 3 | Compare daemon PID | Unchanged |

**Expected outcome**: each setting effective <= 5 s; daemon PID unchanged (no restart).
**Max execution time**: 60 s

### FT-P-50: Folder and file choosers are the operating system's native dialogs [v1.0]

**Summary**: The settings window opens native pickers; it draws no file browser.
**Traces to**: AC-17, RESTRICT-06
**Category**: Desktop UI

**Preconditions**:
- Settings window open on Windows, Ubuntu (GNOME), macOS [v1.1].

**Input data**: settings (RR Desktop #6).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Click "Choose folder" and any file chooser (e.g. exclude-rules import) | Native dialog opens: Windows common item dialog, Linux portal FileChooser (visible on the session bus), macOS open panel |
| 2 | Inspect the app window's accessibility tree | No app-drawn directory tree or file list widget |

**Expected outcome**: 100% of chooser entry points open the OS dialog (exact); 0 custom file browsers.
**Max execution time**: 60 s

### FT-P-51: A downloaded file is not uploaded again [v1.0]

**Summary**: There is no echo loop.
**Traces to**: AC-18
**Category**: Desktop sync

**Preconditions**:
- Device A and B online; fault proxy counts uploads per file.

**Input data**: one 1 MiB file (RR Desktop #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create the file on A; wait for B to download it | B has the file |
| 2 | Watch B's upload requests for 90 s, including one safety rescan | 0 uploads of that file from B |
| 3 | Check server revision count | 1 |

**Expected outcome**: upload count from B = 0 (exact); revision count = 1.
**Max execution time**: 120 s

### FT-P-52: A file still being written is uploaded only after 2 s of stillness [v1.0]

**Summary**: The daemon waits for writes to settle.
**Traces to**: AC-18
**Category**: Desktop sync

**Preconditions**:
- Daemon running; proxy timestamps first upload byte.

**Input data**: one 2 GiB file copied in over 60 s (RR Desktop #3); a log file appended every 1 s for 30 s.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Copy the 2 GiB file into the synced folder at throttled speed | No upload commit while writing |
| 2 | Record first upload byte vs last write timestamp | >= 2 s after last write |
| 3 | Append to the log file every second for 30 s | No commit of an intermediate version; one commit after the last append plus >= 2 s |

**Expected outcome**: upload start - last write >= 2 s; 0 partially-written versions on server.
**Max execution time**: 180 s

### FT-P-53: Daemon syncs with no tray and starts at login [v1.0]

**Summary**: Autostart works through the platform mechanism and sync does not need a tray.
**Traces to**: AC-26, RESTRICT-25
**Category**: Desktop platform

**Preconditions**:
- Ubuntu with GNOME and no tray extension; Windows 10/11; macOS [v1.1].

**Input data**: daemon (RR Desktop #7).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Linux: log in; check the systemd user unit | Unit enabled and active; no tray icon visible |
| 2 | Windows: log on; query the scheduled task; list Windows services | Task with "At log on" trigger for the user exists and daemon running; no Windows service for it |
| 3 | Edit a file in the sync folder on each OS | Appears on a second device <= 5 s |
| 4 | macOS [v1.1]: log in; check launchd | LaunchAgent loaded, daemon running |

**Expected outcome**: daemon running on each OS without a tray; sync works (exact); Windows uses logon task, not a service.
**Max execution time**: 300 s

### FT-P-54: First-sync merge of a folder with 200 colliding paths [v1.0]

**Summary**: Connecting a folder with 100 identical and 100 different colliding files loses nothing and uploads no identical bytes.
**Traces to**: AC-31
**Category**: Desktop sync

**Preconditions**:
- Server already holds the 200 colliding paths; the folder is new on the device.

**Input data**: collide-1000 (RR Desktop #8).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Connect the folder; wait for sync to idle | Sync completes |
| 2 | Proxy: sum of data bytes sent for the 100 identical files | 0 bytes |
| 3 | List collisions on server | Exactly 100 entries named "name (conflict copy)" |
| 4 | Verify every local and every pre-existing server content hash exists on server and the device | All present |

**Expected outcome**: 0 files lost; 0 identical bytes uploaded; exactly 100 conflict copies; 800 non-colliding files uploaded.
**Max execution time**: 300 s

### FT-P-55: Cloud-evicted placeholders are never hydrated or deleted [v1.0]

**Summary**: Evicted OneDrive files are skipped and reported.
**Traces to**: AC-32, RESTRICT-24
**Category**: Desktop sync

**Preconditions**:
- Windows with OneDrive placeholders; macOS iCloud variant [v1.1]; server already holds the 100 files from another device in a second run.

**Input data**: evicted-100 among 400 normal files (RR Desktop #9).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Sync the tree | 400 normal files synced |
| 2 | Read provider hydration counters / disk usage | 0 bytes hydrated |
| 3 | Open the daemon's report | Reports 100 skipped files |
| 4 | Second run with the 100 already on server: sync again | 0 deletions propagated |

**Expected outcome**: 0 B hydration; 100 skipped reported; 0 deletions.
**Max execution time**: 300 s

### FT-P-56: Exclusively locked file is retried quietly and uploaded within 10 s of release [v1.0]

**Summary**: A locked file is neither burned-in CPU nor reported as deleted.
**Traces to**: AC-33
**Category**: Desktop sync

**Preconditions**:
- Windows; a process holds the file with no sharing.

**Input data**: locked file, lock for 60 s then release (RR Desktop #10).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Modify and lock the file for 60 s | Daemon CPU averaged over the 60 s |
| 2 | Observe server file list during the lock | File present throughout; no delete event |
| 3 | Release the lock | New content uploaded |

**Expected outcome**: daemon CPU <= 1% during lock; upload completes <= 10 s after release; 0 delete events.
**Max execution time**: 120 s

### FT-P-57: Save-in-place from five editors yields one revision on the same file ID [v1.0]

**Summary**: Atomic-replace saves do not create copies, deletes or broken share links.
**Traces to**: AC-34
**Category**: Desktop sync

**Preconditions**:
- File shared by link; synced; one run per editor and per supported OS.

**Input data**: editor-saves: vim, JetBrains, Word, LibreOffice, KeePassXC (RR Desktop #11).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Record file ID and revision count; script one save-in-place | Daemon syncs |
| 2 | Query server: file ID, revision count, copies, delete events | Same file ID; revision +1; 0 copies; 0 deletes |
| 3 | Open the pre-existing share link | HTTP 200 with the new content |

**Expected outcome**: 5 of 5 editors: exactly 1 new revision, same file ID, 0 conflict copies, 0 deletes, share link opens.
**Max execution time**: 300 s

### FT-P-58: Devices converge within 60 s after a 30-minute suspend [v1.0]

**Summary**: Changes on both sides during suspend merge cleanly after wake.
**Traces to**: AC-35
**Category**: Desktop sync

**Preconditions**:
- Devices A (Windows), B (Linux), C (macOS [v1.1]) in one folder.

**Input data**: 500 files changed on A (RR Desktop #12) before suspend; a disjoint set of 500 files changed on B during the 30-minute suspend.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Edit 500 files locally on A, suspend A immediately, wait 30 min | - |
| 2 | Edit 500 different files on B during the suspend | B syncs |
| 3 | Wake A; record time until hash sets on A, B and server are equal | Converged |

**Expected outcome**: all devices equal <= 60 s after wake; 0 lost edits; 0 conflict copies (no overlap).
**Max execution time**: 2100 s

### FT-P-59: 200 deletions in one batch are held; "no" restores all 200 [v1.0]

**Summary**: Mass deletes require confirmation and are reversible by refusal.
**Traces to**: AC-29, RESTRICT-24
**Category**: Deletion safety

**Preconditions**:
- Two devices; 300 synced files.

**Input data**: 300 files, delete 200 at once (RR Deletion #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Delete 200 files on A in one batch | Daemon pauses deletes and asks the user |
| 2 | Check server and B | 300 files still present; 0 deletions propagated |
| 3 | Answer "no" | All 200 files return on A (re-downloaded) |
| 4 | Repeat the deletion and answer "yes" | 200 deleted on server and on B |

**Expected outcome**: 0 deletions before confirmation; after "no" 300 of 300 files on A; after "yes" 100 remain everywhere.
**Max execution time**: 180 s

### FT-P-60: Include and exclude rules are gitignore-style patterns [v1.0]

**Summary**: Excluded paths are never uploaded, downloaded or deleted; negation re-includes.
**Traces to**: AC-66, RESTRICT-06
**Category**: Desktop UI

**Preconditions**:
- Devices A and B pair with the same server folder; on A the settings window lists the patterns below, relative to the pair root: `*.tmp`, `build/`, `/secret.txt`, `docs/**/draft?.md`, `!keep.tmp`.

**Input data**: local files on A: `a.tmp`, `keep.tmp`, `build/out.bin`, `src/build/x.bin`, `secret.txt`, `sub/secret.txt`, `docs/a/b/draft1.md`, `docs/a/draft12.md`, `notes.txt`, `z.tmp` (RR "Thresholds added in the second gap round" #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Let A sync; list the server folder | Exactly `keep.tmp`, `sub/secret.txt`, `docs/a/draft12.md`, `notes.txt` (0 excluded files uploaded) |
| 2 | On B create `build/remote.bin` and `z.tmp` on the server side | A downloads neither (0 bytes; files absent on A; A's own `z.tmp` unchanged) |
| 3 | On B delete `z.tmp`; on A delete `a.tmp` | A's local `z.tmp` still exists; 0 delete events sent for `a.tmp` |

**Expected outcome**: server set equals the 4 listed files (exact); 0 excluded paths uploaded, downloaded or deleted; `keep.tmp` (negated) synced; `sub/secret.txt` synced (leading `/` anchors).
**Max execution time**: 120 s

### FT-P-61: Device token is kept in the OS keychain, with a user-only file fallback on Linux [v1.0]

**Summary**: The desktop token is not stored in plain, world-readable places.
**Traces to**: RESTRICT-19
**Category**: Desktop auth

**Preconditions**:
- Device paired through the device grant.

**Input data**: daemon on Windows, Linux with and without a Secret Service.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Windows/Linux (keyring present): inspect keychain entries after pairing | One entry for sunnycloud |
| 2 | Linux without keyring: pair; inspect token file | File in user config dir, mode 0600, owner = user; daemon log has a WARN line about the fallback (text: Gap G-12) |
| 3 | Search sync root, config JSON and process arguments for the token string | 0 occurrences |

**Expected outcome**: keychain entry exists, or fallback file mode exactly 0600 with 1 warning; 0 token strings elsewhere.
**Max execution time**: 60 s

### FT-P-62: Sign-in with GitHub and generic OIDC (Google through OIDC); operator picks the provider subset [v1.0]

**Summary**: Each configured provider creates a session; only enabled providers are offered; identity is provider plus provider user id.
**Traces to**: AC-19, RESTRICT-17
**Category**: Authentication

**Preconditions**:
- oauth-mock registered as GitHub and as generic OIDC (Google profile); invited user exists.

**Input data**: oauth-mock valid callback (RR Auth #20).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Enable GitHub and OIDC; sign in with each | Session created for each |
| 2 | Enable only GitHub, then only OIDC | Sign-in page lists exactly the enabled providers |
| 3 | Log in again with the same provider and id but a changed email | Same account (0 new accounts) |
| 4 | Log in with another provider that returns the same email | Separate account, not linked |

**Expected outcome**: 2 of 2 providers create sessions; provider list equals the enabled set; accounts keyed by (provider, id) (exact).
**Max execution time**: 60 s

### FT-P-63: Sign in with Apple, and the iOS app offers Google or GitHub only when Apple is enabled [v1.1]

**Summary**: Apple works and the App Store rule is enforced in the iOS app.
**Traces to**: AC-19, RESTRICT-02, RESTRICT-21
**Category**: Authentication

**Preconditions**:
- Server with Apple + GitHub enabled; second server with GitHub only; iOS app.

**Input data**: oauth-mock Apple profile.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | iOS app on the Apple+GitHub server: open sign-in | Shows Apple and GitHub |
| 2 | Sign in with Apple | Session created |
| 3 | iOS app on the GitHub-only server: open sign-in | Shows no Google or GitHub button (see FT-N-34) |

**Expected outcome**: step 1 shows both; step 2 session created; step 3 empty of Google/GitHub.
**Max execution time**: 120 s

### FT-P-64: First admin is created with the one-time setup token [v1.0]

**Summary**: On an empty database the setup token creates the first admin exactly once.
**Traces to**: AC-23, RESTRICT-18
**Category**: Bootstrap

**Preconditions**:
- Empty database; setup token printed at first start.

**Input data**: empty install (RR Auth #1); oauth-mock.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Social login presenting the setup token | Account created with admin role |
| 2 | Second login attempt with the same token on a new identity | Rejected; 0 new accounts |

**Expected outcome**: 1 admin account; token usable exactly once.
**Max execution time**: 60 s

### FT-P-65: `sunnycloud admin` command and single-use invites [v1.0]

**Summary**: Operator can create and reset admins; later users register by invite.
**Traces to**: AC-23, RESTRICT-18
**Category**: Bootstrap

**Preconditions**:
- Admin exists.

**Input data**: oauth-mock identity with verified email v@x.test; invite token; configuration listing verified address v@x.test.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Run `sunnycloud admin create` for a second admin and `sunnycloud admin reset <account>` for the first | Both succeed; reset invalidates old admin sessions |
| 2 | Admin creates an invite; user signs in via oauth-mock presenting it | Account created |
| 3 | Another user redeems the same invite | Rejected |
| 4 | Identity with verified v@x.test (listed) redeems an invite | Account created |
| 5 | Identity with unverified v@x.test redeems an invite; a listed verified identity signs in without any invite | Both create 0 accounts |
| 6 | Demote the second admin with `sunnycloud admin demote`, then try to demote or delete the remaining (last) admin, in the web admin section and with the CLI | Refused each time; exit code 2 from the CLI; one admin remains |

**Expected outcome**: invite works exactly once; the listed e-mail is honoured only at redemption of an invite and only when the provider verifies it (exact: 1 account in step 4, 0 in step 5); the last admin cannot be removed (step 6).
**Max execution time**: 60 s

### FT-P-66: Device-approval page shows requester vs approver location and expires in 10 min [v1.0]

**Summary**: The RFC 8628 approval page warns on mismatch.
**Traces to**: AC-57, RESTRICT-19
**Category**: Authentication

**Preconditions**:
- Daemon on IP X (location L1); approver browser on IP Y (location L2) from a mock geo database.

**Input data**: device grant (RR Auth #4).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Daemon requests a device code; approver opens the verification page and enters the user code | Page shows both IP/location pairs and a mismatch warning |
| 2 | Repeat with the same IP | Shows both, no warning |
| 3 | Approve in a run; deny/wait in another until 10 min + 1 s | Approved run: daemon gets a token; waiting run: code rejected |

**Expected outcome**: warning shown iff IPs/locations differ; `expires_in` <= 600 s and code rejected after expiry.
**Max execution time**: 720 s

### FT-P-67: Browser 50 GiB upload resumes after tab close and detects mid-upload edits [v1.0]

**Summary**: Chrome, Firefox and Safari upload 50 GiB with bounded memory and resume by re-selecting the file.
**Traces to**: AC-50, AC-4
**Category**: Web client

**Preconditions**:
- Desktop browsers: Chrome, Firefox, Safari (macOS). Heap sampled via browser tooling.

**Input data**: big-file 50 GiB (RR Web #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start the upload; sample worker + page heap throughout | Heap never above 150 MB |
| 2 | Close the tab at ~50%; reopen, re-select the same file | Upload resumes; bytes re-sent <= 10 MiB |
| 3 | Wait for commit | Hash equals original |

**Expected outcome**: per browser: heap <= 150 MB; re-sent <= 10 MiB; final hash equal (exact). Mid-upload edit: FT-N-03.
**Max execution time**: 7200 s per browser

### FT-P-68: Browser 50 GiB download completes with bounded heap [v1.0]

**Summary**: Large downloads work in all three browsers.
**Traces to**: AC-51, AC-7
**Category**: Web client

**Preconditions**:
- 50 GiB file on server.

**Input data**: big-file 50 GiB (RR Web #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Download in each browser; sample heap | Completes |
| 2 | Hash the saved file | Equals original |
| 3 | Chrome: download a 100 MiB file; Firefox and Safari: download a 1 GiB and a 3 GiB file | Chrome and the 1 GiB downloads are hash-verified in the browser and success is shown; the 3 GiB download shows "not verified in the browser" |

**Expected outcome**: heap <= 200 MB per browser; hashes equal; Firefox/Safari verify in the browser up to 2 GiB and say "not verified in the browser" above it (RR second-round #8).
**Max execution time**: 7200 s per browser

### FT-P-69: Operator reaches a working server in 30 minutes with the default Compose and Caddy [v1.0]

**Summary**: README-driven install on a clean host.
**Traces to**: AC-22, RESTRICT-16, RESTRICT-13
**Category**: Installation

**Preconditions**:
- Clean host with Docker; one GitHub OAuth app registered; no prior state.

**Input data**: clean host (RR Auth #19).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Follow the README exactly; start timer at first command | Compose stack up |
| 2 | List Compose services | api, worker, postgres, caddy present; no redis or other queue |
| 3 | Create first admin, sign in, upload a file through the web | A 2 MiB data block upload returns no 413 (raised body limit); 5 GiB upload through Caddy shows proxy-container disk growth <= 100 MB |

**Expected outcome**: working sign-in and upload within 30 min; Caddy service present; request buffering off (disk growth bound); no Redis service.
**Max execution time**: 1800 s

### FT-P-70: API is versioned under `/api/v1` and described by OpenAPI [v1.0]

**Summary**: All operations are under `/api/v1` and the published OpenAPI document matches reality.
**Traces to**: AC-64, RESTRICT-11, RESTRICT-05
**Category**: API

**Preconditions**:
- Server running; OpenAPI document location from README (Gap G-15).

**Input data**: OpenAPI document; traffic from the full blackbox suite.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Fetch and validate the OpenAPI document | Valid OpenAPI 3.x, 0 validator errors |
| 2 | Compare every API route called by this suite with the document | 100% listed |
| 3 | Check every path starts with `/api/v1` | 100% |

**Expected outcome**: 0 validator errors; 100% route coverage; 100% paths under `/api/v1`.
**Max execution time**: 60 s

### FT-P-71: Backup and restore on a clean host returns all data [v1.0]

**Summary**: Documented restore of database, blobs and master key reproduces the installation.
**Traces to**: AC-65, RESTRICT-31, AC-43
**Category**: Operations

**Preconditions**:
- Clean host; install guide open.

**Input data**: backup-set (RR Auth #18).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Follow the documented restore with DB dump, blobs and master key, using exactly the documented commands (`pg_dump` and `pg_restore` as the Postgres superuser inside the db container) | Server starts |
| 2 | Download every file; hash it | Hash set equals the recorded set |
| 3 | Sign in and sync a device | Devices converge (FT-P-13 behavior) |
| 4 | Read the install guide | States that losing the master key makes data unrecoverable and describes backing it up (regex: master key AND unrecoverable/cannot be recovered) |

**Expected outcome**: 100% hash-set equal; documentation statements present.
**Max execution time**: 1800 s

### FT-P-72: Upgrade between releases keeps all data and working clients [v1.0]

**Summary**: Upgrading the stack to the next release loses nothing.
**Traces to**: AC-65, RESTRICT-31
**Category**: Operations

**Preconditions**:
- Installation at release N with backup-set data.

**Input data**: backup-set (RR Auth #18).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Pull and start release N+1 per the upgrade notes | Migrations complete, server starts |
| 2 | Download every file; hash it | Equal to recorded set |
| 3 | Existing client of supported version syncs a new file | Succeeds |

**Expected outcome**: 100% hash-set equal; supported client syncs.
**Max execution time**: 1200 s

### FT-P-73: v1.0 build exposes only v1.0 scope [v1.0]

**Summary**: Features of later releases are absent from a v1.0 deployment.
**Traces to**: RESTRICT-01, RESTRICT-02, RESTRICT-03, RESTRICT-04, RESTRICT-14
**Category**: Release scoping

**Preconditions**:
- v1.0 release candidate deployed.

**Input data**: sign-in page, web UI, OpenAPI document, release artifacts list.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Open the sign-in page with Apple not configured | No Apple option; only GitHub/OIDC |
| 2 | Look for vault creation in web UI and any vault operation in OpenAPI | Neither exists |
| 3 | Check worker configuration option for NVIDIA | Absent or off; transcode encoder is libx264 |
| 4 | Check release downloads | Android, Linux, Windows installers; no iOS or macOS artifacts |

**Expected outcome**: 0 vault, Apple, NVENC-on, iOS or macOS surfaces in a v1.0 build (operation names: Gap G-15).
**Max execution time**: 60 s

### FT-P-74: Worker image and repository carry the required licence and media-build properties [v1.0]

**Summary**: ffmpeg is built without non-redistributable parts and the licence files exist.
**Traces to**: RESTRICT-13, RESTRICT-29
**Category**: Distribution

**Preconditions**:
- Release worker image and repository checkout.

**Input data**: worker image; repository root.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Run `ffmpeg -version` and `ffmpeg -encoders` in the worker image | Configuration contains `--enable-libx264` and `--enable-gpl`; contains neither `--enable-libfdk-aac` nor `--enable-nonfree`; encoder `aac` present |
| 2 | Inspect repository root | AGPL-3.0 licence text and the App Store exception file exist |
| 3 | Inspect worker image notices | Offer of corresponding source for GPL components |

**Expected outcome**: all 3 checks pass (exact strings).
**Max execution time**: 60 s

### FT-P-75: macOS access denied is reported and recovers after the user grants access [v1.1]

**Summary**: A folder macOS blocks is flagged in the UI, other folders keep syncing, and the blocked folder resumes after permission is granted.
**Traces to**: AC-67, RESTRICT-26, RESTRICT-02
**Category**: Desktop platform

**Preconditions**:
- macOS machine with two sync pairs: `~/Documents` (permission not granted) and `~/Sync-ok`. Without a macOS machine the result is `NOT-RUN(HW-DEVICE)`.

**Input data**: 10 files in each pair (RR "Thresholds added in the second gap round" #9).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start the daemon with access to `~/Documents` denied | Within 10 s tray and settings window show "access denied: allow access in System Settings" |
| 2 | Edit a file in `~/Sync-ok` | Syncs normally (<= 5 s) |
| 3 | Grant access in System Settings | `~/Documents` resumes syncing within 10 s; its 10 files reach the server |

**Expected outcome**: message exact and shown <= 10 s; other pair unaffected (1 of 1 edit synced); resume <= 10 s after grant.
**Max execution time**: 120 s

### FT-P-76: Foreground and background runs upload the same asset once [v1.0 Android]

**Summary**: Opening the app while the background worker runs never creates a second server file for the same asset.
**Traces to**: AC-78, RESTRICT-15
**Category**: Mobile backup

**Preconditions**:
- Android emulator with the app signed in and the account empty; battery exempt, Wi-Fi.

**Input data**: phone-fixtures: 100 new photos pushed to the device (RR Photos #10 subset).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Push the 100 photos and trigger the background worker through the job scheduler (`adb shell cmd jobscheduler run`) at the same moment the app is brought to the foreground | Both start; the app shows one running backup |
| 2 | Wait until the backup status shows 100 uploaded | Completed |
| 3 | List the camera-backup folder through the API; compare hashes with the pushed photos | 100 files, one per photo, 0 duplicates, 0 conflict copies |
| 4 | Repeat steps 1 to 3 with 100 other photos, and kill the app process once during the run | 200 files in total, 0 duplicates |

**Expected outcome**: server files = assets pushed (exact), duplicates = 0, in both repetitions.
**Max execution time**: 900 s

### FT-P-77: A feed row never overwrites a locally edited file; remote deletes go to the trash [v1.0 Linux and Windows]

**Summary**: The desktop uploads its unsynced local edit first (with the base revision) and only then applies the pending server change; a clean remote delete moves the file to the OS trash.
**Traces to**: AC-8, AC-35
**Category**: Desktop sync

**Preconditions**:
- Desktop A and B synced on file f (revision R1); A's network can be cut (`e2e-ctl`).

**Input data**: a 3 MiB text file; a second file g.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Cut A's network; edit f on A (content EA); edit f on B (content EB, becomes R2 on the server); restore A's network | A reports f as changed; A's feed poll returns the R2 row |
| 2 | Watch the proxy log and A's file | A's upload of f with base R1 reaches the server before A downloads R2; A's file never holds EB while EA is not yet on the server |
| 3 | Wait for convergence | Server: R2 current, EA stored as a conflict copy named by the server; A and B hold both files with equal hashes |
| 4 | Cut A's network; B deletes g; A edits g (dirty); restore the network | A's edit wins: g is revived on the server with EA; A's file is not removed locally |
| 5 | B deletes a file h that A has not touched | A moves h to its OS trash or recycle bin (freedesktop trash on Linux); h is in the trash, not unlinked |

**Expected outcome**: local edits lost = 0 (exact); downloads over a dirty path before the upload ack = 0; the clean remote delete lands in the trash (1 of 1).
**Max execution time**: 120 s

## Negative Scenarios

### FT-N-01: A bit flipped in an upload block is rejected and re-sent [v1.0]

**Summary**: In-transit upload corruption never reaches a committed file.
**Traces to**: AC-7 (negative case)
**Category**: Integrity

**Preconditions**:
- Fault proxy flips 1 bit in the body of one block.

**Input data**: big-file 5 GiB.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload through the proxy | Server rejects the corrupted block (non-2xx, not acknowledged) |
| 2 | Client re-sends that block | Accepted |
| 3 | After commit compare hash | Equals original |

**Expected outcome**: 1 rejected block; extra bytes <= 2 MiB plus headers; final hash equal; 0 undetected corruptions.
**Max execution time**: 300 s

### FT-N-02: Corrupted download is detected and never delivered as a good file [v1.0]

**Summary**: Download tampering is caught by the client.
**Traces to**: AC-7 (negative case)
**Category**: Integrity

**Preconditions**:
- Proxy flips 1 bit in one downloaded block; then in every attempt.

**Input data**: 100 MiB file; browsers Chrome, Firefox, Safari; desktop daemon.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Desktop daemon downloads with one corrupted block | Block rejected, re-fetched, final hash equals source |
| 2 | Corrupt every attempt | Daemon reports an integrity error; final path holds no new/partial file (previous version intact) |
| 3 | Each browser downloads through the persistent corruption | Page shows an integrity error after download; success is not shown |

**Expected outcome**: 0 corrupted files accepted as complete across daemon and 3 browsers.
**Max execution time**: 300 s

### FT-N-03: A file edited during browser upload restarts the upload [v1.0]

**Summary**: A changed lastModified/content during upload is detected; the stored file is not a mix.
**Traces to**: AC-50 (negative case)
**Category**: Web client

**Preconditions**:
- Upload of a 5 GiB file running in each browser.

**Input data**: big-file 5 GiB edited at 40% (append 1 MiB and touch).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Edit the source file mid-upload | Page detects the change |
| 2 | Let it proceed | Upload restarts from block 0 under a new session |
| 3 | After commit, hash the server file | Equals the edited file's hash |

**Expected outcome**: detection in 3 of 3 browsers; final hash = edited file; 0 mixed files.
**Max execution time**: 900 s per browser

### FT-N-04: Creating `a.txt` beside `A.txt` never overwrites it [v1.0]

**Summary**: Case-differing names are not allowed to coexist or overwrite.
**Traces to**: AC-42 (negative case), RESTRICT-07
**Category**: Naming

**Preconditions**:
- `A.txt` exists with hash H1.

**Input data**: tricky-names `A.txt`, `a.txt` (RR Sync #7).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create `a.txt` (different content) via API and web UI | Rejected (e.g. 409) or renamed |
| 2 | Upload `a.txt` as a new file without a base revision | Same |
| 3 | Fetch `A.txt` | Same ID, name and hash H1 |

**Expected outcome**: `A.txt` untouched (exact); listing names are pairwise distinct case-insensitively.
**Max execution time**: 20 s

### FT-N-05: Desktop with local `A.txt` and `a.txt` (Linux) loses neither file [v1.0]

**Summary**: A case-sensitive local filesystem cannot create a case collision on the server.
**Traces to**: AC-42 (negative case)
**Category**: Naming

**Preconditions**:
- Linux ext4 sync folder containing both files with different content.

**Input data**: tricky-names case pair.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Sync the folder | First file synced under its name |
| 2 | Inspect server and the daemon report | Second file renamed on server (content kept) or reported as skipped |
| 3 | Hash local files | Unchanged |

**Expected outcome**: 0 files overwritten; local files unchanged; the second file is renamed or reported (never silently dropped).
**Max execution time**: 60 s

### FT-N-06: Hung ffmpeg is killed within 90 s and retried once on CPU [v1.0]

**Summary**: A stalled transcode does not block the queue.
**Traces to**: AC-45 (negative case)
**Category**: Video

**Preconditions**:
- Test ffmpeg shim hangs (no progress lines) on one designated clip on the first run only.

**Input data**: clip-mixed-100 with one hung job (RR Video #7).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload all 100 clips | Jobs start |
| 2 | Note time of the hung job's last progress line; watch the ffmpeg process | Process gone <= 90 s later |
| 3 | Watch job state | Retried exactly once on CPU; output valid |
| 4 | Variant: shim hangs on both runs | Job ends failed after 2 runs; original intact; no third run |

**Expected outcome**: kill <= 90 s after last progress; retry count = 1; 99 other jobs complete; variant ends failed with original hash unchanged.
**Max execution time**: 3600 s

### FT-N-07: Undecodable or hostile media fails the job without losing the file [v1.0]

**Summary**: Bad media gives a failed derivative, not a lost or corrupted original.
**Traces to**: AC-10, AC-25 (negative case)
**Category**: Photos

**Preconditions**:
- Worker running.

**Input data**: redteam-media: truncated HEIC, SVG renamed .png, 30000x30000 PNG.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the three files | Uploads succeed |
| 2 | Poll thumbnails | No thumbnail; job state failed for each |
| 3 | Download originals | Hash equals source |
| 4 | Upload a normal 12 MP JPEG | Thumbnail ready <= 5 s |

**Expected outcome**: 3 of 3 jobs failed, 0 originals altered, worker alive and next thumbnail <= 5 s.
**Max execution time**: 180 s

### FT-N-08: NVENC missing or failing falls back to CPU; other vendors not used; default is off [v1.1]

**Summary**: GPU problems never produce a failed or invalid result.
**Traces to**: AC-28, RESTRICT-04, RESTRICT-14
**Category**: Video

**Preconditions**:
- GPU option enabled; then GPU removed; separate worker with Intel/AMD GPU only.

**Input data**: clip-1080-sdr.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Remove the NVIDIA GPU, restart worker (test encode fails) | Worker logs probe failure; uses CPU |
| 2 | Inject an NVENC job failure mid-job | Job retried on CPU |
| 3 | Worker with Intel/AMD GPU, option on | libx264 used; no VAAPI/QSV |

**Expected outcome**: 3 of 3 runs yield valid H.264 High 1080p AAC MP4 via CPU; 0 failed jobs.
**Max execution time**: 900 s

### FT-N-09: Revoked, expired and nonexistent share links answer identically [v1.0]

**Summary**: Revocation is quick and the three dead-link states are indistinguishable.
**Traces to**: AC-12 (negative case)
**Category**: Sharing

**Preconditions**:
- Link L1 revoked, link L2 expired (clock control), token T3 never issued.

**Input data**: one file; 3 tokens (RR Sharing #2, #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Revoke L1; poll it every 500 ms | HTTP 404 within 5 s of revoke |
| 2 | Request L1, L2, T3 1,000 times each | Same status (404), same body bytes, same header names and values (excluding Date) |
| 3 | Compare median response time per token | Max pairwise difference of medians <= 1 ms |

**Expected outcome**: revoked effective <= 5 s; status, body, headers identical (exact); median timing delta <= 1 ms.
**Max execution time**: 120 s

### FT-N-10: Wrong share password returns no content [v1.0]

**Summary**: A password-protected link never leaks bytes to a wrong guess.
**Traces to**: AC-12 (negative case)
**Category**: Sharing

**Preconditions**:
- Link with password "correct-horse".

**Input data**: passwords "wrong", empty. Rate-limit (429) behavior is covered by the security tests (AC-20).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Submit wrong and empty passwords | Non-2xx; 0 file bytes |
| 2 | Submit the right password | 200 with original bytes |

**Expected outcome**: 0 content bytes returned for wrong inputs; correct input returns hash-equal bytes.
**Max execution time**: 20 s

### FT-N-11: Hostile file types are always served as attachments with `nosniff` and sandbox CSP [v1.0]

**Summary**: Uploaded html, svg, xml, js, pdf and a polyglot never render inline.
**Traces to**: AC-47 (negative case)
**Category**: Content serving

**Preconditions**:
- Each file uploaded and shared.

**Input data**: uploads-hostile: html, svg, xml, js, pdf (not on the inline allowlist), polyglot GIF/HTML (RR Sharing #5).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Request each file by authenticated download and by share link (12 requests) | `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff`, `Content-Security-Policy` containing `sandbox` |
| 2 | Navigate headless Chromium to each URL | File downloads; no page rendered; marker `window.__pwned` undefined |

**Expected outcome**: 12 of 12 attachment responses; 0 inline renders; 0 script executions.
**Max execution time**: 120 s

### FT-N-12: Stolen disk, missing or wrong master key never yields plaintext or wrong bytes [v1.0]

**Summary**: Without the correct key the data is unreadable and the server fails closed.
**Traces to**: AC-43 (negative case), RESTRICT-08
**Category**: Encryption at rest

**Preconditions**:
- 50 encrypted files with known markers.

**Input data**: blob volume and database dump copied; server started with no key and with a random wrong 32-byte key.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Search copied blobs and dump for markers | 0 hits |
| 2 | Start server with no master key | Exits non-zero (or refuses requests); no content served |
| 3 | Start server with the wrong key and download all files | No 200 with wrong bytes; each fails with 5xx/4xx |
| 4 | Hash all blob files before and after | Unchanged |

**Expected outcome**: 0 plaintext hits; 0 downloads returning bytes != original; blob files unmodified.
**Max execution time**: 180 s

### FT-N-13: Vault rejects wrong passphrase and tampered, truncated, reordered or wrong-vault ciphertext [v1.2]

**Summary**: The client refuses bad ciphertext and writes no output.
**Traces to**: AC-13 (negative case)
**Category**: Vault

**Preconditions**:
- Vault file V uploaded (vault-file).

**Input data**: wrong passphrase; ciphertext with one flipped byte; truncated by 1 byte and by one chunk; two chunks swapped; ciphertext moved from another vault (RR Vault #3).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Try to unlock with the wrong passphrase | Rejected with clear error |
| 2 | Download each of the 5 corrupted ciphertext variants and decrypt | Each rejected with a clear error |
| 3 | Check the output location | 0 output bytes written |

**Expected outcome**: 6 of 6 rejected; 0 output files.
**Max execution time**: 120 s

### FT-N-14: Vault refuses a 101 MiB file with a clear message [v1.2]

**Summary**: The default size limit is enforced.
**Traces to**: AC-14 (negative case)
**Category**: Vault

**Preconditions**:
- Default limit (100 MiB).

**Input data**: vault-file 101 MiB (RR Vault #4).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload the file into the vault | Rejected, message names the 100 MiB limit |
| 2 | Check vault contents and storage | 0 new files, 0 new blobs |

**Expected outcome**: rejection with message containing "100 MiB"; vault unchanged.
**Max execution time**: 60 s

### FT-N-15: Swapping wrapped keys or ciphertext is rejected in 1,000 of 1,000 attempts; unknown format versions are refused [v1.2]

**Summary**: Ciphertext and keys are bound to vault and file; a move inside the vault does not break decryption.
**Traces to**: AC-60 (negative case)
**Category**: Vault

**Preconditions**:
- Several vaults and folders with vault files.

**Input data**: vault-file; 1,000 randomised swaps of wrapped key plus ciphertext between files and vaults (RR Vault #5), plus 100 moves of a file between folders of the same vault.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Perform each swap on server-side stored objects and open the file on the client | Client rejects |
| 1b | Move a vault file to another folder of the same vault (tree change only) and open it | Opens, content hash equal |
| 2 | Set the format version byte to unknown values (current + 1, 0xFF) | Rejected |

**Expected outcome**: 1,000 of 1,000 swaps rejected; unknown versions rejected (2 of 2).
**Max execution time**: 600 s

### FT-N-16: Out-of-bounds or altered vault headers are rejected [v1.2]

**Summary**: Header authentication and KDF bounds hold at their edges.
**Traces to**: AC-61 (negative case)
**Category**: Vault

**Preconditions**:
- Client built with the shared crypto core.

**Input data**: vault-fuzz (10,000 mutated headers); boundary headers: memory 63/64/512/513 MiB, passes 2/3, parallelism 0/1/2, chunk size 64 KiB-1/64 KiB/4 MiB/4 MiB+1 (RR Vault #6).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Open 10,000 mutated headers | 0 accepted |
| 2 | Open each boundary header | 64 MiB, 512 MiB, 3 passes, parallelism 1, 64 KiB and 4 MiB accepted; all others rejected |

**Expected outcome**: 0 of 10,000 fuzzed headers accepted; boundary table matches exactly.
**Max execution time**: 300 s

### FT-N-17: Missing, unmounted or empty sync root pauses deletes [v1.0]

**Summary**: A vanished root never turns into mass deletion on the server.
**Traces to**: AC-29 (negative case), RESTRICT-24
**Category**: Deletion safety

**Preconditions**:
- Folder with 300 synced files.

**Input data**: sync root (RR Deletion #1): (a) root directory deleted, (b) volume unmounted, (c) all files removed leaving an empty root.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Cause each condition in turn (restore between) | Daemon shows the hold and asks the user |
| 2 | Check the server and other devices | 300 of 300 files present; 0 deletions propagated |
| 3 | Restore the root, confirm | Daemon resumes; no files lost |

**Expected outcome**: 0 deletions propagated in 3 of 3 conditions until confirmed.
**Max execution time**: 180 s

### FT-N-18: Deleting 199 files is not held [v1.0]

**Summary**: The mass-delete guard triggers at 200, not below.
**Traces to**: AC-29 (boundary case)
**Category**: Deletion safety

**Preconditions**:
- 300 synced files.

**Input data**: 300 files, delete 199 at once.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Delete 199 files on A | No prompt; deletions propagate |
| 2 | Check server and B | 101 files remain |

**Expected outcome**: 199 deletions propagated without prompt; 101 files remain.
**Max execution time**: 60 s

### FT-N-19: Hostile server-supplied names cause 0 writes outside the root and 0 overwrites [v1.0]

**Summary**: The 40-case hostile-name corpus is neutralised on each desktop OS.
**Traces to**: AC-37 (negative case)
**Category**: Desktop security

**Preconditions**:
- Sentinel files with known hashes placed at the traversal targets, outside the root and inside it.

**Input data**: tricky-names: `..` segments, alternate data stream names, Windows reserved names, bidi characters, 300-character path, symlink and junction pointing outside the root, case collisions; 40 cases per OS generated from these categories ('Thresholds added' #11).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | A mock sync server that bypasses the real server's name validation provides each hostile name; daemon syncs | A name the OS cannot represent is saved under a `%XX`-escaped name and reported; no entry written outside the root |
| 2 | Diff the whole filesystem outside the root; hash sentinels | 0 new or changed files outside the root |
| 3 | Check that safe files in the same sync completed | Safe files synced |

**Expected outcome**: 0 writes outside the root and 0 overwritten files for 40 of 40 cases on Windows and Linux; every unrepresentable name appears in the daemon report.
**Max execution time**: 300 s per OS

### FT-N-20: Social login without the setup token creates no account [v1.0]

**Summary**: A fresh install cannot be claimed by whoever reaches it first.
**Traces to**: AC-23 (negative case), RESTRICT-18
**Category**: Bootstrap

**Preconditions**:
- Empty database.

**Input data**: empty install, oauth-mock valid identity (RR Auth #1).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Complete a social login with no setup token | Refused with an explanation |
| 2 | Check accounts with `sunnycloud admin user list` | 0 accounts |

**Expected outcome**: 0 accounts and 0 sessions created.
**Max execution time**: 20 s

### FT-N-21: Reused setup token, reused invite, and email-based matching are refused [v1.0]

**Summary**: One-time tokens are one-time; email never grants access.
**Traces to**: AC-23 (negative case), RESTRICT-18
**Category**: Bootstrap

**Preconditions**:
- Setup token and one invite already redeemed; an existing user with email e@x.test.

**Input data**: oauth-mock identities.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Reuse the setup token | Rejected |
| 2 | Redeem the used invite with another identity | Rejected |
| 3 | New identity logs in with verified email e@x.test, no invite | No account; not linked to the existing user |

**Expected outcome**: 0 new accounts from 3 attempts; existing user's identities unchanged.
**Max execution time**: 30 s

### FT-N-22: Unverified provider email never creates or links an account [v1.0]

**Summary**: A provider that does not verify email cannot be used to take over an account.
**Traces to**: AC-55, RESTRICT-18
**Category**: Authentication

**Preconditions**:
- Victim account exists with email v@x.test; open invite exists.

**Input data**: oauth-mock returning v@x.test unverified (RR Auth #2).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Attacker logs in with a new provider id and the unverified victim email | No account created |
| 2 | Attacker attempts to link this identity to the victim | No link made |
| 3 | Attacker redeems an invite matched to v@x.test | Rejected |

**Expected outcome**: 0 accounts created, 0 identities linked; victim account unchanged.
**Max execution time**: 30 s

### FT-N-23: Invalid OAuth callbacks are all rejected [v1.0]

**Summary**: Each broken callback ends with no session and no account.
**Traces to**: AC-56
**Category**: Authentication

**Preconditions**:
- oauth-mock reachable; clean cookie jar per case.

**Input data**: oauth-mock invalid callbacks (RR Auth #3): missing state; replayed state; foreign state; wrong provider; wrong nonce; wrong audience; wrong issuer; absolute return URL; foreign Host header; reused one-time code; wrong code verifier; expired state (12 cases, all required).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Run each case | Non-2xx, no session cookie, no redirect to the absolute URL |
| 2 | Count accounts after all cases | Unchanged |

**Expected outcome**: 12 of 12 cases rejected; 0 sessions, 0 accounts, 0 off-site redirects.
**Max execution time**: 120 s

### FT-N-24: Device tokens cannot call privileged endpoints; wrong or expired codes grant nothing [v1.0]

**Summary**: A sync-only device token cannot approve devices, create shares, delete accounts or link providers.
**Traces to**: AC-57, RESTRICT-19
**Category**: Authentication

**Preconditions**:
- Device paired; valid device token T.

**Input data**: device token (RR Auth #5).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Call approve-device, create-share, delete-account, link-provider with T | HTTP 403 each |
| 2 | Call a sync endpoint with T | 200 |
| 3 | Enter a wrong user code; enter a code after 10 min | Rejected; 0 tokens granted |

**Expected outcome**: 4 of 4 privileged calls return 403; sync call succeeds; 0 grants from wrong/expired codes.
**Max execution time**: 660 s

### FT-N-25: Server with no sign-in provider configured does not start [v1.0]

**Summary**: At least one provider is required.
**Traces to**: AC-19 (negative case)
**Category**: Authentication

**Preconditions**:
- Compose stack with all providers unset.

**Input data**: empty provider config.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start the API | Exits non-zero within 30 s; port not listening; log names missing provider (text: Gap G-2) |

**Expected outcome**: exit code != 0; no listener on 443/API port.
**Max execution time**: 30 s

### FT-N-26: Mobile apps refuse plain-HTTP sign-in by default [v1.0 Android, v1.1 iOS]

**Summary**: Credentials and codes never travel over cleartext.
**Traces to**: AC-54, RESTRICT-22
**Category**: Mobile security

**Preconditions**:
- Server reachable at `http://<host>:80` with TLS off; default app settings.

**Input data**: mobile app (RR Auth #16).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Enter `http://<host>` as the server address and sign in | Refused with a message that HTTPS is required |
| 2 | Capture traffic | 0 cleartext requests carrying tokens or codes |

**Expected outcome**: sign-in refused; 0 cleartext auth requests.
**Max execution time**: 60 s

### FT-N-27: Old client gets a clear upgrade message, not corrupt behavior [v1.0]

**Summary**: A client below the minimum supported version is stopped cleanly.
**Traces to**: AC-64
**Category**: API

**Preconditions**:
- Server minimum client version V; client reporting V-1.

**Input data**: old client (RR Auth #17).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Old client calls sync and upload endpoints | Non-2xx with a machine-readable error and a message containing "upgrade" (case-insensitive; exact text: Gap G-19) |
| 2 | Check server state | 0 files created or modified by the old client |

**Expected outcome**: 2 of 2 calls refused with an upgrade message; 0 state changes.
**Max execution time**: 20 s

### FT-N-28: Facebook sign-in does not exist [v1.0]

**Summary**: The out-of-scope provider is absent everywhere.
**Traces to**: RESTRICT-04
**Category**: Release scoping

**Preconditions**:
- Any release build.

**Input data**: sign-in page, provider list API (Gap G-15), operator config with a `facebook` provider block.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Inspect the sign-in pages (web, Android, desktop approval page) | No Facebook option |
| 2 | Start the server with a `facebook` provider configured | Rejected as an unknown provider |

**Expected outcome**: 0 Facebook entries in any UI or provider list; config with Facebook refused.
**Max execution time**: 60 s

### FT-N-29: No local password sign-in exists [v1.0]

**Summary**: Only OAuth/OIDC sign-in is possible.
**Traces to**: RESTRICT-17
**Category**: Authentication

**Preconditions**:
- Running server with an admin.

**Input data**: web sign-in page; `sunnycloud admin --help`.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Count password inputs on the sign-in page and in account settings | 0 |
| 2 | Post username/password to any documented or guessed sign-in path | Non-2xx; no session cookie |
| 3 | Read `sunnycloud admin --help` | No password option |

**Expected outcome**: 0 password fields; 0 sessions from password attempts.
**Max execution time**: 30 s

### FT-N-30: With no internet access new sign-ins fail clearly and existing sessions keep working [v1.0]

**Summary**: An isolated server refuses new provider sign-ins without a fallback, yet does not cut sessions already issued.
**Traces to**: RESTRICT-20, AC-21
**Category**: Authentication

**Preconditions**:
- Running install with a web session and a paired desktop daemon (device token); then egress to the provider blocked at the network level.

**Input data**: oauth-mock unreachable (RR "Thresholds added in the second gap round" #4).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Attempt a new sign-in | Provider call times out at 10 s; clear error shown within 12 s; no session; no local fallback |
| 2 | Use the existing web session (list folder, refresh the session) | Succeeds (HTTP 200) |
| 3 | Edit a file on the paired desktop device | Syncs to the server <= 5 s |

**Expected outcome**: 0 sessions created by new sign-ins; existing web session and device token keep working (3 of 3 calls succeed).
**Max execution time**: 120 s

### FT-N-31: Out-of-scope features are absent [v1.0]

**Summary**: No version history, user trash, cross-user dedupe, streaming, placeholder files, Android file browser or folder share links.
**Traces to**: RESTRICT-24, RESTRICT-10, RESTRICT-28
**Category**: Scope

**Preconditions**:
- Two accounts; one Windows desktop; one Android emulator with the v1.0 app.

**Input data**: a file edited 3 times; two accounts uploading the same 100 MiB file; clip-compliant.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Edit a file 3 times; list versions via API and web UI | No version list; OpenAPI has no version/history operation; exactly 1 current version |
| 2 | Delete a file | No "Trash"/restore control in UI; deleted file ID returns 404 to the owner |
| 3 | Both accounts upload the identical 100 MiB file | 2 distinct blob files; blob volume grows by ~200 MiB (>= 1.9x single) |
| 4 | Upload a video | Only an MP4 download is offered; no HLS/DASH manifest or streaming route |
| 5 | Sync 100 files to Windows desktop | All fully hydrated (allocated size = file size); no cloud-file placeholders |
| 6 | Walk every screen of the Android app | Sign-in, backup status, settings, permission and battery help, about only; no file browser, download or share screen (file browsing and download on Android are out of v1.0) |
| 7 | Create a share link for a folder through the API | Rejected (share links target files only in v1.0); no link row created |

**Expected outcome**: each of 7 out-of-scope features absent (exact).
**Max execution time**: 300 s

### FT-N-32: Transport accepts only TLS 1.3 over HTTP/2; no HTTP/3 and no cleartext API [v1.0]

**Summary**: Transport matches the protocol restriction.
**Traces to**: RESTRICT-11, RESTRICT-22, RESTRICT-32
**Category**: Transport

**Preconditions**:
- Default Compose with Caddy.

**Input data**: TLS handshake probes; plain HTTP request; UDP 443 probe.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Handshake with TLS 1.2 only, then TLS 1.3 | 1.2 refused; 1.3 succeeds with ALPN `h2` |
| 2 | Read response headers for `Alt-Svc`; probe UDP 443 | No `h3` advertised; no QUIC listener |
| 3 | Request `http://<host>/api/v1/...` | Redirect to HTTPS or refusal; never 200 with API data |
| 4 | Read response headers of any HTTPS response | `Strict-Transport-Security` with max-age=31536000 |

**Expected outcome**: all 4 probes as listed (exact).
**Max execution time**: 30 s

### FT-N-33: Restoring without the master key leaves data unreadable and says so [v1.0]

**Summary**: Master-key loss is unrecoverable and the product fails closed.
**Traces to**: AC-43, AC-65, RESTRICT-08, RESTRICT-31
**Category**: Operations

**Preconditions**:
- backup-set restored on a clean host with database and blobs but no master key.

**Input data**: backup-set (RR Auth #18).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start the server | Refuses to start or refuses to serve content |
| 2 | Attempt downloads; run `sunnycloud key-check` | 0 successful downloads of wrong or partial plaintext; `key-check` exits 2 and prints no key bytes |

**Expected outcome**: 0 plaintext bytes obtained; clear operator-facing error.
**Max execution time**: 120 s

### FT-N-34: iOS app hides Google and GitHub sign-in when Sign in with Apple is not enabled [v1.1]

**Summary**: App Store guideline 4.8 compliance.
**Traces to**: AC-19 (negative case), RESTRICT-21
**Category**: Authentication

**Preconditions**:
- Server with GitHub and Google (OIDC) enabled, Apple disabled; iOS app.

**Input data**: oauth-mock.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Open sign-in in the iOS app | No Google and no GitHub button |
| 2 | Enable Apple on the server and reopen | Apple, Google and GitHub appear |

**Expected outcome**: 0 Google/GitHub buttons without Apple; 3 buttons with Apple.
**Max execution time**: 60 s

### FT-N-35: Clock-control endpoint exists only in the test image [v1.0]

**Summary**: Release builds cannot be driven through time.
**Traces to**: AC-30 (negative case)
**Category**: Operations

**Preconditions**:
- One `-test` image server and one release-image server, same config.

**Input data**: `POST /test/clock` on `:9091` with body `{"advance_seconds": 691200}` (8 days) (RR "Thresholds added in the second gap round" #5); `GET /internal/v1/test/clock-offset` on `:8081`, the endpoint the worker supervisor uses to read the same offset.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Send the request to the test image | Accepted (2xx); FT-P-16 day-8 behavior observable |
| 2 | Send the same request to the release image, with and without an admin session | HTTP 404 in both cases; server time unchanged (a share created before and after has the same expiry offset) |
| 3 | On the test image, advance the clock, then read `GET /internal/v1/test/clock-offset` with the worker credential and without it | Offset equals the advance with the worker credential; refused without it; the supervisor's job lease times follow the same offset |
| 4 | `GET /internal/v1/test/clock-offset` on the release image | HTTP 404 |

**Expected outcome**: control works on test image (1 of 1); release image returns 404 for 3 of 3 requests (2 `POST`, 1 `GET`).
**Max execution time**: 30 s

### FT-N-36: Android session token is excluded from platform backups [v1.0]

**Summary**: A device backup or transfer never carries the sign-in token.
**Traces to**: RESTRICT-33
**Category**: Mobile security

**Preconditions**:
- Android phone signed in; token value known to the harness; mobile sign-in yields a `sessions` row, so the session is visible in the server session list.

**Input data**: phone-fixtures (Pixel).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create a full app-data backup with the platform tooling (`adb backup` / `bmgr` local transport) | Backup archive produced |
| 2 | Search the archive for the token string and its base64/hex forms | 0 occurrences |
| 3 | Restore the archive to a second phone and open the app | App is signed out; the `sessions` row of the first phone is not usable from the second |

**Expected outcome**: 0 token occurrences; restored app requires sign-in.
**Max execution time**: 300 s

### FT-N-37: Vault passphrase must be at least 12 characters with zxcvbn score 4, or be generated [v1.2]

**Summary**: Short or weak vault passphrases are refused at vault creation.
**Traces to**: RESTRICT-33, RESTRICT-22, AC-14
**Category**: Vault

**Preconditions**:
- Signed-in account; no vault yet.

**Input data**: an 11-character passphrase; a 12-character passphrase with zxcvbn score 3; a 12-character passphrase with zxcvbn score 4; a passphrase from the client's generator (6 words) (the two 12-character strings are chosen and checked against the reference zxcvbn implementation before the run). The rule is the one in `restrictions.md`, `19_vault_core` and ADR-007; RR "Thresholds added in the second gap round" #6 (scores 2 and 3) and gap decision 25 (score 3) are stale.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create a vault with the 11-character passphrase | Rejected, message names the 12-character minimum |
| 2 | Create a vault with the score-3 passphrase | Rejected by the strength check |
| 3 | Create a vault with the score-4 passphrase, and with the generated passphrase | Created (2 of 2) |

**Expected outcome**: 2 of 2 weak passphrases rejected; 2 of 2 acceptable ones accepted; 0 vaults created by the rejected attempts.
**Max execution time**: 60 s

### FT-N-38: A retried batch commit is idempotent by commit ID [v1.0]

**Summary**: Losing the response of a batch commit and retrying does not duplicate files.
**Traces to**: RESTRICT-32, AC-40
**Category**: Sync

**Preconditions**:
- Fault proxy can drop a response after the server has processed the request.

**Input data**: 100 files from many-small in one batch with client commit ID C.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Send the batch commit; proxy drops the response | Client sees a network error |
| 2 | Client retries the identical batch with commit ID C | Success response equal to what a first success would return |
| 3 | Replay the same request a third time by hand | Same response body |
| 4 | List the folder | Exactly 100 new entries |

**Expected outcome**: 100 entries, 0 duplicates, 0 conflict copies, each file revision count = 1; 3 identical responses for C.
**Max execution time**: 60 s

### FT-N-39: An interrupted upgrade rolls back and the server refuses to start until it is re-run [v1.0]

**Summary**: A killed migration leaves the data intact and the server stopped, not half-migrated.
**Traces to**: AC-65 (negative case), RESTRICT-32, RESTRICT-31
**Category**: Operations

**Preconditions**:
- Installation at release N with backup-set data; release N+1 images available.

**Input data**: backup-set (RR Auth #18).

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start the documented upgrade to N+1 and `kill -9` the migration at about 50% of its normal duration | Migration aborted |
| 2 | Start the N+1 server | Refuses to start with non-zero exit and an error telling the operator to re-run the upgrade (text: Gap G-2) |
| 3 | Re-run the documented upgrade step, then start N+1 | Completes; server starts |
| 4 | Download every file and hash it | Hash set equals the recorded set |

**Expected outcome**: step 2 exit code != 0 and no listener; after step 3 100% of files present with equal hashes.
**Max execution time**: 1200 s

### FT-N-40: Abandoned upload sessions expire after 24 h and release quota; orphan blobs are swept [v1.0]

**Summary**: An upload that is never finished cannot hold quota or disk forever.
**Traces to**: RESTRICT-32, RESTRICT-09
**Category**: Upload lifecycle

**Preconditions**:
- Clock control; account quota known; operator read access to blob volume.

**Input data**: big-file 5 GiB, upload stopped at 50%.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Start the upload, abandon at 50%; read quota used (includes the reservation) | Reservation visible |
| 2 | Advance 23 h; resume the upload | Resume accepted |
| 3 | Abandon again; advance to 25 h after the last activity; try to resume | Session expired (client error); quota used back to the value before step 1 |
| 4 | Advance until 49 h after last activity, run the sweep | No blob file left on disk for that session |
| 5 | With the account at its quota, start an upload larger than the remaining quota | HTTP 507 with error code `quota_exceeded`; no session and no reservation created; the desktop and Android clients show a quota message and pause instead of retrying |

**Expected outcome**: resume works at 23 h, fails at 25 h; reservation released; 0 orphan blob files after 49 h; over-quota start answers 507 `quota_exceeded`.
**Max execution time**: 120 s

### FT-N-41: Server rejects unsafe names and accepts all others [v1.0]

**Summary**: Name validation at the API follows the confirmed rule.
**Traces to**: AC-37 (negative case), AC-42
**Category**: Naming

**Preconditions**:
- Empty folder.

**Input data**: tricky-names. Rejected set: `a/b`, name containing NUL, `.`, `..`, `a<U+0007>b` (control), `a<U+202E>b` and `<U+2066>x` (bidi control). Accepted set: `CON`, `aux.txt`, `a.txt:stream`, `trailing.`, `Report.TXT`.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Create a file with each name in the rejected set via API and web UI | 4xx for all 7 |
| 2 | Create a file with each name in the accepted set | 2xx for all 5 |
| 3 | List the folder | Exactly the 5 accepted names, case and characters preserved; none of the rejected names |

**Expected outcome**: 7 of 7 rejected, 5 of 5 accepted, listing equals the accepted set (exact).
**Max execution time**: 30 s

### FT-N-42: A folder move racing an edit inside it keeps the edit, in the moved folder, on the same file ID [v1.0]

**Summary**: Device A moves a folder while device B edits a file inside it; the edit is not lost, not duplicated and not turned into a conflict copy.
**Traces to**: AC-70, RESTRICT-24
**Category**: Sync races
**Env**: C (API clients A and B with different device labels; the last step uses `desktop-linux-a/-b`, profile `desktop`), group P

**Preconditions**:
- One account with two device tokens (labels `dev-a`, `dev-b`); folders `/src/d1` to `/src/d200`, each holding `file.txt` (revision R1); an empty folder `/dst`.

**Input data**: 200 files of 64 KiB from the seeded stream (R1), and for each a second content E_i of 64 KiB (the edit), BLAKE3 known from the generator. Seed fixes the per-round offset between the two calls, -20 to +20 ms.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | For each round i, release together: A `PATCH /api/v1/nodes/{d_i}` with `parent_id = /dst`; B uploads E_i (`PUT /api/v1/staged-uploads/...`) and commits it to the node `file.txt` of `d_i` with `base_revision = R1` (`commits:batch`, `Target::Existing`) | Both answer 2xx; the commit outcome is `Updated` (never `ConflictCopied`); no `name_conflict` and no `edited_since` |
| 2 | `GET /api/v1/nodes/{file_i}` | Same node id as before the round; the parent chain resolves to `/dst/d_i`; current revision hash equals E_i |
| 3 | List `/src` and `/dst/d_i` | `/dst/d_i` holds exactly 1 entry, `file.txt`; no name containing `(conflict copy` in either listing; `d_i` is absent from `/src` |
| 4 | Read `GET /api/v1/changes` from the cursor taken before the round | Both `d_i` (new parent) and `file_i` (new revision) appear, each once, at their latest `seq` |
| 5 | Desktop variant, 10 rounds: on `desktop-linux-a` move the folder inside the sync root while on `desktop-linux-b` the file inside is rewritten with new content; wait for convergence (at most 60 s) | Both sync roots hold `/dst/d_i/file.txt` with the new content, `d_i` is gone from `/src`, 0 conflict copies, server file id unchanged |

**Expected outcome**: 200 of 200 API rounds end with the edit inside the moved folder on the unchanged file ID and 0 conflict copies; 10 of 10 desktop rounds converge to the same state; calls failing or answering 5xx = 0.
**Max execution time**: 600 s

### FT-N-43: A worker fetch of a 1 GiB blob equals the plaintext, and a server with a non-matching master key refuses to start [v1.0]

**Summary**: The internal worker endpoint returns the exact decrypted bytes of a 1 GiB blob, and the key check stops a server that was given a different master key before it serves anything.
**Traces to**: AC-72, AC-43, RESTRICT-08, RESTRICT-13
**Category**: Encryption at rest / worker seam
**Env**: C, group X (stops the supervisor and the API; needs the `e2e-ctl` actions `internal-http` and alternate-key start of `environment.md`)

**Preconditions**:
- Encryption at rest on; the supervisor stopped with `docker stop worker-supervisor` so the test holds the only worker identity; `master_key_2` available as the wrong key.

**Input data**: clip-1gib (DS-36), exactly 1,073,741,824 bytes, whole-file BLAKE3 in the manifest.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload clip-1gib on 4 lanes; download it through the public API and hash it | Hash equals the manifest; a video job is queued |
| 2 | `internal-http`: `POST /api/v1/worker-jobs/claims` (`{lane: video}`), then `GET /api/v1/worker-jobs/{job}/input`, streamed into BLAKE3 inside ctl | 200; `Content-Length` 1,073,741,824; byte count 1,073,741,824; hash equals the manifest and equals step 1 |
| 3 | `DELETE` the lease; start the supervisor again | 204; the job runs normally |
| 4 | `docker stop api`; start the API image once with `master_key_2` mounted as the master key (same database and volume); ctl probes `:8080` and `:8081` every 100 ms from process start | The process exits non-zero within 30 s; no listener is ever observed; the log says the data is unreadable without the master key; no key bytes in the log |
| 5 | `sunnycloud key-check` with `master_key_2` | Exit code 2; no key bytes printed |
| 6 | Hash all blob files before step 4 and after step 5; then start the API with the correct key and download clip-1gib again | All blob hashes unchanged; service healthy; download hash equals the manifest |

**Expected outcome**: worker-fetched bytes = 1 GiB with hash equal (exact); wrong-key server listeners opened = 0; blob files modified = 0.
**Max execution time**: 900 s

### FT-N-44: A share link follows its file across rename and move, and answers as missing within 5 s of a delete [v1.0]

**Summary**: The link is bound to the file, not to its path, and dies with the file.
**Traces to**: AC-77, AC-12
**Category**: Sharing
**Env**: C, group P

**Preconditions**:
- One account; `share_lookup_cache` at its default (1 s).

**Input data**: small-fixtures (DS-23): two 1 MiB files f and h (distinct content), and a second 1 MiB content f2 for the in-place edit.

**Steps**:

| Step | Consumer Action | Expected System Response |
|------|----------------|------------------------|
| 1 | Upload f to `/a/b/f.txt` and h to `/a/b/h.txt`; create a link S for f and a link S2 for h; open S | 200; bytes hash equals f; metadata name `f.txt` |
| 2 | Rename f to `g.txt`, then to `G.txt` (case only) | S opens each time with the same bytes; metadata name follows the rename |
| 3 | Move f into `/c/d`, move `/c` into `/e`, rename `/a` to `/a2` | S opens after each step with the same bytes |
| 4 | Save f2 over f in place (same node id, new revision) | S serves the new bytes |
| 5 | Delete f; poll S every 250 ms | The first 404 arrives within 5 s of the delete response; the status, body and headers equal those of a never-issued token (FT-N-09); 40 further polls over 10 s stay 404 |
| 6 | Open S2 | Still 200 with the bytes of h |

**Expected outcome**: S valid after 3 renames, 2 moves and 1 in-place edit (7 of 7 opens correct); missing within 5 s of the delete (exact); S2 unaffected.
**Max execution time**: 90 s

## Gaps

Resolved by the owner decisions of 2026-11-03 (rounds 1 and 2) and removed; numbers of open gaps are kept stable for traceability. Round 1: former G-4, G-13, G-17, G-18 (G-10 narrowed). Round 2: former G-3 (orientation, SSIM >= 0.95), G-6 (Samsung/Xiaomi p95 <= 5 min), G-11 (pattern grammar, AC-66), G-14 (2 GiB browser verification), G-16 (macOS permission, AC-67), G-20 (existing sessions), G-22 (`POST /test/clock`), G-23 (zxcvbn; the final rule is score 4 or generated, see FT-N-37). Former G-8 is not a gap: FT-P-33 (Live Photo) is DEFERRED to v1.1 and its fixture and expected result are written at v1.1 planning.

| Gap | Affects | Needed data / expected result |
|-----|---------|-------------------------------|
| G-1 | FT-P-15, AC-42 | Expected handling of NFC vs NFD names (treated as the same name or different; normalisation form stored). |
| G-2 | FT-P-17, FT-N-25, FT-N-39 | Exact error text (or substring) for out-of-range retention, "no provider configured" and "upgrade must be re-run". |
| G-5 | FT-P-27, AC-45 | Expected result for audio-only clips in clip-mixed-100 (transcoded to AAC m4a, passed through, or no derivative). |
| G-7 | FT-P-31, AC-9 | Time bound for iOS foreground catch-up of a 100-photo backlog. |
| G-9 | FT-P-37, AC-12 | HTTP status/response shape for a password-protected link opened without a password. |
| G-10 | FT-P-39, FT-N-11, AC-47 | Whether inline eligibility is decided by the stored/declared MIME type, by content sniffing or by file name (e.g. HTML bytes named `.jpg`). |
| G-12 | FT-P-61, RESTRICT-19 | Required text of the Linux fallback warning and token file location. |
| G-15 | FT-P-70, FT-P-73, FT-N-28 | OpenAPI document URL, provider-list endpoint and names of vault/Apple operations used for absence checks. |
| G-19 | FT-N-27, AC-64 | Exact substring of the upgrade message, the mechanism used to report the client version, and the minimum version value. |
| G-21 | FT-P-17 | GC cycle interval or trigger command used to assert "removed in next cycle"; behavior above 50 GiB (AC-4) is also unspecified. |

The only still-blocked item is the NFC/NFD sub-case of FT-P-15 (G-1); every other scenario is runnable. Remaining non-runnable states: FT-P-30 and FT-P-75 report `NOT-RUN(HW-DEVICE)` when the phones or a macOS machine are missing; FT-P-33 is DEFERRED to v1.1.

## AC coverage index

| AC | Tests |
|----|-------|
| AC-3 | FT-P-01 |
| AC-4 | FT-P-01, FT-P-03, FT-P-67 |
| AC-5 | FT-P-07, FT-P-08 |
| AC-6 | FT-P-09 |
| AC-7 | FT-P-01, FT-P-02, FT-P-03, FT-P-68, FT-N-01, FT-N-02 |
| AC-8 | FT-P-08, FT-P-10, FT-P-11, FT-P-12, FT-P-77 |
| AC-9 | FT-P-29, FT-P-30, FT-P-31 |
| AC-10 | FT-P-20, FT-P-21, FT-P-22, FT-N-07 |
| AC-11 | FT-P-23, FT-P-24, FT-P-25 |
| AC-12 | FT-P-36, FT-P-37, FT-P-38, FT-N-09, FT-N-10, FT-N-44 |
| AC-13 | FT-P-44, FT-N-13 |
| AC-14 | FT-P-44, FT-P-45, FT-N-14, FT-N-37 |
| AC-16 | FT-P-48, FT-P-49 |
| AC-17 | FT-P-50 |
| AC-18 | FT-P-51, FT-P-52 |
| AC-19 | FT-P-62, FT-P-63, FT-N-25, FT-N-34 |
| AC-21 | FT-N-30 |
| AC-22 | FT-P-69 |
| AC-23 | FT-P-64, FT-P-65, FT-N-20, FT-N-21 |
| AC-24 | FT-P-43 |
| AC-25 | FT-N-07 |
| AC-26 | FT-P-53 |
| AC-27 | FT-P-19 |
| AC-28 | FT-P-28, FT-N-08 |
| AC-29 | FT-P-59, FT-N-17, FT-N-18 |
| AC-30 | FT-P-16, FT-P-17, FT-P-18, FT-N-35 |
| AC-31 | FT-P-54 |
| AC-32 | FT-P-55 |
| AC-33 | FT-P-56 |
| AC-34 | FT-P-57 |
| AC-35 | FT-P-58, FT-P-77 |
| AC-37 | FT-N-19, FT-N-41 |
| AC-38 | FT-P-06 |
| AC-40 | FT-P-04, FT-P-05, FT-N-38 |
| AC-41 | FT-P-13, FT-P-14 |
| AC-42 | FT-P-15, FT-N-04, FT-N-05, FT-N-41 |
| AC-43 | FT-P-40, FT-P-41, FT-P-42, FT-P-71, FT-N-12, FT-N-33, FT-N-43 |
| AC-44 | FT-P-25, FT-P-26 |
| AC-45 | FT-P-27, FT-N-06 |
| AC-46 | FT-P-32, FT-P-33 |
| AC-47 | FT-P-39, FT-N-11 |
| AC-50 | FT-P-67, FT-N-03 |
| AC-51 | FT-P-68 |
| AC-52 | FT-P-34 |
| AC-53 | FT-P-35 |
| AC-54 | FT-N-26 |
| AC-55 | FT-N-22 |
| AC-56 | FT-N-23 |
| AC-57 | FT-P-66, FT-N-24 |
| AC-60 | FT-P-46, FT-N-15 |
| AC-61 | FT-N-16 |
| AC-62 | FT-P-47 |
| AC-64 | FT-P-70, FT-N-27 |
| AC-65 | FT-P-71, FT-P-72, FT-N-33, FT-N-39 |
| AC-66 | FT-P-60 |
| AC-67 | FT-P-75 |
| AC-70 | FT-N-42 |
| AC-72 | FT-N-43 |
| AC-77 | FT-N-44 |
| AC-78 | FT-P-76 |

## Restriction coverage index

| Restriction | Tests |
|-------------|-------|
| RESTRICT-01 | FT-P-73 |
| RESTRICT-02 | FT-P-28, FT-P-63, FT-P-73, FT-P-75 |
| RESTRICT-03 | FT-P-73 |
| RESTRICT-04 | FT-P-73, FT-N-08, FT-N-28 |
| RESTRICT-05 | FT-P-70 |
| RESTRICT-06 | FT-P-48, FT-P-50, FT-P-60 |
| RESTRICT-07 | FT-P-15, FT-P-40, FT-N-04 |
| RESTRICT-08 | FT-P-40, FT-N-12, FT-N-33, FT-N-43 |
| RESTRICT-09 | FT-P-06, FT-P-13, FT-P-14, FT-N-40 |
| RESTRICT-10 | FT-P-16, FT-P-17, FT-P-19, FT-N-31 |
| RESTRICT-11 | FT-P-01, FT-P-04, FT-P-05, FT-P-70, FT-N-32 |
| RESTRICT-13 | FT-P-23, FT-P-25, FT-P-69, FT-P-74, FT-N-43 |
| RESTRICT-14 | FT-P-28, FT-P-73, FT-N-08 |
| RESTRICT-15 | FT-P-29, FT-P-34, FT-P-76 |
| RESTRICT-16 | FT-P-69 |
| RESTRICT-17 | FT-P-62, FT-N-29 |
| RESTRICT-18 | FT-P-64, FT-P-65, FT-N-20, FT-N-21, FT-N-22 |
| RESTRICT-19 | FT-P-61, FT-P-66, FT-N-24 |
| RESTRICT-20 | FT-N-30 |
| RESTRICT-21 | FT-P-63, FT-N-34 |
| RESTRICT-22 | FT-P-44, FT-N-26, FT-N-32, FT-N-37 |
| RESTRICT-23 | FT-P-44 |
| RESTRICT-24 | FT-P-10, FT-P-12, FT-P-55, FT-P-59, FT-N-17, FT-N-31, FT-N-42 |
| RESTRICT-25 | FT-P-53 |
| RESTRICT-26 | FT-P-75 |
| RESTRICT-27 | FT-P-31 |
| RESTRICT-28 | FT-N-31 |
| RESTRICT-29 | FT-P-74 |
| RESTRICT-31 | FT-P-71, FT-P-72, FT-N-33, FT-N-39 |
| RESTRICT-32 | FT-P-40, FT-N-32, FT-N-38, FT-N-39, FT-N-40 |
| RESTRICT-33 | FT-N-36, FT-N-37 |
