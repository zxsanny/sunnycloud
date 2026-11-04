# Resource Limit Tests

Black-box specifications: resources are observed from outside the product (OS process counters, cgroup stats, `df`, container stats, platform profilers) while public interfaces drive the load. Thresholds come from `_docs/00_problem/input_data/expected_results/results_report.md` and `data_parameters.md`. Gap defaults confirmed by the owner (`gap-decisions.md`, now in `acceptance_criteria.md`, `restrictions.md` and the last section of `results_report.md`) are applied. A scenario marked `Status: BLOCKED — missing data` has no numeric limit: it is run and measured but yields no verdict until the item named in the Gaps section at the end is closed. Hardware the owner has not named yet is also listed there; a run off the reference hardware is `NOT-RUN(HW-REF)`, never substituted.

Common rules:
- Sampling interval is 1 s unless stated; a limit is breached if any sample exceeds it (peak, not average), except where the criterion says "average".
- Memory means resident set size (RSS) of the named process tree unless stated otherwise.
- Each run reports hardware, OS, filesystem and build ID. A run on the wrong host class is "not run", never substituted.

## Test hardware classes

| Class | Definition | Used by |
|-------|------------|---------|
| REF-SRV | Reference server (AC-2): 8 physical cores, 32 GB RAM, NVMe, XFS, 1 GbE, PostgreSQL 18 on the same host, default Compose stack with Caddy, encryption at rest on; exact model recorded per report | RES-LIM-01, 02, 05 |
| REF-LOAD | Separate load-generator host(s), 1 GbE LAN to REF-SRV | RES-LIM-01, 02, 05 |
| REF-CPU4 | CPU-only worker, 4 physical cores (`cpuset` on distinct physical cores, SMT siblings idle), sandbox as shipped (supervisor plus its interactive and general decoder containers on the Docker `internal` network) | RES-LIM-04 |
| REF-NV | REF-CPU4 plus an NVIDIA GPU with NVENC [v1.1]; GPU model owner-held | RES-LIM-09 |
| HW-PHONE-50K | Android phone (DEV-PIXEL class) with the 50,000-asset library from `phone-fixtures`; DEV-IPHONE for v1.1 | RES-LIM-03 |
| HW-DESK-LINUX / HW-DESK-WIN / HW-DESK-MAC | Desktop machines: Ubuntu LTS (LIN-VM; REF-LIN-WAYLAND-NV for GNOME without tray), Windows with Defender real-time protection on (WIN-VM; REF-WIN-WEAK as the weak case), macOS (MAC) [v1.1] | RES-LIM-06, 07 |
| HW-BROWSER | Desktop Chrome and Firefox on REF-LOAD, Safari on MAC (latest stable on test date) | RES-LIM-08 |

---

### NFT-RES-LIM-01: Disk-free floor and session cap under 100 declaring sessions [v1.0]

**Summary**: 100 upload sessions that each declare 50 GiB are refused beyond the per-account session cap and the quota, and free disk space never falls below the configured floor.
**Traces to**: AC-39, RESTRICT-09
**Hardware**: REF-SRV, REF-LOAD

**Preconditions**:
- Blob volume sized to a known free space F0 (for example a dedicated 200 GiB XFS volume); free-space floor FL, per-account session cap C and quota Q set explicitly in the server config and recorded in the report.
- `many-small` unused; payload: 100 sessions from one account (case A) and from 100 distinct accounts with the same quota (case B).

**Monitoring**:
- `df` of the blob volume every 1 s throughout (free bytes).
- Response status of every session-create call; count of accepted and refused sessions; refusal body code.
- After each case: sum of reserved bytes reported through the public quota/usage API.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Case A: one account creates 100 sessions, each declaring 50 GiB, then uploads blocks at full speed in all accepted sessions for 5 min | accepted count, refusals, free space |
| 2 | Case B: 100 accounts each create one session declaring 50 GiB and upload for 5 min | accepted count, free space |
| 3 | Abort all sessions; wait for cleanup | reserved bytes return to 0, free space recovers |

**Duration**: 2 x 5 min upload plus setup, about 20 min
**Pass criteria**: accepted sessions in case A <= C and accepted reserved bytes <= Q; every other session is refused (exact: refused = 100 - accepted; results_report Transfer row 6); in both cases free space >= FL in every 1 s sample; reserved bytes back to 0 after step 3.

---

### NFT-RES-LIM-02: 1,000 stalled HTTP/2 connections [v1.0]

**Summary**: 1,000 stalled unauthenticated HTTP/2 connections cost the server at most 200 MB and slow an authenticated 1 GiB upload by at most 10%.
**Traces to**: AC-59, RESTRICT-11, RESTRICT-12
**Hardware**: REF-SRV (server container memory measured via cgroup `memory.current` of the API container; Caddy measured separately and reported), REF-LOAD (20 ms RTT for the upload)

**Preconditions**:
- Server idle for 5 min; baseline RSS B0 taken; baseline authenticated upload of a 1 GiB random file measured 5 times (median T0).
- Stall tool opens 1,000 TCP+TLS+HTTP/2 connections and then sends nothing, or sends partial headers / half-open streams at 1 byte per 10 s (both variants).

**Monitoring**:
- API server RSS every 1 s; Caddy RSS and file-descriptor count every 5 s.
- Upload wall time and goodput; connection count seen by the server.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Open the 1,000 stalled connections (idle variant); wait 2 min | RSS increase over B0 |
| 2 | While they are held, upload the 1 GiB file 5 times | upload time Tn |
| 3 | Repeat steps 1 and 2 with the slow-header variant | same |
| 4 | Close the connections | RSS returns within 20% of B0 within 2 min |

**Duration**: about 30 min
**Pass criteria**: peak RSS increase <= 200 MB in both variants (results_report Auth row 11); median of the five Tn <= 1.10 x T0 in both variants; all uploads hash-equal; no 5xx for the authenticated client.

---

### NFT-RES-LIM-03: Mobile uploader memory during background runs and first sync [v1.0 Android, v1.1 iOS]

**Summary**: The shared Rust uploader keeps process memory <= 150 MB in background runs and during the 50,000-asset first sync with forced kills.
**Traces to**: AC-52, RESTRICT-15
**Hardware**: HW-PHONE-50K (Android; iOS at v1.1), Wi-Fi, REF-SRV

**Preconditions**:
- 50,000-asset library installed; app freshly installed and signed in; battery optimisation exempted (Android).
- RSS read from outside the app: `dumpsys meminfo <package>` (PSS and RSS total) every 5 s on Android; Instruments on iOS [v1.1]. All processes of the package are summed (UI process plus WorkManager worker process).

**Monitoring**:
- Peak RSS across the whole run; RSS at 1-minute marks; process restarts (from `logcat`/exit info).

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Start the first sync with the screen off (background run); let it run 2 h or until finished | RSS timeline |
| 2 | Force-kill the process 10 times at random points (same run as NFT-PERF-23) | RSS after each restart |
| 3 | Add 100 photos while the app is in background | RSS during catch-up |

**Duration**: up to 6 h (shares the run with NFT-PERF-23)
**Pass criteria**: peak RSS <= 150 MB in every sample, including the first 60 s after each restart (results_report Photos row 10). The "one implementation serves every mobile entry point" part of AC-52 is a code-review item, not tested here.

---

### NFT-RES-LIM-04: Decoder process CPU, memory and time limits on hostile media [v1.0]

**Summary**: Every red-team sample ends in job failure with the decoder staying inside its CPU, memory and wall-time limits (photos 2 CPU, 2 GiB, 120 s; video 4 CPU, 4 GiB, 40 min) and with no exposure.
**Traces to**: AC-25, RESTRICT-13
**Hardware**: REF-CPU4, sandbox as shipped; HEIC profile included

**Preconditions**:
- `redteam-media` uploaded: SVG and PDF renamed .png, HLS playlist referencing `/etc/passwd`, 30000x30000 PNG, 20 GB sparse video, truncated HEIC.
- Limits (AC-25): photo jobs 2 CPU / 2 GiB / 120 s; video jobs 4 CPU / 4 GiB / 40 min. The shipped worker configuration is read first and must equal these values (cgroup `cpu.max`, `memory.max` of each decoder container, job time-out). Sample classes: photo = SVG and PDF renamed .png, 30000x30000 PNG, truncated HEIC; video = HLS playlist referencing `/etc/passwd`, 20 GB sparse video.

**Monitoring**:
- Decoder container cgroups (interactive and general): `cpu.stat` (usage per 10 s window), `memory.peak`, OOM-kill events; wall time per job.
- Job final state through the job-status surface; supervisor and API availability (`/healthz` every 1 s).
- Inside the decoder (via `docker inspect`/`exec` from the harness): network interfaces, mounts, environment for database credentials.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Process each sample, one at a time, 3 repetitions | final job state, peak memory, CPU, wall time |
| 2 | Process all samples at once with 4 jobs in parallel | same; host load average |

**Duration**: about 2 h (the 20 GB sparse video may run up to the 40 min limit)
**Pass criteria**: every job ends `failed` (or `rejected`), never `succeeded` with output (results_report Auth row 12: 0 exposures); configuration equals the limits above; decoder `memory.peak` <= 2 GiB (photo) / 4 GiB (video); CPU use averaged over every 10 s window <= 2.0 cores (photo) / 4.0 cores (video); every job reaches its terminal state at or before the wall-time limit plus 10 s (kill and report allowance, a test tolerance); (results_report, added thresholds row 5); no decoder network interface other than loopback/internal, no blob mount, no DB credential variable; supervisor and `/healthz` keep answering throughout.

---

### NFT-RES-LIM-05: Scrub throttle against user transfers [v1.0]

**Summary**: While a scrub of stored blobs runs, concurrent user transfers keep at least 90% of their baseline throughput.
**Traces to**: AC-24, RESTRICT-08
**Hardware**: REF-SRV, REF-LOAD (20 ms RTT)

**Preconditions**:
- Store seeded with at least 500 GiB of blobs (so a scrub runs for several minutes); one blob with a flipped bit on disk; scrub started with `sunnycloud scrub run` (or by the schedule).

**Monitoring**:
- Disk `iostat` (read MB/s, util %) every 1 s; scrub CPU share; goodput of a reference 5 GiB upload and download (as NFT-PERF-02/03) before, during and after the scrub.
- Scrub report: mismatches found, duration.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Measure reference upload and download goodput with no scrub (3 runs each) | baseline G0 |
| 2 | Start the scrub; repeat the same transfers during it | goodput Gs |
| 3 | Wait for the scrub to end | report lists the flipped-bit blob |

**Duration**: about 1 h
**Pass criteria**: scrub report lists exactly the corrupted blob (results_report Auth row 13); for both upload and download, the mean goodput of the 3 runs during the scrub is >= 0.9 x the median baseline goodput G0 (results_report, added thresholds row 4); the scrub is still running during every measured run (otherwise the run is repeated).

---

### NFT-RES-LIM-06: Desktop daemon idle CPU and memory with 100,000 watched files [v1.0 Linux, Windows; v1.1 macOS]

**Summary**: With `tree-100k` watched, the daemon uses <= 1% CPU when idle and <= 100 MB memory, and initial hashing takes <= 25% of one core while the user is active; the settings window is not counted.
**Traces to**: AC-15, RESTRICT-06, RESTRICT-25
**Hardware**: HW-DESK-LINUX, HW-DESK-WIN (Defender on), HW-DESK-MAC [v1.1]; REF-WIN-WEAK as the weak Windows case

**Preconditions**:
- Server reachable; tray absent (GNOME without extension) or present; settings window closed. For step 2 onward, `tree-100k` is fully scanned and initial hashing is complete, with no filesystem activity in the tree.
- "User active" (step 1): a scripted input injector (xdotool / SendInput / cliclick) sends a key or mouse event every 5 s and a foreground editor window is open for the whole hashing phase.

**Monitoring**:
- Daemon process (not the settings window, not the tray if it is a separate process): CPU % and RSS every 5 s (`pidstat`/`ps` on Linux, Performance counters on Windows, `top` on macOS). Peak RSS also sampled during scan and hashing.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Add the folder with the user-active injector running; observe RSS and CPU during the first scan and the whole initial hashing phase (CPU sampled every 1 s as process CPU time per core-second) | peak RSS; mean CPU of one core per 10 s window during hashing; hashing progress counter |
| 2 | After hashing completes, leave the daemon idle for 30 min | CPU % per sample; mean CPU; RSS |
| 3 | Make 100 changes spread over the tree during minute 20 | RSS afterwards |

**Duration**: about 2 h per OS
**Pass criteria**: peak RSS <= 100 MB in all phases (scan, hashing, idle); during hashing with the user active, CPU averaged over every 10 s window <= 25% of one core (results_report, added thresholds row 3) and the hashing progress counter still advances at least every 60 s (throttled, not stalled); idle CPU, averaged over each 60 s window, <= 1% in all 30 windows (results_report Desktop row 1). Scan time is in NFT-PERF-24.

---

### NFT-RES-LIM-07: Exclusively locked file causes no CPU spin [v1.0]

**Summary**: A file held under an exclusive lock for 60 s is retried without CPU spin and uploaded within 10 s after release.
**Traces to**: AC-33
**Hardware**: HW-DESK-WIN (primary, exclusive-lock semantics); HW-DESK-LINUX (advisory lock via `flock` held by a harness process)

**Preconditions**: daemon idle with a synced folder; a 50 MiB file in the folder.

**Monitoring**: daemon CPU % every 1 s; server revision list of the file; deletion events seen on the server.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | A harness process opens the file with an exclusive lock and modifies it, holds the lock 60 s | daemon CPU per second |
| 2 | Release the lock | time until the new revision is on the server |

**Duration**: about 5 min per OS
**Pass criteria**: daemon CPU <= 1% averaged over the 60 s lock window and over every 10 s window within it; new revision on the server <= 10 s after release; 0 delete operations for the file (results_report Desktop row 10).

---

### NFT-RES-LIM-08: Vault 100 MiB memory peaks in browser and mobile [v1.2]

**Summary**: Encrypting and decrypting a 100 MiB vault file, and locking the vault, keeps memory bounded and leaves no vault key in the heap.
**Traces to**: AC-14, AC-62, RESTRICT-23
**Hardware**: HW-BROWSER (page plus module Worker heap), low-end Android phone

**Status**: READY — peak memory at most 250 MB total on iOS Safari (iPhone 12 or newer) and Chrome on a 4 GB Android phone (AC-14, added after the gap review). Previously blocked for lack of this limit. The heap-scan outcome after lock (no vault key, results_report Vault row 7) and the 101 MiB rejection are fully specified and are verdict-bearing. Unlock time is in NFT-PERF-29.

**Preconditions**: `vault-file` 100 MiB and the 101 MiB file; vault created; passphrase known to the harness.

**Monitoring**: heap and WASM linear-memory size every 1 s (Chrome `measureUserAgentSpecificMemory`, Firefox memory report, Safari Web Inspector; Android `dumpsys meminfo`); heap snapshot after lock, searched for the 32-byte vault key and file keys known to the harness.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Upload the 100 MiB file into the vault, then download and decrypt it | peak memory |
| 2 | Lock the vault; take a heap snapshot; scan for key bytes | matches |
| 3 | Try the 101 MiB file | rejected with a clear message |

**Duration**: about 30 min per browser
**Pass criteria**: step 2 finds 0 vault-key matches; step 3 rejects (exact); decrypted file hash equals the original. Peak memory: recorded, no verdict until the owner sets a limit.

---

### NFT-RES-LIM-09: NVENC worker under the production sandbox memory limit (gate S10) [v1.1]

**Summary**: NVENC runs inside the no-network general decoder container with its configured memory limit; a limit that is too tight is detected as a CPU fallback, not as a failed job.
**Traces to**: AC-28, AC-25, RESTRICT-13, RESTRICT-14
**Hardware**: REF-NV

**Preconditions**: NVENC enabled by the operator; general decoder container on the `internal` network with GPU passthrough and the video-job limits of AC-25 (4 CPU, 4 GiB, 40 min); `clip-1080-sdr`.

**Monitoring**: container `memory.peak`, GPU memory (`nvidia-smi`), encoder reported by the job result, job final state.

**Steps**:

| Step | Consumer Action | Observation |
|------|----------------|-------------|
| 1 | Transcode `clip-1080-sdr` with NVENC under the 4 GiB limit | encoder used, peak memory, state |
| 2 | Lower the container memory limit to 3 GiB (below 4 GiB, where the research shows `RLIMIT_AS` breaks NVENC) and repeat | encoder used, state |

**Duration**: about 20 min
**Pass criteria**: step 1 job succeeds with NVENC, `memory.peak` <= 4 GiB and CPU <= 4 cores, valid output (needs REF-NV; otherwise `NOT-RUN(HW-REF)`); step 2 job succeeds with valid output via CPU fallback (never `failed`), no OOM kill of the supervisor.

---

## Gaps

| # | Scenario | Open item |
|---|----------|-----------|
| G1 | RES-LIM-01 | Per-account session cap C and free-space floor FL have no default values in the docs or in the confirmed defaults; they are set and recorded per run. |
| G2 | RES-LIM-03 | RSS definition for mobile (RSS versus PSS, sum of processes) is not stated; summed RSS is used. |
| G3 | RES-LIM-04 | The 10 s CPU averaging window and the 10 s kill-and-report allowance on wall time are test tolerances, not AC values. |
| G4 | RES-LIM-06 | "User active" and the 10 s averaging window for the 25% hashing limit are test definitions; AC-15 does not define them. The idle window (30 min after hashing completes) is likewise a test choice. |
| G5 | RES-LIM-08 | Resolved: AC-14 now states the 250 MB peak-memory limit. |
| G6 | RES-LIM-09, 03, 08 | Owner-held hardware not yet named: REF-NV GPU model, DEV-IPHONE, low-end Android phone, MAC. Scenarios are READY and report `NOT-RUN(HW-REF)` until the hardware exists. |
| G7 | all server scenarios | REF-SRV is fixed in class (8 physical cores, 32 GB, NVMe, 1 GbE); the exact model is owner-held and recorded per report. |

## AC coverage index

| AC | Test IDs |
|----|----------|
| AC-14 | NFT-RES-LIM-08 |
| AC-15 | NFT-RES-LIM-06 |
| AC-24 | NFT-RES-LIM-05 |
| AC-25 | NFT-RES-LIM-04, 09 |
| AC-28 | NFT-RES-LIM-09 |
| AC-33 | NFT-RES-LIM-07 |
| AC-39 | NFT-RES-LIM-01 |
| AC-52 | NFT-RES-LIM-03 |
| AC-59 | NFT-RES-LIM-02 |
| AC-62 | NFT-RES-LIM-08 |

## Restriction coverage index

| Restriction | Test IDs |
|-------------|----------|
| RESTRICT-06 | NFT-RES-LIM-06 |
| RESTRICT-08 | NFT-RES-LIM-05 |
| RESTRICT-09 | NFT-RES-LIM-01 |
| RESTRICT-11 | NFT-RES-LIM-02 |
| RESTRICT-12 | NFT-RES-LIM-02 |
| RESTRICT-13 | NFT-RES-LIM-04, 09 |
| RESTRICT-14 | NFT-RES-LIM-09 |
| RESTRICT-15 | NFT-RES-LIM-03 |
| RESTRICT-23 | NFT-RES-LIM-08 |
| RESTRICT-25 | NFT-RES-LIM-06 |
