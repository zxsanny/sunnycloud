# Admin section: users, invites, quotas, health

**Task**: AZ-3946_admin_section
**Name**: Admin section
**Description**: The admin-only pages: user list with role and state changes and the last-admin refusal, single-use invites shown once, default and per-user quotas, and the aggregated health checks
**Complexity**: 5 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3950_playwright_web_harness, AZ-3817_admin_user_management, AZ-3818_last_admin_guard, AZ-3821_admin_health_aggregation, AZ-3688_setup_token_invites_operator_ops, AZ-3751_quota_admin_lifecycle, AZ-3828_identity_admin_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3946
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/contracts/07_admin/admin_service.md` — users, role and state, quota, health, produced by E-17.
- `_docs/02_document/contracts/07a_quota/quota-admin.md` — quota limits, produced by E-11.

## Problem

A self-hosted server needs a small admin area: who has access, invites for new people, storage limits and whether the server is healthy (16 §5). Admin pages must be hidden from non-admins and still guarded by the server, and the server's last-admin rule must be visible when it refuses (AC-23, FT-N-46).

## Outcome

- Admins reach Users, Invites, Quotas and Health tabs (`?tab=` opens one; mockup `ui_mockups/web-admin.html`, states "Users", "Last admin", "Invite created", "No invites", "Health degraded", "Loading").
- Every change is one server call; refusals are shown inline with nothing changed.
- Non-admins see no admin navigation and get "not found" on admin routes.

## Scope

### Included
- Users: paged list with name or email, role, status, storage used and limit, last sign-in; make admin or user, deactivate or reactivate; accounts in deletion shown as "Removing data…" with no actions; the last-admin refusal text.
- Invites: create with an optional note; URL shown once with copy, "Single use · expires <date>"; list with Pending, Used, Expired, Revoked; revoke pending ones; empty state.
- Quotas: default quota read-only with its configuration note (DESIGN §9 #5); per-user quota set in GB or TB; "Use default" clears it; invalid limits refused inline.
- Health: readiness banner (ok, degraded, down) naming the failing check, checks table (name, status, detail), "Check again".

### Excluded
- Deleting another user's account (no API, G-AS-1; see Flagged concerns); billing and organisations (out of scope).

## Acceptance Criteria

**AC-1: Visible to admins only (AC-23, IT-16-17)**
Given an admin session and a plain user session
When each opens the app and navigates directly to each admin tab
Then the admin sees the admin group and all four tabs; the user sees no admin navigation, each admin route shows "not found", and the API answers 403 for the user's admin calls

**AC-2: Role and state changes**
Given a list with an active user and a deactivated user
When the admin makes the first an admin, deactivates and reactivates the second
Then each action is one call and the row shows the server's new role or state; an account in deletion shows "Removing data…" and offers no actions

**AC-3: Last-admin refusal (FT-N-46 step 2)**
Given the admin is the last active admin
When it demotes or deactivates itself in the admin section
Then the server refusal text is shown inline ("…the last active admin…") and role and state are unchanged

**AC-4: Invites (AC-23, FT-P-65 web half)**
Given the Invites tab
When the admin creates an invite with note "Lesia", copies it, and revokes an older pending invite
Then the URL is shown once with a copy action that copies exactly it, the list shows the new invite as Pending with its expiry, the revoked one as Revoked, used and expired ones with their status, and the URL is not shown again after the panel closes; with no invites the empty state shows

**AC-5: Quotas (07a)**
Given the Quotas tab
When the admin sets 250 GB for a user, then chooses "Use default", and enters an invalid limit
Then the first is saved and listed, the second clears the user's quota so the default applies, and the invalid limit is refused inline with nothing saved; the default quota is shown read-only with its configuration note

**AC-6: Health (07 `health`)**
Given one degraded check (blob volume below the free-space floor)
When the Health tab opens and "Check again" is chosen
Then the banner says degraded and names the check, the checks table lists every reported check with status and detail, and "Check again" re-reads the health operation

**AC-7: No delete of other users (G-AS-1)**
Given the user menu of another account
When it opens
Then it offers no "Delete account…" action while the API has no such operation

## Non-Functional Requirements

**Reliability**
- A failing tab shows its error with the request id and leaves the other tabs usable.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Navigation by role | Admin group only for the admin role |
| AC-5 | Quota input in GB and TB to bytes; clear to default | Exact byte values; null for default |
| AC-6 | Health items to banner state | Worst status wins; failing check named |

## Blackbox Tests

Playwright against the real stack, group X for last-admin cases (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Admin and user sessions | IT-16-17 (visibility) | Hidden for user; 403 from the API | AC-23, NFT-SEC-13 |
| AC-2, AC-3 | Fresh stack, one admin | FT-N-46 step 2 | Changes applied; last-admin refused inline | AC-23 |
| AC-4 | Invites | IT-16-17 (invites); FT-P-65 web half | Shown once; statuses; revoke | AC-23 |
| AC-5 | Users with and without own quota | Quotas tab | Set, clear, refuse | AC-39 |
| AC-6 | Blob volume below floor (`blobs-small`) | IT-16-17 (health) | Degraded banner; checks listed | |
| AC-7 | Another user's menu | Menu items | No delete action | |

## Constraints

- Admin routes are guarded by the server; the hidden navigation is convenience only (16 §11).

## Risks & Mitigation

**Risk 1: Client-side guard mistaken for security**
- *Risk*: a page trusts the hidden menu.
- *Mitigation*: AC-1 checks the API's 403 for a user session on every admin call.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-AS-1: the mockup and 16 §5 list "Delete account…" for another user, but 07 offers only `PATCH {role, state}` (`admin_service.md` Non-Goals); AC-7 offers no delete until the owner adds the operation or drops the item | Product owner | open | Medium |
| The invite URL format is not specified (DESIGN §9 #6); AC-4 copies whatever URL the create answer defines | E-15 and E-17 owners | open | Medium |
| Health figures drawn as "Storage free", "Queue depth" and "Workers" are not fixed fields; the health operation returns a list of checks with detail text (DESIGN §9 #4) | E-17 owner | open | Low |
| The mockup has "Search users", but `list_users` takes only a page; search over loaded rows only, or drop it | E-17 owner | open | Low |
| The `InvalidLimit` code and status are open (`admin_service.md`) | E-11, E-17 owners | open | Low |
