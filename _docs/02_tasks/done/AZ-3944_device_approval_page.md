# Device approval page

**Task**: AZ-3944_device_approval_page
**Name**: Device approval page
**Description**: The page where a signed-in user enters a desktop app's user code, sees the requester's and their own IP and location side by side, and approves or denies once, with deny as the default when the locations differ
**Complexity**: 3 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3950_playwright_web_harness, AZ-3687_device_grant_exact_once, AZ-3691_identity_abuse_limits_audit, AZ-3828_identity_admin_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3944
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `DeviceGrantService` and `ApprovalView`, produced by E-00/c03.

## Problem

The desktop app signs in with a device grant (RFC 8628). A phishing attacker can send a victim a user code from another place; the page is the only defence the user sees (AC-57, F3). The owner approved on 2026-11-04 that Deny is the primary action on a location mismatch and Approve needs an explicit confirmation (FT-P-81).

## Outcome

- The page shows device, both locations, a mismatch warning and a countdown (mockup `ui_mockups/web-device-approval.html`, states "Enter code", "Same location", "Different location", "Approved", "Denied", "Expired").
- On a mismatch, approving needs the tick "I started this sign-in on that device"; on a match, Approve is direct.
- Approve and deny each call the server exactly once; wrong, expired or used codes grant nothing.

## Scope

### Included
- Code entry (mono input) and lookup, which returns the grant id and the code's expiry (owner decision 2026-11-05); device name and platform; requester IP and location next to the approver's ("You"); "Same location" or "Different location" with the warning text.
- Actions: match → Approve primary and Deny; mismatch → "Deny request" primary, Approve disabled with "Approve stays off until you tick this box." until the confirmation is ticked.
- Countdown "Code expires in m:ss"; approved, denied and expired or invalid pages; "What this device can do" text.
- Unknown location shown as such, with the warning when the IPs differ (01 `GeoLookup`); an already decided code shown as no longer valid; wrong-code limit with the 429 countdown.
- Signed-out visitors go through sign-in and return to this page.

### Excluded
- The daemon side (E-24) and grant logic (E-15/c07).

## Acceptance Criteria

**AC-1: Both locations side by side (AC-57, IT-16-11, FT-P-66, NFT-SEC-07 step 1)**
Given a device grant requested from subnet X and the approver on subnet Y (DS-32 GeoIP)
When the approver enters the user code
Then the page shows device name and platform, both IP and location pairs side by side and the mismatch warning; for a grant from the approver's own subnet it shows "Same location" and no warning

**AC-2: Deny is the default on a mismatch (FT-P-81 steps 1-3)**
Given the mismatch case
When the page shows the decision
Then "Deny request" is the primary action and Approve is disabled with its reason; ticking the confirmation enables Approve and unticking disables it again; choosing Deny shows the denied page and the daemon's next poll gets `access_denied`

**AC-3: Approve on a match (FT-P-81 steps 4-5, AT-16-4)**
Given the match case
When the approver chooses Approve without any confirmation
Then the approved page shows "<device> is connected" and the daemon's next poll returns a device token

**AC-4: One call per decision**
Given either case
When the approver double-clicks Approve or Deny
Then exactly one decision call is sent, carrying the grant id the lookup returned

**AC-5: Countdown and expiry (AC-57, FT-P-66 step 3)**
Given a grant with a 10-minute code
When the page stays open past the expiry (test clock)
Then the countdown, taken from the expiry the lookup returned, reaches 0, the decision controls are disabled and the expired page shows; an approval attempted after expiry grants nothing

**AC-6: Wrong, used and unknown codes (FT-N-24, FT-N-45 step 3)**
Given a wrong code, a code already denied, and a code from a grant whose location is unknown
When each is entered
Then the wrong code shows "This code has expired or isn't valid" and grants nothing; the denied code shows it can't be used again; the unknown location shows as unknown with the warning when the IPs differ

**AC-7: Wrong-code limit (AC-20, NFT-SEC-05)**
Given 5 wrong codes from one approving session
When the next code is entered
Then the page shows the 429 countdown and Continue stays disabled until it ends

**AC-8: Session only (AC-57)**
Given no browser session
When the page is opened
Then sign-in is shown first and the approval page follows after sign-in; no decision is possible without a cookie session

## Non-Functional Requirements

**Reliability**
- The countdown follows the server's expiry returned by the lookup, not a client-side 10-minute timer.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Action state for match, mismatch, ticked and unticked | Approve enabled only for match or ticked mismatch |
| AC-4 | Decision guard under repeated clicks | One call |

## Blackbox Tests

Playwright as approver from subnet Y; an API client in the daemon role from subnet X (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Grants from X and from Y | IT-16-11; FT-P-66 steps 1-2; NFT-SEC-07 steps 1-2 | Both pairs; warning only on mismatch | AC-57 |
| AC-2, AC-3 | Mismatch then match | FT-P-81 | Deny primary; confirmation needed; tokens 0 and 1 | AC-57 |
| AC-4 | Double click | One call | One decision | |
| AC-5 | Test clock past 10 min | FT-P-66 step 3 | Expired; nothing granted | AC-57 |
| AC-6 | Wrong, denied, unknown-location codes | FT-N-24; FT-N-45 step 3 | Messages; nothing granted | AC-57, AC-76 |
| AC-7 | 6 wrong codes | NFT-SEC-05 | Countdown | AC-20 |
| AC-8 | Signed out | Sign-in first | No decision without a session | AC-57 |

## Constraints

- The page uses only the session-only approval operations; device tokens get 403 there (AC-57).

## Risks & Mitigation

**Risk 1: Users approve out of habit**
- *Risk*: a user ticks the confirmation without reading.
- *Mitigation*: the warning text names the other place, and Deny stays the primary action (FT-P-81).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `ApprovalView` (01 §2, contracts part 1) has no grant id and no expiry, but `decide` takes the grant id and the page shows a countdown; AC-2, AC-3 and AC-5 cannot be built until both fields are added | E-15/c07, E-00/c03 | resolved | High |
| Resolved by owner decision 2026-11-05: the approval lookup returns the grant id (used by Approve and Deny) and the code's expiry (drives the countdown) | Owner | resolved | High |
| NFT-SEC-07 step 1 wants the device name marked as unverified; the approved mockup shows no such marker | Owner, E-31 | open | Low |
| The mockup shows the app version ("sunnycloud desktop 1.0.3"); `ApprovalView` carries no version | E-15 owner | open | Low |
| The already-decided and unknown-location states are not drawn (DESIGN §9 #10); the texts here follow the "Expired" state's style | Owner | open | Low |
