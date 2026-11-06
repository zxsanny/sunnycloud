# Feed client: poll_changes, head_cursor, backoff, page invariants

**Task**: AZ-3965_feed_client
**Name**: Change-feed client
**Description**: The long-poll client of the change feed: `poll_changes` with the client timeout rule, immediate re-poll on `has_more`, backoff with jitter and `Retry-After`, the page invariant check on every page, `ResyncRequired` on 410 and 422 `invalid_cursor`, and `head_cursor`
**Complexity**: 3 points
**Dependencies**: AZ-3955_sync_core_proto, AZ-3958_sync_typed_errors, AZ-3959_http_sync_client_connect, AZ-3967_sync_host_traits_foundation, AZ-3970_scripted_fake_server
**Component**: 13_sync_core (crate `sync-core`)
**Tracker**: AZ-3965
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — `poll_changes`, `head_cursor` (this task contributes them).
- `_docs/02_document/contracts/13_sync_core/sync_core_proto.md` — `Cursor`, `ChangePage`, `check_page`, `ResyncReason`.
- `_docs/02_document/contracts/05_sync_feed/change-feed.md` — server read, head and resync semantics.

## Problem

Devices learn about remote changes only through the feed (F7). A page that is out of order, skips a sequence number or jumps epochs must never be applied silently (AC-69 client side), and after a database restore every device must resync without missing changes (AC-41): head cursor first, then listing (F14). The long-poll must survive idle NATs and must not wait forever on a dead connection. 13 never persists the cursor; the host does after applying.

## Outcome

- `poll_changes(cursor, wait, limit) -> ChangePage | ResyncRequired{reason}` and `head_cursor() -> Cursor` as in the `sync_api` contract.
- Every page checked with rules P1..P6 before it is returned; a violation is `ProtocolViolation{detail}` naming the rule.
- Client timeout = `wait + sync.longpoll_client_margin`; `wait` clamped to `ServerInfo.longpoll_max_wait`; errors back off with jitter between `sync.retry_base` and `sync.retry_max`, honouring `Retry-After`.

## Scope

### Included
- The poll request with cursor, wait and limit; `has_more = true` returns so the caller can re-poll immediately (no wait inside the client).
- Timeout and backoff rules; 429 `Retry-After` honoured.
- `ResyncRequired{reason}` from 410 and `ResyncRequired{InvalidCursor}` from 422 on this route.
- `head_cursor`.
- A test helper that performs the host-side resync order (head first, then listing) so the order is checked in the request log.

### Excluded
- Cursor persistence and applying changes (hosts: 14 feed loop, F7).
- The real-server restore leg of IT-13-12 (E-23/c18a).
- The server's feed (E-12).

## Acceptance Criteria

**AC-1: Invalid pages are refused (AC-69)**
Given cursor `1.100` and scripted pages v1..v6 of IT-13-11
When `poll_changes` receives each
Then each returns `ProtocolViolation{detail}` naming the violated rule, and no page is returned to the caller

**AC-2: Valid page and immediate re-poll**
Given a valid page with seq 101..150, `next_cursor = 1.150` and `has_more = true`
When `poll_changes` returns it and the caller polls again from `1.150`
Then the page is returned unchanged and the second request is sent with no client-side delay

**AC-3: Resync from 410 and 422 (AC-41)**
Given a fake answering 410 `resync_required` with `reason = epoch_changed` and, separately, 422 `invalid_cursor`
When `poll_changes` runs
Then the results are `ResyncRequired{EpochChanged}` and `ResyncRequired{InvalidCursor}`, and the resync helper's request log shows `head_cursor` before the first `list_children`

**AC-4: Long-poll timeout rule**
Given `wait = 30 s`, `sync.longpoll_client_margin = 10 s`, a server cap `longpoll_max_wait = 20 s` and a fake that never answers
When `poll_changes` runs (short test durations from a config mock, not real waits)
Then the request asks for the clamped wait (20 s) and the client gives up at wait plus margin with a `Transport` error that the caller can retry

**AC-5: Backoff with jitter and Retry-After**
Given a fake answering 503 three times, then 429 with `Retry-After: 3`
When the caller polls in a loop through the client's backoff
Then successive delays grow from `sync.retry_base` up to at most `sync.retry_max` with jitter, and the attempt after the 429 is not earlier than 3 s (jitter under 1 s)

**AC-6: The core never stores the cursor**
Given the `CheckpointStore` trait and a full poll session
When the session ends
Then no host-trait method received a cursor

## Non-Functional Requirements

**Reliability**
- Keep-alive pings (`sync.keepalive_interval`) keep a held long-poll alive through NATs (transport from E-23/c07).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Wait clamp and timeout computation | Clamped wait; timeout = wait + margin |
| AC-5 | Backoff sequence with a seeded jitter | Within base..max; `Retry-After` respected |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Scripted fake (E-23/c17), pages v1..v6 | Poll | `ProtocolViolation` per rule | IT-13-11, NFT-RES-35 |
| AC-2 | Valid page, `has_more` | Poll twice | No delay | IT-13-11 |
| AC-3 | Fake 410 and 422 | Poll; resync helper | `ResyncRequired`; head first | IT-13-12, FT-P-13, NFT-RES-07 |
| AC-4 | Silent fake, config mock durations | Poll | Clamp; timeout at wait + margin | NFT-RES-25 |
| AC-5 | Fake 503 x 3, 429 | Poll loop | Backoff; >= 3 s after 429 | IT-13-19 |
| AC-6 | Store spy | Poll session | No cursor in any call | IT-13-12 |

## Constraints

- `ChangeItem` is the latest node state, not an event; the client does not derive create, move or delete (13 risk 5).
- Durations come from config; tests use short values, never production sleeps.

## Risks & Mitigation

**Risk 1: Wire unit of the long-poll cap**
- *Risk*: seconds versus milliseconds misread gives a 1000x wrong wait.
- *Mitigation*: flagged to E-21; AC-4 checks the clamp against the parsed value.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `longpoll_max_wait` unit on the wire not specified (`server_info.md` open point 1) | E-21 owner | open | Low |
| The server's long-poll hold-cap error has no name yet (G-05-2, `change-feed.md`); it is mapped as `Unexpected` until named | E-12/c01 | open | Low |
