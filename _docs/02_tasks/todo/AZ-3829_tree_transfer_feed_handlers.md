# Handlers for nodes, uploads, commits, content and changes (E-12, E-13, E-14 ports)

**Task**: AZ-3829_tree_transfer_feed_handlers
**Name**: Tree, transfer and feed handlers
**Description**: Register and implement the thin handlers of the sync data path: folders and nodes, revisions and the revision lookup, upload sessions, blocks, staged uploads, single and batch commits, node and revision content, the change feed and its head
**Complexity**: 5 points
**Dependencies**: AZ-3803_body_limits_timeouts, AZ-3806_auth_extraction, AZ-3808_csrf_origin_check, AZ-3810_error_mapper, AZ-3812_content_header_mapper, AZ-3827_route_registry_openapi, AZ-3769_contracts_part2
**Component**: 11 http_api
**Tracker**: AZ-3829
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — `TreeService`, `TreeReader`, `UploadService`, `CommitService`, `DownloadService`, `ChangeFeed` (E-00/c04).
- `_docs/02_document/contracts/02_tree/tree_service.md`, `_docs/02_document/contracts/02_tree/tree_reader.md` — node and revision behaviour (E-13).
- `_docs/02_document/contracts/03_transfer/upload_service.md`, `commit_service.md`, `download_service.md` — transfer behaviour (E-14).
- `_docs/02_document/contracts/05_sync_feed/change-feed.md` — cursor rules and hold behaviour (E-12).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md`, `problem_details.md`, `content_serving_headers.md` — rows, errors and content headers (E-21/c11, c09, c10).

## Problem

The sync data path (upload, commit, feed, download) is the vertical slice every client depends on (R15). Its handlers must pass exactly what the ports need (account from the principal, block hash from `X-Blake3`, the device label for conflict-copy names, the principal and revalidator for long-polls) and add nothing between the port's return and the response, so commit order seen by clients is the order the server decided (AC-69).

## Outcome

- Every operation of the Tree, Transfer and Sync rows of `api_v1_surface.md` is registered with its declarations and calls exactly one port method.
- Commit responses carry exactly the result the port returned, with no await between the port's return and the response.
- Content routes use the shared header mapper; the feed routes answer `Cache-Control: no-store`.

## Scope

### Included
- Tree: root, create folder, get, rename/move (PATCH), delete with `base_revision`, children with keyset `after` and `limit`, revision metadata, revisions by `client_device`, `revisions:lookup`.
- Transfer: start or resume upload (including `NothingToSend`), upload state, abort, block PUT (`X-Blake3`, exact `Content-Length`, 204 after the port acknowledges), staged PUT (zero bytes allowed), commit, batch commit, node content and revision content (GET and HEAD; `retained_ok` only for admins).
- Sync: changes with `cursor`, `wait`, `limit`; changes head. The principal and 01's `PrincipalRevalidator` are passed into `ChangeFeed::read`.
- `CommitActor{account_id, device_label}` filled from the principal.

### Excluded
- Business rules of 02, 03 and 05; graceful wake of held long-polls (E-21/c17); thumbnails, transcodes and shares (E-21/c14).
- End-to-end flows through real components (pipelines AZ-3655 to AZ-3659).

## Acceptance Criteria

**AC-1: One port call per operation**
Given a recording stub for every port of these groups
When each operation is called once with valid input and a session or device credential
Then exactly one call reaches the named port method with the principal's account (never an id from the request), and the answer status is the documented one (204 for an acknowledged block, 200 with a body or 204 elsewhere)

**AC-2: Commit responses are not reordered (AC-69)**
Given a commit stub that returns increasing `seq` values after random delays of 0 to 50 ms, called by 50 concurrent clients
When 1,000 commits are sent
Then each response body contains exactly the `seq` its own stub call returned (0 mismatches), and each response is sent in the order its stub call completed

**AC-3: Block upload passes the hash and length**
Given a block PUT with `X-Blake3`, `Content-Length` 2097152 and a recording `put_block` stub
When the block is sent
Then `put_block` receives the upload id, index, expected hash, length and a streaming body (not a buffered copy), and the answer is 204 only after the stub returns its acknowledgement

**AC-4: Device label reaches the commit**
Given a device principal labelled `laptop`
When it calls `POST /api/v1/commits` and `POST /api/v1/commits:batch`
Then `CommitActor.device_label` is `laptop` in both calls

**AC-5: Retained revisions only for admins**
Given a member session and an admin session
When both request `/api/v1/revisions/{id}/content`
Then `retained_ok` is false for the member and true for the admin; a `NotFound` from the stub is a 404

**AC-6: Feed passes principal and revalidator; errors map**
Given a `ChangeFeed` stub that returns a page, then `ResyncRequired{reason: below_purge_horizon}`, then `BadCursor`, then `Unauthenticated` mid-hold
When `GET /api/v1/changes?cursor=E.S&wait=25&limit=100` is called four times
Then the stub receives the principal, the revalidator, the cursor, the wait and the limit; the answers are 200 with `Cache-Control: no-store`, 410 `resync_required` with `reason: below_purge_horizon`, 422 `invalid_cursor` and 401

**AC-7: Listing keyset paging passes through**
Given a children stub returning a page with `next`
When `GET /api/v1/nodes/{id}/children?after=<key>&limit=500` is called
Then the stub receives the folder id, the `after` key and the limit, and the answer carries the items and the next key

## Non-Functional Requirements

**Performance**
- Listing a 10,000-entry folder through the stack meets AC-6 (300 ms p95), measured with real components by the system scenario NFT-PERF-10 (E-31).

**Reliability**
- No handler awaits anything after its port call returns, other than writing the response (AC-69).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Registry rows of these groups | Class, body class, flag, owner and ID parameters declared |
| AC-6 | Cursor text `E.S` and `wait`, `limit` binding | Passed unchanged; malformed cursor reaches the port's `BadCursor` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, recording stubs | Every operation once | One port call; principal's account | AC-59 |
| AC-2 | Mode S, delayed commit stub | 1,000 commits (IT-11-2, ordering part) | 0 mismatches; completion order | AC-69, NFT-RES-35 |
| AC-3 | Mode S | Block PUT | Hash, length, stream; 204 after ack | FT-P-05, AC-38 |
| AC-4 | Mode S | Device label | `laptop` in both calls | AC-8 |
| AC-5 | Mode S | Retained revision | `retained_ok` admin only | FT-N-44 |
| AC-6 | Mode S | Feed mapping | 200, 410 with reason, 422, 401 | AC-21, AC-41 |
| AC-7 | Mode S | Listing paging | Key and limit passed | AC-6 |

## Constraints

- Paths follow 02 §3, 03 §5 and 05 §3 (owners are authoritative, 11 §8).
- This group is the vertical slice: it is implemented before E-21/c12 and E-21/c14.

## Risks & Mitigation

**Risk 1: Task at the 5-point cap (about 20 operations)**
- *Risk*: breadth.
- *Mitigation*: bind, call, map only; content headers come from E-21/c10.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Body class of `revisions:lookup` (1,000 hashes, about 70 KB) is not assigned; `Small` is too small | E-13, E-21 owners | open | Medium |
| G-11-1 statuses for `need_blocks` and `edited_since`, and batch-too-large 413 vs 422, come from E-21/c02 | E-13, E-14 owners | open | Medium |
| The long-poll hold-cap error has no name or status yet (G-05-2) | E-12 owner | open | Low |

## Contract

This task implements the Tree, Transfer and Sync rows of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
