# Identity security suite ST-01-1, 2, 4, 5 and acceptance AT-01-1

**Task**: AZ-3692_identity_security_acceptance_suite
**Name**: Identity security and acceptance tests
**Description**: Prove at the identity port boundary that one account cannot reach another's sessions or devices, that row-level security holds for the runtime role, that no foreign credential opens a principal, that no secret is stored or logged, and that an operator can bootstrap and onboard users
**Complexity**: 3 points
**Dependencies**: AZ-3683_authenticator_scopes_cache, AZ-3685_invite_redemption_account_creation, AZ-3686_session_refresh_rotation, AZ-3687_device_grant_exact_once, AZ-3688_setup_token_invites_operator_ops, AZ-3689_mobile_code_identity_janitor, AZ-3690_account_admin_and_guard, AZ-3691_identity_abuse_limits_audit
**Component**: 01 identity
**Tracker**: AZ-3692
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/authenticator.md` — foreign-credential rule (produced by E-15/c03).
- `_docs/02_document/contracts/01_identity/operator_identity_ops.md` — setup token and invites used by AT-01-1 (produced by E-15/c08).

## Problem

The identity services are built across several tasks. Tenant isolation (R09, AC-59), the foreign-credential rule (AC-79) and the no-secrets rule need one suite that runs against all of them together, plus the first-admin acceptance scenario at the port boundary.

## Outcome

- ST-01-1, ST-01-2, ST-01-4 and ST-01-5 pass against the real services on PostgreSQL 18.
- AT-01-1 passes using only the ports.
- The suite runs in CI with the component tests (env `C`).

## Scope

### Included
- The four security tests and the acceptance test exactly as 01 `tests.md` specifies them.
- Fixtures through the real services where a service exists; migrator-role fixtures only for the RLS probe.

### Excluded
- The generated two-account matrix over every HTTP operation (E-21/c18, NFT-SEC-13).
- System-level scenarios (E-31).

## Acceptance Criteria

**AC-1: No cross-account administration (ST-01-1, AC-59)**
Given accounts A and B with 2 sessions and 2 devices each, and a pending grant of A
When B calls `SessionService::revoke` and `list`, `DeviceGrantService::revoke_device`, `list_devices` and `decide` with A's ids
Then 0 calls succeed (id-bearing calls `NotFound` or no row changed), lists hold only B's rows, A's credentials still authenticate, and no audit event names A as actor

**AC-2: Row-level security for the runtime role (ST-01-2, AC-59)**
Given fixture rows for A and B written by the migrator role
When the API role scoped to B selects from `sessions`, `device_tokens` and `identities`, and tries `SET ROLE sunnycloud_migrator`, `CREATE TABLE` and `ALTER`
Then only B's rows are returned, and every DDL and role switch fails with permission denied

**AC-3: No foreign credential opens a principal (ST-01-4, AC-79)**
Given a worker bearer value, a refresh token, a login code and a device code
When each is passed to `authenticate` as a session and as a device credential
Then all 8 calls return `Unauthenticated` and no row is touched

**AC-4: Only hashes at rest, nothing secret in logs (ST-01-5)**
Given 100 each of session, refresh, device, invite, login-code and setup tokens minted through the services
When every column of the identity tables is dumped with the migrator role and all logs and audit details are captured
Then no plaintext token substring appears anywhere; each token has its type prefix and decodes to at least 32 random bytes; the 100 tokens of each kind are pairwise distinct

**AC-5: First admin, then a member by invite (AT-01-1, AC-19, AC-23, AC-75)**
Given an empty database and mock-idp with three identities
When identity 1 signs in with the token from `ensure_setup_token`, the admin creates a member invite and identity 2 signs in with it, and identity 3 signs in without an invite
Then identity 1 gets an admin account and a session, identity 2 a member account, identity 3 `InvitationRequired`, and there are exactly 2 accounts

## Non-Functional Requirements

**Reliability**
- Each test uses its own database from the migrated template; no shared mutable state.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | Integration-only suite | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | A and B, 2 sessions and devices each | ST-01-1 | 0 cross-account successes; A intact | NFT-SEC-13, NFT-SEC-14 |
| AC-2 | Migrator fixtures | ST-01-2 | Only B rows; DDL and role switch denied | NFT-SEC-46 |
| AC-3 | 4 foreign credentials | ST-01-4 | 8 of 8 `Unauthenticated` | NFT-SEC-50 |
| AC-4 | 100 tokens per kind | ST-01-5 | 0 plaintext matches; prefix; >= 32 bytes; distinct | NFT-SEC-38 |
| AC-5 | Empty DB, 3 identities | AT-01-1 | Admin, member, refusal; 2 accounts | FT-P-64, FT-P-65 |

## Constraints

- Real services and real PostgreSQL 18; only the provider (mock-idp) is a double.

## Risks & Mitigation

**Risk 1: A late identity task changes a table without the suite noticing**
- *Risk*: a new column stores a token in plaintext.
- *Mitigation*: AC-4 dumps every column of every identity table, not a fixed list.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None beyond those on the producing tasks | — | resolved | Low |
