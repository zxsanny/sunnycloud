# Mobile code refusal follows the server's answer

**Task**: AZ-4238_mobile_code_refusal
**Name**: Mobile code refusal
**Description**: sync-core turns the server's refusal of an unknown, used or expired mobile code (400 `invalid_request` naming the field `code`) into `SyncError::InvalidCode`.
**Complexity**: 1 point
**Dependencies**: AZ-4236
**Component**: 13 sync-core (tests in 11)
**Tracker**: AZ-4238
**Epic**: AZ-3650

## Problem

The server refuses a bad mobile code with 400 `invalid_request` for the field `code` (batch 36; `sc_error` has no `invalid_code` row). sync-core only maps `invalid_code` to `SyncError::InvalidCode`, so against the real server a reused code surfaces as `Unexpected`. The sync-core fixture answers `invalid_code`, which hides the mismatch. Found during AZ-4236 (2026-11-12).

## Acceptance Criteria

**AC-1: Refused code**
Given the mobile code exchange
When the server answers 400 `invalid_request` naming the field `code`
Then the call answers `SyncError::InvalidCode`; any other 400 answer keeps its current mapping

**AC-2: Fixture follows the server**
Given the sync-core auth fixture
When it refuses a code
Then it answers the server's shape (400 `invalid_request`, field `code`)

**AC-3: End to end**
Given the full 11 stack and the real `HttpSyncClient`
When a used code is exchanged again
Then the call answers `SyncError::InvalidCode`

## Scope

Included: sync-core's exchange error mapping, its fixture, one end-to-end case in 11's identity tests.
Excluded: changes to the server's answer or to `sc_error`.
