# Performance Tests

All scenarios are black-box: they drive public interfaces only (`/api/v1`, the web app, the desktop daemon and settings window, the Android app, the operator CLI/admin surface, the worker job output). Thresholds and data sets come from `_docs/00_problem/input_data/expected_results/results_report.md` and `data_parameters.md`. Gap defaults confirmed by the owner (`gap-decisions.md`, now in `acceptance_criteria.md`, `restrictions.md` and the last section of `results_report.md`) are applied. A scenario marked `Status: BLOCKED — missing data` would have no numeric expected result; none remains in this file. Hardware the owner has not yet named is listed in the Gaps section at the end; a run off the reference hardware is reported `NOT-RUN(HW-REF)`, never substituted.

Common rules:
- Warm-up: discard the first 20 requests/jobs (or 60 s, whichever is longer) unless the scenario says "cold".
- p95 means the 95th percentile over the stated sample count (nearest-rank). Each throughput figure is decimal MB/s (1 Gbps = 125 MB/s; 90% = 112 MB/s, as in results_report).
- Network shaping (20 ms RTT) is applied with `netem` on the load-generator host, not on the server.
- Every run reports the exact hardware, kernel/OS, filesystem and build ID; a run on different hardware is not comparable.

## Test hardware classes

Names follow `environment.md` (Reference hardware classes).

| Class | Definition | Used by |
|-------|------------|---------|
| REF-SRV | Reference server (AC-2): 8 physical cores, 32 GB RAM, NVMe, XFS, 1 GbE, PostgreSQL 18 on the same host, default Compose stack with Caddy, buffering off, blob encryption at rest on. Same host re-run with ext4 where stated. Exact model recorded in every report. | PERF-01 to 11, 20 to 23, 27, 28, 30 |
| REF-LOAD | Separate load-generator host(s), 1 GbE LAN to REF-SRV, `netem` delay for 20 ms RTT where stated | all server scenarios |
| REF-CPU4 | CPU-only worker with 4 physical cores (`cpuset` on distinct physical cores, SMT siblings idle, governor `performance`), no GPU, sandbox on; two such hosts for PERF-17 | PERF-13 to 17, 20, 21 |
| REF-NV | REF-CPU4 plus one NVIDIA GPU with NVENC, driver and container toolkit [v1.1] | PERF-18, 19 |
| REF-WIN-WEAK | 2019 Windows laptop, HDD-class system disk, Defender real-time protection on | PERF-24, 25, 26 |
| REF-LIN-WAYLAND-NV | Ubuntu LTS desktop session on Wayland with an NVIDIA GPU, no environment-variable workarounds | PERF-24, 25, 26, 29 |
| MAC | Apple-silicon Mac: macOS daemon [v1.1] and Safari | PERF-24, 27, 28, 29 |
| DEV-PIXEL / DEV-SAMSUNG / DEV-XIAOMI | The three Android phones of `phone-fixtures`, Wi-Fi, battery optimisation exempted | PERF-12 |
| HW-PHONE-5K / HW-PHONE-50K | Android phone holding 5,000 backed-up assets / phone with the 50,000-asset library (`phone-fixtures`) | PERF-22, 23 |
| HW-BROWSER | Desktop Chrome and Firefox on REF-LOAD, Safari on MAC (latest stable on test date), 16 GB RAM, SSD, 1 Gbps to REF-SRV | PERF-27, 28, 29 |

---

## Throughput and scale

### NFT-PERF-01: 100 concurrent syncing sessions at aggregate 1 Gbps [v1.0]

**Summary**: One installation serves 500 registered accounts and 100 concurrent syncing sessions (70% idle long-poll, 20% small-file edits, 10% large transfers) at an aggregate load of 1 Gbps without failed sessions.
**Traces to**: AC-1, RESTRICT-07, RESTRICT-09, RESTRICT-11, RESTRICT-16
**Metric**: failed sessions, 5xx rate, achieved aggregate throughput (recorded)
**Hardware**: REF-SRV + REF-LOAD (several hosts so the load generator is not the bottleneck)
**Data**: 500 accounts (seeded); `many-small` files for the edit sessions, `big-file` for the large transfers

**Preconditions**:
- 500 accounts created; each holds at least 1 GiB of seeded data; encryption at rest on; Caddy buffering off.
- Session mix (AC-1): 70 idle sessions parked on the held long-poll of their account cursor; 20 sessions making small-file edits (one `many-small` file of 1 KiB to 1 MiB every 2 s, single-request upload plus batch commit); 10 sessions doing large transfers (alternating upload and download of `big-file`, each shaped to 100 Mbit/s so the 10 together offer 1 Gbps, 125 MB/s, towards REF-SRV). The per-session rates are test parameters chosen to realise the AC mix.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Warm up: ramp from 0 to 100 sessions over 2 min | none |
| 2 | Hold the 70/20/10 mix, 100 concurrent sessions from 100 distinct accounts, at 1 Gbps aggregate for 10 min | count of failed sessions (including dropped long-polls); HTTP status counts; achieved MB/s every 10 s |
| 3 | Rotate to a second set of 100 accounts and repeat for 10 min, so all 500 accounts have connected at least once in the run | same |

**Pass criteria**: 0 failed sessions; 5xx rate <= 0.1% of requests (results_report Transfer row 8). Achieved aggregate throughput is recorded (no floor stated, see Gaps).
**Duration**: 2 min ramp + 2 x 10 min

### NFT-PERF-02: Single large upload reaches 90% of 1 Gbps with encryption at rest (gate S1) [v1.0]

**Summary**: A single 5 GiB upload at 20 ms RTT reaches >= 112 MB/s with blob encryption on, large HTTP/2 windows and proxy request buffering off.
**Traces to**: AC-2, AC-43, RESTRICT-08, RESTRICT-11, RESTRICT-16
**Metric**: goodput = file bytes / (session create to final commit response)
**Hardware**: REF-SRV (NVMe), REF-LOAD client with NVMe source disk
**Data**: `big-file` 5 GiB

**Preconditions**:
- Named conditions all true and recorded: SSD/NVMe on both ends; RTT 20 ms (measured, <= 20 ms); Caddy `request_buffers` off with body limit above 2 MiB; blob encryption on (per-blob key, 64 KiB chunks); server idle; link verified by `iperf3` at 20 ms RTT >= 940 Mbit/s.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload `big-file` with the reference client (4 lanes, 2 MiB blocks), 1 warm-up run discarded | wall time, goodput |
| 2 | Repeat 5 runs, fresh target path each | goodput of each run |
| 3 | Compare uploaded BLAKE3 with source | hash |

**Pass criteria**: every one of the 5 runs >= 112 MB/s (>= 90% of 1 Gbps; results_report Transfer row 2) and hash equal.
**Duration**: about 10 min (5 x ~45 s plus setup)

### NFT-PERF-03: Single large download reaches 90% of 1 Gbps (gate S1) [v1.0]

**Summary**: Downloading the same 5 GiB blob streams through decryption at >= 112 MB/s.
**Traces to**: AC-2, AC-7, AC-43, RESTRICT-08, RESTRICT-16
**Metric**: goodput of the download, hash equality
**Hardware**: REF-SRV, REF-LOAD (20 ms RTT)
**Data**: `big-file` 5 GiB uploaded in NFT-PERF-02; server page cache dropped before each run (cold) plus 1 run warm

**Preconditions**: as NFT-PERF-02; no concurrent load.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Download the file (cold cache) 5 times | goodput of each run |
| 2 | Compute BLAKE3 of each downloaded copy | hash |

**Pass criteria**: every run >= 112 MB/s and hash equals the original (results_report Transfer row 3).
**Duration**: about 10 min

### NFT-PERF-04: Desktop daemon large transfer at 90% of 1 Gbps (gate S1, client side) [v1.0]

**Summary**: The shipped Rust desktop daemon (reqwest, 4 lanes) meets the same 112 MB/s target as the reference client, which settles the open "one shared TCP versus 4 connections" question.
**Traces to**: AC-2, RESTRICT-11
**Metric**: time from the 5 GiB file being closed in the watched folder to the file being present on the server (hash-verified), minus the 2 s settle time (AC-18)
**Hardware**: REF-SRV; desktop client on Linux (Ubuntu LTS, NVMe) and on Windows (NVMe, Defender on), 20 ms RTT

**Preconditions**: as NFT-PERF-02; daemon idle with an empty synced folder.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Copy `big-file` into the synced folder (closed at t0) | server-side commit time t1 (from the revision timestamp via the API) |
| 2 | Download it on a second daemon (Linux) | time to final file on disk |
| 3 | Repeat 3 times per OS | goodput = size / (t1 - t0 - 2 s) |

**Pass criteria**: every run, upload and download, >= 112 MB/s; hashes equal.
**Duration**: about 15 min per OS

### NFT-PERF-05: Interrupted upload resumes with bounded re-send [v1.0]

**Summary**: After a connection drop at 50% the retry re-sends at most 10 MiB and the file is intact.
**Traces to**: AC-3, AC-7, RESTRICT-09, RESTRICT-11
**Metric**: bytes sent on the wire (client-side byte counter on the TCP connection) after the retry, minus the bytes not yet acknowledged at the drop
**Hardware**: REF-SRV, REF-LOAD (20 ms RTT)
**Data**: `big-file` 5 GiB

**Preconditions**: encryption at rest on; reference client with 4 lanes.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Start upload; when the server has acknowledged 50% (2.5 GiB), reset the TCP connections (`tc`/`iptables` drop) | acknowledged byte count A at the drop |
| 2 | Client reconnects and resumes using the session's received-bitmap | bytes re-sent = bytes sent in the second attempt for data blocks already acknowledged before the drop |
| 3 | Commit; download and BLAKE3 | hash |
| 4 | Repeat 10 times, with the drop at random points between 10% and 90%, and once with `kill -9` of the server instead of a connection drop | re-sent bytes per run |

**Pass criteria**: re-sent bytes <= 10 MiB in all 11 runs; final BLAKE3 equals the original (results_report Transfer row 1).
**Duration**: about 25 min

### NFT-PERF-06: Constant-time upload finish, files up to 50 GiB [v1.0]

**Summary**: Finishing an upload does not scale with file size, and a 50 GiB file uploads and downloads intact.
**Traces to**: AC-4, RESTRICT-09, RESTRICT-11
**Metric**: finish time = time from the last block acknowledgement to the commit response
**Hardware**: REF-SRV with >= 200 GiB free disk, REF-LOAD (20 ms RTT)
**Data**: `big-file` 5 GiB; `big-file` 50 GiB (sparse-generated, high-entropy content so encryption and hashing do real work); additionally 1 GiB

**Preconditions**: free-space floor and quota set to admit 50 GiB; encryption at rest on.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload 1 GiB, 5 GiB and 50 GiB, 3 runs each | finish time per run |
| 2 | Download the 50 GiB file and compute BLAKE3 | hash, completion |
| 3 | Compare upload and download hashes with the source | hash |

**Pass criteria**: finish time <= 2 s for every run of every size (results_report Transfer row 4); hashes equal; 50 GiB upload and download complete without error.
**Duration**: about 3 h (dominated by the 50 GiB transfers)

### NFT-PERF-07: 10,000 small files in 60 s [v1.0]

**Summary**: The single-request upload path plus batch commit moves the small-file set in <= 60 s at 20 ms RTT.
**Traces to**: AC-40, RESTRICT-11
**Metric**: total wall time from the first request to the last batch-commit response
**Hardware**: REF-SRV, REF-LOAD (20 ms RTT, one client)
**Data**: `many-small` (10,000 files, 1 KiB to 1 MiB, tree up to 5 levels)

**Preconditions**: empty target account; encryption on; reference client uses single-request upload for files <= 2 MiB and `commits:batch`.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload all 10,000 files (folders created first, included in the time) | wall time |
| 2 | List the tree and compare file count and BLAKE3 of all files | counts, hashes |
| 3 | Repeat 3 times on fresh accounts | wall time per run |

**Pass criteria**: wall time <= 60 s in all 3 runs (results_report Transfer row 7); 10,000 of 10,000 files present with equal hashes.
**Duration**: about 10 min

---

## Sync latency and listing

### NFT-PERF-08: Cross-device sync latency, idle server [v1.0]

**Summary**: From the moment a file is closed on device A until device B holds the complete verified file takes at most 5 s at p95.
**Traces to**: AC-5, RESTRICT-11
**Metric**: latency per AC-5 = moment the file is closed on device A (harness: last byte written to A's local file, upload starts at once) to device B holding the complete file with BLAKE3 verified; both harness devices on REF-LOAD, one clock
**Hardware**: REF-SRV, REF-LOAD (20 ms RTT)
**Data**: random 5 MB files and random 10 MB files

**Preconditions**: A and B are signed in as the same account; B holds a long-poll on the cursor; no other load.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | A edits an existing file (new content, correct base revision), 20 warm-up edits | none |
| 2 | 100 edits with 5 MB files, 100 edits with 10 MB files, one every 3 s | latency per edit; hash equality |
| 3 | Repeat step 2 with two desktop daemons (Linux, same account, each with a synced folder) instead of harness devices: file closed on A to complete verified file on B. This includes the 2 s settle time of AC-18 | latency per edit; hash equality |

**Pass criteria**: p95 <= 5 s for each size in step 2 and in step 3; 400 of 400 contents equal (results_report Sync row 1). Interplay of the 2 s settle with the 5 s limit in step 3: see Gaps.
**Duration**: about 25 min

### NFT-PERF-09: Cross-device sync latency under 100 background uploads, XFS and ext4 (gate S14) [v1.0]

**Summary**: AC-5 still holds while 100 concurrent 100 MiB uploads run, on the documented XFS default; ext4 is measured for the documented comparison.
**Traces to**: AC-5, RESTRICT-07, RESTRICT-09, RESTRICT-11
**Metric**: same as NFT-PERF-08, plus fsync-bound upload block acknowledgement latency (client side) recorded
**Hardware**: REF-SRV with XFS blob volume; then the same host reformatted with ext4
**Data**: 100 accounts each uploading 100 MiB random files back to back (100 concurrent 100 MiB uploads at all times); 5 MB and 10 MB observer edits

**Preconditions**: background load = 100 concurrent 100 MiB uploads (AC-5), unshaped on the shared 1 GbE link; encryption on; observers use separate accounts.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Start 100 background upload sessions; wait 2 min | none |
| 2 | Run 200 observer edits (100 x 5 MB, 100 x 10 MB) A to B, one every 3 s; latency as in NFT-PERF-08 | latency, hash equality |
| 3 | Stop the load; reformat the blob volume as ext4; repeat steps 1 and 2 | same |

**Pass criteria**: XFS: p95 <= 5 s for each size, 200 of 200 hashes equal (results_report Sync row 1). ext4: p95 measured and published in the install guide; the acceptance verdict rests on XFS (an ext4 miss changes documentation, not the verdict; see Gaps).
**Duration**: about 40 min

### NFT-PERF-10: Listing a folder with 10,000 entries [v1.0]

**Summary**: Server time to answer a listing of `listing-10k` is <= 300 ms at p95.
**Traces to**: AC-6, RESTRICT-07, RESTRICT-11
**Metric**: server time = time to last byte of the full paged listing, measured from a load client on the same LAN (RTT <= 1 ms, no netem); also per-page time
**Hardware**: REF-SRV, REF-LOAD (LAN)
**Data**: `listing-10k` (one folder, 10,000 entries); the account also holds the 10,000-file `many-small` tree elsewhere

**Preconditions**: PostgreSQL warm; 1 warm-up traversal; no other load.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Traverse all pages of the folder with the default page size, 1,000 times sequentially | full-traversal time per request; time per page |
| 2 | Repeat with 20 parallel clients (20 accounts holding identical folders) | same |

**Pass criteria**: p95 of full-traversal time <= 300 ms in step 1 and step 2 (results_report Sync row 2). Interpretation recorded in Gaps.
**Duration**: about 10 min

### NFT-PERF-11: Range-read cost of encrypted blobs [v1.0]

**Summary**: Range reads and downloads of an encrypted blob meet AC-2; 4 KiB random reads are byte-exact and take <= 5 ms at p95.
**Traces to**: AC-43, AC-2, RESTRICT-08
**Metric**: (a) goodput of a download made of 8 MiB `Range` requests over 4 lanes; (b) latency of 4 KiB `Range` requests at random offsets (request sent to last byte received, LAN RTT <= 1 ms, no netem)
**Hardware**: REF-SRV, REF-LOAD (20 ms RTT for (a); LAN for (b))
**Data**: `big-file` 5 GiB; one 1 GiB blob

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | (a) Download the 5 GiB blob entirely with 8 MiB ranges, 4 lanes, 3 runs | goodput; reassembled hash |
| 2 | (b) 10,000 random 4 KiB range reads from the 1 GiB blob, one at a time, after a server restart (cold) and 20 discarded warm-up reads; compare to the source bytes | bytes equal; latency p50/p95 |

**Pass criteria**: (a) every run >= 112 MB/s and hash equal. (b) all 10,000 reads byte-exact (results_report Encryption row 3) and p95 <= 5 ms (results_report, added thresholds row 1).
**Duration**: about 15 min

---

## Mobile

### NFT-PERF-12: Android camera backup latency per device (gate S9) [v1.0]

**Summary**: A new photo reaches the server within 60 s on Pixel; Samsung and Xiaomi are measured and published per device.
**Traces to**: AC-9, RESTRICT-15
**Metric**: latency = shutter time (MediaStore `date_added`) to the file being listed on the server with correct hash
**Hardware**: DEV-PIXEL, DEV-SAMSUNG, DEV-XIAOMI, each on Wi-Fi, screen off, app in background, battery optimisation exempted, app not force-stopped
**Data**: `photos` (12 MP JPEG, written into MediaStore by the camera app)

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Idle the device 10 min with screen off (Doze reached) | none |
| 2 | Take 30 photos per device, spaced 3 min apart (screen off between), over ~90 min | latency per photo |
| 3 | Publish per-device table (min, p50, p95, max) in the test report | table present |

**Pass criteria**: Pixel: all 30 photos <= 60 s (results_report Photos row 8). Samsung and Xiaomi: no numeric limit by AC design; pass = measured table published for each device with 30 samples and the number of photos > 60 s stated. Hash of every uploaded file equals the original.
**Duration**: about 2 h per device

### NFT-PERF-22: Reinstall uploads zero media bytes [v1.0 Android]

**Summary**: Re-installing the app on a phone with 5,000 already-backed-up assets uploads nothing and links edited copies.
**Traces to**: AC-46, RESTRICT-15
**Metric**: media bytes sent (server-side request-body byte count per account, plus time to reconcile)
**Hardware**: HW-PHONE-5K, Wi-Fi, REF-SRV
**Data**: `phone-fixtures` (5,000 backed-up assets, including 10 edited photos whose originals are also on the server; an edit is linked when the same MediaStore asset ID has a new content hash)

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Record server byte counters; uninstall and reinstall the app; sign in; grant photo access | none |
| 2 | Wait until the app reports backup complete | reconcile time (recorded); upload bytes of media bodies |
| 3 | Inspect the edited photos through the API | linked entry vs duplicate |

**Pass criteria**: 0 media bytes uploaded; 10 of 10 edited photos linked to their originals, 0 duplicate entries (results_report Photos row 9). Reconcile time recorded (no limit given, see Gaps).
**Duration**: about 30 min

### NFT-PERF-23: First sync of 50,000 assets with 10 forced kills; background start latency [v1.0 Android]

**Summary**: The shared Rust uploader survives forced process kills with minimal re-hashing and starts uploading within 10 s of a background run.
**Traces to**: AC-52, RESTRICT-15
**Metric**: re-hashed assets after the kills, duplicate server files, time from background trigger to first upload byte
**Hardware**: HW-PHONE-50K (Android), Wi-Fi, REF-SRV
**Data**: `phone-fixtures` library of 50,000 assets

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Start the first sync; at 10 random points between 5% and 95% progress, kill the app process (`am kill`/force stop equivalent) and let WorkManager restart it | count of assets hashed more than once (app diagnostics export) |
| 2 | Finish the sync | duplicates (server listing by content hash) |
| 3 | 20 times: add a new photo while the app is in background and measure the trigger-to-first-byte time | start latency |

**Pass criteria**: re-hashed assets <= 1% (<= 500 of 50,000); 0 duplicate server files; trigger-to-first-byte <= 10 s in all 20 runs; the run reaches 50,000 of 50,000 uploaded (results_report Photos row 10). Memory is in NFT-RES-LIM-03.
**Duration**: about 6 h

---

## Media pipeline

### NFT-PERF-13: Thumbnail latency for interactive uploads (gate S4) [v1.0]

**Summary**: A thumbnail for a 12 MP photo, JPEG and HEIC, is ready within 5 s of upload completing at p95 via the reserved lane.
**Traces to**: AC-10, RESTRICT-13
**Metric**: time from the final commit response to the thumbnail being fetchable (poll every 100 ms)
**Hardware**: REF-SRV + REF-CPU4 (worker idle apart from this test)
**Data**: `photos` (100 JPEG 12 MP, 20 HEIC of which 10 EXIF-rotated)

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload the 100 JPEGs one at a time, 1 every 5 s; 5 warm-up uploads discarded | thumbnail latency |
| 2 | Upload the 20 HEICs, repeating each 5 times with new names (100 samples) | thumbnail latency per HEIC; orientation of the 10 rotated ones |

**Pass criteria**: JPEG p95 <= 5 s; HEIC p95 <= 5 s (gate S4; threshold taken from AC-10, see Gaps); the thumbnails are valid images with correct orientation.
**Duration**: about 25 min

### NFT-PERF-14: Bulk import of 10,000 photos with interactive p95 (gate S4) [v1.0]

**Summary**: A 10,000-photo import finishes thumbnails in <= 15 min while interactive uploads keep p95 under 5 s.
**Traces to**: AC-10, RESTRICT-13
**Metric**: bulk completion = first bulk upload start to last bulk thumbnail present; interactive thumbnail latency p95 during the bulk window
**Hardware**: REF-SRV + REF-CPU4 (single worker, 4 physical cores)
**Data**: `photos-bulk` (10,000 JPEG 12 MP); interactive: `photos`

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload `photos-bulk` flagged as bulk (as the import client does) at full link speed | bulk completion time |
| 2 | In parallel, from a separate account upload 1 interactive photo every 10 s until the bulk finishes (about 90 photos) | latency of each interactive thumbnail |

**Pass criteria**: bulk finishes <= 15 min after the first upload started; interactive p95 <= 5 s (results_report Photos row 2); 10,000 of 10,000 thumbnails present.
**Duration**: about 20 min

### NFT-PERF-15: Transcode 1080p SDR clip on 4 physical cores (gate S5) [v1.0]

**Summary**: A 10-minute 1080p 30 fps SDR clip transcodes within 5 min on the CPU reference worker.
**Traces to**: AC-11, RESTRICT-13, RESTRICT-14
**Metric**: job wall time from the worker claiming the job to the result being written back
**Hardware**: REF-CPU4, NVENC disabled (default)
**Data**: `clip-1080-sdr`

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload the clip; one job in the queue, worker otherwise idle | job time |
| 2 | Repeat 3 times; `ffprobe` the output | codec, profile, size, audio |

**Pass criteria**: every run <= 5 min; output is MP4, H.264 High, 1080p, AAC, playable end to end (results_report Photos row 3).
**Duration**: about 20 min

### NFT-PERF-16: Transcode 4K HLG clip to 1080p SDR (gate S5) [v1.0]

**Summary**: A 10-minute 4K HEVC HLG clip is tone-mapped to 1080p SDR within 20 min.
**Traces to**: AC-11, RESTRICT-13
**Metric**: job wall time; colour tags of the output
**Hardware**: REF-CPU4, NVENC disabled
**Data**: `clip-4k-hlg`

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload the clip, one job, worker idle; 3 runs | job time |
| 2 | `ffprobe` the output | resolution, transfer/primaries/matrix tags |

**Pass criteria**: every run <= 20 min; output 1920x1080 SDR with BT.709 colour tags (results_report Photos row 4).
**Duration**: about 70 min

### NFT-PERF-17: Compliant clip never re-encoded; a second worker raises throughput >= 1.7x [v1.0]

**Summary**: A compliant clip is not transcoded, and two workers drain the 1080p clip queue at least 1.7x faster than one.
**Traces to**: AC-11, AC-44, RESTRICT-13, RESTRICT-28
**Metric**: derived transcode count for the compliant clip; jobs completed per hour with 1 and 2 workers
**Hardware**: 2 x REF-CPU4 on separate hosts (no shared cores)
**Data**: `clip-compliant` (precondition: `ffprobe` shows H.264 High or lower, long side <= 1920 and short side <= 1080, <= 10 Mbit/s video, AAC, moov atom first, SDR BT.709); 8 uploads of `clip-1080-sdr` made unique by a distinct container metadata tag (no re-encode) so each is a separate job

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload `clip-compliant` | derived transcode count |
| 2 | Queue the 8 `clip-1080-sdr` uploads with one worker; measure time from first claim to last result | jobs per hour W1 |
| 3 | Repeat with a fresh set of 8 and two workers | jobs per hour W2 |

**Pass criteria**: derived transcode count = 0 for the compliant clip (results_report Photos row 5 and added thresholds row 7); W2 >= 1.7 x W1 (owner-confirmed scaling factor); both drains complete with 8 of 8 valid outputs.
**Duration**: about 90 min

### NFT-PERF-18: NVENC at least 1.3x faster than CPU on 1080p (gate S5) [v1.1]

**Summary**: With NVENC enabled the 1080p reference clip transcodes at least 1.3x faster than on CPU.
**Traces to**: AC-28, RESTRICT-02, RESTRICT-14
**Metric**: ratio of median CPU job time to median NVENC job time for `clip-1080-sdr`
**Hardware**: REF-NV
**Data**: `clip-1080-sdr`

**Status**: READY. Needs REF-NV (owner hardware, GPU model not yet stated); without it the scenario is `NOT-RUN(HW-REF)`.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | NVENC off (default): transcode 3 times | CPU job time |
| 2 | Operator enables NVENC; worker start probe passes; transcode 3 times | NVENC job time, `ffprobe` of output |

**Pass criteria**: ratio >= 1.3 (results_report, added thresholds row 2); every output valid (MP4, H.264 High, 1080p, AAC).
**Duration**: about 30 min

### NFT-PERF-19: NVENC failure falls back to CPU within the CPU time limit [v1.1]

**Summary**: Removing or failing the GPU makes the job complete on CPU with a valid result, still inside the CPU acceptance time.
**Traces to**: AC-28, AC-11, RESTRICT-14
**Metric**: job wall time including the failed GPU attempt; encoder used
**Hardware**: REF-NV
**Data**: `clip-1080-sdr`

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | With NVENC off, start the worker: no NVENC job runs | encoder used |
| 2 | Enable NVENC; worker probe succeeds; job runs on GPU | encoder used |
| 3 | Disable the GPU (driver unload or `CUDA_VISIBLE_DEVICES` emptied) and submit the clip, then fail a job mid-run (kill the NVENC session) | encoder used; job time; output validity |

**Pass criteria**: step 1 no GPU use; step 2 GPU used; step 3 job ends on CPU with valid output (results_report Photos row 12) and total job time <= 5 min (the AC-11 CPU limit, because CPU is the acceptance baseline; derived limit, see Gaps).
**Duration**: about 30 min

### NFT-PERF-20: Derived storage on mixed phone clips [v1.0]

**Summary**: Thumbnails plus transcodes of 100 mixed clips occupy <= 20% of the original bytes.
**Traces to**: AC-44, RESTRICT-13
**Metric**: derived bytes / original bytes
**Hardware**: REF-SRV + REF-CPU4
**Data**: `clip-mixed-100` (rotated, variable-frame-rate, audio-only clips included)

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload all clips; wait until the queue is empty | job success count |
| 2 | Sum the sizes of thumbnail and transcode variants (downloaded through the public API) and divide by the sum of original sizes | ratio |

**Pass criteria**: ratio <= 0.20 (results_report Photos row 6); 100 of 100 jobs finished (skipped-as-compliant counts as finished).
**Duration**: about 2 h

### NFT-PERF-21: Stalled ffmpeg watchdog timing [v1.0]

**Summary**: A hung ffmpeg is killed within 90 s of its last progress line and retried once on CPU.
**Traces to**: AC-45, RESTRICT-13
**Metric**: time from the last progress line to the kill (worker diagnostic log, timestamped); retry count and encoder of the retry
**Hardware**: REF-CPU4
**Data**: `clip-mixed-100` (one clip per run); hang injected by suspending the ffmpeg process (`SIGSTOP`) from the test harness

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Submit a clip; when progress passes 20%, `SIGSTOP` the ffmpeg child | last progress timestamp |
| 2 | Observe the kill and the retry; repeat 5 times | kill delay; retry count; output validity |

**Pass criteria**: kill delay <= 90 s in all 5 runs; exactly 1 retry on CPU; output valid (results_report Photos row 7). Orientation and drift (<= 40 ms) belong to the functional suite.
**Duration**: about 30 min

---

## Desktop

### NFT-PERF-24: First scan of 100,000 watched files per OS (gate S2) [v1.0 Linux, Windows; v1.1 macOS]

**Summary**: The daemon enumerates and stats 100,000 files within 60 s and starts background hashing afterwards.
**Traces to**: AC-15, RESTRICT-06, RESTRICT-25
**Metric**: time from daemon start (folder added) to scan-complete state (status IPC/CLI)
**Hardware**: Linux Ubuntu LTS (NVMe), Windows with Defender real-time protection on (also REF-WIN-WEAK as the weak case), MAC [v1.1]
**Data**: `tree-100k`

**Preconditions**: file-system cache cold (reboot or cache drop); daemon started with no tray; server reachable but initial upload paused for the scan measurement.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Add `tree-100k` as a synced folder | time to scan-complete; 3 runs, cold cache |
| 2 | Let initial hashing start; confirm it runs in the background (status shows hashing while the scan state is already complete) | hashing flag; its CPU limit is checked in NFT-RES-LIM-06 |

**Pass criteria**: scan <= 60 s in all 3 runs on every OS (results_report Desktop row 1); hashing starts after the scan and does not block it. Hashing CPU (<= 25% of one core while the user is active), idle CPU and memory limits are in NFT-RES-LIM-06.
**Duration**: about 30 min per OS

### NFT-PERF-25: Settings window cold start (gate S3) [v1.0]

**Summary**: The settings window is interactive within 1 s at p95 on the weak Windows laptop and on Ubuntu Wayland with NVIDIA, without environment workarounds.
**Traces to**: AC-16, RESTRICT-06
**Metric**: time from the launch command (tray menu click or CLI start) to the first frame that accepts input (UI automation probe: UIA on Windows, AT-SPI on Linux)
**Hardware**: REF-WIN-WEAK and REF-LIN-WAYLAND-NV (separately)
**Data**: settings window of the shipped daemon package

**Preconditions**: no env vars set (`WEBKIT_DISABLE_*`, `SLINT_BACKEND`, etc.); cold = file cache dropped or machine rebooted before each launch; the daemon is running.

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Cold-launch the settings window, close it, drop caches; 40 runs per machine | time to interactive per run |
| 2 | Record RSS of the window process at 5 s after open | memory (informational) |

**Pass criteria**: p95 <= 1.0 s on each machine (results_report Desktop row 5); if either machine fails, the Tauri fallback decision is triggered, not a rerun.
**Duration**: about 40 min per machine

### NFT-PERF-26: Setting change reaches the daemon without restart [v1.0]

**Summary**: A setting changed in the settings window takes effect in the daemon within 5 s.
**Traces to**: AC-16, RESTRICT-06
**Metric**: time from clicking Apply to the daemon reporting the new value (status query) and acting on it
**Hardware**: REF-WIN-WEAK and REF-LIN-WAYLAND-NV
**Data**: setting: add an exclude pattern, then create a matching file

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Change the exclude pattern in the window and Apply; 20 times | apply latency; daemon PID unchanged |
| 2 | After each apply, create a file matching the pattern | file is not uploaded |

**Pass criteria**: latency <= 5 s in 20 of 20 runs; daemon PID unchanged (no restart); 0 matching files uploaded (results_report Desktop row 4).
**Duration**: about 15 min per machine

---

## Web

### NFT-PERF-27: Browser 50 GiB upload memory and resume (gate S12) [v1.0]

**Summary**: A 50 GiB upload from Chrome, Firefox and Safari keeps worker plus page heap <= 150 MB and resumes with <= 10 MiB re-sent.
**Traces to**: AC-50, AC-3, RESTRICT-11
**Metric**: peak (page + worker) heap sampled every 5 s; bytes re-sent after tab close and re-select; restart on mid-upload edit
**Hardware**: HW-BROWSER (Chrome: `performance.measureUserAgentSpecificMemory()` plus DevTools heap; Firefox: memory report of the content process; Safari: Web Inspector heap and Activity Monitor footprint of the web content process), REF-SRV
**Data**: `big-file` 50 GiB (read from local disk)

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Start the upload; sample memory for the full run | peak heap |
| 2 | At about 40%, close the tab; reopen, select the same file | bytes re-sent |
| 3 | Complete the upload; compare BLAKE3 | hash |
| 4 | Start a second upload, modify the source file (change `lastModified` and one byte) at about 30% | upload restarts, state shown |

**Pass criteria**: peak heap <= 150 MB in all three browsers; re-sent <= 10 MiB; final hash equal; edited file detected and upload restarted (results_report Web row 1).
**Duration**: about 4 h per browser

### NFT-PERF-28: Browser 50 GiB download memory (gate S12) [v1.0]

**Summary**: A 50 GiB download completes in Chrome, Firefox and Safari with browser heap <= 200 MB.
**Traces to**: AC-51, AC-7, RESTRICT-11
**Metric**: peak heap (same instruments as NFT-PERF-27); completion; hash
**Hardware**: HW-BROWSER, REF-SRV
**Data**: `big-file` 50 GiB uploaded in NFT-PERF-06

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Download to disk through the web app; sample heap | peak heap |
| 2 | Compute BLAKE3 of the saved file | hash |

**Pass criteria**: download completes, peak heap <= 200 MB in all three browsers (results_report Web row 2); hash equal.
**Duration**: about 3 h per browser

---

## Installation

### NFT-PERF-30: Time to a working server from the README [v1.0]

**Summary**: Following the README with one sign-in provider ready and the default Caddy Compose stack, an operator reaches a working server in at most 30 minutes. The functional checks (services present, no 413, Caddy buffering off) are FT-P-69; this scenario repeats the run for timing and breaks down the phases.
**Traces to**: AC-22, RESTRICT-13, RESTRICT-16
**Metric**: elapsed time from the first README command to "first file uploaded and visible in the web app"; per-phase times (image pull, stack healthy, first admin created, first sign-in)
**Hardware**: clean host of the REF-SRV class with Docker, 1 GbE internet access (image registry reachable), no prior images; one GitHub OAuth app already registered (provider-side registration time is not counted, per AC-22)
**Data**: README at the release commit

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | A scripted operator (command list taken verbatim from the README, with an allowance of 5 s per command for reading and typing) runs the install on a freshly reset host | phase timestamps |
| 2 | Create the first admin with the setup token, sign in through the web, upload a 10-file subset of `many-small` | upload visible with equal hash |
| 3 | Repeat on 3 freshly reset hosts (cold image cache) | total time per run |

**Pass criteria**: total <= 30 min (1,800 s) in every run (results_report Auth row 19); the uploaded files are hash-equal. The scripted operator makes the figure a lower bound; the human walkthrough is part of FT-P-69.
**Duration**: about 2 h

---

## Vault [v1.2]

### NFT-PERF-29: Vault unlock time and 100 MiB round trip (gate S7) [v1.2]

**Summary**: Vault unlock (Argon2id key derivation) takes at most 1 s native and 2 s in the browser; the 100 MiB round trip is recorded and must be hash-exact.
**Traces to**: AC-14, AC-62, RESTRICT-23
**Metric**: unlock time (Argon2id 64 MiB, 3 passes, parallelism 1) from passphrase submitted to vault usable; encrypt and decrypt time of the 100 MiB file (recorded)
**Hardware**: native: low-end Android phone (UniFFI build; model owner-held, not yet stated) and Linux desktop (REF-LIN-WAYLAND-NV); browser: HW-BROWSER (Chrome, Firefox, Safari on MAC)
**Data**: `vault-file` 100 MiB

**Steps**:

| Step | Consumer Action | Measurement |
|------|----------------|-------------|
| 1 | Upload and then download the 100 MiB file through the vault page; 5 runs per browser | times; hash equality |
| 2 | Unlock the vault 20 times per native device and 20 times per browser, each time from a locked state (cold page in the browser) | unlock time per run |

**Pass criteria**: unlock <= 1 s on each native device and <= 2 s in each browser, every run (results_report, added thresholds row 6); decrypted 100 MiB file equals the original in all runs. Encrypt and decrypt times have no limit and are only recorded.
**Duration**: about 1 h

---

## Gaps

No scenario is BLOCKED. Items below are non-blocking ambiguities, assumptions or owner-held hardware.

| # | Scenario | Open item |
|---|----------|-----------|
| G1 | all server scenarios | REF-SRV is fixed in class (8 physical cores, 32 GB, NVMe, 1 GbE); the exact model is owner-held and recorded per report. |
| G2 | PERF-01 | AC-1 gives the mix but not per-session rates or a throughput floor; rates here are test parameters and the achieved aggregate is only recorded. |
| G3 | PERF-02/03 | 112 MB/s is about 95% of the TCP goodput ceiling of 1 GbE (about 117 MB/s); the margin for TLS and HTTP/2 framing is thin. The `iperf3` calibration is recorded next to each result. |
| G4 | PERF-08 step 3 | With desktop daemons the AC-18 settle of 2 s counts inside the 5 s limit (clock starts at file close); the owner has not confirmed that this end-to-end reading is intended. |
| G5 | PERF-09 | The pass limit for ext4 is not stated; the verdict rests on XFS and ext4 is published. |
| G6 | PERF-10 | "Server time" for a paged listing is not defined (first page, any page or whole traversal); the stricter whole-traversal reading is used. |
| G7 | PERF-12 | Samsung and Xiaomi have no numeric limit (publication only, by design); sample count 30 is a test choice. |
| G8 | PERF-13 | HEIC has no own threshold; the 5 s p95 of AC-10 is applied (gate S4 outcome may change this). |
| G9 | PERF-19 | The fallback time limit (5 min) is derived from AC-11, not stated in AC-28. |
| G10 | PERF-22 | No time limit for reinstall reconciliation (recorded only). |
| G11 | PERF-18, 19, 29, 24, 12, 25 | Owner-held hardware not yet named: REF-NV GPU model, low-end Android phone, DEV-PIXEL/SAMSUNG/XIAOMI models, REF-WIN-WEAK, MAC. The scenarios are READY and report `NOT-RUN(HW-REF)` until the hardware exists. |
| G12 | PERF-29 | No time limit for vault encrypt and decrypt of 100 MiB (recorded only). |
| G13 | PERF-30 | Scripted-operator timing is a lower bound for AC-22; the human walkthrough is FT-P-69. |

## AC coverage index

| AC | Test IDs |
|----|----------|
| AC-1 | NFT-PERF-01 |
| AC-2 | NFT-PERF-02, 03, 04, 11 |
| AC-3 | NFT-PERF-05, 27 |
| AC-4 | NFT-PERF-06 |
| AC-5 | NFT-PERF-08, 09 |
| AC-6 | NFT-PERF-10 |
| AC-7 | NFT-PERF-03, 05, 28 |
| AC-9 | NFT-PERF-12 |
| AC-10 | NFT-PERF-13, 14 |
| AC-11 | NFT-PERF-15, 16, 17, 19 |
| AC-14 | NFT-PERF-29 |
| AC-15 | NFT-PERF-24 |
| AC-16 | NFT-PERF-25, 26 |
| AC-22 | NFT-PERF-30 |
| AC-28 | NFT-PERF-18, 19 |
| AC-40 | NFT-PERF-07 |
| AC-43 | NFT-PERF-02, 03, 11 |
| AC-44 | NFT-PERF-17, 20 |
| AC-45 | NFT-PERF-21 |
| AC-46 | NFT-PERF-22 |
| AC-50 | NFT-PERF-27 |
| AC-51 | NFT-PERF-28 |
| AC-52 | NFT-PERF-23 |
| AC-62 | NFT-PERF-29 |

AC-12 (share open and revoke timing), AC-21 (revocation within 5 s) and AC-58 (replay revocation within 5 s) are already timed in the blackbox and security suites (FT-P-36, FT-N-09, NFT-SEC-08, NFT-SEC-10) and are not duplicated here.

## Restriction coverage index

| Restriction | Test IDs |
|-------------|----------|
| RESTRICT-02 | NFT-PERF-18, 19 |
| RESTRICT-06 | NFT-PERF-24, 25, 26 |
| RESTRICT-07 | NFT-PERF-01, 09, 10 |
| RESTRICT-08 | NFT-PERF-02, 03, 11 |
| RESTRICT-09 | NFT-PERF-01, 05, 06, 09 |
| RESTRICT-11 | NFT-PERF-01, 04, 05, 06, 07, 08, 09, 10, 27, 28 |
| RESTRICT-13 | NFT-PERF-13 to 17, 20, 21, 30 |
| RESTRICT-14 | NFT-PERF-15, 18, 19 |
| RESTRICT-15 | NFT-PERF-12, 22, 23 |
| RESTRICT-16 | NFT-PERF-01, 02, 03, 30 |
| RESTRICT-23 | NFT-PERF-29 |
| RESTRICT-25 | NFT-PERF-24 |
| RESTRICT-28 | NFT-PERF-17 |
