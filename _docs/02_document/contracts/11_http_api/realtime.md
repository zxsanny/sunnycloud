# Contract: realtime — `GET /api/v1/realtime`, the pushed-frames channel of the web client

**Component**: 11_http_api (adapter code in 05_sync_feed and 08_media_jobs for the hooks of the section "What the server builds")
**Producer**: MVP roadmap `_docs/02_document/mvp_roadmap.md`, decision D5, package 5E
**Consumers**: web app (AZ-3938 file browser, AZ-3945 previews), tests of both languages
**Version**: 0.3.0 (protocol version in `Hello`: 1)
**Status**: implemented (stages 1 and 2): handshake, session loop, every frame of this document, close codes, limits, `media_type`, `ThumbState` pushes
**Last Updated**: 2026-11-20

## Purpose

The web client gets live state as pushed data frames over one WebSocket, in both directions, from the first version. No timer asks the server "anything new?", and no frame says "changed, read again": a frame carries the data. REST stays for first load, writes and the public API (rule: `coderule.md`, "No polling between a server and its clients"). The channel replaces, for the web only, the 2 s thumbnail polling of the design and the long-poll of `GET /api/v1/changes`.

Frames are binary, packed with `packbin` 0.5.1 (Rust crate on the server, npm package in the browser). Both sides keep the same field list; this document is that list.

## Verified facts (checked 2026-11-20, scratch area only, nothing in the workspace)

**packbin.** Crate `packbin =0.5.1` (checksum `a5e9ce26…cd3f`) and npm `packbin@0.5.1` (Node 22, `engines >=22`), both published. The local checkout `/develop/packbin/rust` says 0.1.0 and is not the published code. A scratch Rust crate and a TypeScript project packed the same 10 sample rows (all frames below plus a probe with every feature the task named: type byte, integers, utf8, list, flags-optional field, `times` group): **the bytes are identical** in both languages, each side decodes the other's bytes to the same rows, and an `esbuild --minify` bundle packs the same bytes (the TypeScript API reads member names from the accessor lambdas' source text, so a minified build had to be proven). `tsc --strict` with `skipLibCheck: false` accepts the `.d.ts` files (TypeScript 5.9). `bigint` is required for `u64`. The whole library is about 54 kB minified plus 8 kB of `@noble/hashes 2.4.0` (a dependency of the unused frame-session code).

**What protects the server when it unpacks a client frame.**
- Every unreadable frame is an error value, never a panic or a throw: Rust `UnpackError` (`Short{field,needed,left}`, `Trailing{left}`, `Type{expected,actual}`), TypeScript `{ok:false, field, needed, left}` (trailing bytes: `needed 0`) or `{ok:false, actual}` for an unknown first byte. Truncated, trailing, empty and wrong-type frames were all refused in the scratch run.
- Round limits (`maxRounds` 65,535, `maxSlots` 1,048,576) bound `times` and `repeat`; a count larger than the bytes left is refused; strings are validated (invalid compact text is refused). `maxBytes` (16 MiB) bounds **pack** only.
- **Unpack has no size limit of its own**: README "Untrusted input" says to cap the packet length where it is read from the network. A 128 KB list of 65,535 items parsed in 9 ms (release) and 67 ms (debug). So the cap is ours: WebSocket `max_message_size` (section "Frame size").
- Client-to-server frames use only fixed-width integers (no `utf8`, `list`, `dict`, `times`, `repeat`): the smallest possible untrusted surface. Keep it so.

**Awkward for a server fed by a browser** (all verified):
1. An unknown first byte in the typed dispatch (`BinaryPacker::unpack_with`) is `Type{expected: 0, actual}`: `expected` is a placeholder. The server therefore reads `frame[0]` itself, picks the scheme, and calls `unpack` (then `expected` is the real number).
2. `MapScheme`, `Values` and the typed `Scheme<T>` are **not `Send` and not `Sync`** (`Rc<str>` inside). They cannot be shared state or live across an `.await` of a spawned task. Phase 2 builds them in `thread_local!` and encodes/decodes in synchronous functions that return plain structs.
3. `UnpackError` and `PackError` have `Debug` only (no `Display`, no `std::error::Error`): no `?` into `Box<dyn Error>`, log with `{:?}`.
4. The typed `Scheme<T>` has optional fields only for `u8` and `u16`; optional `u64`, `bytes`, `utf8` need the untyped `MapScheme` with `Values` (used here). Booleans are `Value::U8(0|1)`.
5. A scheme is built from explicit field numbers; a wrong anchor panics at construction (found twice in the scratch run). Build at startup in a test, not on first use.
6. `unpack` ignores an unused flag bit (bit 7 of `Change.present` set: accepted). Senders write 0; receivers ignore it.
7. The compact `utf8` format has frozen tables and no version byte (Ukrainian "Старі фото", 10 characters, is 8 bytes). Both languages agree on it (a sample is in the vectors).

**axum `ws` feature.** `axum =0.8.9` features `["ws"]` adds `tokio-tungstenite 0.29.0` and `tungstenite 0.29.0` (neither is in `src/Cargo.lock` today), plus `sha1 0.10.7` (the lock has 0.11.0 only), `rand 0.9.5`, `rand_chacha 0.9.0`, `rand_core 0.9.5`, `getrandom 0.3.4`; the rest (`data-encoding`, `base64 0.22`, `thiserror 2`, `bytes`) is already locked. `cargo deny check advisories licenses bans sources` with the repo's `src/deny.toml` on a scratch crate with exactly that dependency: all four ok (`multiple-versions` is `warn`). `scripts/ci/check-pinning.sh` scans workflows and images, not Cargo. The libraries default to a 64 MiB message and 16 MiB frame limit and an unbounded write buffer: both must be set explicitly.

## Endpoint

`GET /api/v1/realtime`, a WebSocket upgrade (RFC 6455, HTTP/1.1 only; the Caddy upstream of the same host must not force h2c for this path). One registry entry like every route, so the router, the OpenAPI document and the parity tests keep one source (`api_v1_surface.md`). Auth class **`Session`** (cookie or bearer session; a device token gets 403); body class `None`; no subprotocol; no query parameters (the resume point is a frame, not a URL, so no cursor reaches logs).

| Step | Rule | Failure |
|------|------|---------|
| Version gate | runs first as on every route; a browser handshake has no `X-Sunnycloud-Client` and passes with its cookie (`server_info.md`), so `Hello.version` is the only version check of this channel | 426 |
| Auth | the session cookie, same code as every `Session` route | 401 |
| **Origin** | for a cookie-authenticated handshake the `Origin` header must be present and equal `public_url` (scheme, host, port; the canonical rule of `csrf/origin.rs`). `Sec-Fetch-Site` is not accepted instead: browsers always send `Origin` on a WebSocket handshake. `SameSite=Lax` does not cover a sibling subdomain (same site), so the check is needed | 403 `csrf_rejected`, no upgrade |
| Connection cap | at most `realtime_max_connections_per_account` (proposed default 8) open connections per account, counted by a 20-line counter in `realtime/cap.rs` (05's `HoldCap` has the same shape, but 11 may not depend on a component crate). The number is a field of `RealtimeLimits` with the default 8, not a config key (`sc_config` is outside the package) | 429 `rate_limited` with `Retry-After` |
| Upgrade | `max_message_size` and `max_frame_size` 1,024 bytes, small write buffer, no unmasked frames | 400 when the upgrade headers are wrong |

CSRF today covers only POST, PUT, PATCH and DELETE (`csrf/mod.rs`: GET routes get no layer), so the Origin check is a new guard on this route, reusing the same origin parser (one rule, not two). Bearer handshakes (native clients) carry no cookie and skip the Origin check, as bearer requests skip CSRF; **no native client uses this endpoint in the MVP and the bearer handshake has no test**.

## Connection lifecycle

1. After the 101 the server sends `Hello`. Live pushes are already registered at that moment, so anything the client reads over REST **after** `Hello` cannot miss a later change.
2. The client sends `Subscribe{epoch, seq}` (it may send it before `Hello` arrives). The server first sends the changes after that point (catch-up), then keeps pushing (live). One `Subscribe` per connection is expected; a second one restarts the catch-up from its point (replace, no error).
3. The client sends `Ping` every `Hello.ping_interval_s` (30) and treats a missing `Pong` after 10 s as a dead socket (close, reconnect). The server answers each `Ping` with the same nonce in a `Pong`.
4. The server also sends a WebSocket control `Ping` every `ping_interval` (30 s); browsers answer it at protocol level without JavaScript, so a hidden tab whose timers are throttled is not cut at the proxy's 100 s idle limit. The answer counts as inbound traffic for the idle rule. Idle policy: no inbound frame (data or control) for 90 s (3 intervals), or no `Subscribe` within 10 s of `Hello`, ends the connection with 4408. A proxy cuts idle sockets at about 100 s (Cloudflare); with 30 s traffic in both directions nothing is idle. 
5. Server shutdown (`ApiServices::feed_shutdown`, cancelled after the listeners stop accepting): `Close 1001`. After the upgrade hyper no longer drives the socket, so `close_connections` is expected not to reach it; the task watches that token itself (RT-14 proves it).
6. **Cap coupling.** Every subscribed socket parks one hold of its session (`max_holds_per_principal`), so that key must be at least `max_connections_per_account` (the MVP environment sets 20; the default is 4). A socket past the hold cap is closed 1013 (`rt07b`).
7. The session is revalidated by the held read (every `longpoll_auth_recheck`, 2 s, through `PrincipalRevalidator`); a refused principal ends the connection with 4401 and no further frame.

**Close codes** (the client reconnects with backoff 1 s to 30 s with jitter after every code except 4401, where it goes to sign-in, and 1002, a bug: it reports and reloads):

| Code | Meaning | Sent when |
|------|---------|-----------|
| 1000 | normal | either side leaves on purpose |
| 1001 | going away | server shutdown |
| 1002 | protocol error | unknown first byte, undecodable frame, bad value (`seq` above `i64::MAX`), a frame type that is server-to-client only |
| 1003 | unsupported data | a text message (all frames are binary) |
| 1009 | message too big | a message above 1,024 bytes (tungstenite reports the overflow as a read error without a Close frame, so the server sends the 1009 itself; tested) |
| 1011 | internal error | unexpected server failure (details only in the log, correlation id) |
| 1013 | try again later | the held read refused with `TooManyHolds` after the upgrade (the cap coupling above) |
| 4401 | session ended | principal revoked or expired |
| 4408 | idle | the idle policy above |
| 4429 | too slow | a send did not complete in 10 s |

## Frames

Conventions: a frame is one binary WebSocket message; byte 0 is the `packbin` scheme number. Integers are little-endian. `id` is a UUID as its 16 bytes in textual order; `hash` is a 32-byte BLAKE3 content hash (the bytes behind the 64 hex digits of the REST API). `utf8` is packbin's compact text. `u64` is JavaScript `bigint`. A field marked `?` is in the `present` flags byte (bit 0 is the first `?` field). **A released frame never changes its field list**; a change is a new number (and a new `Hello.version` when an old client cannot ignore it).

| Number | Direction | Frame |
|--------|-----------|-------|
| 0x01 | client to server | `Subscribe` |
| 0x02 | client to server | `Ping` |
| 0x03 to 0x0F | client to server | free (core) |
| 0x10 to 0x1F | client to server | reserved: upload progress, share actions |
| 0x20 | server to client | `Hello` |
| 0x21 | server to client | `Change` |
| 0x22 | server to client | `ThumbState` |
| 0x23 | server to client | `Reset` |
| 0x24 | server to client | `Pong` |
| 0x25 to 0x2F | server to client | free (core) |
| 0x30 to 0x3F | server to client | reserved: upload progress, share events |
| 0x40 to 0x5F | both | reserved: vault (v1.2) |

Everything from 0x60 to 0xFF is unassigned. The server closes on an unknown first byte from the client (1002). **The client ignores a frame with an unknown first byte** (logs it), so a newer server can add frames for a newer SPA without breaking a cached older one.

### `Subscribe` 0x01 (C to S)

| Field | Type | Meaning |
|-------|------|---------|
| `epoch` | u64 | resume point, epoch of the cursor `E.S` of `GET /changes` |
| `seq` | u64 | resume point, `S`: the server sends every node whose feed `seq` is greater |

`01 | 01 00 00 00 00 00 00 00 | 2a 00 00 00 00 00 00 00` = epoch 1, seq 42 (17 bytes). A fresh client uses the cursor of `GET /api/v1/changes/head`.

### `Ping` 0x02 (C to S) and `Pong` 0x24 (S to C)

`nonce` u32, echoed. `02 | 07 00 00 00` and `24 | 07 00 00 00` (5 bytes each). Heartbeats carry no state: they keep the path alive and let the browser see a dead socket (JavaScript cannot see WebSocket control frames).

### `Hello` 0x20 (S to C)

| Field | Type | Meaning |
|-------|------|---------|
| `version` | u16 | protocol version, 1 |
| `ping_interval_s` | u16 | seconds between client `Ping`s (30) |
| `max_client_frame` | u32 | largest message the server accepts (1024) |

`20 | 01 00 | 1e 00 | 00 04 00 00` (9 bytes). A client that does not know `version` closes (1000) and shows "reload to update".

### `Change` 0x21 (S to C): the latest state of one node

| # | Field | Type | Source |
|---|-------|------|--------|
| 0 | `epoch` | u64 | feed position: epoch |
| 1 | `seq` | u64 | feed position: `ChangeItem.seq` |
| 2 | `node_id` | id | `ChangeItem.node_id` |
| 3 | `kind` | u8 | 0 file, 1 folder |
| 4 | `thumb` | u8 | 0 none, 1 pending, 2 ready, 3 failed (`ThumbState` of the port; a client treats an unknown value as none) |
| 5 | `name` | utf8 | `ChangeItem.name` (at most 255 bytes) |
| 6 | `present` | flags | bit 0 `parent_id` id, 1 `revision_id` id, 2 `size` u64, 3 `content_hash` hash, 4 `media_type` utf8, 5 `deleted` (bool: a tombstone), 6 `end` (bool: last item of its `seq` batch), 7 unused |

`parent_id` is absent for a root, `revision_id`, `size`, `content_hash` and `media_type` for a folder or a tombstone, as in `ChangeItem`. Examples (`|` separates fields; names are the compact text):

```
live file, 131 bytes, present = 5f (bits 0 to 4 and 6):
21 | 01 00 00 00 00 00 00 00 | 2a 00 00 00 00 00 00 00 | 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f 10
 | 00 | 01 | 0d 2b ff fa df 8f ff 7f a8 88 a3 f5 42 ec          epoch 1, seq 42, node, file, pending, "IMG_0001.HEIC"
 | 5f | 30..3f (parent) | 50..5f (revision) | ce ca 23 00 00 00 00 00 (size 2345678)
 | 20..3f (content hash, 32 bytes) | 08 69 6e 9c 0f fe 95 0b b0   (media type "image/heic")
tombstone folder, 61 bytes, present = 21 (parent, deleted):
21 | 01 00.. | 2b 00.. | 02 03 .. 11 | 01 | 00 | 08 83 e5 9c ab 21 fc 67 60 ("Старі фото") | 21 | 30..3f
minimal folder, 39 bytes, present = 00:
21 | 01 00.. | 2c 00.. | 03 04 .. 12 | 01 | 00 | 02 10 61 ("a") | 00
```

**Why `end`.** The feed never splits a batch that shares one `seq` (`ChangePage` ends on a `seq` boundary), but frames of one batch can reach a client one by one, and a connection can die between them. The client moves its resume point to `(epoch, seq)` only after a `Change` with `end`; resuming at a `seq` it only half received would skip the rest. Applying a half batch is harmless (idempotent).

### `ThumbState` 0x22 (S to C): a preview became ready or failed

| Field | Type | Meaning |
|-------|------|---------|
| `content_hash` | hash | the derived outputs are keyed by account and content hash, not by node |
| `state` | u8 | 2 ready, 3 failed (1 and 0 are never pushed; same numbers as `Change.thumb`) |

`22 | 20 21 .. 3f (32 bytes) | 02` (34 bytes). The client updates every node it holds with that `content_hash`. Sent only to the connections of the owning account.

### `Reset` 0x23 (S to C): the resume point cannot be trusted

| Field | Type | Meaning |
|-------|------|---------|
| `reason` | u8 | 1 epoch changed, 2 below the purge horizon, 3 cursor ahead (the three `ResyncReason` values of `GET /changes`), 4 lagged (pushed events were dropped for this connection) |
| `epoch`, `seq` | u64, u64 | the account's head now (`ChangeFeed::head`) |

`23 | 02 | 01 00 00 00 00 00 00 00 | 84 03 00 00 00 00 00 00` = reason 2, head 1.900. After a `Reset` the server sends nothing until a new `Subscribe`. The client reloads its listing and thumbnail states once over REST (first load) and subscribes at the `Reset` position: changes between that head and the reload are re-sent (duplicates, idempotent), none is lost.

## Semantics

- **Order.** Frames of one connection follow the feed order of the account: `seq` never decreases, items of one `seq` are ordered by node id. `Change` frames and `ThumbState` frames come from one task in the order it handles them.
- **Delivery.** `Change`: at least once, per node latest state (a node changed twice while the client was away arrives once). The client applies a `Change` only if its `seq` is not older than the one it holds for that node. `ThumbState`: pushed live to every open connection of the account; the server keeps no replay (see open question 1). A frame is never "changed, read again".
- **Catch-up then live.** The server reads pages after the resume point until none is newer (page size `feed_page_limit_default`, 500), then waits for the next change and sends it. The client cannot tell the two apart and does not need to.
- **Thumbnail state in `Change`.** One `ThumbnailStateSource::states` call per page fills `thumb` for the page's media files. (`ready` there means the thumb row exists, so a `Change` can say ready a moment before the `ThumbState` that follows the stored preview.) `ThumbState` and `Change` for one hash come from one task, so a later frame is never staler than an earlier one.
- **Client procedure** (first load): `GET /changes/head`, then the listing, then open the socket, wait for `Hello`, `Subscribe` at the head, and for the visible media `POST /thumbnail-states` once after `Hello`. After a reconnect: `Subscribe` at the stored resume point and one `POST /thumbnail-states` for the still pending hashes (ready flips during the gap are not replayed, section "Open questions", 1).
- **Malformed input.** An unknown first byte, a frame that does not unpack, an empty message, a value that does not fit (`seq` above `i64::MAX`) or a server-only type from the client closes with 1002; a text message 1003; a message above `max_client_frame` 1009. The close reason is a fixed word, never the bytes received. No frame is answered with an error frame: an error means the client has a bug or a version mismatch.
- **Slow consumer, backpressure.** There is no per-connection queue. One task per connection sends inline; a client that stops reading stops that task (TCP backpressure), so memory per connection is one page of frames. Each send has a 10 s timeout, then the connection is dropped (4429 when the close frame can still be sent); the socket's 60 s write-idle timer (`write_idle.rs`) is a second guard. Pushed `ThumbState` events wait in a bounded broadcast channel (1,024 events); a receiver that lags loses events and the connection gets `Reset` reason 4. A drop never blocks any other connection.
- **Frame size.** All client frames are at most 17 bytes; the limit 1,024 leaves room for the reserved client frames. Server frames are at most about 700 bytes (name 255 bytes, media type from the fixed sniffed set).

## What the server builds (each item outside `src/components/11_http_api/src/realtime/` is named)

1. **One source of truth.** The connection task calls `ChangeFeed::read(principal, revalidator, cursor, wait, limit)` in a loop with `wait = longpoll_wait_max` — the same call, the same SQL (`READ_PAGE_SQL`), the same cursor rules and notifier wake (`InProcessNotifier`, `FeedNotifier::notify` after COMMIT in 02), safety recheck and revocation check as `GET /changes`. The task never reads the database itself and the web needs no new feed query. An empty page after the wait is re-armed inside the server; no client is involved. Cost to name: each connection uses one hold (`max_holds_per_principal`, default 4 per session, 20 in the MVP environment), so a session keeps at most that many sockets; and one database read per `feed_safety_recheck` (10 s) per connection.
2. **`ChangeItem.media_type`** (`contracts/src/feed.rs`; `READ_PAGE_SQL` selects `r.media_type`). Added; the `/changes` JSON does not emit it (`registry/feed/wire.rs` lists its members), proved by a test that compares the body byte for byte with and without media types.
3. **Preview events: `ThumbNotifier`, the one new primitive of the package.** Nothing was notified when a preview landed: `DerivedStore::store_row` (08) inserts the `derived` row and closes the job, and the feed (`nodes.seq`) is stamped by tree transactions only. `contracts::ThumbNotifier` has `notify(ThumbEvent{account, content_hash, state})` (non-blocking, called after the commit) and `subscribe()` (a `ThumbSubscription` whose `next()` is an event or a lag notice). `sunnycloud_sync_feed::InProcessThumbNotifier` implements it with one process-wide `tokio::sync::broadcast` (1,024 events); each socket subscribes **before it sends `Hello`** and keeps the events of its own account. 08 announces `Ready` after the commit of `store_row` when the job closed and its kind produces a `thumb` (a still job closes when thumb and preview are both stored), and `Failed` when such a job dies in `JobQueue::fail`, in `reap` or in `claim` (the `Closed` row of those statements now carries the account and the content hash). `serve` builds one notifier, gives it to 08 (`with_thumb_notifier`) and to 11 (`HttpServer::with_thumb_events`). Rejected: stamping the nodes that share the hash into the feed (replayable, but 08 would write 02's table across a component border, find nodes by hash, and serialise on the account feed row); reusing `FeedNotifier` (it wakes a reader with a number, a preview event needs its payload); a per-account map of channels (a global channel filtered by account is 15 lines and needs no cleanup).
4. **Accept loop.** `accept.rs` serves connections with `serve_connection_with_upgrades`. An upgraded socket leaves hyper's connection list (and, when the connection future ends at the upgrade, the `max_connections` gate: not measured), so the account cap is the limit that is certain; the task watches `feed_shutdown` itself.
5. **Registry.** `registry::validate` allows success 101; the CSRF read matrix lists this route among the reads that do not run (a plain GET is 400); the OpenAPI document lists it with response 101 and class `Session`.
6. **Cargo.** `axum` feature `ws`; `packbin =0.5.1`; dev-dependencies `tokio-tungstenite =0.29.0` and `futures-util`. The account cap is a field of `RealtimeLimits` (default 8), not a config key.

**Coexistence with `GET /changes`.** Desktop and mobile clients keep the long-poll until they move (roadmap section 7); the web uses only the socket, plus `GET /changes/head` for first load. Both paths end in `ChangeFeed::read`, so a change reaches both by the same code and a divergence is not possible by construction; test RT-16 asserts it. When the last native client has moved, the long-poll route is deleted and one pipeline remains.

## Test plan

Rust tests use the real `sunnycloud serve` on PostgreSQL 18 (`src/server/tests/pipeline_support`, as `pipeline_change_feed.rs`) with a WebSocket client; AAA style.

Status: RT-01 to RT-09, RT-13 (with RT-13b failed and RT-13c isolation), RT-14 and RT-07b (the cap coupling) are `src/server/tests/pipeline_realtime.rs` on the real stack, the worker played over the internal listener; RT-10 (oversize), RT-11 (slow consumer), RT-12 (idle), RT-15 (lag: scripted `ThumbWake::Lagged`), RT-17 (document) and the handshake, close-code and thumb-frame cases are `src/components/11_http_api/tests/it/realtime/` over a scripted feed; RT-16 is RT-01 (the socket equals `/changes` and the node, `media_type` included) plus `datapath::feed` (the `/changes` body does not change). 08 has `thumb_announce` (ready, failed, retry, reaper, other kinds) and 05 reads the media type from the revision. V-01 and V-02 are `realtime/frames/tests.rs` reading `src/components/11_http_api/tests/fixtures/realtime_frames.txt`; the TypeScript side (V-01 to V-03) reads the same file.

| ID | Test | Asserts |
|----|------|---------|
| RT-01 | subscribe at head, commit a file | a `Change` within 5 s whose fields equal `GET /nodes/{id}` and the `/changes` item |
| RT-02 | resume | receive up to `end`, disconnect, 3 commits, reconnect at that point: the 3 latest states in order, nothing older |
| RT-03 | batch commit of 3 nodes, subscribe one `seq` earlier | 3 frames, only the last has `end` |
| RT-04 | two connections of one account, one of another | both get the frame, the other account gets none (isolation) |
| RT-05 | Origin missing, foreign, `null`, sibling subdomain, right | refused 403 without an upgrade; right one upgrades |
| RT-06 | no cookie; session revoked while connected | 401; close 4401 within 5 s |
| RT-07 | cap reached | the next handshake 429 with `Retry-After`; closing one frees a slot |
| RT-08 | cursor ahead, epoch bump, below horizon | `Reset` reasons 3, 1, 2 with the head; a new `Subscribe` then works |
| RT-09 | malformed: text, unknown byte, truncated, trailing byte, `seq` above `i64::MAX`, server-only type | close 1003 / 1002; server keeps serving others |
| RT-10 | message of 2 KiB | close 1009 (or connection ended) |
| RT-11 | client never reads, 5,000 commits | 4429 or drop within the send timeout; other connections unaffected; memory bounded |
| RT-12 | silence for the idle limit (short configured value); pings only | 4408; with pings it stays open |
| RT-13 | upload a real JPEG, then a corrupt one | `Change` has `thumb` pending, then `ThumbState` ready; for the corrupt one, failed |
| RT-14 | SIGTERM | close 1001 |
| RT-15 | force a lagged broadcast receiver | `Reset` reason 4 |
| RT-16 | the same commit read by `GET /changes` and by the socket | identical node fields |
| RT-17 | OpenAPI document and registry parity | the operation exists, success 101, existing parity tests green |
| V-01 | **shared vector file** `src/components/11_http_api/tests/fixtures/realtime_frames.txt` (`name hex` lines, the 9 frame vectors of this document plus the probe) | Rust and TypeScript both pack the rows to exactly those bytes and unpack the bytes to those rows |
| V-02 | hostile lines of the same file (`bad_*`: empty, `7e`, truncated, trailing byte, `Change` name length that lies) | both languages refuse, naming the field; no panic or throw |
| V-03 | V-01 and V-02 run in the web unit tests and against the Vite production bundle | minification does not change the bytes |

The vector lines are in the appendix; the phase 1 scratch programs (Rust, TypeScript) that produced and cross-checked them are outside the repository.

## Decisions of the owner (2026-11-20) and what is still open

1. Missed `ThumbState` during a reconnect gap: v1 is one REST `POST /thumbnail-states` for the client's pending hashes after a reconnect (counts as first load); no server replay, no `Watch` frame.
2. `ThumbState` is pushed when the job closes (thumb and preview both stored).
3. Per-account socket cap 8; `max_holds_per_principal` must not undercut it (section Connection lifecycle, point 6).
4. Idle 90 s, `Subscribe` within 10 s, send timeout 10 s, broadcast 1,024 events, close codes 4401, 4408, 4429 as proposed.
5. `ChangeItem.media_type` in `contracts` and in `READ_PAGE_SQL`; the `/changes` JSON stays unchanged (done, tested byte for byte).
6. The new `ThumbNotifier` port in `contracts` and the hooks in 08 (`store_row` close, `fail`, `reap`, `claim`) (done).
7. No inbound rate limit in v1.
8. The server sends a control `Ping` every 30 s besides the application `Ping`/`Pong`.
9. The bearer handshake is allowed by the class and untested.

Reconnect procedure (decision 1): after a reconnect the client subscribes at its stored resume point and, once `Hello` arrived (so the preview channel is registered), asks `POST /thumbnail-states` once for the hashes of its still pending nodes; ready or failed flips during the gap are not replayed. Still open: whether an upgraded socket keeps a `max_connections` slot (the connection future ends at the upgrade, so it probably does not; the account cap is the limit); the explicit wait for open sockets at shutdown (the 1001 close arrived before exit in every run, but nothing waits for it).

## Appendix: byte vectors (produced by Rust and by TypeScript, identical; the canonical copy is the fixture file named in the test plan)

Rows: `subscribe` epoch 1 seq 42; `ping` and `pong` nonce 7; `hello` 1, 30, 1024; `thumb_state_ready` hash 20..3f; `reset_below_horizon` reason 2, epoch 1, seq 900; the three `change_*` rows as in the examples above (id bytes count up from the first byte shown); `probe_all` is the packbin check scheme 0x7f (not a frame), kept to prove `list`, flags and `times` in both languages.

```
subscribe 0101000000000000002a00000000000000
ping 0207000000
pong 2407000000
hello 2001001e0000040000
thumb_state_ready 22202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f02
reset_below_horizon 230201000000000000008403000000000000
change_file 2101000000000000002a000000000000000102030405060708090a0b0c0d0e0f1000010d2bfffadf8fff7fa888a3f542ec5f303132333435363738393a3b3c3d3e3f505152535455565758595a5b5c5d5e5fceca230000000000202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f08696e9c0ffe950bb0
change_folder_tombstone 2101000000000000002b0000000000000002030405060708090a0b0c0d0e0f101101000883e59cab21fc676021303132333435363738393a3b3c3d3e3f
change_root_child_minimal 2101000000000000002c00000000000000030405060708090a0b0c0d0e0f101112010002106100
probe_all 7f092c010000ffffffffffffffff062bfcfee119b2020001000200055a00020a000000140000001e0000002800000007
```

## Change Log

| Version | Date | Change |
|---------|------|--------|
| 0.1.0 | 2026-11-20 | First draft, phase 1: protocol, frames with verified bytes, test plan, open questions |
