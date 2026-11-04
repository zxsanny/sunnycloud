# Risk Assessment — sunnycloud architecture — Iteration 01

Plan Step 4. Sources: three independent read-only reviews (server data path, media/operations/security, clients and sync protocol) over the confirmed component list. Mitigations are applied in the docs listed under "Architecture/Component Changes Applied"; open items are marked.

## Risk Scoring

Probability and impact use the template matrix (Low / Medium / High). `H*` marks a critical consequence (cross-tenant data exposure, remote code on every client, silent loss of all data); a Low-probability `H*` risk scores High, not Medium.

| Level | Action |
|-------|--------|
| Low | Accepted, monitored |
| Medium | Mitigation plan before implementation |
| High | Mitigation and contingency, reviewed weekly |
| Critical | Mitigation defined before leaving planning; verified by a named gate |

## Risk Register

| ID | Risk | Category | P | I | Score | Mitigation | Owner | Status |
|----|------|----------|---|---|-------|------------|-------|--------|
| R01 | Remote code execution in ffmpeg/libheif/libvips leads to cross-tenant plaintext | Technical | M | H* | Critical | Hardened runner (non-dumpable, subreaper, kill all descendants after every job, seccomp denies ptrace and process_vm), exit after a hostile outcome, `network_mode: none`, setsid test | 10, 09 | Mitigated |
| R02 | DB and blob volume restored together from one snapshot roll the `.epoch` marker back; devices ahead of the server diverge silently | Technical | M | H | High | `admin epoch-bump` mandatory after any restore; `serve` refuses to start when DB epoch < marker (covers DB-only restore); client `S > seq` check; DB incarnation id evaluated in ADR | 05, 04, 12 | Open |
| R03 | AC-2, AC-10, AC-11 unproven: reference hardware not provisioned; bulk thumbnail throughput was design-limited | Schedule | H | H | Critical | Design changed (thumb+preview in one job and one fetch; interactive decoder may claim any priority; video on its own lane); provision REF-SRV and REF-CPU4 before the first perf gate; spikes S1/S4 first | 08, 09, tests | Open (owner: hardware) |
| R04 | Client-facing HTTP/2 hop is Caddy with default flow-control windows; transfers may stall near 400 Mbit/s | Technical | M | H | High | Run S1 and NFT-PERF-04 through Caddy; fall back to a second connection (13) or direct TLS on the API | 11, deployment | Open (gate S1) |
| R05 | Mobile token lifecycle: 15-minute access token stops background backup, or a lost rotation triggers reuse revocation | Technical | M | H | High | Single-flight refresh, refresh token persisted before use, 30 s server grace, 15-minute-token test | 13, 17, 01 | Mitigated |
| R06 | One API process holds upload progress, notifier, limiter and janitors: single point of failure, no zero-downtime upgrade | Technical | H | M | High | Accepted for v1.0 (decision 12); checkpoints survive restart; notifier behind a port; documented limit | architecture | Accepted |
| R07 | Per-account serialisation: advisory lock plus 500-entry batch, hot `quota` and `account_feeds` rows | Technical | M | M | Medium | Lock only in publish/move; aggregate deltas per batch; gates NFT-PERF-09, S1, S6 | 02, 07a | Open (gate) |
| R08 | Hand-built random-access STREAM nonces: rewriting an unacknowledged block with other bytes could reuse a nonce | Technical | L | H* | High | Known-answer tests; a block is bound to its content hash before ack; external review of blob encryption before v1.0 (owner to confirm cost) | 04 | Open |
| R09 | Tenant isolation rests on RLS plus application scoping | Technical | L | H | Medium | API role is not the owner (migrator split): no DDL, no BYPASSRLS; two-account IDOR matrix (AC-59); grep tests | helper 07 | Mitigated |
| R10 | Worker credential or plaintext exposed on a plaintext internal port or by remote workers | Technical | M | H | High | Workers API-only; remote workers out of v1.0; `:8081` on the internal network only, never routed by Caddy | 08, 09, deployment | Mitigated |
| R11 | Quota drift: clamped releases, negative adjustments, replace and conflict-copy accounting | Technical | M | M | Medium | Alert on `recompute` difference, `--repair`, AC-74 race tests | 07a | Open |
| R12 | Janitors (GC, erase, scrub) share CPU and I/O with requests; backlog | Technical | M | M | Medium | Drain-until-empty within a time budget, lag metrics, sensible default intervals | 04, 07 | Mitigated |
| R13 | One user exhausts the buffer semaphore; HTTP/2 windows times connections use memory | Technical | M | M | Medium | Per-session and per-account in-flight caps; quota and staged-cap pre-checks before writing; NFT-SEC-15 | 03, 11 | Mitigated |
| R14 | Postgres churn (`block_hashes` rewrites, `jobs`, hot rows) | Technical | M | M | Medium | Autovacuum tuning; measure in S1/S6 | data_model | Open (gate) |
| R15 | Complexity: 21 components, about 50 port traits, real dependencies in tests; schedule slip | Schedule | H | H | Critical | Simplifications applied (one account-erase hook, merged tree reader, no lock-order contract, API-only workers, no flutter_rust_bridge); first vertical slice (upload, commit, feed, download) before breadth; release staging v1.0/v1.1/v1.2 | plan | Mitigated; contingency below |
| R16 | Unicode fold version and `%XX` escaping are user-visible and hard to migrate | Technical | L | M | Low | `name_key_version`, shared corpus | helper 06 | Accepted |
| R17 | Signing trust: updater key theft means malware on all desktops; second CI as second signing root; mutable decoder tag | External | L | H* | High | Offline-signed manifest, two pinned keys, expiry and serial, no signing in the fallback CI, release compose pins images by digest (the decoder alone keeps a moving `<semver>` tag so weekly security rebuilds reach operators; it is cosign-verified), 404 assertion on the pushed digest | ci_cd, 14 | Mitigated |
| R18 | Store and distribution policy: Play closed-testing and media-permission declaration, Android developer verification for sideload, App Store plus AGPL exception needs consent of all copyright holders, no GPL crates in the iOS binary | External | M | H | High | Open accounts early; DCO from the first outside contribution; `cargo deny` allowlist for the iOS build; Play pre-review | ci_cd, 17, 18 | Open (owner) |
| R19 | Windows MSI unsigned or signing rejected: SmartScreen warnings, updater friction | External | M | M | Medium | Apply once the LICENSE exists; document the warning meanwhile | ci_cd | Open (owner) |
| R20 | GPL source offer gaps for weekly images; codec patent pools for a published decoder image | External | L | M | Low | Per-build source tarball including weekly; README patent note | containerization | Mitigated |
| R21 | Decoder scratch fills the host disk and takes Postgres down | Technical | M | H | High | Numeric tmpfs size, dedicated scratch mount or quota, worker `mem_limit` | deployment | Mitigated |
| R22 | One master key for all tenants; `rewrap` does not change data keys, so a leaked old key plus an old dump still decrypts those blobs | Technical | L | H* | High | State in operator docs; consider a data-key re-encrypt tool after v1.0 | 04, 12 | Accepted |
| R23 | Vault (v1.2): external review changes the format; unaudited `aead-stream` | Technical | H | M | High | `format_version` kept; schedule slack; review before release | 19, 20 | Open (v1.2) |
| R24 | Remote delete irreversibly removes local files (no user trash) | Technical | M | H | High | Desktop deletes go to the OS trash/recycle bin; mass-delete guard | 14 | Mitigated |
| R25 | Linux fallback token is a 0600 plaintext file (owner accepted); no Secret Service on headless hosts | Technical | M | M | Medium | Warn once; use the keyring when present | 14 | Accepted |
| R26 | Rename or move on file systems without stable IDs, or a lost index, causes a mass-delete hold and full re-upload (no cross-node dedupe) | Technical | M | M | Medium | Directory-level move detection, (size, mtime, hash) fallback; case-sensitivity detection | 14 | Mitigated |
| R27 | 100 MB daemon budget (AC-15) versus in-flight upload buffers, hashing and SQLite | Technical | M | M | Medium | Lower in-flight cap (16 MiB); measure under load in S2 | 13, 14 | Open (gate S2) |
| R28 | inotify watch limit forces rescan-only pairs (AC-5, AC-35 not met there) | Technical | M | M | Medium | Show in status; ship the `max_user_watches` guide; shorter rescan for such pairs | 14 | Open |
| R29 | Staged-file cap (1000 per account, all devices) throttles bulk imports | Technical | M | M | Medium | Client pacing, `max_staged_per_account` in `ServerInfo`, release staged rows on abort | 03, 13 | Mitigated |
| R30 | Firefox/Safari verified download over 2 GiB with bounded memory unproven (spike S12) | Technical | M | M | Medium | Labelled fallback; gate the release on S12 | 16 | Open (gate S12) |
| R31 | UniFFI/NDK/16 KB page-size/JNA version problems on Android | Technical | M | M | Medium | flutter_rust_bridge dropped (removes the `syn` pin clash and a build tool); pin NDK; early device build | 17 | Mitigated |
| R32 | OEM task killers and the Play photo/video permission and background-work policy | External | H | M | High | Device matrix, generation catch-up, early Play pre-review; minSdk 30 | 17 | Open |
| R33 | Backup ordering: GC can unlink retained blobs due between the dump and the blob copy; doctor then reports missing blobs | Operational | M | M | Medium | Rule: retention minus GC interval must exceed the copy time; documented procedure; superuser dump | deployment | Mitigated |
| R34 | Docker does not restart a container on a failed healthcheck; a hung decoder leaves its slot dead | Operational | M | M | Medium | Runner watchdog self-aborts and exits; supervisor reports slot state | 10, 09 | Mitigated |
| R35 | Vault (v1.2): the key-wrap no longer binds the parent folder (so moves, restores and conflict copies by clients that cannot re-wrap keep working); a malicious server can move, hide or roll back vault files | Technical | M | M | Medium | Stated in 19; header generation counter with `If-Match` and pinned last-seen generation; include in the external vault review | 19, 20 | Open (v1.2) |

## Detailed Analysis (Critical and unresolved High)

### R01: Decoder compromise
**Trigger**: a crafted media file exploits a parser bug; the child calls `setsid`, ptrace or reads the next tenant's scratch.
**Mitigation**: see register; a test makes a hostile child try `setsid` and read the next job's input. The runner processes one job at a time and is restarted after any hostile-class outcome.
**Contingency**: switch the decoder to a gVisor runtime (documented as an option, not default); disable a codec by feature flag.
**Residual**: Medium (kernel-level escape is not addressed by seccomp alone).

### R03: Performance acceptance not provable
**Trigger**: no reference machine, or throughput below 11 photos/s sustained.
**Mitigation**: structural changes above; hardware provisioned before the first perf gate; the perf tier reports NOT-RUN rather than pass until then.
**Contingency**: relax AC-10 with the owner after S4 numbers; scale with more workers (same host).
**Residual**: Medium until measured.

### R02: Snapshot restore
**Contingency**: operator procedure forbids restoring DB and volume from one point-in-time without `epoch-bump`; the client-side check catches devices that are ahead of the server.
**Residual**: Medium.

### R04: Caddy HTTP/2 windows
**Contingency**: terminate TLS in the API's hand-built accept loop (large windows already planned for the Caddy-to-API hop) or open a second connection per transfer.

### R06: Single API process
**Contingency**: if downtime during upgrade is unacceptable, move upload progress to Postgres (the `upload_sessions` bitmap is already durable); the notifier port is replaceable.

### R08: Blob encryption construction
**Contingency**: replace the hand-built construction with a reviewed STREAM implementation per 64 KiB chunk if the review demands it (format version bump, rewrap tool).

### R15: Complexity
**Contingency**: if the first vertical slice slips more than 25%, move Linux/Windows desktop agents to v1.0.1 and ship server, web and Android first; the sync-core crate and protocol are shared, so no rework.

### R18: Distribution
**Contingency**: ship Android through the sideload/F-Droid channel while Play review runs; ship desktop MSI unsigned with a documented warning.

### R23: Vault
**Contingency**: v1.2 slips; v1.0 and v1.1 are unaffected (the vault is opt-in, separate crates).

## Defects Fixed in the Evaluator Pass (Step 4a)

About 70 defects (26 server data path, 24 media and operations, 21 clients) were found and fixed in the docs; the significant ones:
- Garbage collection of a deleted file's blob was blocked by a restrictive foreign key; revision rows are now removed with the blob owner rows.
- Restore tried to create a second revision for one blob (blob is unique per revision); restore now re-points the existing revision.
- Account deletion could not finish (sessions, tokens, grants, codes, receipts remained); one erase hook per component, driven by one purger.
- Garbage collection throughput could not meet the 15-minute deletion target; the sweeper drains within a time budget.
- Constructor-injection cycles between quota, tree, transfer, blobstore, identity and feed removed (late-registered lists, caller-supplied recompute values).
- Mobile PKCE binding, operator invites, zero-byte files, commit effects without an error type, per-account in-flight caps, RLS on the job table.
- Backup commands ran as the RLS-bound owner; decoder containment gaps; interactive decoder starvation of bulk photo jobs; video jobs in the RAM-scratch lane; unreachable healthcheck restart; scratch quotas; the proxy network could reach Postgres; CPU overcommit; test-clock assertion on a rebuilt (not pushed) image; unpublished GPL sources for weekly images; environment-variable names that would refuse start.
- Clients: token refresh, feed overwriting unsynced local files, upload checkpoint persistence, missing IPC calls, limited photo access diff, GPS redaction on Android 10+, in-process lock instead of a DB lease, batch commit ids, directory renames and case-insensitive roots, error-code mapping, naming drift.
- Vault (v1.2): per-revision wrapped keys, key-wrap bound without the parent, bounded Argon2 parameters, header generation counter, honest residual risk.

## Architecture/Component Changes Applied

| Risk | Document | Change |
|------|----------|--------|
| R01 | `components/10_decoder`, `09_worker_supervisor`, `tests/security-tests.md` | hardened runner, exit after hostile outcome, containment test |
| R02 | `architecture.md`, `components/05_sync_feed`, `12_operator_cli`, `system-flows.md` F14 | `serve` refusal, doctor Fail |
| R03 | `components/08_media_jobs`, `09_worker_supervisor` | one thumb+preview job, lanes, any-priority interactive claim |
| R05 | `components/13_sync_core`, `17_android_app`, `01_identity` | refresh contract |
| R09, R10 | `canonical-names.md`, `data_model.md` §12, `common-helpers/07_helper_db.md`, `deployment/*` | migrator role, API-only workers |
| R17 | `components/14_desktop_daemon`, `deployment/ci_cd_pipeline.md` | offline manifest signing, pinned keys, digest assertions |
| R21, R33, R34 | `deployment/containerization.md`, `deployment_procedures.md`, `components/10_decoder` | quotas, backup rule, watchdog |
| R24, R26 | `components/14_desktop_daemon` | OS trash, directory moves |
| R31 | `components/17_android_app`, `18_ios_app`, `architecture.md` | UniFFI only |

## Items Not Changed (owner may revisit)

- Trimming the WASM surface of `sync-core` to hashing only (reviewer proposed; simpler but the web and native planners could diverge). Kept as confirmed.
- Cross-node content dedupe (commit by hash): out of scope; reinstall uses `POST /api/v1/revisions:lookup`.
- Vault share links: not in v1.2.
- gVisor for the decoder: documented as a contingency.

## Summary

**Total risks identified**: 35
**Critical**: 3 | **High**: 13 | **Medium**: 17 | **Low**: 2
**Risks mitigated in the docs this iteration**: 16
**Open and needing an owner action or a gate**: R02, R03, R04, R07, R08, R11, R14, R18, R19, R23, R27, R28, R30, R32, R35
**Owner answers applied this iteration**: workers API-only; migrator role split; offline two-key updater; UniFFI only
**Remaining owner choices**: decoder image tag versus digest pinning (weekly rebuilds),  Windows reserved-name marker and collision suffix, poster-frame tolerance, external review of blob encryption before v1.0, Windows signing and store accounts, hardware for performance gates
