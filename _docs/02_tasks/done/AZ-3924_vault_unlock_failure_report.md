# `report_unlock_failure` with the `vault_failure` audit event (ids only)

**Task**: AZ-3924_vault_unlock_failure_report
**Name**: Vault unlock-failure report
**Description**: Implement `VaultService::report_unlock_failure`: a client that failed to unlock a vault reports it, and the server writes one `vault_failure` audit event that carries ids and time only
**Complexity**: 2 points
**Dependencies**: AZ-3923_vault_header_service, AZ-3784_sc_audit
**Component**: 20 vault_service [v1.2]
**Tracker**: AZ-3924
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `VaultService::report_unlock_failure` (E-00/c05).
- `_docs/02_document/contracts/shared_audit/sc_audit.md` — `AuditWriter`, `AuditEventKind::vault_failure`, ids-only `AuditDetail` (E-07/c01).

## Problem

The passphrase check happens on the client, so the server never sees a wrong passphrase. Operators still need to see repeated unlock failures in the audit log (architecture answer 13). The report must never carry the passphrase, a key or attempt text, and a report for another account's vault must not leak that the vault exists.

## Outcome

- Each report for an own vault produces exactly one `vault_failure` audit row and one audit log line, holding account id, vault id and time only.
- A report for an unknown or foreign vault writes nothing and answers like an unknown id.

## Scope

### Included
- `report_unlock_failure(account, vault_id)`: look up the vault in the caller's account; write `vault_failure` with the account as actor and the vault as subject.
- The audit log line that `sc_audit` emits for every recorded event (ids only).

### Excluded
- The route and the handling of a request body (E-30/c06); retry limits on the client (system-flows F17 open question 10).
- Rate limiting of reports (flagged).

## Acceptance Criteria

**AC-1: One event per report, ids only (AC-13, ST-20-2)**
Given a vault V of account A
When A reports 20 unlock failures for V
Then 20 `vault_failure` rows exist, each holding account id A, vault id V and the time only, and the 20 audit log lines hold the same ids and no other value

**AC-2: Unknown or foreign vault**
Given a vault of account B and a random UUID
When A reports a failure for each
Then both answer `NotFound` and 0 audit rows and 0 audit log lines are written

**AC-3: No free text reaches the event**
Given the `report_unlock_failure` input (account and vault id only)
When the event is built
Then its detail holds no string value (the `AuditDetail` type has none), so no passphrase-shaped or key-shaped text can be stored

## Non-Functional Requirements

**Reliability**
- The audit write runs in its own short transaction (`AuditWriter::record`); no transaction spans the vault lookup and an external call.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Event built for (A, V) | Kind `vault_failure`, actor A, subject V, empty detail |
| AC-3 | Event type check | Detail carries no string field |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Real PostgreSQL 18, vault V of A | 20 reports | 20 rows and 20 log lines with ids and time only | ST-20-2, AC-13 |
| AC-2 | Vault of B; random UUID | Report each as A | `NotFound`; 0 rows; 0 log lines | ST-20-2 |

## Constraints

- No passphrase, key, attempt count or attempt text in rows, logs or errors (20 §7; RESTRICT-22).

## Risks & Mitigation

**Risk 1: Report flooding**
- *Risk*: a client in a loop fills the audit log.
- *Mitigation*: flagged for an owner decision on a rate limit (E-08 limiter); the audit retention applies meanwhile.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 20 calls the report "best-effort", but `sc_audit` allows `record_best_effort` only for `share_accessed`, `scrub_mismatch` and `scrub_summary`; this task uses `record` (fail closed). If the owner wants a report that never fails, `sc_audit`'s invariant must list `vault_failure` | E-07 / E-30 owners | open | Low |
| The port lists no error for `report_unlock_failure`, while ST-20-2 expects 404 for an unknown vault id; this task returns `NotFound`, and the `contracts` part 3 row needs it when frozen (E-30/c03) | E-00 contract owner / E-30 | open | Low |
| No rate limit is specified for unlock-failure reports | E-08 / E-30 owners | open | Low |
