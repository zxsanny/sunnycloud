# Account page: profile, storage, devices and sessions, sign-out and account deletion

**Task**: AZ-3937_account_page
**Name**: Account page
**Description**: The signed-in account page: profile with linked providers and provider linking, storage use, one list of desktop devices and phone sessions, browser sessions with revoke, sign-out and the account-deletion request
**Complexity**: 3 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3950_playwright_web_harness, AZ-3686_session_refresh_rotation, AZ-3687_device_grant_exact_once, AZ-3694_provider_linking_and_list, AZ-3818_last_admin_guard, AZ-3819_account_view_deletion_request, AZ-3828_identity_admin_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3937
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c07b

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.

## Problem

Users need one place to see who they are signed in as, how much storage they use, which apps and browsers hold access, and to end that access (AC-21). They also need to sign out, link a second provider (never by email, AC-55) and request deletion of their account, which the server refuses for the last active admin (FT-N-46).

Split note: second half of epic row 7 (see E-26/c07a for the reason).

## Outcome

- The page shows profile, storage, devices and sessions from the server's views (mockup `ui_mockups/web-account.html`, states "Account", "Revoke confirm", "No devices", "Loading").
- Every revoke, sign-out, link and deletion is one server call and the page shows the server's answer.

## Scope

### Included
- Profile: name, email with its verified mark, linked providers with last sign-in, "Link another provider", role.
- Storage: used, reserved for uploads in progress, quota and its "server default" mark, from the quota view.
- Devices: desktop devices and phone sessions (`client_kind` android or ios) in one list (DESIGN §9 #3) with name, platform, added and last seen; revoke with an inline confirmation; empty state.
- Browser sessions: list with the "This browser" chip, IP, signed-in date and last seen; revoke for other sessions.
- Sign out (ends this browser's session) and "Delete account…" with the server's confirmation rule and the 15-minute removal text.

### Excluded
- Admin actions on other users (E-26/c16); quota changes (admin only).

## Acceptance Criteria

**AC-1: Profile and storage match the server**
Given an account with GitHub linked and the default quota of 100 GB
When the account page loads
Then name, email with verified mark, provider with last sign-in, role, used, reserved and the quota with "server default" equal the account view and the quota view

**AC-2: Devices and sessions listed**
Given 2 desktop devices, 1 Android session and 3 browser sessions
When the page loads
Then the devices list shows 3 rows (2 desktop, 1 phone) with name, platform, added and last seen, and the sessions list shows 3 rows with the current one marked "This browser" and offering no revoke

**AC-3: Revoke with inline confirmation (AC-21)**
Given a desktop device row
When the user chooses Revoke and then Cancel, and again Revoke and confirms
Then Cancel makes no call; the confirmation makes exactly one revoke call, the row leaves the list, and a failed revoke shows the server's error in the row with the row kept

**AC-4: Sign out**
Given a signed-in browser
When Sign out is chosen
Then this browser's session is revoked, the sign-in page shows, and going back shows no account data (the next call answers 401 and leads to sign-in)

**AC-5: Link another provider (AC-55)**
Given an account with GitHub only
When the user links OIDC, and in a second run picks an identity already owned by another account
Then after the provider step both providers are listed; the second run shows the `IdentityTaken` refusal and links nothing

**AC-6: Account deletion (FT-N-46 step 2)**
Given a member account, and the last active admin
When each requests deletion with the confirmation
Then the member's request is accepted, the page shows the deadline text and signs the user out; the admin gets the server refusal inline and nothing changes

**AC-7: Empty and loading states**
Given an account with no apps, and a slow account view
When the page loads
Then the devices section shows the empty state with its app pointers, and while loading the sections show skeleton rows

## Non-Functional Requirements

**Reliability**
- A failing section (for example the sessions list) shows its error with the request id and does not blank the other sections.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Merging device and phone-session rows | One list, phones marked by platform, no duplicates |
| AC-1 | Quota line with an own quota versus the default | "server default" only for the default |

## Blackbox Tests

Playwright against the real stack (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Seeded devices and sessions | Page content | Matches server views | |
| AC-3 | One device | Revoke, cancel and confirm | One call; row removed | AC-21, NFT-SEC-08 |
| AC-4 | Signed in | Sign out | Session revoked; sign-in shown | AC-21 |
| AC-5 | mock-idp identities | Link and taken identity | Linked; `IdentityTaken` shown | AC-55, FT-N-22 |
| AC-6 | Member; last admin | Deletion request | Accepted; refusal inline | FT-N-46 |

## Constraints

- Linking never uses an email address (01 email rule); the page only starts the link attempt.

## Risks & Mitigation

**Risk 1: Self-revoke from the list**
- *Risk*: revoking the current session from the list would sign the user out unexpectedly.
- *Mitigation*: the current session row has no revoke; Sign out is the only way (AC-2).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The deletion confirmation value is not specified (07 `ConfirmationMismatch`) | E-17 owner | open | Low |
| Device and session rows have no app version (DESIGN §9 #3); rows show name, platform, added and last seen | E-15 owner | accepted-risk | Low |
| Where "Get the desktop app" and "Get the Android app" point is not specified | Owner | open | Low |
