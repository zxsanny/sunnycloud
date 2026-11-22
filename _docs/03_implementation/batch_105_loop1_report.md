# Batch 105 (loop 1): the web pages use the server fields of batches 103 and 104

| Task | Result |
|------|--------|
| AZ-3944 | COMPLETE at unit level: AC-1 done (the device card shows "Desktop app · Linux/Windows/macOS" from `platform`); all eight ACs have unit tests. The Blackbox Playwright rows are NOT RUN. No app version is shown: the server does not send one (open Low concern) |
| AZ-3937 | PARTIAL: AC-2, AC-3, AC-5 done (one Devices list with desktop devices and phone sessions; "Browser sessions" with a "This browser" chip, the address when recorded, no Revoke on the current session; the link outcome notice from `/account?linked=1` and `?error=`, the query removed afterwards). AC-1 still lacks the email "Verified" mark and the provider's last sign-in: `Account.identities` has only `{provider, email?}` |
| AZ-3946 | PARTIAL: AC-2 done (display name, id and "(you)" secondary), AC-5 done (default quota value), AC-4 shows the invite URL once and the expiry from the create answer; "Revoked" cannot be shown (revoke is a hard delete, owner decision 2026-11-22) |
| AZ-3943 | AC-2 done: the expiry is a select (1, 7, 30 days, the default and the maximum, nothing above `share_max_ttl`), the default preselected, from `server-info`; with no limits only "Server default" is offered |

Files: `src/web/src/pages/account/*`, `AccountPage*.svelte/ts`, `pages/deviceApproval/*`, `pages/admin/*`, `AdminPage*.test.ts`, `pages/files/share/*`, `ShareDialog*`, `lib/shares/*`, `lib/api/client.ts` (`getSessions`, `deleteSession`), `lib/api/index.ts`, `lib/api/limits.ts` (`shareDefaultTtl`, `shareMaxTtl`). `parseExpiryDays` and its tests were removed as dead code.

Verification (re-run by the parent): `bun run typecheck` 0 errors, `bun run lint` clean, `bun run test` 676 passed in 51 files.

## Code Review Verdict

Not run: pending the batch review of the loop. The Blackbox Playwright rows of the four specs were NOT RUN (no acceptance stack with the new binary).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `Account.identities` carries no verified flag and no last sign-in time | AZ-3937 AC-1 | 01 adds them to the account view, or the AC drops them | unclear (E-15 owner) |
| 2 | Revoked invites cannot be shown (hard delete) | AZ-3946 AC-4 | owner decision 2026-11-22: keep the hard delete; the AC drops "Revoked" | clear |
| 3 | `ApiError` drops the 400 body's `problems[].field`, so a field error is placed by what the form sent | api_client.md | carry `problems` in `ApiError` | unclear |
| 4 | AZ-3943 server gaps found by the other session: password links never open, no hash on `ShareOpen` | AZ-3943 | see that spec's flagged concerns and leftover | unclear |

## Commit

`[AZ-3937] [AZ-3944] [AZ-3946] [AZ-3943] Web: account sessions, device platform, admin names, invite URL, default quota and share expiry limits` — Loop: 1
