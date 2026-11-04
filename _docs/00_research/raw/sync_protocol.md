# Sync protocol, transfer performance, change notification, desktop watcher (raw research)

Date: 2026-11-02. Scope: AC-2, AC-3, AC-5, AC-7, AC-8, AC-15, AC-18 (+AC-4, AC-21 touch points).
Method note: the web tools returned page summaries; claims that a fetch did not confirm are tagged [limited source]. Numbers marked "computed" are arithmetic from cited values; "measured" are my own runs on this workstation (AMD Ryzen 9 3900XT, 24 threads, SHA-NI, no AVX-512, tmpfs/page cache - an upper bound, not a cold-disk or NTFS figure).

## Sources

Tiers: L1 official/spec, L2 maintainer/benchmark/engineering blog, L3 blog, L4 forum.

1. https://dropbox.tech/infrastructure/rewriting-the-heart-of-our-sync-engine - Rewriting the Heart of Our Sync Engine (Nucleus). L2, 2020-03. Strong-consistency protocol, stable file ids, atomic moves, single deterministic control thread, seeded simulation testing.
2. https://dropbox.tech/infrastructure/-testing-our-new-sync-engine (via https://www.infoq.com/news/2020/04/dropbox-testing-sync-engine) - Testing sync at Dropbox. L2, 2020-04. Three trees (remote/local/synced), CanopyCheck planner checker, Trinity concurrency tester. Seen via search summaries only.
3. https://dropbox.tech/infrastructure/streaming-file-synchronization - Streaming File Synchronization. L2, date not shown (older post). 4 MB blocks, per-block SHA-256 blocklist, "need blocks" commit protocol, streaming sync up to 2x.
4. https://docs.syncthing.net/specs/bep-v1.html - Block Exchange Protocol v1. L1. Blocks 128 KiB-16 MiB, sequence-numbered delta index, version vectors, LZ4, 500 MB message cap.
5. https://docs.syncthing.net/users/syncing.html - Understanding Synchronization. L1. SHA-256 blocks, block reuse, 10 s watcher batching, conflict rule (older mtime loses, tie by device id), case-conflict handling.
6. https://docs.syncthing.net/users/faq.html - Syncthing FAQ. L1. inotify limit ~8192 default on old kernels, fallback to periodic scan.
7. https://forum.syncthing.net/t/why-does-periodically-restarting-syncthing-dramatically-improve-overall-transfer-speed/17875 and https://forum.syncthing.net/t/system-is-slow-on-windows-look-here/21925 - QUIC slower than TCP on LAN (250 vs 600 Mbit/s), tiny default UDP buffer on Windows. L4, 2022-2024.
8. https://docs.nextcloud.com/server/stable/developer_manual/client_apis/WebDAV/chunking.html - Nextcloud chunked upload v2. L1. Chunks 5 MB-5 GB, max 10,000 chunks, assemble via MOVE, 24 h expiry.
9. https://help.nextcloud.com/t/max-chunked-upload-size-desktop-client-and-server-connection-closed/130292 - Desktop dynamic chunk size up to 1000 MB broke behind 512 MB proxy limits. L4. (Client default 10 MB, dynamic sizing, from a desktop docs/source mirror via search.)
10. https://nextcloud.com/de/?p=10701 (File Packing / Sync Engine 2.0 announcement) and https://help.nextcloud.com/t/syncing-25k-small-files-need-faster-solution/234301 - small-file bundling; users still report ~10 files/s. L2/L4, 2021 and 2025.
11. https://manual.seafile.com/develop/data_model/ - Seafile data model. L1. Git-like commits/fs/blocks, content-defined chunking, ~8 MB average block.
12. https://tus.io/protocols/resumable-upload - tus 1.0.0. L1. HEAD/PATCH offset resume, checksum and concatenation extensions, HTTP-version independent.
13. https://developers.google.com/drive/api/guides/manage-uploads - Drive resumable uploads. L1. 256 KiB multiples, "as large as possible", session 1 week.
14. https://learn.microsoft.com/en-us/graph/api/driveitem-createuploadsession - OneDrive upload session. L1, updated 2026-08. Fragment <60 MiB, multiple of 320 KiB, recommended 5-10 MiB, 10 MiB optimal on stable links.
15. https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html - S3 multipart limits. L1. 5 MiB-5 GiB parts, 10,000 parts.
16. https://docs.aws.amazon.com/cli/latest/topic/s3-config.html - AWS CLI defaults. L1. 8 MB chunk, 10 concurrent requests.
17. https://rclone.org/onedrive/ - rclone OneDrive. L2. chunk_size default 10Mi, buffered in RAM.
18. https://arxiv.org/abs/2310.09423 - "QUIC is not Quick Enough over Fast Internet" (WWW '24). L1 (peer-reviewed), 2023-10/2024-09. HTTP/3 up to 45.2% lower data rate than HTTP/2 on fast links; cause: receiver-side processing, user-space ACKs.
19. https://www.iankduncan.com/engineering/2026-02-10-http3-not-always-faster/ - HTTP/3 is not always faster. L3, 2026-02. Restates [18]; 231K vs 15K receive calls.
20. https://conferences.sigcomm.org/sigcomm/2020/files/slides/epiq/0%20QUIC%20and%20HTTP_3%20CPU%20Performance.pdf - QUIC CPU performance. L2, 2020. UDP sendmsg up to 3.5x TCP cycles/byte; GSO helps. Seen via search summary.
21. https://docs.rs/crate/quinn/latest - quinn 0.11.12 (2026-09-14). L1. No quantified performance claims; advises larger UDP buffers.
22. https://github.com/hyperium/h3 - h3 crate README. L1. "still very experimental", use "at your own risk".
23. https://docs.rs/hyper/latest/hyper/server/conn/http2/struct.Builder.html - hyper 1.12.0 HTTP/2 builder. L1. max_concurrent_streams 200; window defaults "not stable"; adaptive_window available.
24. https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html - axum 0.8.9. L1. 2 MB default limit on buffering extractors; direct body streaming is not limited.
25. https://docs.rs/tokio/latest/tokio/fs/index.html - tokio 1.54.1 fs. L1. spawn_blocking per op, 2 MB default per-call cap, io_uring only under tokio_unstable.
26. https://docs.rs/crate/armature-framework/latest/source/docs/framework-comparison.md and https://www.bacancytechnology.com/insights/axum-vs-actixweb-vs-rocket - axum vs actix numbers. L4. Contradict each other (actix +10-15% vs axum ahead on JSON).
27. https://papers.freebsd.org/2019/eurobsdcon/shwartsman_gallatin-kernel_tls_harware_offload/ - Netflix kTLS. L2, 2019. TLS breaks sendfile zero-copy; kTLS restores it (FreeBSD: 90 Gb/s at 68% CPU software kTLS).
28. https://github.com/BLAKE3-team/BLAKE3 - BLAKE3 README. L1. Merkle tree, SIMD runtime detection, rayon feature, Bao verified streaming.
29. LOCAL MEASUREMENT 2026-11-02 (this machine): openssl speed sha256 1 MiB = 2.07 GB/s (SHA-NI); AES-128-GCM = 4.99 GB/s (AES-NI); b3sum 1.8.7 on 4 GiB: 1 thread 1.51 s (2.85 GB/s), default threads 0.33 s (13 GB/s); coreutils sha256sum 10.55 s (0.41 GB/s, evidently no SHA-NI path). L2-equivalent, single machine, page cache.
30. LOCAL MEASUREMENT 2026-11-02: tmpfs tree of 100,000 files/10,100 dirs/3.3 GB: Python scandir+stat full walk 0.30 s; BLAKE3 of all files with 8 parallel b3sum processes 0.85 s. Warm cache, tmpfs: lower bound for walk time, not representative of HDD/NTFS.
31. https://www.usenix.org/conference/atc16/technical-sessions/presentation/xia - FastCDC (ATC '16) and https://docs.rs/fastcdc. L1. Gear-based CDC; skipping sub-minimum cut points costs up to ~15% dedup ratio; Rust crate has several non-identical versions. No absolute GB/s found [limited source].
32. https://mutagen.io/documentation/synchronization/ - Mutagen. L2. rsync-style differential transfer, staging outside sync root then atomic relocation, waits for filesystem to stabilize, recursive watching only on Windows/macOS else polling.
33. https://docs.rs/notify/latest/notify/ (8.2.1) and https://github.com/notify-rs/notify - notify. L1. inotify/FSEvents/RDCW backends, MSRV 1.88, documented unreliability on large dirs, PollWatcher fallback, parent watch needed for folder-delete events.
34. https://dev.to/jacksonxly/our-rust-file-watcher-ate-236-gb-of-ram-and-our-ignore-rules-never-had-a-chance-49pn - Rust watcher memory blow-up. L3. FSEvents dropped events -> debouncer rescan; per-root rescan fan-out. Also notes notify-debouncer-full file-id cache needed for rename stitching on FSEvents/Windows.
35. https://man7.org/linux/man-pages/man7/inotify.7.html - inotify(7). L1. Not recursive, IN_Q_OVERFLOW, rename cookies pairing unreliable, mmap writes not reported, no network FS.
36. https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw - ReadDirectoryChangesW. L1. Overflow discards buffer (0 bytes) or ERROR_NOTIFY_ENUM_DIR -> must re-enumerate; 64 KB limit on network paths; fixed buffer for handle lifetime.
37. https://developer.apple.com/documentation/coreservices/file_system_events - FSEvents. L1, but the fetch returned only the title; flag semantics (MustScanSubDirs, UserDropped/KernelDropped, event-id replay, per-device ids) are from model knowledge [limited source].
38. https://android-kvm.googlesource.com/linux/+/14bd41e41899cbd1de4bb5ddfa46c85b08091a69 (and coreos/Flatcar tracker issues) - Linux 5.11 inotify default max_user_watches scaled to ~1% RAM within [8192, 1048576]. L2.
39. https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation - Windows path limits. L1. MAX_PATH 260; 32,767 with \\?\; long paths need registry LongPathsEnabled + longPathAware manifest; no Unicode normalization by NTFS APIs.
40. https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution - notarization. L1, fetch returned only the title; Developer ID + hardened runtime + notarytool, bare binaries cannot be stapled [limited source].
41. https://learn.microsoft.com/en-us/answers/a/12725314 , https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore , https://ngrok.com/blog/windows-ev-certificate - Windows signing/SmartScreen 2026. L3/L4. EV no longer gives instant reputation; Azure Artifact Signing about $10/month; eligibility limits per one blog.
42. https://www.firedaemon.com/post/microsoft-windows-interactive-services-and-session-0-isolation - Session 0 isolation. L3. Services cannot show UI/tray; split service + logon-session UI.
43. https://developer.apple.com/forums/thread/804548 - FDA/TCC for daemons and SMAppService. L4. Grant attaches to the app bundle/responsible process; no clear answer for bare LaunchAgent binary [limited source].
44. https://www.bleepingcomputer.com/news/google/google-shares-fix-for-deleted-google-drive-files/ - Drive for desktop 84.x data loss. L2, 2023-11. Unsynced local files lost, root cause never published.
45. https://github.com/rclone/rclone/issues/7072 and https://forum.syncthing.net/t/german-umlaute-like-aou-seems-to-make-trouble/16804 - NFC/NFD duplicates on macOS. L2/L4.
46. https://fasterdata.es.net/host-tuning/linux/ - ESnet host tuning. L2. 10 Gb/s x 100 ms needs ~120 MB buffer; 4-8 parallel streams typical; leave cubic.
47. https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events - SSE. L1. Auto-reconnect, id field, 6-connection limit on HTTP/1.1, 100 streams on HTTP/2.
48. https://github.com/nextcloud/notify_push - Nextcloud notify_push. L2. Rust push server, websocket, Redis, emits notify_file; README still advises a slower periodic poll as fallback.
49. https://etke.cc/help/extras/ntfy/ and Element/ntfy docs - UnifiedPush/ntfy. L3. Android push without FCM works; iOS needs APNs, only the app publisher's certificate can send, so self-hosted iOS push needs a central relay.
50. https://recall.ai/blog/postgres-listen-notify-does-not-scale - LISTEN/NOTIFY. L2/L3, 2025-07. Global commit-time lock under many concurrent writers; disputed on pgsql-hackers (wake-up storm scales with listener count); upstream improvement reported.
51. https://www.cloudscale.ch/en/engineering-blog/2025/10/09/generating-truly-sequential-ids-in-postgresql and https://event-driven.io/en/ordering_in_postgres_outbox/ - sequences are not commit-ordered; pollers by "last id" skip late commits; xmin-horizon workaround. L3.
52. https://www.netdata.cloud/guides/cassandra/cassandra-clock-skew-data-corruption/ - LWW with skew silently loses writes; mitigations: server-side timestamps, reject future timestamps. L3.

## Fact cards

### A. Prior systems
- F1. Dropbox Nucleus replaced a path-keyed model (move = delete+add, transient loss/duplication) with stable globally unique ids and atomic moves; the authors call the data model the main factor. [1] High.
- F2. Nucleus keeps three trees (remote, local, last-synced) and persists observations; sync is "done" when they converge; bugs found by seeded randomized simulation (millions of runs/night). [1][2] High (L2 via summaries).
- F3. Dropbox splits files into 4 MB blocks, SHA-256 each; commit sends a blocklist, server answers "need blocks", client uploads only those, then commits again. [3] High. (This is the exact protocol shape, and it gives resumability, parallelism and block-level delta for free.)
- F4. Syncthing: block size 128 KiB-16 MiB, SHA-256 per block, sequence-numbered delta index (index id + max sequence), version vectors, no wall-clock dependence for ordering (sequence is a local counter). [4][5] High.
- F5. Syncthing conflict rule uses mtime (older loses), tie by device id; a deletion conflicting with a modification yields a conflict copy; watcher batches 10 s, deletes held an extra minute; periodic full scan hourly is the safety net. [5][6] High.
- F6. Syncthing prefers TCP over QUIC; users measure QUIC 2.4x slower on LAN and Windows UDP-buffer starvation. [7] Med (L4).
- F7. Nextcloud v2 chunking: 5 MB-5 GB chunks, <=10,000 chunks, final MOVE assembles; a client that auto-grew chunks to 1000 MB broke behind 512 MB proxy limits; per-file overhead dominated small-file sync (bundling "File Packing" added; users still see ~10 files/s). [8][9][10] High/Med.
- F8. Seafile: git-like commit model, content-defined chunking, ~8 MB average blocks, blocks content-addressed so unchanged blocks are shared. [11] High (manual) / Med (dedup claim from third party).
- F9. Mutagen: rsync-style delta, staging outside sync root and atomic relocation, avoids syncing while files are changing; inode-keyed digest cache for cheap rescans. [32] Med.
- F10. Google Drive for desktop 84.x (2023-11) lost months of unsynced local files; root cause never published. Lesson: never delete/overwrite local data without a confirmed durable server commit. [44] High that it happened, Low on cause.
- F11. Resilio Sync and rsync/rclone internals were not researched from primary sources [limited source]; rsync's rolling-checksum delta is well known but not re-verified.

### B. Famous correctness traps (each with a source or marked)
- F12. Rename/move: inotify gives IN_MOVED_FROM/TO with a cookie but pairing is "not guaranteed consecutive", no TO event if moved out of the watch; FSEvents and Windows need file-id tracking to stitch (debouncer-full does it only with its file-id cache). Without ids a rename degrades to delete+create = re-upload of a 50 GiB file. [33][34][35] High.
- F13. Dropped events are a normal condition: inotify IN_Q_OVERFLOW, RDCW returns 0 bytes / ERROR_NOTIFY_ENUM_DIR, FSEvents UserDropped/KernelDropped/MustScanSubDirs; every one means "re-enumerate". [35][36][37] High (37 Med).
- F14. inotify does not report mmap writes, network filesystems, or recursive subdirs; new subdir can already contain files when its watch is added -> scan after adding. [35] High.
- F15. Case-insensitive FS (Windows, default macOS) vs case-sensitive peers: Syncthing reports an unsyncable case conflict and cannot predict which variant wins. [5] High. APFS is normalization-insensitive/preserving, HFS+ stored NFD [54-training] Med [limited source]; NFC/NFD duplicates were fixed in rclone only in 2024 and bit Syncthing users. [45] Med.
- F16. Windows MAX_PATH 260 unless registry + manifest opt in; NTFS does not normalize Unicode. [39] High.
- F17. Echo loops / partial writes: Mutagen waits for stability; Dropbox/Syncthing write to temp then atomic rename; client must record (file id, size, mtime, hash) of what it just wrote so the resulting watcher event is recognized as its own. [32][5] Med (pattern from sources, no explicit "echo" doc).
- F18. LWW with client wall clocks silently loses writes under skew; mitigation is server-assigned order/timestamps and rejecting future timestamps. [52] Med (L3). Syncthing avoids this for ordering by using local counters + version vectors, using mtime only to pick a conflict loser. [4][5] High.
- F19. Postgres sequences are not commit-ordered: a poller reading "id > cursor" can permanently skip a late-committing lower id. Fixes: per-account counter assigned under row lock inside the transaction, or xmin-horizon reads. [51] Med (L3, but the mechanism is standard Postgres semantics).

### C. Transfer design
- F20. Industry chunk sizes: Dropbox 4 MB; OneDrive recommends 5-10 MiB (10 MiB optimal on stable links, max 60 MiB per request); AWS CLI 8 MB x 10 concurrent; rclone OneDrive 10 MiB; Nextcloud default 10 MB; Google "as large as possible", 256 KiB multiples. S3/Nextcloud minimum 5 MiB. [3][8][14][16][17][13] High. None is <=2 MiB except by design choice.
- F21. tus: server stores as much as received; HEAD returns the persisted offset; chunk size is client's choice, HTTP-version independent; checksum extension per request; concatenation extension for parallel parts. [12] High. => with streamed PATCH, bytes re-sent after a drop = bytes not yet persisted, not "chunk size".
- F22. Interruption cost with block/part uploads: a part in flight is discarded (OneDrive: "all bytes in that request are ignored"). With P parallel lanes of size B, worst-case re-send = P x B. [14] High; arithmetic computed.
- F23. 1 Gbps = 125 MB/s; TCP goodput on 1500-MTU Ethernet is ~94.1% (1448/1538, computed), TLS 1.3 record and HTTP/2 DATA framing add <0.2% (computed). 90% of link therefore leaves ~4% for all overhead, and a 50 GiB file takes ~8 min at 90% (computed). Medium-high.
- F24. Window limits: a single TCP flow needs window >= BDP (1 Gb/s x 50 ms = 6.25 MB; x 100 ms = 12.5 MB, computed); ESnet recommends 4-8 parallel streams and 32-64 MB socket maxima for fast long paths. [46] High. HTTP/2 initial window is 65,535 B by RFC 9113 (stable spec; hyper's own defaults are documented as unstable; use adaptive_window or explicit windows) [23] Med: at 64 KiB window and 50 ms RTT the cap would be ~10 Mbit/s (computed).
- F25. HTTP/3: up to 45.2% lower throughput than HTTP/2 on fast links, cause receiver-side per-packet processing and user-space ACKs; UDP send path up to 3.5x cycles/byte of TCP, GSO/GRO narrow it; quic-go on Syncthing slower than TCP. quinn 0.11 has no published throughput claims; `h3` crate self-declared experimental. QUIC helps on lossy/mobile/migrating paths. [18][19][20][21][22][7] High that it is not a win for LAN/broadband bulk; Low-Med on exact deltas.
- F26. Framework: no credible public axum-vs-actix bulk-upload benchmark; blog numbers contradict. For multi-MB bodies the cost is socket read + disk write, not request routing, so framework differences are noise. axum 0.8.9 default 2 MB limit applies only to buffering extractors; stream the Body to file. [24][26] Med (reasoning + L1 doc).
- F27. tokio::fs = spawn_blocking per operation (2 MB per-call cap by default); io_uring only behind tokio_unstable; write large buffers (>=1 MiB), not small ones. [25] High. 112 MB/s needs ~56 write calls/s at 2 MB: negligible; io_uring not needed for 1 Gbps.
- F28. sendfile + userspace TLS loses zero-copy; kTLS restores it (Netflix). For downloads, a reverse proxy serving the file (nginx sendfile/X-Accel-Redirect) is the practical zero-copy route, and only if files are stored plain (conflicts with app-level encryption at rest). [27] Med; relevance to 1 Gbps is low [reasoned].
- F29. Hash speeds (measured, Zen 2): SHA-256 with SHA-NI 2.07 GB/s; BLAKE3 1 thread 2.85 GB/s, multi-thread 13 GB/s; software-only SHA-256 (coreutils) 0.41 GB/s. All >= 3.3x the 125 MB/s link even without hardware help. Hash cost at 1 Gbps: <=6% of one core for SHA-256 SHA-NI (computed). [29] High for this CPU; other CPUs [limited source]. Published expectation: SHA-NI narrows the gap, BLAKE3 wins without it [crypt-io bench comment, L4].
- F30. BLAKE3 is a Merkle tree (parallel hashing, verified streaming via Bao) and the Rust crate has runtime-detected SIMD plus rayon. [28] High. Browser WebCrypto digest is not incremental [training knowledge] Med [limited source], so a 50 GiB browser upload needs a WASM hasher for either algorithm.
- F31. AES-128-GCM 4.99 GB/s on this CPU: TLS or at-rest encryption costs ~2.5% of one core at 112 MB/s (computed). [29] High.
- F32. Compression: photos/video/zip/Office are already compressed; Syncthing offers LZ4 for metadata by default and optional for data. [4] Med. Compression helps only text/DB-like files; skip by extension/entropy probe.
- F33. Content-defined chunking (FastCDC, Seafile ~8 MB avg) wins when edits insert/delete bytes mid-file; fixed blocks (Dropbox 4 MB, Syncthing) win on simplicity and on append/overwrite edits (logs, VM images, databases). Typical user data (photos, video, office zips, PDFs) is rewritten or immutable: delta gains are small. FastCDC has several incompatible implementations; cut points would become a permanent wire contract. [3][11][31] Med (the "typical data" judgement is reasoned, no dataset).
- F34. Global cross-user dedup via block hashes leaks existence ("do you have this hash?") and conflicts with per-user encryption/vault; keep per-user scope at most. [reasoned] Low-Med.

### D. Change notification
- F35. Nextcloud's own Rust push server uses a websocket that only says "file changed"; the client then runs the normal sync; README still advises a low-frequency poll fallback. [48] High. Pattern = tiny wake-up + cursor pull.
- F36. SSE: auto-reconnect and id field built in; 6 connections per origin on HTTP/1.1, 100 streams on HTTP/2; one-way; proxies (nginx) buffer unless disabled [47] High; nginx buffering header detail from search summaries Med.
- F37. Mobile push: Android can use UnifiedPush/ntfy without FCM; iOS cannot receive background push without APNs, whose certificate is held by the app publisher, so a self-hosted install needs a central relay. [49] Med (L3).
- F38. LISTEN/NOTIFY: commit-time global lock reported under heavy concurrent writes, disputed; fine at hundreds of users and tens of writes/s; single API instance can use an in-process channel. [50] Med.
- F39. AC-5 time budget (computed): settle 2.0 s (AC-18) + watcher latency (inotify/RDCW ms; FSEvents latency parameter, typically 0.1-1 s [37]) + hash 10 MB ~5 ms + upload 10 MB ~85 ms + commit/notify tens of ms + download ~85 ms + temp-write/rename ~ms = ~2.4-3.6 s. A 30 s poll cannot meet p95 5 s; any push or held long-poll can. Med-High.

### E. Desktop watcher and OS integration
- F40. inotify watches are per directory; 100,000 files in ~10,000 dirs = ~10,000 watches. Default limit 8,192 on pre-5.11 kernels (Syncthing FAQ), ~1% of RAM within [8192, 1,048,576] since 5.11; this machine: 501,194. Per-watch kernel memory not verified (estimates 100 B-1 KB) [38] Med. Must handle ENOSPC by degrading to periodic scan; document sysctl. [6][33]
- F41. Windows RDCW: one handle per root with bWatchSubtree; buffer fixed at open; overflow = discard + full re-enumeration; cheap per file. [36] High. macOS FSEvents: one recursive stream per root, coalesces by latency, can replay from a persisted event id after restart (per-volume), flags overflow as must-scan. [37] Med [limited source].
- F42. Linux has no unprivileged persistent journal: after daemon restart you must rescan against the local DB (mtime+size+inode). macOS can use event-id replay; NTFS has the USN change journal [36 mentions change journals]. Rescan-on-start is the single portable path. [reasoned] Med.
- F43. notify 8.2.1 (MSRV 1.88) documents loss of events on very large dirs and recommends PollWatcher where limits bite; debouncer-full stitches renames only with its file-id cache. [33][34] High. Memory of notify at 100k files not found in any source [limited source].
- F44. Measured (tmpfs, warm): full walk + stat of 100,000 files 0.30 s with Python scandir; hashing all 3.3 GB 0.85 s with 8 BLAKE3 processes. [30] High for the measurement, Low for transfer to cold HDD/NTFS/Defender (no benchmark found: [limited source]). Reasonable cold-cache estimate 2-20 s [reasoned].
- F45. Windows: a service lives in session 0 and cannot show a tray icon; the supported shape is a per-user logon-started process (Run key / Startup / logon task) or service + UI split. [42] Med (L3). Restrictions say "service or logon task": choose logon task / Run key.
- F46. macOS: TCC grant attaches to the signed app bundle / responsible process, not obviously to a bare launchd binary; ship a signed, notarized .app (menu bar agent) registered via SMAppService/LaunchAgent so the user grants Full Disk Access to one stable identity. Developer ID ($99/yr) + hardened runtime + notarytool; bare CLI binaries cannot be stapled. [40][43] Low-Med [limited source].
- F47. Windows signing 2026: EV certs no longer give instant SmartScreen reputation; OV ~ $219/yr or Azure Artifact Signing ~ $10/month (eligibility restricted per one blog). Unsigned = SmartScreen block. [41] Med.
- F48. Linux: systemd user unit (`systemctl --user enable`), tray via StatusNotifierItem; GNOME needs an extension for tray [training knowledge] Low [limited source].
- F49. Idle CPU <=1% is a duty-cycle question: event-driven watchers wake only on changes; the risks are polling fallbacks, 2 s settle timers per busy file, and periodic safety rescans (Syncthing hourly). [5][reasoned] Med.

## Options comparison tables

### T1. Transfer protocol shape
| Rank | Option | Resume cost | Parallel | Delta | Complexity | Notes |
|---|---|---|---|---|---|---|
| 1 | Blocklist + "need blocks" (Dropbox-style), fixed blocks, block PUT, final commit | P x B (computed) | native (lanes) | changed blocks only | medium | server writes block to preallocated temp file at offset, hash-verifies, atomic rename on commit; DB keeps blocklist |
| 2 | tus 1.0 streaming PATCH (+ concatenation for parallel) | ~unpersisted bytes (<1 MiB) | via concatenation ext. | none | low (standard, client libs) | single stream limited by one TCP window on WAN; no per-block verify unless checksum ext. |
| 3 | S3-style multipart (part numbers, ETag list) | P x part (5 MiB min) | native | none | medium | minimum part 5 MiB makes AC-3 tight with lanes >1 |
| 4 | Nextcloud-style WebDAV chunk folder + MOVE | chunk | possible | none | medium | MKCOL/MOVE round trips; adds nothing over #1 |
| 5 | CDC (FastCDC) + rsync-like delta | block | yes | insert-resilient | high | permanent cut-point contract; poor fit for already-compressed media |

### T2. Block/part size (hash unit = upload unit)
| Size | Blocks per 50 GiB | Worst-case re-send, 4 lanes | Worst-case, 2 lanes | Per-block time at 1 Gbps | Verdict |
|---|---|---|---|---|---|
| 1 MiB | 51,200 | 4 MiB | 2 MiB | 8 ms | too many rows/requests |
| 2 MiB | 25,600 | 8 MiB | 4 MiB | 17 ms | recommended (fits AC-3 with margin) |
| 4 MiB | 12,800 | 16 MiB (violates) | 8 MiB | 34 ms | Dropbox precedent; ok with <=2 lanes |
| 8 MiB | 6,400 | 32 MiB | 16 MiB | 67 ms | violates AC-3 with any parallelism; matches AWS CLI default |
| 16 MiB | 3,200 | 64 MiB | 32 MiB | 134 ms | only with sub-block resume |

### T3. Transport for bulk data
| Rank | Option | Evidence | Verdict |
|---|---|---|---|
| 1 | TLS 1.3 over TCP, HTTP/2 (or HTTP/1.1 per-lane connections) | kernel TCP offloads, no UDP per-packet CPU; [18][19][20] | default. Set h2 window/adaptive explicitly (F24) |
| 2 | HTTP/1.1 only, N connections | no h2 flow-control or TCP-HOL-across-streams; browsers cap 6 per host | fine fallback, simplest |
| 3 | HTTP/3 (quinn+h3) | [18] up to -45% on fast links; h3 crate experimental [22]; UDP often blocked | not for v1; revisit for lossy mobile if measured |
| 4 | gRPC streaming | needs h2 end-to-end, grpc-web for browser; proxy friction | reject |
| 5 | Custom TCP/QUIC protocol (BEP-like) | best control, worst tooling, no browser | reject (web client exists) |

### T4. Hash
| Option | Speed (measured) | Portability | Notes |
|---|---|---|---|
| BLAKE3 (rank 1) | 2.85 GB/s 1 thread, 13 GB/s multi | Rust everywhere; Flutter via FFI; web via WASM | tree mode/Bao for verified ranged download; 6x faster than non-accelerated SHA-256 |
| SHA-256 (rank 2) | 2.07 GB/s SHA-NI, 0.41 GB/s without | native in every OS/phone SDK; FIPS/auditable with sha256sum | within 1.4x of BLAKE3 on SHA-NI hardware; use if operators/auditors demand it |
| xxHash/CRC (reject for AC-7) | fastest | - | not cryptographic; may be used only as a transport checksum, not as the verification hash |
Wire format should carry an algorithm id so the choice is reversible.

### T5. Change notification
| Rank | Option | AC-5 | Complexity | Failure modes |
|---|---|---|---|---|
| 1 | Cursor changelog + held long-poll (`GET /api/changes?cursor=N&wait=30`) | ~RTT after commit | lowest: one endpoint = catch-up and wake-up | proxy timeout must exceed wait; 1 extra request per batch |
| 2 | Cursor changelog + SSE wake-up (`id:` = cursor) | same | +1 endpoint | proxy buffering, EventSource cannot set auth header, 6-conn limit on h1.1 |
| 3 | WebSocket | same | + ping/pong, state | bidirectionality unused |
| 4 | gRPC stream | same | high | see T3 |
| 5 | Periodic poll 30 s (+ push-less) | fails p95 5 s | trivial | - |
Mobile push (FCM/APNs): not needed for AC-5 (app online = foreground connection). It is needed only for background wake-up; iOS needs a central relay [49], Android can use UnifiedPush; AC-9 iOS (15 min) is best-effort via OS background tasks (not researched [limited source]).

### T6. Server framework
| Rank | Option | Basis |
|---|---|---|
| 1 | axum 0.8 on hyper 1.x + tokio | ecosystem (tower, tower-http, utoipa for OpenAPI), body streaming is hyper's; bulk path is IO-bound |
| 2 | actix-web 4 | blog benchmarks claim +10-15% on tiny requests; irrelevant for 2 MiB bodies [26] |
| 3 | raw hyper | no gain; re-implements routing |

## AC realism table
| Criterion | Our value | Researched value | Note |
|---|---|---|---|
| AC-2 >=90% of up to 1 Gbps | 90% (=112.5 MB/s) | TCP goodput ceiling ~94.1% on 1500 MTU; overheads of TLS/h2 <0.2%; AES-GCM and hash each ~2-6% of one core | Achievable on LAN/low RTT with 1 stream; WAN (50-100 ms) needs window >= 6-12 MB or 3-4 lanes. Define test: server NVMe/SSD, client NVMe, RTT <= 20 ms, large file >= 1 GiB, measure after warm-up. HDD servers may cap below 112 MB/s. Cost: none extra; do NOT enable HTTP/3. Nginx-style proxy request buffering to disk would halve it (turn off) [limited source] |
| AC-3 resume re-sends <=10 MiB | 10 MiB | P x B with blocks; ~0 with streaming PATCH | 2 MiB x <=4 lanes = 8 MiB worst case. Server must fsync/commit block before ack. Need session TTL (Nextcloud 24 h, Google 1 week) |
| AC-4 50 GiB | 50 GiB | S3 allows 10,000 parts; Nextcloud 10,000 chunks; at 2 MiB that is 25,600 blocks | Fine for own protocol (no 10,000 cap) but blocklist 800 KiB per 50 GiB file, send/commit in pages; reverse proxy body-size limits must be raised/disabled; ~8 min at 90% |
| AC-5 <=5 s p95 (<=10 MB) | 5 s | budget 2.4-3.6 s dominated by 2 s settle (F39) | Holds with long-poll/SSE/WS; fails with poll. Margin ~1.5 s; do not add extra debounce on top of AC-18 settle; hash while uploading |
| AC-7 hash-verified E2E | 0 corrupt | per-block hash on receive + file blocklist hash verified after commit and on download | Cost small (<6% of a core at 1 Gbps). Verify download against blocklist client-side |
| AC-8 later write wins, no mix | later write wins | server-assigned commit order (per-file row lock, per-account seq); client mtime only metadata | Client clocks not trusted (F18); atomic temp-file + rename guarantees no mix; stale offline edit overwrite is accepted per restrictions. Optional `base_rev` field to warn the user is NOT required (avoid scope growth) |
| AC-15 100k files: <=1% CPU idle, <=100 MB, first scan <=60 s | as stated | walk+stat 0.3 s warm tmpfs (measured); 100k files ~10k watches; hash 3.3 GB 0.85 s with 8 procs | Realistic if: state in SQLite not in RAM; scan = walk+stat, hashing lazily/in background (first scan must be defined as enumerate+stat, otherwise hashing 100k files of unknown size is disk-bound and can exceed 60 s); GUI toolkit not resident in daemon process (a resident webview alone can exceed 100 MB, cross-area). Windows/NTFS + Defender cold numbers unknown [limited source] - needs a spike |
| AC-18 settle 2 s, no echo | 2 s | Syncthing batches 10 s; Mutagen waits for stability | 2 s is aggressive but meets AC-5; use size+mtime unchanged for 2 s AND file not open exclusively on Windows. Echo: remember (file id, size, mtime, hash) of every file the daemon wrote, drop matching events; write to temp file in same dir/volume then atomic rename |
| AC-21 revoke <=5 s | 5 s | held connections can be woken | Revocation publishes a wake-up to the device's waiter; needs the channel from T5 (long-poll/SSE/WS) - polling alone would not meet it |

## Recommendation

Protocol shape (rank 1): Dropbox-style blocklist protocol over plain HTTPS, with a per-account change cursor.
1. Data model: stable server-assigned `file_id` for every node (files and folders); operations are create/update/move/delete by id with `parent_id` + `name`, so rename = one atomic `move` (F1). Client keeps a local SQLite table mapping OS file identity (inode+dev / NTFS file index+volume serial / APFS inode) to `file_id` plus last-synced (size, mtime, blocklist hash) = Nucleus-style "synced tree" (F2).
2. Upload: client computes fixed 2 MiB block hashes, `POST /api/files/{id}/commit` with blocklist; server replies `need` list; client PUTs missing blocks over <=4 lanes (lanes x block <= 8 MiB), each PUT hash-verified and written at its offset into a preallocated temp file; second commit atomically publishes (rename + DB row) and bumps the account sequence. Unchanged blocks of a modified file are never re-sent (free delta). No CDC, no rolling checksum, no cross-user dedup in v1.
3. Download: ranged GET by block with hash verification, temp file + atomic rename.
4. Hash: BLAKE3 (measured 2.85 GB/s 1 thread, 13 GB/s multi-thread) with algorithm id on the wire; SHA-256 is an acceptable fallback (2.07 GB/s with SHA-NI) if auditors or platform SDKs demand it.
5. Transport: TLS 1.3 over TCP; HTTP/2 (explicit window/adaptive) or HTTP/1.1 with per-lane connections; no HTTP/3, no kernel-bypass or io_uring work in v1. axum 0.8/hyper 1.x/tokio, stream body to file with large writes, disable the default 2 MB body limit only on the block route and replace it with an explicit size cap.
6. Ordering and LWW: order = server commit sequence per account (assigned under row lock inside the commit transaction); never use client mtime for ordering; reject mtimes far in the future.
7. Notification: `GET /api/changes?cursor=N&wait=30` long-poll over the same cursor (rank 1); SSE is the drop-in upgrade if long-poll request churn shows up in profiles. In-process wake-up channel on a single API instance; Postgres LISTEN/NOTIFY if replicas are added. No FCM/APNs in v1; Android background wake via UnifiedPush/WorkManager later.
8. Desktop watcher: `notify` 8.x native backends with own thin rename/echo layer (file-id table), 2 s settle timer per path, any overflow/rescan event or startup triggers a targeted re-enumeration against SQLite, plus a low-frequency safety rescan (e.g. 1 h like Syncthing). Linux: handle ENOSPC on inotify by falling back to periodic scan and show the sysctl hint. Autostart: systemd user unit (Linux), signed+notarized .app with LaunchAgent/SMAppService (macOS), per-user logon task/Run entry (Windows, not a session-0 service).
9. Name policy: store names as received, detect NFC/case-fold collisions server-side per folder and refuse/rename with a visible error rather than silently overwriting; enable long-path manifest + `\\?\` on Windows.

What would change it:
- AC-3 relaxed to >=16 MiB -> 4 MiB blocks (fewer requests; Dropbox precedent). AC-2 raised to 10 GbE -> 8-16 MiB blocks plus sub-block resume (tus-style offset PATCH) and kTLS/proxy tuning.
- Benchmarks (spike) show h2 single-connection collapses under loss -> drop to h1.1 multi-connection; if many users are on lossy mobile/cellular -> evaluate HTTP/3 behind a reverse proxy that already supports it (client-only), not quinn in the API.
- Real user data dominated by append/insert-edited large files (VM images, DB dumps, large PST) -> add FastCDC for those via a second chunker only after measuring; fixed blocks stay for the rest.
- Server uses app-level encryption at rest -> sendfile/X-Accel zero-copy is off the table (no loss at 1 Gbps).
- Operators run many API replicas or >~100 writes/s -> move wake-ups from in-process to NOTIFY or a small queue.
- If the GUI toolkit decision forces a resident webview, split watcher daemon and tray/settings into two processes to hold the 100 MB budget.

## Open questions
1. Will the Compose stack ship a reverse proxy (Caddy/nginx/Traefik)? Its request buffering, body-size and idle-timeout defaults decide AC-2/AC-4 and the long-poll wait; a spike must test the real chain. [limited source]
2. Server storage: plain tree on ext4 vs block store changes how "commit" materializes the file (rename of the temp file is assumed). Encryption at rest design (cross-area) decides sendfile viability.
3. Vault (E2E, <=100 MiB): are blocks encrypted deterministically per block index so delta works, or whole-file re-upload? Hash of ciphertext is what the server can verify (cross-area).
4. Cold-cache first-scan numbers on HDD, NTFS with Defender, and APFS for 100k files; notify memory/CPU with ~10k directories - needs a spike on each OS (no published figure found).
5. Exact semantics of FSEvents/TCC for an unsigned vs signed LaunchAgent binary on current macOS; not confirmed from primary Apple docs (fetch failed).
6. Does AC-5 apply to desktop-to-desktop only, or also web/mobile origin? Web/mobile origin skips the 2 s settle, so budget is easy; desktop origin is the binding case.
7. Does iOS background upload (AC-9, 15 min) need APNs silent push? Not researched; depends on PhotoKit change observers and BGTaskScheduler [limited source].
8. Per-file base revision on commit (to flag stale offline overwrites) is deliberately omitted; owner decision whether a "your edit overwrote a newer version" notice is wanted despite no-versions policy.
