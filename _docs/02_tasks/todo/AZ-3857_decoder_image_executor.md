# ImageExecutor: sniffing, Profiles A and B, pixel limit, vipsthumbnail, read-back

**Task**: AZ-3857_decoder_image_executor
**Name**: Decoder ImageExecutor
**Description**: Choose the image profile from magic bytes, refuse oversize images from the header before decoding, render the 320 px thumbnail and 1440 px preview with `vipsthumbnail` (orientation applied, metadata stripped) and report the real dimensions of the output
**Complexity**: 3 points
**Dependencies**: AZ-3853_decoder_runner_socket_loop, AZ-3854_decoder_process_guard, AZ-3856_decoder_safe_open_watchdog, AZ-3850_decoder_s4_heic_stubs_spike, AZ-3863_decoder_media_build_stages, AZ-3837_media_shared_params_protocol
**Component**: 10_decoder
**Tracker**: AZ-3857
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — `render_still`, `ImageResult`, `LimitExceeded` variants.
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `Params`, `OutputSpec`.

## Problem

Photos are the most frequent and most attacked input. The loader must be chosen from the bytes, not the name (an SVG renamed `.png` must never reach a loader); a 30000x30000 PNG must be refused before any decode (AC-25); thumbnails must have the right orientation and dimensions and match a reference render (AC-10); and the dimensions the API stores must be the output's real ones, not assumed.

## Outcome

- Each image input runs under the correct profile or is `Unsupported`.
- Oversize images fail as `LimitExceeded{pixels}` from the header alone.
- `render_still` on a photo writes one WebP at `out/result.webp` and returns its real width, height and size.

## Scope

### Included
- Magic-byte sniffing: JPEG, PNG, WebP, GIF -> Profile A (`VIPS_BLOCK_UNTRUSTED=1`); HEIC/HEIF/AVIF `ftyp` brands -> Profile B (variable unset); video containers (`ftyp` non-HEIF brands, EBML, RIFF AVI, MPEG-TS sync) -> handed to the poster path (E-19/c10); anything else `Unsupported` (G-10-5: SVG named `.png` is `Unsupported`).
- `vipsheader` under the same profile reads width and height; above `photo_max_pixels` -> `LimitExceeded{pixels}` before any decode.
- `vipsthumbnail` with the option strings verified in gate S4 (E-19/c02): size `max_edge`, auto-rotate (dimensions swap for orientations 5-8), `keep=none`, never `--linear`, never libheif "unlimited"; the HEIC 320 px source as decided in S4.
- `VIPS_CONCURRENCY` equal to the clamped cpu limit; environment exactly the profile's allowlist.
- Output check: a regular WebP of at most `max_output_bytes` (else `LimitExceeded{output}`); width and height read back from the file.
- Every child run through `ProcessGuard`; exit codes mapped to `Corrupt` or `Unsupported`, never by message text.

### Excluded
- The photo `probe` (E-19/c09), the video poster path (E-19/c10), containment after the job (E-19/c07a).

## Acceptance Criteria

**AC-1: Sniff table (ST-10-3)**
Given a JPEG, PNG, WebP, GIF, HEIC, AVIF, an MP4, an SVG named `.png` and a PDF named `.png`
When `render_still` is sent for each
Then the first four run under Profile A, HEIC and AVIF under Profile B, the MP4 takes the poster path, and the SVG and PDF are `Unsupported` without any loader running

**AC-2: Pixel limit before decode (IT-10-6)**
Given `photo_max_pixels = 1000000`
When a 1000x1000 PNG and a 1001x1000 PNG are rendered, and with the default limit the DS-16 30000x30000 PNG
Then the first renders, the second is `LimitExceeded{pixels}` in under 1 s, and the third is `LimitExceeded{pixels}` in under 2 s with runner memory never above 200 MiB

**AC-3: Orientation, size and fidelity (AC-10, IT-10-5)**
Given DS-03 JPEGs with EXIF orientation 1 to 8 and 20 HEIC, at `max_edge` 320 and 1440
When each is rendered
Then the long edge equals `max_edge`, width and height are swapped for orientations 5 to 8, SSIM against the harness reference render is at least 0.95, and the output has no EXIF, GPS or ICC data

**AC-4: Reported dimensions are real**
Given any successful render
When `ImageResult` is compared with the file at `out/result.webp`
Then width, height and bytes equal the file's own values and the file is a regular WebP

**AC-5: Output size limit**
Given `max_output_bytes` set below the size of a 1440 px render
When the render runs
Then the result is `LimitExceeded{output}` and no oversize file is reported as success

**AC-6: Profile environment (ST-10-2, IT-10-12)**
Given a Profile A job, a Profile B job and a cpu limit clamped to 2
When the child environments are logged by the stub variant
Then Profile A has `VIPS_BLOCK_UNTRUSTED=1`, Profile B does not, both have `VIPS_CONCURRENCY=2` and nothing outside the allowlist

## Non-Functional Requirements

**Performance**
- JPEG preview + thumb pair p95 600 ms (failure 2 s); HEIC pair p95 1.5 s (failure 3 s) (PT-10-1, measured in E-19/c16).

**Security**
- SVG, PDF and Magick loaders are absent from the image (checked by E-19/c14b); sniffing is the first line, not the only one.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Sniff table over magic-byte samples | Profile per input; `Unsupported` otherwise |
| AC-6 | Argv and environment builders (golden) | Exact argv and env per profile |
| AC-2 | Pixel-limit decision from header values | Refuse above the limit |

## Blackbox Tests

Runner-level cases run against the runner in the decoder image (E-19/c14a) through the protocol client; the production-container run of the same cases is E-19/c15.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Polyglots and renamed files | ST-10-3 | Profile per sniff; `Unsupported` | AC-25 |
| AC-2 | PNG pair, DS-16 huge PNG | IT-10-6 | `LimitExceeded{pixels}` on time | AC-25, NFT-RES-LIM-04 |
| AC-3, AC-4 | DS-03 orientations, HEIC | IT-10-5 | Swap, SSIM >= 0.95, stripped | AC-10, FT-P-22 |
| AC-5 | Low output limit | Render | `LimitExceeded{output}` | |

## Runtime Completeness

- Named capability: libvips 8.18.x CLI (`vipsheader`, `vipsthumbnail`) with libheif >= 1.22.0, built into the decoder image by E-19/c14a.
- Production code that must exist: the real invocation of those tools under `ProcessGuard` with the verified options.
- Allowed external stubs: only the `test-stubs` image variant, and only to create hostile behaviour (E-19/c15).
- Unacceptable substitutes: the libvips crate, an in-process decoder, or a fake image writer standing in for `vipsthumbnail` in any test that claims a media outcome.

## Constraints

- libvips is used as a CLI only, never through the crate (E-19 architecture notes).
- No policy decisions: the edge comes from the request.

## Risks & Mitigation

**Risk 1: HEIC misses the budget**
- *Risk*: a 1440 px HEIC decode exceeds 3 s at p95.
- *Mitigation*: the S4 decision (E-19/c02) on deriving the 320 px from the preview.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| WebP quality must come from `Params[params_version]`, but `render_still` carries no `params_version` (decoder protocol contract open point 7) | E-18 and E-00/c06 owners | open | Medium |
