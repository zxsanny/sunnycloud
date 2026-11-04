# 05_sync_feed — Per-account change feed and long-poll

## 1. High-Level Overview

**Purpose**: Own the per-account `epoch.seq` counter and serve it: stamp every tree change with a gapless sequence number, answer `GET /changes` (immediate or held long-poll), detect `resync_required`, and bump the epoch after a restore.

**Single responsibility**: ordering and delivery of change notifications. It does not decide what a change means (02_tree) and does not authenticate (01_identity; it only asks whether a held principal is still valid through a `PrincipalRevalidator` that the caller passes in).

**Architectural pattern**: counter-last stamp trait, read projection over `nodes`, in-process notifier plus periodic safety recheck (no LISTEN/NOTIFY, no held connection).

**Scope in**: `account_feeds` row, the stamp statement, feed reads, cursor validation, head cursor, long-poll hold/wake/timeout/revocation recheck, epoch bump, purge-horizon raise (called by 02's purge in the same transaction), the account-erase hook for the feed row.
**Scope out**: tombstone purge selection (02), notification fan-out across hosts (v1.0 runs one API process, decision 12; replicas are unsupported, see risks), client cursor persistence.

**Upstream dependencies**: helpers (clock, config, error, db) and `contracts` ports only (see Ports). 05 requires no component port except 04's `EpochMarker`; the `PrincipalRevalidator` arrives as a parameter of `ChangeFeed::read`, so there is no wiring cycle with 01.
**Downstream consumers**: through ports only; the clients (13, 14, 16, 17) use the HTTP API.
**No cycle**: 05 reads `nodes` through its own projection and never calls 02. Only 02 stamps; it calls 05 through the ports below, inside its commit transaction.

## 2. Internal Interfaces

### FeedStamp (called last in every tree-changing transaction, by 02 only)
| Method | Input | Output | Errors |
|---|---|---|---|
| `stamp` | `&mut DbTx`, account, `TouchedNodes` (non-empty) | `Seq` | `AccountFeedMissing` (invariant) |
`stamp` is one data-modifying statement: `UPDATE account_feeds SET seq = seq + 1 … RETURNING seq` plus `UPDATE nodes SET seq = $new WHERE id = ANY(touched)` (single CTE). Nothing may run between it and COMMIT: no reads, no file I/O, no cycle checks. A batch commit (at most 500 entries) shares one `seq`. 02 does not call `stamp` when no node was touched (all entries failed or `NoChange`). A rolled-back transaction leaves no gap because the bump is the final statement. This is the only writer of `nodes.seq`; 05 owns that column and `nodes_feed_idx`.

### FeedNotifier (called by 02 after COMMIT)
`notify(account, seq)` is a non-blocking in-process wake of held requests for that account. Lost or late wakes are healed by `feed_safety_recheck`. A change made by a CLI command in another process (`admin restore`, `admin epoch-bump`, `admin user ...`) cannot call this notifier; held polls in the API process notice it on the 10 s recheck.

### ChangeFeed (called by 11 for `GET /changes`)
| Method | Input | Output | Errors |
|---|---|---|---|
| `read` | `Principal`, `&dyn PrincipalRevalidator` (from 01, passed by 11), `Cursor{epoch, seq}`, `wait?` (capped by config), `limit?` | `ChangePage{items, next_cursor, has_more}` | `ResyncRequired{reason}` (410), `Unauthenticated` (revoked while held), `BadCursor` (422, code `invalid_cursor`) |
| `head` | account | `Cursor` | none |
`ChangeFeed` also takes a shutdown signal: on shutdown every held read is woken and returns an empty page with the same cursor, so graceful stop does not wait for the hold timeout.
**ChangeItem** (latest state of one node, not an event): `node_id`, `parent_id`, `kind`, `name`, `state: live|tombstone`, `revision_id?`, `size?`, `content_hash?`, `seq`. Clients derive create/update/move/delete by comparing with their local index (data model D1; the "op" column of the flows document is not stored). A folder tombstone means its whole subtree is deleted: cascade marking does not stamp descendants. When a folder item changes from tombstone to live (a commit revived it as an ancestor, 02), its children may have been cascade-deleted meanwhile, so a client re-lists that folder (`GET /nodes/{id}/children`) instead of trusting its old local children. A page always ends on a `seq` boundary, so it can exceed `limit` when a batch shares one `seq`.
**Cursor rules** (data model 7.2): `ResyncRequired` when `E != epoch` (`reason = epoch_changed`), `S < purge_horizon` (`below_purge_horizon`) or `S > seq` (`cursor_ahead`, a restore without a bump); the `reason` travels as an extra field of the 410 body. Otherwise `nodes WHERE account_id = $1 AND seq > S ORDER BY seq, id`.
**Hold algorithm** (no connection while parked): if the first read is empty and `wait > 0`, park on the account's notifier; wake on notify, on every `longpoll_auth_recheck` (calls `revalidate`, ends with `Unauthenticated` if revoked), and every `feed_safety_recheck` (re-read, one short query); on `wait` expiry return an empty page with the same cursor.

### FeedHorizon (called by 02's purge)
`raise_purge_horizon(tx, account, seq)` is the last statement of the purge transaction. It is a plain `UPDATE`; zero rows (the feed row of an account being erased) is not an error.

### EpochAdmin (operator/system scope)
`marker_status() -> MarkerStatus{db_epoch, marker: Absent | Present(epoch)}` is read by `serve` at start (refuse to start when the database epoch is below the marker) and by the doctor check.

`bump_epoch(now) -> new_epoch` sets `epoch = greatest(epoch + 1, unix_seconds(now))` for every row in one transaction, then calls `EpochMarker::write(new_epoch)` (04) after COMMIT; if the marker write fails the call returns the error, and the operator reruns the idempotent command (a second bump only raises the epoch again). `head_report()` returns `max(epoch)` and per-account heads for `doctor`; the missing-bump check is the `DoctorCheck` described at the end of this section (decision 5). `AccountLifecycleHook` impl: `initialize(tx, account)` inserts `account_feeds` with the installation epoch it picks itself (`max(epoch)` or clock seconds on an empty install; the latter case also writes the marker via `EpochMarker`); `erase_batch(account, limit) -> EraseProgress{remaining}` deletes the row (`remaining = 0`); 12 registers this hook late in the list so it runs after the tree's.

**Doctor check** (`DoctorCheck` impl, decision 5): fails when `.epoch` is absent on a volume that holds blobs, or when `EpochMarker::read` differs from the database `max(epoch)`; the operator then runs `admin epoch-bump`.

## Ports (crate `contracts`)
| Direction | Port | Counterpart | Used for |
|---|---|---|---|
| Provides | `FeedStamp`, `FeedNotifier`, `FeedHorizon` | 02 | counter, wake, purge horizon inside the commit transaction |
| Provides | `ChangeFeed` | 11 | `GET /changes`, `GET /changes/head`; 11 passes 01's `PrincipalRevalidator` into `read` to end a held poll after revoke or account deletion |
| Provides | `EpochAdmin` (`bump_epoch`, `head_report`, `marker_status`) | 12 | `admin epoch-bump`, doctor head report |
| Provides | `DoctorCheck` impl (epoch marker) | 12 | `doctor`: DB epoch against the `.epoch` marker |
| Provides | `AccountLifecycleHook` impl | 01 (initialize), 07 (erase_batch) | feed row at account creation and deletion |
| Requires | `EpochMarker` (`write`, `read`) | 04 | `.epoch` marker after the epoch is set; read by the doctor check |

## 3. External API Specification (resources owned; routed by 11_http_api)
| Endpoint | Method | Scope | Rate limit | Description |
|---|---|---|---|---|
| `/api/v1/changes?cursor=E.S&wait=&limit=` | GET | SessionOrDevice | per-principal concurrent holds cap | page of changed nodes or empty after hold |
| `/api/v1/changes/head` | GET | SessionOrDevice | normal | current cursor for a new device (take head first, then list the tree) |
`resync_required` is HTTP 410 with stable code `resync_required` and an extra `reason` field (`epoch_changed`, `below_purge_horizon`, `cursor_ahead`); a malformed cursor is 422 `invalid_cursor`. The wait is bounded below the proxy and API timeouts. Response and request schemas in OpenAPI; clients ignore unknown fields.

## 4. Data Access Patterns
**Owned**: `account_feeds` (`account_id`, `epoch`, `seq`, `purge_horizon`), the `nodes.seq` column and `nodes_feed_idx (account_id, seq)`.
| Query | Frequency | Hot | Index |
|---|---|---|---|
| Stamp (counter row UPDATE + node stamp) | every tree change | Yes (serialises one account's commits at the tail) | PK, `nodes` PK |
| Feed read `seq > S` | every wake, every safety recheck | Yes | `nodes_feed_idx` |
| Head / horizon check | each request | Yes | PK |
No cache beyond the in-process notifier (a map of account to `Notify`, dropped when no waiter). Storage: one row per account. **Rollback**: no data to roll back; restore uses `bump_epoch`.
**Locking**: the counter row is locked only by `stamp`, as the last statement of the transaction; there is no lock-order contract with the other components (02 takes the account advisory lock in `publish`, move and restore only).

## 5. Implementation Details
**State**: in-memory only (notifier map). Nothing to rehydrate: after a restart held requests are gone and clients re-poll with the same cursor.
**DB use while holding**: none; each safety recheck takes a pooled connection for one query (NFT-RES-21: few connections, many held polls).
**Libraries**: sqlx 0.9, tokio (`Notify`, timers). No PgListener in v1.0: wake-up is the in-process notifier plus the safety recheck, as in the flows; NFT-RES-20 "listener loss" then reduces to "safety recheck catches up".
**Error handling**: typed results above; DB outage during hold returns 503 to the client (it re-polls); no swallow.

## 6. Configuration keys
`longpoll_wait_max` (30 s), `longpoll_auth_recheck` (2 s, must stay below 5 s minus cache TTL), `feed_safety_recheck` (10 s), `feed_page_limit_default` (500) and `_max`, `max_holds_per_principal` (4), `tombstone_horizon` (90 d, shared with 02 for the horizon). Tests use milliseconds.

## 7. Security notes
All reads scoped by the caller's `account_id` (cursor of one account is meaningless to another). The held principal is revalidated, so revocation or account deletion ends the poll with 401 within 5 s (AC-21). Responses contain names; they are served with `Cache-Control: no-store` by 11. No other identifying data. The counter row is never exposed.

## 8. Dependency graph
Implement after: helpers, `contracts`. Depends on no component crate. Parallel with every other component. 02's integration tests run against the real 05; its contract (stamp, notify, horizon) is stable and small.

## 9. ACs, tests, acceptance
**ACs [release]**: AC-5 (server share of latency: wake after COMMIT within ms) [v1.0]; AC-8 (order is commit order) [v1.0]; AC-21 (held streams end on revoke) [v1.0]; AC-35 (catch-up after suspend) [v1.0]; AC-41 (epoch, horizon, future cursor) [v1.0]; AC-1 (70% idle long-poll without connections) [v1.0]; AC-65 (epoch bump in the restore runbook; `doctor` flags a missing bump through the `.epoch` marker) [v1.0].
**Interaction AC folded in**: AC-69: with 50 concurrent commits on one account, a reader from cursor 0 sees every change exactly once, in order, no skipped or repeated `seq`. Mechanism: counter last, one statement, batch shares one `seq`. Test ID for AC-69 added in Step 5 (pending).
**Tests**: FT-P-07, FT-P-08, FT-P-13, FT-P-14, FT-P-58; NFT-PERF-01, 08, 09; NFT-RES-07, 08, 20, 21, 25; NFT-SEC-08; FT-N-30 (401 on revoke).
**Acceptance (observable)**: commit on A wakes B's held poll in well under the 5 s budget; stale cursor, other epoch and future cursor all answer 410 `resync_required`; `admin epoch-bump` makes every device resync; no `seq` gap under concurrent commits; a revoked device's held request ends with 401 within 5 s; 100 held polls use no more than a handful of DB connections.

## 10. Logging
ERROR stamp invariant violation (missing feed row); WARN safety recheck found changes a notify missed (counts a metric `feed_missed_wake_total`); INFO epoch bump (old, new), resync_required count per account in metrics only. Never names or paths.

## 11. Risks
In-process notifier does not fan out across API replicas; with two replicas only the safety recheck (10 s) delivers cross-replica changes, which breaks AC-5, so v1.0 runs one API process (decision 12; the notifier port keeps a PgListener implementation possible later). Hot counter row contention is bounded by the stamp-last rule and measured in NFT-PERF-09. A page that ends on a batch boundary can be large (500 entries). Stamping a node set that includes revived ancestors needs 02 to pass them all in `TouchedNodes`; clients must re-list a folder that turns live.
