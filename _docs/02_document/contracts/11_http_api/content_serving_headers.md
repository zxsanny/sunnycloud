# Contract: content_serving_headers — headers and statuses of every content download

**Component**: 11_http_api
**Producer task**: E-21/c10 — `_docs/02_tasks/todo/B07-10_content_header_mapper.md`; keys assigned at Step 4
**Consumer tasks**: E-21/c13 (node and revision content), E-21/c14 (share content, share thumbnail, thumbnail, transcode); clients E-23 (verified ranged download with resume), E-26 (download strategies, inline preview), E-24, E-27; E-31 (FT-N-11, FT-P-39, NFT-SEC-18, NFT-SEC-19); resolved at Step 4
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

One mapping from 03's `Download` value to HTTP, used by every route that streams stored bytes, so that a hostile upload is never rendered inline (AC-47), Range and validators behave the same on every content route, and a failed stream is never mistaken for a complete file. The inline-or-attachment decision itself is 03's; 11 only renders it.

Sources: `components/11_http_api/description.md` §7, §6 (stream errors); `contracts/03_transfer/download_service.md`; `contracts/contracts/part2_tree_transfer_feed.md` (`Download`).

## Shape

### Input (from 03 `DownloadService`)

`Download{status: Full | Partial | NotModified, total_size, range?, etag, media_type?, disposition: Inline | Attachment, suggested_name, body}`.

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

Inline types are exactly `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/avif`, `video/mp4`, `video/webm` (AC-47). Name sanitising: no CR or LF, quotes escaped, no path separators, no bidi controls, percent-encoded UTF-8.

### Status

| `Download.status` / outcome | HTTP |
|-----------------------------|------|
| `Full` | 200 |
| `Partial` | 206 |
| `NotModified` (matching `If-None-Match`) | 304 |
| `RangeNotSatisfiable` | 416 with `Content-Range: bytes */<total>` |
| `NotFound` / `ContentUnavailable` | 404 problem body (`problem_details.md`), `nosniff` kept |

`HEAD` uses the same call and the same headers with no body. One byte range per request (03 serves a single range; multi-range is answered as a full 200 or 416, never as multipart).

### Public share content only

May add `Access-Control-Allow-Origin: *`; never `Access-Control-Allow-Credentials`.

## Invariants

- No content response leaves the server without `nosniff`, including errors on these routes.
- A non-allowlisted type is never served with its own `Content-Type` or with `inline`.
- A body error after headers aborts the stream (RST_STREAM or close) and never ends as a clean response shorter than `Content-Length`.
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

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B07) |
