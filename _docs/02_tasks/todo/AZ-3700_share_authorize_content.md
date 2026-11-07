# authorize_content: current revision, 1 s cache and the download-start audit rule

**Task**: AZ-3700_share_authorize_content
**Name**: Share content authorization
**Description**: Re-check the link and its file on every visitor content request with a cache of at most 1 s, return the file's current revision so edits keep the link alive, follow renames and moves, answer missing within 5 s of revoke or delete, and audit one access per download start
**Complexity**: 3 points
**Dependencies**: AZ-3695_sharing_gaps_grant_key_spike, AZ-3698_share_lookup_open_uniform, AZ-3784_sc_audit, AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3710_tree_conflict_rules, AZ-3714_tree_cascade_purge_janitors
**Component**: 06 sharing
**Tracker**: AZ-3700
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c06

## Problem

06 never serves bytes: 11 asks 06 which revision a visitor may read and then streams it through 03. That answer must reflect the link and the file as they are now: a save-in-place edit must keep the link working (AC-34), a rename or move must not break it, and a revoke or delete must close it within 5 s (AC-12, AC-77). Range requests must not flood the audit log.

## Outcome

- `PublicShareService::authorize_content(token, grant?, ip, starts_download)` returns `ShareContent{account_id, node_id, revision_id}` with the current revision, or `NotFound` / `PasswordRequired`.
- Lookup and target results are cached for at most `share_lookup_cache` (1 s), invalidated by age only; `open` uses the same bound.
- `share_accessed` is audited once per call with `starts_download = true`.

## Scope

### Included
- `authorize_content`: the lookup of E-16/c04, then `TreeReader::resolve_live_file` for the current revision; a gone or folder target is `NotFound`; a password link without a grant is `PasswordRequired` (grant acceptance is added by E-16/c05).
- The result cache shared by `open` and `authorize_content`, no entry older than `share_lookup_cache`.
- `share_accessed` audit with ids only when `starts_download` is true; audit write failure logged at ERROR and not visible in the result, for create, revoke and access.

### Excluded
- Streaming, Range and serving headers (E-14, E-21/c14); deciding `starts_download` from the Range header (11).
- Grants (E-16/c05); counters (E-16/c07).

## Acceptance Criteria

**AC-1: The current revision is returned (AC-34)**
Given a link on file f with revision r1 and `share_lookup_cache = 200 ms`
When `authorize_content` runs, the owner publishes r2 on the same node (save-in-place), and `authorize_content` is polled
Then the first answer is `ShareContent{account_id, node_id, revision_id: r1}`, the answer becomes r2 within 200 ms, and the link row is untouched (same id, same token)

**AC-2: One audit event per download start**
Given 5 calls with `starts_download = true` and 5 with `false` on the same link
When the audit log is read
Then there are exactly 5 `share_accessed` events, with ids only

**AC-3: Password links need a grant here too**
Given a password link and a link without password
When `authorize_content` runs on each without a grant
Then the password link gives `PasswordRequired` and the other is allowed

**AC-77: The link follows its file and dies with it (component part, R10)**
Given a link on `/a/f.txt` and `share_lookup_cache = 200 ms`
When f is renamed to `g.txt`, moved to `/b/`, renamed case-only to `G.TXT`, moved into a deeper folder, then deleted; and separately a folder holding another shared file is deleted
Then after each rename and move `open` shows the new name and `authorize_content` returns the same `node_id` with the current revision and 0 changes to `share_links`; after the file delete and after the folder delete (before the background cascade runs) the answer is `NotFound` within 200 ms + 100 ms and always within 5 s

**AC-4: Revival and purge**
Given a deleted shared file
When an edit revives it (edit beats delete), then it is deleted again and purged past `tombstone_horizon`
Then after revival the link works again with the same token; after purge its row is gone (cascade) and the answer is `NotFound`

**AC-5: Expiry and revocation timing (AC-12)**
Given a link with `expires_in = 1 h` and the manual clock
When the clock is set to `expires_at - 1 ms`, `expires_at` and `expires_at + 1 s`, and separately the link is revoked while `authorize_content` is polled
Then the answers follow the boundary of G-06-3 (live before `expires_at`, `NotFound` at and after); revocation gives `NotFound` within `share_lookup_cache` + 100 ms and always within 5 s; moving the clock moves the answer, so no product time comes from SQL `now()`

**AC-6: Audit failure does not leak (IT-06-11)**
Given an `AuditWriter` decorator that fails every write
When a link is created, revoked and accessed with `starts_download = true`
Then each call returns its normal result and an ERROR is logged for each failed write

## Non-Functional Requirements

**Performance**
- `authorize_content` p95 on a cache miss (two primary-key reads) at most 15 ms, fail above 60 ms (PT-06-1, run by E-16/c09).

**Reliability**
- Revocation and deletion visible within 5 s (AC-12, AC-77).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Cache entry age with the manual clock | No entry older than `share_lookup_cache` served |
| AC-2 | Audit decision | Event only when `starts_download` is true |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Link on f (r1); cache 200 ms | IT-06-4 save-in-place | r1, then r2 within 200 ms; row untouched | FT-P-57 |
| AC-2 | 5 true, 5 false | IT-06-4 audit count | 5 events; ids only | |
| AC-3 | Password and plain links | IT-06-4 grant-less calls | `PasswordRequired`; allowed | |
| AC-77 | Link on `/a/f.txt`; folder with a shared file | IT-06-5, IT-06-6 | Same node and current revision after each move; `NotFound` within 300 ms after deletes, before cascade | FT-N-44 |
| AC-4 | Deleted shared file | IT-06-6 revival and purge | Works after revival; row gone after purge | |
| AC-5 | `expires_in = 1 h`, manual clock | IT-06-7 | `expires_at > now` live; revoke within cache + 100 ms | NFT-RES-25 |
| AC-6 | Failing writer | IT-06-11 | Normal results; ERROR logged | |

## Constraints

- The cross-component AC-77 through the real server (routes, 03 streaming) is owned by the pipeline task AZ-3662 (E-22/p10); this task owns the cache bound and the current-revision rule.
- A file under a deleted folder is gone at once through 02's parent-chain walk in `resolve_live_file` (gap G-06-1, resolved at the Step 5 gap pass); this task relies on it and does not wait for the cascade.
- Tests run against the real tree and feed (E-13, E-12) on PostgreSQL 18.

## Risks & Mitigation

**Risk 1: Stale cache serves a deleted file**
- *Risk*: a cache keyed too broadly keeps a deleted target alive past 5 s.
- *Mitigation*: entries expire by age at `share_lookup_cache` (1 s); AC-77 and AC-5 measure it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The expiry boundary depends on E-16/c01 (G-06-3); decided 2026-11-07 as `expires_at > now` live | E-16 owner | resolved | Low |
