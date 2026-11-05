# Admin security suite and acceptance checks

**Task**: AZ-3823_admin_security_acceptance
**Name**: Admin security and acceptance
**Description**: Prove at the 07 ports that non-admins and device tokens cannot use admin operations, that audit and logs carry no personal data, that a deleted account leaves nothing behind after a crash mid-purge, and that no route or command removes the last admin
**Complexity**: 3 points
**Dependencies**: AZ-3817_admin_user_management, AZ-3818_last_admin_guard, AZ-3819_account_view_deletion_request, AZ-3820_deletion_purger, AZ-3821_admin_health_aggregation
**Component**: 07 admin
**Tracker**: AZ-3823
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/07_admin/admin_service.md` — operations under test (E-17/c03, c04, c07).
- `_docs/02_document/contracts/07_admin/account_services.md` — deletion request (E-17/c05).
- `_docs/02_document/contracts/07_admin/deletion_purger.md` — purge end state (E-17/c06).

## Problem

The admin layer guards privilege, personal data and destructive actions. Each behaviour is built in its own task; this task runs the security cases ST-07-1 and ST-07-4 and the component acceptance checks AT-07-1 and AT-07-2 against the real components behind the ports.

## Outcome

- 0 successful admin calls by non-admins or device tokens; unknown ids are indistinguishable.
- 0 occurrences of a seeded name or email in audit details, JSON logs or metrics.
- After a deletion with a purger restart mid-cycle: 0 rows, 0 blob files and 0 shares for the account; its share link answers as missing.
- The last active admin survives every admin, account and operator path.

## Scope

### Included
- ST-07-1, ST-07-4; AT-07-1 and AT-07-2 at the port level with the real components.

### Excluded
- ST-07-2 (E-17/c05) and ST-07-3 (E-17/c04).
- The HTTP status, the CLI exit code and the container kill of AT-07-1/AT-07-2: run by the account-deletion pipeline AZ-3665 (AC-1 to AC-4) and the system scenarios FT-N-46, FT-P-19 and NFT-RES-30.

## Acceptance Criteria

**AC-1: Non-admin and foreign calls (AC-59)**
Given ordinary user U, admin A's device token, and ids that do not exist or are random
When U (as `Actor::Admin(U)`) and the device token call `list_users`, `update_user`, `set_quota` and `health`, and A calls `update_user` and `set_quota` with the unknown ids
Then the first two give `Forbidden`, the third gives `NotFound` with no difference between missing and random ids, no role, state or limit changes, and no `admin_action` row is written for a refused call other than an optional refusal entry with ids only

**AC-2: No personal data in audit and logs**
Given accounts named `Zed Unique-7Q` with email `zed.unique7q@example.test`
When the IT-07-2 changes and an IT-07-9 deletion run
Then the audit details, the JSON logs and the metrics output contain neither string

**AC-3: Deleting leaves nothing behind (AC-27, AC-30, AC-21)**
Given an account with DS-23 files, a share link and 2 sessions
When its deletion is requested, its old session and device token are tried, the purger instance is dropped mid-cycle and a new one is started, and cycles run until done
Then the request returns a deadline, both credentials are rejected within 5 s, and within the test deadline the account has 0 rows, 0 blob files and 0 shares, and its share link is reported missing by the sharing port

**AC-4: The last admin cannot be removed (AC-23)**
Given exactly one active admin A and one degraded health contributor
When A is demoted and disabled through `update_user`, A requests deletion of A's account, the operator demotes and deletes A, and `health` is read
Then every removal returns `LastAdmin` with A unchanged, and the health report shows the degraded item

## Non-Functional Requirements

**Reliability**
- AC-3 target: purge done within 60 s with test settings (15 min limit, AC-27).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Audit detail builder | Ids and action codes only |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | User U, admin device token, unknown ids | ST-07-1 | `Forbidden`; `NotFound`; no change | NFT-SEC-13 |
| AC-2 | Unique name and email | ST-07-4 | 0 matches | |
| AC-3 | DS-23 files, share, sessions | AT-07-1 (port form) | Credentials rejected in 5 s; 0 rows, files, shares | FT-P-19, NFT-RES-30, NFT-SEC-27 |
| AC-4 | One active admin | AT-07-2 (port form) | All refused; degraded shown | FT-N-46 |

## Constraints

- Real 01, 02, 03, 04, 05, 06 and 07a behind the ports; the only doubles are named test health contributors.

## Risks & Mitigation

**Risk 1: Port-level checks miss a route-level gap**
- *Risk*: the HTTP layer maps a refusal wrongly.
- *Mitigation*: FT-N-46 and AZ-3665 check the wire forms; E-21/c18 runs the matrix.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AT-07-1 and AT-07-2 are written as compose-stack tests in 07 tests.md; their stack steps (container kill, 409, CLI exit 2) run in AZ-3665 and FT-N-46, and this task runs their port form. The owner may want the stack form here instead | E-17 owner | open | Low |
