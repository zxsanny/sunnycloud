# Handlers for shares, public shares, thumbnail, transcode and thumbnail-states (E-16, E-18 ports)

**Task**: AZ-3830_share_media_handlers
**Name**: Share and media handlers
**Description**: Register and implement the owner share routes, the visitor routes of a public link (open, password verification, content, thumbnail) and the media routes (thumbnail, transcode, thumbnail states), streaming bytes through 03's `DownloadService` and the shared header mapper
**Complexity**: 3 points
**Dependencies**: AZ-3803_body_limits_timeouts, AZ-3806_auth_extraction, AZ-3808_csrf_origin_check, AZ-3810_error_mapper, AZ-3812_content_header_mapper, AZ-3827_route_registry_openapi, AZ-3769_contracts_part2, AZ-3770_contracts_part3
**Component**: 11 http_api
**Tracker**: AZ-3830
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c14

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `ShareService`, `PublicShareService`, `ThumbnailStateSource`, `DerivedOpener` (E-00/c05).
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md`, `_docs/02_document/contracts/03_transfer/download_service.md` — `DownloadService::open_revision_content` and `open_blob_content` (E-00/c04, E-14/c10).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md`, `problem_details.md`, `content_serving_headers.md` — rows, errors and content headers (E-21/c11, c09, c10).

## Problem

Share links are opened by anonymous visitors, so every missing, expired or revoked link must look the same (AC-12), a password-protected link must reveal nothing before unlock, and the grant must travel in a cookie the visitor's scripts cannot read. 06 and 08 never serve bytes: 11 asks them what may be served and streams it through 03. Thumbnails and transcodes follow the same serving rules as any other content (AC-47).

## Outcome

- Every operation of the Sharing and Media rows of `api_v1_surface.md` is registered with its declarations.
- Visitor routes answer a byte-identical 404 for every unknown, expired, revoked or deleted link, 401 `password_required` before unlock, and set the access grant as an HttpOnly, path-scoped cookie after a correct password.
- Content, share thumbnail, thumbnail and transcode bytes stream through `DownloadService` with the shared header mapper; public share content may answer `Access-Control-Allow-Origin: *` without credentials.

## Scope

### Included
- Owner routes: create (201, token shown once), list (no token), revoke.
- Visitor routes: open, password verification, content (deciding `starts_download` from the request: no `Range` header, or a range starting at 0), share thumbnail (06 `authorize_derived`, then 08 `open_derived_for`, then 03 `open_blob_content`, the one documented composition).
- Media routes: node thumbnail with `size`, node transcode, `thumbnail-states` (body capped at 1,000 items).
- Passing the client IP to visitor calls; redacting the token path segment from logs (layer from E-21/c04).

### Excluded
- Token, password and grant rules, abuse counting (06); derived lookups and `NotReady` semantics (08); range and inline decisions (03).

## Acceptance Criteria

**AC-1: Owner share routes**
Given a session and stubs for `create`, `list` and `revoke`
When a share is created, listed and revoked, and a device token tries to create one
Then `create` answers 201 with the token once, `list` never shows a token, `revoke` is called with the principal's account, and the device token gets 403 (AC-57)

**AC-2: Uniform missing link (AC-12)**
Given `open` and `authorize_content` stubs returning `NotFound` for an unknown, an expired, a revoked and a deleted link
When each is opened and its content requested
Then all eight answers are 404 with bodies identical apart from `request_id`

**AC-3: Password before content**
Given a password-protected link
When it is opened and its content requested without a grant, then the password is verified and the content requested again
Then the first two answers are 401 `password_required` with no file name, size or type, the verification sets an HttpOnly cookie scoped to the link's path, and the content request with the cookie reaches `authorize_content` with that grant

**AC-4: Download start is decided by 11**
Given the content route
When it is requested without `Range`, with `Range: bytes=0-99` and with `Range: bytes=100-199`
Then `authorize_content` receives `starts_download` true, true and false

**AC-5: Share thumbnail composition**
Given stubs for `authorize_derived`, `open_derived_for` and `open_blob_content`
When `GET /api/v1/public-shares/{token}/thumbnail` is called
Then the three are called once each in that order, the `LiveFile` from 06 is what 08 receives, the `BlobRef` from 08 is what 03 receives, and nothing in 08 is asked to authorise the visitor

**AC-6: Media routes and CORS**
Given a node thumbnail stub returning a `BlobRef`, a `NotReady`, and a public share content download
When the thumbnail route is called for both and the share content is downloaded
Then the ready thumbnail streams with the content headers, the not-ready one answers its 404 code, and only the share content may carry `Access-Control-Allow-Origin: *`, never with `Access-Control-Allow-Credentials`

## Non-Functional Requirements

**Security**
- Share tokens never reach a log line or a metric label (NFT-SEC-16).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | `starts_download` from the `Range` header | No range or start 0 is true; otherwise false |
| AC-1 | Registry rows of these groups | Declarations present |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S | Owner share routes | 201 once; no token in list; device 403 | AC-57, FT-N-24 |
| AC-2 | Mode S | Uniform 404 for 4 link states | Identical bodies | AC-12, NFT-SEC-17 |
| AC-3 | Mode S | Password then content | 401 without file data; grant cookie | NFT-SEC-51 |
| AC-4 | Mode S | `starts_download` | true, true, false | |
| AC-5 | Mode S | Share thumbnail composition | 06, 08, 03 in order | AC-12 |
| AC-6 | Mode S | Media routes and CORS (IT-11-5, share part) | Headers; `*` only on share content | AC-47, NFT-SEC-09 step 6 |

## Constraints

- Paths follow 06 §3 and 08 §5 (owners are authoritative, 11 §8).

## Risks & Mitigation

**Risk 1: Grant cookie leaks to other paths**
- *Risk*: a grant cookie scoped to `/` would reach every route.
- *Mitigation*: AC-3 asserts the path scope.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The share thumbnail route is named in 11 §8 and 08 §5 but missing from 06's route table | E-16, E-21 owners | open | Low |
| `thumbnail-states`: 08 sends node ids, `ThumbnailStateSource::states` takes content hashes; one port call per handler must hold (G-08-3, spike E-18/c01) | E-18 owner | open | Medium |
| The grant cookie's name and exact attributes (other than HttpOnly and path scope) are not specified; it cannot use the `__Host-` prefix because that requires `Path=/` | E-16, E-21 owners | open | Low |
| 11 §8 names the visitor password route `.../access`; 06 names it `.../password-verifications`; this task follows 06 | E-21 owner | open | Low |

## Contract

This task implements the Sharing and Media rows of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
