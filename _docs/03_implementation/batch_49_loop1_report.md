# Batch Report

**Batch**: 49
**Tasks**: AZ-4231_device_login_wire (done); AZ-3908_token_store_device_login (its remaining ACs closed by AZ-4231, done); new follow-up AZ-4236
**Date**: 2026-11-12

One worker across sync-core, sync-core-proto, 11 (the devices route and an end-to-end test) and the desktop daemon. The parent wrote the contract changes (workers do not edit `_docs/`), reviewed, ran the gate and committed. Owner decision 2026-11-12: the device approval page is the SPA route `/device`.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-4231_device_login_wire | Done | AC-1: the grant answer adds `verification_uri` = `<public_url>/device` (from `HttpConfig.public_url`, never `Host`) and keeps `expires_in`; the OpenAPI schema requires it. AC-2: sync-core decodes the grant with `expires_in` and the poll as the server answers it (200 `{access_token, token_type}` or 400 `authorization_pending` / `slow_down` / `access_denied` / `expired_token`); the old `{status}` decoder is gone. AC-3: the daemon's `LoginStarted.expires_at` = now + `expires_in`. AC-4: the real `HttpSyncClient` runs a device login against the full 11 stack (through a per-run TLS terminator, because the client speaks TLS 1.3 and h2 only) and gets the four refusals and then the token. AC-5: `CredentialProvider::bearer()` returns `Result<Credential, CredentialUnavailable>`; on `Err` nothing is sent, the call answers `SyncError::CredentialUnavailable`, never retried, never a refresh or a stop; the daemon answers it for a missing or unreadable token, so a locked keyring pauses sync instead of signing out |
| AZ-3908_token_store_device_login | Done | The open items of batch 47 (`expires_at`, the end-to-end login, the locked-keyring sign-out) are closed by AZ-4231 |
| AZ-4236_session_token_wire (new) | Spec written, Jira AZ-4236 | sync-core decodes the mobile code exchange and the refresh as `{access, refresh, expires_in}` while the server sends `{access_token, refresh_token, expires_in}`; mobile sign-in and refresh fail against the real server |

## Contract changes (parent)

`api_v1_surface.md` 1.8.0 (the grant answer); `sync_core_proto.md` 1.1.0 (`DeviceAuthorization.expires_in`, `DevicePoll` from the server's answers, `DeviceToken`, `DeviceRefusal`); `host_traits.md` 1.1.0 (fallible `bearer()`, open point 4 closed, a test case); `sync_api.md` (`CredentialUnavailable` among the common errors, `DeviceGrantRefused` internal). The 16 description names the approval page route `/device`.

## Code Review Verdict: PASS

No findings on the request path (`transmit`, `renew`), the poll decoding or the grant answer.

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (2977 passed, 0 failed, 19 ignored), wasm build and the four check scripts all exit 0.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | Mobile code exchange and refresh decoded with the wrong field names in sync-core (and its `FakeAuth`) | AZ-4231 | New task AZ-4236 | clear |
| 2 | The daemon does not yet map `LoginStarted` to the IPC `LoginPrompt`; `LoginPrompt.expires_at` is an RFC 3339 string | 14 ipc | For the IPC wiring task | unclear |
| 3 | 11's dev-dependencies gain `sync-core`, `sync-core-proto`, `sc_config`, `rcgen`, `rustls`, `tokio-rustls` (no new external package; the crate-graph check allows a server crate to dev-depend on sync-core) | AZ-4231 AC-4 | As implemented | clear |

## Commit

`[AZ-4231] [AZ-3908] Device login wire and unavailable credentials` — Loop: 1

## Next Batch

AZ-4236 (session token wire), then the remaining ready tasks after recomputing.
