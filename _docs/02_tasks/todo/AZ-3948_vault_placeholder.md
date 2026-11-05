# `/vault` placeholder behind the build flag

**Task**: AZ-3948_vault_placeholder
**Name**: Vault placeholder route
**Description**: In v1.0 builds the `/vault` route shows "not available in this version" and the bundle contains no vault code; a build-time flag hands the route to the v1.2 vault page
**Complexity**: 1 point
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3950_playwright_web_harness
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3948
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c18

## Problem

The vault ships in v1.2 (ADR-007, ADR-016). A v1.0 deployment must expose no vault surface (FT-P-73), and no vault code may reach v1.0 users, yet the route needs a stable place for the v1.2 page.

## Outcome

- v1.0 builds: `/vault` shows "not available in this version"; no vault navigation, control, WASM or chunk exists.
- A build-time flag (off for v1.0 and v1.1 release builds) switches the route to E-26/n01.

## Scope

### Included
- The placeholder page, the build flag and a build-output check for vault chunks.

### Excluded
- The vault page itself (E-26/n01).

## Acceptance Criteria

**AC-1: Placeholder in v1.0 (IT-16-19, FT-P-73 step 2)**
Given a v1.0 release build
When `/vault` is opened and the app's navigation and menus are inspected
Then the page shows "not available in this version" and no vault navigation item or create control exists anywhere

**AC-2: No vault code in v1.0 (IT-16-19)**
Given the v1.0 build output
When its files are listed and the check runs
Then no vault WASM and no vault chunk exist, and a seeded vault import into a v1.0 build makes the check fail

**AC-3: Build-time flag only**
Given a build with the flag on
When `/vault` is opened
Then the route renders the vault page; no runtime setting, query parameter or stored preference can turn the flag on in a build made without it

## Non-Functional Requirements

None beyond the criteria.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Build-output check with a seeded vault import | Check fails |
| AC-3 | Route table with the flag off and on | Placeholder off; vault page on |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | v1.0 release build on the test stack | IT-16-19; FT-P-73 step 2 | Placeholder; no vault chunk | AC-13 |

## Constraints

- No vault code ships in v1.0 (16 §5, FT-P-73).

## Risks & Mitigation

**Risk 1: Shared code pulls vault modules in**
- *Risk*: a shared import drags vault code into the v1.0 bundle.
- *Mitigation*: AC-2 build-output check in CI.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| None | — | — | — |
