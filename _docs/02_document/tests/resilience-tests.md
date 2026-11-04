# Resilience Tests

Black-box specifications (no test code). Each scenario is a behavioral test: fault trigger, observable, quantifiable verdict. Data sets are those of `_docs/00_problem/input_data/data_parameters.md`; expected values come from `expected_results/results_report.md` (cited as "RR <section> #n"), the acceptance criteria, or the research reviews (cited as `b_server_storage` / `b_security` finding numbers, "research" in brackets when the value is not yet in an AC).

## Conventions

- **Harness access**: public HTTP API, the real clients (desktop daemon, Android, browsers), the operator CLI (`sunnycloud admin ...`, `admin restore`, `scrub run`, `doctor`), and operator-level inspection of the deployed stack (compose, blob volume read-only, Postgres read-only, container logs and process table). No test touches internal modules.
- **Faults** are injected from outside: `docker kill -s KILL|STOP|TERM`, `docker restart`, Toxiproxy or `tc netem` (reset, latency, partition), egress firewall rules, an external writer filling the volume, VM suspend or hard reset, process kill on clients.
- **T-CLOCK**: scenarios that need days or hours (retention 7 d, tombstone horizon 90 d, offline 91 d, reaper 24 h, orphan age 48 h) advance the server clock through the injectable clock of the test image (AC-30): `POST /test/clock` on `:9091` with `{"advance_seconds": N}` on the API; the supervisor reads the same offset through `GET /internal/v1/test/clock-offset` on `:8081`. Both exist only in the test image. Waits that are config values (job lease, GC interval, session expiry) are tested with short mock durations from config, and the scenario asserts the production default by reading the config value.
- **Hash-set compare**: map of path to BLAKE3 for a whole tree. "Converged" means client tree, server tree and every other device tree have equal hash-sets.
- **Status**: `READY` = pass criteria fully defined by an AC, restriction or results-report row (RR sections "Thresholds added after owner confirmation" included). `BLOCKED — missing data` = a data set or hardware item the verdict needs does not exist yet; none remain in this file.
- Values confirmed by the owner (gap-decisions.md) and used below: job lease 5 min with 3 attempts; upload-session reaper 24 h; orphan sweep age 48 h; provider call timeout 10 s; database recovery within 60 s; account deletion within 15 min; idempotent batch commit by client commit ID; interrupted upgrade rolls back and refuses to start until re-run; after a restore local-only files are uploaded, server-only files downloaded, nothing deleted except by the mass-delete guard.
- Releases: [v1.0] unless tagged otherwise.

---

### NFT-RES-01: Upload resumes after connection drop [v1.0]

**Traces to**: AC-3, AC-4, AC-7; RESTRICT-09, RESTRICT-11, RESTRICT-16 | **Data**: big-file 5 GiB | **Status**: READY (RR Transfer #1, #4)

**Fault injection**: (A) TCP reset on all client connections when 50% of bytes are acknowledged; (B) Caddy container restart at 25% and again at 75%; (C) full network partition for 120 s at 50%.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Inject fault during a 5 GiB upload on 4 lanes | Client keeps the upload session; no new session created; no error shown to the user |
| 2 | Restore network, let client resume | Client re-queries the received set and sends only missing blocks |
| 3 | Final commit | File listed; download BLAKE3 equals source |

**Pass criteria**: final BLAKE3 equals original (exact); bytes re-sent after each resume that were already acknowledged ≤ 10 MiB per fault; upload finish ≤ 2 s after the last block; exactly 1 blob file for the file on the volume; 0 duplicate or conflict-copy entries.

---

### NFT-RES-02: Download resumes after interruption [v1.0]

**Traces to**: AC-2, AC-7; RESTRICT-11 | **Data**: big-file 5 GiB | **Status**: READY (RR Transfer #3)

**Fault injection**: reset the connection at 50% of a download by the desktop daemon and by Chrome; repeat with the Range-capable server restarted (`docker restart api`).

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Interrupt the download | Partial data is kept as temp data; the destination file is not finalized |
| 2 | Resume | Client uses a Range request from the verified offset |
| 3 | Finish | BLAKE3 of the result is verified before the file is finalized |

**Pass criteria**: result hash equals original (exact); bytes fetched twice ≤ 10 MiB; 0 truncated files left under the final name.

---

### NFT-RES-03: `kill -9` during 100 concurrent uploads leaves no zeroed blocks [v1.0]

**Traces to**: AC-38, AC-3, AC-7; RESTRICT-07, RESTRICT-09 | **Data**: many-small plus 100 multi-block files of 64 MiB (seeded slices of big-file, G-R3) | **Status**: READY (RR Transfer #5)

**Fault injection**: (A) `docker kill -s KILL api` at a random time 0.5 to 10 s into the run, 10 independent runs; (B) in 5 of those runs, hard-reset the VM hosting the blob volume (power-loss emulation, G-R18) right after the kill; (C) `docker kill -s TERM api` (rolling restart) with the compose stop grace period.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | 100 clients upload concurrently (4 lanes each), record which commits returned 2xx | Baseline of acknowledged commits |
| 2 | Fault, then restart server | Server healthy; blob volume mounted |
| 3 | List all committed files and download each | Every committed file hash equals its source |
| 4 | Clients resume files that were not committed | Each resume re-sends ≤ 10 MiB; all 100 files committed correctly |

**Pass criteria**: committed files with a missing or zero-filled block = 0 in all runs and on both XFS and ext4 volumes (exact); every file acknowledged before the fault is present; for TERM: in-flight requests finish or abort cleanly, 0 corrupted files, resumes ≤ 10 MiB.

---

### NFT-RES-04: Crash between blob write and DB commit; orphan reconciliation sweep [v1.0]

**Traces to**: AC-3, AC-4, AC-24, AC-38; RESTRICT-07, RESTRICT-09, new restrictions bullet (reaper 24 h, orphan age 48 h) | **Data**: many-small, big-file 5 GiB | **Status**: READY (restrictions: sessions expire after 24 h, orphan blobs older than 48 h are swept)

**Fault injection**: `kill -9` the API in a loop (every 2 to 5 s, 20 times) during batch commits, so some blob files are fully written while their commit transaction never ran. Separately, the harness places 50 random 1 MiB files with valid names under `blobs/` that no database row references and ages them 49 h with T-CLOCK, and places 50 more aged 47 h.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Restart server; retry the commits from the clients within 24 h | Blobs whose upload session is still valid commit; no duplicate file |
| 2 | Advance the clock past 24 h; let the reaper run | Expired `uploading` blobs are removed; the account's reserved bytes return to baseline |
| 3 | Let the orphan sweep run | The 50 orphans older than 48 h are removed; the 50 younger orphans and all live files untouched |
| 4 | Stop Postgres during a sweep | Sweep removes nothing and reports failure |

**Pass criteria**: live files lost = 0; live-file hash-set unchanged; old orphans left = 0, young orphans left = 50 (exact); files deleted while the database is unavailable = 0; reserved bytes equal baseline (exact).

---

### NFT-RES-05: Quota and session race with 100 sessions of 50 GiB [v1.0]

**Traces to**: AC-39, AC-4; RESTRICT-09 | **Data**: 100 sessions declaring 50 GiB | **Status**: READY (RR Transfer #6; the session cap, quota and floor values are read from the deployment config, G-R9)

**Fault injection**: one account fires all 100 session-create calls within 100 ms; repeat with 3 accounts × 100 sessions at once; 10 of the accepted sessions stream real data.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Create sessions concurrently | Exactly min(cap, floor(quota / 50 GiB)) accepted per account; the rest refused with a client error and a machine-readable code |
| 2 | Sample free space on the blob volume every 1 s | Never below the configured floor |
| 3 | Abandon sessions; wait for the reaper | Reserved bytes released; new sessions accepted again |

**Pass criteria**: accepted = configured cap or quota limit exactly (no over-admission under the race); `used + reserved` ≤ quota at every sample; free space ≥ floor at every sample; 0 server errors (5xx).

---

### NFT-RES-06: Blob volume fills up outside the server's control [v1.0]

**Traces to**: AC-3, AC-38, AC-39; RESTRICT-07, RESTRICT-09 | **Data**: big-file 5 GiB, many-small | **Status**: READY (verdict rests on data integrity and process survival; the failing upload may answer any non-2xx)

**Fault injection**: an external process fills the blob volume to 100% while 20 uploads run (bypassing the free-space floor); Postgres data stays on its own volume.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Fill the volume | In-flight uploads fail with a non-2xx; API process stays up; health endpoint reports degraded |
| 2 | Read 100 sampled committed files and list a folder | Hashes equal originals; listing p95 ≤ 300 ms |
| 3 | Free 10 GiB | Interrupted uploads resume, re-sending ≤ 10 MiB each, and commit |

**Pass criteria**: committed files truncated or zero-filled = 0; process restarts = 0; every upload acknowledged as committed is readable; resumes ≤ 10 MiB.

---

### NFT-RES-07: Database restored from an older backup [v1.0]

**Traces to**: AC-41, AC-8, AC-65; RESTRICT-09, RESTRICT-31 | **Data**: backup-set, 3 synced devices (2 desktop, 1 web) | **Status**: READY (RR Sync #5, RR Thresholds #12)

**Fault injection**: take a backup at T1; afterwards on the devices create 20 files, edit 20 files and delete 10 files so cursors advance; restore the T1 database by the documented procedure (blobs kept); start the server.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 0 | After the restore and before any device polls, run `sunnycloud doctor` and `sunnycloud serve` on the restored install that has not had `sunnycloud admin epoch-bump` | `doctor` reports **Fail** (not a warning): epoch bump missing (database epoch is below the `.epoch` marker on the blob volume); `serve` refuses to start and prints `admin epoch-bump`; after `admin epoch-bump` both succeed |
| 1 | Devices poll the change feed with their old cursors | Each device receives `resync_required` (HTTP 410) |
| 2 | Devices run a full tree resync | Local-only files (the 20 new) are uploaded; server-only files (the 10 deleted after T1) are downloaded again; edited files keep the newer content as current and the other as conflict copy where both differ; nothing is deleted |
| 3 | New change on device A after the restore | Appears on B within 5 s |
| 4 | Send a cursor whose seq is above the server's head, or with an unknown epoch (restore done without rotating the epoch) | `resync_required` again [G-R12] |

**Pass criteria**: devices receiving `resync_required` = 3 of 3 (exact); converged hash-sets ≤ 60 s after server start; deletions performed by the resync = 0 (exact); content versions lost = 0.

---

### NFT-RES-08: Device offline longer than the 90-day tombstone horizon [v1.0]

**Traces to**: AC-41, AC-8, AC-30; RESTRICT-09 | **Data**: 2 devices, 100 files | **Status**: READY (RR Sync #6; uses T-CLOCK); the outcome for file Y is not asserted (G-R4)

**Fault injection**: device A goes offline. On B: delete file Y, rename a folder, create file W. On A while offline: edit file X, create file Z, edit file Q (that B deletes). Advance server time by 91 days and run the purge job; run a second case with 89 days.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | A reconnects after 91 days | `resync_required`; full enumeration by file ID, no cursor replay |
| 2 | A reconnects after 89 days (second case) | Incremental sync from the old cursor; no full resync |

**Pass criteria**: 91 d: A's offline edits X, Z kept and uploaded, Q kept (edit beats delete, AC-8), W present on A, rename applied; local data lost = 0 (outcome recorded for the unchanged, remotely deleted Y: either removed or restored, never an error); converged hash-sets ≤ 60 s. 89 d: `resync_required` count = 0.

---

### NFT-RES-09: Blob retention and operator restore [v1.0]

**Traces to**: AC-30; RESTRICT-10 | **Data**: one file | **Status**: READY (RR Deletion #3, #4; uses T-CLOCK)

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Upload v1 (hash H1); overwrite with v2; delete the file; all on day 0 | v1 and v2 blobs queued for GC in the same change |
| 2 | Day 6: check the blob volume for the v1 blob; run `sunnycloud admin restore --path --at <time-before-overwrite>` | v1 blob file present; restore returns a file with hash H1 |
| 3 | Day 8, after one GC cycle: same checks | v1 blob file absent; restore reports a clear error |
| 4 | Retention set to 0, 90 and 91 | 0: removed within one configured GC interval (polled, short mock interval); 90: present on day 89, absent on day 91; 91 rejected as invalid config |
| 5 | Restore to a time before the file existed | Clear error; nothing written |

**Pass criteria**: presence/absence exactly as above on day 6 and day 8; restored hash equals H1 (exact); config range enforced 0 to 90.

---

### NFT-RES-10: Older database plus newer blob copy stays consistent [v1.0]

**Traces to**: AC-30, AC-65, AC-24; RESTRICT-10, RESTRICT-31 | **Data**: backup-set | **Status**: READY (`b_server_storage` #1; pass by hash-set equality, RR Auth #18)

**Fault injection**: dump DB at T1; at T1+1 h delete 20 files and overwrite 20; copy blobs at T1+2 h (after the deletes). Restore both on a clean host. Second run with retention 0.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Restore and start | Server up; reconciliation runs |
| 2 | Read all files in the T1 tree | All 40 affected files readable with their T1 hashes |
| 3 | Run with retention 0 | Reconciliation reports exactly the 40 missing files by path; non-zero exit status; no silent success |

**Pass criteria**: retention ≥ backup interval: missing files = 0 and hash-set equals the T1 set; retention 0: missing files reported = 40 (exact); untracked newer blobs reported, 0 auto-deleted before minimum age.

---

### NFT-RES-11: Sync root missing, unmounted or empty pauses deletes [v1.0]

**Traces to**: AC-29; RESTRICT-24 | **Data**: 300 files from many-small; second tree of 150 files | **Status**: READY (RR Deletion #1)

**Fault injection**: (A) `rm -rf` the root directory; (B) unmount the volume holding the root (mount point remains, empty); (C) `rm -rf root/*` on the 150-file tree (empty root, fewer than 200 deletes). Linux and Windows.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Inject fault | Daemon reports a "deletes held, confirmation needed" state within 10 s; no delete reaches the server |
| 2 | Restart the daemon while held | Hold persists after restart |
| 3 | Create one new file in a still-present root (case C) | New file uploads normally during the hold |
| 4a | User answers "no" / remount | Local files restored from the server: hash-set equals server |
| 4b | User answers "yes" | Deletions propagate; tombstones = file count |

**Pass criteria**: deletions propagated before confirmation = 0 in A, B, C (exact; checked on a second device's change feed); after "no", 300 (150 in C) files back with original hashes.

---

### NFT-RES-12: 200-deletion batch guard, both directions [v1.0]

**Traces to**: AC-29; RESTRICT-24 | **Data**: 300 files | **Status**: READY (RR Deletion #2); batch window definition in G-R5

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Delete 199 local files in one burst | Propagate without a prompt |
| 2 | Delete 200 local files in one burst (fresh tree) | Hold and prompt; 0 deletes reach the server |
| 3 | Answer "no" | All 200 files return locally with original hashes |
| 4 | Another device deletes 200 files; this daemon receives them | Daemon holds applying them; 0 local files removed until confirmed; "yes" then applies all 200 |

**Pass criteria**: 199 → 199 propagated, 0 prompts; 200 → 0 propagated before confirmation (exact), 200 restored after "no"; remote batch of 200 → 0 local deletions before confirmation.

---

### NFT-RES-13: Backup, restore on a clean host, and upgrade [v1.0]

**Traces to**: AC-65, AC-43, AC-41, AC-64; RESTRICT-08, RESTRICT-31 | **Data**: backup-set | **Status**: READY (RR Auth #18); run in CI

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Follow the documented procedure with the exact documented commands: `pg_dump` run as the Postgres superuser inside the db container, then copy blobs, keep the master key separately; GC runs during the copy and the retention minus GC interval exceeds the copy time | Backup artifacts complete; no retained blob is missing from the copy |
| 2 | Restore on a clean host with a fresh compose stack and the same master key (`pg_restore` as the Postgres superuser; the init script has created the roles); run `sunnycloud admin epoch-bump` (and `admin keycheck-init` if the restored volume has no key check yet), then `sunnycloud scrub run --full` and `sunnycloud doctor` | Server starts; all files present; scrub exit 0; doctor has no failures |
| 3 | Open an existing share link; sign in; connected devices poll | Link opens (HTTP 200, bytes equal); devices get `resync_required` and converge |
| 4 | Upgrade the restored install from release N-1 to N | All data present; devices resume with 0 bytes re-uploaded |

**Pass criteria**: path→hash set after restore and after upgrade equals the backup-set manifest (exact, 0 missing, 0 extra); share links valid; bytes re-uploaded by devices after upgrade = 0.

---

### NFT-RES-14: Upgrade interrupted by a crash [v1.0]

**Traces to**: AC-65; RESTRICT-31, new restrictions bullet (interrupted upgrade) | **Data**: backup-set | **Status**: READY (restrictions: an interrupted upgrade rolls back its migration transaction and the server refuses to start until it is re-run)

**Fault injection**: `kill -9` the API container (and separately Postgres) while the migrations of release N run on a backup-set copy; 5 kill points.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Start release N again without re-running the upgrade | Server refuses to start with an explicit message; serves 0 requests |
| 2 | Inspect the database | Schema and data as before the upgrade (migration rolled back) |
| 3 | Re-run the upgrade | Succeeds; server starts |

**Pass criteria**: starts that serve a partially migrated schema = 0 (exact); after the re-run, path→hash set equals the manifest (exact, 0 missing, 0 extra).

---

### NFT-RES-15: Exclusively locked file is retried without CPU spin [v1.0]

**Traces to**: AC-33; RESTRICT-24 | **Data**: a 100 MiB file with a pending edit | **Status**: READY (RR Desktop #10)

**Fault injection**: Windows: another process opens the file with an exclusive lock (no share) for 60 s, then releases. Linux (analog): file made unreadable (`chmod 000`) for 60 s, then restored.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Hold the lock; sample daemon CPU every 1 s | Average over the 60 s ≤ 1% |
| 2 | Inspect server and change feed | File still listed; 0 delete events |
| 3 | Release the lock | Edit uploaded within 10 s; server hash equals local |

**Pass criteria**: CPU ≤ 1% during the lock; upload completes ≤ 10 s after release; deletes reported = 0 (exact).

---

### NFT-RES-16: Suspend and wake convergence [v1.0]

**Traces to**: AC-35, AC-8, AC-21 | **Data**: 500 files changed on a second device, 500 local changes | **Status**: READY (RR Desktop #12)

**Fault injection**: device A (Linux systemd suspend; Windows sleep) suspends for 30 min. During it, device B changes 500 files (disjoint set) and 20 files that A also changes right after wake; A makes 500 local changes within 5 s after wake; clock steps forward on wake.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Wake A | Daemon reconnects, refreshes expired access token without user action, resumes long-poll |
| 2 | Wait for convergence | All device hash-sets equal |

**Pass criteria**: converged ≤ 60 s after wake on each OS; conflict copies = exactly 20; files lost = 0; re-login prompts = 0.

---

### NFT-RES-17: Cloud-evicted placeholder files are never hydrated or deleted [v1.0 Windows OneDrive, v1.1 macOS iCloud]

**Traces to**: AC-32; RESTRICT-24 | **Data**: evicted-100 (100 placeholders among 400 normal files) | **Status**: READY (RR Desktop #9; environment G-R13)

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Connect the folder, full scan, steady state 10 min | Daemon status reports 100 skipped files |
| 2 | Observe the cloud client's network and placeholder state | 0 bytes downloaded for placeholders; all 100 remain offline |
| 3 | Another device deletes 20 placeholder files on the server | Daemon does not delete them locally; reports them skipped |
| 4 | User hydrates one placeholder | Daemon then syncs it as a normal file |

**Pass criteria**: hydrated bytes = 0; skipped reported = 100; deletions propagated = 0 and local deletions of placeholders = 0 (exact).

---

### NFT-RES-18: First-sync merge interrupted by daemon kills [v1.0]

**Traces to**: AC-31, AC-8, AC-42; RESTRICT-24 | **Data**: collide-1000 | **Status**: READY (RR Desktop #8)

**Fault injection**: connect the folder; `kill -9` the daemon at about 30% and again at about 70% of progress; restart each time.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Merge runs to completion across restarts | Merge state resumes; no restart from zero |
| 2 | Compare local, server and conflict names | 800 non-colliding files uploaded; 100 identical paths unchanged; 100 different paths each keep both versions |

**Pass criteria**: files lost = 0 (all 1,000 local hashes present as file or conflict copy); blocks uploaded for the 100 identical-content paths = 0; conflict copies = exactly 100 and none named "(conflict copy) (conflict copy)".

---

### NFT-RES-19: Hung ffmpeg is killed and retried once on CPU [v1.0]

**Traces to**: AC-45, AC-11, AC-44; RESTRICT-13 | **Data**: clip-mixed-100 | **Status**: READY (RR Photos and videos #7)

**Fault injection**: (A) `kill -STOP` the ffmpeg process of one job; (B) an ffmpeg that ignores SIGTERM; (C) hang the retry too; (D) control: a legitimately slow 4K job that keeps printing progress for more than 90 s in total.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Hang the process | Killed within 90 s of its last progress line |
| 2 | Observe the job | Retried exactly once on CPU; output valid and playable |
| 3 | Case C | Second hang kills it again; job ends `failed`, no third attempt |
| 4 | Case D | Not killed |
| 5 | Inspect rotated and variable-frame-rate clips | Orientation correct; audio drift ≤ 40 ms |

**Pass criteria**: kill time ≤ 90 s after last progress; attempts per job ≤ 2; orphan ffmpeg processes after 30 s = 0; drift ≤ 40 ms; thumbnail lane p95 ≤ 5 s during the hang.

---

### NFT-RES-20: PostgreSQL restart, listener loss and cursor catch-up [v1.0]

**Traces to**: AC-5, AC-8, AC-21, AC-1; RESTRICT-09, RESTRICT-11, new restrictions bullet (recovery within 60 s) | **Data**: 100 syncing sessions, 3 devices on one account | **Status**: READY (restrictions: server recovers within 60 s of the database returning)

**Fault injection**: (A) `docker restart postgres` (20 s down); (B) `kill -9` Postgres (crash recovery); (C) reset all database connections without stopping Postgres. In each, device A commits a change within 1 s before the fault while B and C hold parked long-polls.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | During the outage | Database-backed calls return 503 (Retry-After); no 2xx with stale or partial data; API process keeps running |
| 2 | After Postgres is ready | Long-polls reconnect without client restart; the pre-fault change reaches B and C |
| 3 | New change on A | Reaches B within 5 s |

**Pass criteria**: commits acknowledged 2xx before the fault present afterward = 100%; missed changes on B and C = 0; API restarts = 0; DB connections after recovery ≤ pool max; first successful authenticated request ≤ 60 s after Postgres accepts connections.

---

### NFT-RES-21: Few database connections with many held long-polls [v1.0]

**Traces to**: AC-1, AC-5, AC-6; RESTRICT-11 | **Data**: 100 sessions, 2,000 parked long-polls, listing-10k | **Status**: READY (`b_server_storage` #10)

**Fault injection**: Postgres `max_connections` and the API pool limited to 20.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | 100 uploads run, 2,000 clients hold long-polls for 10 min | Requests served; connection count ≤ 20 |
| 2 | A 5 MB edit on A; list the 10,000-entry folder | Edit reaches B ≤ 5 s p95; listing ≤ 300 ms p95 |

**Pass criteria**: failed requests = 0; 5xx rate ≤ 0.1%; AC-5 p95 ≤ 5 s; AC-6 p95 ≤ 300 ms.

---

### NFT-RES-22: Batch commit is atomic under a crash [v1.0]

**Traces to**: AC-40, AC-38; RESTRICT-11, new restrictions bullet (idempotent batch commit) | **Data**: many-small | **Status**: READY (restrictions: batch commits are idempotent by a client-supplied commit ID)

**Fault injection**: send a 500-entry `commits:batch` and `kill -9` the API while it executes; 20 repetitions at varying offsets.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | After restart, list the affected folder | Either all 500 entries visible or none |
| 2 | Compare change feed | The batch is one cursor advance, or none |
| 3 | Client retries the batch with the same commit ID | Exactly one application of the batch, whether or not the first attempt had committed |
| 4 | Send the same commit ID twice concurrently | One application; the second call returns the first result |

**Pass criteria**: partial batches observed = 0 in 20 runs (exact); entries after retry = 500 exactly, duplicate entries = 0, conflict copies = 0; blob files with live rows but missing data = 0.

---

### NFT-RES-23: Worker crash and job lease expiry [v1.0]

**Traces to**: AC-10, AC-11, AC-25; RESTRICT-13, RESTRICT-28, new restrictions bullet (lease 5 min, 3 attempts) | **Data**: photos, clip-1080-sdr, clip-compliant | **Status**: READY (restrictions: job lease 5 min, 3 attempts; the test runs with a short mock lease L from config and asserts the production default by reading the config)

**Fault injection**: 200 queued jobs (photos plus 3 clips) on 2 workers; `kill -9` one supervisor container while its decoder runs ffmpeg; later kill the second worker too, then restart both. For the attempt limit, one clip job is killed during each of its attempts.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Kill worker 1 | No orphan ffmpeg or vipsthumbnail in either of its two decoder containers after 5 s |
| 2 | Wait for lease expiry | Jobs of worker 1 are claimed by worker 2 and finish |
| 3 | Kill worker 2, restart both | Remaining jobs finish |
| 4 | Kill the worker on every attempt of one clip job | After 3 attempts the job is `failed`; no 4th attempt |

**Pass criteria**: jobs stuck `running` beyond L + 60 s = 0; attempts per job ≤ 3 (exact); derived artifacts per original = exactly the expected set (1 thumbnail set; 1 transcode, 0 for clip-compliant), duplicates = 0, partial or unplayable outputs = 0; thumbnail lane p95 ≤ 5 s on the surviving worker; configured production defaults read as 5 min and 3.

---

### NFT-RES-24: Identity provider unreachable [v1.0]

**Traces to**: AC-19, AC-21; RESTRICT-17, RESTRICT-19, RESTRICT-20, new restrictions bullet (provider timeout 10 s) | **Data**: oauth-mock | **Status**: READY (restrictions: provider calls time out after 10 s; the verdict is an error outcome, not specific wording)

**Fault injection**: block API egress to the provider; also (B) provider answers 500 on the token endpoint; (C) provider hangs for 60 s; (D) the server has no internet at all.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Existing web sessions and device tokens sync for 10 min, including refresh rotation | 0 failures; no provider call needed |
| 2 | A new sign-in attempt | Error response, no hang beyond the timeout, 0 accounts created, no half-created session |
| 3 | Revoke a device during the outage | Takes effect ≤ 5 s |
| 4 | Provider restored | Sign-in works without restarting the server |

**Pass criteria**: sync failures for existing sessions = 0; refresh failures = 0; sign-in ends in an error response ≤ 11 s after the attempt (10 s timeout + 1 s margin; a test config with a 2 s timeout asserts ≤ 3 s); accounts and sessions created during outage = 0; health endpoint OK.

---

### NFT-RES-25: Client and server clock skew [v1.0]

**Traces to**: AC-8, AC-12, AC-35; RESTRICT-11, RESTRICT-24 | **Data**: one 1 MiB file | **Status**: READY (RR Sync #3)

**Fault injection**: client clocks set to +1 day, -1 day, +1 year; server clock stepped back 1 h during a commit stream.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | A (clock +1 day) and B (clock -1 day) edit offline from base rev N; A commits first, B later | B's content is current; A's kept as "name (conflict copy YYYY-MM-DD HH-MM <device>)" with B as the committing device; no mtime comparison |
| 2 | Client at +8 days opens a 7-day share link created now | Link opens (server clock decides expiry) |
| 3 | Step the server clock back 1 h under load | Commit sequence stays strictly increasing; 0 reordered or lost changes; 0 sessions mass-expired |

**Pass criteria**: current version is the later commit by server order (exact hash); conflict copies = 1; stored file intact; cursor seq strictly increasing across the step; 5xx = 0.

---

### NFT-RES-26: Revocation survives restarts and mid-upload [v1.0]

**Traces to**: AC-21, AC-3; RESTRICT-19 | **Data**: big-file 5 GiB | **Status**: READY (RR Auth #7)

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Revoke a device while its 5 GiB upload is in flight | Further block uploads rejected (401) ≤ 5 s; session cannot commit; no partial file listed |
| 2 | Revoke, then `kill -9` the API within 1 s and restart | Revoked token rejected on 100 of 100 attempts after restart |
| 3 | Stop Postgres and call revoke | Call fails (5xx); never answers 2xx without persisting |

**Pass criteria**: blocks accepted from the revoked device more than 5 s after revocation = 0; post-restart accepted calls with the revoked token = 0; false 2xx on revoke during outage = 0.

---

### NFT-RES-27: Mobile first sync survives forced process kills [v1.0 Android, v1.1 iOS]

**Traces to**: AC-52, AC-9; RESTRICT-15 | **Data**: phone-fixtures (library of 50,000 assets) | **Status**: READY (RR Photos and videos #10)

**Fault injection**: force-stop the app 10 times at random points during the first sync; once reboot the phone; once toggle airplane mode.

**Pass criteria**: assets re-hashed that were already hashed ≤ 1% (≤ 500); duplicate server files = 0; RSS ≤ 150 MB at every 1 s sample; all 50,000 assets present on the server at the end; first upload after a background wake starts ≤ 10 s.

---

### NFT-RES-28: Re-install keeps backed-up photos from uploading again [v1.0 Android, v1.1 iOS]

**Traces to**: AC-46, AC-52 | **Data**: phone-fixtures (5,000 backed-up assets, plus one edited photo) | **Status**: READY (RR Photos and videos #9; link rule from AC-46)

**Fault injection**: uninstall and reinstall the app, sign in again. Separately, edit one photo in place (same MediaStore asset ID, new content hash) and add one photo with a new asset ID but identical content to another.

**Pass criteria**: media bytes uploaded on reinstall = 0; the edited photo (same asset ID, new hash) is linked to its original, not a duplicate; a photo with a different asset ID is a new file; duplicate server files = 0.

---

### NFT-RES-29: Web upload survives tab close and edit mid-upload [v1.0]

**Traces to**: AC-50, AC-3 | **Data**: big-file 50 GiB | **Status**: READY (RR Web #1); Chrome, Firefox, Safari

**Fault injection**: close the tab at 50%; reopen and re-select the same file. In a second run edit the file (change content and mtime) mid-upload.

**Pass criteria**: re-sent bytes after re-select ≤ 10 MiB; final hash equals source; worker plus page heap ≤ 150 MB at every sample; edited-mid-upload case: change detected, upload restarts with a new session, final file equals the edited content, old session reserved bytes released.

---

### NFT-RES-30: Account deletion job survives a crash [v1.0]

**Traces to**: AC-27, AC-30; RESTRICT-10 | **Data**: many-small (10,000 files), a second untouched account | **Status**: READY (AC-27: deletion finishes within 15 min of the request)

**Fault injection**: delete account A, `kill -9` the API after about 1 s (mid-job), restart.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Right after the delete call | A's sessions and devices rejected ≤ 5 s; A's IDs and share links answer 404 |
| 2 | After restart | Job continues through the account erase hooks (each hook drains its rows in batches until it reports nothing remaining); only then are the identity rows deleted |

**Pass criteria**: the restart happens ≤ 60 s after the kill; blob files of A remaining = 0 and database rows of A = 0 within 15 min of the delete request (exact); account B hash-set unchanged; retention not applied to A (blobs gone with retention still 7 d).

---

### NFT-RES-31: NVENC missing or failing falls back to CPU [v1.1]

**Traces to**: AC-28; RESTRICT-14 | **Data**: clip-1080-sdr | **Status**: READY (RR Photos and videos #12)

**Fault injection**: enable GPU encoding; then remove the GPU from the worker; separately make the GPU encode fail mid-job; separately leave the option off.

**Pass criteria**: option off → GPU never used (default); on with GPU → test encode at start succeeds and GPU is used; GPU removed or job failing → job completes on CPU, output playable H.264 High 1080p AAC, job failures = 0.

---

### NFT-RES-32: Internet cut: existing sessions keep working, new sign-ins fail clearly [v1.0]

**Traces to**: AC-21, AC-19; RESTRICT-19, RESTRICT-20, new restrictions bullet (no-internet sessions) | **Data**: running install with web, desktop and Android sessions, oauth-mock | **Status**: READY (AC-21; RR Second-round #4)

**Fault injection**: deny all egress from the API and worker containers (no DNS, no route to the provider); in a second run also `docker restart api` while offline; clock advanced 7 days with T-CLOCK in a third run.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Cut the internet; keep a web session, a desktop device token and an Android session (a `sessions` row from the mobile sign-in) syncing for 30 min with edits on each | Syncs continue; 0 auth failures; refresh rotation succeeds |
| 2 | Restart the API while offline | Server starts and serves requests without contacting the provider; the same sessions and tokens still work |
| 3 | Advance the clock 7 days and use a device token and a refreshed web session | Still accepted while valid; no provider call needed |
| 4 | Revoke one device while offline | Stops syncing within 5 s |
| 5 | Attempt a new sign-in (web, and a new device-grant approval that needs a provider login) | Fails with a clear error (error page or JSON with an error code) within the 10 s provider timeout + 1 s; 0 accounts created; no stack trace in the response |
| 6 | Restore the internet | New sign-in succeeds without restarting the server |

**Pass criteria**: existing-session request failures = 0 over the offline period (exact); server restarts needing the provider = 0; revocation ≤ 5 s; new sign-in error within 11 s; accounts created offline = 0; recovery without restart (exact).

---

### NFT-RES-33: 15-minute access tokens and rotating refresh tokens on mobile [v1.0 Android, v1.1 iOS]

**Traces to**: AC-58, AC-9, AC-52; RESTRICT-19 | **Data**: phone-fixtures (a 2 GiB video and 200 photos), a mobile session, T-CLOCK | **Status**: READY (access token 15 min, refresh rotation with a 30 s reuse grace)

**Fault injection**: the access-token lifetime is the production 15 minutes (the clock is advanced through T-CLOCK, not slept); a kill of the app process between the server's rotation and the app's token write is armed through `e2e-ctl`.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Start a backup of the 2 GiB video; advance the server clock 16 minutes mid-upload | One refresh happens; the interrupted block is repeated; the upload completes; 0 failed blocks, 0 sign-in prompts |
| 2 | Trigger the foreground backup and the background worker together at the moment the token expires | Exactly one refresh request reaches the server (single flight); both runs continue |
| 3 | Kill the app after the server answered a refresh and before the app stored the new tokens; restart within 20 s | The app refreshes again with the stored token inside the 30 s grace and continues; no sign-out |
| 4 | Repeat step 3 but restart 40 s later | The server revokes the token family (reuse); the app shows the sign-in screen and uploads nothing more |
| 5 | Sign out in the app | `DELETE /api/v1/devices/me` is called; the old access and refresh tokens answer 401 |

**Pass criteria**: refresh requests per expiry = 1 (exact); blocks lost = 0; the new refresh token is stored before the new access token is used (observed in the app's call order); after a kill within the grace the app continues, after the grace it asks for sign-in.

---

### NFT-RES-34: Crash between blob write and commit never leaves a revision without a blob [v1.0]

**Traces to**: AC-68; RESTRICT-07, RESTRICT-09, RESTRICT-32 | **Data**: multiblock-100 (DS-34, first 20 files), many-small (DS-01, first 40 files, all at most 2 MiB so they take the single-request path) | **Env**: C, group X (kill/restart, T-CLOCK) | **Status**: READY (AC-68 text; reaper 24 h and orphan sweep 48 h from RESTRICT-32; the stack runs the reaper and the sweep on short config intervals and the clock moves through T-CLOCK)

**Fault injection**: Variant A (the seam, deterministic): the consumer uploads every block of the 20 multi-block files (`need_blocks` empty, every block acknowledged) and the 40 single-request files (`PUT /api/v1/staged-uploads/...` answered 204), sends no commit, and `e2e-ctl` runs `docker kill -s KILL api` and `docker start api`. Variant B (random): 20 rounds of 100 concurrent single-entry `commits:batch` calls for pre-uploaded files, `kill -s KILL api` at a seeded 0 to 200 ms after the barrier release, restart, repeat. The test uses its own account; the reservation is read through `GET /api/v1/accounts/me/quota` just before each kill.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Variant A: after restart list the account tree and read the quota | 0 of the 60 files listed; `reserved_bytes` equals the value read just before the kill; the blob files of the 60 uploads are on the volume (ctl listing) |
| 2 | Variant B, after every restart: list the tree, download every listed file, run `sunnycloud doctor --json` | Every listed file answers 200 and its BLAKE3 equals the manifest; `doctor` lists 0 revisions with a missing blob |
| 3 | Variant B: re-send every commit with its original commit ID | Each file ends up listed exactly once (committed by the retry or already committed before the kill); 0 duplicates, 0 conflict copies |
| 4 | Variant A: advance the clock 25 h and let the reaper run | The 20 sessions answer expired; `reserved_bytes` returns to the baseline read before the uploads (exact) |
| 5 | Variant A: advance the clock to 49 h after the uploads and let the orphan sweep run | ctl lists 0 blob files without a `blobs` row (ctl compares the volume listing with the ids it reads, read-only); no live file touched |

**Pass criteria**: committed revisions whose blob is missing or unreadable = 0 in variant A and in all 20 rounds of variant B (exact); files visible in variant A before any commit = 0; reserved bytes after step 4 equal the baseline (exact); orphan blob files after step 5 = 0; live files lost = 0; duplicates after step 3 = 0.

---

### NFT-RES-35: 50 concurrent commits give a change feed with no gap, no repeat and no reordering [v1.0]

**Traces to**: AC-69; RESTRICT-09, RESTRICT-11 | **Data**: many-small (DS-01, first 1,400 files, each at most 2 MiB) | **Env**: C, group P (one account of its own) | **Status**: READY (AC-69 text; the feed returns the latest state of each touched node with the `seq` of the stamping transaction, so the test makes every stamped node distinct and each `seq` is then visible)

**Concurrency**: 50 workers released from one barrier on one account. Per worker: 1 folder create, 18 single-entry commits (`POST /api/v1/commits:batch`, new files), 1 ten-entry batch commit, and 2 batches whose entries are all invalid (name with a NUL). That is 50 x 20 = 1,000 stamping calls (50 folders, 900 single commits, 50 batches) touching 1,450 nodes, plus 100 calls that stamp nothing. Run 3 times with seeds 1, 2, 3, each on a new account.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Read `GET /api/v1/changes/head` as cursor H0; start reader R1, a long-poll loop from H0 (`wait=5`, `limit=100`) that records `(seq, node_id)` in arrival order | R1 is parked or returns an empty page |
| 2 | Release the 50 workers; wait until every worker has returned and R1 has drained | Every stamping call answers 2xx; the 100 all-invalid batches answer 200 with per-entry errors |
| 3 | Read the head again as H1 | `H1.seq - H0.seq = 1000` (exact): the invalid batches consumed no `seq` |
| 4 | Reader R2 reads from cursor `E.0` (the account's epoch with seq 0) up to H1 with `limit=100` | Same item sequence as R1 for every seq above H0.seq |

**Pass criteria**: the distinct `seq` values of R1 and of R2 above H0.seq are exactly H0.seq+1 to H0.seq+1000, each once, strictly increasing across pages (gaps 0, repeats 0, out-of-order items 0); items per `seq` = 1 for folders and single commits and 10 for batches; distinct node ids seen = 1,450; R1 and R2 identical; responses of 5xx = 0 over the 3 runs.

---

### NFT-RES-36: Killing the media supervisor mid-job leaves exactly one derived file per key and no scratch data [v1.0]

**Traces to**: AC-71; RESTRICT-13, RESTRICT-32 | **Data**: photos (DS-03, first 20 JPEGs), clip-1080-sdr (DS-05, 1 clip) | **Env**: C, group X (kill/restart; job lease L set by config to 20 s, reaper interval 1 s; the production default of 5 min is asserted by reading the config) | **Status**: READY (AC-71 text; lease behaviour is also covered by NFT-RES-23, this scenario adds the exactly-once and scratch checks)

**Fault injection**: three `kill -s KILL worker-supervisor` points, each followed by `docker start`: K1 while `decoder-general` runs an ffmpeg process for the clip (ctl sees the process); K2 for each of the 20 photo jobs, at a seeded 0 to 30 ms after the API access log shows `PUT /api/v1/worker-jobs/{id}/outputs/thumb` (this hits the window between "output accepted" and "supervisor learns it" in part of the 20 runs, which the pass criteria do not depend on); K3 when `admin media-stats` shows half of the 20 photo jobs succeeded.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Upload the 20 photos and the clip; apply K1, K2, K3; wait until `admin media-stats` shows 0 queued and 0 running jobs | All jobs `succeeded` |
| 2 | ctl read-only probe of `derived` and `jobs` grouped by (account, content hash, kind, params) | Exactly 1 derived row per expected key (photo: the thumbnail kinds the planner defines for it, clip: 1 transcode and 1 poster thumbnail); exactly 1 job row per key |
| 3 | Re-upload the 20 photos under new names while the queue is busy | 0 new job rows, 0 new derived rows (same content hash) |
| 4 | Run `sunnycloud admin gc-run`, then compare blob files on the volume with the ids ctl reads from `blobs` | Blob files = originals + derived rows; the duplicate output blobs of a retried accept are gone |
| 5 | `e2e-ctl inspect` of `decoder-interactive` and `decoder-general`: scratch contents, process list; the supervisor work directory | 0 files in each scratch area, 0 leftover ffmpeg or vipsthumbnail processes |
| 6 | Fetch the thumbnail of each photo and of the clip | 200 each, valid image |

**Pass criteria**: derived rows per (account, content hash, kind, params) = 1 (exact), duplicate derived blob files after step 4 = 0, scratch files after step 5 = 0, jobs `running` beyond L + 60 s = 0, attempts per job at most 3, new jobs from step 3 = 0.

---

### NFT-RES-37: After a restore, `doctor` lists every revision whose blob is gone and listings stay error-free [v1.0]

**Traces to**: AC-73, AC-65; RESTRICT-10, RESTRICT-31 | **Data**: backup-set (DS-22: 20 accounts, about 2,000 files) | **Env**: C, group X (DB restore, T-CLOCK, retention 7 d read from config) | **Status**: READY (AC-73 text; `doctor` exit code 1 or 2 is policy of component 12, so the test requires non-zero and the listed set, not a specific level)

**Fault injection**: take `pg_dump` at T0 on the populated stack; then delete 50 chosen files (random across 5 accounts, current revisions) through the API, advance the clock 8 days, run `admin gc-run` so their blobs are removed after retention; stop the API; `pg_restore` the T0 dump; run `admin epoch-bump`; start the API. The expected set is the 50 revision ids the consumer recorded.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | `sunnycloud doctor --json` | Missing-blob finding lists exactly the 50 revisions with path, account and revision id; exit code non-zero (1 or 2) |
| 2 | As each of the 5 owners, page through the listing of every folder that contains an affected file (limit 1000) | 200 on every page; the 50 files are listed; 0 responses of 5xx |
| 3 | Download each of the 50 files | 404 with code `content_unavailable` (distinct code), never 5xx; the same answer on a second request without any file access (the row is marked unavailable) |
| 4 | Download 100 other files of the same accounts | 200, hash equals the manifest |
| 5 | Grep the API logs for ERROR lines during steps 2 to 4 | 0 lines caused by the missing blobs |

**Pass criteria**: `doctor` set equals the injected set (missing 0, extra 0; exact); listing pages with 5xx = 0; downloads of the 50 with any status other than 404 `content_unavailable` = 0; healthy files unreadable = 0.

---

### NFT-RES-38: 1,000 concurrent commits and deletes keep stored usage equal to live bytes and below quota [v1.0]

**Traces to**: AC-74; RESTRICT-09, RESTRICT-10 | **Data**: many-small (DS-01 generator with fixed size, `gen_tree.py --size 1MiB`, 1,300 files of 0.5 to 1.5 MiB plus 80 zero-byte files) | **Env**: C, group P (account of its own with a quota set through the admin API) | **Status**: READY (AC-74 text)

**Concurrency**: account quota Q = 512 MiB; pre-fill 800 files of 0.5 MiB (400 MiB) in 80 folders of 10. 1,000 operations released from one barrier over 10 HTTP/2 connections: 300 new-file commits (up to 1.5 MiB, most exceed the 112 MiB headroom), 80 zero-byte new files, 250 replace-edits with a different size, 150 stale-base edits (the base was superseded before the run, so each makes a conflict copy), 200 file deletes, 20 folder deletes of 10 files each (cascade). Targets of edits and deletes are disjoint. Run 5 times, each on a new account.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | During the run sample `GET /api/v1/accounts/me/quota` every 50 ms | `used + reserved <= Q` in every sample; no negative value |
| 2 | Wait until every call returned and the cascade is drained (the folder listing shows no pending tombstone work, 3 x `tree_maintenance_interval`) | `reserved_bytes = 0` |
| 3 | The consumer walks the live tree and sums the sizes of the current revision of every live file (conflict copies count, tombstones and retained revisions do not) as S | `used_bytes = S` (exact) |
| 4 | `sunnycloud doctor --json` | Quota drift finding for the account: none |
| 5 | Check every refused call | Answers 507 with code `quota_exceeded`; no other 5xx |

**Pass criteria**: `used_bytes - S = 0` in 5 of 5 runs; samples above Q = 0; reserved after settle = 0; 5xx other than 507 = 0; drift reported by `doctor` = 0.

---

### NFT-RES-39: The scrub never reports a blob that is being uploaded or removed by garbage collection [v1.0]

**Traces to**: AC-80, AC-24; RESTRICT-07, RESTRICT-10 | **Data**: many-small (DS-01, 10,000 files), multiblock-100 (DS-34), big-file 5 GiB slice for the control blob | **Env**: C, group X (retention 0 and short `gc_claim_lease` by config) | **Status**: READY (AC-80 text)

**Fault injection**: on a stack with 10,000 committed files, ctl flips 1 bit in the blob of control file C. Start `sunnycloud scrub run --full --rate 20MiB/s` as a CLI process (about 75 s for the data volume). While it runs: delete 5,000 of the files and replace the content of 1,000 others through the API, with `sunnycloud admin gc-run` in a loop every 2 s so the old blobs are claimed, unlinked and their rows removed during the run; start 100 multi-block uploads (DS-34) and commit them as they finish. Repeat 3 times on fresh stacks.

| Step | Action | Expected Behavior |
|------|--------|------------------|
| 1 | Read the scrub summary (CLI output and the `scrub_summary` audit event) and the `scrub_mismatch` audit events | Exactly 1 mismatch event, for the blob of C; 0 events for any other blob id |
| 2 | ctl probe: count of blobs with `unavailable_at` set | 1 (the blob of C) |
| 3 | Download the 100 uploaded files and 20 random replaced files | 200, hash equals the manifest |
| 4 | Run a second `scrub run --full` after GC drained | Same single mismatch; exit code 2 (findings) in both runs |

**Pass criteria**: blobs reported as corrupt or missing that were uploading, committed after the run started, or collected by GC during the run = 0 (exact); the control corruption reported 1 of 1 (the scrub is not trivially silent); live files modified or deleted by the scrub = 0.

---

## Gaps

No scenario in this file is BLOCKED. Remaining open items do not stop a verdict:

- **G-R3** (test data) many-small files are at most 1 MiB (single block); NFT-RES-03 needs 100 multi-block files, defined here as seeded 64 MiB slices of big-file. Add this set to `data_parameters.md`.
- **G-R4** AC-41 covers restores only. For a device offline longer than 90 days, the outcome for a locally unchanged file that another device deleted (removed locally or restored) is not specified; NFT-RES-08 records it and asserts only that no edited or new file is lost.
- **G-R5** The "one batch" window of AC-29 is undefined; NFT-RES-12 uses a burst within 5 s.
- **G-R9** Default values of the per-account session cap and the free-space floor are not fixed; NFT-RES-05 reads them from the deployed config.
- **G-R12** Cursor above the server head, or unknown epoch after a restore without epoch rotation, returning `resync_required` is in `b_server_storage` #6 but not in an AC; NFT-RES-07 step 4 follows the research.
- **G-R13** (environment) Hardware named in `environment.md` must exist for NFT-RES-15, -16, -17, -27: WIN-VM, LIN-VM, DEV-PIXEL/EMU, MAC for the v1.1 iCloud part. Cloud-evicted files use the Cloud Files test provider; a real OneDrive account is optional.
- **G-R15** `restriction-ids.md` has no IDs yet for the new restrictions bullets (derived blobs, leases and timeouts; Android token and vault passphrase). They are cited as "new restrictions bullet" until IDs exist.
- **G-R18** Power-loss durability (hard VM reset or `dm-flakey` drop of writes) goes beyond AC-38's `kill -9`, which does not discard the page cache; NFT-RES-03 variant B is a proposed extension and stays so.

## AC coverage index

| AC | Tests |
|----|-------|
| AC-1 | NFT-RES-20, NFT-RES-21 |
| AC-2 | NFT-RES-02 |
| AC-3 | NFT-RES-01, -03, -04, -06, -26, -29 |
| AC-4 | NFT-RES-01, -04, -05 |
| AC-5 | NFT-RES-20, NFT-RES-21 |
| AC-6 | NFT-RES-21 |
| AC-7 | NFT-RES-01, -02, -03 |
| AC-8 | NFT-RES-07, -08, -18, -20, -25 |
| AC-9 | NFT-RES-27, -33 |
| AC-10 | NFT-RES-19, -23 |
| AC-11 | NFT-RES-19, -23 |
| AC-12 | NFT-RES-25 |
| AC-19 | NFT-RES-24, -32 |
| AC-21 | NFT-RES-16, -20, -24, -26, -32 |
| AC-24 | NFT-RES-04, -10, -39 |
| AC-25 | NFT-RES-23 |
| AC-27 | NFT-RES-30 |
| AC-28 | NFT-RES-31 |
| AC-29 | NFT-RES-11, -12 |
| AC-30 | NFT-RES-08, -09, -10, -30 |
| AC-31 | NFT-RES-18 |
| AC-32 | NFT-RES-17 |
| AC-33 | NFT-RES-15 |
| AC-35 | NFT-RES-16, -25 |
| AC-38 | NFT-RES-03, -04, -06, -22 |
| AC-39 | NFT-RES-05, -06 |
| AC-40 | NFT-RES-22 |
| AC-41 | NFT-RES-07, -08, -13 |
| AC-42 | NFT-RES-18 |
| AC-43 | NFT-RES-13 |
| AC-44 | NFT-RES-19 |
| AC-45 | NFT-RES-19 |
| AC-46 | NFT-RES-28 |
| AC-50 | NFT-RES-29 |
| AC-52 | NFT-RES-27, -28, -33 |
| AC-58 | NFT-RES-33 |
| AC-64 | NFT-RES-13 |
| AC-65 | NFT-RES-10, -13, -14, -37 |
| AC-68 | NFT-RES-34 |
| AC-69 | NFT-RES-35 |
| AC-71 | NFT-RES-36 |
| AC-73 | NFT-RES-37 |
| AC-74 | NFT-RES-38 |
| AC-80 | NFT-RES-39 |

## Restriction coverage index

| Restriction | Tests |
|-------------|-------|
| RESTRICT-07 | NFT-RES-03, -04, -06, -34, -39 |
| RESTRICT-08 | NFT-RES-13 |
| RESTRICT-09 | NFT-RES-01, -03, -04, -05, -06, -07, -08, -20, -34, -35, -38 |
| RESTRICT-10 | NFT-RES-09, -10, -30, -37, -38, -39 |
| RESTRICT-11 | NFT-RES-01, -02, -20, -21, -22, -25, -35 |
| RESTRICT-13 | NFT-RES-19, -23, -36 |
| RESTRICT-14 | NFT-RES-31 |
| RESTRICT-15 | NFT-RES-27 |
| RESTRICT-16 | NFT-RES-01 |
| RESTRICT-17 | NFT-RES-24 |
| RESTRICT-19 | NFT-RES-24, -26, -32, -33 |
| RESTRICT-20 | NFT-RES-24, -32 |
| RESTRICT-24 | NFT-RES-11, -12, -15, -17, -18, -25 |
| RESTRICT-28 | NFT-RES-23 |
| RESTRICT-31 | NFT-RES-07, -10, -13, -14, -37 |
| New bullet: derived blobs encrypted, lease 5 min / 3 attempts, 24 h reaper, 48 h orphan sweep, idempotent batch, interrupted upgrade, 10 s provider timeout, 60 s recovery, no-internet sessions (no ID yet) | NFT-RES-04, -14, -20, -22, -23, -24, -32, -34, -36 |
| RESTRICT-01 to -06, -12, -18, -21 to -23, -25 to -27, -29, -30 | not in this file's area (see functional, performance, security tests) |
