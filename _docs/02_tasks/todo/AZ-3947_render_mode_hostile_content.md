# `renderMode`, hostile-content rendering and no-secrets-in-storage checks

**Task**: AZ-3947_render_mode_hostile_content
**Name**: renderMode and hostile-content checks
**Description**: The pure `renderMode` rule (inline only for seven media types, decided by the server's type), the file browser's open action that follows it, and the cross-page checks that hostile names never run as markup and no secret lands in browser storage
**Complexity**: 2 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3936_sign_in_setup_pages, AZ-3938_file_browser, AZ-3939_transfer_service_small_path, AZ-3941_resume_edit_detection, AZ-3950_playwright_web_harness
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3947
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c17

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — `download` used by the open action's download offer, produced by E-26/c09.

## Problem

Users upload anything, including HTML, SVG with script and polyglots. The server serves non-allowlisted types as attachments with `nosniff` and a sandbox CSP (AC-47); the page must never put them in an image, video, frame or object either (defence in depth), and must decide by the server's media type, never by the file name. File names and server texts are untrusted, and no secret may be written to browser storage (ST-16-2).

## Outcome

- `renderMode(mediaType)` returns `inline` only for `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/avif`, `video/mp4`, `video/webm` (case and parameters normalised); everything else, including an empty string, is `attachment`.
- Opening a file from the file browser shows allowlisted types inline and offers a download for all others.
- Hostile names and texts render as text across the signed-in pages; storage holds no secret.

## Scope

### Included
- The `renderMode` function (used also by the public share page, E-26/c13).
- The file browser's open action: inline view for `inline`, a download offer through `TransferService.download` for `attachment`.
- Checks: IT-16-1; IT-16-2 file-browser half; ST-16-1 for the listing, transfers panel and app notices; ST-16-2 after sign-in, upload and resume.

### Excluded
- Share-link halves of IT-16-2, ST-16-1 and ST-16-2 (E-26/c13); the `{@html}` lint rule itself (E-26/c06).

## Acceptance Criteria

**AC-1: The allowlist rule (AC-47, IT-16-1)**
Given the media types `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/avif`, `video/mp4`, `video/webm`, `IMAGE/PNG`, `image/png; charset=binary`, `image/svg+xml`, `text/html`, `application/xml`, `text/javascript`, `application/pdf`, `application/octet-stream` and the empty string
When `renderMode` is applied
Then the seven types and the uppercase and parameter variants give `inline`, and every other value gives `attachment`

**AC-2: Hostile files never render inline (AC-47, IT-16-2 file-browser half, NFT-SEC-19)**
Given the hostile uploads of DS-17 and controls jpg, png, webp, gif and mp4
When each is opened from the file browser in Chromium, Firefox and WebKit
Then html, svg with script, xml, js, pdf and the GIF89a/HTML polyglot get a download offer only, a DOM observer sees no image, video, frame, object or `srcdoc` created for them, the script sentinel stays unset, and the controls show inline

**AC-3: The server's type decides (G-10 narrowed)**
Given a file named `x.png` that the server serves as `text/html`
When it is opened
Then it gets a download offer only

**AC-4: Untrusted text stays text (ST-16-1)**
Given file names and server error texts containing `<img src=x onerror=…>`, `"><script>`, bidi override characters and 255-character strings
When they appear in the listing, the transfers panel and the app notices
Then they render as plain text with 0 script executions, and the source has 0 `{@html}` uses (lint)

**AC-5: No secrets in storage (ST-16-2)**
Given sentinels for the session value and a device user code
When the user signs in, uploads and resumes a file, and `localStorage`, `sessionStorage`, IndexedDB and Cache Storage are dumped
Then 0 sentinels are found, and with storage blocked the same run works with no uncaught storage exception

## Non-Functional Requirements

**Compatibility**
- Checks run in Chromium, Firefox and WebKit; WebKit is used only for these inline checks (environment.md).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `renderMode` table from IT-16-1 | Exact outputs |
| AC-3 | Open action given a name and a server type that disagree | Decision by server type only |

## Blackbox Tests

Playwright against the real stack (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2, AC-3 | DS-17 uploaded; controls | IT-16-2 (file-browser half); FT-N-11; FT-P-39 (controls) | Download only; 0 executions; controls inline | AC-47, NFT-SEC-19 |
| AC-4 | Hostile names and error texts | ST-16-1 (listing, panel, notices) | Text only | AC-47 |
| AC-5 | Sentinels; storage blocked | ST-16-2 (sign-in, upload, resume) | 0 sentinels; app works | AC-58 |

## Constraints

- The page uses the server's media type only; no sniffing and no file-name rule (16 §2).

## Risks & Mitigation

**Risk 1: A later page renders content without the rule**
- *Risk*: a new view embeds content directly.
- *Mitigation*: every inline view goes through `renderMode`; IT-16-2's DOM observer runs over every page that shows content.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The file browser's open/preview view is not drawn in the approved mockups; AC-2 needs it | Owner (design) | open | Low |
| G-10 (inline eligibility by declared type, sniffing or name) is narrowed to the server's declared type here; the owner gap stays open for the server side | Owner | open | Low |
