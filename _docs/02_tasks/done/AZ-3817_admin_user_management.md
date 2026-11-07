# AdminService: user list, role and state changes, quota limits

**Task**: AZ-3817_admin_user_management
**Name**: Admin user management
**Description**: Admins and the operator list users with their usage, change a user's role or state, and set a user's quota limit; every change is audited with ids only
**Complexity**: 3 points
**Dependencies**: AZ-3816_admin_crate_skeleton, AZ-3683_authenticator_scopes_cache, AZ-3690_account_admin_and_guard, AZ-3751_quota_admin_lifecycle, AZ-3784_sc_audit
**Component**: 07 admin
**Tracker**: AZ-3817
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/account_admin_and_guard.md` — `list_accounts`, `set_role`, `set_state` (E-15/c10).
- `_docs/02_document/contracts/01_identity/authenticator.md` — `require(principal, Scope::Admin)` (E-15/c03).
- `_docs/02_document/contracts/07a_quota/quota-admin.md` — `views`, `set_limit` (E-11/c03).
- `_docs/02_document/contracts/shared_audit/sc_audit.md` — `admin_action` events (E-07/c01).

## Problem

An admin must see all users with their usage (500 users, AC-1) and change roles, states and limits (AC-23). The accounts belong to 01 and the usage to 07a; 07 must combine them with two calls and no cross-owner SQL, check the caller on every call, and leave an audit trail without personal data.

## Outcome

- `list_users(page)` returns `AdminUserView{account, role, state, last_login, used, limit}` from one `AccountAdmin::list_accounts` call and one `QuotaAdmin::views` call per page.
- `update_user(account, role?, state?)` applies the change in one short transaction; disabling revokes sessions and device tokens (01 does that in the same transaction).
- `set_quota(account, limit?)` passes the limit to 07a; `None` restores the installation default.
- Only `Actor::Admin` with a session principal holding the admin role, or `Actor::Operator`, may call these.
- Every successful change writes one `admin_action` audit row with actor, subject id and action code.

## Scope

### Included
- The three operations, the actor check and the audit rows.
- `NotFound` for an unknown account, with nothing written.

### Excluded
- The last-admin guard inside `update_user` (E-17/c04).
- Health (E-17/c07); HTTP routes (E-21/c12); CLI commands (E-22/c08, E-22/c09).
- Deleting another user's account (see Flagged concerns, G-AS-1).

## Acceptance Criteria

**AC-1: Paged listing with usage (AC-1)**
Given 45 accounts (3 admins, 2 disabled, one with limit 1 GiB, used 300 MiB, reserved 100 MiB) and page size 20
When pages are requested until none is left
Then the pages hold 20, 20 and 5 users, every account appears exactly once, role, state, last login, used and limit equal the seeded values, no token- or password-like field is present, the order is the same on a second run, and each page makes one accounts call and one usage call

**AC-2: Role and state change**
Given admin A and user U with 2 sessions and 1 device token
When A sets U's role to admin, then U's state to disabled
Then each change commits in one transaction, and after disabling all 3 of U's credentials are rejected by the authenticator

**AC-3: Quota limit**
Given user U
When A sets U's limit to 5 GiB and then to none
Then U's quota view shows 5 GiB and then the installation default; an out-of-range limit returns `InvalidLimit` from 07a and changes nothing

**AC-4: Audit with ids only**
Given the four successful changes of AC-2 and AC-3
When the audit log is read
Then it holds 4 `admin_action` rows with actor A, subject U's id and the action code, and none contains U's display name or email (substring scan)

**AC-5: Unknown account**
Given an id that does not exist and a random UUID
When `update_user` and `set_quota` are called with them
Then each returns `NotFound`, no row changes and no `admin_action` row is written

**AC-6: Admin-only**
Given ordinary user U's session principal and admin A's device-token principal
When each calls `list_users`, `update_user` and `set_quota` as `Actor::Admin`
Then each call returns `Forbidden` and nothing changes; the same calls as `Actor::Operator` are accepted

## Non-Functional Requirements

**Performance**
- A page of 20 over 500 accounts: p95 30 ms, p99 60 ms (PT-07-1, run in E-17/c10).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Actor check per principal kind and role | Only admin session or operator passes |
| AC-1 | Merge of account page and usage map | Every account once; missing usage entry handled as 07a reports |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | accounts-45 data set | IT-07-1 listing and paging | 20, 20, 5; values equal; stable order | NFT-PERF-01 |
| AC-2 | Admin A, user U with 3 credentials | IT-07-2 role and state | One transaction each; credentials rejected | NFT-SEC-08 |
| AC-3 | User U | IT-07-2 quota | 5 GiB then default | |
| AC-4 | After AC-2, AC-3 | IT-07-2 audit scan | 4 rows, ids only | |
| AC-5 | Missing id, random UUID | IT-07-2 unknown id; ST-07-1 step 3 | `NotFound`, nothing written | |
| AC-6 | User session, admin device token | ST-07-1 steps 1-2 (port part; full suite in E-17/c09) | `Forbidden`, no change | NFT-SEC-13 |

## Constraints

- No SQL in 07; accounts come from `AccountAdmin`, usage from `QuotaAdmin` (ADR-015).
- Not releasable before E-17/c04 adds the last-admin guard to `update_user`.

## Risks & Mitigation

**Risk 1: Listing joins drift from 01's paging**
- *Risk*: 01's page boundary and 07a's map disagree, so a user is shown twice or missed.
- *Mitigation*: AC-1 checks every account exactly once across pages and two runs.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-AS-1: the web app (16 §5, `ui_mockups/DESIGN.md` §9 #1) lists deleting another user, but 07 exposes only `PATCH {role, state}` for another account. No delete-user operation is built here; the owner adds the operation or drops the menu item. FT-N-46 is unaffected | Product owner | open | Medium |
| `contracts` part 1 declares only `Unexpected` (or no error) for `list_users` and `health`; AC-6 needs `Forbidden` on all four `AdminService` methods, a `contracts` change (E-17/c01 AC-4) | E-17 owner | open | Medium |
| `last_login` and the paging fields come from 01's `AccountInfo` and `list_accounts`, whose fields are still open (contracts part 1 open point 1) | E-15 / E-17 owners | open | Low |

## Contract

This task produces the `list_users`, `update_user` and `set_quota` parts of `_docs/02_document/contracts/07_admin/admin_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
