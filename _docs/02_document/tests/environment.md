# Test Environment

## Overview

**System under test (SUT)**: sunnycloud v1.0 (v1.1 and v1.2 items are tagged and run only when that release is under test). Entry points:

| Entry point | Address (test stack) | Notes |
|-------------|----------------------|-------|
| Public HTTPS API and web app | `https://sunnycloud.test` -> Caddy :443 -> API, `/api/v1/...` | TLS 1.3, HTTP/2, request buffering off |
| Operator CLI | `sunnycloud admin ...` (incl. `admin restore`, `admin rewrap`, `admin keycheck-init`, `admin epoch-bump`, `admin media-requeue`), `scrub run [--full]`, `doctor`, `key-check` | Reached only through the `e2e-ctl` operator-command action (see Fault injectors) |
| Desktop daemon (Linux, Windows; macOS v1.1) | Real client binaries, driven from outside | Observed through the sync folder, the server API and OS facilities |
| Desktop settings window (Slint) | Separate process of the same package | Driven by UI automation only |
| Android app (v1.0), iOS app (v1.1) | Release-signed builds on emulator or device | Driven with Appium; no source access |
| Worker supervisor and its two decoders (interactive, general) | Containers in the stack | Observed through jobs/results via the API and through `e2e-ctl inspect` for isolation checks |

**Consumer app purpose**: a standalone test runner that exercises sunnycloud only through the same interfaces a user, a client or an operator has, with no internal imports and no shared memory. Where a test needs a fact only the host can see (blob bytes on disk, `docker inspect` of the decoder, a DB row holding a token hash), the consumer asks the separate `e2e-ctl` service, which performs one named, read-only or fault-injecting action and records it in the report.

**Execution decision (hardware assessment, Step 3-5): BOTH.** Docker Compose runs everything that does not depend on specific hardware or a real OS. A fixed set of tests needs real hardware, a real desktop session or a real OS and is classified `H-*` below; those run on a lab (VMs, a physical reference machine, devices) and are reported as NOT-RUN when the hardware is absent, never as PASS. The owner has not yet confirmed this choice; it is the recommended option because Docker alone cannot exercise AC-9, AC-11, AC-15 (Windows), AC-16, AC-17, AC-26, AC-28, AC-32, AC-33, AC-34, AC-35 and Safari in AC-50/51.

Source scan note: the repository holds documentation only (no `src/` yet), so the code-indicator scan of the hardware assessment is deferred. Indicators found in the documents: NVENC/GPU passthrough (RESTRICT-14, `solution.md` Media pipeline, gates S5/S10), Windows logon task, systemd user unit, LaunchAgent, OS keychain, Cloud Files placeholders (RESTRICT-24/25/26), MediaStore/WorkManager (RESTRICT-15), Slint on Wayland+NVIDIA (AC-16). Re-run the scan when code exists and update the classification table.

## Docker Environment

Compose project `sunnycloud-e2e`, file `tests/compose.test.yml` (to be created in the Decompose/implement step), driven by `tests/run.sh --profile <name>`. All images are pinned by digest (RESTRICT-30); the SUT images come from `SUT_TAG` (CI build of the commit under test, or the release candidate).

### Services

| Service | Image / Build | Purpose | Ports | Profile |
|---------|--------------|---------|-------|---------|
| caddy | `caddy@sha256:<pin>`, the Compose file's own Caddyfile from the release (not a test copy) | Reverse proxy: TLS, HTTP/2 to clients, h2c to the API, `request_buffers` off, `flush_interval -1`, raised body limit (>= 64 GiB) | 443 (host-published only in `lan` profile) | core |
| migrate | SUT API image (`-test` tag), command `migrate` | One-shot schema step, role `sunnycloud_migrator` (`db_migrator_url` mounted here only); the API starts after it completes | none | core |
| api | SUT API image `ghcr.io/zxsanny/sunnycloud-api:${SUT_TAG}-test` (the test image: same build plus the injectable-clock control endpoint on an internal listener that Caddy does not route; the release image has no such endpoint). Performance runs (AC-1, AC-2, AC-40, AC-43, AC-59) use the release image `:${SUT_TAG}`. Always exactly one container: v1.0 keeps upload progress in memory, so replicas are unsupported | API server (one process) | public `:8080` (h2c, Caddy only), worker `:8081` (supervisor only, `/api/v1/worker-jobs/**`), test clock `:9091` (test image only); all on backend-net, none published | core |
| db | `postgres:18@sha256:<pin>` | Metadata, queue, change feed | 5432 (backend-net only; not published) | core |
| worker-supervisor | SUT worker image | Trusted supervisor: no database credentials (it claims, reads, writes and fails jobs through the API on `:8081` only), no blob mount, no master key; `cpuset` and memory from `WORKER_CPUSET`, `WORKER_MEM`; `mem_limit` on the container | none | core |
| decoder-interactive | SUT decoder image | Untrusted decoder for the reserved interactive lane (thumbnails): no credentials, no blob mount, `network_mode: none` (the socket volume is the only channel), read-only rootfs, tmpfs scratch with a numeric size cap, photo limits 2 CPU / 2 GiB / 120 s | none | core |
| decoder-general | SUT decoder image | Untrusted decoder for bulk thumbnails and transcodes: same isolation as above, but scratch is a disk-backed volume on a dedicated mount or XFS project quota (`${SCRATCH_PATH}`, large videos); video limits 4 CPU / 4 GiB / 40 min | none | core |
| mock-idp | `tests/mocks/idp` (own build) | Mock OIDC provider, GitHub-shaped OAuth2 provider, Apple-shaped provider [v1.1]; scripted faults, including the 12 negative callbacks, a slow-provider scenario (response later than the 10 s provider timeout) and verified/unverified email per address | 9000 | core |
| mock-update-server | `tests/mocks/update` (own build, static files + signing keys) | Stands in for the GitHub Releases host that is the update source, fixed at build time in the test build (AC-49): serves Ed25519-signed update manifests (version, per-OS SHA-256, serial, expires) and payloads for valid (active and recovery key), unsigned, unpinned-key, lower-version, expired, replayed-serial and tampered cases | 9100 | desktop |
| wan-shaper | `tests/tools/shaper` (alpine + iproute2) | Router between the consumer and Caddy: `tc netem` delay 10 ms each way (RTT 20 ms) and `tbf`/`rate 1gbit` | none (needs NET_ADMIN) | core (shaping on by default; `SHAPE=off` for non-timing tests) |
| e2e-consumer | `tests/consumer` (Python 3.13, pytest) | Black-box runner and HTTP API client | none | core |
| e2e-consumer-b | same image, second IP and subnet | Second client address for device-approval IP/location tests (AC-57) and multi-device sync | none | core |
| browsers | `mcr.microsoft.com/playwright@sha256:<pin>` (Chromium, Firefox, WebKit) | Web client automation | none | web |
| desktop-linux-a / -b | `tests/desktop/linux` (Ubuntu LTS userland + product daemon .deb) | Two headless Linux devices for sync, conflict, echo, mass-delete, hostile-name and IPC tests | none | desktop |
| android-emulator | Android emulator on a KVM host (userdebug image for `adb root`), Appium server | Functional Android tests | adb, 4723 | android-emu |
| ctl (host process, not a container) | `tests/tools/e2e-ctl` (root-capable through a narrow sudoers rule) | Fault injection and host-side observation (see Fault injectors) | bound to the ctl-net gateway only, bearer token | all |
| load-gen (separate host) | `tests/tools/perf-client` (Rust) + `iperf3` | 1 Gbps load for AC-1, AC-2, AC-40, AC-59 | none | perf |

Not needed: mock SMTP (the system sends no email; sign-in is social only and invites are tokens), Redis (the queue is Postgres), object store.

### Networks

| Network | Services | Purpose |
|---------|----------|---------|
| client-net | e2e-consumer, e2e-consumer-b, browsers, desktop-linux-*, wan-shaper (client side) | Client side of the simulated WAN |
| edge-net | wan-shaper (server side), caddy, mock-idp, mock-update-server | The "internet side"; `sunnycloud.test`, `idp.test`, `update.test` resolve here |
| proxy-net | caddy, api | Caddy reaches the public listener `:8080` only; Caddy is not on `backend-net` |
| backend-net | api, db, migrate, worker-supervisor, mock-idp | Server-side traffic; DB is reachable only here; `:8081`, `:9090` and `:9091` of the API are bound to its backend address |
| (none) | decoder-interactive, decoder-general (`network_mode: none`) | The decoders sit on no network; each one's only peer is the supervisor through the socket volume |
| ctl-net (`internal: true`) | e2e-consumer, e2e-consumer-b, ctl gateway | Control channel; the SUT is not attached |

The consumer has no route to `backend-net` or `proxy-net`. In the `lan` profile Caddy and mock-idp are also reachable on the lab LAN so VMs, phones and the Safari host can use them.

### Volumes

| Volume | Mounted to | Purpose |
|--------|-----------|---------|
| blobs | api:`/var/lib/sunnycloud/blobs` | Blob store on **XFS**: a loop-mounted XFS image on the host (`tests/tools/make-blob-volume.sh`). Two sizes: `blobs-small` 32 GiB (free-space-floor, disk-fill, quota tests) and `blobs-large` 200 GiB sparse (50 GiB tests, 20 GB red-team video, bulk photos). An ext4 variant (`blobs-ext4`) exists only for the XFS-vs-ext4 comparison of AC-5 (gate S14) |
| pgdata | db:`/var/lib/postgresql` | PostgreSQL 18 data; golden snapshot taken after migration and admin bootstrap |
| master-key | Docker secret `master_key` (32 random bytes, generated per run; `master_key_2` for rotation) | Operator-supplied master key; never on the blob volume; not mounted in workers |
| secrets | Docker secrets `setup_token`, `db_api_url` (DB role `sunnycloud`: DML only, api), `db_migrator_url` (DB role `sunnycloud_migrator`: migrate service only), `worker_api_token`, provider client secrets (no worker database secret) | Credentials from secrets, never from env files or git |
| testdata | consumer, desktop-linux-*, browsers: read-only `/data` | Seeded generated data sets (`test-data.md`); not in git |
| e2e-results | consumer: `/results` | CSV report and metrics |
| sync-a, sync-b | desktop-linux-a/-b: `/home/test/Sync` | Sync roots (ext4 or XFS inside the volume; `ctl` can unmount for AC-29) |

### docker-compose structure

```yaml
# Outline only - not runnable code
secrets: { master_key, master_key_2, setup_token, db_api_url, db_migrator_url, worker_api_token }
networks:
  client-net: {}
  edge-net: {}
  backend-net: {}
  proxy-net: {}
  ctl-net: { internal: true }
services:
  wan-shaper:        # NET_ADMIN, ip_forward, tc netem delay 10ms + rate 1gbit on both interfaces
  caddy:             # edge-net, proxy-net; ports 443 in lan profile
  migrate:           # backend-net; one-shot; secret db_migrator_url; api depends_on it (service_completed_successfully)
  api:               # one instance; proxy-net + backend-net; cpu_shares like the operator install; listeners :8080 (h2c from caddy), :8081 (workers), :9091 (test image); blobs volume; secrets master_key, db_api_url, setup_token
  db:                # backend-net; pgdata; cpu_shares like the operator install
  worker-supervisor: # backend-net; cpuset ${WORKER_CPUSET}; mem_limit; secret worker_api_token; talks to api:8081 only; NO database secret, NO blobs, NO master_key
  decoder-interactive: # network_mode none; read_only; tmpfs /scratch with numeric size; cpus, mem_limit, pids_limit; no secrets
  decoder-general:             # network_mode none; read_only; disk-backed /scratch on ${SCRATCH_PATH} (dedicated mount or quota); cpus, mem_limit, pids_limit; no secrets
  mock-idp:          # edge-net + backend-net; alias idp.test
  mock-update-server:# profile desktop; alias update.test
  e2e-consumer:      # client-net + ctl-net; depends_on caddy healthy; mounts testdata:ro, e2e-results
  e2e-consumer-b:    # second client IP; same image
  browsers:          # profile web
  desktop-linux-a:   # profile desktop; sync-a
  desktop-linux-b:   # profile desktop; sync-b
```

Baseline and reset: after `up`, run migrations and bootstrap (`SETUP_TOKEN`, `sunnycloud admin`), then take the golden snapshot (pgdata tar plus `cp --reflink` of the XFS image). `tests/run.sh reset` restores both in under 60 s. Exclusive tests (below) always start from a reset stack.

### TLS and names

- `core`/`web`/`desktop` (container-only) profiles use Caddy's internal CA; its root is exported to `/certs/root.pem` and trusted by the consumer, browsers and Linux desktop containers.
- `lan` profile (VMs, phones, Safari host) needs a publicly trusted certificate, because Safari and iOS hosts need a certificate they trust (the Android app also accepts user-added CAs with a warning, an owner decision at Step 5). Use a wildcard `*.sc-test.<owner-domain>` through ACME DNS-01 (Caddy DNS plugin); the names resolve to the lab host's LAN address through split-horizon DNS. This is an owner item (domain and DNS API token).

## Consumer Application

**Tech stack**: Python 3.13, `pytest` + `pytest-asyncio`, `httpx[http2]`, `blake3`, `hypothesis` (seeded) for generated matrices. Browser tests: Playwright (Python). Android: Appium (UiAutomator2) + `adb`. Desktop VMs: SSH (OpenSSH on Windows too) plus a small harness agent per VM. Performance clients (AC-1, AC-2, AC-40, AC-59) use the Rust `perf-client` so the client is never the bottleneck; it is built from the OpenAPI document, not from SUT source.
**Entry point**: `tests/run.sh --profile <core|web|desktop|android-emu|perf|lan|gpu> [--release 1.0]`; inside the container `pytest -v --tb=short -p e2e_report` (verbose, never quiet).

### Communication with system under test

| Interface | Protocol | Endpoint / Topic | Authentication |
|-----------|----------|-----------------|----------------|
| REST API | HTTPS, HTTP/2, TLS 1.3 | `https://sunnycloud.test/api/v1/...` (spec: OpenAPI, also the source for generated CSRF and IDOR matrices) | Opaque session cookie or bearer from the real sign-in flow against mock-idp; device-grant token for daemons |
| Upload / download | HTTPS, 2 MiB blocks, up to 4 lanes, single-request upload up to 2 MiB, `POST /api/v1/commits:batch` | same | session or device token |
| Change feed | Held long-poll `GET /api/v1/changes?cursor=epoch.seq&wait=30` | same | session or device token |
| Sign-in | OIDC / GitHub OAuth2 against mock-idp (`idp.test`) | browser-less flow driven by the consumer; browser flow in Playwright | provider-issued codes from the mock |
| Share links and content | Anonymous HTTPS GET | `https://sunnycloud.test/s/<token>` (path per final API) | token, optional password |
| Setup / admin | One-time setup token, invite tokens | API and `sunnycloud admin` | `setup_token` secret, then admin session |
| Operator commands | `sunnycloud admin ...`, `admin restore`, `admin rewrap`, `scrub run`, `doctor`, `key-check` | `e2e-ctl exec-operator` | ctl bearer token |
| Host observation and faults | HTTP on ctl-net | `e2e-ctl` actions | ctl bearer token; every call is written to the report |

### What the consumer does NOT have access to

- No direct database connection, no Postgres credentials, no network path to `db`.
- No blob volume mount and no master key. Reading raw blob bytes (AC-43, AC-24 bit flip) is an `e2e-ctl` action on the host, logged.
- No internal module imports, no SUT source, no shared memory or filesystem with SUT containers.
- No ability to start the daemon, settings window or app from SUT-internal entry points; only the packaged binaries and public OS facilities.

## Client drivers

| Client | Driver | Where it runs | Release | What the driver can see |
|--------|--------|---------------|---------|-------------------------|
| HTTP API client | `httpx[http2]` in e2e-consumer; `perf-client` for throughput | container; load host for perf | v1.0 | status, headers, bodies, timing, raw H2 frames for stalled-connection tests |
| Web: Chrome, Firefox | Playwright, Chromium and Firefox builds, CDP / Marionette for heap numbers | `browsers` container | v1.0 | DOM, network, downloads, JS heap (`Performance.getMetrics`, `measureUserAgentSpecificMemory`, Firefox memory report) |
| Web: Safari | `safaridriver` on a real Mac; Playwright WebKit is **not** a substitute (different engine build, different file-save and streaming support) | owner Mac | v1.0 (AC-50, AC-51, AC-7 Safari part) | DOM, downloads; heap via Web Inspector / `vmmap` footprint (weak; documented limit) |
| Android | Appium + UiAutomator2, `adb` (media push, MediaStore scan, `dumpsys meminfo`, `am`, `logcat`) | emulator (API 34+, userdebug, rooted for `kill -9` loops) and physical Pixel, Samsung, Xiaomi on a USB hub or Wi-Fi adb | v1.0 | UI tree, process RSS, server-side arrival time |
| Desktop Linux | Container `desktop-linux-a/-b` for headless sync tests; Ubuntu LTS VM with GNOME on Wayland for tray, autostart, dialogs, editors, suspend | docker; libvirt VM; physical machine for AC-16 | v1.0 | sync folder, server API, `systemctl --user`, D-Bus (`org.freedesktop.portal.FileChooser`), AT-SPI, `/proc` CPU and RSS |
| Desktop Windows | Windows 11 VM (Defender on) with OpenSSH + harness agent; UI Automation (UIA); Cloud Files test provider; physical weak 2019 laptop for AC-16 | libvirt/Hyper-V VM; physical laptop | v1.0 | sync folder (NTFS), Task Scheduler, UIA tree (`#32770` common dialogs), ETW/perf counters |
| Desktop macOS | Real Mac, LaunchAgent, AX API, TCC grant done once by hand | owner Mac | v1.1 | as above |
| iOS | Appium XCUITest on a real iPhone (and simulator for non-background flows) | owner Mac + iPhone | v1.1 | UI, server-side arrival |
| Vault clients (web WASM, mobile) | Playwright (CSP, heap snapshot via CDP after lock), vault-header tooling | `browsers`, emulator | v1.2 | headers, ciphertext, CSP, heap |

## Network shaping

| Profile | Setup | Used by |
|---------|-------|---------|
| `shape-wan` (default in `core`) | `wan-shaper`: `tc qdisc ... netem delay 10ms` on both interfaces (RTT 20 ms), `rate 1gbit`, no loss | AC-3, AC-40, AC-5 and everything with a time bound under Compose |
| `shape-off` | shaper forwards unshaped | Functional tests where timing is irrelevant (faster) |
| `shape-perf` (two hosts) | Real 1 GbE link between `load-gen` host and the SUT host; `netem delay 10ms` on egress of both hosts; no `rate` limit (the NIC is the limit) | AC-1, AC-2, AC-43 throughput, AC-59 upload delay |
| `shape-flaky` | Drop the connection at a byte offset (TCP reset via `tc`/`iptables REJECT --reject-with tcp-reset` armed by ctl), loss 1% and 5% | AC-3 (drop at 50%), AC-7 corruption-in-transit checks |

Calibration step before every AC-2 run: `iperf3` at the same RTT and 4 streams gives the achievable TCP goodput. The AC threshold in `results_report.md` is >= 112 MB/s, which is 90% of the 125 MB/s nominal line rate and about 95% of what 1 GbE TCP goodput usually allows (about 117 MB/s), so the margin is small; record the calibrated goodput in the report next to the result. A veth/loopback run on one host is a functional check only and is reported NOT-RUN for AC-2.

## Reference hardware classes

| ID | Description | Needed for | Release |
|----|-------------|-----------|---------|
| REF-SRV | **8 physical cores, 32 GB RAM, NVMe (XFS), 1 GbE** (as fixed in AC-2), Docker, performance governor; runs the full stack for perf and large-file tests. Exact CPU/NVMe model: owner to state; it is recorded in every perf report. For AC-1, AC-2, AC-40, AC-59 the SUT may use all 8 cores; for AC-10, AC-11, AC-44, AC-45 it is split 4/4 (see REF-CPU4) | AC-1, AC-2, AC-5 (perf), AC-6, AC-40, AC-43 (throughput), AC-59 (delay), AC-4/50/51 (large files) | v1.0 |
| REF-LOAD | Second machine with 1 GbE NIC for `load-gen` and the web/desktop clients under load | same | v1.0 |
| REF-CPU4 | CPU-only transcode worker with **4 physical cores**: `cpuset` of four logical CPUs on distinct physical cores, their SMT siblings idle (offlined), fixed frequency governor `performance`, nothing else running; on REF-SRV this is four of the eight physical cores (`cpuset`), with api, db and caddy pinned to the other four and the SMT siblings of the worker cores idle | AC-10, AC-11, AC-44, AC-45 timing | v1.0 |
| REF-NV | Worker with an NVIDIA GPU that has NVENC, driver + `nvidia-container-toolkit`, GPU visible inside the no-network general decoder container | AC-28, gate S10 | v1.1 |
| REF-WIN-WEAK | The weak 2019 Windows laptop with an HDD-class disk, current Windows 11, Defender on, power-plugged, cold boot per sample | AC-16 | v1.0 |
| REF-LIN-WAYLAND-NV | Physical machine with the current Ubuntu LTS, Wayland GNOME session, NVIDIA proprietary driver, **no** environment-variable workarounds | AC-16, tray-less check of AC-26 | v1.0 |
| WIN-VM | Windows 11 VM (4 vCPU, 8 GB, NTFS, Defender on) with snapshots | Windows desktop functional tests | v1.0 |
| LIN-VM | Ubuntu LTS VM (stock GNOME on Wayland, no AppIndicator extension, systemd user session) with snapshots | Linux desktop session tests | v1.0 |
| MAC | A Mac (Apple silicon) with an iCloud account; Safari; Xcode | Safari, macOS daemon, iOS | v1.0 (Safari), v1.1 |
| DEV-PIXEL, DEV-SAMSUNG, DEV-XIAOMI | Physical Android devices, Wi-Fi, battery optimisation exempted, one of each vendor (models and OS versions: owner to state) | AC-9, AC-52 (RSS), AC-53 | v1.0 |
| DEV-IPHONE | Physical iPhone | AC-9 iOS, AC-52, AC-53 | v1.1 |
| EMU | Android emulator, API 34 or newer (limited-photo-access API) on a KVM host, userdebug image | AC-46, AC-52 kill loop, AC-53, AC-54 functional | v1.0 |

## Fault injectors (`e2e-ctl`)

`e2e-ctl` is a host process (not in the SUT network) with a fixed list of actions, a bearer token, and an audit line per call in the report. It is the only place where root, Docker and filesystem access exist.

| Fault | Mechanism | ACs |
|-------|-----------|-----|
| Process kill | `docker kill -s KILL api` (and `worker-supervisor`, `decoder-*`), then `docker start`; also `SIGSTOP` of an ffmpeg PID to simulate a stall | AC-38, AC-45, AC-3, AC-68, AC-71 |
| Power-loss style crash (optional strengthening) | `dm-flakey` with `drop_writes` on the blob loop device, or hard stop of a VM. `kill -9` alone cannot prove `fdatasync` ordering because the page cache survives a process kill; the AC as written only needs `kill -9` | AC-38 |
| Disk fill / free-space floor | `fallocate` filler file on `blobs-small`; configured floor read from the SUT config | AC-39 |
| Server clock | The server reads time through an injectable clock; the test image exposes a control endpoint (`POST /test/clock`: advance by N seconds) on the internal test listener `:9091` reached only through `e2e-ctl`, which calls it once, on the api. The supervisor reads the same offset from the test-image endpoint `GET /internal/v1/test/clock-offset` on the worker listener `:8081` (no second call, no second offset), so both move together. The endpoint does not exist in release images: a CI check asserts that the release image refuses both calls and does not listen on the test listener. Time comparisons done by SQL `now()` would not move; if a retention/expiry test fails because of that, it is a SUT defect (all product timestamps must come from the injectable clock). Client side: `faketime` for Linux daemons, VM clock offset for VMs (AC-8 client clock ignored) | AC-8 (client clock ignored), AC-12, AC-30, AC-41, AC-68, AC-73, AC-76 |
| DB restore | `pg_dump` at time T0, more activity, stop api, `pg_restore` to T0 state, start api | AC-41, AC-65, AC-73 |
| Unmount / missing root | `umount` or `rm -rf` of the sync volume mount; Windows: dismount VHD or rename the root | AC-29 |
| Network drop | `iptables` reset at a byte offset (see `shape-flaky`) | AC-3, AC-50 |
| Bit flip at rest | Read-modify-write one byte of a blob file via ctl (path found by ctl from the blob id) | AC-24, AC-7 |
| Raw blob read | Return the first N bytes / grep for a marker in a blob file | AC-43 |
| Isolation inspection | `docker inspect`, `nsenter` into each decoder: env, mounts, routes, DNS, capabilities, cgroup limits, scratch contents and process list | AC-25, AC-71 |
| Read-only DB probe | Fixed queries (token hashes only, row counts after account deletion, GC queue, derived and job rows grouped by key, account and identity counts, device token and grant timestamps, count of blobs with `unavailable_at`); no free-form SQL | AC-12, AC-27, AC-30, AC-71, AC-75, AC-76, AC-80 |
| Blob-volume reconciliation | Compare the file ids under `blobs/` with the `blobs` ids (read-only listing, no bytes) | AC-68, AC-71 |
| Internal worker call | One HTTP request from `backend-net` to `api:8081`, or to the public URL, with a chosen `Authorization` header. With the real `worker_api_token` it can claim a job and stream its input into BLAKE3 and return status, headers, byte count and hash. The worker secret is held by ctl only and never reaches the consumer | AC-72, AC-79 |
| Alternate-key start | Start the API image once with `master_key_2` mounted as the master key against the same database and volume; probe `:8080` and `:8081` every 100 ms until the process exits | AC-72 |
| Operator command | Run `sunnycloud admin ...`, `admin restore`, `admin rewrap`, `admin keycheck-init`, `admin epoch-bump`, `admin media-requeue`, `scrub run --full`, `doctor`, `key-check` inside the api container | AC-23, AC-30, AC-43, AC-24 |
| Memory / CPU sampling | `docker stats` and `/proc` for api RSS during 1,000 stalled connections; daemon CPU and RSS | AC-59, AC-15 |
| Proxy byte counters | Caddy access log: request bytes per path (uploaded identical bytes) | AC-31, AC-3, AC-46 |
| Provider faults | mock-idp control API: valid callback, each of the 12 negative callbacks, unverified victim email, slow provider (> 10 s) | AC-19, AC-55, AC-56, provider-timeout checks |
| Database outage | `docker stop db`, wait, `docker start db`; recovery measured from the moment the database accepts connections (<= 60 s) | resilience (gap 7) |
| Interrupted upgrade | `docker kill` of the new api during migration, restart | AC-65 (rolls back, refuses to start until re-run) |

## Hardware-dependency classification

Codes used by every test spec in an `Env:` field. A test with a code other than `C` that runs on a machine without that environment is recorded as SKIP with the message `NOT-RUN(<code>): <reason>`; it is never PASS.

| Code | Meaning | Runs where |
|------|---------|-----------|
| C | Container only; any Linux runner with Docker, 4+ cores, 16 GB, 50 GB free | CI on every PR |
| C-LARGE | Container, but needs `blobs-large` (>= 200 GiB) and hours | self-hosted runner, nightly or release |
| C-REF | Functional part runs as `C`; the numeric performance bound is valid only on REF-SRV/REF-LOAD/REF-CPU4. Off reference hardware the functional assertions run and the timing assertion is `NOT-RUN(HW-REF)` | perf runner |
| EMU | Android emulator (KVM host). Functional only; latency claims need the devices | emulator runner |
| H-ANDROID | Physical Pixel, Samsung, Xiaomi | device lab, release |
| H-LIN | Real Linux desktop session (LIN-VM; REF-LIN-WAYLAND-NV where stated) | desktop lab |
| H-WIN | Real Windows (WIN-VM; REF-WIN-WEAK where stated) | desktop lab |
| H-MAC | Real Mac (Safari, macOS daemon, iCloud) | owner Mac, v1.1 except Safari |
| H-IOS | Real iPhone | v1.1 |
| H-GPU | NVIDIA GPU worker (REF-NV) | v1.1 |
| H-LIVE | Live provider accounts (GitHub, Google OIDC, Apple) and a public name | owner accounts |
| CI | A pipeline job, not a Compose test (supply chain, licence, pinned actions) | CI |
| MAN | Timed manual walk-through by a person on a clean VM | release |
| EXT | External party (vault review) | v1.2 |

### Classification of every acceptance criterion

| AC | Rel | Env | Notes |
|----|-----|-----|-------|
| 1 | 1.0 | C-REF | 500 accounts, 100 sessions, 1 Gbps aggregate, 10 min, `shape-perf`; confirmed mix 70% idle long-poll / 20% small-file edits / 10% large transfers (`sync-mix-v1`, test-data.md); release image |
| 2 | 1.0 | C-REF | 5 GiB dense file, `shape-perf`, encryption on; calibrated goodput recorded |
| 3 | 1.0 | C | drop at 50%; count re-sent bytes from proxy log |
| 4 | 1.0 | C-LARGE | 50 GiB; finish <= 2 s measured after the last block |
| 5 | 1.0 | C-REF | 100 concurrent 100 MiB background uploads (DS-24); clock runs from close on device A to the verified complete file on device B; XFS (and ext4 comparison, gate S14) |
| 6 | 1.0 | C-REF | p95 <= 300 ms over repeated listings |
| 7 | 1.0 | C + H-MAC | Chrome/Firefox in `C`; Safari verify-after-download on the Mac |
| 8 | 1.0 | C | server order, conflict copy, edit beats delete |
| 9 | 1.0 / 1.1 | H-ANDROID (Pixel gate); H-IOS | Samsung and Xiaomi measured and published, not pass/fail |
| 10 | 1.0 | C-REF | reserved lane p95 <= 5 s; bulk 15 min on REF-CPU4 |
| 11 | 1.0 | C-REF on REF-CPU4 | three clips; compliant = H.264 High or lower, long side <= 1920 and short side <= 1080 (orientation independent), <= 10 Mbit/s video, AAC, SDR BT.709; index at the front is served as is, index at the end is a lossless remux, not a re-encode (DS-07, DS-35); off reference hardware only the "never re-encoded" and "playable output" parts run |
| 12 | 1.0 | C | timing delta <= 1 ms needs a quiet host: 10,000 samples per case, medians |
| 13 | 1.2 | C | vault ciphertext checks |
| 14 | 1.2 | C + EMU | 100 MiB limit; camera backup never writes into the vault |
| 15 | 1.0 | C-REF (Linux container) + H-WIN (WIN-VM, Defender on) | host `fs.inotify.max_user_watches` >= 524288 |
| 16 | 1.0 | H-WIN (REF-WIN-WEAK) + H-LIN (REF-LIN-WAYLAND-NV) | physical hardware only; VM results are informational |
| 17 | 1.0 | H-LIN + H-WIN | native dialog detected via portal D-Bus call / UIA `#32770` |
| 18 | 1.0 | C + H-WIN | slow-writer script for the 2 s settle |
| 19 | 1.0 / 1.1 | C (mock-idp) + H-LIVE; H-IOS for the iOS rule | live flows are gate S8 |
| 20 | 1.0 | C | rate limits per code, account, link, IP (needs consumer-b for the IP dimension) |
| 21 | 1.0 | C | live long-poll cut within 5 s |
| 22 | 1.0 | MAN | clean Ubuntu LTS VM snapshot, README only, stopwatch, one provider ready |
| 23 | 1.0 | C | empty database baseline (before bootstrap) |
| 24 | 1.0 | C | bit flip via ctl; throttle checked against concurrent transfer speed |
| 25 | 1.0 | C (+ H-GPU for the GPU no-network case, gate S10, v1.1) | inspection via ctl |
| 26 | 1.0 | H-LIN (stock GNOME, no tray) + H-WIN (logon task) | |
| 27 | 1.0 | C | probe via ctl: 0 rows, 0 blobs |
| 28 | 1.1 | H-GPU | enable, then remove the GPU: CPU fallback |
| 29 | 1.0 | C (Linux container) + H-WIN | missing/unmounted root, 200 deletes |
| 30 | 1.0 | C | needs the test clock (day 6, day 8; test image) |
| 31 | 1.0 | C + H-WIN | |
| 32 | 1.0 | H-WIN | Cloud Files test provider; real OneDrive sample optional; iCloud part is H-MAC v1.1 |
| 33 | 1.0 | H-WIN | exclusive lock; Linux locks are advisory, so the Linux variant is reported "not applicable", not pass |
| 34 | 1.0 | C (vim, KeePassXC CLI, JetBrains replay) + H-LIN (LibreOffice, JetBrains) + H-WIN (Word, others) | |
| 35 | 1.0 | H-LIN + H-WIN | real 30-minute guest suspend (LIN-VM `dompmsuspend`, Windows sleep with scheduled wake); a paused VM is weaker evidence and is labelled so |
| 37 | 1.0 | C (Linux) + H-WIN (reserved names, ADS, junction) | 40 cases per OS from the confirmed server policy (server rejects `/`, NUL, `.`, `..`, control and bidi-control characters; everything else accepted; unrepresentable names escaped and reported); macOS corpus is H-MAC v1.1 |
| 38 | 1.0 | C | kill -9 during 100 concurrent uploads of ~100 multi-block files (DS-34, slices of big-file) |
| 39 | 1.0 | C | `blobs-small` |
| 40 | 1.0 | C-REF | 60 s at 20 ms RTT |
| 41 | 1.0 | C | DB restore; 91-day offline needs the test clock; after restore local-only files upload and server-only files download, deletions only through the mass-delete guard |
| 42 | 1.0 | C | server side; client side of case folding is H-WIN/H-MAC |
| 43 | 1.0 | C + C-REF | disk inspection `C`; throughput part C-REF |
| 44 | 1.0 | C | 100-clip corpus; ratio is independent of CPU |
| 45 | 1.0 | C | SIGSTOP stall, <= 90 s kill, retry on CPU; A/V drift by marker analysis |
| 46 | 1.0 | EMU (+ H-ANDROID confirmation) | 5,000 assets; edited photo is linked when the same MediaStore asset ID has a new content hash |
| 47 | 1.0 | C | Chromium, Firefox, WebKit render check |
| 48 | 1.0 | C (Linux second user) + H-WIN (second OS user and a remote Windows client) | |
| 49 | 1.0 | C (Linux) + H-WIN | test-key builds; update URL fixed at build time |
| 50 | 1.0 | C-LARGE (Chromium, Firefox) + H-MAC (Safari) | |
| 51 | 1.0 | C-LARGE (Chromium, Firefox) + H-MAC (Safari) | needs 50 GiB free on the browser host |
| 52 | 1.0 / 1.1 | EMU (rooted kill loop, 50,000 assets) + H-ANDROID (RSS, latency) ; H-IOS | |
| 53 | 1.0 / 1.1 | EMU | partial photo access on API 34+ |
| 54 | 1.0 | EMU | release build, plain-HTTP server |
| 55 | 1.0 | C | mock returns unverified victim email |
| 56 | 1.0 | C | 12 negative callbacks, all 12 must pass (AC text now says 12) |
| 57 | 1.0 | C | two source IPs; GeoIP data needed |
| 58 | 1.0 | C | matrix generated from OpenAPI; 4 concurrent refreshes |
| 59 | 1.0 | C + C-REF | IDOR matrix `C`; 1,000 stalled H2 connections and the 10% upload delay C-REF |
| 60 | 1.2 | C | |
| 61 | 1.2 | C | 10,000 mutations |
| 62 | 1.2 | C + EXT | CSP, heap scan in Chromium; review is external |
| 63 | 1.0 | CI | audit/deny gates, image scan, signature and SBOM verification, weekly rebuild check |
| 64 | 1.0 | C | old-version header against `/api/v1` |
| 65 | 1.0 | C | restore on a clean host and upgrade from the previous release image |
| 66 | 1.0 | C (desktop-linux containers, patterns from the config file) + H-LIN/H-WIN (entry through the settings window) | gitignore-style patterns; classification added in plan Step 5 |
| 67 | 1.1 | H-MAC | access denied walk-through; `NOT-RUN(HW-DEVICE)` without a Mac; classification added in plan Step 5 |
| 68 | 1.0 | C | group X: `kill -9` of the API after blob write and in a commit loop, T-CLOCK 25 h and 49 h, blob-volume reconciliation |
| 69 | 1.0 | C | group P: 50 concurrent writers on one account, feed read from cursor 0 |
| 70 | 1.0 | C (API clients) + C (`desktop-linux-a/-b`, profile `desktop`) | group P: 200 move-versus-edit rounds, 10 with real daemons |
| 71 | 1.0 | C | group X: supervisor kills at three points, lease set short by config, decoder scratch inspection; timing not asserted |
| 72 | 1.0 | C | group X: 1 GiB (DS-36) through the internal endpoint with the ctl `internal-http` action; wrong-key start with the ctl alternate-key action |
| 73 | 1.0 | C | group X: DS-22 restore, retention through T-CLOCK, `doctor` |
| 74 | 1.0 | C | group P: 1,000 operations over 10 HTTP/2 connections on an account with its own quota |
| 75 | 1.0 | C | group P: 202 trials against mock-idp with fresh identities |
| 76 | 1.0 | C | group X: short `device_code_ttl` and poll interval by config, T-CLOCK |
| 77 | 1.0 | C | group P: rename, move, delete and a 5 s poll of the public link |
| 78 | 1.0 | EMU | foreground app and job-scheduler-triggered background run over 100 new photos; one server file per photo |
| 79 | 1.0 | C | group X: supervisor paused; `internal-http` against `:8081` and the public URL |
| 80 | 1.0 | C | group X: retention 0, short GC claim lease, CLI scrub at a throttled rate beside uploads and GC |

AC-36 does not exist in the criteria file (79 numbered criteria: AC-1 to AC-35 and AC-37 to AC-80).

## Test Execution

**Decision**: both - Docker Compose for `C`, `C-LARGE`, `C-REF`, `EMU` (container/KVM runners) and a hardware lab for `H-*`, `MAN`, `EXT`.

**Docker mode**
- Command: `tests/run.sh --profile core` (PR gate), `--profile web`, `--profile desktop`, `--profile android-emu`, `--profile perf` (REF-SRV + REF-LOAD), `--profile gpu` (v1.1).
- Required: Docker with Compose, `/dev/kvm` for `android-emu`, `xfsprogs`, root for `e2e-ctl` (loop mounts, `tc`, `umount`), `fs.inotify.max_user_watches` >= 524288, SUT images by digest.
- Results: `./e2e-results/report.csv` plus `metrics.json` and `not-run.csv`.

**Local / lab mode** (H-*)
- Prerequisites: the lab inventory above, test CA or the `lan` wildcard certificate trusted by each VM/device, the SUT stack running on REF-SRV or the lab host in the `lan` profile, `SUNNYCLOUD_URL` and `E2E_CTL_URL` exported to the lab runner.
- Start: `tests/run.sh --profile lan` on the SUT host, then `tests/lab/run-lab.sh --target <win-vm|lin-vm|weak-win|lin-nv|android|mac>` on the controller; each target reverts its VM snapshot first.
- Results: the lab runner writes the same CSV rows to the shared results directory; `tests/report-merge` produces one report.

**Which CI runner runs what**: GitHub-hosted runner: `core` with small data (the 14 GiB-class disk rules out `blobs-large`); self-hosted REF-SRV runner: `perf`, `web` with 50 GiB, nightly; lab controller: desktop and device matrix at release candidate; owner Mac: Safari and (v1.1) macOS/iOS.

## Where each tier runs

| Tier | Host | Never on |
|------|------|----------|
| Unit | developer workstation | n/a |
| `C` black-box (Compose) | CI runner (GitHub-hosted or self-hosted) or a developer machine with Docker | n/a (functional only) |
| `C-REF`, `C-LARGE`, perf, resource-limit | REF-SRV + REF-LOAD (self-hosted runner); REF-CPU4 for transcode | the workstation; a result from any other host is `NOT-RUN(HW-REF)`, recorded in `_docs/_process_leftovers/`, not substituted |
| `H-*` desktop, device, GPU, Mac | the lab hosts in Reference hardware classes | CI-hosted runners, VMs standing in for physical AC-16 machines |

Configurable product durations (retention, link expiry, device-code TTL, settle time, watchdog, lease and purge windows) are set to short values through the SUT config in the test stack, and tests assert against those values. The server test clock (injectable clock) is used only where the AC asserts the production default itself (7-day retention and share expiry, 10-minute code TTL, 90-day tombstone horizon); those tests are group `X`.

## CI/CD Integration

**When to run**: `core` on every PR and push to `dev`; `perf`, `web`, `android-emu`, `desktop` nightly; full `lan` lab matrix and `MAN` items at each release candidate.
**Pipeline stage**: after image build and `cargo audit`/`cargo deny`/signing jobs, before the release workflow (Woodpecker fallback uses the same `tests/run.sh`).
**Gate behavior**: any FAIL blocks. For a release, every item in the release's required list must be PASS; NOT-RUN is allowed only for items the owner lists as waived for that release, with a reason. Perf results from non-reference hardware never count as PASS.
**Timeout**: `core` 45 min; `perf` 4 h; `C-LARGE` 8 h; desktop lab 12 h; any step with no new log output for 2 minutes is reported STALLED and aborted (30 s watcher, per the long-running-jobs rule).

## Reporting

**Format**: CSV
**Columns**: Test ID, Test Name, Execution Time (ms), Result (PASS/FAIL/SKIP), Error Message (if FAIL). A documented not-run is `SKIP` with an Error Message starting `NOT-RUN(<code>):` so it cannot be read as a pass; the end-of-run summary prints PASS, FAIL and NOT-RUN counts per code and the exit code is non-zero on any FAIL.
**Output path**: `./e2e-results/report.csv` (+ `metrics.json` with measured values and the reference-hardware ids, `not-run.csv`, `ctl-audit.log`)
**Logs**: never committed; `e2e-results/` is gitignored.

## Isolation and parallelism

- Default group `P` (parallel, up to 8 workers): every test creates its own account(s) through invite tokens and uses only them; unique folder prefixes; no global settings changed.
- Group `X` (exclusive, serial, reset stack first): test-clock advances, kill/restart, DB restore, disk fill, master-key rotation, provider enable/disable, retention config, 1,000 stalled connections, all `C-REF` timing tests, all `H-*` tests that change the machine (suspend, unmount).
- Teardown uses `e2e-ctl` account purge, not the account-deletion API, except in AC-27 tests, so cleanup does not depend on the feature under test; a leak check compares blob-volume bytes before and after each group.

## ACs and RESTRICT items this environment supports

**ACs** (79): AC-1 to AC-35 and AC-37 to AC-80, as classified above. Fully supported by Compose alone (`C`): 3, 8, 20, 21, 23, 24, 27, 30, 38, 39, 41, 42, 44, 45, 47, 55, 56, 57, 58, 64, 65, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 79, 80 and the server parts of 12, 13, 14, 19, 25, 60, 61. Need reference hardware (`C-REF`/`C-LARGE`): 1, 2, 4, 5, 6, 10, 11, 40, 43 (throughput), 50, 51, 59 (delay). Need a lab or devices: 9, 15 (Windows), 16, 17, 26, 28, 29 (Windows), 32, 33, 34, 35, 37 (Windows), 46, 48, 49 (Windows), 52, 53, 54, 67, 78 (emulator), plus Safari parts of 7, 50, 51 and the settings-window part of 66. Not Compose: 22 (MAN), 62 (EXT part), 63 (CI).
**RESTRICT**: RESTRICT-01 to 04 (release tags drive the profile), 07 (PG 18, XFS, case-insensitive names), 08 (encryption, master key as secret), 09 (fdatasync, cursor, resync), 10 (retention through the test clock and ctl probe), 11 (protocol, shaped link), 13 (decoder split, internal network), 14 (NVENC, v1.1), 15 (Android uploader), 16 (Caddy, buffering off), 17 to 21 (mock-idp, live accounts), 24 (sync model, mass-delete guard), 25 to 27 (autostart, GNOME, macOS grants), 31 (backup/restore/upgrade). Only partly or not covered by this environment: RESTRICT-05, 06 (design choices; behaviour covered through AC-16/17), 12 and 30 (CI jobs), 22 and 23 (v1.2 vault), 28 (hosting statement), 29 (licence compliance: static CI check, no runtime test).
