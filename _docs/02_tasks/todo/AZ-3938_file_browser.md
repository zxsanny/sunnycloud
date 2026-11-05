# File browser: keyset paging, virtual list, create, rename, move, delete

**Task**: AZ-3938_file_browser
**Name**: File browser
**Description**: The folder page: server-cursor paging into a virtual list, breadcrumb navigation by folder id, list and photo-grid views, create folder, rename, move and delete with the server's answer shown in place
**Complexity**: 5 points
**Dependencies**: AZ-3935_spa_scaffold, AZ-3956_design_tokens_components, AZ-3950_playwright_web_harness, AZ-3707_tree_reader, AZ-3708_tree_folder_mutations, AZ-3829_tree_transfer_feed_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3938
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — `download` called by the row action, produced by E-26/c09.

## Problem

Folders can hold 10,000 entries or more; the API pages them by cursor (AC-6) and the browser must not render them all. Folder state changes on other devices (AC-70), so the page must show the server's listing, never a client-side path cache. File names are untrusted input.

## Outcome

- Any folder size scrolls smoothly and shows every entry exactly once (mockup `ui_mockups/web-files.html`, states "Folder", "Photo grid", "Empty", "Loading").
- Create, rename, move and delete each make one server call and show the server's result or refusal in place.
- The page navigates by folder id and reflects moves made elsewhere on refresh.

## Scope

### Included
- Breadcrumb and folder navigation by node id; root from the API.
- Listing by the server cursor and page limit; a virtual list holding at most 200 rows in the DOM; list and photo-grid views (preference kept in `localStorage`, wrapped so blocked storage falls back to the default).
- Rows: thumbnail slot (filled by E-26/c15), name as text with ellipsis and the full name as tooltip, size, modified date, selection checkbox, row menu with Download (E-26/c12), Share link (E-26/c13), Rename, Move to, Delete.
- Create folder, rename, move (folder picker) and delete with inline confirmation; server refusals shown next to the item.
- Empty state (upload action and desktop app pointer) and loading skeleton.

### Excluded
- Upload controls and the transfers panel (E-26/c09), downloads (E-26/c12), the share dialog (E-26/c13), thumbnail states (E-26/c15), file preview (E-26/c17).
- "Select all" across pages: not offered (16 §7).

## Acceptance Criteria

**AC-1: Paged and complete (AC-6, IT-16-16, FT-P-09)**
Given a folder with 10,000 entries (DS-02)
When the user scrolls from the top to the end
Then every entry appears exactly once, at most 200 rows are in the DOM at any time, and each page request uses the cursor the server returned

**AC-2: Folder operations show the server result**
Given a folder
When the user creates folder "a", renames a file, moves it into "a" and deletes another file after confirming inline
Then each action makes one API call and the listing then shows the server's result; a refusal (name taken, `InvalidName`, gone) is shown next to the item and the listing is unchanged

**AC-3: Moves elsewhere appear on refresh (AC-70, IT-16-16)**
Given another client moves the open folder to another parent and renames a file in it
When the page refreshes
Then it shows the folder under its new parent with the new file name, opened by folder id, and no stale path or name remains

**AC-4: Untrusted names are text (ST-16-1 browse half)**
Given entries named `<img src=x onerror=alert(1)>`, `"><script>alert(1)</script>`, a name with a bidi override character and a 255-character name
When the folder is listed in Chromium, Firefox and WebKit
Then each renders as plain text, long names are truncated with the full name in the tooltip, and 0 scripts run

**AC-5: Selection stays within loaded rows (IT-16-16)**
Given a 10,000-entry folder with 300 rows loaded
When the user selects rows and opens the actions
Then actions apply only to the selected loaded rows and no control selects entries that are not loaded

**AC-6: View preference survives and degrades**
Given the user switches to the photo grid
When the page reloads, and in a second run with storage blocked
Then the grid is kept after reload; with storage blocked the page works and shows the list view

**AC-7: Empty and loading states**
Given an empty folder, and a listing that answers slowly
When each opens
Then the empty state shows its upload action and desktop app pointer, and while loading skeleton rows in the list's shape are shown

## Non-Functional Requirements

**Performance**
- At most 200 DOM rows for 10,000 entries (epic NFR); server answer at most 300 ms p95 is the server's (AC-6).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Page merging by cursor with overlapping and empty pages | Each entry once, order kept |
| AC-1 | Virtual list window over 10,000 items | Rendered rows never above 200 |

## Blackbox Tests

Playwright against the real stack (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-5 | DS-02 (10,000 entries) | IT-16-16 listing half; FT-P-09 | Each entry once; <= 200 rows; no cross-page select | AC-6 |
| AC-2 | Small folder | Create, rename, move, delete; refused rename | One call each; refusal in place | |
| AC-3 | Second client moves and renames | IT-16-16 move half; FT-N-42 contribution | New location and name on refresh | AC-70 |
| AC-4 | Hostile names (DS-17 names) | ST-16-1 browse half | Text only; 0 scripts | AC-47 |
| AC-6, AC-7 | Storage blocked; empty and slow folder | Preferences and states | Default view; empty state; skeleton | ST-16-2 |

## Constraints

- No client-side path cache; every navigation is by node id (16 §12, AC-70).
- Names are rendered only as text (no `{@html}`, lint rule from E-26/c06).

## Risks & Mitigation

**Risk 1: Scroll position jumps while pages load**
- *Risk*: the virtual list resizes as pages arrive.
- *Mitigation*: fixed row height (52 px, DESIGN §5) and placeholder rows for pages not yet loaded.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The mockup offers sorting by Modified and Size and puts folders first, but the listing pages by name key only; order across pages for other keys is undefined | E-13 and E-21 owners | open | Medium |
| The folder summary line (counts and total size) and the "Shared" chip on rows have no field in the listing | E-13 and E-16 owners | open | Low |
