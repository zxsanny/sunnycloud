# Thumbnails: state polling and placeholders

**Task**: AZ-3945_thumbnail_polling
**Name**: Thumbnail states and polling
**Description**: Show each listing item's thumbnail by state (ready image, pending skeleton, failed or none type icon) and poll pending items at `web.thumb_poll_interval` only while the tab is visible
**Complexity**: 2 points
**Dependencies**: AZ-3938_file_browser, AZ-3950_playwright_web_harness, AZ-3826_media_gaps_spike, AZ-3844_thumbnail_states_derived_opener, AZ-3830_share_media_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3945
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper, produced by E-26/c06.
- `_docs/02_document/contracts/08_media_jobs/derived_access.md` — thumbnail states and derived reads, produced by E-18.

## Problem

Thumbnails appear seconds after an upload (AC-10, FT-P-20). The page must show progress without a reload and without a request storm, must stop polling in hidden tabs, and must show a clear placeholder for items that never get a thumbnail (ADR-009).

## Outcome

- Listing rows and grid tiles show the state of each item (DESIGN §5 Thumbnails; mockup `ui_mockups/web-files.html`, "Folder" and "Photo grid": "Preview pending", "No preview").
- Pending items are polled together, at most once per interval, only while visible; ready replaces the placeholder in place.

## Scope

### Included
- States `pending` (skeleton pulse, stopped under reduced motion), `ready` (image from the thumbnail route, rounded, cover), `failed` and `none` (type icon on the sunken surface, not polled).
- Polling every `web.thumb_poll_interval` (2 s, build constant) for pending items in view, one request per interval for all of them, paused on `visibilitychange` to hidden and resumed on visible.
- Grid tiles with the same states.

### Excluded
- The listing itself (E-26/c08); public-share thumbnails (the public page shows the original per `renderMode`).

## Acceptance Criteria

**AC-1: Pending items poll until ready (AC-10, IT-16-15, FT-P-20)**
Given `web.thumb_poll_interval` set to 200 ms by config mock and a listing with pending, ready, failed and none items (DS-03)
When the page is visible
Then pending items are re-requested about every 200 ms (± 50 %) until ready, ready replaces the skeleton without a reload, and there is at most 1 state request per interval covering all pending items in view

**AC-2: No polling while hidden**
Given pending items
When the tab is hidden and shown again
Then no state request is sent while hidden, and polling resumes within one interval of becoming visible

**AC-3: Failed and none get a placeholder**
Given failed and none items
When they are shown
Then each shows its type icon with "No preview" and is never polled

**AC-4: Pending look and reduced motion**
Given a pending item, and the reduced-motion preference set
When it is shown
Then it shows the skeleton with "Preview pending"; with reduced motion the pulse is still

**AC-5: Only thumbnail images are images (AC-47 cross-check)**
Given a ready item of a non-allowlisted type (for example a PDF with a poster thumbnail) and an HTML file
When rows and tiles render
Then images come only from the thumbnail route, and the HTML file's content is never placed in an image or frame

## Non-Functional Requirements

**Performance**
- At most one state request per interval per page view; no polling while hidden.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-2 | Poll scheduler with visibility changes and a fake timer | One batched request per interval; none while hidden |
| AC-3 | State-to-view mapping | Failed and none never scheduled |

## Blackbox Tests

Playwright against the real stack with the config mock (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-3 | DS-03; 200 ms interval | IT-16-15; FT-P-20 (web half) | Polling, hiding, placeholders as stated | AC-10 |
| AC-4 | Reduced motion | Skeleton | Pulse stops | |
| AC-5 | PDF and HTML items | Rendered rows and tiles | Images only from the thumbnail route | AC-47 |

## Constraints

- One state request covers all pending items in view; the request body follows the thumbnail-states operation as fixed by E-18/c01.

## Risks & Mitigation

**Risk 1: Request storm in big folders**
- *Risk*: polling every pending item separately in a 10,000-item folder.
- *Mitigation*: one batched request for the items in view (AC-1).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The thumbnail-states request body (node ids or content hashes) is open (`api_v1_surface.md` open point 6, settled by E-18/c01) | E-18/c01 | open | Medium |
| Video tiles show a duration chip (DESIGN §5), but the listing carries no duration field | E-13 and E-18 owners | open | Low |
