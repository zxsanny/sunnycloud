# Contract: sc_observability — JSON logs, metrics registry, helper collectors, ops router

**Component**: shared/observability (crate `sc_observability`; declares `HealthContributor` and `ReadinessCheck`, which `contracts` re-exports; never imports `contracts`, owner decision 2026-11-05, module-layout exception 5)
**Producer task**: E-09/c01 — `_docs/02_tasks/todo/B01-20_sc_observability.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-00/c03 (`contracts` re-exports the two traits); other batches: 11 (E-21), 12 (E-22), 09 (E-20, its own ops listener), every component that registers a metric family
**Version**: 1.2.0
**Status**: draft
**Last Updated**: 2026-11-06

## Purpose

The one logging setup (fixed JSON field schema, redaction), one Prometheus registry with the `sunnycloud_` prefix and enumeration-only labels, collectors for the helper-owned families, and the ops router `/healthz`, `/readyz`, `/metrics` shared by the API and the worker.

Sources: `epics/E-09_cross-cutting-observability.md`, `deployment/observability.md`, `components/11_http_api/description.md` §10, `components/09_worker_supervisor/description.md` §8.

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `init_logging` | `fn init_logging(cfg: LogConfig) -> Result<LogGuard, ObsError>`; `LogGuard::stats() -> LogStats{dropped_lines, rejected_fields}`; `subscriber(cfg, clock, sink)` is the same subscriber with an injected clock and `LineSink` (tests, `-test` images) | `AlreadyInstalled` | sync |
| `log_error!` / `log_warn!` / `log_info!` / `log_debug!` | `log_info!("literal msg", request_id = CorrelationId, account_id = Uuid, node_id = Uuid, route = &'static str, status = u16, duration_ms = u64, err = &dyn Error)`; `request_span(CorrelationId)` carries the id to the events inside it | none (a field outside the schema does not compile) | sync |
| `Metrics::new` | `fn new(build: BuildInfo{version, commit, test_clock}) -> Result<Metrics, ObsError>`; registers `build_info` and the process collector | `Registry` | sync |
| `Metrics::counter` / `gauge` / `histogram` | `fn counter<L: LabelSet>(&self, name: &'static str) -> Result<Counter<L>, ObsError>` (name without prefix; `L: LabelSet` is a static label enumeration, `NoLabels` for none; histograms share `HISTOGRAM_BUCKETS`) | `BadLabel` at construction, `BadName`, `Duplicate` | sync |
| `Metrics::register_collector` | `fn register_collector(&self, c: Box<dyn Collector>) -> Result<(), ObsError>` (`Collector` is `prometheus::core::Collector`); `register_probe(Arc<dyn ScrapeProbe>)` for state that needs an async call; `render() -> Result<String, ObsError>` runs the probes then encodes | `Duplicate` | sync / async |
| `ops_router` | `fn ops_router(metrics: Arc<Metrics>, readiness: ReadinessRegistry, liveness: Liveness) -> axum::Router` (`/healthz`, `/readyz`, `/metrics`) | none | |
| `HealthContributor` | `fn name(&self) -> &str`; `fn check(&self) -> CheckFuture<'_, HealthItem>` (`CheckFuture` is a boxed `Send` future, so the trait stays object safe; implementors write `Box::pin(async move { .. })`) | none (each check has a timeout in the caller) | |
| `ReadinessCheck` | `fn name(&self) -> &str`; `fn check(&self) -> CheckFuture<'_, ReadinessItem>` | none | |
| `ReadinessRegistry::add` | `fn add(&mut self, check: Arc<dyn ReadinessCheck>)`; `with_timeout(Duration)` (default 2 s per check, checks run concurrently; a timeout is a `Failing` item) | none | sync |
| `Liveness::shut_down` | turns `/readyz` to 503 on the shutdown signal (checks are then not run); `Liveness` is a clonable shared flag; `/healthz` ignores it | none | sync |
| helper collectors | `db_collector(Db) -> DbCollector` (`.register(&Metrics)` registers the collector and the `db_up` probe), `audit_collector(impl Fn() -> AuditStats)`, `ratelimit_collector(impl Fn() -> LimiterStats)`: sources are closures over `AuditWriter::stats` / `AbuseLimiter::stats` | `Duplicate` | |

| Type | Shape |
|---|---|
| `HealthItem` | `status: Ok \| Degraded \| Down`, `detail` |
| `ReadinessItem` | `status: Ok \| Degraded \| Failing`, `detail` (the item of the `/readyz` body; no secrets) |
| `LogConfig` | `format: Json \| Text` (`log`), `level` (`log_level`, a `tracing::Level`), `component: Api \| Worker \| DecoderRunner` |
| Log line | one JSON object per line, in this field order: `ts` (UTC RFC 3339, microseconds), `level`, `target`, `msg`, `request_id` (`null` outside a request), `component`, optional `account_id`, `node_id`, `route`, `status`, `duration_ms`, `err`; lines of target `audit` carry `"stream":"audit"` and the audit fields (`event`, `actor_account_id`, `actor_session_id`, `subject_type`, `subject_id`, `ip`, `detail`). Any other field of a raw event is dropped and counted |
| `/readyz` body | `{"status":"ok\|degraded","checks":[{"name","status","detail"}]}`; 200 only when every check is ok, else 503; `detail` never holds a secret |
| Families owned here | `build_info{version,commit,test_clock}`, process collector (`process_resident_memory_bytes`, `process_open_fds`, CPU seconds), `db_up`, `db_pool_connections{state=in_use\|idle\|max}`, `db_query_duration_seconds{op}` (a summary: count and sum, because `Db::stats` records no distribution), `migrations_pending` (absent until a read has seen the count), `auth_rate_limited_total{limit}` (limited plus tripped, every limit exported), `audit_write_failures_total` (exported name; the audit docs call it `sc_audit_write_failures_total`) |

## Invariants

- Every exported name starts with `sunnycloud_` exactly once; labels are enumerations only, never an account, node, file, share or IP value (an unbounded label type fails to compile or panics at construction).
- Never logged: secrets, tokens (even hashed), master key material, provider tokens, file names and paths, emails, share URLs, the setup token (except the single labelled first-start line).
- `request_id` in a log line equals the `X-Request-Id` header and the error body field.
- `/healthz` checks no dependency; `/readyz` runs every registered check with a timeout each.
- `sc_db`, `sc_audit` and `sc_ratelimit` never depend on this crate; it reads their snapshots (no cycle).
- This crate never imports `contracts`. `HealthContributor`, `ReadinessCheck`, `HealthItem` and `ReadinessItem` are declared here, object safe and `Send + Sync`; `contracts` re-exports them unchanged (owner decision 2026-11-05).

## Non-Goals

- Not covered: component metric families (owning epics), HTTP-layer metrics and request-id middleware (E-21), janitor metrics and `doctor` (E-22), the ops listener address binding and `sunnycloud healthcheck` (E-21, E-22), tracing backends.

## Versioning Rules

- Renaming a metric or label is breaking for operators' dashboards; adding a family or a label value is minor.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| line-schema | one log event with every optional field | single-line JSON with exactly the schema fields | |
| redaction | planted token, secret, path, email in event fields | none appears in output | NFT-SEC-38 |
| readyz | one failing check / all ok / after shutdown signal | 503 naming the check / 200 / 503 | AC-22, AC-65 |
| healthz | database stopped | 200 | E-09 AC 4 |
| prefix | `/metrics` output through a Prometheus text parser | parses; every name `sunnycloud_`-prefixed once | AC-24, AC-25 |
| label-guard | label from a free `String` | compile failure or panic at construction | |

## Open points (flagged in the producer task)

1. Resolved: `prometheus` 0.14 with an explicit registry, `default-features = false` (no protobuf), `process` feature; recorded in the crate docs.
2. `Liveness` is a shared atomic flag and `LogGuard` holds the counters and flushes stdout on drop; `/readyz` 503 on any degraded contributor may be too strict for 04 suspect blobs (E-09 risk 4, open with the owner).
3. The worker's ops listener key is `worker_ops_listen`; the API's is `ops_addr` (both in helper 01).

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
| 1.1.0 | 2026-11-05 | `HealthContributor`, `ReadinessCheck` and their items declared here (moved from `contracts` part 1, which re-exports them); no `contracts` import (owner decision 2026-11-05) | decompose Step 4 |
| 1.2.0 | 2026-11-06 | Implemented shape recorded: typed log macros and field schema order, `Metrics::new(BuildInfo)`, `LabelSet`, probes, boxed-future check traits, closure sources for the audit and rate limit collectors | AZ-3786 |
