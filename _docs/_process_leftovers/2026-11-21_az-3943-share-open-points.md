# AZ-3943 share dialog and public page: open points

kind: server gap (blocks one AC) and owner decisions
recorded: 2026-11-21

## Server gaps
1. **Password links never open.** Fixed 2026-11-22: 06 `open` takes `Option<&AccessGrant>`, 11 `visitor::open` passes `visitor.grant` (contracts part 3 1.6.0, api_v1_surface 1.12.1); the e2e test "AC-8 (after unlock)" no longer carries `test.fail`.
2. **Share lifetime limits.** Fixed 2026-11-22 by the server-info `share_default_ttl` and `share_max_ttl` (c6b099b) and the web dialog that reads them (49e87c6). `ApiError` still drops the 400 body's `problems[].field`.
3. Public-link download was always `SavedUnverified{NoServerHash}`. Fixed 2026-11-23 (AZ-4350): `ShareOpen::Available` and the open answer carry `content_hash` (contracts part 3 1.7.0, api_v1_surface 1.14.0); the web share download verifies, e2e 15 of 15 in `web-share.spec.ts`.

## Decisions for the owner
Decided 2026-11-22: (1) keep the one identical "isn't available" page; (2) add `Referrer-Policy: no-referrer` for `/s` and `/s/*` (done: `deploy/compose/Caddyfile.mvp`, `tests/e2e/server/web-server.ts`); (3) hide the general 429 notice on the share page (done: `handleApiError.ts`); (4) wait for the row menu (E-26/c08) for the tooltip and the transfers-panel overlap.

- Reload of the public page after the token left the address opens "This link isn't available", and a transient 5xx on first load looks the same (spec AC-7 wants one page for every failure; recovery needs the token). Keep, or add a non-identical retry.
- A path-scoped `Referrer-Policy: no-referrer` for `/s/*` in Caddy (and `tests/e2e/server/web-server.ts`) would close the referrer leak of the pre-mount same-origin requests; the page's own meta covers its API calls only.
- A 429 also shows the app-level "Too many requests" notice next to the countdown (`handleApiError`).
- The folder-row reason is a button tooltip until the row menu (E-26/c08) exists. The transfers panel covers row buttons until "Clear finished".

## Not run
AC-12 / PT-16-5 (100 opens over `shape-wan`: needs `tc` and root), AC-11 in Firefox and WebKit (only Chromium installed), real H.264 playback (the e2e uses VP9 in MP4).
