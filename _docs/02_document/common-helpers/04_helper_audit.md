# Helper: Audit (append-only event writer, IDs only)

**Crate**: `sc_audit` (server only). **Used by**: 01_identity (sign-in, device approve/revoke, session revoke, refresh reuse), 06_sharing (create, revoke, access), 07_admin and 07a_quota (admin actions, account deletion), 04_blobstore (`scrub_mismatch`, `scrub_summary`), 12_operator_cli (`audit export`; admin actions run from the CLI are written by the owning services), 11_http_api (vault failure report, v1.2).

## Purpose
One typed way to write the `audit_log` table (data model 10.1). It makes it impossible to write a name, email or path into the log and makes a failed security write visible instead of swallowed.

## Interface

| Operation | Input | Output | Async | Errors |
|-----------|-------|--------|-------|--------|
| `AuditWriter::record_in` | `&mut DbTx`, `AuditEvent` | `()` | Yes | `AuditError::Rejected` (shape check), `AuditError::Store` |
| `AuditWriter::record` | `AuditEvent` (opens its own short transaction) | `()` | Yes | same |
| `AuditWriter::record_best_effort` | `AuditEvent` | `()` | Yes | none; failures are logged at ERROR and counted in `sc_audit_write_failures_total` |
| `AuditPurge::purge_older_than` | `Timestamp` | `rows_deleted: u64` | Yes | `AuditError::Store` |
| `AuditReader::export` | `since: Timestamp`, optional event filter, page cursor | page of `AuditRow` (ids only) | Yes | `AuditError::Store` |

```
AuditEvent:
  event: enum (sign_in, device_approved, device_revoked, session_revoked, refresh_reuse,
               share_created, share_revoked, share_accessed, admin_action, account_deleted,
               abuse_trip, vault_failure, scrub_mismatch, scrub_summary)
  actor_account_id: Uuid (optional)
  actor_session_id: Uuid (optional)
  subject: SubjectRef { type: enum, id: Uuid } (optional)
  ip: IpAddr (optional)
  detail: AuditDetail — typed struct of Uuid / enum / integer fields only; serialised to jsonb
```

## Rules
- **IDs only**: `AuditDetail` has no free `String` field, so a name, email, path or token cannot be passed. The 2,048-byte check also exists as a database `CHECK`.
- **Append only**: the only code paths that write `audit_log` are `record*` (insert) and `purge_older_than` (retention janitor, retention set by the operator in config). The migrator role owns the table (ADR 014); the API role `sunnycloud` has only `INSERT` and `SELECT` on it, and retention deletes go through a `SECURITY DEFINER` purge function owned by the migrator. A CI test fails if any other statement on `audit_log` appears and a role-privilege test checks the grants (NFT-SEC-47).
- **Fail closed for state changes**: `record_in` runs inside the business transaction, so a failed write rolls the change back (sign-in, device approval, share create/revoke, admin action, account deletion). Only `share_accessed`, `scrub_mismatch` and `scrub_summary` use `record_best_effort` (read path and background job); this is the one explicit best-effort exception and is counted.
- **Scrub summaries** (decision 6) are stored as one `scrub_summary` event per run (counts of verified, mismatched and missing blobs, duration, as integers) plus metrics; there is no scrub table.
- Abuse counters (AC-20) are not audited per attempt; a limiter trip writes one `abuse_trip` event (limiter kind and the link or account id as ids in `detail`, the client address in `ip`, no token).
- `share_accessed` is written once per download start (a request without `Range`, or with a range starting at 0), not per Range request (06).
- Readable only by admin routes; no row-level security (installation data).

## Tests
NFT-SEC-38 (no secrets in logs), NFT-SEC-16 (tokens absent from logs and audit), FT-P-36..38 (share events; one `share_accessed` for a download split into Range requests), NFT-SEC-05/07 (device approval events), FT-P-19 (account deletion event kept), unit test: attempt to compile an event with a name field fails (compile-fail test).

## Risks
- Audit table growth: retention janitor and index `(at)`; the volume is low (security events only).
- `record_in` inside long transactions: callers use it as the last statement before commit.
