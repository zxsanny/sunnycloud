# Batch Report

**Batch**: 117
**Tasks**: AZ-3664 sign-in pipeline, part 1 (AC-1, AC-4, AC-6, AC-75). AZ-3664 stays open: AC-2, AC-3 and AC-5 wait on harness pieces (`_process_leftovers/2026-11-24_az-3664-remaining.md`).
**Date**: 2026-11-24

## What changed

- `tests/consumer/suites/sign_in/test_sign_in.py` (3 tests, real routes through Caddy, mock identity provider only):
  - AC-1: an operator invite creates one complete account; the database probe shows account, identity, quota row and feed row each up by one, and the cookie authenticates `accounts/me`.
  - AC-4: a mobile sign-in gives a one-time code; the exchange with the verifier returns tokens that work as a bearer; a second exchange is refused; a code bound to another app's challenge is refused.
  - AC-75: one invite and two new identities signing in at the same moment, 20 rounds with fresh invites: every round exactly one winner, and the rows go up by exactly one account, identity, quota row and feed row (the loser leaves nothing).
- `src/server/tests/wiring_flows.rs` (renamed from `wiring_first_run.rs`): AC-6, the sign-in operations have production callers (routes of 11, the hook list of 12) and exactly one `AbuseLimiter` is built, in 12. The scan now skips `tests` directories.
- Harness: ctl action `db-probe` (one fixed query of integer counts, no free-form SQL); the mock identity provider takes `MOCK_IDP_ACCEPT_ANY_HINT=1`, under which a plain `login_hint` signs in as a new verified user (the compose stack sets it; 1 new bun test); `suites/conftest.py` holds the `fresh_install` fixture both account suites use.
- Finding, not a defect: the race rounds hit a real 429 on `login-attempts`. The AC-20 limit is 10 failed sign-in steps per client address per minute, fixed (no key), and every race round makes one loser fail. The test paces its rounds to the limiter window instead of loosening anything; the whole suite adds about 3 minutes.

## Code Review Verdict: PASS (own review)

Test code, one mock option and one read-only ctl action; no product change this batch.

## Test Suite

- `tests/run.sh --profile core`: PASS=38 FAIL=0 NOT-RUN=1 (the sentinel), exit 0.
- CI-parity: PASS: `cargo fmt --all --check`; clippy `-D warnings` on `sunnycloud-server`; `wiring_flows` 3 of 3; bun tests 19 of 19 and typecheck; ctl unit tests 25 OK. (The server crate's full suite is unchanged since batch 116: no product change.)

## Not covered

AC-2, AC-3 and AC-5, see the leftover.

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| AC-75 "repeated 20 times" meets AC-20's 10 failed steps per minute from one address | Pace the rounds (done); a per-test client address would need the stack to give the consumer several source addresses | clear |
| AC-5 needs a distinct client address per test, or a limiter reset between tests | A restore of the golden snapshot restarts the api and so resets the in-memory limiter; AC-5 runs right after one | clear |

## Commit

`[AZ-3664] Sign-in on the compose stack: invite account rows, mobile code, 20 concurrent-redemption rounds; wiring check of the sign-in flow` - Loop: 1

## Next: AZ-3662 share links, AZ-3665 account deletion on the stack; the scripted mock identity provider and the clock action that finish AZ-3664
