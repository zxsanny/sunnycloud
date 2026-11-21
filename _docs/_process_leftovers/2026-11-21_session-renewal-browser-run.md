# Session renewal: real-browser run not done

Blocked action: a Playwright run that signs alice in through the mock IdP, deletes only the session cookie (the same
state as an expired 15-minute cookie), reloads, and expects the app to stay signed in with a rotated `__Secure-refresh`
cookie limited to `/api/v1/auth/session-renewals`.

Reason: the acceptance stack (`scripts/dev/acceptance-up.sh`) was running a binary built before this change and was in
use by another session's download run on 2026-11-21 15:07, so it was not restarted. The server (495 tests, IT-11 gate,
clippy) and the web client (488 unit tests) are covered at their own tier; the browser tier is NOT RUN.

Replay: rebuild the acceptance binary, restart the stack, add `tests/e2e/tests/web-renewal.spec.ts` (two assertions
above), run it on this host. The live stack on drive.zxsanny.com runs the old binary until `scripts/dev/mvp-up.sh` is
run again; that also waits for the Google OAuth client.
