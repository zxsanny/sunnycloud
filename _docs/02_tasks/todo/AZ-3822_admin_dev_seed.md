# DevSeed: demo data outside production

**Task**: AZ-3822_admin_dev_seed
**Name**: Development seed
**Description**: `DevSeed::seed` creates a small demo installation through the normal service ports in development and refuses to run in production, writing nothing
**Complexity**: 2 points
**Dependencies**: AZ-3816_admin_crate_skeleton, AZ-3817_admin_user_management, AZ-3688_setup_token_invites_operator_ops, AZ-3708_tree_folder_mutations, AZ-3737_transfer_stage_small, AZ-3754_transfer_commit_batch, AZ-3697_share_create_list_revoke, AZ-3751_quota_admin_lifecycle, AZ-3841_job_planner_enqueue_effect
**Component**: 07 admin
**Tracker**: AZ-3822
**Epic**: AZ-3624 (E-17)
**Ref**: E-17/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/07_admin/admin_service.md` — `set_quota` (E-17/c03).

## Problem

Developers need a populated installation without signing in through providers many times (`data_model.md` §13.2). Running the seed against production would plant demo data and accounts there.

## Outcome

- In development, `seed` creates through the same service ports (so every invariant holds): one admin and two users, a 1 GiB quota on one user, a small tree with Unicode and mixed-case names, one open upload session, one share link per user, and queued media jobs from a committed photo.
- A second `seed` adds no duplicates or reports that the installation is already seeded.
- With `environment = production`, `seed` returns a refusal and writes nothing.

## Scope

### Included
- The seed content of `data_model.md` §13.2 and the production refusal.

### Excluded
- The `admin dev-seed` command (E-22/c09); test fixtures (E-31).

## Acceptance Criteria

**AC-1: Seed in development**
Given `environment = development` and an empty database
When `seed` runs
Then it returns Ok, at least one admin and one user exist, one user has a 1 GiB limit, each user has a share link, one upload session is open and at least one media job is queued

**AC-2: Second run adds nothing**
Given a seeded installation
When `seed` runs again
Then no duplicate account, node or share appears, or the call reports that the installation is already seeded

**AC-3: Refused in production**
Given `environment = production`
When `seed` runs
Then it returns the refusal and the row counts of `accounts` and `nodes` are unchanged

## Non-Functional Requirements

**Reliability**
- The seed goes through ports only; no direct row inserts.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Environment check before any call | Production refused first |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Development, empty database | IT-07-14 | Ok; admin and user exist; §13.2 content present | |
| AC-2 | Seeded installation | IT-07-14 second call | No duplicates | |
| AC-3 | Production | IT-07-14 | Refusal; 0 rows written | |

## Constraints

- The ports beyond 07's Requires (tree, upload, share, account creation) are passed by the composition root to `DevSeed` only.

## Risks & Mitigation

**Risk 1: Seed drifts from real invariants**
- *Risk*: a seed shortcut creates data the services would refuse.
- *Mitigation*: ports only; AC-1 runs against the real components.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No port creates an account without a provider sign-in: `OperatorIdentityOps` returns invite tokens, whose redemption needs a sign-in (mock-idp in dev). How `seed` creates its accounts is not specified | E-15 / E-17 owners | resolved (owner decision 2026-11-14: setup token and invites redeemed by sign-ins; the provider step is 07's `ProviderLogin` port) | Medium |
| The refusal has no variant name (contracts part 1 open point 5) | E-17 owner | resolved: the existing `DevSeedError::RefusedInProduction` (contracts part 1 1.5.1) | Low |
| Queued media jobs: 08's `MediaEnqueueEffect` is built for one account, and 03's `UploadPorts::commit_effects` is a fixed list, so one `CommitService` cannot queue jobs for accounts the seed creates; the seed commits the photo and queues whatever the composition root's effects queue (none today). IT-07-14 does not assert a queued job | E-16 / E-22 owners (per-account effect slot in the composition root) | open | Medium |
| `ProviderLogin` has no development implementation: mock-idp (`tests/mocks/idp`, E-31/c13) does not exist yet, so `admin dev-seed` (E-22/c09) cannot be composed; IT-07-14 drives 01's in-process provider | E-22 / E-31 owners | open | Medium |

## Owner decision

Owner decision 2026-11-14: the seed creates its accounts through the normal path — operator invites redeemed by sign-ins against the development mock identity provider; no development-only account-creation port.
