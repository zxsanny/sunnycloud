# sunnycloud-worker skeleton: typed config, ops listener, secret handling, no-database graph check

**Task**: AZ-3868_worker_crate_skeleton
**Name**: Worker crate skeleton
**Description**: The `sunnycloud-worker` binary with its typed config section, the worker bearer held as a secret, the ops listener (`/healthz`, `/readyz`, `/metrics`), the `healthcheck` subcommand, and the checks that the worker has no database, blob or media-library path
**Complexity**: 2 points
**Dependencies**: AZ-3763_sc_config, AZ-3786_sc_observability, AZ-3741_docker_compose_skeletons, AZ-3761_ci_skeleton, AZ-3652_initial_structure
**Component**: 09_worker_supervisor
**Tracker**: AZ-3868
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_config/sc_config.md` — typed sections and `Secret<T>`, produced by E-01/c01.
- `_docs/02_document/contracts/shared_observability/sc_observability.md` — logging, registry and ops router, produced by E-09/c01.

## Problem

The worker is trusted but must hold only its bearer (S4-1, AC-79): no database login, no master key, no blob mount, no media library (ADR-009). Compose health checks and `doctor` rely on its ops port. These properties must hold from the first commit of the crate, so a later task cannot quietly add a database dependency.

## Outcome

- The worker starts from a validated config section or refuses with every problem listed.
- The bearer is a secret that never appears in logs, metrics or debug output.
- `/healthz`, `/readyz` and `/metrics` answer on `worker_ops_listen`; `healthcheck` reports them for Compose.
- CI fails when the worker crate or image gains sqlx, a database key or a media library.

## Scope

### Included
- Typed worker section from helper 01's "Worker (09)" and "Limits" rows (defaults: `worker_api_url` `http://api:8081`, `worker_claim_poll_interval` 500ms, `worker_status_interval` 30s, `decoder_silence_timeout` 20s, `decoder_cancel_grace` 5s, `worker_max_input_photo` 256MiB, `worker_max_input_video` 10GiB, `worker_scratch_general_cap` 24GiB, `worker_shutdown_grace` 30s, `worker_ops_listen` `:9090`, `nvenc` off); `worker_api_token` read as a secret file.
- Logging setup with `component=worker`; the ops listener from E-09's router with the worker's readiness checks registered later by the slot tasks.
- The `healthcheck` subcommand used by the image's health check.
- Checks: crate graph has no sqlx, `sc_db`, `contracts` or component crate; the image has no ffmpeg or vips binary and no database variable; the binary has no sqlx symbols.

### Excluded
- Readiness contents (decoder gate E-20/c05, slot health E-20/c07), metrics families (E-20/c08), the main composition (E-20/c08).

## Acceptance Criteria

**AC-1: Config defaults and refusal**
Given no overrides
When the section loads
Then every value equals the documented default; and given an invalid duration and a missing `worker_api_token` secret, the worker refuses to start listing both problems

**AC-2: Secret never printed (ST-09-1, AC-79)**
Given `worker_api_token` set to a 48-character marker
When the section is debug-printed and a startup log line is written
Then the marker appears nowhere; only a masked value is shown

**AC-3: Ops listener**
Given a started worker with no failing readiness check
When `/healthz`, `/readyz` and `/metrics` are called on `worker_ops_listen`
Then `/healthz` and `/readyz` answer 200 and `/metrics` returns Prometheus text with the `sunnycloud_` prefix; with a registered failing check `/readyz` answers 503 naming it

**AC-4: healthcheck subcommand**
Given a running worker, and a stopped one
When `sunnycloud-worker healthcheck` runs
Then it exits 0 for the running worker and non-zero for the stopped one

**AC-5: No database, blob or media path (IT-09-1 image part, ST-09-2, NFT-SEC-46)**
Given the worker crate and its image
When the crate-graph check and the image inspection run
Then there is no sqlx, `sc_db`, `contracts` or component crate in the graph, no sqlx symbol in the binary, no `ffmpeg` or `vips` binary in the image, no database variable in its environment, and the image is under 50 MB

## Non-Functional Requirements

**Security**
- Only `worker_api_token` is read as a secret (containerization "Worker supervisor").

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Section defaults and validation | Defaults; all problems listed |
| AC-2 | Debug and log output of the section | Masked |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-3, AC-4 | Worker image | Ops endpoints, healthcheck | 200/503; exit codes | |
| AC-5 | Worker image, CI graph job | IT-09-1 image inspection, ST-09-2 | No database, blob or media path | NFT-SEC-29 step 4, NFT-SEC-46 |

## Constraints

- No `contracts`, `sc_db`, `sqlx` or component crate (module-layout 09); `media-shared` and helpers only.

## Risks & Mitigation

**Risk 1: A transitive dependency brings sqlx**
- *Risk*: a helper pulls the database crate into the binary.
- *Mitigation*: AC-5 checks symbols in the binary, not only direct dependencies.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-09's `sc_observability` (logging, registry, ops router) imports `sc_db` (module-layout line "sc_db consumed by sc_observability", database collector). Using it as the cross-cutting rule requires still puts sqlx into the worker, against module-layout 09 and AC-5; a split or feature of `sc_observability` without the `sc_db` edge is needed (the `contracts` edge is resolved, next row) | E-09 and E-00 owners | open | High |
| Resolved by owner decision 2026-11-05: `HealthContributor` and `ReadinessCheck` live in `sc_observability`, `contracts` re-exports them, and `sc_observability` no longer depends on `contracts`, so linking it brings no `contracts` into the worker | Owner | resolved | High |
| `healthcheck` is named in containerization but its exact check (`/healthz` or `/readyz`) is not stated; `/readyz` includes "both decoders reachable" | E-20 owner | open | Low |
