# Ops endpoints and `ReadinessCheck` list

**Task**: AZ-3832_ops_endpoints_readiness
**Name**: Ops listener and readiness
**Description**: Bind the ops listener (`ops_addr`, default `:9090`) with the shared `sc_observability` router for `/healthz`, `/readyz` and `/metrics`, feed `/readyz` from the `ReadinessCheck` list handed in by 12, and export the HTTP and accept-loop metric families
**Complexity**: 3 points
**Dependencies**: AZ-3801_crate_skeleton_accept_loop, AZ-3786_sc_observability, AZ-3768_contracts_part1
**Component**: 11 http_api
**Tracker**: AZ-3832
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_observability/sc_observability.md` — ops router, `/readyz` body, `Liveness` (E-09/c01).
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `ReadinessCheck` (E-00/c03).
- `_docs/02_document/contracts/11_http_api/http_server.md` — listeners and `ApiServices` (E-21/c03).

## Problem

Compose health checks (`sunnycloud healthcheck`), operators and Prometheus need to know whether the API process is alive and whether it can serve: database reachable, migrations current, master-key check passed, blob volume writable and above the floor, scheduler alive (`observability.md`). An upgrade or restore that leaves a migration pending must show as not ready (AC-22, AC-65). The ops endpoints must never be reachable through Caddy.

## Outcome

- `/healthz` answers 200 whenever the process runs, with no dependency check.
- `/readyz` answers 200 only when every registered check is ok, otherwise 503 with the JSON list naming the failing or degraded items and no secrets.
- `/metrics` exports Prometheus text with `http_requests_total`, `http_request_duration_seconds`, `connections_open`, `connections_rejected_total{reason}` and `body_limit_exceeded_total`, plus every registered collector.
- The ops listener serves only these three paths.

## Scope

### Included
- Binding the ops listener from `HttpConfig`; mounting the shared router; handing it the `ReadinessCheck` list and the metrics registry from `ApiServices`.
- Registration of the HTTP and accept-loop families (their increments live in E-21/c03, c04, c05).
- Readiness turning 503 through `Liveness` when the shutdown signal arrives (used by E-21/c17).

### Excluded
- The checks themselves (04, 12 and others register them); the router and body format (`sc_observability`); `sunnycloud healthcheck` (E-22/c15).
- The Mode R database outage and pending-migration runs (E-21/c19).

## Acceptance Criteria

**AC-1: Readiness aggregates the checks (AC-22, AC-65)**
Given registered checks for database, migrations current, key check, blob volume and scheduler alive
When all are ok, then each in turn fails
Then `/readyz` is 200 with `status: ok`, and for each failure 503 with a list naming that item, its detail free of SQL, paths and keys

**AC-2: Degraded is not ready (G-11-4)**
Given a blob-volume check reporting degraded because of suspect blobs
When `/readyz` is called
Then it answers 503 with `status: degraded`

**AC-3: Liveness is shallow**
Given the database check failing
When `/healthz` is called
Then it answers 200

**AC-4: Metric families present and low cardinality**
Given 1,000 requests to `/api/v1/nodes/{id}` with distinct ids and 10 rejected connections
When `/metrics` is scraped and parsed with a Prometheus text parser
Then it parses, every name carries the `sunnycloud_` prefix once, the five HTTP families are present, the ids form one series per route template, and no label holds an id, token, name or share URL

**AC-5: Ops paths only on the ops listener**
Given the three listeners of one server
When `/healthz`, `/readyz` and `/metrics` are requested on the public and internal listeners, and an API path on the ops listener
Then the ops paths answer 404 on the other listeners and the API path answers 404 on the ops listener

**AC-6: Shutdown turns readiness off**
Given all checks ok
When the shutdown signal is sent to `run`
Then `/readyz` answers 503 before the listeners stop accepting

## Non-Functional Requirements

**Reliability**
- Each check runs with its own timeout (`sc_observability`); a hung check makes `/readyz` 503, never hangs it.

**Security**
- The ops listener is unauthenticated by design and bound to the backend address; it is never routed by Caddy.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Route tables of the three listeners | Ops paths only on ops |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, five checks | Readiness per failing check (IT-11-17, Mode S part) | 200 all ok; 503 naming the item | AC-22, AC-65, FT-P-69, NFT-PERF-30 |
| AC-2 | Mode S, degraded check | Degraded (IT-11-17) | 503 `status: degraded` | AC-73 |
| AC-3 | Mode S | Liveness (IT-11-17) | 200 | |
| AC-4 | Mode S, 1,000 ids | Metrics (IT-11-17) | Five families, one series per template | AC-24, AC-25 |
| AC-5 | Mode S | Listener separation | 404 off the ops listener | NFT-SEC-50 step 5 |
| AC-6 | Mode S | Readiness on shutdown (IT-11-16, first step) | 503 before accept stops | AC-38 |

## Constraints

- 11 adds no second readiness registry or ops router; it uses `sc_observability`'s.

## Risks & Mitigation

**Risk 1: Degraded too strict**
- *Risk*: suspect blobs make the whole API unready and Compose restarts it.
- *Mitigation*: the default is fixed by the Step 5 gap pass (G-11-4); `sc_observability` open point 2 tracks the owner's review.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| E-21 says 11 provides the `ReadinessCheck` registry; `sc_observability` provides `ReadinessRegistry`; this task uses the latter | E-09, E-21 owners | open | Low |
| `/readyz` 503 on any degraded contributor may be too strict for suspect blobs (E-09 risk 4) | E-09, E-10 owners | open | Low |
| `containerization.md` says the ops address key "is still to be added" while helper 01 lists `ops_addr` | deployment owner | open | Low |

## Contract

This task contributes the ops listener and readiness list to the contract at `_docs/02_document/contracts/11_http_api/http_server.md` (producer E-21/c03).
Consumers MUST read that file — not this task spec — to discover the interface.
