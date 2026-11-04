# ADR-009: Sandboxed media pipeline with API-only workers

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Thumbnails and video transcodes are made by parsers of attacker-controlled files (libheif, libvips, ffmpeg). The research found 2026 CVEs in all three (`01_solution/solution.md` § Assessment Findings, worker sandbox row), so a decoder is assumed compromisable. The worker must still turn a decrypted original into outputs, and the queue must serve a reserved interactive lane (AC-10) and a video lane on one Compose host. The earlier design gave the supervisor a database login and reached Postgres from the sandbox; the owner replaced that at Step 4 (S4-1).

- Acceptance criteria addressed: AC-25 (no credentials, no blob mount, no network, red-team corpus ends in job failure), AC-10 (reserved lane, 5 s p95), AC-45 (stalled ffmpeg killed within 90 s), AC-71 (supervisor kill leaves one derived file per key), AC-72 (1 GiB fetch equals plaintext hash), AC-79 (worker routes reject user sessions and are not public), AC-63 (weekly rebuild), AC-22 (30-minute install)
- Restrictions addressed: RESTRICT-13 (supervisor and decoder split, subprocess tools, Postgres `jobs` with SKIP LOCKED, no Redis), RESTRICT-32 (5 min lease, 3 attempts, derived blobs encrypted), RESTRICT-16 (Compose install)
- Risks addressed: R01 (RCE leads to cross-tenant plaintext), R03 (AC-10 throughput unproven), R21 (scratch fills host disk), R34 (hung decoder is not restarted by Docker), R10 (worker credential on a plaintext port)
- Research source: `_docs/01_solution/solution.md` § Component: Media pipeline (Queue and sandbox row); `_docs/00_research/raw/media_pipeline.md` § T3, § T5

The decision is non-trivial because ffmpeg cannot read a trailing-index MP4 from a pipe (35 MB sample: 0 frames from `pipe:0`, 600 from a file), so the decoder needs a decrypted scratch file, while the master key and the blob volume must stay on the API host.

## Decision

We will split the media worker into a trusted supervisor and an untrusted decoder, and the supervisor will talk only to the API: it has no database login, no blob mount and no master key.

The supervisor (`sunnycloud-worker`, distroless, `mem_limit`) claims, heartbeats, fetches decrypted input, posts outputs and fails jobs over the worker listener `:8081` (`/api/v1/worker-jobs/**`, `backend` network, worker bearer, plaintext, never routed by Caddy). The queue is the Postgres `jobs` table with `FOR UPDATE SKIP LOCKED`; the SQL lives in 08 media_jobs inside the API, and `media-shared` holds only job DTOs, the transcode policy and the decoder protocol. Remote workers on other hosts are out of v1.0. The role design is in ADR-014.

The decoder (`decoder-runner`) runs `vipsthumbnail` and an `ffmpeg` subprocess, probes only, and holds no credentials. It runs with `network_mode: none`, `read_only`, `cap_drop: ALL`, `no-new-privileges`, and is reached only through a Unix socket in a shared volume plus a scratch volume. Each supervisor drives two decoder containers: `decoder-interactive` (tmpfs scratch with a numeric size, photo lane) and `decoder-general` (disk scratch on a dedicated mount or quota, video and photo lanes). The runner is hardened: `PR_SET_DUMPABLE=0`, child subreaper, SIGKILL of every descendant after each job, seccomp denying `ptrace` and `process_vm_*`, a self-abort watchdog, and exit after any hostile-class outcome so Compose restarts it. Format and protocol whitelists and `VIPS_BLOCK_UNTRUSTED=1` apply, with a separate HEIC profile.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| In-process libraries (`libvips` crate, `ffmpeg-next`) in the worker or API | A decoder bug becomes RCE in a process with credentials; fit matrix marks the `libvips` crate Rejected for workers and `raw/media_pipeline.md` § T3 notes `ffmpeg-next` is in maintenance mode. Fails AC-25 by construction |
| WASM sandbox for the codecs | Not evaluated by any research card, so there is no evidence it meets AC-11 (10 min 1080p in 5 min on 4 cores needs native multi-threaded x264); not adopted without a spike (inference, not measured) |
| gVisor runtime for the decoder | Needs an extra runtime on every operator host against AC-22; kept as the documented contingency for R01, switchable without a design change; no gVisor in v1.0 |
| Worker database role (previous design: `sunnycloud_worker`, Unix socket or `internal` network to Postgres) | A compromised or leaked supervisor holds a database login (R10); adds a role, a `db_worker_url` secret and a queue module in `media-shared`. Owner decision S4-1 replaced it |
| External queue (Redis, NATS, pgmq, apalis) | Extra service against AC-22; `raw/media_pipeline.md` § T5 puts Redis at over 100 jobs/s sustained, while the worst case here is 10,000 photos in 15 min, about 11 jobs/s (AC-10); sqlxmq and pgmq need sqlx 0.8 |

## Consequences

### Positive

- A decoder exploit reaches only one job's scratch file: no database, no blob volume, no key, no network, and the next job's input is created after the kill-all (R01 mitigated, tested by a hostile child that tries `setsid`).
- One credential surface (the worker bearer, checked only by 08 `WorkerAuthenticator`); the worker image has no database secret and no media libraries.
- Throughput scales by adding supervisor and decoder pairs on the same host; the API orders claims by priority so interactive photos come first (R03 design change).
- Docker not restarting on a failed healthcheck is covered by the runner's own watchdog and exit-and-restart (R34); scratch has numeric caps (R21).

### Negative

- The decrypted original crosses `:8081` in plaintext on the internal network, and the API decrypts every job's input, adding CPU and one extra copy per job.
- A shared-kernel container is hardening, not a VM boundary; kernel-level escape is a residual Medium (R01).
- Two decoder containers, two socket volumes and two scratch volumes per supervisor raise Compose complexity.
- Exiting after hostile outcomes costs a restart and a repeat of `hello` and `self_check` per event.

### Neutral / Open

- Reference hardware (REF-SRV, REF-CPU4) is not provisioned, so AC-10 and AC-11 report NOT-RUN until gates S4 and S10 run (R03).
- Remote workers need TLS and a transport decision before they can be added after v1.0.
- Decoder image tag versus digest pinning for weekly rebuilds is an open owner choice (R17).

## Evidence

- `_docs/02_document/canonical-names.md` § Owner decisions at Step 4 (S4-1); § Technical defaults applied at Step 4 (decoder isolation)
- `_docs/02_document/architecture.md` § 3 Deployment Model (Runtime units); § 7 Security Architecture (Decoder isolation)
- `_docs/02_document/components/08_media_jobs/description.md` § 4 Internal Interfaces (JobQueue); § 5 External API Specification
- `_docs/02_document/components/09_worker_supervisor/description.md` § 3 Behaviour (job flow); § 7 Security notes
- `_docs/02_document/components/10_decoder/description.md` § 3 Internal Interfaces (Containment); § 4 Execution profiles
- `_docs/02_document/deployment/containerization.md` § Images (Decoder); § Docker networks, volumes and secrets
- `_docs/02_document/system-flows.md` § Flow F10: Media pipeline
- `_docs/02_document/data_model.md` § 9 Media: `jobs` and `derived`
- `_docs/02_document/risk_mitigations.md` § Risk Register (R01, R03, R10, R21, R34); § Detailed Analysis (R01)

## Notes

Revisit gVisor if a decoder escape is found in practice or if S10 shows the hardened runner cannot be tested adequately. Revisit the external queue only above 100 jobs/s or with workflow graphs.
