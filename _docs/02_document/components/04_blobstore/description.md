# 04 Blobstore

## 1. High-Level Overview

**Purpose**: Owns every byte on the blob volume: layout, encrypted writes and reads, the durability (fdatasync) policy, the master key and key check, the GC queue, retention, orphan sweep, scrub, and operator reinstatement. Nothing else touches `blobs/`.

**Architectural Pattern**: Storage service behind traits (`BlobWriter`, `BlobReader`, `BlobRegistry`, `GcQueue`, sweepers), plus one in-process group-commit sync barrier. One owner of the master key.

**Single responsibility**: "Given an account-scoped blob id, store or return its plaintext safely." It knows nothing about nodes, revisions, quota policy, HTTP or Range headers.

**Upstream dependencies**: helpers 03_clock, 01_config, 04_audit (scrub findings and summaries), 05_ids_hashing, 02_error, 07_db (pool, short-tx runner, account scope; GC and scrub run in system scope) and `contracts` ports only (see Ports). Tables `blobs`, `blob_gc` only. Owner cleanup of rows that reference a blob goes through the `BlobOwnerCleanup` port, implemented by 02_tree (revisions) and 08_media_jobs (derived), so 04 depends on neither.

**Downstream consumers**: through ports only; see Ports.

## 2. Scope

**In**: path layout `blobs/<id[0:2]>/<id[2:4]>/<id>`; file format; XChaCha20-Poly1305 STREAM in 64 KiB chunks with random access; per-blob data key wrapped by the master key; key ring, rotation by rewrap, key check; `fdatasync` group commit; free-space floor probe; GC queue and its sweeper; retention validation (0..90 d); session-reaper file removal; orphan sweep; scrub; reinstate (operator restore); missing-blob inventory; the `.keycheck` and `.epoch` marker files; blob I/O metrics; unavailable-blob tracking.
**Out**: quota arithmetic (07), upload sessions and received-bitmap (03), Range parsing and headers (03, 11), derived rows (08), which revision is current (02), tombstone purge (02). Session reaper rows are 03's; 04 only deletes files and `blobs` rows on request.

## 3. Internal Interfaces

### Interface: BlobWriter

| Method | Input | Output | Async | Errors |
|---|---|---|---|---|
| `plan` | `account_id`, `plaintext_size` | `BlobPlan{blob_id (v4), wrapped_key, master_key_id}`; pure, no I/O | No | `KeyUnavailable` |
| `create_file` | `BlobPlan`, `preallocate: bool` | `BlobHandle` (header written, space reserved with `fallocate`) | Yes | `NoSpace`, `FreeSpaceFloor`, `Io` |
| `open_for_write` | `blob_id`, `wrapped_key`, `master_key_id`, `plaintext_size` | `BlobHandle` (reopen after restart or on another request) | Yes | `BlobMissing`, `KeyUnavailable` |
| `write_block` | `BlobHandle`, `block_index`, `plaintext: Bytes` (whole 2 MiB block, hash already verified by the caller) | `DurableToken`; the bytes are encrypted and written at the deterministic offset. A second write of an index that already holds a block succeeds only with identical bytes (the stored block is decrypted and compared; the stored ciphertext is written back unchanged, never re-sealed, so a re-send after a failed flush really reaches the disk, owner decision 2026-11-12; the token is returned as for a first write; a never-written all-zero region is a first write, a torn region that does not authenticate is `BlockConflict`); different bytes fail with `BlockConflict` and nothing is written (rule decided 2026-11-11, AZ-3721) | Yes | `NoSpace`, `Io`, `OutOfRange`, `BlockConflict` |
| `await_durable` | `DurableToken` | `()`; completes only after the group `fdatasync` that covers the write | Yes | `Io` (the caller must not ack) |
| `write_complete` | `account_id`, `size`, `body: ByteStream` | `WrittenBlob{plan, size}`; file synced, **no row yet** (used for staged small files and derived outputs) | Yes | `NoSpace`, `FreeSpaceFloor`, `SizeMismatch`, `Io` |
| `discard` | `blob_id` | unlinks the file; missing is `Ok` | Yes | `Io` |

`ByteStream` is a neutral `Stream<Item = Result<Bytes, StreamError>>`; no framework request types cross the trait.

### Interface: BlobReader

| Method | Input | Output | Async | Errors |
|---|---|---|---|---|
| `open` | `blob_id`, `wrapped_key`, `master_key_id`, `plaintext_size`, `range: Option<ByteRange>` | `PlaintextStream`; first chunk is read and AEAD-verified before return, later chunks as the stream is polled | Yes | `BlobMissing`, `RangeNotSatisfiable`, `Corrupt`, `KeyUnavailable` |
| `read_head` | same, `max_bytes` (<= 64 KiB) | `Bytes` (type sniffing at commit) | Yes | as above |

Contract: a stream that hits a failed tag yields `Err(Corrupt)` and ends; it never yields a complete-looking short body. The caller (03, 08) passes the row's key material, so 04 does not read `blobs` per request (stateless reader, no cache). A 4 KiB range reads at most 2 chunks.
The internal worker fetch uses this same reader (08 calls `open` with no range); there is no second decrypt path.

### Interface: BlobRegistry (row writes, always inside the caller's transaction)

| Method | Output | Notes |
|---|---|---|
| `register_in_tx(tx, WrittenBlob \| BlobPlan, state)` | inserts the `blobs` row (`uploading` or `committed`) | Caller owns the transaction (03 for sessions, 08 for derived) |
| `mark_committed_in_tx(tx, blob_id, size)` | `uploading -> committed` | called from the commit effect |
| `reinstate_in_tx(tx, blob_id)` | `Reinstated \| BeyondRetention \| FileMissing` | clears the `blob_gc` row, `gc_pending -> committed`; stats the file first (outside the tx) |
| `remove_row_in_tx(tx, blob_id)` | deletes `blobs` + `blob_gc` rows | used by GC and the session reaper |

`BlobCatalog::get_ref(account_id, blob_id) -> Option<BlobRef{blob_id, size, wrapped_key, master_key_id}>` is the only read of `blobs` on the request path; 03 and 08 call it and pass the result to `BlobReader::open`. Account-scoped: a foreign blob id is `None`. `BlobRef` is defined here and only here; the media type of a download is added by 03 in its own download request, not in `BlobRef`.

### Interface: GcQueue

`pending_for_account(account) -> u64` counts the account's blobs not yet removed (02's erase progress waits for zero). `schedule_in_tx(tx, blob_id, reason, due: Due::AfterRetention | Due::Now)` is the single function that writes `blob_gc` and sets `blobs.state = 'gc_pending'` (one repository function owns both writes). `due_at = clock.now + retention` is computed here, so the clock and retention config live in one place. Callers: 02_tree (replace, delete, cascade, account purge with `Due::Now`), 08_media_jobs (derived removed).

### Interface: Sweepers and scrub (each exposes a bounded `run_cycle` as a `Janitor`; the scheduler in the `sunnycloud` binary owns the timing, decision 16)

| Operation | Behaviour |
|---|---|
| `GcSweeper::run_cycle` | Repeats the batch below until no row is due or `gc_cycle_budget` (default 30 s) is spent; a backlog therefore drains in one cycle instead of one batch per `gc_interval`. Batch: a short tx claims up to `gc_batch` due rows by moving `due_at` forward by `gc_claim_lease` (rows stay claimable if the process dies; no tx is held across I/O). Outside any tx: unlink the file (missing is logged, not an error). Second short tx: call every `BlobOwnerCleanup::remove_owner_rows(tx, blob_id)` (the revision row **whatever its state**, the `derived` row), then `remove_row_in_tx`. Returns `GcStats{removed, remaining_due, budget_spent}`. |
| `OrphanSweeper::run_cycle(mode)` | For each of the 65,536 prefix directories: list ids on disk, query `blobs` for the matching uuid range, and treat a file with no row and mtime older than `orphan_age` as an orphan: log, then delete if mode is `delete`. Aborts the whole run if any listing or query fails or is partial. Modes `off`, `report`, `delete`. |
| `Scrubber::start_run(mode, rate)` | `mode` is `Full` (every committed blob) or `Due` (blobs not scrubbed within `scrub_interval`); `rate` is `Option<ByteRate>` (default `scrub_max_rate`; the CLI passes `scrub run --full --rate`). Returns `ScrubRun{started_at}`. `started_at` is the run-start cursor: a run covers only committed blobs with `created_at <= started_at` and `last_scrubbed_at` NULL or older than the threshold (`started_at` for `Full`, `started_at - scrub_interval` for `Due`). Blobs committed after the start belong to the next run, so `done` is defined and a run terminates under live traffic. No scrub table. |
| `Scrubber::run_step(&ScrubRun)` | Select the oldest `batch` rows of the run's set by `last_scrubbed_at NULLS FIRST` (no lock), read ciphertext under the rate cap, verify every chunk tag and the final-flag chunk and the file length. On mismatch: re-read the row in a new short tx; report only if still `committed` (AC-80), set `blobs.unavailable_at` and write a `scrub_mismatch` audit event. Update `last_scrubbed_at`. In the API process the step pauses while the activity gauge is above `scrub_pause_active_streams`; a CLI run has no gauge and is throttled only by `rate`. Returns `StepResult{checked, mismatched, missing, done}`; `done` is true when the run's set is empty. |
| `Scrubber::finish_run` / `status` | `finish_run(&ScrubRun)` writes one `scrub_summary` audit event (integer counts and duration) and sets the scrub metrics (decision 6). `status` returns the oldest `last_scrubbed_at`, the unscrubbed count, the latest `scrub_summary` and the count of blobs with `unavailable_at` set. |
| `ScrubJanitor::run_cycle` | The scheduled form: `start_run(Due, default rate)`, `run_step` until `done` or `scrub_cycle_budget` (default 30 min), then `finish_run`. Scheduled every hour, so the 7-day coverage is spread over many short cycles and an interrupted cycle loses nothing (the set is recomputed from `last_scrubbed_at`). |
| `BlobInventory::find_missing(batch)` | Lists committed rows whose file is absent (throttled stat); feeds `sunnycloud doctor` (AC-73). |
| `MasterKeyring::rewrap_batch(from_id, to_id, n)` | Re-wraps `wrapped_key` for up to n rows in short txs; never rewrites files. The CLI command `admin rewrap` loops it until 0 rows remain (decision 17). |
| `MasterKeyring::init_key_check` | For a restored volume (`admin keycheck-init`, decision 17). First unwraps `wrapped_key` of a random sample of up to 100 existing committed blobs with the candidate key (authenticated unwrap); any failure returns `KeyCheckSampleFailed{blob_id}` and nothing is written, so a wrong key can never become the installation's check value. On success it writes `.keycheck`; audited as `admin_action`. An empty `blobs` table skips the sample. |
| `EpochMarker::write(epoch)` / `read()` | The only access to `<blob_root>/.epoch` (decision 5). `write` is called by 05 when it chooses the installation epoch (first account creation, after commit) and after every epoch bump commits. |

`BlobOwnerCleanup` (port in `contracts`, implemented by 02_tree and 08_media_jobs): `remove_owner_rows(tx, blob_id)`. 02 deletes the revision row whatever its state (the FK `ON DELETE SET NULL` clears `nodes.current_revision_id`); 08 deletes the `derived` row.

**Doctor checks** (`DoctorCheck` impls, port in `contracts`): master key and key check; blob volume writable, filesystem, free space under the floor; retention against `--backup-interval`; missing blobs (`find_missing`). The **missing epoch bump** check (decision 5) is 05's `DoctorCheck`: it reads the marker through `EpochMarker::read` and compares it with the database epoch.

### Startup and health

`MasterKeyring::load_and_verify() -> Result<Keyring, KeyError>` with `FileMissing`, `WrongLength` (must be exactly 32 bytes), `KeyCheckMismatch`, `KeyCheckAbsentButBlobsExist`. Any error stops the server before it binds a listener with the operator message "data is unreadable without the master key". `BlobHealth::probe() -> {writable, free_bytes, below_floor, unavailable_count}` (`unavailable_count` = `SELECT count(*) FROM blobs WHERE unavailable_at IS NOT NULL`) for `/readyz`, `/metrics` (`blob_free_bytes`, `blob_free_floor_bytes`, `block_fsync_duration_seconds`, `blob_read_duration_seconds`).

## Ports (crate `contracts`)

| Direction | Port | Counterpart | Used for |
|---|---|---|---|
| Provides | `BlobWriter`, `BlobReader`, `BlobRegistry`, `BlobCatalog` | 03, 08 | upload, download, rows, derived blobs, internal plaintext fetch |
| Provides | `GcQueue` | 02, 08 | schedule removal of replaced, deleted and derived blobs (account erase uses `Due::Now`, driven by the `AccountLifecycleHook`s of 02 and 08) |
| Provides | `Janitor` (`GcSweeper`, `OrphanSweeper`, `ScrubJanitor`) | 12 scheduler | timed cycles |
| Provides | `Scrubber` (`start_run`, `run_step`, `finish_run`, `status`), `BlobInventory` (`find_missing`), `MasterKeyring` (`load_and_verify`, `init_key_check`, `rewrap_batch`), `GcSweeper::run_cycle` | 12 | `scrub run [--full] [--rate]`, `scrub status`, `admin rewrap`, `admin keycheck-init`, `key-check`, `admin gc-run`, doctor |
| Provides | `EpochMarker` (`write`, `read`) | 05, 12 | `.epoch` marker (05 writes it; 12 `serve` refuses to start when the database epoch is below it) |
| Provides | `DoctorCheck` impls, `HealthContributor` impl | 12, 07 | doctor and health aggregation |
| Requires | `BlobOwnerCleanup` | 02, 08 | remove rows that point at a collected blob |

## 4. File Format and Encryption

- **File**: fixed 64-byte header (magic, format version, STREAM nonce prefix, zero pad), then plaintext chunks of 64 KiB, each stored as chunk + 16-byte tag. Chunk `i` lives at `64 + i * (65536 + 16)`, so a block (32 chunks) is written at a deterministic offset from any lane, and a range maps to a chunk range without an index.
- **Nonce**: STREAM layout (prefix, 32-bit chunk counter, last-chunk flag); the last chunk of the declared size carries the final flag (size is declared at start, so out-of-order writers know it). AAD = `blob_id`, so a chunk cannot be moved between blobs.
- **Key hierarchy**: random 256-bit data key per blob; wrapped with the master key of id `master_key_id` using XChaCha20-Poly1305, AAD = `blob_id`; stored in `blobs.wrapped_key` (nonce, ciphertext, tag). No key cache: unwrap is a single AEAD call per request.
- **Implementation note**: the sequential `aead-stream` encryptor cannot jump to chunk `i`, so chunks are sealed with the raw cipher using a hand-built STREAM-format nonce; the pinned `aead-stream =0.6.0` stays as a test oracle. Known-answer tests cover the nonce construction itself: prefix, counter and final flag at chunk 0, 1, n-1, the single-chunk file, the empty file, and refusal at the 32-bit counter limit; each vector is also checked byte for byte against `aead-stream`.
- **Key check**: `<blob_root>/.keycheck` holds, per key id, a sealed constant. Created on first start only when the `blobs` table and the volume are both empty; otherwise a missing file is `KeyCheckAbsentButBlobsExist` and the operator runs `sunnycloud admin keycheck-init` after confirming the key.
- **Epoch marker**: `<blob_root>/.epoch` holds the installation epoch as decimal text, written when the installation epoch is first chosen and after every bump. It lets `doctor` notice a database restore without a bump when the blob volume was not rolled back; if volume and database are both restored to the same point, the marker matches and only the client-side check `S > seq` (05) protects devices that were ahead. The master key file is mounted in the API container only and never lies on the blob volume.
- **Rotation**: ring of `{id, file}`; one current id for new blobs; old ids are read-only until `rewrap` reports 0 rows left and backups no longer need them.

## 5. Durability Policy (fdatasync)

- `write_block` does `pwrite` on the blocking pool, then registers the file in the **group-commit barrier**. The barrier is leader/follower: the first waiter starts a flush at once (after the optional `sync_coalesce_delay`, default 0), runs `fdatasync` on every dirty file in the group, then releases all waiters; writers arriving during a flush join the next one. No artificial 1 s wait, so a 4-lane 2 MiB window keeps throughput near line rate while sync cost is amortised across lanes and sessions.
- A block is acked to the client only after `await_durable` returns. The caller (03) then sets its bit in memory and checkpoints the bitmap to the DB later (every 8 MiB or 1 s); a block acked but not checkpointed is re-requested after a crash (at most 8 MiB, inside AC-3).
- Space is preallocated at create (`blob.preallocate`, default on) so a running upload cannot hit `ENOSPC`; reads of never-written extents yield zeros, which is exactly why the bitmap moves only after the sync. The directory entry is fsynced once at file create.
- `write_complete` (small and derived blobs) writes the whole object, syncs once, then returns; the row is inserted by the caller afterwards (crash window leaves an orphan, swept after `orphan_age`).
- Nothing in 04 holds a DB transaction across a sync, a read, an unlink or a directory walk.

## 6. Data Owned

| Table | Columns owned | Notes |
|---|---|---|
| `blobs` | all, including nullable `unavailable_at` (first time a read or scrub found the file missing or corrupt; cleared by a clean re-read or `reinstate`; decision 11) | rows inserted by registry methods from the caller's tx |
| `blob_gc` | all | only `GcQueue` writes it |
| Volume files | `blobs/**`, `.keycheck`, `.epoch` | no other process mounts the volume (worker and decoder never do) |

Estimates: one `blobs` row about 150 B; 2.5 M revisions plus derived about 5 M rows in 1 year (500 users), under 1 GB. Scrub index `(last_scrubbed_at NULLS FIRST, id) WHERE state = 'committed'` is the only hot index.

## 7. Configuration (durations from config; test values short; key names and env names are those of helper 01)

| Key (env) | Default | Notes |
|---|---|---|
| `SUNNYCLOUD_RETENTION` | 7d | validated 0..90d at start (release builds); test image relaxes the floor |
| `SUNNYCLOUD_ORPHAN_SWEEP_AGE` | 48h | |
| `SUNNYCLOUD_ORPHAN_MODE` | `delete` | `report` recommended after a restore (CLI prints this) |
| `SUNNYCLOUD_ORPHAN_SWEEP_INTERVAL` | 24h | |
| `SUNNYCLOUD_BLOB_FREE_RESERVE` | 20GiB | free-space floor |
| `SUNNYCLOUD_GC_INTERVAL` / `SUNNYCLOUD_GC_BATCH` / `SUNNYCLOUD_GC_CLAIM_LEASE` / `SUNNYCLOUD_GC_CYCLE_BUDGET` | 5m / 100 / 10m / 30s | the cycle drains until nothing is due or the budget ends |
| `SUNNYCLOUD_SCRUB_INTERVAL` / `SUNNYCLOUD_SCRUB_CYCLE_BUDGET` / `SUNNYCLOUD_SCRUB_MAX_RATE` / `SUNNYCLOUD_SCRUB_PAUSE_ACTIVE_STREAMS` | 7d / 30m / 50MiB/s / 8 | interval = target coverage period; the pause threshold counts active block writes and downloads (API process only) |
| `SUNNYCLOUD_BLOB_SYNC_COALESCE_DELAY` | 0ms | max wait before a flush starts |
| `SUNNYCLOUD_BLOB_PREALLOCATE` | true | |
| `SUNNYCLOUD_BLOB_READ_AHEAD_CHUNKS` | 16 | download read-ahead (1 MiB) |
| `SUNNYCLOUD_BLOB_IO_THREADS` | 2 x cores | blocking pool for pread/pwrite/AEAD |
| `SUNNYCLOUD_BLOB_ROOT` | blob mount in the api container | the only path 04 touches |
| `SUNNYCLOUD_MASTER_KEY_FILE` / `SUNNYCLOUD_MASTER_KEY_RING` / `SUNNYCLOUD_MASTER_KEY_CURRENT` | `/run/secrets/master_key` (id 1) / empty / highest id | ring `id=path,...` for rotation; secrets only from files |

## 8. Security Notes

- A stolen disk without the master key yields no plaintext (AC-43). Wrong or missing key stops startup; no code path serves bytes after a failed tag.
- Master key and data keys never appear in logs, metrics or error bodies; error text carries ids only. Key material buffers are zeroized on drop.
- Blob ids are server-generated v4 UUIDs; callers pass an account-scoped row, and the path never contains user input (no traversal).
- Corrupt detection on a download writes one `scrub_mismatch` audit row per blob per process run and sets `blobs.unavailable_at` (health shows degraded from the column, so it survives a restart without any rehydration).
- Error mapping: all failures are typed (`BlobError`); no silent catch. An unlink of a missing file during GC is the single documented best-effort case and is logged.

## 9. ACs Implemented

| AC | Release | Contribution |
|---|---|---|
| AC-24 | v1.0 | Scrub with throttle, findings, pause under load |
| AC-30 | v1.0 | Retention, GC queue, reinstate; account deletion via `Due::Now` |
| AC-38 | v1.0 | Ack only after group fdatasync |
| AC-39 (part) | v1.0 | Free-space floor check; reservation/session cap are 03/07 |
| AC-43 | v1.0 | Envelope encryption, rotation by rewrap, 4 KiB range <= 5 ms p95, derived blobs identical |
| AC-7 (at rest) | v1.0 | Per-chunk AEAD verification on read and scrub |
| AC-27 (files) | v1.0 | GC removes account blobs and derived blobs |
| AC-65 (part) | v1.0 | Key check, reinstate, doctor inventory for restore |

**Interaction ACs folded in**
- **AC-68**: row exists before the file for sessions; `write_complete` writes the file before any row, so a committed revision can never name a file that is not synced. Orphans are swept after `orphan_age`; quota release is the session reaper's (03).
- **AC-72**: `KeyCheckMismatch` refuses startup; worker fetch of 1 GiB equals the plaintext hash because there is one reader.
- **AC-73**: `BlobMissing` is a typed, non-5xx result; `find_missing` feeds `doctor`; GC and scrub treat a missing file as a finding, not a crash. Restore procedure documents `orphan_mode = report` first.
- **AC-80**: scrub selects only `committed` rows, re-checks the row state after any mismatch, and the GC claim moves `due_at` so a blob under removal is `gc_pending` before its file is touched.

## 10. Tests

FT-P-40, FT-P-41, FT-P-42, FT-P-43, FT-P-06, FT-P-16, FT-P-17, FT-P-18, FT-N-12, FT-N-33, FT-N-35, FT-N-40, NFT-SEC-23, NFT-SEC-24, NFT-SEC-25, NFT-SEC-26, NFT-PERF-02, NFT-PERF-03, NFT-PERF-11, NFT-RES-03, NFT-RES-04, NFT-RES-06, NFT-RES-09, NFT-RES-10, NFT-RES-LIM-01, NFT-RES-LIM-05. New Step 5 scenarios needed for AC-72 (key-check refusal with 1 GiB worker fetch), AC-73 (doctor missing list), AC-80 (scrub during upload and GC). Unit: known-answer vectors for the nonce construction and against `aead-stream`, random-access chunk math, group-commit barrier with fault injection, rewrap idempotence, GC cycle drains a backlog larger than `gc_batch`, `remove_owner_rows` removes a revision row in each state, scrub run terminates while new blobs arrive, `init_key_check` refuses a wrong key when blobs exist.

## 11. Acceptance for the Component

Kill -9 under 100 concurrent uploads leaves 0 zero-filled acked blocks; a 4 KiB range read p95 <= 5 ms; single-stream read and write each at least 90% of 1 Gbps with encryption on; wrong or absent master key refuses start; scrub flags a flipped bit and stays under 10% impact on transfers; GC never removes a blob before `due_at`; coverage 90% on write, read, key and GC paths (critical).

## 12. Risks

| Risk | Mitigation |
|---|---|
| Hand-built nonce departs from `aead-stream` | Known-answer tests of the nonce construction, cross-checked against the crate in CI |
| Home-built random-access use of XChaCha20-Poly1305 STREAM is not reviewed | **Open item for v1.0: external review of the blob encryption** (nonce construction, wrap AAD, rewrite rule) before release; owner item in `canonical-names.md` |
| Two different plaintexts sealed with the same key and nonce if an unacked block is rewritten with other bytes | Decided 2026-11-11 (owner, AZ-3721): `write_block` of an index that already holds a block accepts identical bytes only (decrypt the stored block and compare, a re-send after a crash is identical) and refuses different bytes with `BlockConflict` before anything is sealed or written, so a second plaintext is never sealed at a used nonce. 03 still verifies the hash first (ST-03-2) |
| Group commit stalls on a slow disk (ext4 jbd2) | Measured 2026-11-11 (AZ-3722, informational, not REF-SRV). XFS stays the default. `blob_sync_coalesce_delay` stays 0 ms. Alert `block_fsync_duration_seconds` stays. Probe: 400 writers of 2 MiB blocks, leader/follower group `fdatasync` per dirty file, 3 minutes, equal 20 GiB loop-mounted images on one NVMe (Viper VP4300L, Ryzen 9 3900XT, kernel 7.0.0-42, one run each, XFS run first). At the offered 112 MB/s (PT-03-1 load): XFS ack wait p50 5.1 / p95 13.1 / p99 23.4 ms (max 214), 49 flushes/s, 112 MB/s delivered; ext4 p50 5.7 / p95 14.6 / p99 23.2 ms (max 126), 49 flushes/s, 112 MB/s. Both meet PT-04-3 (p95 <= 100 ms, 250 ms failure, >= 112 MB/s). Saturated (400 writers back to back, 60 s): XFS 508 MB/s, ext4 237 MB/s (XFS about 2.1x); ack waits of 2.5 s (XFS) and 4.4 s (ext4) at p95 are queueing behind about 800 MiB of outstanding blocks, not a stall, and each flush then covers about 400 files, so batching needs no coalesce delay. Operator caveat: ext4 meets the targets at the design load but has half the saturation headroom of XFS and a longer queue when overloaded; recommend XFS, accept ext4 with the alert. Gate S14 status for E-31/c02: PASS (informational; non-reference host); `NOT-RUN(HW-REF)` for the gate itself until REF-SRV exists (R03); repeat on REF-SRV in E-10/c14 (PT-04-3). Probe not merged (outside the repository) |
| Key-check file lost with the volume | `admin keycheck-init` after confirmation; it unwraps a sample of existing blob keys first and refuses on failure |
| Orphan sweep after a restore deletes live data | default abort-on-partial; `report` first after restore; `admin epoch-bump` prints the warning |
| 65,536-directory walk cost | rate-limited, daily, no cursor needed |
| CLI scrub cannot see the API's activity gauge | `--rate` is the only throttle for a CLI run; documented in the restore drill |

## 13. Dependency Graph

Must be implemented after: helpers 01, 02, 03, 05, 07 and `contracts`. Depends on no component crate. Parallel with every other component. Integration tests of 03, 08 and 12 use the real 04 behind the ports.

## 14. Logging

| Level | When |
|---|---|
| ERROR | Corrupt chunk on read, key unwrap failure, I/O error on write, sweep aborted |
| WARN | Free-space floor hit, orphan found (report mode), GC unlink of an already missing file, scrub paused too long |
| INFO | Scrub/GC/orphan cycle start and summary, key rotation batch, startup key check passed |
No per-chunk or per-block lines. Fields: blob id and account id only; never file names or key material. JSON on stdout.

## 15. External review scope (v1.0 release gate)

The blob encryption is reviewed by an external cryptographer before the v1.0 release (owner item in `canonical-names.md`, risk R08). Decided 2026-11-11 (AZ-3721); the owner names the reviewer and the date, and the review must finish before the v1.0 release tag.

**Items to review**
1. The hand-built random-access STREAM nonce: prefix, 32-bit chunk counter and final flag, 64 KiB chunks sealed with the raw XChaCha20-Poly1305 cipher so that chunk `i` is written without the sequential encryptor (section 4).
2. The data-key wrap: a random 256-bit data key per blob, wrapped under the master key of `master_key_id` with AAD = `blob_id`.
3. The key check at start and the re-wrap of data keys under a new master key without rewriting files (`MasterKeyring::rewrap_batch`).
4. The rewrite rule: a second `write_block` of one index accepts identical bytes only and never seals a second plaintext at a used nonce.
5. The file header (magic, format version, nonce prefix) and the truncation defence of the final-chunk flag.

**Evidence package**: this description (sections 4, 8, 12), the contracts `blob-writer.md` and `blob-reader.md`, the known-answer vectors of IT-04-2 for the nonce construction (chunk 0, 1, n-1, single-chunk, empty file, refusal at the counter limit) with their byte-for-byte cross-check against the pinned `aead-stream =0.6.0`, and ST-04-8 for the rewrite rule.

**Out of scope**: the vault construction (E-29, its own review before v1.2); re-encrypting data keys after a master-key leak (R22, accepted).
