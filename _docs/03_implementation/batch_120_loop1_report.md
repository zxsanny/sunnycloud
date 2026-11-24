# Batch Report

**Batch**: 120
**Tasks**: AZ-3665 account-deletion pipeline (AC-1 to AC-5), archived.
**Date**: 2026-11-24

## What changed

- `tests/consumer/suites/account_deletion/test_account_deletion.py` (4 tests; two accounts: `alice`, the admin with no data, and `bob`, a user with a PNG and its thumbnail job, a share link, a staged upload, an open upload session, a device token and a held long-poll):
  - AC-1: the owner confirms with their own id; the answer is 202 with a deadline; the held long-poll ends with 401 within 5 s, the device token gets 401, and the pending commit is refused (401, as the session is already revoked) with no new node.
  - AC-2: the real scheduler runs the purger and the sweeper; 15 tables (accounts, identities, sessions, device tokens, nodes, revisions, blobs, blob GC queue, upload sessions, share links, jobs, derived rows, quota, feeds, login codes) are back to what they held before bob existed; exactly one `account_deleted` audit row, whose detail names no secret or person; a later sign-in as the deleted identity is an unknown identity and needs an invite.
  - AC-3: the api is killed (SIGKILL) right after the 202, while the account is still `deleting`; after the restart the purge finishes with the same clean result and the audit row is written once. The test skips as NOT-RUN(NO-CRASH-WINDOW) if the purge beat the kill; it did not skip in the run.
  - AC-4: the last admin gets 409 `last_admin`; a device token gets 403; a wrong confirmation is a 4xx; both accounts stay active.
- `src/server/tests/wiring_flows.rs` (AC-5): the route and `admin user delete` ask for deletion, the purger and the sweeper are on the scheduler, 07 begins and finalizes the deletion. The documented hook order is the existing `composition_root` test.
- Harness: ctl action `process-kill` (SIGKILL or start of the api or worker), `db-probe` queries `table_rows` and `account_states`, `account_deleted` in `audit_events`. The compose stack sets `account_delete_interval` to 8 s and `gc_interval` to 5 s (production: 5 s and 5 min).

## Code Review Verdict: PASS (own review)

Test and harness code, one Rust test file; no product change.

## Test Suite

- `tests/run.sh --profile core`: PASS=65 FAIL=0 NOT-RUN=1 (the sentinel), exit 0.
- CI-parity: `cargo fmt --all --check`, clippy `-D warnings` on `sunnycloud-server`, `wiring_flows` 5 of 5; ctl unit tests 27 OK.

## Not covered

- The files of the account on disk are not listed (the ctl action `blob-reconcile` does not exist yet): a purge is shown by the blob rows and the GC queue being empty, which the sweeper clears only after it unlinked the file. A file check belongs to the action.
- A large account (15 minutes, AC-27) is E-17/c10; this task uses a small one.

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| AC-1 "the pending commit is refused as account not active" | The session is revoked first, so the commit is a 401; the refusal is asserted by status (401 or 403) and by no node being created | clear |
| AC-3 "a purge in progress" | A small account purges in one cycle; the kill lands between the request and the first cycle (state `deleting`), which is the state a resumed purge starts from | clear |

## Commit

`[AZ-3665] Account deletion on the compose stack: access ends at once, everything purged, crash resume, guards; process-kill action` - Loop: 1

## Next: AZ-3661 media outputs, AZ-3813 and AZ-3815 (CLI suites), AZ-3835 AC-1 and AC-4
