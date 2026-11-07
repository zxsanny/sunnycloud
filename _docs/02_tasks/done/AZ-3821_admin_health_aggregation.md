# Admin health aggregation with a timeout per check

**Task**: AZ-3821_admin_health_aggregation
**Name**: Admin health aggregation
**Description**: `AdminService::health` collects every registered health contributor, bounds each check by its own timeout, and reports each by name with status and detail, so a slow or failing component shows as not ok instead of blocking the page
**Complexity**: 3 points
**Dependencies**: AZ-3816_admin_crate_skeleton, AZ-3817_admin_user_management, AZ-3768_contracts_part1
**Component**: 07 admin
**Tracker**: AZ-3821
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `HealthContributor`, `HealthItem` (E-00/c03).
- `_docs/02_document/contracts/04_blobstore/epoch-marker-and-health.md` — 04's contributors (E-10).

## Problem

Admins need one view of the installation's health (database, blob volume against the floor, master key, scrub result, dead jobs, janitor lag). The contributors live in other components; one slow check must not hold the whole page.

## Outcome

- `health(actor)` returns a `HealthReport` with one item per registered contributor: its `name()`, status `ok`, `degraded` or `down`, and detail.
- Each check runs under `health_check_timeout`; a check that does not answer in time is reported `down` with a timed-out detail, and the other items are still present.
- Only an admin session or the operator may call it (same actor rule as E-17/c03).

## Scope

### Included
- Aggregation over the contributor list passed to the constructor; per-check timeout; the actor check.

### Excluded
- The contributors themselves (04, 08 in E-18/c11, 12); `/healthz`, `/readyz` and their `ReadinessCheck`s (E-21/c16); the route (E-21/c12).

## Acceptance Criteria

**AC-1: Every contributor reported**
Given test contributors named database (ok), blob volume (degraded), master key (ok), scrub (ok), dead jobs (down) and janitor lag (ok)
When an admin calls `health`
Then the report lists each by name with its status and detail, and the report is not ok

**AC-2: A slow contributor is bounded**
Given the AC-1 contributors plus one that sleeps 5 s, and `health_check_timeout` of 300 ms
When an admin calls `health`
Then the sleeping contributor is reported `down` with a timed-out detail within 500 ms, the other items are present, and the whole call returns in under 1 s

**AC-3: A failing contributor**
Given a contributor whose check fails
When an admin calls `health`
Then it is reported `down` under its name and the other items are present

**AC-4: Degraded is visible**
Given one contributor reporting degraded
When `health` is called
Then that item shows `degraded`

**AC-5: Admin-only**
Given an ordinary user's session and an admin's device token
When each calls `health`
Then each gets `Forbidden`

## Non-Functional Requirements

**Performance**
- Whole call under 1 s with one hung contributor (IT-07-6).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Timeout per check with a manual clock | Hung check marked down; others kept |
| AC-5 | Actor check | Non-admin refused |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Six named test contributors | IT-07-6 | All listed; report not ok | |
| AC-2 | Plus a 5 s sleeper; timeout 300 ms | IT-07-6 | Down within 500 ms; call under 1 s | |
| AC-3 | Plus a failing contributor | IT-07-6 | Down; others present | |
| AC-4 | One degraded contributor | AT-07-2 step 4 (port form) | `degraded` shown | |
| AC-5 | User session; admin device token | ST-07-1 (health) | `Forbidden` | NFT-SEC-13 |

## Constraints

- Test contributors are allowed here (named in IT-07-6); the real contributors are wired by E-22/c03.
- Stateless: no cached report.

## Risks & Mitigation

**Risk 1: Many slow contributors add up**
- *Risk*: several hung checks exceed the page budget if they wait one after another.
- *Mitigation*: AC-2 bounds the whole call; the check count is small (six named).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-07-6 expects "overall is not ok", but `HealthReport` in contracts part 1 has items only; the rule for an overall status is not stated | E-17 owner | open | Low |
| IT-07-6 uses a contributor "returning an error", but `HealthContributor::check` has no error channel in contracts part 1; AC-3 treats a failing check (error or panic) as `down` | E-17 / E-00 owners | open | Low |

## Contract

This task produces the `health` part of `_docs/02_document/contracts/07_admin/admin_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
