# Batch 100 (loop 1): AZ-3937 account page, roadmap section 7 item 2

| Task | Status | Files | Tests | Issues |
|------|--------|-------|-------|--------|
| AZ-3937 | PARTIAL: profile, storage, desktop devices with revoke, sign-out, provider linking start, account deletion work; the browser-sessions list, phone rows and the link completion are not built because the server does not provide what they need. Spec stays in `todo/`, Jira stays To Do | `src/web/src/pages/AccountPage.svelte`, `pages/account/*`, `app/routes.ts`, `pages/AppShell.svelte`, `lib/api/client.ts`, `lib/api/index.ts` | 16 page tests, 5 helper tests; web suite 515 passed, typecheck 0 errors, lint clean | Discovered #1-#4 |

| AC | Result |
|---|---|
| AC-1 profile and storage | PARTIAL: the email "Verified" mark and the provider's last sign-in are missing, `Account.identities` has only `{provider, email?}` |
| AC-2 devices and sessions list | PARTIAL: desktop devices only; phone sessions, browser sessions and the "This browser" chip NOT DONE |
| AC-3 revoke inline | DONE for devices; browser-session revoke NOT DONE |
| AC-4 sign-out | DONE |
| AC-5 provider linking | PARTIAL: start and redirect work; completion and `IdentityTaken` have no route or wire code |
| AC-6 account deletion | DONE for the page |
| AC-7 empty and loading states | DONE |

## Code Review Verdict

Not run: pending the batch review of the loop; the parent re-ran typecheck, lint and the web suite.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `GET /sessions` answers `{id, label, created_at, last_seen_at}`: no `client_kind`, no IP, no current-session marker, and no `GET /sessions/current`. A Revoke button on every row could end the user's own session (spec risk 1) | AZ-3937 AC-2, AC-3 | 01 and 11 add the three fields to the sessions list; then build the sessions section | unclear (E-15 owner) |
| 2 | `complete_link` has no HTTP route, `IdentityTaken` has no wire code, the link request has no `return_to` (open point 34) | AZ-3937 AC-5 | 01 and 11 decide the completion route and the error code | unclear (E-15, E-21 owners) |
| 3 | The deletion confirmation is the caller's own account id (contracts part 1 and `deletion_service.rs`); the page shows it with a copy button and enables Delete only when the typed text matches; the mockup has no state for it | AZ-3937 flagged concern | the owner confirms the wording | unclear |
| 4 | "Get the desktop app" and "Get the Android app" have no link targets | AZ-3937 flagged concern | text only until the owner names the targets | unclear |

## Commit

`[AZ-3937] Web: account page, profile, storage, desktop devices, sign-out, provider linking, deletion` — Loop: 1
