# Contract: content_serving_headers — headers and statuses of every content download

**Component**: 11_http_api
**Producer task**: E-21/c10 — `_docs/02_tasks/todo/B07-10_content_header_mapper.md`; keys assigned at Step 4
**Consumer tasks**: E-21/c13 (node and revision content), E-21/c14 (share content, share thumbnail, thumbnail, transcode); clients E-23 (verified ranged download with resume), E-26 (download strategies, inline preview), E-24, E-27; E-31 (FT-N-11, FT-P-39, NFT-SEC-18, NFT-SEC-19); resolved at Step 4
**Version**: 1.0.2
**Status**: draft
**Last Updated**: 2026-11-11

## Purpose

One mapping from 03's `Download` value to HTTP, used by every route that streams stored bytes, so that a hostile upload is never rendered inline (AC-47), Range and validators behave the same on every content route, and a failed stream is never mistaken for a complete file. The inline-or-attachment decision itself is 03's; 11 only renders it.

Sources: `components/11_http_api/description.md` §7, §6 (stream errors); `contracts/03_transfer/download_service.md`; `contracts/contracts/part2_tree_transfer_feed.md` (`Download`).

## Shape

### Input (from 03 `DownloadService`)

`Download{status: Full | Partial | NotModified, total_size, range?, etag, media_type?, disposition: Inline | Attachment, suggested_name, body}`.

### Entry point

`sunnycloud_http_api::content_response(result: Result<Download, DownloadError>, method: &Method, request_id) -> Response`. A route calls 03's open method, passes the result and the request method, and returns the response. An `Err` is answered by `ApiProblem` (`problem_details.md`), so every port error keeps its code and `nosniff`.

### Response headers

| Header | `Inline` | `Attachment` |
|--------|----------|--------------|
| `Content-Type` | the allowlisted media type | `application/octet-stream` |
| `Content-Disposition` | `inline` | `attachment; filename*=UTF-8''<suggested_name, sanitised>` |
| `X-Content-Type-Options` | `nosniff` | `nosniff` |
| `Content-Security-Policy` | - | `sandbox` |
| `Accept-Ranges` | `bytes` | `bytes` |
| `ETag` | from `etag` | from `etag` |
| `Content-Range` | on 206 and on 416 (`bytes */<total>`) | same |
| `Cache-Control` | `private` | `private` |

An `Inline` answer whose `media_type` is not on the list below (compared case-insensitively, served with the canonical spelling) is rendered as an attachment; an `Attachment` is never upgraded. `ETag` is `etag` wrapped in double quotes unless 03 already quoted it. `Content-Length` is `total_size` on 200 and the range length on 206; 304 has no body or length; `HEAD` keeps the length of the answer it stands for.

Inline types are exactly `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/avif`, `video/mp4`, `video/webm` (AC-47). Name sanitising: control characters (including CR and LF) and bidi controls (U+061C, U+200E, U+200F, U+202A to U+202E, U+2066 to U+2069) are dropped, `/` and `\` become `_`, the name is cut to 255 UTF-8 bytes on a character boundary, an empty or dots-only result becomes `download`, and the name is percent-encoded UTF-8 (RFC 8187 `attr-char` stay; a quote becomes `%22`).

### Status

| `Download.status` / outcome | HTTP |
|-----------------------------|------|
| `Full` | 200 |
| `Partial` | 206 |
| `NotModified` (matching `If-None-Match`) | 304 |
| `RangeNotSatisfiable { total_size }` | 416 problem body (`range_not_satisfiable`, `nosniff`) with `Content-Range: bytes */<total_size>` and `Accept-Ranges: bytes`; no `ETag` (the error carries none) |
| `NotFound` / `ContentUnavailable` | 404 problem body (`problem_details.md`), `nosniff` kept |

A `Partial` download without a range inside `total_size`, a negative `total_size` or an `etag` that is not a header value is a broken 03 answer: it is logged and answered as an opaque 500. `HEAD` uses the same call and the same headers with no body. One byte range per request (03 serves a single range; multi-range is answered as a full 200 or 416, never as multipart).

### Public share content only

May add `Access-Control-Allow-Origin: *`; never `Access-Control-Allow-Credentials`.

## Invariants

- No content response leaves the server without `nosniff`, including errors on these routes.
- A non-allowlisted type is never served with its own `Content-Type` or with `inline`.
- A body error after headers aborts the stream (RST_STREAM or close) and never ends as a clean response shorter than `Content-Length`.
- A body error after headers is logged (`request_id`, error chain) and returned to the server, which closes an HTTP/1.1 connection and sends RST_STREAM (`INTERNAL_ERROR`) on HTTP/2.
- Downloads have a write-idle timeout (60 s) and no total time cap.

## Non-Goals

- Not covered: range parsing, ETag computation and the inline decision (03); share authorisation (06); derived lookup (08).

## Versioning Rules

- **Breaking**: a header removed, an inline type removed from or added to the allowlist without an AC change, a status changed.
- **Non-breaking**: an extra informational header.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| inline-7 | jpeg, png, gif, webp, avif, mp4, webm | exact type, `inline`, `nosniff` | IT-11-14 |
| hostile-6 | html, svg, xml, js, pdf, polyglot | `application/octet-stream`, attachment, sandbox CSP | IT-11-14, NFT-SEC-19 |
| names | `a"b.txt`, CR/LF, `..\x`, U+202E, 300-byte UTF-8 | sanitised `filename*` | IT-11-14 |
| ranges | `bytes=0-0`, `bytes=5-`, `bytes=-3`, `bytes=99999999-`, `If-None-Match` = ETag | 206 with `Content-Range`, 416 `bytes */len`, 304 | IT-11-14 |
| stream-error | 10 MiB declared, error after 3 MiB | h2 RST_STREAM; h1 close before `Content-Length` | IT-11-20 |

## Open points

1. G-10 (owner): inline eligibility by declared type, sniffing or name is 03's decision; this contract renders whatever `disposition` 03 returns.

2. The 416 carries no `ETag` or `Cache-Control: private`: `DownloadError::RangeNotSatisfiable` carries only `total_size`. AC-3 of AZ-3812 says every answer has `ETag`; adding the tag to the error is a further contracts change if the owner wants it.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B07) |
| 1.0.1 | 2026-11-09 | Implemented (AZ-3812): entry point, ETag quoting, allowlist downgrade, name rules, broken-answer 500, open point 2 | implement |
| 1.0.2 | 2026-11-11 | `RangeNotSatisfiable { total_size }`: the 416 carries `Content-Range: bytes */<total>` and `Accept-Ranges`; open point 2 reduced to the missing `ETag` | owner-approved change |
