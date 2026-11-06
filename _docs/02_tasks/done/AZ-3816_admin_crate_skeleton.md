# Admin crate skeleton, settings and constructor wiring

**Task**: AZ-3816_admin_crate_skeleton
**Name**: Admin crate skeleton
**Description**: The admin crate builds against the 07 ports, maps its typed errors to wire codes, binds and validates its settings, and takes every port and list it needs through constructors
**Complexity**: 2 points
**Dependencies**: AZ-3768_contracts_part1, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3765_sc_clock, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3784_sc_audit, AZ-3652_initial_structure
**Component**: 07 admin
**Tracker**: AZ-3816
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c02

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — 07 port signatures (E-00/c03).
- `_docs/02_document/contracts/shared_config/sc_config.md` — key table and validation (E-01/c01).
- `_docs/02_document/contracts/shared_error/sc_error.md` — codes and status table (E-02/c01).

## Problem

Every 07 task needs the crate, the typed errors its ports declare, the six admin settings, and one way for the composition root to hand over the ports and the late-registered lists (the gap pass chose constructor injection, G-07-1). Without this, each task would invent its own wiring.

## Outcome

- The crate builds against `contracts` and the helpers config, error, clock, audit and db only.
- `LastAdmin`, `NotFound`, `Forbidden`, `ConfirmationMismatch` and the passed-through `InvalidLimit` are typed errors with one wire code each where the code exists.
- The settings `account_delete_batch_size` (5000), `account_delete_interval` (5 s), `account_delete_alert_after` (10 min), `account_delete_deadline` (15 min, 1 min to 60 min), `health_check_timeout` and `admin_list_page_size` are bound and validated at start.
- The admin services and the purger take `AccountAdmin`, `QuotaAdmin`, `TreeReader`, the ordered `AccountLifecycleHook` list and the `HealthContributor` list through their constructors; there is no global registry.

## Scope

### Included
- Crate skeleton, error types and their mapping to `sc_error` codes.
- The typed admin settings section with defaults and bounds.
- Constructors that take the ports and lists, keeping the list order exactly as given.

### Excluded
- Port behaviour (E-17/c03 to E-17/c08).
- Building the lists in the composition root (E-22/c03) and scheduling (E-22/c06).

## Acceptance Criteria

**AC-1: Defaults and bounds**
Given no admin keys set
When the settings load
Then batch size is 5000, interval 5 s, alert after 10 min and deadline 15 min; and when `account_delete_deadline` is 30 s or 61 min, start is refused with a message naming the key

**AC-2: One wire code per error**
Given each 07 error
When it is converted for the wire
Then `LastAdmin` is `last_admin` (409), `NotFound` is `not_found` (404) and `Forbidden` is `forbidden` (403), as in the shared status table

**AC-3: Ports and lists through constructors**
Given recording test doubles for the ports and a hook list in the order 06, 02, 03, 08, 05, 07a
When the admin services and the purger are built from them in a test
Then they build without any global or static registry (grep gate), and the purger holds the hooks in the order given

**AC-4: Allowed imports only**
Given the crate graph
When the crate-graph test runs
Then the crate imports `contracts` and the helpers config, error, clock, audit and db, and no component crate

## Non-Functional Requirements

**Compatibility**
- Builds with the workspace toolchain and `--locked`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Settings defaults and deadline bounds | Defaults as listed; 30 s and 61 min refused |
| AC-2 | Error to code conversion | Codes and statuses as listed |
| AC-3 | Constructor with recorded hook order | Order kept; no static registry |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-4 | Workspace | Crate-graph test (AZ-3652) | No component edge | |

## Constraints

- 07 owns no table and no migration (module-layout).
- Settings keys and environment names come from the helper key table; this task binds them, it does not add keys there.

## Risks & Mitigation

**Risk 1: Wiring drift with the composition root**
- *Risk*: 12 builds the lists differently from what 07 expects.
- *Mitigation*: the construction section of the deletion-purger and admin-service contracts is the single reference for E-22/c03.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `health_check_timeout` and `admin_list_page_size` have no default in 07 §7 or the helper key table (tests use 300 ms and 20) | E-17 / E-01 owners | open | Low |
| 07 §1 and §12 say the composition root passes "the cleanup lists" to the purger, but no cleanup list for 07 is defined (the `BlobOwnerCleanup` list goes to 04) | E-17 owner | open | Low |
| `ConfirmationMismatch` and `InvalidLimit` have no code in the shared status table | E-02 / E-11 / E-17 owners | open | Medium |

## Contract

This task produces the construction sections of `_docs/02_document/contracts/07_admin/admin_service.md` and `_docs/02_document/contracts/07_admin/deletion_purger.md`.
Consumers MUST read those files — not this task spec — to discover the interface.
