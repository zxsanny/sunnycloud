# Worker authentication and the WorkerJobs facade

**Task**: AZ-3843_worker_auth_jobs_facade
**Name**: Worker authentication and job facade
**Description**: Authenticate the supervisor by its bearer secret with one uniform refusal, expose every worker operation behind one port that only the job's current holder can use, and record worker status on the API clock
**Complexity**: 3 points
**Dependencies**: AZ-3839_media_tables_config, AZ-3840_job_queue, AZ-3842_derived_store_accept_input, AZ-3763_sc_config, AZ-3765_sc_clock
**Component**: 08 media_jobs
**Tracker**: AZ-3843
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — operations, uniform 403 and 409 semantics (E-18/c02).
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `WorkerAuthenticator`, `WorkerJobs` signatures (E-00/c05).

## Problem

The worker listener must accept only the supervisor's secret and nothing a user holds (AC-79, R12), must not reveal why a credential failed, and must let a worker act only on jobs it currently holds (AC-25). Worker authentication belongs to 08; 01 has no worker credential.

## Outcome

- `WorkerAuthenticator::authenticate(bearer)` compares the SHA-256 digest of the presented secret in constant time with the loaded digests of `worker_api_token` and, during rotation, `worker_api_token_next`; only digests stay in memory; every failure is the same `Forbidden`.
- `WorkerJobs` (`claim`, `extend_lease`, `release`, `complete_skipped`, `fail`, `open_input`, `accept_output`, `record_status`) delegates to the queue and the derived store; a caller that does not hold the job gets `LeaseLost`, never `Forbidden`.
- `record_status(worker_id, WorkerStatus)` upserts one `worker_status` row with `seen_at` from the API clock.
- Auth failures increment a counter and log a WARN with no secret.

## Scope

### Included
- The authenticator, the facade and worker status.

### Excluded
- The internal listener and route binding (E-21/c15), auth extraction on the routes (E-21/c07), the public 404 (E-21, AZ-3660 AC-79).
- Doctor levels from `worker_status` (E-18/c11).

## Acceptance Criteria

AC-1..AC-4 are local to this task. AC-79 keeps its number from `acceptance_criteria.md` (component part kept here by `interaction-risks.md`; the listener and public-route parts are AZ-3660).

**AC-1: Rotation**
Given `worker_api_token_next` configured beside the current token
When either secret is presented, and then the server restarts with the old token removed
Then both authenticate before the restart, and afterwards the old one is refused with the same `Forbidden`

**AC-2: Only the holder acts (AC-25)**
Given worker W1 holding job J and an authenticated worker W2 with its own worker id
When W2 calls every job-scoped operation on J
Then each returns `LeaseLost`, W2 receives no input byte and J's state is unchanged

**AC-3: Worker status on the API clock**
Given a supervisor reporting `decoder_ok = true, image_age_days = 3` with its own clock 1 hour off
When `record_status` is called twice
Then one row exists for that worker id, updated by the second call, with `seen_at` equal to the API clock

**AC-4: Nothing beyond the job fields**
Given a claimed job
When the claim result is inspected
Then it carries only job id, kind, content hash, params version, attempt and `lease_ms`; no account id, path or name

### Interaction criteria

**AC-79: Uniform refusal of every non-worker credential (R12)**
Given no header, an empty bearer, a wrong secret, a valid user session token, a valid device token and a session cookie value
When each is presented to `authenticate`
Then every one returns the same single `Forbidden` value, a valid secret returns the worker identity, the failure counter rises by 6, and no log line contains a presented secret

## Non-Functional Requirements

**Reliability**
- Coverage at least 90% on worker authentication (critical path).

**Security**
- Secrets are never logged and are held only as digests after load.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-79 | Secrets differing in the first and in the last byte | Same refusal; only digests compared |
| AC-1 | Digest set with and without the next token | Both or one accepted |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-79 | Six credential kinds | IT-08-16 port part | One `Forbidden`; counter; no secret in logs | NFT-SEC-50 |
| AC-1 | Next token configured, then removed | IT-08-16 rotation | Both, then new only | |
| AC-2 | W1 holds J; W2 authenticated | IT-08-6; ST-08-2 port part | `LeaseLost`; no data | NFT-SEC-29 |
| AC-3 | Skewed client clock | IT-08-20 status part | One row; API `seen_at` | |
| AC-4 | Claimed job | ST-08-2 | Job fields only | |

## Constraints

- One token serves all supervisors; per-worker revocation is not planned (08 §9).

## Risks & Mitigation

**Risk 1: Worker credential exposed on a plaintext port (R10)**
- *Risk*: `:8081` is plaintext on the internal network.
- *Mitigation*: internal network only, never routed by Caddy, uniform refusal, digest-only secrets.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| A worker presenting the real holder's worker id is accepted by design (one shared secret, no per-worker auth; ST-08-2) | E-18 owner | accepted-risk | Low |
