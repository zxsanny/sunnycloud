# Media enqueue effect takes the account per commit

**Task**: AZ-4311_media_effect_account_per_commit
**Name**: Media effect account per commit
**Description**: Give `PublishedEntry` the committing account and let 08's `MediaEnqueueEffect` read it, so one shared effect instance serves every account (owner decision 2026-11-14)
**Complexity**: 2 points
**Dependencies**: AZ-3843_worker_auth_jobs_facade, AZ-3789_composition_root (partly done)
**Component**: 08 media_jobs (with an additive contracts field and 02's publish)
**Tracker**: AZ-4311
**Epic**: AZ-3629 (E-18)

## Problem

Owner decision 2026-11-14 (AZ-3789): 08's media enqueue effect reads the account from each commit instead of its constructor. `CommitEffect::on_entry(tx, entry)` cannot see the account: `PublishedEntry` has no `account_id` and `DbTx` exposes no scope.

## Outcome

- Additive contracts field `PublishedEntry.account_id` (part 2 version and change-log row), filled by 02's `publish` from `CommitActor::account_id`.
- `MediaEnqueueEffect::new` without an account; `on_entry` uses `entry.account_id`.
- Callers updated (02, 03, 07 tests; 07 dev seed media part, which unblocks AZ-3822's media jobs); the shared effect registered in 12's commit-effect list for 03.

## Acceptance Criteria

**AC-1**: Given one shared `MediaEnqueueEffect`, When two accounts each commit a JPEG, Then each gets exactly one job under its own account.
**AC-2**: Given 02's publish, When an entry is published, Then `PublishedEntry.account_id` equals the actor's account.
**AC-3**: Given the composition root, When 03's commit effects are listed, Then they hold the one shared media effect.
