# Video poster path: frame at poster_at_ms, tone map, Profile A to WebP

**Task**: AZ-3859_decoder_video_poster
**Name**: Decoder video poster path
**Description**: For `render_still` on a video container, decode the frame nearest `poster_at_ms` with ffmpeg (tone-mapped when HDR) into `work/`, then turn that runner-made PNG into the WebP through image Profile A
**Complexity**: 2 points
**Dependencies**: AZ-3857_decoder_image_executor, AZ-3858_decoder_probe, AZ-3849_decoder_s5_whitelist_hdr_spike, AZ-3851_decoder_watchdog_hook_ssim_spike
**Component**: 10_decoder
**Tracker**: AZ-3859
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c10

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — `render_still` with `poster_at_ms`.

## Problem

Videos need a thumbnail too (AC-10). The poster offset is the supervisor's decision (`media-shared::policy`: 1 s, or 10% of the duration for clips under 10 s); the decoder must take the frame at exactly that offset, tone-map HDR frames so the poster is not washed out, and never hand untrusted bytes straight to vips: only a PNG the runner itself produced reaches the image profile.

## Outcome

- `render_still` on a video writes a WebP poster of `max_edge` from the frame nearest `poster_at_ms`.
- HDR posters go through the same tone-map chain as the transcode (gate S5).
- HLG poster quality is recorded, not judged, until the owner sets the tolerance (G-10-6).

## Scope

### Included
- ffmpeg with the same whitelists decodes one frame at `poster_at_ms` to a PNG in `work/`; HDR frames tone-mapped with the S5 chain.
- That PNG goes through image Profile A (E-19/c08) to `out/result.webp` at `max_edge`.
- `poster_at_ms` ignored for image inputs.

### Excluded
- Computing the offset (E-20/c10a with `media-shared::policy`); the transcode (E-19/c11).

## Acceptance Criteria

**AC-1: Frame at the requested time (IT-10-7, AC-10)**
Given DS-08 clips with flash frames at known timestamps
When `render_still{max_edge: 320, poster_at_ms}` is sent with 1000 and with 900
Then the poster shows the flash marker of the frame nearest the requested time (tolerance one frame) and is a 320 px WebP

**AC-2: Offset ignored for images**
Given a JPEG and a `poster_at_ms` of 5000
When `render_still` is sent
Then the output equals the render without `poster_at_ms`

**AC-3: Only runner-made bytes reach vips**
Given any video poster job
When the stub argv log is read
Then vips is invoked only on the PNG under `work/`, never on `in/source`

**AC-4: HLG poster recorded (G-10-6)**
Given a 4K HLG clip (DS-06 base)
When its poster is rendered
Then the output is SDR tagged and its SSIM against the harness reference is recorded with no pass criterion

## Non-Functional Requirements

**Performance**
- Poster in time for the 5 s thumbnail budget of interactive jobs (NFT-PERF-13).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Argv builder for the frame grab (golden, SDR and HDR) | Seek, whitelists and tone-map chain as fixed |
| AC-3 | Executor wiring | Image profile receives the `work/` PNG path only |

## Blackbox Tests

Runner-level cases run against the runner in the decoder image (E-19/c14a); the production-container run is E-19/c15.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | DS-08 flash clips, JPEG | IT-10-7 SDR part | Nearest frame; offset ignored for images | AC-10, FT-P-23 |
| AC-4 | DS-06 base | IT-10-7 HLG part | SSIM recorded only | G-10-6 |

## Runtime Completeness

- Named capability: ffmpeg frame decode with libzimg tone mapping, then libvips Profile A, from the decoder image (E-19/c14a).
- Unacceptable substitutes: extracting an embedded cover image or a fixed first frame in place of the frame at `poster_at_ms`.

## Constraints

- The decoder does not decide the offset (10 §4 "Video poster").

## Risks & Mitigation

**Risk 1: Seek lands on the wrong frame**
- *Risk*: fast seeking returns the previous keyframe.
- *Mitigation*: AC-1 checks the flash marker within one frame.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-10-6: HLG poster-frame SSIM tolerance is an open owner item; AC-4 records only | Project owner | open | Medium |
