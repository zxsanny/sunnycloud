# sc_observability: JSON logging, metrics registry and helper collectors, shared ops router

**Task**: AZ-3786_sc_observability
**Name**: sc_observability logging, metrics, collectors and ops router
**Description**: The one logging setup with a fixed JSON field schema and redaction, a Prometheus registry with the `sunnycloud_` prefix and enumeration-only labels, collectors for the helper-owned families, and the `/healthz`, `/readyz`, `/metrics` router shared by the API and the worker
**Complexity**: 5 points
**Dependencies**: AZ-3763_sc_config, AZ-3764_sc_error, AZ-3765_sc_clock, AZ-3783_sc_db_migrator_accounts_base, AZ-3784_sc_audit, AZ-3785_sc_ratelimit
**Component**: shared/observability
**Tracker**: AZ-3786
**Epic**: AZ-3622 (E-09)
**Ref**: E-09/c01

## Document Dependencies

- `_docs/02_document/contracts/shared_config/sc_config.md` — `log`, `log_level`, `ops_addr`, produced by E-01/c01.
- `_docs/02_document/contracts/shared_error/sc_error.md` — `CorrelationId`, produced by E-02/c01.
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — timestamps, produced by E-03/c01.
- `_docs/02_document/contracts/shared_db/sc_db_migrations.md` — `Db::stats`, produced by E-06/c01b.
- `_docs/02_document/contracts/shared_audit/sc_audit.md` — audit stats and `audit` stream, produced by E-07/c01.
- `_docs/02_document/contracts/shared_ratelimit/sc_ratelimit.md` — limiter stats, produced by E-08/c01.

## Problem

Operators run their own Prometheus or Loki; no monitoring stack is bundled. Logs must carry the request id that matches the error body, never contain secrets, tokens, names or paths, and stay quiet on 2xx. Metrics must stay low cardinality. Compose health checks call `/readyz`, so readiness must mean the same in the API and the worker.

## Outcome

- Every process logs single-line JSON with one field schema; secrets and personal data cannot be logged through the typed field API.
- Every exported metric is `sunnycloud_`-prefixed once and has enumeration-only labels.
- `/readyz` answers 200 only when every registered check is ok and names the failing ones otherwise; `/healthz` checks nothing.
- The helper-owned families (database, audit write failures, rate limits, build info, process) are exported.

## Scope

### Included
- `init_logging` (JSON or text, level, component `api`, `worker`, `decoder-runner`), the field schema, `stream:"audit"` for the audit target, the typed log-field API and the redaction harness.
- The metrics registry: constructors that accept only static label enumerations, one shared histogram bucket set per family, `build_info{version,commit,test_clock}`, the process collector, `register_collector` with `Duplicate`.
- Collectors for `db_up`, `db_pool_connections{state}`, `db_query_duration_seconds{op}`, `migrations_pending`, `auth_rate_limited_total{limit}` and audit write failures, reading the helpers' snapshots.
- The ops router: `/healthz`, `/readyz` (registered `ReadinessCheck`s, a timeout each, JSON body `status` and failing checks, 503 on any not-ok), `/metrics` (Prometheus text); the readiness switch to 503 on the shutdown signal.
- The `HealthContributor` (with `HealthItem`) and `ReadinessCheck` (with `ReadinessItem`) traits, with the methods and items of `contracts` part 1, declared in this crate; `contracts` re-exports them (owner decision 2026-11-05).
- A note in the crate docs on adapting 07's `HealthContributor` to `ReadinessCheck`.
- Choice of the metrics library against precedent, recorded in the crate docs (proposal: `prometheus` with an explicit registry).

### Excluded
- Component metric families (owning epics); HTTP metrics and request-id middleware (E-21); janitor metrics and `doctor` (E-22); the ops listener bind address and `sunnycloud healthcheck` (E-21, E-22); the example alert file (release bundle); tracing backends.

## Acceptance Criteria

**AC-1: No secrets in logs (NFT-SEC-38)**
Given log events carrying a planted access token, refresh token, setup token, invite and share tokens, OAuth `code` and `state`, a master key in hex and base64, a provider secret, a file path and an email through the typed field API
When they are logged at every level
Then 0 of the planted values appear in the output

**AC-2: Line schema**
Given one event with every optional field set
When it is logged with `log = json`
Then the output is one line of JSON with `ts` (UTC RFC 3339), `level`, `target`, `msg`, `request_id`, `component` and the optional fields, and nothing else

**AC-3: Request id correlates**
Given a failing request whose `X-Request-Id` was set by the caller (through E-21)
When its error body and log line are read
Then both carry the same `request_id`

**AC-4: Readiness semantics (AC-22, AC-65)**
Given three registered checks
When one fails, then all pass, then the shutdown signal arrives
Then `/readyz` answers 503 naming the failing check, then 200 with `status: ok`, then 503

**AC-5: Liveness is shallow**
Given the database stopped
When `/healthz` is called
Then it answers 200

**AC-6: Low cardinality**
Given a metric constructor called with a label taken from a free `String`
When the code is compiled or the constructor runs
Then it fails to compile or returns `BadLabel`; and after a 10,000-request run (through E-21) no exported label value is an account, node, file, share or IP value

**AC-7: Prefix and format (AC-24, AC-25 metrics reachable)**
Given a registry with the helper collectors
When `/metrics` is scraped and parsed with a Prometheus text parser
Then it parses, every name starts with `sunnycloud_` exactly once, and the helper families are present

**AC-8: Quiet 2xx**
Given 1,000 successful requests (through E-21)
When the logs are read
Then there are 0 per-request `info` lines

**AC-9: Health and readiness traits without `contracts` (owner decision 2026-11-05)**
Given the crate graph and a recording `ReadinessCheck` and `HealthContributor`
When the crate-graph test runs and the doubles are registered through this crate and through the `contracts` re-export
Then `sc_observability` has no edge to `contracts`, and both paths name the same trait

## Non-Functional Requirements

**Performance**
- A disabled-level log event costs under 5 microseconds; counter increments are lock-free; `/metrics` renders under 50 ms with 5,000 series.

**Security**
- The ops listener is unauthenticated by design and never published on a public interface by default.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Redaction harness with planted values | 0 hits |
| AC-2 | JSON schema of a line | Exact fields |
| AC-4 | `/readyz` with failing, passing and shutdown states | 503 / 200 / 503 |
| AC-5 | `/healthz` with a failing check registered | 200 |
| AC-6 | Free-string label | Compile failure or `BadLabel` |
| AC-7 | Prometheus text parse; prefix once | Parses; prefix correct |
| AC-9 | Crate graph; double registered through both paths | No `contracts` edge; same trait |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Full functional suite (later) | NFT-SEC-38, NFT-SEC-16 | 0 secret or token hits in logs | Security |
| AC-4 | Fresh install (through E-22) | FT-P-69 | Health checks green within the install | — |
| AC-4 | Worker listener (through E-21) | NFT-SEC-50 | Worker routes absent from the public listener; ops routes on backend only | Security |
| AC-7 | After restore (through E-22) | NFT-RES-37 | Doctor and metrics consistent | — |

## Constraints

- `sc_db`, `sc_audit` and `sc_ratelimit` never depend on this crate (no cycle); this crate never imports `contracts`; it declares `HealthContributor` and `ReadinessCheck`, which `contracts` re-exports (owner decision 2026-11-05, module-layout exception 5).
- Log lines never include per-block or per-chunk events.

## Risks & Mitigation

**Risk 1: Task at the 5-point cap (E-09 risk 1)**
- *Risk*: logging, registry, collectors and router overrun.
- *Mitigation*: component families stay out; generic constructors.

**Risk 2: A component logs a name or path by accident**
- *Risk*: free-form log text leaks personal data.
- *Mitigation*: typed field helpers plus the full-suite grep of NFT-SEC-38.

## Runtime Completeness

- Named capability: Prometheus text exposition and JSON structured logging.
- Production code that must exist: a real registry and text encoder, the `tracing` JSON layer.
- Unacceptable substitutes: a hand-built text format without a parser test, logging through `println!`.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The metrics library is not fixed by the architecture; the task decides against precedent and records it. | E-09 owner | open | Low |
| `/readyz` 503 on any degraded contributor may be too strict for 04 suspect blobs (E-09 risk 4, G-11-4). | Project owner | open | Medium |
| `ReadinessCheck` has no method list in any spec (contracts part 1 open point 2). | E-09 / E-21 owners | open | Low |
| Resolved by owner decision 2026-11-05: `HealthContributor` and `ReadinessCheck` live in `sc_observability` and `contracts` re-exports them; `sc_observability` no longer depends on `contracts` (dependency on AZ-3768 dropped). Its `sc_db` edge (database collector) is unchanged | Owner | resolved | High |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_observability/sc_observability.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
