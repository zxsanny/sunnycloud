# Identity operator commands: `admin setup-token|create|reset|invite|promote|demote`

**Task**: AZ-3794_identity_operator_commands
**Name**: Identity operator commands
**Description**: Create or reset admins and invites without the web: one call on 01's operator ports or 07's `AdminService`, tokens printed once, refusals mapped to exit 2
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3681_identity_hooks_admin_reset_spike, AZ-3688_setup_token_invites_operator_ops, AZ-3817_admin_user_management, AZ-3818_last_admin_guard
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3794
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/operator_identity_ops.md` — `OperatorIdentityOps`, `InviteService` (E-15/c08).
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `AdminService::update_user`, `Actor::Operator`.
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — command rows and exit codes (E-22/c02).

## Problem

An operator must be able to create the first admin, recover a lost setup token, reset an admin and manage invites from the server host (AC-23). Tokens are shown once and only their hashes are stored, so the command is the only place the operator ever sees them. A refusal (setup already done, last admin) must be a clear exit code for scripts.

## Outcome

- `admin setup-token`: one `ensure_setup_token` call; a token prints once with a sign-in hint and exit 0; when the owner returns nothing (an account exists) the command prints `SetupNotAvailable`, nothing on stdout, exit 2.
- `admin create [--note] [--ttl]`: one `create_admin_invite` call; the invite token prints once with the sign-in hint; exit 0.
- `admin reset <account>`: one `reset_admin` call; the fresh invite prints once; `NotFound` exits 2; the sole-admin outcome follows the G-12-3 answer recorded by E-15/c01.
- `admin promote|demote <account>`: one `AdminService::update_user` call with the role and `Actor::Operator`; `LastAdmin` exits 2 with code `last_admin`.
- `admin invite create|list|revoke`: one `InviteService` call each; the token prints once on `create` and never on `list`.

Narrowed against the plan: the full first-run flow (setup token to first admin) is AZ-3654 (E-22/p01) and invite redemption through sign-in is AZ-3664 (E-22/p02); the last-admin rule and its concurrency are 07's (E-17/c04). This task owns the command side.

## Scope

### Included
- The commands above: argument parsing, the one business call, output through the secret writer, exit mapping.

### Excluded
- Token generation, hashing, validity and redemption (01), the last-admin rule (07), printing the setup token at `serve` start (E-22/p01).

## Acceptance Criteria

**AC-1: Setup token while no account exists (FT-P-78, IT-12-2)**
Given zero accounts
When `admin setup-token` runs twice
Then each run makes one `ensure_setup_token` call, prints one token once on stdout with the sign-in hint and exits 0

**AC-2: Setup not available (FT-P-78 step 4, IT-12-2)**
Given an owner answer of "no token" because an account exists
When `admin setup-token` runs, in text and in `--json`
Then it reports `SetupNotAvailable`, exits 2, and prints no token

**AC-3: Admin invite once (FT-P-65, IT-12-2)**
Given an install with one admin
When `admin create --note x --ttl 1h` runs
Then one `create_admin_invite` call is made, the token appears once on stdout with the sign-in hint, and the exit code is 0

**AC-4: Reset (FT-P-65 step 1, IT-12-2)**
Given an admin account, an unknown account id, and the sole admin
When `admin reset` runs for each
Then the first prints a fresh invite once and exits 0, the unknown id exits 2 with `NotFound`, and the sole admin gets the outcome E-15/c01 records for G-12-3 (exit 2 when it is a refusal)

**AC-5: Promote and demote (FT-P-65 step 6, FT-N-46, IT-12-3)**
Given a user U and a last admin A
When `admin promote U` and `admin demote A` run
Then each makes one `update_user` call with `Actor::Operator`, the promote exits 0, and the demote exits 2 with `--json` code `last_admin`

**AC-6: Invites (IT-12-3)**
Given an admin
When `admin invite create`, `admin invite list` and `admin invite revoke <id>` run
Then `create` prints the token once, `list` prints ids, notes and expiry with no token, `revoke` exits 0, and each makes exactly one `InviteService` call

## Non-Functional Requirements

**Security**
- A token never appears in stderr, logs, `--json` log lines or audit rows (ST-12-1, NFT-SEC-38).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1..AC-6 | Each command with a recording port | Exactly one call with the parsed arguments |
| AC-2 | "No token" owner answer | `SetupNotAvailable`, exit 2, empty stdout token |
| AC-4, AC-5 | `NotFound`, `LastAdmin` | Exit 2 with the owner code |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Harness, recording `OperatorIdentityOps` | `setup-token` with and without accounts (IT-12-2) | Token once, exit 0; then exit 2, no token | FT-P-78 |
| AC-3 | Harness | `admin create --note x --ttl 1h` (IT-12-2) | Token once on stdout; exit 0 | ST-12-1 |
| AC-4 | Harness | `reset` admin, unknown, sole admin (IT-12-2) | Invite once; exit 2 `NotFound`; G-12-3 outcome | FT-P-65 |
| AC-5 | Harness, recording `AdminService` | `promote`, `demote` last admin (IT-12-3) | Exit 0; exit 2 `last_admin` | FT-N-46 |
| AC-6 | Harness, recording `InviteService` | `invite create`, `list`, `revoke` (IT-12-3) | Token once on create, never on list | |

The owner-effect parts of IT-12-2 and IT-12-3 (old token refused after a new setup token, only the SHA-256 stored, sessions revoked after reset, concurrent demotes giving exit codes {0, 2}) are proven by AZ-3654 (E-22/p01) AC-2 and AC-4, E-15/c08, E-17/c04 and FT-N-46 in E-31.

## Constraints

- One business call per command; the token is printed only through the secret writer.

## Risks & Mitigation

**Risk 1: Reset locks the installation out**
- *Risk*: resetting the only admin leaves nobody signed in until the re-bind invite is used.
- *Mitigation*: the command prints the fresh invite once and the sign-in hint; the rule itself is G-12-3.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-12-3: reset of the sole admin (`LastAdmin` or success) waits for the owner answer recorded by E-15/c01 | E-15/c01 / product owner | open | Low |
| `admin create --note --ttl`: `OperatorIdentityOps::create_admin_invite()` takes no note and no TTL in the identity contract (contract open point 6) | E-15 owner | open | Medium |
| The error of `reset_admin` for an unknown or non-admin account, and of `revoke` for an unknown or redeemed invite, is not named yet in the identity contract; AC-4 assumes `NotFound` | E-15 owner | open | Low |
