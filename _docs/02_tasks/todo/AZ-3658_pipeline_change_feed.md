# Change-feed long-poll pipeline (F7)

**Task**: AZ-3658_pipeline_change_feed
**Name**: Change-feed pipeline
**Description**: Commits stamp the feed, the in-process notifier wakes held long-polls, and the changes route returns gapless pages, resync and revocation outcomes, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3801_crate_skeleton_accept_loop, AZ-3803_body_limits_timeouts, AZ-3829_tree_transfer_feed_handlers, AZ-3833_graceful_shutdown, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3775_feed_stamp, AZ-3776_change_feed_read, AZ-3777_feed_long_poll, AZ-3709_tree_publish_core, AZ-3683_authenticator_scopes_cache, AZ-3686_session_refresh_rotation, AZ-3687_device_grant_exact_once, AZ-3655_pipeline_large_upload, AZ-3656_pipeline_small_upload_batch, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3658
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p06

## Problem

The feed joins 02's commit (stamp last, notify after COMMIT), 05's `ChangeFeed` and hold algorithm, 01's `PrincipalRevalidator` (passed in by 11) and the shutdown signal from `serve`. The notifier must be one instance shared by 02 and 05, which only the composition root can guarantee. No planned task proves that a real commit wakes a real held request, or that the sequence stays gapless under concurrent commits through the real routes.

## Outcome

- A long-poll held on the real `GET /api/v1/changes` returns within 5 s after a commit on the same account, with latest-state items and the next cursor.
- Resync, revocation and shutdown outcomes reach the client through the real stack.
- Every component operation of flow F7 has a production caller; at least one integration test runs F7 through the real binary over HTTP.

## Scope

### Included
- Flow order and seams (`system-flows.md` F7 steps 1-6):
  1. Commit (E-22/p03, p04 paths) → `FeedStamp::stamp` last → COMMIT → `FeedNotifier::notify(account, seq)`.
  2. `GET /api/v1/changes?cursor=E.S&wait&limit` → 11 passes the `Principal` and 01's `PrincipalRevalidator` → `ChangeFeed::read` → `ChangePage{items, next_cursor, has_more}` or `ResyncRequired{reason}` (410).
  3. Hold: parked without a database connection, woken by the notifier, rechecked by the safety recheck and the auth recheck.
  4. Revocation: `SessionService::revoke` or device revoke → revalidate fails → held request ends with 401.
  5. `serve` shutdown → `ChangeFeed` shutdown signal → held reads return an empty page.
  6. `ChangeFeed::head` for a new device.
- The one-instance notifier binding in the composition root.
- The end-to-end integration test of the steps above.

### Excluded
- Client feed loops (E-23/c12, E-24/p03), tombstone purge and horizon (E-22/p11), epoch bump (E-22/p13).

## Acceptance Criteria

AC-1..AC-6 are local to this task. AC-69 keeps its number from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: A commit wakes a held poll**
Given device B holding a poll on the account at the head cursor
When device A commits a file through the real routes
Then B's poll returns within 5 s with that node's latest state and a next cursor equal to the new head

**AC-2: Timeout without change**
Given no change on the account and a short configured wait
When a device polls
Then it gets an empty page with the same cursor after the wait, and the pool's in-use connection count does not grow while it is parked

**AC-3: Resync outcomes**
Given cursors with an unknown epoch, a sequence above head and a sequence below the purge horizon
When each is polled
Then each gets 410 `resync_required` with the reason `epoch_changed`, `cursor_ahead` or `below_purge_horizon`

**AC-4: Revocation ends held polls**
Given a held poll by a session and another by a device token
When the session and the device are revoked
Then both polls end with 401 within 5 s

**AC-5: Shutdown wakes held polls**
Given held polls
When `serve` receives SIGTERM
Then the polls return empty pages with their cursors before the grace period ends

**AC-6: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `ChangeFeed::{read, head}`, `FeedNotifier::notify` (from 02 after COMMIT), `PrincipalRevalidator` (passed by 11), the shutdown signal (from `serve`), and exactly one notifier construction shared by 02 and 05 (in 12)

### Interaction criteria (Step 1.8)

**AC-69: Concurrent commits give a gapless, once-only feed** (R2)
Given one account
When 50 commits on distinct files run concurrently through the real routes (large and batch paths mixed) and a device then reads the feed from cursor 0 to head
Then the device sees every changed node exactly once in latest state, the `seq` values are strictly increasing with no gap and no repeat, and the final cursor equals the head

## Non-Functional Requirements

**Performance**
- Change visible to a held poll within 5 s at p95 (AC-5); measured under load in E-31/c43.

**Reliability**
- No database connection is held while a poll is parked.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Static caller check over non-test sources | Each named operation has a production caller; one notifier construction |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Held poll at head | Commit from another device | Poll returns within 5 s with the change | AC-5 |
| AC-2 | No change, short wait | Poll | Empty page, same cursor, no extra pool connection | |
| AC-3 | Three bad cursors | Poll | 410 with each reason | AC-41 |
| AC-4 | Held polls by session and device | Revoke | 401 within 5 s | AC-21 |
| AC-5 | Held polls | SIGTERM | Empty pages before grace ends | |
| AC-69 | One account | 50 concurrent commits, read from cursor 0 | Each node once, seq gapless and increasing, cursor = head | NFT-RES-35 |

## Constraints

- Owner: component 12 (`src/server/`): notifier and revalidator bindings for this flow and its end-to-end tests among 12's integration tests. No SQL or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 over HTTP; no sunnycloud component is faked.
- The stamp's own concurrency suite stays in E-12/c03 and the in-process "via 02" case in E-12/c09; AC-69 here checks the outcome through the routes.
- Downstream: E-31/c24 (FT-P-07..09, FT-P-13, FT-P-14) and E-31/c41 (NFT-RES-35) should depend on this task.

## Risks & Mitigation

**Risk 1: Two notifier instances**
- *Risk*: 02 and 05 get separate notifier objects; commits never wake holds and only the 10 s safety recheck delivers changes, so tests pass slowly but AC-5 fails under load.
- *Mitigation*: AC-1 bounds the wake at 5 s with a safety recheck configured longer than that in the test.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-05-2 hold-cap error name and code is still a spike (E-12/c01) | E-12 | open | Low |
