# Batch 101 (loop 1): AZ-3944 device approval page, roadmap section 7 item 2

| Task | Status | Files | Tests | Issues |
|------|--------|-------|-------|--------|
| AZ-3944 | PARTIAL: all ACs done except AC-1 (the platform is not shown, the server sends none). Spec stays in `todo/`, Jira stays To Do | `src/web/src/pages/DeviceApprovalPage.svelte`, `pages/deviceApproval/*`, `app/routes.ts` (`/device`), `lib/api/client.ts` (`getDeviceGrant`, `patchDeviceGrant`), `lib/api/index.ts` | 22 page tests, helper tests; web suite 546 passed, typecheck 0 errors, lint clean (re-run by the parent) | Discovered #1-#3 |

| AC | Result |
|---|---|
| AC-1 device and both locations | PARTIAL: device, both IP and location pairs, same or different location and the warning are shown; the platform is not |
| AC-2 mismatch: Deny primary, Approve off until the tick | DONE |
| AC-3 match: Approve direct | DONE |
| AC-4 one call per decision (double click) | DONE |
| AC-5 countdown from the server's expiry, expired page at 0 | DONE (a local display of the expiry; no request is made) |
| AC-6 wrong, used, unknown-location states | DONE; a decided code is indistinguishable from an unknown one (server answers 404) |
| AC-7 429 countdown, Continue disabled | DONE; with no `Retry-After` the page says "Try again later" and shows no number |
| AC-8 signed-out visitor goes through sign-in and returns | DONE |

## Code Review Verdict

Not run: pending the batch review of the loop; the parent re-ran typecheck, lint and the web suite. The Blackbox Playwright rows were NOT RUN (no acceptance stack with the new binary).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `DeviceGrantApproval` has `device` only: no platform (AC-1 says "name and platform") and no app version | AZ-3944 AC-1, flagged concern | 01 adds `platform` to the approval lookup, or the AC drops it | unclear (E-15 owner) |
| 2 | The server answers 404 for a decided code, so "already used or denied" looks like "wrong"; the page adds that sentence to the expired page | AZ-3944 AC-6 | accept, or 01 returns a distinct answer | unclear |
| 3 | The "unverified device name" marker (NFT-SEC-07) and the unknown-location and already-decided states are not drawn in the mockup | AZ-3944 flagged concerns | owner draws them or accepts the expired page's style | unclear |

## Commit

`[AZ-3944] Web: device approval page with code entry, both locations, countdown and one decision` — Loop: 1
