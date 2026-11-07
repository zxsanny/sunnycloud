# Contract: change-feed

**Component**: 05_sync_feed
**Producer task**: E-12/c04 — `_docs/02_tasks/todo/B02-25_change_feed_read.md` (read path, head, cursor, items); E-12/c05 — `B02-26_feed_long_poll.md` (hold behaviour) (renamed to their AZ keys at Step 4)
**Consumer tasks**: E-21 http_api (changes and changes-head handlers, E-21/c13; long-poll wake at shutdown, E-21/c17; OpenAPI), E-23 sync core and E-24 desktop daemon (through the HTTP API), E-22 pipelines (change feed, restore and resync)
**Version**: 1.0.0
**Status**: draft (frozen when E-12/c05 is accepted)
**Last Updated**: 2026-11-04

## Purpose

What `ChangeFeed::read` and `ChangeFeed::head` return, how a cursor is judged, and how a held read behaves. The HTTP routes (`GET /api/v1/changes?cursor=E.S&wait=&limit=`, `GET /api/v1/changes/head`, scope SessionOrDevice) are 11's; 11 passes the caller's principal and 01's `PrincipalRevalidator` into `read`. Signatures are declared in `contracts` part 2 (E-00/c04). Source: `components/05_sync_feed/description.md` §2-3, `data_model.md` §7.2.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| cursor parse | text `E.S` -> `Cursor{epoch, seq}` | `BadCursor` (wire 422 `invalid_cursor`) | sync |
| `ChangeFeed::read` | `Principal, &dyn PrincipalRevalidator, Cursor, wait?, limit?` -> `ChangePage{items, next_cursor, has_more}` | `ResyncRequired{reason}` (wire 410 `resync_required`), `Unauthenticated` (401), `BadCursor` (422), `TooManyHolds{retry_after}` (wire 429 `rate_limited` + `Retry-After`; decided G-05-2), database unavailable (maps to 503) | async |
| `ChangeFeed::head` | `account` -> `Cursor` | none | async |

`ChangeItem` = `node_id, parent_id, kind, name, state (live | tombstone), revision_id?, size?, content_hash?, seq`. `reason` is `epoch_changed`, `below_purge_horizon` or `cursor_ahead`.

## Invariants

- Every read is scoped by the caller's account. A cursor of one account says nothing about another; `ResyncRequired` carries only `reason`.
- Cursor rules: `E != epoch` -> `epoch_changed`; `S < purge_horizon` -> `below_purge_horizon` (`S == purge_horizon` is valid); `S > seq` -> `cursor_ahead` (a restore without an epoch bump). A malformed cursor is `BadCursor`, never a database error.
- A page holds the latest state of each node with `seq > S`, ordered by `seq` then `node_id`; a node changed many times appears once. A page ends on a `seq` boundary, so it can hold more than `limit` items when a batch shares one `seq`. `limit` defaults to `feed_page_limit_default` (500) and is capped by `feed_page_limit_max`.
- A folder tombstone means its whole subtree is deleted. A folder that turns from tombstone to live must be re-listed by the client.
- `head` returns the account's current `epoch.seq`; a new device takes head first, then lists the tree.
- Hold: when the first read is empty and `wait > 0` (capped by `longpoll_wait_max`, 30 s), the read parks without a database connection. It returns on a notify for its account, on a safety recheck that finds changes (`feed_safety_recheck`), with `Unauthenticated` when the revalidator reports the principal revoked (checked every `longpoll_auth_recheck`, 2 s), with an empty page and the same cursor when `wait` expires or the process shuts down, and with the database-unavailable error during an outage. A principal holds at most `max_holds_per_principal` (4) reads at once; the next one is refused at once with `TooManyHolds{retry_after}` (proposed `retry_after`: fixed 5 s, a constant of the long-poll task) and the parked reads stay parked.
- No item is returned after the revalidator reports the principal revoked.

## Non-Goals

- Not covered: HTTP status mapping, `Cache-Control: no-store` and OpenAPI (11); client cursor persistence (13, 14); authentication (01).

## Versioning Rules

- **Breaking changes** (a cursor rule, the item fields, the page boundary rule) require a new major version and notice to E-21 and E-23.
- **Non-breaking additions** (a new optional item field; clients ignore unknown fields) require a minor version bump. The hold-cap error from E-12/c01 is added as a minor version.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| latest-state | node changed at seq 1 and 5 | appears once with seq 5 | IT-05-3 |
| batch-boundary | 500 items at seq 7, `limit = 100` | all 500 in one page | IT-05-3 |
| cursor-rules | epoch E, seq 50, horizon 20: `E+1.10`, `E.19`, `E.20`, `E.51` | `epoch_changed`, `below_purge_horizon`, ok, `cursor_ahead` | IT-05-4 |
| malformed | `abc`, `1.2.3`, `E.1 OR 1=1` | `BadCursor` | IT-05-4, ST-05-4 |
| hold-wake | parked read, stamp + notify | page within 50 ms | IT-05-6 |
| hold-expiry | `wait = 300 ms`, no change | empty page, same cursor | IT-05-6 |
| revoked | revalidator turns false | `Unauthenticated`, no items | IT-05-7 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
