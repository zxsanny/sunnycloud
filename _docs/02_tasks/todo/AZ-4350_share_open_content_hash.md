# Share link answer carries the file hash; share downloads verify

**Task**: AZ-4350_share_open_content_hash
**Name**: Share open answer with content hash
**Description**: The open answer of a public share link (`GET /api/v1/public-shares/{token}`) carries the `content_hash` of the shared current revision, so the web download of a shared file is checked like an owner's download instead of ending `SavedUnverified{NoServerHash}`
**Complexity**: 3 points
**Dependencies**: AZ-3698_share_lookup_open_uniform, AZ-3699_share_password_access_grant, AZ-3942_download_strategies, AZ-3943_share_dialog_public_page
**Component**: 06 sharing, 11 http_api, 16 web_app
**Tracker**: AZ-4350
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c13 follow-up (owner decision 2026-11-22, `_docs/_process_leftovers/2026-11-21_az-3942-download-open-points.md` item 2)

## Problem

A file opened through a share link downloads, but the app cannot compare it with the stored fingerprint: `ShareOpen::Available` (`src/contracts/src/sharing/public.rs`) has `name`, `size`, `media_type`, `expires_at` and no hash, so the web download ends `SavedUnverified{NoServerHash}` (`src/web/src/lib/transfer/downloads.svelte.ts`).

## Outcome

- `ShareOpen::Available` and the 200 answer of `GET /api/v1/public-shares/{token}` carry `content_hash` of the current revision (the same value, in the same encoding, as the owner's file answer).
- The public page passes it to the download strategy; a share download ends `Verified` when the bytes match and `Failed(HashMismatch)` when they do not.

## Scope

### Included
- Contracts part 3 (`ShareOpen`), 06 `open` (the hash is on the live file it already reads), 11 `visitor::open` mapping, `api_v1_surface.md`, OpenAPI, the web API client and the share page wiring; tests at each layer.

### Excluded
- Hash on any other visitor route; a hash before unlock (a password link without a valid grant still answers `password_required` with no file data); changes to grants, limits or the uniform not-available page.

## Acceptance Criteria

**AC-1: The answer carries the hash**
Given an available link, and a password link with a valid grant
When `GET /public-shares/{token}` is called
Then the 200 body has `content_hash` equal to the current revision's hash; a password link without a grant (401) and an unknown link (404) carry no file data

**AC-2: Share downloads verify**
Given a shared file, in a browser path that verifies
When the recipient downloads it, and again with one corrupted byte served
Then the first outcome is `Verified`, the second `Failed(HashMismatch)`, and `NoServerHash` does not occur

**AC-3: Contract and client**
Given the change
Then contracts part 3 and `api_v1_surface.md` carry a version bump and the new field, the OpenAPI document and the web client type include it, and the existing visitor-route tests still pass

## Constraints

- A breaking change for implementers of 06 `open`'s result: bump contracts part 3 (now 1.6.0) and `api_v1_surface` (now 1.13.0) accordingly.
- No new endpoint, no new config.
