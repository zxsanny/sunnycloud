# `TlsTrustBridge` and the persistent user-added-CA warning

**Task**: AZ-3996_tls_trust_bridge
**Name**: TLS trust bridge
**Description**: Give sync-core's TLS the Android trust store, including user-added CAs, through the platform verifier in every process, mark a server whose certificate chains to a user-added CA as `UserAdded`, and show a persistent warning on the server screen
**Complexity**: 3 points
**Dependencies**: AZ-3991_backup_core_surface, AZ-3992_android_auth_token_vault, AZ-3999_flutter_backup_ui, AZ-4000_flutter_channel_bridge, AZ-4003_android_emulator_harness, AZ-3968_sync_uniffi_surface
**Component**: 17 android_app (Kotlin platform layer)
**Tracker**: AZ-3996
**Epic**: AZ-3637 (E-27)
**Ref**: E-27/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/13_sync_core/uniffi_surface.md` — platform-verifier initialisation entry point (E-23/c15; mechanism from E-27/c01).
- `_docs/02_document/contracts/17_android_app/backup_core.md` — `ServerCheck.trust` (E-27/c07).
- `_docs/02_document/contracts/17_android_app/status_channel.md` — `server_trust` (E-27/c16).
- `tests/spec-gaps.md` IT-17-17 (owner decision at Step 5: user-added CAs are accepted).

## Problem

Self-hosted servers often use a private CA. The owner decided at Step 5 that Android accepts user-added CAs, with a warning (spec-gaps IT-17-17). Rust's TLS must therefore use the Android trust store through `rustls-platform-verifier`'s Kotlin component, initialised before the first request in every process (worker or UI). The user must see, persistently, that the server is trusted only through a CA they added. Downgrade must stay impossible: no plaintext, no TLS 1.2-only server (ST-17-3).

## Outcome

- The platform verifier initialised once per process before any request, in workers and in the UI process.
- `ServerCheck.trust = UserAdded` when the server chain ends at a user-added CA; `System` otherwise.
- A persistent warning on the server screen while the server is `UserAdded`; it stays until the server changes or the trust becomes `System`.
- Connections fail when the user CA is removed.

## Scope

### Included
- Verifier initialisation per the E-27/c01 and E-23/c15 decision.
- Trust classification for `set_server` and its exposure as `server_trust` on the status channel.
- The warning text and its placement on the server screen.
- Tests IT-17-17 (TLS legs) and ST-17-3.

### Excluded
- The sign-out leg of IT-17-17 (E-27/c08).
- TLS version and ALPN policy (sync-core, TLS 1.3 only).

## Acceptance Criteria

**AC-1: Public certificate in every process (IT-17-17)**
Given a server with a publicly trusted certificate
When the UI process calls `set_server` and a background worker runs a backup in a fresh process
Then both connect, `ServerCheck.trust` is `System`, and no warning is shown

**AC-2: User-added CA accepted with a persistent warning (IT-17-17, owner decision)**
Given a server whose CA is installed on the emulator as a user CA
When the user sets that server and later reopens the app and the server screen
Then the connection succeeds, `trust` is `UserAdded`, and the warning is shown on the server screen every time until the server changes

**AC-3: Removed CA refused**
Given the AC-2 setup
When the user CA is removed from the device and a backup run starts
Then the connection is refused with a typed error, nothing is sent, and the failure appears in `last_error`

**AC-4: No cleartext and no downgrade (ST-17-3, AC-54)**
Given a server address with `http://`, an https server that redirects to http, and a TLS 1.2-only server
When sign-in and a backup run are attempted against each with all traffic captured
Then 0 cleartext bytes carry a token or a code, the http address is refused, the redirect is not followed, and the TLS 1.2-only server is rejected

## Non-Functional Requirements

**Reliability**
- Initialisation happens before the first request in every process; a worker started cold never connects without it.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Trust classification for a system chain and a user-CA chain | `System`; `UserAdded` |

## Blackbox Tests

Emulator against the real server behind certificates of a public test CA and a private CA installed as a user CA (harness E-27/c19).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Public certificate | UI and cold worker connect | Both connect; `System` | IT-17-17 |
| AC-2 | User CA installed | Set server, reopen | Connects; warning persists | IT-17-17 |
| AC-3 | User CA removed | Run | Refused; nothing sent | IT-17-17 |
| AC-4 | http, redirect, TLS 1.2-only | Sign-in and run, capture | 0 cleartext tokens; all refused | ST-17-3, NFT-SEC-12 |

## Constraints

- No custom trust-all verifier, no disabled verification, no pinning that bypasses the system store.

## Runtime Completeness

- **Capability**: `rustls-platform-verifier` with its Kotlin component (Android trust store, user CAs).
- **Production code that must exist**: the per-process initialisation and the trust classification in release builds.
- **Allowed stubs**: none.
- **Unacceptable substitutes**: a bundled CA list, a trust-all verifier, a debug-only path in release.

## Risks & Mitigation

**Risk 1: A cold worker connects before initialisation**
- *Risk*: a background process starts a request before the verifier is ready.
- *Mitigation*: AC-1's cold-worker leg.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The user-CA warning is not drawn in the approved mockup (`DESIGN.md` "Not drawn" row 10); this task writes its text, which the owner reviews | Owner | open | Low |
