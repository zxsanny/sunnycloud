# Session token wire in sync-core

**Task**: AZ-4236_session_token_wire
**Name**: Session token wire
**Description**: Make sync-core decode the mobile code exchange and the refresh answers exactly as the server sends them
**Complexity**: 2 points
**Dependencies**: AZ-4231_device_login_wire, AZ-3828_identity_admin_handlers, AZ-3966_token_lifecycle
**Component**: 13 sync_core (with 11 http_api tests)
**Tracker**: AZ-4236
**Epic**: AZ-3650 (E-23)
**Ref**: found in AZ-4231 (batch 49, 2026-11-12)

## Problem

sync-core decodes the mobile code exchange (`POST /api/v1/auth/mobile-code-exchanges`) and the refresh (`POST /api/v1/auth/refresh-tokens`) as `{access, refresh, expires_in}`. The server (`contracts/11_http_api/api_v1_surface.md`, batch 36 item 9) answers `{access_token, refresh_token, expires_in}`. Mobile sign-in and token refresh fail against the real server. sync-core's `FakeAuth` test double answers the wrong shape too, so its tests pass.

## Scope

### Included
- The `SessionTokens` decoding in sync-core-proto and sync-core, using one decoder, following the server contract.
- The `FakeAuth` double answering the server's shape.
- An end-to-end test from sync-core against the real 11 routes with stub identity services, in the same place as AZ-4231's device-login test (11's integration tests, sync-core as a dev-dependency).

### Excluded
- Any change to the server's answers.

## Acceptance Criteria

**AC-1: Server shape decoded**
Given the server's mobile code exchange and refresh answers
When sync-core decodes them
Then it gets the access token, the refresh token and the expiry, and a body in the old shape is a typed protocol error

**AC-2: Test double matches the server**
Given sync-core's `FakeAuth`
When a test signs in or refreshes through it
Then the body it answers equals the shape the server sends

**AC-3: End to end**
Given the real API routes with stub identity services
When sync-core runs a mobile code exchange and then a refresh
Then both succeed and the refresh rotates the tokens

## Constraints

- sync-core follows the server contract, with one decoder (owner decision 2026-11-12 for the device wire, AZ-4231).
