# FirstSyncMerger and resume after kills

**Task**: AZ-3906_first_sync_merger
**Name**: First-sync merger
**Description**: Merge a local tree and a server tree by `name_key` when a pair is first connected, after an index rebuild and after `ResyncRequired`: local-only uploads, server-only downloads, equal content links with 0 bytes sent, different content uploads the local file so the server names a conflict copy and then downloads the server version; nothing is deleted and an interrupted merge resumes from the index
**Complexity**: 5 points
**Dependencies**: AZ-3891_local_index_store, AZ-3892_platform_fs_linux, AZ-3901_decide_from_disk_pair_worker, AZ-3905_change_applier, AZ-3960_sync_tree_operations, AZ-3767_sc_names; notes: E-31/c18 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon
**Tracker**: AZ-3906
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `list_children` (files carry `content_hash`) (E-23/c08 contribution).
- `_docs/02_document/contracts/shared_names/sc_names.md` — `name_key` (E-05/c01).

## Problem

Connecting a folder that already has files to a server folder that also has files is the riskiest moment of sync: a naive client either overwrites one side or uploads everything again. AC-31 requires, for 1,000 local files of which 200 collide (100 identical, 100 different): 0 files lost, 0 identical bytes uploaded, exactly 100 conflict copies. The same rule must run after a lost index and after a server restore (AC-41), and survive the daemon being killed in the middle (NFT-RES-18).

## Outcome

- Both trees enumerated by `name_key` (server listings through `list_children` carry `content_hash` for files).
- Local only: upload. Server only: download (verified path of E-24/c12b). Equal hash: the entry is linked in the index with 0 bytes sent, compared against the listing's `content_hash` with no extra call.
- Different content: the local file is uploaded as a new file at that name; the server answers with a conflict copy it names itself; the local path is replaced by the server version only after the upload is acknowledged (dirty-path rule); the copy comes back through the feed.
- Case-colliding local names on a case-sensitive file system lose neither file.
- Nothing is deleted on either side.
- Progress is kept in the index so a killed merge resumes to the same end state.
- Called on `add_pair` for a non-empty root or server folder, after `rebuild`, and by the feed loop on `ResyncRequired` (E-24/c12a).

## Scope

### Included
- The merge rule, its resume, and its use of the index, `list_children`, the pair worker's upload and the applier's download.

### Excluded
- Who triggers it (E-24/c19 on `add_pair`, E-24/c12a on resync, E-24/c06 on rebuild); hashing and transfer mechanics (E-23).

## Acceptance Criteria

**AC-1: Collisions merged safely (AC-31)**
Given DS-13: 1,000 local files; the server folder has 100 identical and 100 different paths
When the pair is added
Then 0 files are lost, 0 bytes are uploaded for the 100 identical files (proxy log), exactly 100 server-named conflict copies exist, and all 1,000 local files are present at the end

**AC-2: Replace only after the upload ack**
Given one different-content path
When the merger processes it
Then the order is: local upload acknowledged, then the server version downloaded and renamed into place; the local content is never lost if the process stops between the two

**AC-3: Resume after kills (NFT-RES-18)**
Given the same merge killed at 25%, 50% and 75%
When the daemon restarts each time
Then the merge resumes from the index and reaches the same end state as AC-1, with no duplicate upload of an already-acknowledged file

**AC-4: Nothing is deleted**
Given server-only and local-only files on both sides
When the merge runs
Then 0 `delete_node` calls and 0 local deletions or trash moves occur

**AC-5: Case collisions on a case-sensitive root**
Given local `A.txt` and `a.txt` on ext4 and a server folder with `a.txt`
When the merge runs
Then neither local file is lost and the server holds both contents (one as a conflict copy)

**AC-6: Merge after resync (AC-41)**
Given the feed loop's `ResyncRequired` handling after a database restore
When the merger runs
Then local-only files upload, server-only files download, and deletions happen only through the guard

## Non-Functional Requirements

**Reliability**
- Unit coverage target 90% on this module (E-24 Testing Strategy).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Fake listing with `content_hash`; local-only, server-only, equal, different | Upload, download, link with 0 bytes, upload then download |
| AC-2 | Order of upload ack and replace | Replace only after ack |
| AC-3 | Kill points with a fake `SyncApi`, reopen the index | Same end state; no repeat upload |
| AC-4 | Any mix | 0 deletes |
| AC-5 | Case-colliding local names on a case-sensitive pair | Both kept |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-3 | DS-13; server pre-seeded; `e2e-ctl` kill at 25%, 50%, 75% | IT-14-11, AT-14-2 | 0 lost; 0 identical bytes; 100 copies; same end state after kills | AC-31, FT-P-54, NFT-RES-18 |
| AC-6 | DB restored from an older backup | IT-14-13 merge half | Local-only up, server-only down, no deletes | AC-41, FT-P-13 |

## Constraints

- IT tier: the module runs in-process with the real platform layer, the real LocalIndex and the real `HttpSyncClient` against the server container (profile `desktop`); runs of the whole daemon binary belong to E-24/p01..p03.
- The daemon never chooses a conflict-copy name (ADR-005); a new-file collision gets its name from the server.
- `lookup_revisions` and `server_has_hashes` are not used.

## Risks & Mitigation

**Risk 1: Listings without `content_hash`**
- *Risk*: an older server listing omits the hash and every collision becomes a conflict copy.
- *Mitigation*: the E-24 assumptions require `content_hash` on every file in a listing (E-21, 13 `list_children`); the behaviour for a file listed without it is flagged below.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| NFC versus NFD names (G-1) decide whether two such names collide | Owner | open | Low |
| What the merger does with a server file listed without `content_hash` is not stated | E-24 / E-21 | open | Low |
