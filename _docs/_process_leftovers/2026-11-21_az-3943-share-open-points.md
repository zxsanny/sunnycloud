# AZ-3943 share dialog and public page: open points

kind: server gap (blocks one AC) and owner decisions
recorded: 2026-11-21

## Server gaps (the web side is done and waits for them)
1. **Password links never open.** `GET /api/v1/public-shares/{token}` ignores the grant cookie: in component 11 `visitor::open` passes only the token and client IP to 06 `open`, so a password link answers 401 `password_required` after a correct password. Only the content routes read the grant. Effect: no name, size, type or expiry after unlock, and `transfer.download({shareToken})` fails for password links. Fix: 06 `open` takes `Option<AccessGrant>`, 11 passes `visitor.grant`. The e2e test "AC-8 (after unlock)" is marked `test.fail()`; Playwright fails the suite once the server is fixed, which is the signal to remove `test.fail`.
2. **No default or maximum share lifetime** is returned by any operation (spec flagged concern). The dialog sends no `expires_in` for "Server default" and offers whole days with no client cap; AC-2 (preselect the real default, nothing above the maximum) needs `share_default_ttl` and `share_max_ttl` in `server-info`. `ApiError` also drops the 400 body's `problems[].field`.
3. Public-link download is always `SavedUnverified{NoServerHash}` (no hash on `ShareOpen`), see the AZ-3942 note.

## Decisions for the owner
- Reload of the public page after the token left the address opens "This link isn't available", and a transient 5xx on first load looks the same (spec AC-7 wants one page for every failure; recovery needs the token). Keep, or add a non-identical retry.
- A path-scoped `Referrer-Policy: no-referrer` for `/s/*` in Caddy (and `tests/e2e/server/web-server.ts`) would close the referrer leak of the pre-mount same-origin requests; the page's own meta covers its API calls only.
- A 429 also shows the app-level "Too many requests" notice next to the countdown (`handleApiError`).
- The folder-row reason is a button tooltip until the row menu (E-26/c08) exists. The transfers panel covers row buttons until "Clear finished".

## Not run
AC-12 / PT-16-5 (100 opens over `shape-wan`: needs `tc` and root), AC-11 in Firefox and WebKit (only Chromium installed), real H.264 playback (the e2e uses VP9 in MP4), password-link download (server gap 1).
