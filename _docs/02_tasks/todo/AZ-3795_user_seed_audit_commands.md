# `admin user list|set-quota|delete`, `admin dev-seed`, `admin audit export`

**Task**: AZ-3795_user_seed_audit_commands
**Name**: User, dev-seed and audit commands
**Description**: List users, set a quota and delete an account through 07's services, seed a development install, and export audit events as id-only JSON lines
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3817_admin_user_management, AZ-3819_account_view_deletion_request, AZ-3822_admin_dev_seed, AZ-3784_sc_audit
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3795
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `AdminService`, `AccountDeletionService`, `DevSeed`, `Actor::Operator`.
- `_docs/02_document/contracts/shared_audit/sc_audit.md` — `AuditReader::export` paging.
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — command rows (E-22/c02).

## Problem

The operator manages accounts from the host: who exists and how much they use, a quota change, a deletion through the normal deletion path (AC-27), seed data for development, and an audit export for review. A deletion without confirmation or a seed on production would be destructive; an export that leaks names or paths would break the id-only audit rule.

## Outcome

- `admin user list`: `AdminService::list_users` with `Actor::Operator`, paged until done; prints account, role, state, last login, used and limit; no token.
- `admin user set-quota <account> <size>`: one `AdminService::set_quota` call; an unparsable size exits 3.
- `admin user delete <account> --yes`: one `AccountDeletionService::request` call; without `--yes` on a non-TTY exit 3; `LastAdmin` exits 2; success prints the deletion deadline.
- `admin dev-seed`: one `DevSeed::seed` call; refused when `environment = production` (exit 2).
- `admin audit export --since <time>`: `AuditReader::export` paged until no next page; one JSON line per event to stdout with ids and action codes only.

Narrowed against the plan: the deletion flow and purge to zero rows is AZ-3665 (E-22/p12); this task owns the command side.

## Scope

### Included
- The five commands: parsing, the business call, output, exit mapping.

### Excluded
- Listing, quota, deletion, seeding and audit logic (07, 07a, helper 04), the `admin_action` audit row of each mutation (the owner writes it with `Actor::Operator`).

## Acceptance Criteria

**AC-1: User list (IT-12-3)**
Given a recording `AdminService` with three users over two pages
When `admin user list` runs, in text and in `--json`
Then both pages are read, each user shows role, state, used and limit, no token is printed, and the exit code is 0

**AC-2: Set quota (IT-12-3)**
Given user U
When `admin user set-quota U 1GiB` runs, and `admin user set-quota U lots` runs
Then the first makes one `set_quota` call with 1 GiB and exits 0, the second exits 3 with no call

**AC-3: Delete needs confirmation (AC-27, ST-12-2, IT-12-3)**
Given user U and the last admin A
When `admin user delete U` runs on a non-TTY without `--yes`, then `admin user delete U --yes`, then `admin user delete A --yes`
Then the first exits 3 with no call, the second makes one `request` call, prints the deadline and exits 0, and the third exits 2 with code `last_admin`

**AC-4: Dev seed refused on production (IT-12-17)**
Given `environment = production`, and then `development`
When `admin dev-seed` runs
Then the production run exits 2 reporting the refusal, and the development run exits 0

**AC-5: Audit export (IT-12-17)**
Given a recording `AuditReader` with events before and after T over two pages
When `admin audit export --since T` runs
Then every event after T prints as one JSON line with ids and action codes only, both pages are read, and the exit code is 0

## Non-Functional Requirements

**Performance**
- `admin user list` with 500 users: target 1 s, failure above 3 s (PT-12-1, measured in E-22/c17).

**Security**
- Export lines carry ids only: no name, email, token or path.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-5 | Paging loop with a recording port | Every page read; one operation repeated |
| AC-2 | Size parsing | `1GiB` accepted; garbage exit 3 |
| AC-3 | Guard and `LastAdmin` mapping | Exit 3 no call; exit 2 `last_admin` |
| AC-4 | Owner refusal mapping | Exit 2 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Harness, recording `AdminService` | `user list`, `set-quota` (IT-12-3) | Pages read; one call; exit 0 / 3 | |
| AC-3 | Harness, stdin `/dev/null` | `user delete` without and with `--yes`, last admin (IT-12-3) | Exit 3 no call; exit 0 with deadline; exit 2 | AC-27, ST-12-2 |
| AC-4 | Harness, both environments | `dev-seed` (IT-12-17) | Exit 2; exit 0 | |
| AC-5 | Harness, recording `AuditReader` | `audit export --since T` (IT-12-17) | Id-only JSON lines after T | |

The owner-effect parts (quota visible at `GET /accounts/me/quota`, U purged to 0 rows, one `admin_action` row per mutation, nothing written by a refused seed) are proven by AZ-3665 (E-22/p12), E-17/c03, E-17/c08 and E-31.

## Constraints

- One business operation per command; paged reads repeat the same operation.

## Risks & Mitigation

**Risk 1: Scripted deletion by mistake**
- *Risk*: a piped command deletes an account.
- *Mitigation*: AC-3; ST-12-2 in E-22/c17.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `AccountDeletionService::request(principal, confirmation)` takes a principal, but `admin user delete` has none (operator); the operator form is not defined (contract part 1, open point 5) | E-17 owner | open | Medium |
| `DevSeed`'s production refusal has no variant name in 07 | E-17 owner | open | Low |
| The format of `--since` is not stated (`--at` is RFC 3339 UTC; contract open point 7) | E-22 owner | open | Low |
| 12 §2 names `QuotaAdmin` for `set-quota`, while 12 §3 and 07 name `AdminService::set_quota`; this task calls `AdminService` (one call, audit by 07) | E-22 owner | resolved | Low |
