# First-run bootstrap pipeline (F1)

**Task**: AZ-3654_pipeline_first_run_bootstrap
**Name**: First-run bootstrap pipeline
**Description**: The `migrate` step, `serve` and the setup token take a fresh install to its first signed-in admin, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3801_crate_skeleton_accept_loop, AZ-3804_version_gate_server_info, AZ-3828_identity_admin_handlers, AZ-3789_composition_root, AZ-3790_per_command_config, AZ-3791_serve_start_stop, AZ-3793_migrate_commands, AZ-3794_identity_operator_commands, AZ-3684_provider_sign_in_callbacks, AZ-3685_invite_redemption_account_creation, AZ-3688_setup_token_invites_operator_ops, AZ-3694_provider_linking_and_list, AZ-3936_sign_in_setup_pages, AZ-3652_initial_structure; notes: E-31/c13 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3654
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p01

## Problem

Each part of a first install has its own task: the migrator (E-06), `migrate` and `serve` (E-22), the setup token and sign-in (E-15), the routes (E-21) and the setup page (E-26). None of them drives the whole path from an empty database to a signed-in first admin, and no planned task owns the `serve` callsite that issues the setup token while zero accounts exist. Every part can pass its own tests while a fresh install still cannot be bootstrapped.

## Outcome

- On an empty database, `sunnycloud migrate` followed by `sunnycloud serve` prints a one-time setup token, and the first sign-in that presents it becomes the first admin.
- Every component operation of flow F1 has a production caller in the `sunnycloud` binary or in the routes it serves.
- At least one integration test runs F1 through the real binaries over HTTP, with only the identity provider mocked.

## Scope

### Included
- Flow order and the data at each seam (`system-flows.md` F1 steps 1-5):
  1. Compose `migrate` step → `sunnycloud migrate` → `Migrator::run_pending` (helper db, role `sunnycloud_migrator`).
  2. `serve` → `Migrator::check_version` (refuse unless `Current`) → typed config validation (at least one provider) → composition root.
  3. `serve` with zero accounts → `OperatorIdentityOps::ensure_setup_token` → token printed once through the secret channel; only its SHA-256 is stored.
  4. Provider list route → `SignInService::list_providers` → `setup_required = true` (read by the web setup page, E-26/c07).
  5. Sign-in carrying the token → `SignInService::begin` (token kept in the login state, never sent to the provider) → mock-idp → `SignInService::complete` → account with role admin created and token consumed in one transaction → session.
  6. Operator recovery: `admin setup-token` (`ensure_setup_token`) while zero accounts exist; `admin create` (`OperatorIdentityOps::create_admin_invite`).
- The `serve` callsite of step 3, which no planned task owns.
- The end-to-end integration test of the steps above.

### Excluded
- Migration content (each component), sign-in internals (E-15), the setup page UI (E-26/c07), sign-in with invites and sessions in general (E-22/p02), the 30-minute install measurement (AT-12-1 in E-22/c17).

## Acceptance Criteria

AC-1..AC-5 are local to this task. No interaction risk of `interaction-risks.md` (R1-R13) lies on F1.

### Wiring and flow criteria

**AC-1: Fresh install reaches a first admin**
Given an empty PostgreSQL 18 database, a blob volume, a master key and one configured provider (mock-idp)
When the test runs `sunnycloud migrate`, starts `sunnycloud serve`, reads the setup token from the log and signs in with it through the real routes
Then exactly one account exists with role admin, the setup token is consumed, and the returned session authenticates the next request

**AC-2: Setup token is required and single use**
Given the install of AC-1 before the first sign-in
When one sign-in runs without the token and two sign-ins run concurrently with the same token
Then the tokenless sign-in creates no account and answers "invitation required", exactly one tokened sign-in creates the admin, and the other is refused

**AC-3: Start refusals**
Given a database one migration behind the binary, or a configuration with no provider
When `sunnycloud serve` starts
Then it exits non-zero before any listener binds, and the message names `sunnycloud migrate` or the missing provider setting

**AC-4: Operator recovery commands**
Given zero accounts and a lost token, and separately an install with one admin
When `admin setup-token` runs, and `admin create` runs
Then a new setup token replaces the old one (the old one is refused), and a one-time admin invite is printed once and a sign-in with it creates a second admin

**AC-5: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `Migrator::run_pending`, `Migrator::check_version`, `OperatorIdentityOps::ensure_setup_token`, `OperatorIdentityOps::create_admin_invite`, `SignInService::list_providers`, `SignInService::begin` and `SignInService::complete`, and the integration test starts the server only through the `sunnycloud` binary

## Non-Functional Requirements

**Performance**
- `serve` start to `/readyz` 200 within 10 s (PT-12-1).

**Reliability**
- The setup token appears once on stdout or the log and never in `--json` output, audit rows or a second log line (ST-12-1).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Static caller check over non-test sources | Each named operation has at least one production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty DB, mock-idp `valid` | migrate, serve, setup-token sign-in | One admin account, token consumed, session works | PT-12-1 |
| AC-2 | Empty DB after serve | Tokenless sign-in; two concurrent tokened sign-ins | No account; one admin; one refusal | |
| AC-3 | Schema one behind; no provider | `serve` | Non-zero exit before any listener; message names the fix | |
| AC-4 | Zero accounts; one admin | `admin setup-token`; `admin create` | Old token refused, new works; invite creates an admin | ST-12-1 |

## Constraints

- Owner: component 12 (`src/server/`): composition-root and `serve` wiring for this flow and the flow's end-to-end tests among 12's integration tests. 12 holds no SQL, blob I/O or business rule (grep tests); a missing behaviour inside a component goes back to that component's task.
- Tests run the real `sunnycloud` binary (`migrate`, then `serve`) against PostgreSQL 18 and a real blob volume over HTTP. Only the identity provider is mocked (mock-idp, `tests/environment.md`); no sunnycloud component is faked or bypassed.
- Downstream: E-31/c30 (FT-P-64, FT-P-65, FT-N-20, FT-N-21) and E-22/c17 (AT-12-1) should depend on this task.

## Risks & Mitigation

**Risk 1: Token leaks through a log collector**
- *Risk*: the token is printed to a log that the operator ships elsewhere.
- *Mitigation*: print through the secret channel only, once; document `admin setup-token` to replace a leaked token.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `serve` callsite that issues the setup token with zero accounts is named by neither E-22/c05 nor E-15/c08 | This task owns it | resolved | Low |
| Setup page route and fields are still a spike (E-26/c04) | E-26 | open | Low |
