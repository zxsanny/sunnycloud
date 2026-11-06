# Batch Report

**Batch**: 8
**Tasks**: AZ-3958_sync_typed_errors, AZ-3970_scripted_fake_server, AZ-3888_stub_daemon_ui_driver, AZ-3786_sc_observability
**Date**: 2026-11-06

Three parallel workers on disjoint paths (AZ-3958 then AZ-3970 in one worker). The Jira connector was back this session: the batch 6 and 7 transitions were replayed (all seven tickets `In Testing`) and the leftover file was deleted.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3958_sync_typed_errors | Done | sync-core `error.rs`, `wire_table.rs`, 2 test files | 18 (16 row tests, 2 property tests of 5,000 bodies each) | Open owner points below |
| AZ-3970_scripted_fake_server | Done | sync-core `tests/support/` (11 files), 6 test files; workspace `rustls` pin | 26 incl. 3 release-isolation tests | Fixture reachable from integration tests only |
| AZ-3888_stub_daemon_ui_driver | Partial | new dev-only crate `sunnycloud-settings-testkit` (`desktop/settings/test-tools`); `crate_graph.rs` registers it | 45 passed, 1 ignored | AC-7 NOT RUN: no window binary, no AT-SPI bus; Windows UIA compiled and clippy-clean for `x86_64-pc-windows-gnu`, never run |
| AZ-3786_sc_observability | Done | `sc_observability` (logging, metrics, collectors, ops router, health traits), contract doc v1.2.0 | 45 + 1 doctest + 4 compile_fail doctests on real PG 18 | AC-3/6/8 "through E-21" legs and the `contracts` re-export half of AC-9 not run |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 10 findings, none blocking, all carried below as owner questions (stub vs daemon on bandwidth -1, UI driver never run, shared `rustls` pin, db duration summary instead of histogram, dropped-line counter without a consumer, `EditedSince.current`, 503 without a retry path, 426 without a code, `request_id: null`, fixture only reachable from integration tests).

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles created as `ensure-db-roles.sh` does). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, wasm32 builds of `sync-core-wasm`, `sc_ids`, `sc_names`, `sync-core-proto`, `cargo deny check advisories bans licenses sources`; from the root `check-pinning.sh`, `check-no-sql-now.sh`, `check-db-gates.sh`, `check-audit-sql.sh`. The first full run failed one test: `sunnycloud-server` `crate_graph` rejected the new workspace member. The member was registered in the planned-package list and in `CLIENTS`, and the target re-ran green together with fmt and clippy for that crate.

## Test Suite

- Total: 702 (workspace, default features, real PostgreSQL 18)
- Passed: 701
- Failed: 0
- Skipped: 1 (AZ-3888 AC-7, `#[ignore]` with its reason)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `ApiError::parse` drops the raw code, `update_url` and non-UUID request ids; sync-core reads them from the body | AZ-3958 | E-02 exposes the raw code | unclear |
| 2 | `EditedSince{current}` has no wire source; always `None` | AZ-3958 | E-02 / E-21 / E-14 owners | unclear |
| 3 | `UpgradeRequired{message, min_version, update_url}`; `update_url` kept only when `https`; 426 without a code is not mapped | AZ-3958 AC-3 | Executors check status 426 before the table | unclear |
| 4 | Codes known to `sc_error` with no table row (`service_unavailable`, `overloaded`, `forbidden`, ...) map to `Unexpected`; 503 has no retry path | sync_api open point 5 | Owner decides | unclear |
| 5 | `resync_required` with a missing reason maps to `Unexpected` | AZ-3958 AC-4 | As built | unclear |
| 6 | `sign_out` route undecided; fixture serves `DELETE /api/v1/devices/me` | AZ-3970 | Add `sessions/current` when decided | unclear |
| 7 | Fixture is in `sync-core/tests/support`, so only integration tests use it; its client is hyper, not `HttpSyncClient` | AZ-3970 | E-23/c07 decides test CA trust | unclear |
| 8 | Workspace `rustls` pin is now `default-features = false` with `std`, `ring`, `tls12` (aws-lc is outside the licence allowlist); ring needs ISC allowed once rustls reaches the release tree | AZ-3970 | Confirm provider choice at E-23/c07 | unclear |
| 9 | `set_settings` with bandwidth -1 cannot decode (`bandwidth_limit` is `u64`); real dispatch gives -32602, the stub answers `SettingInvalid` | AZ-3888 AC-2 | Align contract or type | unclear |
| 10 | Settings key name: IT-15-5 says `bandwidth`, schema says `bandwidth_limit` | AZ-3888 | Stub uses the field name | unclear |
| 11 | Slint AccessKit has no `EditableText`; Linux typing uses GrabFocus plus key synthesis (X11/Xvfb, appends); Wayland unproven | AZ-3888 AC-7 | Use empty fields; check when the window exists | unclear |
| 12 | AC-7 control names come from the HTML mockup; the window has no documented endpoint override (Linux `XDG_RUNTIME_DIR` suffices, Windows needs a pipe name) | AZ-3888 | Define with the window task | unclear |
| 13 | AT-SPI crates dropped: they pull zbus 5 (RUSTSEC-2026-0370..0372); the Linux backend calls `org.a11y.atspi.*` on zbus 6 directly | AZ-3888 | Revisit when the crates update | clear |
| 14 | `first_interactive` measures process start to the first interactive control in the tree, not a rendered frame | PT-15-1 | Reword PT-15-1 | clear |
| 15 | New workspace member `desktop/settings/test-tools` registered in the `crate_graph` planned list and `CLIENTS` | module-layout 15 | Add a line to module-layout when the layout is next edited | unclear |
| 16 | `ReadinessCheck` shape: `name()` plus `check() -> ReadinessItem{status, detail}` with a boxed future; `Degraded` and `Failing` both give 503 | AZ-3786 | Owner confirms (contracts part 1 open point 2) | unclear |
| 17 | `db_query_duration_seconds{op}` is a summary (count, sum) because `Db::stats` keeps no buckets | AZ-3786 | E-06 owner records observations | unclear |
| 18 | `request_id` is `null` outside a request; free text in a raw `tracing` message is not enforceable by the typed API | AZ-3786 AC-1 | NFT-SEC-38 grep stays the backstop | unclear |
| 19 | `/readyz` 503 on any degraded check may be too strict for 04 suspect blobs | E-09 risk 4 | Owner (open) | unclear |
| 20 | Audit failures metric is `sunnycloud_audit_write_failures_total` (doc said `sc_audit_...`) | AZ-3786 | As built | clear |
| 21 | Carried from batch 7: #1-3, #8, #9, #12-14 (purge cutoff, system scope, mirror line, range table, session rules, planner give-up, G-03-1) remain open | AZ-3784 / 3852 / 3957 | Owner | unclear |

## Commit

`[AZ-3958] [AZ-3970] [AZ-3888] [AZ-3786] typed errors, fake server, stub daemon, observability` — Loop: 1

## Next Batch: AZ-3768 contracts part 1 (needs AZ-3786), then AZ-3967/3959 sync client chain and AZ-3890 daemon client
