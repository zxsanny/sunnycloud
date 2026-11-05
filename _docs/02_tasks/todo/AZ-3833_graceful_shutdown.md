# Graceful connection shutdown (readiness 503, long-poll wake, drain)

**Task**: AZ-3833_graceful_shutdown
**Name**: Graceful connection shutdown
**Description**: On the signal passed to `run`: readiness 503, stop accepting, answer held long-polls with an empty page, close connections gracefully within `shutdown_grace` and abort any stream still running at grace end, then return
**Complexity**: 2 points
**Dependencies**: AZ-3801_crate_skeleton_accept_loop, AZ-3812_content_header_mapper, AZ-3832_ops_endpoints_readiness
**Component**: 11 http_api
**Tracker**: AZ-3833
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c17

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/http_server.md` — `run` and its shutdown order (E-21/c03).
- `_docs/02_document/contracts/05_sync_feed/change-feed.md` — held reads return an empty page with the same cursor on shutdown (E-12).

## Problem

A restart or upgrade sends SIGTERM to the API while clients hold long-polls and stream blocks. Held long-polls would otherwise wait up to 30 s; Compose would keep routing to a dying process; and a download cut at the end of the grace period must not look like a complete file. 12 runs the rest of the shutdown (scheduler, upload checkpoints, final group sync, pool) after `run` returns, so `run` must return within the grace period (AC-38, AC-21).

## Outcome

- The HTTP side of the shutdown runs in the order of `http_server.md`: readiness 503, accept stops, held long-polls answered, connections closed gracefully, streams still running at grace end aborted, `run` returns within grace + 1 s.

## Scope

### Included
- Ordering the steps on the shutdown signal; graceful close of HTTP/1.1 and HTTP/2 connections; waiting at most `shutdown_grace`; abort of remaining streams.
- Making sure a held long-poll response written on wake is sent before its connection closes.

### Excluded
- Constructing the `ChangeFeed` shutdown signal (12) and the hold logic (05); the process shutdown order after `run` (E-22/c05); SIGTERM handling (E-22/c05).
- AT-11-2 (SIGTERM during 100 uploads, E-21/c20).

## Acceptance Criteria

**AC-1: Order of steps (AC-21, AC-38)**
Given 20 held long-polls (stub `ChangeFeed` honouring the shutdown signal), 3 downloads in progress and `shutdown_grace = 2s`
When the shutdown signal is sent
Then the timestamps are ordered: `/readyz` 503, then new connections refused or closed, then the 20 long-polls answered 200 with an empty page and their own cursor, then connections closed

**AC-2: Short downloads finish, long ones are aborted**
Given one download needing 1 s and one needing 5 s when the signal arrives
When grace (2 s) passes
Then the short one completes with its full body, and the long one is ended at about 2 s with RST_STREAM (HTTP/2) or a connection close before `Content-Length` (HTTP/1.1), never a clean short body

**AC-3: Run returns in time**
Given the same setup
When the signal is sent
Then `run` returns within grace + 1 s (3 s)

## Non-Functional Requirements

**Reliability**
- The grace period is a config value; tests use 2 s.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Step sequencing on the signal with fake listeners | Steps in the stated order |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, 20 polls, 3 downloads | Shutdown order (IT-11-16) | Ordered timestamps; empty pages, no cursor loss | AC-21, NFT-RES-26 |
| AC-2 | Mode S | Downloads at grace end (IT-11-16) | Full short body; long one reset | AC-38 |
| AC-3 | Mode S | `run` return (IT-11-16) | Within 3 s | AC-38 |

## Constraints

- 11 stops only its own listeners and connections; nothing after `run` returns is 11's.

## Risks & Mitigation

**Risk 1: Long-poll answered after its connection closed**
- *Risk*: the wake races the connection close and the client sees a reset instead of an empty page.
- *Mitigation*: AC-1 asserts the order with 20 polls.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How the `ChangeFeed` shutdown signal reaches 05 is left to construction by 12 (part 2 open point 5); 11 relies on held reads returning on that signal | E-12, E-22 owners | open | Low |

## Contract

This task contributes the shutdown semantics of `run` to the contract at `_docs/02_document/contracts/11_http_api/http_server.md` (producer E-21/c03).
Consumers MUST read that file — not this task spec — to discover the interface.
