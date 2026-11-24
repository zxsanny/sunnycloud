# Batch Report

**Batch**: 118
**Tasks**: AZ-3664 sign-in pipeline, part 2 (AC-2, AC-3, AC-5); with batch 117 all six ACs and AC-75 are covered, so the spec is archived.
**Date**: 2026-11-24

## What changed

- Mock identity provider (`tests/mocks/idp`): scripted faults of the token endpoint through `POST /__control/fault`: `wrong-nonce`, `wrong-audience`, `wrong-issuer` (one claim of the next id token is wrong) and `slow` (the answer comes 12 s late). A fault applies once. 3 new bun tests (22 of 22 pass, typecheck clean). The compose stack gains the user `mallory` (the admin's email, not verified).
- ctl `db-probe` gains `audit_events` (sign-in, abuse-trip, refresh-reuse, session-revoked counts) and `audit_detail_keys` (the distinct keys of the audit detail).
- Consumer: `Browser` is split into `begin`, `consent`, `callback` and `sign_in`; `FailureBudget` paces tests that make failed sign-in steps (one client address, 10 per minute); `restore_and_wait`, `advance_clock` (the `:9091` test clock) and `idp_fault` helpers.
- `suites/sign_in/test_negative_callbacks.py` (AC-2 and NFT-SEC-03): missing state, replayed state, state of another browser, a callback of another provider, wrong nonce, audience and issuer, an absolute, protocol-relative and backslash return path, a foreign `Host` and `X-Forwarded-Host`, a state older than its TTL (test clock +11 min), the unverified email of a victim (ignored, no session, no account), and a provider slower than the provider timeout (the step ends in time and an existing session keeps working). Each case asserts: refused, no session, no new account, no internals in the answer, and a valid sign-in still works. Cases 10 and 11 (a login code used twice, a wrong verifier) are in `test_sign_in.py` since batch 117.
- `test_session_rules.py` (AC-3): four concurrent refreshes with one token give four 200s with one and the same successor; a replay after the grace (5 s in this stack, clock moved) is 401, the successor stops working within 5 s, the family refresh is 401, a `refresh_reuse` audit row exists; a cookie POST with no origin, a foreign origin or `Sec-Fetch-Site: cross-site` is 403, and one with the public origin is not.
- `test_abuse_and_audit.py` (AC-5): ten refused sign-in steps, the eleventh is 429 with `Retry-After`, a valid request from the same address is refused too, `abuse_trip` and `sign_in` audit rows exist, and the audit detail keys name no secret or person.
- The race test of batch 117 now paces its rounds with the same `FailureBudget`.

## Code Review Verdict: PASS (own review)

Test and harness code only; no product change in this batch.

## Test Suite

- `tests/run.sh --profile core`: PASS=56 FAIL=0 NOT-RUN=1 (the sentinel), exit 0, about 13 minutes with the image builds.
- ctl unit tests 26 OK; mock identity provider: bun 22 of 22 and typecheck. No Rust file changed since batch 117 (fmt, clippy and the server tests were green then).

## Not covered

- NFT-SEC-30 (SSRF through provider discovery) and the full NFT-SEC-04 matrix (invalid-token limit) are not part of AZ-3664's ACs.
- The stack's `provider_timeout` is 2 s (testing rule: durations from config) where production defaults to 10 s; the test asserts the configured value.

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| AC-3 says "exactly one successor" and the grace "30 s" | Within the grace the other refreshes get the same sealed successor (01's design); asserted as one successor; the grace is the stack's configured 5 s | clear |
| AC-2 "a provider slower than 10 s" | The stack sets `provider_timeout` to 2 s; the mock's `slow` fault is 12 s, above both | clear |

## Commit

`[AZ-3664] Negative callbacks, session rules, limiter and audit on the compose stack; scripted mock identity provider faults` - Loop: 1

## Next: AZ-3662 share links, AZ-3665 account deletion (both need a file-upload client in the harness)
