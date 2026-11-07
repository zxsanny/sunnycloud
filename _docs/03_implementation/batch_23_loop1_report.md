# Batch Report

**Batch**: 23 (decision spikes, documentation only)
**Tasks**: AZ-3773_feed_gaps_spike, AZ-3800_status_assignment_spike, AZ-3826_media_gaps_spike, AZ-3695_sharing_gaps_grant_key_spike, AZ-3814_admin_gaps_spike
**Date**: 2026-11-07

No implementation task was ready after batch 22, so the owner approved drafted answers for five spikes and four documentation workers recorded them. No source code changed; the CI-parity gate does not apply to a docs-only commit. Parent edits: `tests/spec-gaps.md` (8 open gaps resolved, 13 resolved rows dated 2026-11-07, counts recomputed 57 to 49), flagged-concern rows in the spike specs.

## Decisions recorded (owner approved 2026-11-07)

| Spike | Decision |
|-------|----------|
| AZ-3773 | G-05-2: the 5th concurrent long-poll is the new typed `ChangeFeedError::TooManyHolds{retry_after}`, sent as the existing `rate_limited` (429) with `Retry-After` (5 s proposed). G-05-3: `raise_purge_horizon` never decreases the horizon (`GREATEST`). |
| AZ-3800 | `need_blocks` and `edited_since` are 409. `batch_too_large` is 422; a body over the 4 MiB class is a separate 413. F5, FT-N-48 and AZ-3656 changed from 413 to 422. |
| AZ-3826 | `thumbnail-states` omits foreign and unknown ids and answers 422 above 1,000 ids; worker input ignores `Range`; a job is obsolete only once the revision is collected; `ClaimedJob.content_hash` is lowercase hex. |
| AZ-3695 | A correct password on a limited link is `RateLimited`; `expires_at > now` is live; `ShareConfig` validation (06) refuses a missing, empty or under-32-byte `share_grant_key`, called by `serve` (12) before binding the listener. |
| AZ-3814 | Per-step purger claim through a proposed `AccountAdmin::lock_deleting`; non-admin gets 403, another account's id 404; deletion confirmation is the caller's own account id (inferred from ST-07-2); operator path is a separate proposed `request_as_operator`. |

## Open items created

| # | Item | Owner / next step |
|---|------|-------------------|
| 1 | The approved claim `FOR UPDATE SKIP LOCKED` can deadlock with deletion hooks that insert rows with a foreign key to `accounts` (they need `FOR KEY SHARE`). `FOR NO KEY UPDATE` avoids it. Recorded as an open risk in E-17, AZ-3665, AZ-3820 and `spec-gaps.md`; E-17/c06 confirms in IT-07-9 | Owner asked; unanswered |
| 2 | Proposed `contracts` changes without tickets: `TooManyHolds` variant (E-12 / E-00), `AccountAdmin::lock_deleting` (E-15), `request_as_operator` (E-17), `AdminError::Forbidden` (E-17/c01 AC-4) | Create tasks after the owner agrees |
| 3 | 5 s `Retry-After` is proposed, not agreed | Owner |
| 4 | `thumbnail-states` takes node ids but `ThumbnailStateSource::states` takes content hashes; the mapping is undecided | E-18 |
| 5 | `request_as_operator` for an unknown or already-deleting account, the `ConfirmationMismatch` wire code, the per-cycle purger time budget | E-17 |
| 6 | The E-01 loader spec and the E-22 start sequence must state the grant-key rules; `EPIC E-31` line 199 still lists G-S6 as a spike | E-01 / E-22 / E-31 |

## Commit

`[AZ-3773] [AZ-3800] [AZ-3826] [AZ-3695] [AZ-3814] record owner decisions for five spikes` — Loop: 1

## Next Batch

Recompute ready tasks. Unblocked by these spikes: AZ-3777 feed long poll, AZ-3778 horizon lifecycle, AZ-3810 error mapper, AZ-3837 media shared params and protocol, AZ-3696 sharing migration and config. Several of these also depend on the proposed contracts changes (item 2).
