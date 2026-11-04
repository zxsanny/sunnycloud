# ADR-002: Single-host Docker Compose with one API process

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

An operator installs sunnycloud on their own hardware for hundreds of users and expects a working server from the README in 30 minutes. The installation has to hold open about 100 concurrent sync sessions, run janitor loops (garbage collection, scrub, account erase), and host media workers that handle untrusted files.

- Acceptance criteria addressed: AC-1 (500 users, 100 concurrent sessions), AC-22 (working server in 30 minutes with the default Caddy), AC-25 (decoder has no credentials, mounts or network), AC-64 and AC-65 (versioned upgrade that keeps data), AC-79 (worker endpoints unreachable through the public proxy, 404), AC-3 (an interrupted upload resumes with at most 10 MiB re-sent)
- Restrictions addressed: RESTRICT-16 (Docker Compose is the install, Caddy default proxy, buffering off), RESTRICT-28 (operator hardware, hundreds of users), RESTRICT-13 (supervisor/decoder split), RESTRICT-32 (interrupted upgrade rolls back and the server refuses to start)
- Risks addressed: R06 (one API process is a single point of failure, no zero-downtime upgrade), R10 (worker credential or plaintext exposed on a plaintext internal port or by remote workers), R04 (Caddy HTTP/2 hop), R21, R34
- Research source: `_docs/01_solution/solution.md` § Component: Sync protocol and server

Upload progress is a received-bitmap that the API advances only after `fdatasync`; it is persisted to `upload_sessions` as a checkpoint every about 8 MiB or 1 s. The feed wake-up, the rate limiter and the janitors are also process-local. Running replicas would force each of these into shared state before there is a measured need. The owner confirmed the single process (component decision 12) and, at Step 4, that workers talk only to the API and that remote workers are out of v1.0 (S4-1).

## Decision

We will ship one installation as one Docker Compose project made of Caddy, one Rust API process, PostgreSQL 18, a blob volume and sandboxed workers, with exactly one API process in v1.0 and no orchestrator.

Units: `caddy` (network `proxy` only; TLS 1.3, buffering off, h2c to `api:8080`), a one-shot `migrate` step (role `sunnycloud_migrator`) that must complete before `api` starts, `api` (`sunnycloud serve`: public `:8080`, worker listener `:8081` for `/api/v1/worker-jobs/**`, ops `:9090`), `db` (PostgreSQL 18, network `backend`), and per supervisor a `worker` plus `decoder-interactive` and `decoder-general` (`network_mode: none`). Networks are `proxy` (caddy, api) and `backend` (api, db, worker); Caddy is not on `backend`.

Inside the one API process: upload progress is held in memory and checkpointed to the database; the feed wake-up is an in-process notifier plus a 10 s safety recheck (no `LISTEN/NOTIFY`); the rate limiter, scheduler and all janitor loops run there, claiming rows with `FOR UPDATE SKIP LOCKED` or leases so no leader exists. Workers reach only the API on `:8081` (claim, heartbeat, input, output, failure); they hold no database login, blob mount or master key. Remote workers on other hosts are out of v1.0; `:8081` is plaintext on the internal network and Caddy never routes it. Scaling is vertical for API and database; transcode throughput scales by adding supervisor and decoder sets on the same host. Upgrades are all-at-once: `docker compose pull && up -d`, with a 120 s graceful stop.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Kubernetes / Helm chart | AC-22 allows 30 minutes for a first install on operator hardware; an orchestrator adds cluster setup and a second install path for hundreds of users. Nothing in AC-1 needs more than one host (100 concurrent sessions, 500 users). |
| Replicated API (two or more instances) | Upload progress, notifier, limiter and janitors are process-local; replicas would need a shared progress store and a cross-process wake-up, and `PgListener` loses messages across reconnects (`mve_server_media.md`, sqlx MVE). The feature buys availability that no AC asks for. |
| nginx or Traefik as the default proxy | nginx defaults to a 1 MB body limit that rejects the 2 MiB blocks of the sync protocol (`06_component_fit_matrix.md`, Round 2 "Reverse proxy"); Caddy works with buffering off out of the box (RESTRICT-16). Other proxies stay possible and are documented as variants. |
| Remote workers on other hosts | The worker listener carries decrypted media and a bearer token over plaintext HTTP (R10); safe remote operation needs TLS and per-host trust on `:8081`, which is new surface for no v1.0 acceptance criterion. |
| Supervisor claims jobs straight from PostgreSQL | Needs a worker database role, a queue module in `media-shared` and a database secret on the worker host. Rejected at Step 4 (S4-1); queue SQL stays in 08 inside the API. |

## Consequences

### Positive

- The install is one `compose.yml`, one `.env` and Docker secrets; no overlay files or cluster (AC-22).
- No cross-process coordination: in-memory progress, in-process wake-up (sync latency target AC-5 is met by the notifier, not by polling) and leaderless janitors keep the code path short.
- The worker and decoder blast radius is small: a compromised decoder sees no database and no network; the supervisor sees only the API (AC-25, AC-79).
- Failure handling is simple: a killed API loses at most the un-checkpointed blocks of open uploads, which are re-requested (AC-3, AC-38).

### Negative

- R06: the single API process is a single point of failure, and each upgrade has a short outage; held long-polls and open uploads reconnect. Accepted for v1.0.
- Janitors share CPU and I/O with requests (R12); `cpu_shares` and time-budgeted drains are the only isolation.
- Without TLS inside `backend`, the worker bearer and decrypted media cross a plaintext hop; this is acceptable only because the network is internal to one host (R10, Mitigated by scope).
- Horizontal API scaling is not available; the ceiling is one host (NFT-PERF-01 measures where it is).

### Neutral / Open

- Contingency from R06: move upload progress to PostgreSQL (the `upload_sessions` bitmap is already durable) and put the notifier behind its port; the port exists, no second implementation is planned.
- CLI commands run in other processes (`docker compose exec api sunnycloud ...`) and learn about changes through the 10 s recheck.
- Whether the Caddy-to-client HTTP/2 window sustains 1 Gbps is spike S1 (R04), not decided here.

## Evidence

- `_docs/02_document/architecture.md` § 3. Deployment Model (runtime units, networks)
- `_docs/02_document/architecture.md` § 5. Integration Points
- `_docs/02_document/component-decisions.md` § Decisions (items 12, 13, 16)
- `_docs/02_document/canonical-names.md` § Owner decisions at Step 4 (2026-11-04) (S4-1, S4-2) and § Runtime
- `_docs/02_document/deployment/containerization.md` § Docker Compose — Operator install (`deploy/compose.yml`, released as one file)
- `_docs/02_document/deployment/deployment_procedures.md` § Deployment strategy
- `_docs/02_document/components/11_http_api/description.md` § 4. Listeners and Accept Loop
- `_docs/02_document/components/09_worker_supervisor/description.md`
- `_docs/02_document/data_model.md` § 12.1 Janitors
- `_docs/02_document/risk_mitigations.md` § Risk Register (R06, R10) and § Detailed Analysis (R06)

## Notes

Re-evaluate the single process when an operator needs an upgrade without an outage or when NFT-PERF-01 shows the one host saturated below 500 users. The first step is the R06 contingency; remote workers need their own ADR with TLS on `:8081`.
