# Sync core integration suite against the real server stack

**Task**: AZ-3971_sync_real_server_suite
**Name**: Real-server integration suite
**Description**: Run the sync core's protocol cases against the real server container with real PostgreSQL: large upload finish time, download resume, base revisions, batch replay, resync after a restore, listings and lookups, lost-commit recovery, cancellation and memory, plus the malicious-server security test
**Complexity**: 3 points
**Dependencies**: AZ-3949_sync_nothing_to_send_spike, AZ-3959_http_sync_client_connect, AZ-3960_sync_tree_operations, AZ-3962_large_upload_executor, AZ-3963_small_upload_batch, AZ-3964_verified_download, AZ-3965_feed_client, AZ-3966_token_lifecycle, AZ-3967_sync_host_traits_foundation, AZ-3742_test_harness_skeleton, AZ-3804_version_gate_server_info, AZ-3828_identity_admin_handlers, AZ-3829_tree_transfer_feed_handlers, AZ-3789_composition_root; notes: E-31/c05 (test epic E-31, decomposed at greenfield Step 9); E-31/c06 (test epic E-31, decomposed at greenfield Step 9); E-31/c09 (test epic E-31, decomposed at greenfield Step 9); E-31/c10 (test epic E-31, decomposed at greenfield Step 9); E-31/c11 (test epic E-31, decomposed at greenfield Step 9); E-31/c15 (test epic E-31, decomposed at greenfield Step 9); E-31/c18 (test epic E-31, decomposed at greenfield Step 9); E-31/c19 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 13_sync_core
**Tracker**: AZ-3971
**Epic**: AZ-3650 (E-23)
**Ref**: E-23/c18a

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/sync_api.md` — operations under test.
- `_docs/02_document/tests/environment.md` — profiles `core`, `C-LARGE`, shaping profiles, groups.

## Problem

The executor tasks (E-23/c08..c14) prove their rules against the scripted fake. The fake cannot prove what only the server knows: that a 50 GiB commit finishes in constant time (AC-4), that a resumed download verifies against stored data (AC-7), that two real clients get a server-chosen conflict copy (AC-8), that a killed batch leaves 0 duplicates in the database (AC-40), that a restored database answers `resync_required` (AC-41), and that a lost commit response heals to one revision (AC-78 contribution). The row "Integration suite against the real server stack and perf tests" (E-23/c18) is split: this task runs the integration and security legs; performance and acceptance tests are E-23/c18b, because together they exceed 5 points (real-server legs of eight tests, four performance tests on reference hardware, two acceptance tests).

## Outcome

- The real-server legs of IT-13-3, IT-13-5, IT-13-6, IT-13-7, IT-13-12, IT-13-14, IT-13-16 and all of IT-13-18 pass against `tests/run.sh --profile core` (and `C-LARGE` for 50 GiB).
- ST-13-3 (malicious server) passes against the scripted fake.
- A sentinel-token and cancellation pass over every operation on the real stack, so no operation bypasses the shared request path.

## Scope

### Included
- Suite setup: account via invite token, golden snapshot, one account and folder prefix per test, fault tests from a reset stack (group `X`), teardown by purging the account through `e2e-ctl`, logs to `e2e-results/` (never committed), verbose test output.
- The real-server legs listed above with their data sets (DS-01 subset, DS-09, DS-10, DS-23) and shaping (`shape-wan`, `shape-flaky`).
- ST-13-3 cases on the fake: cross-host redirect, extreme `ServerInfo` values, `next_cursor.seq = u64::MAX`.

### Excluded
- PT-13-1..4, AT-13-1, AT-13-2 (E-23/c18b).
- The system scenarios FT-*/NFT-* (E-31) and the client pipelines (AZ-3668..AZ-3674).

## Acceptance Criteria

**AC-1: Large upload finishes in constant time (AC-4)**
Given DS-09 (5 GiB) in `C` and DS-10 (50 GiB sparse-hybrid) in `C-LARGE`, `shape-wan`
When each is uploaded
Then the result is `Published`, the server file hash equals the local BLAKE3, and the time from the last block's 204 to `Published` is at most 2 s for both sizes

**AC-2: Download resumes at a verified boundary (AC-7)**
Given DS-09 on the server and a TCP reset at 50% (`shape-flaky`)
When the download continues
Then bytes requested after the reset are at most 2.5 GiB + 2 MiB, each block reaches the sink once, and the result is `Verified` with `file_hash == content_hash`

**AC-3: Base revisions with two clients (AC-8, AC-70)**
Given clients A and B on one account, A holding rev1 of `f.txt` and B uploading rev2
When A uploads with `base_revision = rev1`, then deletes with rev1, then moves the parent, and a third upload collides with an existing name
Then A gets `ConflictCopy{node, copy}` with a server-chosen name, then `EditedSince{current: rev2}`, the proxy log shows the move without `base_revision` and with the node id kept, and the collision yields `ConflictCopy` with exactly 1 commit request

**AC-4: Batch replay leaves no duplicates (AC-40)**
Given 300 files (DS-23 sizes included) and kills after `save_batch`, after 150 stagings and after `batch_commit` was sent
When the client restarts each time and finishes
Then the server holds 300 files with 0 duplicates, and the 2 MiB + 1 file took the block path

**AC-5: Resync after a restore (AC-41)**
Given a client polling at cursor `1.500` while the server is restored from an older `pg_dump` (`e2e-ctl`, group `X`)
When the next poll returns
Then the result is `ResyncRequired{reason}` and the resync helper's request log shows `head_cursor` before the first `list_children`

**AC-6: Listings and lookups (AC-31, AC-46)**
Given 2,500 uploaded files (DS-01 subset)
When they are listed in pages and 2,500 known plus 10 unknown hashes are looked up
Then every file item carries its correct BLAKE3 and folders none, pages chain to `next = None`, and 3 lookup calls (1,000, 1,000, 510) return 2,500 `RevisionRef` with `client_meta`

**AC-7: Lost commit response heals (AC-78 contribution, AC-46)**
Given the proxy drops the commit response once (`e2e-ctl`)
When the client recovers for (A) a new revision of an existing node and (B) a brand-new file
Then (A) yields `NothingToSend{revision}`, 1 revision and 0 block bytes re-sent; (B) yields `NothingToSend{revision}`, 1 revision, 0 block bytes re-sent, no conflict copy (AZ-3949)

**AC-8: Cancel and memory on the real stack (AC-52)**
Given 3 concurrent 2 GiB uploads and RSS sampled every 1 s during a 5 GiB upload
When `cancel()` is called at 5 s
Then all three return `Cancelled` within 1 s and send nothing afterwards, and RSS stays at or below 150 MB

**AC-9: Malicious server is contained (AC-54)**
Given the scripted fake serving a 302 to `https://evil.test/`, `ServerInfo` with `small_file_max = 2^63`, `batch_max_entries = 0`, `max_lanes = 0`, a negative `longpoll_max_wait`, and a page with `next_cursor.seq = u64::MAX`
When the client meets each
Then `evil.test` receives 0 `Authorization` headers, each case ends in a typed error or a documented clamp within 2 s, there is no panic or hang, and allocation stays at or below 64 MiB

**AC-10: No operation bypasses the request path**
Given sentinel tokens and DEBUG logs across AC-1..AC-8
When the logs, error texts and request URLs of the whole run are searched
Then they contain 0 sentinel occurrences

## Non-Functional Requirements

**Performance**
- Time budgets from 13 tests: IT-13-3 20 min (5 GiB) and 4 h (50 GiB, `C-LARGE`); IT-13-5 10 min; IT-13-18 5 min.

**Reliability**
- `C-LARGE` runs on its self-hosted runner; without it AC-1's 50 GiB leg is `NOT-RUN`, never PASS.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (integration suite) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-09 `C`, DS-10 `C-LARGE`, `shape-wan` | Upload | `Published`; <= 2 s finish | IT-13-3, FT-P-03, NFT-PERF-06 |
| AC-2 | DS-09, `shape-flaky` | Download with reset | <= 2.5 GiB + 2 MiB; verified | IT-13-5, FT-P-02, NFT-RES-02 |
| AC-3 | Two clients, proxy log | Upload, delete, move, collision | Typed outcomes; no base on move | IT-13-6, FT-P-10, FT-P-12, FT-N-42 |
| AC-4 | 300 files, kills | Batch replay | 300 files, 0 duplicates | IT-13-7, FT-N-38, NFT-RES-22 |
| AC-5 | `e2e-ctl` restore, group `X` | Poll | `ResyncRequired`; head first | IT-13-12, FT-P-13, NFT-RES-07 |
| AC-6 | DS-01 subset | Listing and lookup | Hashes; 3 calls | IT-13-14, FT-P-54, FT-P-32 |
| AC-7 | `e2e-ctl` drops commit response | Recovery | (A) `NothingToSend`; (B) `NothingToSend`, 1 revision | IT-13-16, FT-P-76 |
| AC-8 | 3 x 2 GiB; 5 GiB RSS | Cancel; memory | 1 s; <= 150 MB | IT-13-18, NFT-RES-LIM-03 |
| AC-9 | Scripted fake | Malicious cases | Contained | ST-13-3 |
| AC-10 | Sentinel tokens | Whole run | 0 occurrences | ST-13-2 |

## Runtime Completeness

- **Named capability**: `HttpSyncClient` against the real sunnycloud server stack (API container, PostgreSQL, blob volume, Caddy) over HTTP/2.
- **Production code that must exist**: every operation under test runs the production client code paths; the server is the release image's code under the test profile.
- **Allowed external stubs**: the scripted fake for AC-9 only; `e2e-ctl`, shaping and the fault proxy as test infrastructure.
- **Unacceptable substitutes**: the scripted fake or an in-memory server standing in for any real-server leg, a fake `SyncApi`, or a database other than PostgreSQL.

## Constraints

- Integration tests always use the real server container with real PostgreSQL; only AC-9 uses the fake (it tests a malicious server).
- Higher tiers run on their designated hosts; a missing host is `NOT-RUN`, recorded in `_docs/_process_leftovers/`.

## Risks & Mitigation

**Risk 1: Server components late**
- *Risk*: the suite waits for E-21, E-22 and E-31 enablers.
- *Mitigation*: the executor tasks are already green on the fake; this suite only adds the real-server legs.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| AC-7 (B) is BLOCKED until E-14/c02 and E-23/c01 record the new-file rule | E-14 / E-23 | resolved 2026-11-13 (AZ-3949) | Low |
| AC-3's exact fields depend on G-03-3 (E-14/c01) | Owner | resolved 2026-11-11 (AZ-3731: AC-3 is 16 MiB; commit answers in commit_service.md) | Medium |
| Split of row E-23/c18 into c18a and c18b; pipelines do not reference c18 | E-23 owner | accepted-risk | Low |
