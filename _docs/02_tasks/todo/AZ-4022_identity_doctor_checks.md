# Identity doctor checks: providers and redirect URIs, setup state

**Task**: AZ-4022_identity_doctor_checks
**Name**: Identity doctor checks
**Description**: Two `DoctorCheck`s owned by 01: per enabled provider, print the exact redirect URI to register and check that the provider's discovery or token endpoint answers within 10 s (also in `doctor --pre-flight`); and report the setup state (setup token pending with its age, no admin yet)
**Complexity**: 3 points
**Dependencies**: AZ-3768_contracts_part1, AZ-3682_identity_crate_migration_config, AZ-3684_provider_sign_in_callbacks, AZ-3688_setup_token_invites_operator_ops, AZ-3694_provider_linking_and_list, AZ-3807_doctor_aggregation; notes: E-31/c13 (test epic, Step 9: mock-idp)
**Component**: 01 identity
**Tracker**: AZ-4022
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/n01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `DoctorCheck`, `DoctorTag`, `DoctorContext`, `Finding`.
- `_docs/02_document/components/12_operator_cli/description.md` §6 — "providers and redirect URIs (01), setup state (01)".
- `_docs/02_document/deployment/observability.md` "`sunnycloud doctor`" table — rows "Providers" and "Setup".

## Problem

The install check of AC-22 needs `doctor --pre-flight` to print "the exact redirect URIs to register for each enabled provider" before the stack starts (`deployment_procedures.md` step 6), and `doctor` to reach exit 0 with providers checked (step 9). 12 §6 assigns these checks to 01, but no E-15 task implements them (AZ-3809 flags the gap and calls a hard-coded redirect list an unacceptable substitute). The redirect URI must come from the same builder as sign-in (`public_url`, never the request host), or an operator registers a URI the server will not send.

## Outcome

- A providers check, tagged `pre-flight`: for each enabled provider, a finding that prints its redirect URI `{public_url}/api/v1/auth/providers/{key}/callback`, built by the same code as `SignInService::begin`, and the result of reaching the provider's discovery endpoint (`oidc`) or token endpoint (`github`) within `provider_timeout` (10 s), through the same outbound rules as sign-in.
- A setup check (not `pre-flight`, it needs the database): while no admin exists it reports that setup is pending, with the age of the open setup token when there is one; once an admin exists it is Ok.
- Both checks are read-only, never print a client secret, setup-token plaintext or token hash, and are registered with the composition root's `DoctorCheck` list (E-22/c03 registers "the `DoctorCheck`s of the owners").

## Scope

### Included
- The two `DoctorCheck` implementations in 01 and their unit tests with recording fakes; a blackbox run through `doctor`.

### Excluded
- Configuration validation (helper 01, through AZ-3809's config check); the aggregation, levels to exit codes and `--json` (AZ-3807); the public URL, TLS and proxy probes (E-21/n01); the Apple provider kind (v1.1, E-15/n02 extends the providers check).

## Acceptance Criteria

**AC-1: Exact redirect URIs before the stack starts (AC-22, IT-12-15 pre-flight, AT-12-1 step 1)**
Given `public_url = https://sunnycloud.test`, providers `github` (key `gh`) and `oidc` (key `google`) configured, and no database running
When `doctor --pre-flight` runs
Then one finding per provider prints `https://sunnycloud.test/api/v1/auth/providers/gh/callback` and `https://sunnycloud.test/api/v1/auth/providers/google/callback`, each equal byte for byte to the `redirect_uri` that `begin` sends for that provider, and no database connection is attempted

**AC-2: Provider reachability within 10 s (observability "Providers")**
Given mock-idp answering for `gh`, a stalled discovery endpoint for `google` (no answer for longer than `provider_timeout`), and a third provider whose endpoint resolves to a link-local address
When `doctor` runs
Then `gh` is Ok, `google` is not Ok within `provider_timeout` plus 1 s with a remedy naming the provider key, the third is not Ok and its address receives 0 requests, and every finding still prints its redirect URI

**AC-3: Setup state (observability "Setup", AC-23)**
Given, in turn, an empty database with an open setup token created 2 days ago, an empty database with no open token, and a database with one admin account
When `doctor` runs
Then the first reports setup pending with the token age (2 days) and never the token, the second reports setup pending with no admin yet and the remedy `sunnycloud admin setup-token`, the third gives Ok for setup, and none of the three is Fail

**AC-4: Install verdict (AT-12-1 step 3, FT-P-69)**
Given a fresh stack with one provider reachable and the first admin signed in
When `doctor` runs
Then both identity checks are Ok, so they do not stop `doctor` from exiting 0

**AC-5: Read-only, no secrets (IT-12-15, NFT-SEC-38)**
Given known client secrets and an open setup token
When `doctor` and `doctor --json` run
Then no client secret, setup-token plaintext or token hash appears in the output, and no identity table row is inserted, updated or deleted

## Non-Functional Requirements

**Performance**
- Probes of all providers together stay inside the `doctor` budget (target 60 s, failure 180 s, PT-12-1): each probe is bounded by `provider_timeout`.

**Reliability**
- A slow or failing provider only makes its own finding non-Ok; the other findings are produced.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Redirect URI in the finding against the `begin` builder | Equal for every configured provider |
| AC-2 | Probe result mapping (answer, timeout, refused destination) | Ok, non-Ok with remedy, non-Ok with 0 requests |
| AC-3 | Setup state mapping (token age, no token, admin present) | Pending with age; pending with remedy; Ok |
| AC-5 | Finding text for planted secrets | 0 secret occurrences |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Two providers, database stopped | `doctor --pre-flight` (IT-12-15, AT-12-1 step 1) | Exact redirect URIs; no connection | AC-22 |
| AC-2 | mock-idp, stalled and link-local providers | `doctor` (no ID yet) | Ok; non-Ok within 11 s; 0 requests to link-local | NFT-SEC-30 |
| AC-3 | Three setup states | `doctor` (no ID yet) | Pending with age; pending; Ok | AC-23 |
| AC-4 | Fresh stack, first admin | `doctor` (AT-12-1 step 3) | Identity findings Ok | AC-22, FT-P-69 |
| AC-5 | Known secrets | `doctor`, `doctor --json` (IT-12-15) | 0 secrets; 0 row changes | NFT-SEC-38 |

## Runtime Completeness

- **Named capability**: the providers and redirect-URI check and the setup check of AC-22 (12 §6, owner 01).
- **Production code that must exist**: both `DoctorCheck`s in 01, using the real redirect-URI builder, the real outbound client with its rules and timeout, and the real setup-state read.
- **Allowed external stubs**: mock-idp as the provider in tests.
- **Unacceptable substitutes**: a hard-coded redirect-URI list, a check that always returns Ok, a probe that bypasses the outbound rules of AZ-3684.

## Constraints

- No second redirect-URI builder: the check calls the one `begin` uses.
- The check makes the same outbound calls sign-in is allowed to make; it never follows redirects.

## Risks & Mitigation

**Risk 1: Pre-flight without network**
- *Risk*: `doctor --pre-flight` runs before the stack, possibly without outbound network, so the reachability half fails while the redirect URIs are what the operator needs.
- *Mitigation*: the redirect URI is printed in every finding, whatever the probe result (AC-2).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The levels are not stated: observability says "Fails or warns when" for providers and "info" for setup, but `Finding` has only Ok, Warn and Fail. The ACs require non-Ok for an unreachable provider and "not Fail" for setup pending; the exact levels need the owner (they decide whether a pre-setup `doctor` exits 0 or 1) | E-15 / E-22 owners | open | Medium |
| Whether `--pre-flight` probes reachability or only prints the redirect URIs is not stated (`deployment_procedures.md` step 6 names only the URIs); this task tags the whole providers check `pre-flight` | E-22 owner | open | Low |
| "No admin yet" while accounts exist cannot happen through the documented commands (the last admin cannot be demoted, IT-12-2); the check treats "no admin" and "setup pending" as one state | E-15 owner | open | Low |
| The registration list of the composition root (AZ-3789) names owner checks generically; the parent should make sure this check is in it once it has a key | Parent (Step 4) | open | Low |
