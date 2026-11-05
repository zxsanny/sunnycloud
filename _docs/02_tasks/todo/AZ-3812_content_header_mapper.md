# Content-serving header mapper

**Task**: AZ-3812_content_header_mapper
**Name**: Content-serving header mapper
**Description**: One mapping from 03's `Download` to status and headers for every content route (node and revision content, share content, thumbnails, transcodes), so hostile types are never inline, Range and validators behave the same everywhere, and a failed stream is never a clean short response
**Complexity**: 2 points
**Dependencies**: AZ-3810_error_mapper, AZ-3827_route_registry_openapi, AZ-3769_contracts_part2
**Component**: 11 http_api
**Tracker**: AZ-3812
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/download_service.md` — `Download`, the inline decision, Range and validators (E-14/c10).
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — `Download` DTO (E-00/c04).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — 404 and 416 bodies (E-21/c09).

## Problem

Uploaded files are user content. Rendering an uploaded HTML, SVG or polyglot inline on the API origin would let an attacker run script there (AC-47). Clients resume large downloads with Range and validators, and verify bytes; a stream that fails half way must look like a failure to them, not like a complete shorter file.

## Outcome

- Content routes answer with the headers and statuses of `content_serving_headers.md`.
- A body error after headers aborts the stream (RST_STREAM on HTTP/2, close on HTTP/1.1).
- Error answers on content routes keep `nosniff`.

## Scope

### Included
- The header mapping (`Content-Type`, `Content-Disposition` with a sanitised RFC 5987 file name, `nosniff`, `Content-Security-Policy: sandbox` on attachments, `Accept-Ranges`, `ETag`, `Content-Range`, `Cache-Control: private`) and status mapping (200, 206, 304, 416).
- HEAD answered from the same call without a body.
- Stream abort on body error; streaming without buffering the whole body.

### Excluded
- The inline decision, Range parsing and ETag values (03); share and derived authorisation (06, 08); the routes themselves (E-21/c13, E-21/c14).

## Acceptance Criteria

**AC-1: Seven inline types (AC-47)**
Given a `DownloadService` stub returning `Inline` for jpeg, png, gif, webp, avif, mp4 and webm
When each is downloaded
Then each answer has its exact `Content-Type`, `Content-Disposition: inline` and `nosniff`

**AC-2: Hostile types are attachments (AC-47)**
Given the stub returning `Attachment` for html, svg, xml, js, pdf and a GIF/HTML polyglot, with `suggested_name` values `a"b.txt`, a name with CR and LF, `..\x`, a name with U+202E and a 300-byte UTF-8 name
When each is downloaded
Then each answer has `Content-Type: application/octet-stream`, `Content-Disposition: attachment; filename*=UTF-8''...` with no CR or LF, escaped quotes, no path separators and no bidi controls, `Content-Security-Policy: sandbox` and `nosniff`; 0 of the 6 is served inline

**AC-3: Range and validators**
Given a 1 KiB file and the stub honouring Range and `If-None-Match`
When `bytes=0-0`, `bytes=5-`, `bytes=-3`, `bytes=99999999-` and `If-None-Match` equal to the ETag are sent
Then the answers are 206 with the matching `Content-Range`, 416 with `Content-Range: bytes */1024`, and 304; every answer has `Accept-Ranges: bytes`, `ETag` and `Cache-Control: private`

**AC-4: Errors keep nosniff**
Given the stub returning `NotFound` and `ContentUnavailable`
When the content route is called
Then the answer is a 404 problem body with `nosniff`

**AC-5: Stream abort after headers**
Given a stub stream with `Content-Length` 10 MiB that fails after 3 MiB
When it is downloaded over HTTP/2 and over HTTP/1.1
Then HTTP/2 ends the stream with RST_STREAM and HTTP/1.1 closes the connection before 10 MiB, so the client sees an error, never a successful end of body

## Non-Functional Requirements

**Security**
- Coverage of the header mapper at least 90% (security-critical).

**Performance**
- The body streams through without being held in memory (PT-11-1 in E-21/c20).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-2 | `Download` to headers for 7 inline and 6 hostile types | Table as stated |
| AC-2 | File-name sanitiser over hostile names | No CR/LF, quotes escaped, no separators or bidi controls |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Mode S stub, DS-17 types | Header mapper (IT-11-14) | 7 inline, 6 attachment, 0 inline hostile | AC-47, NFT-SEC-19, FT-P-39 |
| AC-3 | Mode S stub | Range and ETag (IT-11-14) | 206, 416, 304 with headers | NFT-SEC-18 |
| AC-4 | Mode S stub | Error on content route (IT-11-14) | 404 with `nosniff` | FT-N-11 |
| AC-5 | Mode S stub, failing stream | Stream abort (IT-11-20, stream part) | RST_STREAM; h1 close | AC-47 |

## Constraints

- The disposition is rendered as 03 decided it; this task never upgrades an attachment to inline.

## Risks & Mitigation

**Risk 1: Header injection through file names**
- *Risk*: a stored name carries CR/LF into a header.
- *Mitigation*: AC-2 sanitiser cases; ST-11-2 in E-21/c20.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-10: inline eligibility by declared type, sniffing or name is 03's decision; this task renders 03's answer | E-14 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/11_http_api/content_serving_headers.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
