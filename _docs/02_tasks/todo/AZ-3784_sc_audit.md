# sc_audit: typed ids-only audit writer, purge, export, `audit_log` migration and privilege tests

**Task**: AZ-3784_sc_audit
**Name**: sc_audit events, writer, purge, export, audit migration, privilege tests
**Description**: One typed writer for `audit_log` that cannot take a name, email, path or token, fails closed inside the business transaction, counts its one best-effort exception, and an append-only table the API role can only insert into and read
**Complexity**: 5 points
**Dependencies**: AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3763_sc_config, AZ-3765_sc_clock
**Component**: shared/audit (helper 04)
**Tracker**: AZ-3784
**Epic**: AZ-3642 (E-07)
**Ref**: E-07/c01

## Document Dependencies

- `_docs/02_document/contracts/shared_db/sc_db_access.md` — `Db`, `DbTx`, produced by E-06/c01a.
- `_docs/02_document/contracts/shared_db/sc_db_migrations.md` — migrator and migration rules, produced by E-06/c01b.
- `_docs/02_document/contracts/shared_config/sc_config.md` — `audit_retention`, produced by E-01/c01.
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `Timestamp`, produced by E-03/c01.

## Problem

Sign-ins, device approvals and revocations, share events, admin actions and account deletion must leave a trustworthy trail (AC-21, AC-30). A compromised API process must not be able to update, delete or re-grant audit rows (ADR-014, NFT-SEC-47). A failed audit write for a state change must roll the change back; the read-path exceptions must be explicit and counted.

## Outcome

- Writers can record only ids, enumerations and integers; a free-text field does not compile.
- State-changing events fail closed; `share_accessed`, `scrub_mismatch` and `scrub_summary` are best effort and every failure is logged and counted.
- As `sunnycloud`, `UPDATE` and `DELETE` on `audit_log` fail; retention runs only through the migrator-owned purge function.
- Every recorded event is mirrored as one JSON log line with `"stream":"audit"`.

## Scope

### Included
- `AuditEvent` with the 14 event kinds, `SubjectRef`, `AuditDetail` (Uuid, enum and integer values only), the 2,048-byte limit checked before insert.
- `AuditWriter::record_in` (inside the caller's transaction), `record` (own short transaction), `record_best_effort` (never fails; logs at ERROR and counts); `AuditPurge::purge_older_than`; `AuditReader::export` (paged, ids only); a stats snapshot with the write-failure count for E-09.
- The audit migration pair: table per data_model §10.1 with named checks (event values, detail size), indexes `(at)` and `(actor_account_id, at)`, `INSERT` and `SELECT` for `sunnycloud`, the `SECURITY DEFINER` purge function owned by the migrator with a fixed `search_path`, `EXECUTE` for `sunnycloud`; reversible.
- The mirror log line with `"stream":"audit"`.
- A CI test that fails on any statement against `audit_log` other than the writer's insert, the export select and the purge call.
- Integration tests on real PostgreSQL 18; a compile-fail test for a free-text field.

### Excluded
- Which events each component writes and when (E-15, E-16, E-17, E-10, E-22); the admin audit screen and `admin audit export` command (E-17, E-21, E-22); the retention janitor wrapper (E-22); row-level security (installation data).

## Acceptance Criteria

**AC-1: Append only (NFT-SEC-47)**
Given the migrated schema and a connection as `sunnycloud`
When it runs `INSERT`, `SELECT`, `UPDATE`, `DELETE` and `GRANT` on `audit_log`
Then `INSERT` and `SELECT` succeed and `UPDATE`, `DELETE` and `GRANT` fail

**AC-2: Ids only (NFT-SEC-38, NFT-SEC-16)**
Given an event type with a `String` field in a compile-fail test, and a writer call with a detail of 2,049 bytes
When the test compiles and the call runs
Then the first does not compile and the second returns `Rejected` without an insert

**AC-3: Fail closed (AC-21)**
Given a business transaction that changes a row and records an event with `record_in`, with the insert forced to fail
When the transaction finishes
Then the caller receives `AuditError::Store` and the business change is rolled back

**AC-4: Best effort is visible**
Given `record_best_effort` with the database insert forced to fail
When it is called
Then it returns normally, one ERROR log line names the event kind, and the write-failure count increases by 1

**AC-5: Retention (AC-30)**
Given rows older and newer than the cutoff from `audit_retention`, and separately `audit_retention = 0`
When `purge_older_than` runs as `sunnycloud`
Then only the older rows are deleted and the count is returned; with `0` the purge is not run and all rows stay

**AC-6: Mirror line**
Given 3 recorded events
When the log output is read
Then there are exactly 3 lines with `"stream":"audit"`, each with ids only

**AC-7: Export is bounded and ids-only**
Given 2,500 rows and an event filter
When `export` pages through them
Then every page is bounded, the pages together return each matching row once, and no row contains text other than ids, codes and integers

**AC-8: Migration is reversible**
Given an empty database with the base migration applied
When the audit migration is applied, reverted and applied again
Then each step succeeds

## Non-Functional Requirements

**Performance**
- `record_in` adds one statement to the business transaction.

**Security**
- The purge function has a fixed `search_path` and deletes only by cutoff (E-07 risk 4).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Compile-fail test for a free-text field; size check | Does not compile; `Rejected` |
| AC-6 | Mirror line format | One line per event, `"stream":"audit"` |
| — | CI statement check on a seeded `UPDATE audit_log` in code | Fails |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | PG 18, API role | NFT-SEC-47 | Update and delete fail | Security |
| AC-3 | Forced insert failure (integration); later device approval through E-15 | Fail-closed rollback; NFT-SEC-05, NFT-SEC-07 events | Change rolled back; events present on success | — |
| AC-5 | Rows around the cutoff | Purge as `sunnycloud` (integration) | Only old rows deleted | — |
| AC-2 | Full functional suite (later) | NFT-SEC-38, NFT-SEC-16 | 0 names, emails, tokens in rows and logs | Security |
| AC-6 | Share download in Range requests (through E-16) | FT-P-36..38 | One `share_accessed` per download start | — |
| AC-3 | Account deletion (through E-17) | FT-P-19 | `account_deleted` event kept | — |

## Constraints

- ADR-014 (two roles; `audit_log` INSERT and SELECT for the API role); no foreign keys from `audit_log`, so account deletion keeps history.
- Callers use `record_in` as the last statement before commit (review rule).

## Risks & Mitigation

**Risk 1: `SECURITY DEFINER` abuse**
- *Risk*: the purge function becomes a way to delete arbitrary rows.
- *Mitigation*: owned by the migrator, fixed `search_path`, cutoff-only delete, privilege test in CI.

## Runtime Completeness

- Named capability: the append-only `audit_log` table on PostgreSQL 18 with the real API role.
- Production code that must exist: the migration with grants and the purge function; the typed writer.
- Unacceptable substitutes: testing grants as a superuser, a purge done by a plain `DELETE` from the API role.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Per-event `detail` fields are not specified. Proposal: `AuditDetail` is a list of named typed values (static key with a Uuid, enum code or integer), so writer epics add fields without editing this crate and free text still cannot enter. | E-07 owner | open | Medium |
| `SubjectRef.type` values are not listed. | E-07 owner | open | Low |
| E-07 risk 1 (`abuse_trip` missing from data_model §10.1) is already resolved: data_model §10.1 lists it. | E-07 owner | resolved | Low |
| The audit pair is the second migration, after `accounts_base` (E-06/c01b). | E-07 owner | resolved | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_audit/sc_audit.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
