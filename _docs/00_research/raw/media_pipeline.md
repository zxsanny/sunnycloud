# Media pipeline and mobile camera backup (raw research, 2026-11-02)

Scope: thumbnails, video transcoding, job queue, iOS/Android camera-roll backup, AC-9/10/11/14 realism.
Tier key: L1 official/spec, L2 maintainer/benchmark/engineering, L3 blog/vendor, L4 forum/issue. All pages fetched 2026-11-02; most docs carry no date. "[limited source]" = single weak source or my inference.

## Sources

| # | URL | Title | Tier | Date | Summary |
|---|-----|-------|------|------|---------|
| 1 | https://docs.immich.app/features/hardware-transcoding | Immich hardware transcoding | L1 | undated | NVENC, QSV, RKMPP, VAAPI; Linux only; encode-only accel by default; HW output larger/lower quality; "experimental" |
| 2 | https://docs.immich.app/administration/jobs-workers | Immich jobs and workers | L1 | undated | `api` worker + `microservices` worker; upload triggers metadata, thumbnail, ML jobs |
| 3 | https://docs.immich.app/FAQ | Immich FAQ | L1 | undated | 3 thumbnail jobs per asset; transcode+ML heaviest; iOS background decided by OS; dedup by hash per library |
| 4 | https://docs.immich.app/administration/system-settings | Immich system settings | L1 | undated | WebP/JPEG thumbs; 1440p preview; transcode defaults h264/ultrafast/720/policy `required`; thumbnail concurrency <= cores |
| 5 | https://docs.immich.app/features/supported-formats | Immich supported formats | L1 | undated | HEIC/HEIF/AVIF/JXL/RAW/MOV etc. listed supported |
| 6 | https://docs.immich.app/features/mobile-backup | Immich mobile backup | L1 | undated | foreground+periodic background; Wi-Fi default; Android min-delay + charging options; iCloud assets pulled to cache |
| 7 | https://docs.immich.app/install/requirements | Immich requirements | L1 | undated | min 2 cores/6 GB, rec 4 cores/8 GB; thumbs+transcodes add 10-20% storage |
| 8 | https://github.com/immich-app/immich/releases | Immich releases v3.3.x / v3.4.0-rc.1 | L2 | Oct 2026 | still fixing iOS background-upload races; linear-light thumbnail resampling; JPEG XL |
| 9 | https://github.com/immich-app/immich/discussions/1694 | iOS Background Backup discussion | L4 | 2022-23 | BGAppRefresh (<~20-30 s) vs BGProcessing (charging, overnight); users complain uploads stop when app closed |
| 10 | https://immich.app/blog/sync-v2 | Immich Sync v2 | L2 | 2025-09-24 | download-side sync only; checkpoints; no upload/backup info |
| 11 | https://immich.app/blog/release-1-36 | Immich 1.36 (Live Photos) | L2 | 2023 | iOS Live Photo motion part uploaded; old assets must be re-uploaded |
| 12 | https://github.com/immich-app/immich/discussions/3401 | Immich v1.69 motion photos | L4 | 2023 | Android motion video extracted to separate file server-side |
| 13 | https://docs.photoprism.app/getting-started/faq/ | PhotoPrism FAQ | L1 | undated | thumbs main storage cost; on-demand possible; RAW via Darktable/RawTherapee sidecar; ffmpeg to AVC on demand; native tools exceed Go mem limits |
| 14 | https://github.com/pulsejet/memories/wiki/FAQ | Nextcloud Memories FAQ | L2 | undated | go-vod on-demand transcoder + HLS |
| 15 | https://github.com/pulsejet/memories/issues/936 ; https://help.nextcloud.com/t/hardware-transcoding-go-vod-dont-work/192828 ; https://github.com/nextcloud/all-in-one/discussions/5450 | Memories HW transcoding threads | L4 | 2023-25 | VAAPI/NVENC fail on device permissions, container GPU visibility, HTTP 408 |
| 16 | https://help.nextcloud.com/t/auto-upload-stops-ios/88131 ; https://help.nextcloud.com/t/ios-background-sync/145197 | Nextcloud iOS auto-upload threads | L4 | 2019-25 | auto-upload often works only in foreground; location theory unconfirmed |
| 17 | https://www.synology.com/en-global/dsm/7.0/software_spec/synology_photos ; https://synology.com/en-us/releaseNote/SynologyPhotos | Synology Photos spec/release notes | L1 | 2025-26 | thumbnails via browser/mobile to cut server load; Photos 1.7.0 moved HEIC/HEVC processing to clients |
| 18 | https://github.com/ente/ente/discussions/6584 ; https://github.com/shahil-kv/Memvo/issues/8 | Ente video streaming (secondary) | L4 | 2025-26 | Ente makes 720p H.264 HLS on device, beta; server cannot transcode E2EE data |
| 19 | https://ente.com/architecture/ | Ente architecture | L1 | undated | client-side encryption; nothing on thumbnails/video |
| 20 | https://github.com/libvips/libvips | libvips README | L1 | live | threaded, low memory, LGPL-2.1+, HEIC/AVIF via libheif, fuzzed |
| 21 | https://github.com/libvips/libvips/wiki/Speed-and-memory-use | libvips speed/memory | L2 | 2025 | JPEG pipeline 0.82 s vs ImageMagick 4.38 s, ~286 vs ~1499 mem (16-core TR PRO) |
| 22 | https://www.libvips.org/API/current/ctor.Image.thumbnail.html | vips thumbnail | L1 | live | shrink-on-load -> block shrink -> Lanczos3; EXIF auto-rotate |
| 23 | https://www.libvips.org/API/current/func.block_untrusted_set.html | VIPS_BLOCK_UNTRUSTED | L1 | live | env var blocks operations flagged untrusted |
| 24 | https://lilting.ch/en/articles/rails-cve-2026-66066-active-storage-libvips | CVE-2026-66066 write-up | L3 | 2026 | libvips-via-Rails issue; blocking untrusted ops did not cover 4 of 15 formats tested |
| 25 | https://github.com/Cykooz/fast_image_resize | fast_image_resize README | L2 | live | 4928x3279->852x567 RGB: image 78-174 ms, libvips 6-16 ms, fir AVX2 3.5-13 ms |
| 26 | https://github.com/image-rs/image/issues/2223 | image crate resize speed | L4 | 2024 | image ~5x slower than libvips, ~20x slower than fir AVX2 |
| 27 | https://github.com/etemesi254/zune-image | zune-image | L2 | live | zune-jpeg: pure Rust, SIMD, fuzzed daily; no perf numbers, scaled decode not documented |
| 28 | https://docs.rs/crate/turbojpeg/latest/source/src/decompress.rs | turbojpeg crate | L2 | live | libjpeg-turbo bindings; DCT-domain scaling 1/8..1/1 |
| 29 | https://github.com/strukturag/libheif | libheif README | L1 | Oct 2026 | LGPL; libde265 (LGPL) decoder, x265 (GPL) encoder only; 89 security advisories Jan-Oct 2026; security limits API |
| 30 | https://docs.rs/crate/libheif-rs ; https://docs.rs/crate/libheif-sys/latest/builds/4049231 | libheif-rs / libheif-sys | L2 | Aug 2026 | maintained wrappers (libheif 1.17+, system or embedded libheif) |
| 31 | https://www.fiercevideo.com/cable/hevc-advance-seeks-to-speed-up-codec-adoption-by-waiving-select-software-royalties ; https://accessadvance.com/topic-what-do-we-license/ | HEVC Advance licensing | L3/L1 | 2015 / live | waiver only for app-layer software downloaded after device sale; pool covers decoders in software or cloud services |
| 32 | https://docs.rs/libvips/latest/libvips/ | `libvips` crate 2.3.0 | L2 | 2026-09-10 | safe-ish bindings; 11% documented |
| 33 | https://mail.openbenchmarking.org/test/pts/x264 | OpenBenchmarking x264 Bosphorus 1080p | L2 | 2022-25 runs | median 96 fps; i3-12100 61; Ryzen 3 3300X (4C) 61; i3-10100 52; N100 26; preset not stated on page |
| 34 | https://github.com/cmoore1776/x264-benchmark | x264 benchmark | L3 | undated | i7-8700K, 10 s 1080p30 clip: veryfast 8.9 s, fast 13.3 s, medium 12.7 s (source clip/threads unclear) |
| 35 | https://mail.openbenchmarking.org/performance/test/pts/svt-av1/534a0cefe4de4b72b731626603f51007c7ef973b (via search) ; https://mail.openbenchmarking.org/test/pts/svt-av1 | SVT-AV1 results | L2 | 2023-26 | Ryzen 5 5600X (6C/12T) SVT-AV1 1.6 preset 8 1080p ~79 fps; 4K preset 8 median 153 fps (v4.0, big CPUs) |
| 36 | https://forum.jellyfin.org/t-hw-transcoding-intel-uhd-n100-50-cpu-use-on-1-1080p-stream?pid=21126 | Jellyfin N100 threads | L4 | 2024-25 | N100 4K HEVC->H.264 via QSV ~50 fps (rough); VAAPI vs QSV CPU use |
| 37 | https://research.jfrog.com/vulnerabilities/ffmpeg-is-vulnerable-to-a-heap-out-of-bounds-write-in-the-magicyuv-decoder-cve-2026-8461/ ; https://www.sentinelone.com/vulnerability-database/cve-2026-58049/ ; https://nvd.nist.gov/vuln/detail/CVE-2026-66039 | FFmpeg 2026 CVEs | L2/L1 | 2026 | heap OOB writes in MagicYUV, RASC, MACE6, DVB subtitle, CineForm, TIFF decoders; fix = upgrade or rebuild without decoder |
| 38 | https://github.com/zmwangx/rust-ffmpeg | rust-ffmpeg (ffmpeg-next) | L2 | live | "maintenance mode"; new FFmpeg APIs need contributor PRs; tracks FFmpeg 3.4-8.0 |
| 39 | https://github.com/pgmq/pgmq | pgmq | L1 | live | SQS-like on Postgres; extension or SQL-only install; visibility timeout; archive; Rust client; PG 14-18 |
| 40 | https://github.com/geofmureithi/apalis ; https://docs.rs/apalis-pgmq | apalis | L1/L2 | 2026 | tower-based job framework 1.0; Redis/Postgres/SQLite/PGMQ backends; apalis-pgmq still 0.1.0-rc |
| 41 | https://developer.apple.com/documentation/photokit/uploading-asset-resources-in-the-background | PhotoKit Background Resource Upload | L1 | 2026 | system-run upload extension (iOS 26.1; protocol `PHBackgroundResourceUploadJobExtension` iOS 27) |
| 42 | https://9to5mac.com/2025/10/24/ios-26-1-third-party-photos-backup-background/ | iOS 26.1 third-party backup | L3 | 2025-10-24 | confirms extension type introduced in 26.1 |
| 43 | https://koofr.eu/blog/posts/understanding-background-auto-upload-on-ios | Koofr iOS background upload | L3 | 2026 | adopter guidance: Full Photo Access, Background App Refresh, no force-quit; first run waits for Wi-Fi and battery >80% |
| 44 | https://developer.apple.com/documentation/backgroundtasks/bgprocessingtaskrequest | BGProcessingTaskRequest | L1 | live | fetched body empty; summary from fetch tool's own knowledge only [limited source] |
| 45 | https://developer.apple.com/videos/play/wwdc2025/227/ ; https://developer.apple.com/forums/thread/801126 | BGContinuedProcessingTask | L1/L4 | 2025 | iOS 26 task for user-started work continuing after backgrounding; forum reports it not firing on release |
| 46 | https://developer.apple.com/documentation/photos/phassetresourcetype | PHAssetResourceType | L1 | live | `photo`, `pairedVideo`, `fullSizePairedVideo`, `alternatePhoto`, `adjustmentData` ... |
| 47 | https://developer.android.com/develop/background-work/services/fgs/timeout | FGS timeouts | L1 | live | Android 15: dataSync/mediaProcessing FGS 6 h per 24 h, reset when app foregrounded |
| 48 | https://developer.android.com/develop/background-work/background-tasks/uidt | User-initiated data transfer | L1 | live | Android 14 UIDT must be scheduled while app visible; notification required |
| 49 | https://developer.android.com/develop/background-work/background-tasks/persistent/getting-started/define-work | WorkManager define work | L1 | live | constraints (UNMETERED, charging), periodic min 15 min, expedited quotas |
| 50 | https://developer.android.com/reference/androidx/work/Constraints ; https://ark.sudovanilla.org/Infrastructure/immich/commit/4fe535e5e88cda4e7f3ed5c07f0ffc37e8623165 | Content-URI triggers; Immich commit | L1/L3 | 2024 | `addContentUriTrigger` (API 24+) with update-delay/max-delay; Immich uses it plus a rescheduling service for killed app |
| 51 | https://developer.android.com/training/data-storage/shared/photopicker | Photo picker | L1 | live | picker grants selected media only; 5,000 grant cap |
| 52 | https://support.google.com/googleplay/android-developer/answer/14115180 | Play photo/video permissions policy | L1 | enforced 2025-05-28 | READ_MEDIA_IMAGES/VIDEO only for core functionality; declaration; backup apps not named |
| 53 | https://support.google.com/googleplay/android-developer/answer/13392821 | Play foreground service declaration | L1 | 2024 | dataSync FGS must be user-initiated; demo video "cannot show fully automatic or background backup" |
| 54 | https://developer.android.com/media/platform/motion-photo-format | Motion Photo format 1.0 | L1 | live | JPEG/HEIC/AVIF + appended MP4/MOV; Container XMP `Item:Length`; video last; video AVC/HEVC/AV1 |
| 55 | https://pub.dev/packages/background_downloader | background_downloader 9.6.5 | L2 | 2026 | iOS URLSession; Android WorkManager (9 min cap) / UIDT; iOS 4 h resource timeout; force-quit kills |
| 56 | https://pub.dev/packages/photo_manager | photo_manager 3.12.0 | L2 | 2026 | PhotoKit/MediaStore; limited-access state; iCloud progress; Live Photo filters; HEIC originFile caveat |
| 57 | https://pub.dev/packages/workmanager | workmanager 0.10.10 | L2 | 2026 | wraps iOS background tasks + Android WorkManager |
| 58 | https://developer.apple.com/app-store/review/guidelines/ | App Review Guidelines | L1 | live | 2.1(a) demo account + live backend; 2.5.4 background services only for intended purposes |
| 59 | https://bitmovin.com/blog/apple-av1-support ; https://www.scientiamobile.com/?p=29961 | Apple AV1 support; AV1 HW adoption | L3 | 2023-24 | HW AV1 decode from A17 Pro; 8.46% of devices in 2024 Q1 |
| 60 | https://caniuse.com/hevc | HEVC support table | L2 | live | Chrome/Edge/Safari "partial", Firefox depends on OS decoder |

## Fact cards

### A. What peers do (thumbnails, transcoding, formats, dedup)
1. Immich makes three derivatives per asset: thumbhash blur, WebP thumbnail, preview (default 1440p JPEG/WebP), as separate jobs. [3][4] High
2. Immich originals are never modified; transcode only if policy says (default `required`: not h264/aac-compatible container/codec); default target h264, preset `ultrafast`, 720p, AAC; AV1/HEVC/VP9 optional. [3][4] High
3. Immich says transcoding and ML are the CPU-heaviest jobs, thumbnails next; advice: concurrency 1, transcode threads 1-2, container CPU limits. [3] High
4. Immich warns thumbnail concurrency above core count hurts API responsiveness without speeding generation. [4] High
5. Immich's own docs say 4 cores recommended, 2 minimum; thumbs+transcodes add 10-20% storage. [7] High
6. Immich HW transcode: NVENC/QSV/RKMPP/VAAPI, Linux only, encode-only by default, "experimental", larger lower-quality files vs software, QSV needs recent Intel gens for VP9. [1] High
7. Immich thumbnails run on sharp (libvips); HEIC failures after iOS 18 traced to libheif/libvips mismatch; 200 MP HEIC reported failing. [L4 issue summaries via search; 3rd-party guide] Med [limited source]
8. Nextcloud Memories HW transcoding: recurring failures from GPU device passthrough, permissions, go-vod HTTP 408. [15] Med (L4)
9. PhotoPrism: thumbnails are the main storage cost; can render on demand on fast CPU; RAW via sidecar from Darktable/RawTherapee; transcodes to AVC via ffmpeg; Go memory limits do not cover libvips/FFmpeg child memory; indexing memory spikes. [13] High
10. Synology Photos 1.7.0 moved HEIC/HEVC processing from NAS to clients/browsers; spec says mobile-side conversion and browser thumbnails reduce server load. [17] Med
11. Ente cannot transcode server-side (E2EE) so it generates 720p H.264 HLS on the device (beta). [18] Low-Med (secondary source)
12. Immich dedups by content hash at upload, per library only; identical files from two users stay separate assets; first album selection hashes every local file (iCloud-only assets are downloaded to hash). [3][6] High
13. iOS Live Photo = two PhotoKit resources (`photo` + `pairedVideo`) linked by Content Identifier; Immich pairs by same-request upload or ContentIdentifier. [46][11] High/Med
14. Android Motion Photo = still (JPEG/HEIC/AVIF) with MP4/MOV appended at end of file; location given by Container XMP `Item:Length`; video is AVC/HEVC/AV1. Server extracts it (Immich since v1.69). [54][12] High

### B. Thumbnailing in/for Rust
15. libvips thumbnail = shrink-on-load, then block shrink, then Lanczos3; horizontally threaded, demand-driven. [20][22] High
16. libvips JPEG thumbnail-style pipeline: 0.82 s vs ImageMagick 4.38 s (5.3x), memory ~286 vs ~1499 (5x); benchmark is I/O-heavy and flatters libvips (page says so). [21] Med
17. Resize-only, 4928x3279->852x567 RGB8: `image` 78 ms bilinear / 174 ms Lanczos3; libvips 5.7 / 16 ms; fast_image_resize AVX2 3.5 / 13 ms (single thread, hardware unstated). [25] Med
18. `image` crate resize is ~5x slower than libvips and ~20x slower than fir-AVX2; the default JPEG decoder is zune-jpeg. [26][27] Med
19. libjpeg-turbo (via `turbojpeg` crate) can decode directly at 1/2, 1/4, 1/8 scale in the DCT domain; this is the main reason shrink-on-load is fast. [28] High
20. zune-jpeg: pure Rust (unsafe only for SIMD), daily CI fuzzing; no published throughput and no documented scaled decode. [27] Med
21. libheif is LGPL; default HEVC decoder libde265 is LGPL; x265 (GPL) is encoder only, not needed for thumbnails. [29] High
22. libheif has had 89 security advisories Jan-Oct 2026 (maintainer statement); security limits exist (`heif_security_limits`; env `LIBHEIF_SECURITY_LIMITS=off` disables them, only for trusted files). [29] High
23. Rust bindings: `libheif-rs`/`libheif-sys` maintained (Aug 2026 build, libheif 1.23); `libvips` crate 2.3.0 (Sep 2026). [30][32] Med
24. HEVC patent pools (Access Advance) list decoders in software or cloud services as licensable; the 2015 software waiver covers only app-layer software downloaded after device sale. No source found on enforcement against open-source servers. [31] Med [limited source]
25. VIPS_BLOCK_UNTRUSTED disables loaders flagged untrusted; a third-party test found 4 of 15 formats still loaded with it set; build-dependent. [23][24] Med [limited source]
26. No published per-core numbers found for 12 MP JPEG or HEIC decode; one 2021-era report: 4K HEIC ~2 s RGBA on i7-8700K with libheif 1.12, heif-convert ~0.4 s/image on small files. [search results only] Low [limited source]

### C. Video
27. x264 Bosphorus 1080p (preset unstated): Ryzen 3 3300X 4C = 61 fps, i3-12100 (4C/8T) = 61, i3-10100 = 52, Intel N100 = 26, median of all CPUs 96. [33] Med
28. x264-benchmark, i7-8700K, 10 s 1080p30 clip: veryfast 1.13x realtime, fast 0.75x, medium 0.79x (source clip and thread cap unclear, conflicts with row 27). [34] Low
29. SVT-AV1 1.6 preset 8, 1080p, Ryzen 5 5600X (6C/12T) ~79 fps; scales with cores; no 4-vCPU number found. [35] Med [limited source]
30. A "4 vCPU" cloud worker is usually 2 physical cores with SMT, so expect roughly 50-65% of a 4C/8T desktop number. Inference. Low
31. Hardware H.264 encode on QSV (N100) did 4K HEVC->H.264 at ~50 fps; VAAPI used ~3x more CPU than QSV on the same box. [36] Low (L4)
32. rust-ffmpeg (ffmpeg-next) is in maintenance mode, new FFmpeg APIs need outside PRs. [38] High
33. FFmpeg 2026 CVEs: heap OOB writes in MagicYUV (8.8 CVSS), RASC, MACE6, CineForm, DVB subtitle, TIFF; mitigation is upgrade or rebuild with the decoder disabled. [37] Med (aggregators)
34. HEVC playback: caniuse shows Chrome/Edge/Safari "partial", Firefox depends on the OS decoder; Safari/iOS native; broad but not universal. [60] Med
35. AV1: iOS needs hardware (A17 Pro+, no software fallback in Safari per 2024 reports); hardware decode in ~8.5% of devices in 2024 Q1; Android support is chip-dependent. [59] Med
36. Motion-photo and Live Photo videos are short (seconds), so they add little transcode load. Inference. Low

### D. Job queue
37. pgmq: plain SQL objects (no worker, extension optional), visibility timeout = at-least-once with exactly-once inside the window, archive table, Rust client, PostgreSQL license. No priority feature documented on the README. [39] High
38. apalis 1.0: tower-based framework with retries, concurrency, rate limit, cron, idempotency; Postgres/Redis/SQLite backends; `apalis-pgmq` still 0.1.0-rc. [40] Med
39. Immich splits `api` and `microservices` workers and exposes per-job concurrency; its queue backend is not stated in the docs I could fetch. [2] High / backend [limited source]

### E. iOS
40. iOS 26.1 added PhotoKit "Background Resource Upload": a Swift extension (`com.apple.photos.background-upload`) that the system runs "when conditions allow" (network, power, device activity), even when app is closed or device locked; needs full library access; one job per `PHAssetResource`; inflight job limit; progress tracked via `fetchPersistentChanges`; extension must be low-memory and finish within a runtime limit. [41][42] High
41. That extension uploads with the IETF resumable-upload draft: server must answer `OPTIONS` with `Upload-Limit` (200) or 501, and send `104 Upload Resumption Supported` with `Location`; `Upload-Offset` on completion. [41] High
42. Protocol is `PHBackgroundResourceUploadJobExtension` on iOS 27 (26.1 class deprecated in 27); Developer Mode removes throttling in test only; no Simulator support. [41] High
43. No published latency/SLA for the extension; Apple only says the system schedules by network, power, device activity. [41] High (absence)
44. Before it: BGAppRefresh ~30 s opportunistic, BGProcessing minutes, usually idle+charging, overnight; scheduling never guaranteed; force-quit or Background App Refresh off stops everything. [44][9][3] Med
45. Background URLSession transfers continue when the app is suspended, but a force-quit stops them; `background_downloader` default resource timeout 4 h. [55][43] Med
46. Immich still fixing iOS background races in Oct 2026 (batch waits for remote sync; background uploads cancelled when foreground starts; skip assets server already has). Mobile backup on iOS remains an ongoing maintenance cost. [8] High
47. Immich FAQ: iOS decides background runs; use Background App Refresh, avoid Low Power Mode, open app more often. [3][6] High
48. Koofr (adopter, 2026): needs Full Photo Access; first run waits for stable Wi-Fi and battery >80%; closing the app manually stops uploads. [43] Med
49. Nextcloud iOS users report auto-upload works mainly with app foregrounded; location-change trick is forum speculation. [16] Low (L4)
50. No source found confirming that "significant location change" relaunch is a reliable backup trigger. Guideline 2.5.4 allows background services only for intended purposes (VoIP, audio, location, task completion, notifications), so a location-for-backup trick risks rejection. [58] Med/High for text; risk is my inference
51. BGContinuedProcessingTask (iOS 26) is for user-started tasks continuing after backgrounding; early reports it did not fire on release builds. Not an automatic-backup tool. [45] Low-Med
52. App Review 2.1(a): demo account (or approved demo mode) and a live backend are required; a self-hosted client therefore needs a public demo server or prior approval. [58] High
53. `photo_manager`: PhotoKit+MediaStore, limited-access state, iCloud progress handler, Live Photo filters; `originFile` HEIC limited in Flutter; iOS file access copies asset into sandbox. [56] High

### F. Android
54. Content-URI trigger in WorkManager (`addContentUriTrigger`, API 24+) with update-delay and max-delay; Immich uses it and added a rescheduler for process kills. [50] Med
55. WorkManager: periodic min 15 min; constraints `UNMETERED`, `RequiresCharging`; expedited work is quota-limited in background. [49] High
56. Android 15: `dataSync` and `mediaProcessing` FGS capped at 6 h/24 h; Android 14 UIDT jobs must be scheduled while app visible. Neither suits unattended automatic backup. [47][48] High
57. Play policy: dataSync FGS uses must be user-initiated and review videos "cannot show fully automatic or background backup". Automatic camera backup must therefore run as WorkManager jobs, not an FGS. [53] High
58. Play policy for READ_MEDIA_IMAGES/VIDEO: allowed only for core functionality via Play Console declaration, enforced since 2025-05-28; backup apps not named in the policy page. [52] High / backup eligibility Med
59. Android 14 partial photo access (`READ_MEDIA_VISUAL_USER_SELECTED`): app sees only picked items; `photo_manager` exposes a `limited` state; backup must demand "Allow all". [56][51] High
60. `background_downloader`: Android WorkManager tasks capped ~9 min unless `allowPause`/`largeFile`; force-closing app may stop transfers; no FGS. [55] High
61. Immich Android: options for minimum delay after capture and charging-only; OEM battery optimizers can block backup (points to dontkillmyapp.com). [6] High
62. Upload bytes are not the bottleneck: a 12 MP JPEG is roughly 3-6 MB (typical, not sourced) so ~1 s on 50 Mbps Wi-Fi. Low [limited source]

## Options comparison tables

### T1. Thumbnail engine (server worker)
| Rank | Option | Pros | Cons | Verdict |
|---|---|---|---|---|
| 1 | libvips (system lib or `vipsthumbnail`) run as unprivileged subprocess, `VIPS_BLOCK_UNTRUSTED=1`, libheif security limits | shrink-on-load, threaded, 5x faster / 5x less memory than ImageMagick [21]; HEIC/AVIF/WebP/JXL/PDF via one API; used at Immich scale; EXIF auto-rotate | C library, LGPL, CVE stream (libvips + libheif 89 advisories) so needs sandbox and image rebuilds | Default |
| 2 | Hybrid Rust: `turbojpeg` (scaled decode) + `fast_image_resize` + `libheif-rs` for HEIC only | fastest JPEG path; resize 3.5-13 ms [25]; few moving parts per format | JPEG still C; HEIC still libheif; you own orientation/ICC/format glue (more code than libvips) | Fallback if libvips packaging hurts |
| 3 | Pure Rust `image` + zune-jpeg + fir | memory-safe JPEG/PNG/WebP | no HEIC; full-size decode (no documented scaled decode [27]); `image` own resize 5-20x slower [26] | JPEG-only MVP |
| 4 | ImageMagick | widest format list | 5x slower, 5x memory [21]; biggest attack surface (libvips README warns) [20] | Avoid |
| - | Client-supplied thumbnails (Synology/Ente style) [17][18] | zero server CPU; required for vault | battery on phones, untrusted client content, HEIC in browsers | Vault only (already decided); optional later for iOS HEIC preview |

### T2. Video output codec (compatibility derivative)
| Rank | Codec | Playback | Encode cost (CPU) | Notes |
|---|---|---|---|---|
| 1 | H.264 High, yuv420p 8-bit, AAC, MP4 faststart, max 1080p | everywhere (all browsers, iOS, Android) | cheapest: ~26-61 fps 1080p on 4C parts [33] | Default; keep original |
| 2 | HEVC | iOS native, Chrome/Edge/Safari partial, Firefox OS-dependent [60] | 2-3x x264 (inference), patent-pool exposure for software encoders [31] | Do not transcode to it; just keep originals |
| 3 | AV1 (SVT-AV1) | iPhone 15 Pro+ only in HW, ~8.5% HW share 2024 [59] | preset 8 ~79 fps on 6C/12T [35] so ~25-35 fps on 4 vCPU (inference); lower presets slow | Optional later archive tier |
| 4 | VP9 | Chrome/Firefox yes; iOS partial [limited source] | slow in libvpx | Skip |
Policy: transcode only when original is not web-compatible (HEVC, >1080p, odd container, high bitrate); otherwise remux/faststart. Same as Immich `required` [4].

### T3. FFmpeg integration
| Rank | Option | For | Against |
|---|---|---|---|
| 1 | `ffmpeg`/`ffprobe` subprocess, `-progress pipe:1`, rlimits/cgroup, killed on timeout | crash and CVE isolation [37]; HW flags are just args; any FFmpeg version; no FFI build pain | parse stderr/progress; process spawn (~ms) |
| 2 | `ffmpeg-next` bindings | in-process, frame access | maintenance mode [38]; decoder bug = worker crash/RCE in API-adjacent process; harder static builds |
Build FFmpeg with decoder/demuxer allow-list (h264, hevc, vp8/9, av1, mjpeg, aac, mp3, opus, pcm) to drop obscure decoders behind the 2026 CVEs [37].

### T4. Hardware acceleration (optional per worker)
| Option | Needs | Caveats |
|---|---|---|
| CPU libx264 | nothing | default; meets AC-11 only with preset veryfast/fast on a decent 4 vCPU |
| Intel QSV / VAAPI | `/dev/dri` passthrough, recent iGPU | larger/lower-quality output; device permission failures common [1][15]; QSV beat VAAPI on N100 [36] |
| NVIDIA NVENC | driver + container toolkit | best quality of HW encoders (two-pass only NVENC [1]); GPU cost |
| VideoToolbox (macOS worker) | Mac host, no Docker GPU | not researched in depth [limited source] |
Rule: HW is a per-worker `ffmpeg` argument profile, same output contract, never required.

### T5. Job queue
| Rank | Option | For | Against | Changes if |
|---|---|---|---|---|
| 1 | Plain `jobs` table, `FOR UPDATE SKIP LOCKED`, lease (`locked_until`), `attempts`, `run_after`, unique `(kind, content_hash, params_ver)` | zero new component; SQL-visible admin UI; per-kind lanes and concurrency trivial; ~150 lines | we own lease/retry bugs | need cron/DAG workflows |
| 2 | pgmq (SQL-only install, `pgmq-rs`) | vetted visibility-timeout + archive semantics [39] | no documented priority/lanes (use one queue per kind); extra schema | team prefers a library |
| 3 | apalis + postgres backend | retries, metrics, cron, tower middleware [40] | framework weight; apalis-pgmq pre-release | many job kinds appear |
| 4 | Redis queue (Immich-style BullMQ [limited source]) | very high throughput | extra service; AC-22 30-min install; rehydrate/persistence concerns | >100 jobs/s sustained |
Throughput need is tiny (hundreds of users); Postgres suffices.

### T6. iOS backup approach
| Rank | Approach | Latency reality | Cost/risk |
|---|---|---|---|
| 1 | iOS 26.1+: PhotoKit Background Resource Upload extension + foreground catch-up on app open + `BGAppRefresh` nudge | system decides; minutes-hours, no SLA [41][43] | native Swift extension (+ app group state); server must speak IETF resumable upload (OPTIONS/104) [41]; iOS 26.1+ only |
| 2 | iOS <26.1 fallback: foreground upload + BGProcessing (charging) + background URLSession | next idle+charging window (often overnight) [9][44] | `workmanager` + `background_downloader` cover most; 4 h timeout |
| 3 | Location-based relaunch ("significant location change") | unproven [50] | Guideline 2.5.4/5.1.5 rejection risk; privacy cost |
| 4 | Silent push from server on "new file" | not applicable (photo is on phone) | n/a |

### T7. Android backup approach
| Rank | Approach | Notes |
|---|---|---|
| 1 | Native Kotlin: WorkManager one-time work with content-URI trigger on MediaStore + `UNMETERED` + user "min delay"; periodic 15-min safety scan; upload via chunked resumable client inside the worker | Same pattern Immich converged on [50]; no FGS needed so complies with Play dataSync rule [53] |
| 2 | Flutter `workmanager` + `photo_manager` + `background_downloader` | fine for scan/queue logic; content-URI trigger and process-kill rescheduler need native code [50][57] |
| 3 | FGS `dataSync` for automatic backup | Play policy rejects automatic backup use [53]; 6 h/24 h cap [47] |

## AC realism table

| Criterion | Our value | Researched value | Note |
|---|---|---|---|
| AC-9 Android | <=60 s, Wi-Fi | Trigger via content-URI job: seconds to minutes when app not battery-restricted [50][61]; upload ~1-5 s [62]. Under OEM killers/Doze: minutes to hours; no published latency stats [limited source] | Achievable p50; p95 depends on OEM. Rephrase: "<=60 s with battery optimization exempted and Wi-Fi; otherwise next job run" |
| AC-9 iOS | <=15 min | Foreground/just-opened: seconds-minutes. Background: OS-scheduled, hours/overnight typical pre-26.1 [9][44]; 26.1+ extension has no SLA [41][43]; Immich still patching [8] | 15 min is not guaranteeable in background; keep "best effort" (restrictions.md already says so). Needs native Swift extension and IETF resumable-upload server support |
| AC-10 | thumb <=5 s p95 | Resize 6-16 ms [25]; with scaled JPEG decode a 12 MP JPEG should cost ~50-150 ms CPU, HEIC ~0.3-0.6 s [estimates, unsourced, Low] | Trivially met in steady state; the real risk is queue wait during bulk backlog or when a transcode hogs the box. Needs a dedicated thumbnail lane (>=1 vCPU reserved, priority, newest-first), enqueue on upload commit. Define AC for "non-bulk uploads" |
| AC-11 | 10 min 1080p in <=10 min on 4 vCPU | x264 1080p: 61 fps (4C/8T desktop), 26 fps (N100) [33]; 10 min x 30 fps = 18,000 frames: 4.9 min at 61 fps, 11.5 min at 26 fps; 4 vCPU cloud ~50-65% of desktop (inference) -> ~7-10 min at `fast`/`medium`; 60 fps source doubles frames; 4K HEVC source adds decode cost | Realistic only with preset `veryfast`/`fast`, CRF ~23, 1080p cap and a decent CPU; borderline at `medium`. SVT-AV1 preset 8 ~1x realtime at best on 4 vCPU (inference) so not default. "Adding workers raises throughput" is natural with SKIP LOCKED but workers must reach the file store (open Q). State AC with source fps (30) and preset |
| AC-14 | vault <=100 MiB | Server does no thumbnails/transcode for vault, so client must (phone easy; web needs HEIC decode in browser, not researched) [limited source]. Phone video sizes (Apple per-minute sizes not fetched): ~60-130 MB/min at 1080p30, ~170-350 MB/min at 4K30 [estimate, Low] | 100 MiB cap rejects any phone video beyond ~1 min (4K) to ~1-2 min (1080p). Camera-roll auto-backup to a vault would hit the cap constantly; keep camera backup out of vault or raise cap |

Capacity sketch (illustrative assumptions, not sourced): 500 users x 10 photos/day = 5,000 thumbnails/day (<0.1 core average); 500 x 1 min video/day at ~1.5-2x realtime = 4-5 h of one 4-vCPU worker. One worker covers steady state; initial library import (e.g. thousands of videos per user) is the sizing driver and needs low priority + "transcode only if needed".

## Recommendation

**Thumbnails.** libvips in a sandboxed subprocess (or thin `libvips` crate behind a helper binary): `thumbnail` with shrink-on-load, auto-rotate, WebP 320 px grid size + 1440 px preview (browsers cannot show HEIC, so the preview is mandatory for HEIC). Env `VIPS_BLOCK_UNTRUSTED=1`, libheif security limits on, pixel cap (~150 MP), 1-2 GiB memory limit, 10 s timeout, no network. Fallback (rank 2): `turbojpeg` + `fast_image_resize` + `libheif-rs`. Do not use ImageMagick. RAW: use embedded JPEG preview only (Immich offers "prefer embedded preview" [4]); no RAW development.
*Decided by:* 5x speed/memory over ImageMagick [21], one API for HEIC/AVIF, peer precedent (Immich uses libvips via sharp), AC-10 has >30x headroom. *Would change if:* packaging libvips+libheif in the image proves painful, or the CVE stream demands memory-safe-only decode (then Rust hybrid, accept HEIC via libheif only in a stricter sandbox).

**Video.** `ffmpeg` subprocess, never in-process. Default derivative: H.264 High 8-bit yuv420p, AAC, MP4 `+faststart`, scale to <=1080p, `-preset veryfast` (`fast` if worker >= 4 fast cores), CRF 23, `-threads` = lane vCPUs, `-nostdin`, protocol whitelist `file`, timeout = 3x duration. Transcode only when the original is not web-compatible; otherwise remux. Keep originals. FFmpeg built with a decoder allow-list and patched promptly. HW accel as optional per-worker arg profile (QSV/NVENC/VAAPI/VideoToolbox), CPU is the supported baseline. No HEVC/AV1 output now.
*Decided by:* universal playback of H.264, x264 numbers meeting AC-11 only at fast presets [33], 2026 FFmpeg decoder CVEs favoring process isolation [37], rust-ffmpeg maintenance mode [38]. *Would change if:* storage cost dominates (add AV1 archive tier on HW AV1 decode share growth) or streaming enters scope (HLS ladder).

**Queue.** Postgres `jobs` table with `SKIP LOCKED`, leases and heartbeats, `run_after` backoff, unique key `(kind, content_hash, params_version)`; separate lanes: thumbnail (1 vCPU reserved, high priority), metadata/EXIF, transcode (rest). Idempotent: write derivative to temp path, fsync, atomic rename to content-addressed path; stale results harmless under last-write-wins because key includes hash. Poison job after N attempts surfaces in admin UI. pgmq is the acceptable off-the-shelf alternative. No Redis.
*Would change if:* >100 jobs/s or workflow DAGs (then apalis/Redis).

**Mobile backup.**
- Android: native Kotlin worker (WorkManager, MediaStore content-URI trigger, UNMETERED, user-set min delay, 15-min periodic safety scan, rescheduler for killed process); not an FGS (Play dataSync policy [53]). Declare READ_MEDIA_IMAGES/VIDEO in Play Console as core backup function; require "Allow all" (not partial access).
- iOS: target iOS 26.1+ PhotoKit Background Resource Upload extension (Swift) as primary background path, plus foreground catch-up on app open and BGAppRefresh/BGProcessing as nudges; older iOS = foreground + charging window. Skip location tricks (Guideline 2.5.4/5.1.5 risk, unproven). Document honestly: iOS background is best effort, no SLA.
- Flutter side: `photo_manager` (enumeration, limited-access state, iCloud progress), `workmanager`, `background_downloader` for foreground/fallback uploads; native code needed for the iOS extension and Android content trigger worker.
- Server consequence (cross-area, flag to protocol research): upload endpoint should implement IETF resumable-upload draft (OPTIONS with `Upload-Limit`, `104` + `Location`, `Upload-Offset`) so the iOS extension can use it; dedup by SHA-256 per user before upload (`HEAD`/exists check); Live Photo = two resources linked by content identifier; Android Motion Photo is one file whose video server extracts from appended MP4.
- Store review: Apple needs live demo server + credentials (or pre-approved demo mode) in every submission [58]; Play needs photo-permission and (if used) FGS declarations.
*Would change if:* Apple's extension proves unreliable in field data (then lean on foreground + charging window and set user expectation), or Play rejects the media permission declaration (fall back to photo picker flow, which breaks automatic backup).

## Open questions
1. Per-core decode time of 12 MP JPEG/HEIC with libvips+libheif: no sourced number; run the 100-photo set from `data_parameters.md` on the target worker before fixing AC-10 sizing.
2. HEVC patent exposure when the project ships libheif+libde265 in its Docker image (decode only): legal question, no enforcement evidence found. Ship HEIC decode as default? Or make the HEVC plugin opt-in?
3. x264 preset behind the OpenBenchmarking numbers is unstated; measure `-preset veryfast|fast|medium` on a real 4-vCPU target with the 10-min 1080p30 test clip, plus a 4K60 HEVC iPhone clip (decode+scale cost).
4. Workers on other hosts need access to originals: depends on storage research (shared FS vs signed-URL fetch vs object store). Affects "adding workers increases throughput".
5. EXIF/metadata extraction library (exiftool vs `nom-exif`/`kamadak-exif`) was not researched; exiftool is a Perl process with its own attack surface.
6. Web vault client: how to thumbnail/preview HEIC in browser (wasm libheif) not researched.
7. Actual latency distribution of Android content-URI triggers and the iOS 26.1+ extension: no published data; needs device-lab measurement (Pixel, Samsung, Xiaomi; iPhone on iOS 26.1/27).
8. Does Apple allow the extension for apps whose server URL is user-supplied (self-hosted)? `BackgroundUploadURLBase` is a fixed Info.plist string [41]; a per-operator server URL may conflict. Not confirmed, verify before committing to iOS plan.
9. Play Store acceptance of READ_MEDIA_* declaration for a self-hosted backup app: policy page does not name backup apps [52]; check Immich/Nextcloud Play listings' declared use.
10. Cross-user dedup is out (per-user hash only), consistent with encryption at rest; confirm with security area.
