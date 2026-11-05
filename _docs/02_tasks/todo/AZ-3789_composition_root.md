# Composition root: every component behind its port, `ApiServices`, late-registered lists, one `AbuseLimiter`

**Task**: AZ-3789_composition_root
**Name**: Composition root
**Description**: Build every server component once behind its `contracts` port, hand `ApiServices` to `HttpServer`, construct the single `AbuseLimiter` and the scheduler, and register the late lists in their documented order
**Complexity**: 5 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3790_per_command_config, AZ-3792_janitor_scheduler, AZ-3768_contracts_part1, AZ-3769_contracts_part2, AZ-3770_contracts_part3, AZ-3782_sc_db_access, AZ-3784_sc_audit, AZ-3785_sc_ratelimit, AZ-3786_sc_observability, AZ-3801_crate_skeleton_accept_loop, AZ-3832_ops_endpoints_readiness, AZ-3681_identity_hooks_admin_reset_spike, AZ-3682_identity_crate_migration_config, AZ-3706_tree_skeleton, AZ-3733_transfer_skeleton, AZ-3723_blobstore_skeleton, AZ-3774_sync_feed_skeleton, AZ-3696_sharing_crate_migration_config, AZ-3816_admin_crate_skeleton, AZ-3749_quota_skeleton, AZ-3841_job_planner_enqueue_effect, AZ-3845_media_janitors_erase_hook, AZ-3846_media_admin_health_doctor, AZ-3702_share_account_erase_hook, AZ-3717_tree_lifecycle_blob_cleanup, AZ-3757_transfer_erase_and_crash_cases, AZ-3778_feed_horizon_lifecycle, AZ-3751_quota_admin_lifecycle, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3756_transfer_janitors_reservations, AZ-3730_gc_sweeper, AZ-3743_orphan_sweeper, AZ-3745_scrubber, AZ-3746_blob_maintenance_ports, AZ-3714_tree_cascade_purge_janitors, AZ-3820_deletion_purger, AZ-3821_admin_health_aggregation, AZ-3689_mobile_code_identity_janitor, AZ-3779_epoch_admin
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3789
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — registry ports (`Janitor`, `AccountLifecycleHook`, `BlobOwnerCleanup`, `DoctorCheck`, `HealthContributor`, `ReadinessCheck`) and the 01, 07, 07a ports.
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` and `part3_blob_share_media_vault.md` — the remaining ports the root binds.
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — produced by E-22/c02.

## Problem

ADR-015 removed every component-to-component dependency: components know only `contracts` ports. Something must construct the implementations, hand each consumer the right port, and close the cycles that a constructor graph cannot express (account hooks, blob-owner cleanups, janitors, checks). The rate limiter and the scheduler must exist once, and 11 must construct neither. If a hook is missing from the list, deleted accounts leave rows behind; if a second limiter is built, abuse limits split in two.

## Outcome

- One lazy builder per component service; each service is built at most once per process, only when a command needs it.
- `ApiServices` filled with exactly one implementation of every port 11 requires.
- One `AbuseLimiter` instance shared by 01, 06 and 11; one scheduler instance (E-22/c06) that receives the janitors.
- Late lists registered:
  - `AccountLifecycleHook`: 06, (20 in v1.2 builds only, before 02: owner decision 2026-11-05), 02, 03, 08, then 05, 07a; passed to 07's `DeletionPurger` constructor (gap-pass default G-07-1) and to 01's account-creation path through the entry point E-15/c01 decides.
  - `BlobOwnerCleanup`: 02, 08; given to 04's GC.
  - `CommitEffect`s for 03: the blob effect and receipt effect (03) and the media effect (08).
  - Janitors with the interval of their owner's config key: 03 (session reaper with staged expiry, receipt janitor, idle progress flush), 04 (GC sweeper, orphan sweep, scrub janitor), 02 (tombstone purge, folder cascade), 07 (deletion purger), 01 (identity janitors), 08 (job reaper, derived janitor), and the audit-retention janitor of E-22/c06.
  - `DoctorCheck`s of the owners and of E-22/c14b; `HealthContributor`s to 07's health aggregation; `ReadinessCheck`s to 11, including a database check built on helper 07's health (module-layout open point 4) and `scheduler_alive` (E-22/c06).
- The `ChangeFeed` receives the shutdown signal at construction (contract part 2, open point 5), which `serve` raises.
- The root holds no SQL, blob I/O, crypto or business rule.

## Scope

### Included
- The lazy builders, the `ApiServices` assembly and the `HttpServer` construction.
- Construction of the `AbuseLimiter` and the scheduler, and their injection.
- Registration of the lists above in the stated order.
- The database `ReadinessCheck` adapter over helper 07's health.

### Excluded
- Configuration binding per command (E-22/c04), the start and stop order (E-22/c05), the scheduler itself (E-22/c06), 12's own doctor checks (E-22/c14b).
- Proving each flow through the real routes: the pipelines E-22/p01..p14 own the end-to-end wiring tests (for example the effects list in p03, the hook list order and table sweep in p12, the cleanup list in p11).
- v1.2 vault hook and route registration (E-30 adds it here).
- Doctor checks of AZ-4022 (identity: providers, setup state) and AZ-4023 (HTTP: public URL, TLS, proxy): those tasks add their checks to the doctor registration list; they depend on this task (not the reverse).

## Acceptance Criteria

**AC-1: `ApiServices` complete**
Given the root built for `serve` with the test configuration against PostgreSQL 18 and a blob volume
When `ApiServices` is inspected
Then every port 11 requires has exactly one implementation, and none of them is a test double

**AC-2: Late lists in documented order**
Given the root built for `serve`
When the registered lists are read
Then the hook list is 06, 02, 03, 08, 05, 07a, the cleanup list is 02, 08, the janitor list holds every janitor named in Outcome with its owner's configured interval, and the readiness list holds the database check and `scheduler_alive`

**AC-3: One limiter, one scheduler**
Given the root built for `serve`
When the identity, sharing and HTTP services are built
Then all three hold the same `AbuseLimiter` instance, the limiter and the scheduler are each constructed exactly once, and 11 constructs neither

**AC-4: Only what a command needs**
Given a command that needs only 04's keyring (`key-check`)
When it runs
Then no other component service is constructed; and for `serve` each service is constructed exactly once although several consumers receive it

**AC-5: Every janitor runs (IT-12-7 registration part)**
Given `serve` running with janitor intervals of 200 ms
When `janitor_cycles_total` is read after 10 s
Then every janitor name of AC-2 has a nonzero cycle count and `/readyz` lists `scheduler_alive`

**AC-6: No vault in v1.0 and v1.1 builds**
Given a v1.0 release build
When its hook list, `ApiServices` and route registry are read
Then no vault hook, service or route is present (FT-P-73)

## Non-Functional Requirements

**Performance**
- Building every service for `serve` fits inside the 10 s start-to-ready budget of PT-12-1.

**Reliability**
- A builder failure stops the command with the builder's typed error; nothing is half-registered.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Registration lists against the documented order | Exact order and members |
| AC-3 | Construction counts of limiter and scheduler | One each; same instance in 01, 06, 11 |
| AC-4 | Builder call counts per command | Only needed builders run; each once |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Test config, PostgreSQL 18, blob volume | Construction for `serve` | Every port bound once, no double | |
| AC-5 | `serve`, intervals 200 ms, `/metrics` | Cycle counters and `/readyz` (IT-12-7 part) | Every janitor nonzero; `scheduler_alive` listed | AC-68 |
| AC-6 | v1.0 release build | Lists and routes | No vault item | FT-P-73 |

The deletion sweep of IT-12-7 (every table empty after `admin user delete`) is AZ-3665 (E-22/p12) AC-2; derived-row cleanup after GC is AZ-3663 (E-22/p11) AC-2.

## Constraints

- Wiring only: no SQL, no `sqlx`, no blob paths, no crypto, no business rule (grep test ST-12-3 in E-22/c17; crate-graph test of E-00).
- Every service is held as its `contracts` port; no component crate is imported by another.
- One API process in v1.0 (decision 12).

## Risks & Mitigation

**Risk 1: The root grows into a second business layer (R15)**
- *Risk*: convenience logic creeps into wiring code as components are added.
- *Mitigation*: grep tests, review rule, and the pipelines prove behaviour in owners.

**Risk 2: A new component's hook is never registered**
- *Risk*: its rows survive account deletion.
- *Mitigation*: AC-2 fixes the list; p12 sweeps every table.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How vault crates (19, 20) are kept out of v1.0 and v1.1 builds while 12 wires every component is open (module-layout exception 6, open point 6); AC-6 states only the observable result | E-22, E-27, E-29, E-30 owners | open | Medium |
| The entry point through which 12 hands the hook list to 01's account creation is still a spike (E-15/c01) | E-15 | open | Low |
| How `ChangeFeed` receives its shutdown signal is not a method in `contracts` (part 2 open point 5); this task passes it at construction | E-12 / E-22 owner | open | Low |
| `module-layout.md` open point 4 (12 registers the database readiness check) is applied here | module layout | resolved | Low |
