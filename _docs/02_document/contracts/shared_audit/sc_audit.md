# Contract: sc_audit — append-only, ids-only security events

**Component**: shared/audit (crate `sc_audit`, server only; migration pair `audit` under `src/migrations/`)
**Producer task**: E-07/c01 — `_docs/02_tasks/todo/B01-18_sc_audit.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-09/c01 (stats snapshot, `audit` log stream); other batches: 01 (E-15), 04 (E-10), 06 (E-16), 07 and 07a (E-17, E-11), 11 (E-21), 12 (E-22), 20 (E-30)
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

One typed writer for `audit_log` that cannot take a name, email, path or token, fails closed for state changes, and makes the one best-effort exception visible. The table is append-only for the API role.

Sources: `common-helpers/04_helper_audit.md`, `epics/E-07_cross-cutting-audit-log.md`, `data_model.md` §10.1, ADR-014.

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `AuditWriter::record_in` | `async fn record_in(&self, tx: &mut DbTx, event: AuditEvent) -> Result<(), AuditError>` (last statement before the caller's commit) | `Rejected` (shape check), `Store` | async |
| `AuditWriter::record` | `async fn record(&self, event: AuditEvent) -> Result<(), AuditError>` (own short transaction) | same | async |
| `AuditWriter::record_best_effort` | `async fn record_best_effort(&self, event: AuditEvent)` (never returns an error; a failure is logged at ERROR and counted) | none | async |
| `AuditPurge::purge_older_than` | `async fn purge_older_than(&self, cutoff: Timestamp) -> Result<u64, AuditError>` (through the `SECURITY DEFINER` function) | `Store` | async |
| `AuditReader::export` | `async fn export(&self, since: Timestamp, event: Option<AuditEventKind>, cursor: Option<PageCursor>) -> Result<AuditPage{rows: Vec<AuditRow>, next}, AuditError>` | `Store` | async |
| `AuditWriter::stats` | `fn stats(&self) -> AuditStats{write_failures}` (source of `sc_audit_write_failures_total`) | none | sync |

| Type | Shape |
|---|---|
| `AuditEvent` | `event: AuditEventKind, actor_account_id: Option<Uuid>, actor_session_id: Option<Uuid>, subject: Option<SubjectRef{type, id: Uuid}>, ip: Option<IpAddr>, detail: AuditDetail` |
| `AuditEventKind` | `sign_in`, `device_approved`, `device_revoked`, `session_revoked`, `refresh_reuse`, `share_created`, `share_revoked`, `share_accessed`, `admin_action`, `account_deleted`, `abuse_trip`, `vault_failure`, `scrub_mismatch`, `scrub_summary` |
| `AuditDetail` | typed values of `Uuid`, enumeration and integer kinds only; no `String` field; serialised to `jsonb`, at most 2,048 bytes |
| `AuditRow` | the stored row: `id, at, event, actor_account_id?, actor_session_id?, subject_type?, subject_id?, ip?, detail?` |

### Schema contract (migration pair with domain word `audit`)

- Table `audit_log` as data_model §10.1: `id bigint identity PK`, `at timestamptz`, `actor_account_id uuid NULL`, `actor_session_id uuid NULL` (no foreign keys), `event text` with a named check of the 14 kinds above, `subject_type text NULL`, `subject_id uuid NULL`, `ip inet NULL`, `detail jsonb NULL` with `CHECK (octet_length(detail::text) <= 2048)`; indexes `(at)` and `(actor_account_id, at)`.
- Owned by `sunnycloud_migrator`; `sunnycloud` has `INSERT` and `SELECT` only.
- One `SECURITY DEFINER` purge function owned by the migrator, fixed `search_path`, deleting only rows older than the cutoff; `EXECUTE` granted to `sunnycloud`.
- No row-level security (installation data, admin routes only).

## Invariants

- Only `share_accessed`, `scrub_mismatch` and `scrub_summary` use `record_best_effort`; every other event is written with `record_in` (fail closed: a failed write rolls the business change back) or `record`.
- Every recorded event also produces one JSON log line with `"stream":"audit"`, ids only.
- A limiter trip writes one `abuse_trip` event, never one per attempt; `share_accessed` is written once per download start.
- Retention comes from `audit_retention` (default 365 d; `0` keeps everything).

## Non-Goals

- Not covered: which events each component writes and when (E-15, E-16, E-17, E-10, E-22); the admin list screen and `admin audit export` command; the retention janitor wrapper (12 wraps the purge call in a `Janitor`, module-layout open point 6).

## Versioning Rules

- A new event kind is a minor change and needs a new migration that swaps the named check (additive). Removing a kind is breaking.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| append-only | `UPDATE` and `DELETE` on `audit_log` as `sunnycloud` | both fail; `INSERT`, `SELECT` succeed | NFT-SEC-47 |
| ids-only | an event type with a `name: String` field | does not compile (compile-fail test) | |
| fail-closed | forced insert failure inside a business transaction | `AuditError`, business change rolled back | AC-21 |
| best-effort | forced failure of `record_best_effort` | ERROR log line and the counter + 1 | |
| retention | rows on both sides of the cutoff; `audit_retention = 0` | only older rows deleted; with 0 nothing | AC-30 |
| detail-cap | detail of 2,049 bytes | rejected (`Rejected` before insert, check in the database) | |

## Open points (flagged in the producer task)

1. Per-event `detail` fields are not specified. Proposal: `AuditDetail` is a list of named typed values (static key, `Uuid` / enum code / integer), so writer epics add fields without editing this crate and free text still cannot enter.
2. `SubjectRef.type` values are not listed (data_model has `subject_type text`).
3. data_model §10.1 already lists `abuse_trip` (E-07 risk 1 is resolved in the data model); the check carries all 14 kinds.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
