# Per-command configuration binding and start validation

**Task**: AZ-3790_per_command_config
**Name**: Per-command configuration binding
**Description**: Each command binds only the configuration sections it uses, `serve` binds and validates every section before it connects anywhere, and any configuration problem exits 3 listing every problem
**Complexity**: 2 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3763_sc_config, AZ-3782_sc_db_access, AZ-3682_identity_crate_migration_config, AZ-3696_sharing_crate_migration_config, AZ-3723_blobstore_skeleton, AZ-3801_crate_skeleton_accept_loop
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3790
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_config/sc_config.md` — loader, key table, `ConfigError`.
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — exit code 3 for configuration errors (E-22/c02).

## Problem

The loader, the closed key table, unknown-variable refusal, ranges and secret handling are E-01/c01's, and each section's own rules (at least one provider, retention 0 to 90 days, `public_url`) belong to the section owners. What only 12 can decide is which sections each command needs: `migrate` must work without a master key, `key-check` without providers, `openapi` without a database. If every command bound every section, an unrelated missing setting would block a recovery command.

## Outcome

- Each command binds only the sections its service builders need; a missing or invalid setting of another section never fails it.
- `serve` binds every server section, runs each section's own validation, and refuses before connecting to the database or opening a listener.
- Every configuration problem exits 3 and lists all problems at once, with secrets shown as `***`.
- `migrate` in a container without `db_migrator_url` exits 3 naming the missing secret.

Narrowed against the plan: the row title lists "unknown `SUNNYCLOUD_*` refuses, ranges, providers". Those rules are implemented once in E-01/c01 (loader) and in the section owners (E-15/c02 providers, E-10/c03 retention); this task calls them and does not copy them, so it is 2 points instead of 3.

## Scope

### Included
- The mapping from each command to the sections it binds, driven by the builders it uses (E-22/c03).
- Running each section's validation for `serve` and collecting the problems into one refusal.
- The mapping of `ConfigError` to exit 3 and its printing.

### Excluded
- The loader, the key table and the CLI key rows `cli_confirm_required`, `cli_progress_interval` (already in helper 01's table, E-01/c01).
- Section rules (owners), the rest of the start order (E-22/c05), `doctor`'s config check (E-22/c14b).

## Acceptance Criteria

**AC-1: Unused sections are not required (IT-12-18)**
Given no sign-in provider and no worker token configured
When `migrate`, `admin setup-token` and `key-check` run with what they use present, and `openapi` and `version` run with no database and no master key
Then each command succeeds

**AC-2: `serve` validates before connecting (FT-N-25, FT-P-17, IT-12-6 g, h, i)**
Given, in turn, `SUNNYCLOUD_BOGUS=1`, `retention = 91d`, and no provider configured
When `serve` starts
Then it exits 3 within 10 s without a database connection or a listener, and the message names `SUNNYCLOUD_BOGUS`, `retention` with its range 0 to 90 days, or the provider settings

**AC-3: All problems at once**
Given three invalid keys in three different sections
When `serve` starts
Then one refusal lists all three

**AC-4: `migrate` without its secret (IT-12-10, ST-12-4)**
Given the API container's environment, where `db_migrator_url` is not mounted
When `migrate` runs
Then it exits 3 naming `db_migrator_url` and attempts no connection

**AC-5: No secret in a configuration error (NFT-SEC-38)**
Given a planted value in `db_api_url` and one invalid unrelated key
When the refusal is printed and logged
Then the planted value appears 0 times in stdout, stderr and logs

## Non-Functional Requirements

**Performance**
- Binding and validating fits inside the `serve` start-to-ready budget of 10 s (PT-12-1); helper 01 loads the full table under 50 ms.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Section set per command | No unused section bound |
| AC-3 | Collection of section problems | One error with every problem |
| AC-5 | Printing of a `ConfigError` with a secret key | `***` only |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | No provider, no worker token, no DB for `openapi` | Five commands (IT-12-18 part) | All succeed | |
| AC-2 | Bogus variable; retention 91d; no provider | `serve` (IT-12-6 g, h, i) | Exit 3 within 10 s; no connection; message names the key | FT-N-25, FT-P-17 |
| AC-4 | API container environment | `migrate` (IT-12-10 part) | Exit 3 naming `db_migrator_url` | ST-12-4 |
| AC-5 | Planted secret | Refusal output and logs | 0 occurrences | NFT-SEC-38 |

## Constraints

- The provider rule, the retention range and the `public_url` rule each live once, in their section's validation; 12 calls them.
- Durations and secrets come from configuration only.

## Risks & Mitigation

**Risk 1: A recovery command needs a section it should not**
- *Risk*: a builder pulls in a broad section, so `key-check` fails on a provider typo during an incident.
- *Mitigation*: AC-1 runs the recovery commands with unrelated settings missing.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-2: exact refusal text for out-of-range retention, no provider and a pending upgrade is not fixed; the ACs require only the key or command name and the range | E-22 owner | open | Low |
| AZ-3682 (E-15/c02) flagged that E-22/c04 also lists provider validation; this task calls the identity section's rule and keeps no copy | E-15 / E-22 | resolved | Low |
