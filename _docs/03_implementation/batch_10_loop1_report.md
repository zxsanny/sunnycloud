# Batch Report

**Batch**: 10
**Tasks**: AZ-3959_http_sync_client_connect, AZ-3853_decoder_runner_socket_loop, AZ-3868_worker_crate_skeleton, AZ-3769_contracts_part2
**Date**: 2026-11-06

Four parallel workers on disjoint crates (`sync-core`, `decoder`, `worker`, `contracts`). Shared files touched: `shared/config/src/table.rs` (AZ-3868: `log` and `log_level` readers) with `shared/config/tests/rules.rs`, `src/Cargo.toml` (reqwest pin), `src/deny.toml` (ISC, one exception), `deploy/tests/check-images.sh`, `Cargo.lock`.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3959_http_sync_client_connect | Done | `sync-core` client, negotiation, reqwest transport, TLS policy; 426 mapping; release-isolation checks; `deny.toml`; reqwest pin | 93 in sync-core (27 new) | Android verifier init and plain-HTTP functional test not run |
| AZ-3853_decoder_runner_socket_loop | Done | `decoder-runner`: config, limits, job thread, socket server, health; no executors | 31 (9 unit, 9 process, 13 socket-loop) | Idle CPU, memory, real-job cases wait for E-19 |
| AZ-3868_worker_crate_skeleton | **Blocked by a design decision (AC-5)** | `sunnycloud-worker` lib + bin, config, ops probe, tests; `check-images.sh` image checks | 27 passed, 2 ignored | `sc_observability` pulls `sc_db`/`sc_audit`/sqlx into the worker; two AC-5 graph tests are `#[ignore]`d; image checks never run. Spec stays in `todo/`, ticket stays `In Progress` |
| AZ-3769_contracts_part2 | Done | `contracts`: blob types, tree, transfer, feed ports (16 traits) | 62 integration + 2 compile_fail doctests (26 new) | The `&mut DbTx` doubles for `FeedStamp`, `FeedHorizon`, `CommitEffect` compile only |

## Code Review Verdict: PASS_WITH_WARNINGS

Review of the joined tree: 10 findings, none blocking: sqlx in worker and decoder images, `no_proxy()` in the transport, ALPN not verified and buffered 16 MiB bodies, uncapped `Retry-After`, runner not exiting after a hostile job outcome, `Unsupported` for a missing executor, fixed decoder logging, config doc drift (fixed in this commit), invented part-2 field types, unrun image checks.

CI-parity: PASS with a local PostgreSQL 18 (`DATABASE_URL`, throwaway container, roles as `ensure-db-roles.sh` creates them). In `src/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast`, wasm32 builds of `sync-core-wasm`, `sc_ids`, `sc_names`, `sync-core-proto`, `cargo deny check advisories bans licenses sources`; from the root `check-pinning.sh`, `check-no-sql-now.sh`, `check-db-gates.sh`, `check-audit-sql.sh`. All 13 steps exit 0. `bash -n deploy/tests/check-images.sh` passes.

## Test Suite

- Total: 943 (workspace, default features, real PostgreSQL 18)
- Passed: 940
- Failed: 0
- Skipped: 3 (AZ-3888 AC-7; AZ-3868 AC-5 twice)

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | `sc_observability` links `sc_db`, `sc_audit` and sqlx, so the worker (and the decoder) contain sqlx. Split the crate (collectors behind a feature or in their own crate), then un-ignore the two AZ-3868 tests | AZ-3868 AC-5, flagged concern | **Owner decision**; AZ-3868 waits for it | unclear |
| 2 | Part 2: extra `Unexpected(Fault)` on every port enum, one error enum per port, `DownloadOptions` grouping, invented field types (`client_meta: String`, `import_hint: bool`, `client_key: String`), `EntryError` with 8 variants derived from `commit_upload` errors, `StreamError{Corrupt, Source}`, `StoredResponse(Vec<u8>)`, `Seq = i64` | AZ-3769 contract tables | Owners of E-13, E-14 replace them; owner rules once on `Unexpected` for parts 1 and 2 | unclear |
| 3 | `CommitServiceError::EditedSince` has no payload, but 13's client outcome carries `current`; `PublishedEntry` has no `account_id` | contract vs 13 | Minor contract change when E-13 decides | unclear |
| 4 | Client-side accepted `server-info` ranges are the worker's numbers (batch 1..=10000, staged 1..=1000000, long-poll 1..=300 seconds); `small_file_max` must equal 2 MiB; `max_lanes` 0 is an error, larger is clamped to 4 | AZ-3959 | Owner confirms ranges and the wire unit | unclear |
| 5 | `ConnectError::InvalidConfig{field}` added; refusal mapping of `server-info` (401, 426, transport/429, other) | sync_api.md | Owner confirms the contract extension | unclear |
| 6 | The `min_client_version` `UpgradeRequired` has a client-written message and `update_url: None` | server_info open point 2 | E-21 supplies `update_url` | unclear |
| 7 | `no_proxy()`: system proxies are ignored; keep-alive ack timeout is reqwest's 20 s; ALPN `h2` is enforced by offering only `h2` plus prior knowledge, with no check of the negotiated protocol; responses over 16 MiB are a transport error | AZ-3959 | Owner confirms | unclear |
| 8 | Test CA trust: the only way to add roots is `connect_with_roots`, present only with the `test-support` feature (enabled by a self dev-dependency); `release_isolation` fails if `allow-plain-http` or `test-support` reaches the release tree; `compile_error!` blocks `allow-plain-http` in release | AZ-3970 flagged concern | Decided here, as that concern required | clear |
| 9 | reqwest, hyper-util and tokio-rustls are now in the release tree of `sync-core`; `mobile_backup` and `daemon` link reqwest; `ISC` is allowed in `deny.toml` and one exception for `webpki-root-certs` (wasm32 only) | AZ-3959 | Note for their reviews | clear |
| 10 | Android: the extra-roots verifier path does not exist there; the Android verifier init is E-23/c15 and was not compiled for Android | AZ-3959 Excluded | c15 | unclear |
| 11 | Decoder: no job fields (`job_id`, `kind`, kill reason) in the closed log schema; `log` and `log_level` have no decoder reader so logging is fixed JSON at INFO | AZ-3853 | E-09 owner adds fields; add reader `R` | unclear |
| 12 | Decoder with no executors answers `Unsupported` and reports a failing self-check; there is no typed "executor missing" error; hostile-class outcomes do not yet end the runner; no watchdog heartbeat (c07b); `probe` runs under photo ceilings; `cancel` on an idle session gets no reply | AZ-3853 | c06-c12 | unclear |
| 13 | Worker: `healthcheck` probes `/readyz`, secrets are read from files only, `worker_ops_listen` is parsed by the worker (`:port` means 0.0.0.0), `worker_shutdown_grace` is parsed but unused until c08 | AZ-3868 | As built | unclear |
| 14 | `log` and `log_level` gained the worker reader; `a_binary_ignores_keys_it_does_not_read` now uses `h2_max_streams`; helper 01 doc updated | AZ-3868 | As built | clear |
| 15 | Carried from batches 7-9 and still open: Retry-After cap, `bearer()` blocking, `Unexpected` variant (part 1), `&mut DbTx` doubles, CLI log fields, UI driver, db duration summary, invented sync config ranges | earlier reports | Owner | unclear |

## Commit

`[AZ-3959] [AZ-3853] [AZ-3868] [AZ-3769] sync client connect, decoder runner, worker skeleton, contracts part 2` — Loop: 1

## Next Batch

Open decision first: split `sc_observability` so the worker has no sqlx (AZ-3868 stays in `todo/`, `In Progress`). Ready now besides it: AZ-3682 identity crate, AZ-3749 quota skeleton, AZ-3792 janitor scheduler, AZ-3816 admin crate skeleton, AZ-3892 platform fs linux, and the AZ-3960/3963/3964/3965/3966 sync client operations.
