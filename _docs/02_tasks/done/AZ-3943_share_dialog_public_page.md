# Share dialog and public share page `/s/<token>`

**Task**: AZ-3943_share_dialog_public_page
**Name**: Share dialog and public page
**Description**: The owner's share dialog for files (create with expiry and optional password, link shown once, list and revoke per file) and the anonymous public page that previews or downloads, asks for a password, reveals nothing before unlock and shows one "not available" page for every failure
**Complexity**: 5 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3938_file_browser, AZ-3942_download_strategies, AZ-3947_render_mode_hostile_content, AZ-3950_playwright_web_harness, AZ-3697_share_create_list_revoke, AZ-3698_share_lookup_open_uniform, AZ-3699_share_password_access_grant, AZ-3701_share_abuse_limits, AZ-3830_share_media_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3943
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — `download` with a share token, produced by E-26/c09.
- `_docs/02_document/contracts/11_http_api/content_serving_headers.md` — inline and attachment answers, produced by E-21/c10.

## Problem

Sharing is by link only in v1.0 and for files only (06, decision 15). The token exists in plaintext only in the create answer, so the link must be shown once (FT-P-80). Visitors must not learn why a link fails (AC-12), must learn nothing about a password-protected file before unlock (NFT-SEC-51), and the token must not linger in history or referrers.

## Outcome

- Owners create, copy, list and revoke links per file in the dialog (mockup `ui_mockups/web-share-dialog.html`, states "Create link", "Link created", "Revoke", "Folder").
- Visitors see an inline preview for allowlisted types, a download for others, a password prompt, a rate-limit countdown, or one identical "This link isn't available" page (mockup `ui_mockups/web-public-share.html`, states "Image", "Video", "Download only", "Password", "Wrong password", "Not available", "Rate limited").

## Scope

### Included
- Dialog from the file row menu: expiry choice, optional password with show/hide and the "Send the password separately" helper, create, the one-time URL panel with copy and "Link copied", the per-file list (created, expiry, password flag) with inline revoke confirmation, the "Links follow the file" note.
- Folder rows: "Share link…" disabled with the reason "Folder sharing is not available in this version"; a server `TargetNotShareable` shown as such.
- Public page: open; available → name, size, type, "Available until", preview by `renderMode` (E-26/c17) or "Preview isn't available…" plus Download through `TransferService.download`; `password_required` → prompt without file fields; wrong password message; 429 countdown with Unlock disabled; not found → one page.
- Token hygiene: the token is removed from the address and history after load and never logged.
- PT-16-5 open-time measurement.

### Excluded
- Folder links (not in v1.0), vault links (v1.2: not shareable), an account-wide links page (see Flagged concerns).

## Acceptance Criteria

**AC-1: Link shown once (AC-12, FT-P-80 steps 1-2)**
Given a file f.jpg
When the owner creates a link with the default expiry, copies it, closes and reopens the dialog
Then the dialog showed exactly one URL with the token, the clipboard holds exactly that URL, the URL opens f.jpg with an equal hash, and after reopening the list shows created date, expiry and password flag while the DOM and the list answer contain the token 0 times

**AC-2: Expiry within the server's limits**
Given the server's default and maximum link lifetime
When the dialog opens and the owner picks an expiry
Then the default is preselected, no choice exceeds the maximum, and a server `ExpiryOutOfRange` is shown next to the field with nothing created

**AC-3: Folders cannot be shared (FT-P-80 steps 3-4)**
Given a folder row
When its menu opens, and create-share is called for it through the API with the owner's session
Then "Share link…" is disabled with its reason visible and opens no dialog; the API answers `TargetNotShareable` and the owner's links are unchanged

**AC-4: Revoke inline (FT-P-80 step 5, AT-16-3)**
Given a link in the list
When the owner chooses Revoke and confirms in the row (no second dialog)
Then one revoke call is made, the row leaves the list, and within 5 s the link opens the "This link isn't available" page

**AC-5: Links follow the file (AC-77, IT-16-16 share half, FT-N-44)**
Given a shared file
When it is renamed and moved by another client
Then its dialog still lists the link and the link still opens the file

**AC-6: Preview by type (AC-47, IT-16-10)**
Given links to a JPEG, an MP4 and a PDF
When each is opened anonymously
Then the JPEG shows inline, the MP4 plays inline, the PDF shows "Preview isn't available for this file type…" with a Download button, and Download saves a file whose hash equals the original

**AC-7: Failures look identical (AC-12, FT-N-09, IT-16-10)**
Given an expired (test clock), a revoked and a nonexistent token
When each is opened
Then all three render byte-identical DOM text ("This link isn't available") with the same HTTP status

**AC-8: Nothing revealed before unlock (NFT-SEC-51, FT-N-10)**
Given a password link to `secret-plan.pdf` (5 MB)
When the page opens without a password, a wrong password is entered, then the right one
Then before unlock the DOM contains 0 occurrences of the name, size, media type or expiry; the wrong password adds only "That password isn't right…"; the right one shows name, size and type and the download hash equals the file

**AC-9: Rate limit countdown (AC-20, FT-P-79 step 2)**
Given 11 wrong passwords within 10 minutes
When the server answers 429
Then the page shows "Too many attempts. Try again in N seconds" with N within 1 s of `Retry-After`, and Unlock stays disabled with its reason until the countdown ends

**AC-10: Token hygiene (ST-16-4 token half)**
Given a share link opened in the browser
When history, the console and an outgoing link's referrer are inspected
Then after load the address and history entries no longer contain the token, and the token appears in 0 console lines and 0 referrers

**AC-11: Hostile files and names via a link (IT-16-2 and ST-16-1 share halves, ST-16-2 share half)**
Given hostile uploads (DS-17) and hostile names shared by link, and a share password entered
When they are opened anonymously in Chromium, Firefox and WebKit and storage is dumped
Then non-allowlisted types get a download only with 0 script executions, names render as text, and the share password is in no browser storage

**AC-12: Open time (AC-12, PT-16-5)**
Given a link to a 1 MiB image over `shape-wan`
When it is opened 100 times
Then page ready p95 is at most 1 s (failure above 2 s)

## Non-Functional Requirements

**Performance**
- PT-16-5: p95 at most 1 s, failure above 2 s.

**Reliability**
- Revocation shows within 5 s (AC-12; server side in 06).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Expiry choices against a default and a maximum | Default preselected; nothing above the maximum |
| AC-7 | One renderer for the not-found outcome | Same output for all three causes |
| AC-10 | Token removal from address and history after load | No token left |

## Blackbox Tests

Playwright against the real stack with the test clock, group X (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-3, AC-4 | JPEG and folder | FT-P-80 | Token once; folder refused; revoke within 5 s | AC-12 |
| AC-5 | Second client renames and moves | IT-16-16 (share half); FT-N-44 | Link listed and working | AC-77 |
| AC-6, AC-7 | JPEG, MP4, PDF; expired, revoked, unknown | IT-16-10; FT-N-09; FT-P-36 (bytes equal) | Previews; identical failure page | AC-12, AC-47 |
| AC-8 | Password link | NFT-SEC-51; FT-N-10; FT-P-37 (password half) | 0 metadata before unlock | AC-12 |
| AC-9 | 11 wrong passwords | FT-P-79 step 2 | Countdown; Unlock disabled | AC-20 |
| AC-10 | Opened link | ST-16-4 (token half) | Token gone from history, console, referrer | AC-12 |
| AC-11 | DS-17 via link; share password | IT-16-2, ST-16-1, ST-16-2 (share halves) | Download only; text names; no stored password | AC-47 |
| AC-12 | 1 MiB image, `shape-wan` | PT-16-5; FT-P-36 (web half) | p95 <= 1 s | AC-12 |

## Constraints

- The page never distinguishes failure reasons, because the server does not (16 §5).
- The share password is sent only in the verification request body and never stored.

## Risks & Mitigation

**Risk 1: Reload after the token left the address**
- *Risk*: removing the token from history means a reload no longer has it.
- *Mitigation*: the page keeps the token in memory for its life; the reload behaviour is a flagged owner item.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 16 §5 and DESIGN §9 #8 say the default expiry and the maximum lifetime are read from the API, but no operation returns `share_default_ttl` or `share_max_ttl` (`server_info.md` has neither; 06 applies the default only when `expires_in` is omitted). AC-2 cannot show the real values until an operation exposes them | E-16 and E-21 owners | open | Medium |
| The app shell shows a "Shared links" navigation item, but no screen or spec defines an account-wide links page (06 `list` without a node exists) | Owner | open | Low |
| What a reload of the public page does after the token was removed from the address is not specified | E-26 owner | open | Low |
