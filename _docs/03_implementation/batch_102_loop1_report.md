# Batch 102 (loop 1): AZ-3946 admin section, roadmap section 7 item 2

| Task | Status | Files | Tests | Issues |
|------|--------|-------|-------|--------|
| AZ-3946 | PARTIAL: Users, Invites, Quotas and Health tabs work from the API that exists; the user name and email, the Revoked invite status, the invite URL and the default quota value cannot be shown because the server does not send them. Spec stays in `todo/`, Jira stays To Do | `src/web/src/pages/AdminPage.svelte`, `pages/admin/*`, `app/routes.ts` (`/admin`), `pages/AppShell.svelte` (admin link for role admin), `lib/api/client.ts`, `lib/api/index.ts` | 583 web tests passed (re-run by the parent), typecheck 0 errors, lint clean | Discovered #1-#5 |

| AC | Result |
|---|---|
| AC-1 admin navigation and not-found for non-admins | DONE (the 403 of the API is a server check, not covered here) |
| AC-2 role and state changes, "Removing data…" | DONE |
| AC-3 last-admin refusal inline | DONE |
| AC-4 invites | PARTIAL: create (token once, copy), list with Pending, Used, Expired, revoke pending, empty state; Revoked and the URL are not shown |
| AC-5 quotas | PARTIAL: set in GB or TB, "Use default" clears, invalid refused inline; no default value shown |
| AC-6 health | DONE |
| AC-7 no delete of another user | DONE (no control drawn; G-AS-1 stays open) |

## Code Review Verdict

Not run: pending the batch review of the loop; the parent re-ran typecheck, lint and the web suite. The Blackbox Playwright rows were NOT RUN.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `GET /admin/users` returns no name or email (`id, role, state, last_login, used, limit`); users and the quota user picker show the account id, "(you)" by session id | AZ-3946 AC-2 | 07 adds display name and email to the user list | unclear (E-17 owner) |
| 2 | The invite list has no revoked marker; if the server keeps revoked invites they would show as Pending after a reload | AZ-3946 AC-4 | 07 adds the status or drops revoked invites from the list; the owner confirms which | unclear |
| 3 | The create-invite answer is `{token}` only: no URL, no expiry; the panel shows and copies the token, the expiry date comes from the refreshed list | AZ-3946 AC-4, flagged concern | owner defines the invite URL (DESIGN §9 #6) | unclear |
| 4 | No operation returns the installation default quota value; the tab shows the note naming `default_quota_bytes` only | AZ-3946 AC-5 | 07 exposes the default | unclear |
| 5 | Paging is "Show more" at 100 per page; the user and admin counts cover the loaded rows only | AZ-3946 AC-2 | accept, or the list answer carries a total | clear |

## Commit

`[AZ-3946] Web: admin section with users, invites, quotas and health` — Loop: 1
