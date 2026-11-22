# Batch 108 (loop 1): the decoder container (roadmap section 7, item 4, first slice)

Owner decision 2026-11-22: container hardening first. Tasks AZ-3863 (media build stages), AZ-3864 (image checks) and AZ-3855 AC-6 (seccomp, IT-10-15).

| Task | Result |
|------|--------|
| AZ-3863 | Dockerfile stages fetch, build-media, builder, runtime. Pins in `deploy/docker/versions.env`: libvips 8.18.7 (hash equals the published sha256sum), libheif 1.23.7 (decode only, libde265 and dav1d linked, no plugins; no upstream checksum exists, the hash is the GitHub asset digest recomputed after download), ffmpeg 9.1 (released 2026-11-20; its tarball signature verified against the FFmpeg release key). ffmpeg configure in `ffmpeg.configure` (gpl, libx264, libzimg, native aac, no nonfree/fdk/x265/CUDA, `--disable-autodetect --disable-network --disable-devices`, protocols file and pipe, demuxers mov, matroska, avi, mpegts). SBOM, GPL source offer, OCI label and build facts written by `media-metadata.sh` (`/usr/share/sunnycloud/build-facts.json`). Runtime user 10003, no compiler, 267 MB (160.9 MB file tree). Cold build 261 s on 24 cores, cached rebuild about 1 s |
| AZ-3864 | `scripts/ci/check-decoder-image.sh` (13 properties, including HEVC HEIC decode of a generated sample and the no-test-hooks property); seeded failures: 44 recorded-output cases with no Docker, and 8 real tiny layers on the good image (hls, libfdk_aac, svg loader, vips version, SBOM removed, old libheif, empty source-offer label, test stub), each failing with its property named; AC-1 hash mismatch fails at the fetch stage. Wired into the `images` job with a 60 min timeout for the decoder leg |
| AZ-3855 AC-6 | `deploy/seccomp/decoder.json` (moby seccomp v0.3.0 default with ptrace, process_vm_readv/writev, kcmp and pidfd_getfd denied with EPERM), referenced by both decoder services of `compose.dev.yml`; `scripts/ci/check-decoder-seccomp.sh` proves EPERM under the profile, `Seccomp: 2`, `NoNewPrivs: 1`, and that the same calls succeed without it |

## Verification (re-run by the parent on the built image)

| Check | Result |
|---|---|
| `check-decoder-image.sh sunnycloud-decoder:dev-media` | OK, every property |
| `check-decoder-seccomp.sh` | OK |
| `check-pinning.sh`, `run-script-tests.sh`, compose config, deploy lint | OK |
| `cargo nextest -p decoder-runner` (worker, host tools) | 137 of 137 |
| Smoke (worker): the image with `--network none --read-only --cap-drop ALL` and the profile | a 400x300 JPEG came back as a 320x240 WebP, an HEVC HEIC as a 64x48 WebP (run as the host uid so the host could reach the socket; the 10003 user is checked statically) |

## Code Review Verdict

Not run: pending the batch review of the loop. The CI `images` job (a cold 4-vCPU build, the image check and the seccomp check) is the remaining proof.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The demuxer allowlist is PROVISIONAL: it comes from the S4/S5 whitelist of decoder description 13.1, measured on ffmpeg 8.0.1 over synthetic clips | AZ-3863, AZ-3849, AZ-3850 | the spikes stay open for the reference host | clear |
| 2 | ffmpeg 9.1 was released two days before the pin; the host decoder tests and the spike numbers use 8.0.1 (CI: Ubuntu 24.04's older ffmpeg). No video poster or probe has run through the container yet | AZ-3863 | owner confirms 9.1 over 9.0.2; the container suite (AZ-3865) proves video through the image | RESOLVED 2026-11-22 (owner): keep 9.1 |
| 3 | The build-facts file path (`/usr/share/sunnycloud/build-facts.json`) is named by no doc; the runner's `hello` still returns "unknown" and build_timestamp 1 (placeholder of AZ-3861) | AZ-3861 | AZ-3861 reads the file; the owner confirms the path | unclear |
| 4 | `--disable-devices` removes the lavfi device, so the v1.1 NVENC probe (`-f lavfi -i nullsrc`) needs another source | AZ-3862 | decide at AZ-3862 | unclear |
| 5 | libvips is built without highway and orc (no SIMD speed-up): PT-10-1 timing is unchecked | AZ-3866 | measure at AZ-3866 on the reference host | clear |
| 6 | `VIPS_BLOCK_UNTRUSTED=1` is set and the HEIC still decodes under it on 8.18.7, so heifload is not flagged untrusted: this matters for the Profile B decision (description 4, 13.7) | AZ-3857 | owner decision at Profile B | unclear |
| 7 | Opening `/proc/<runner>/mem` and `environ` depends on the runner's `PR_SET_DUMPABLE=0`, not the profile: `check-decoder-seccomp.sh` does not cover it | AZ-3855 AC-6 | IT-10-15 on the running runner (AZ-3865) | clear |
| 8 | The weekly rebuild and release workflows must pass BUILD_TIMESTAMP and BUILD_DATE (local builds carry placeholders) | AZ-3863 flagged concern | out of this slice, no owner yet | unclear |
| 9 | Only `compose.dev.yml` runs the decoder as a container: `compose.mvp.yml` and the acceptance stack use host processes with distro tools, and `tests/compose.test.yml` has no decoder yet | AZ-3855 | a later slice moves the MVP stack and the test stack to the image | clear |

## Commit

`[AZ-3863] [AZ-3864] [AZ-3855] Decoder container: source-built media image, image checks, seccomp profile` — Loop: 1
