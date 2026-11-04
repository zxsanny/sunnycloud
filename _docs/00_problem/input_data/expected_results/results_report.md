# Expected Results

Maps test inputs from `data_parameters.md` to quantifiable expected results. Rows are grouped by area; "AC" gives the acceptance criterion. Comparison methods follow `templates/expected-results.md`. Rows marked [v1.1] or [v1.2] belong to those releases.

## Transfer, integrity and durability

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 3, 4, 7 | big-file 5 GiB | Upload, drop the connection at 50%, retry | Final BLAKE3 equals original; bytes re-sent after resume | exact (hash); threshold_max | ≤ 10 MiB re-sent |
| 2 | 2 | big-file 5 GiB | Upload on a 1 Gbps link, RTT ≤ 20 ms, encryption at rest on | Throughput | threshold_min | ≥ 90% of link speed (≥ 112 MB/s) |
| 3 | 2, 7 | big-file 5 GiB | Download and compare | Hash equals original; throughput as row 2 | exact; threshold_min | ≥ 112 MB/s |
| 4 | 4 | big-file 50 GiB | Upload and download | Hashes equal; finishing the upload after the last block | exact; threshold_max | finish ≤ 2 s |
| 5 | 38 | many-small | `kill -9` the server during 100 concurrent uploads, restart, list | Every committed file has full correct content | exact (0 files with missing or zero-filled blocks) | N/A |
| 6 | 39 | 100 sessions | Each declares 50 GiB from one account | Sessions beyond cap and quota refused; free space | exact (refusals); threshold_min | free space ≥ configured floor throughout |
| 7 | 40 | many-small | Upload 10,000 files ≤ 1 MiB at 20 ms RTT | Total time | threshold_max | ≤ 60 s |
| 8 | 1 | 500 accounts | 100 concurrent syncing sessions, aggregate 1 Gbps, 10 minutes | Error rate; sessions served | threshold_max; exact | 0 failed sessions; 5xx rate ≤ 0.1% |

## Sync, ordering and conflicts

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 5 | one 5 MB file | Edit on A while B online; 100 background uploads running | Content on B equals A | exact (hash); threshold_max | ≤ 5 s at p95 |
| 2 | 6 | listing-10k | List the folder | Server time | threshold_max | ≤ 300 ms at p95 |
| 3 | 8 | one 1 MiB file | A edits (base rev N) and B edits (base rev N) offline; A commits first, B later | Current version is B's; A's content kept as "name (conflict copy)"; file intact | exact (hash); exact (copy count 1) | N/A |
| 4 | 8 | one file | A edits, B deletes, B commits later | File remains with A's content | exact | N/A |
| 5 | 41 | populated install | Restore a database backup older than the devices' cursors | Each device receives `resync_required` and converges; 0 missed changes | exact | converge ≤ 60 s |
| 6 | 41 | device | Offline 91 days | Safe full resync, 0 data loss | exact | N/A |
| 7 | 42 | tricky-names | Create `A.txt` and `a.txt` in one folder | Second create rejected or renamed; first untouched | exact | N/A |

## Deletion safety

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 29 | sync root | Unmount or delete the sync root | Delete hold with user prompt | exact (0 deletions propagated) | N/A |
| 2 | 29 | 300 files | Delete 200 files at once | Hold with confirmation; after "no" all 200 return | exact | N/A |
| 3 | 30 | one file | Overwrite, then delete; query blob at day 6 and day 8 | Blob present at day 6, removed after day 7 | exact | retention 7 d (config 0-90) |
| 4 | 30 | one file | Operator restore by path and time | File returned with original hash | exact | N/A |
| 5 | 27 | account | Delete account | Files, thumbnails, transcodes, shares, sessions removed at once | exact (0 rows, 0 blobs) | N/A |

## Desktop client

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 15 | tree-100k | Watch, idle | CPU and memory; first scan | threshold_max | ≤ 1% CPU, ≤ 100 MB, scan ≤ 60 s |
| 2 | 18 | one 1 MiB file | Daemon downloads a remote file | Not uploaded again | exact (upload count 0) | N/A |
| 3 | 18 | one 2 GiB file | Copy into synced folder, still being written | Upload starts only after unchanged | threshold_min | ≥ 2 s since last write |
| 4 | 16 | settings | Change a setting | Daemon applies it | threshold_max | ≤ 5 s, no restart |
| 5 | 16 | settings | Cold start of the window | Time to interactive | threshold_max | ≤ 1.0 s p95 (2019 Windows laptop HDD; Ubuntu LTS Wayland+NVIDIA) |
| 6 | 17 | settings | Pick a folder | OS-native dialog opens | exact | N/A |
| 7 | 26 | daemon | Run on GNOME without tray extension | Sync works | exact | N/A |
| 8 | 31 | collide-1000 | Connect folder | 0 files lost; 0 identical bytes uploaded; conflict copies | exact | exactly 100 copies |
| 9 | 32 | evicted-100 | Sync tree | Hydrated bytes; skipped; deletions | exact | 0 B; 100 reported; 0 deletions |
| 10 | 33 | locked file | Exclusive lock for 60 s then release | CPU while locked; upload after release; never reported deleted | threshold_max; exact | ≤ 1% CPU; ≤ 10 s |
| 11 | 34 | editor-saves | Save in place from each editor | New revisions on same file ID; copies; deletes; share link | exact | 1 revision, 0, 0, link opens |
| 12 | 35 | 500 files | 30-minute suspend with changes on both sides | All devices converge | threshold_max | ≤ 60 s after wake |
| 13 | 37 | tricky-names | Server sends 40 hostile names per OS | Writes outside root; overwritten files | exact | 0 and 0 |
| 14 | 48 | daemon IPC | Different OS user or remote client connects | Connection refused; no token-returning method in schema | exact | N/A |
| 15 | 49 | updater | Offer unsigned, wrong-key and lower-version builds | All rejected | exact | N/A |

## Photos and videos

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 10 | photos | Upload one 12 MP JPEG | Thumbnail ready | threshold_max | ≤ 5 s after upload, p95 |
| 2 | 10 | photos-bulk | Bulk import with interactive uploads in parallel | Bulk finish; interactive p95 | threshold_max | ≤ 15 min; ≤ 5 s |
| 3 | 11 | clip-1080-sdr | Transcode on 4 physical cores | Playable MP4, H.264 High 1080p AAC | threshold_max | ≤ 5 min |
| 4 | 11 | clip-4k-hlg | Transcode to 1080p SDR with tone mapping | Playable SDR output with correct colour tags | threshold_max; exact (tags) | ≤ 20 min |
| 5 | 11, 44 | clip-compliant | Upload | Not re-encoded | exact (derived transcode count 0) | N/A |
| 6 | 44 | clip-mixed-100 | Run pipeline | Derived storage / original bytes | threshold_max | ≤ 20% |
| 7 | 45 | clip-mixed-100 | Hang ffmpeg on one job | Killed and retried on CPU; rotated/VFR clips correct | threshold_max; exact | ≤ 90 s after last progress; drift ≤ 40 ms |
| 8 | 9 | photos | New photo on Android Pixel on Wi-Fi, battery exempt | Photo present on server | threshold_max | ≤ 60 s |
| 9 | 46 | phone-fixtures | Reinstall app with 5,000 backed-up assets | Media bytes uploaded; edited vs original linked | exact | 0 bytes; linked, not duplicate |
| 10 | 52 | phone-fixtures | First sync of 50,000 assets, 10 forced kills | Re-hashed assets; duplicates; RSS | threshold_max | ≤ 1%; 0; ≤ 150 MB |
| 11 | 53 | phone-fixtures | Limited photo access | "N of M visible" shown | exact | N/A |
| 12 | 28 [v1.1] | NVENC worker | Enable GPU; remove GPU | GPU used then CPU fallback; valid output | exact | N/A |

## Sharing and content serving

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 12 | one file | Create a link, open it anonymously | HTTP 200 and bytes equal original | exact | opens ≤ 2 s |
| 2 | 12 | one file | Revoke, open again | HTTP 404 | exact | within 5 s of revoke |
| 3 | 12 | one file | Open after expiry; open a missing token | Identical response to revoked | exact (status, body, headers); threshold_max (timing delta) | median delta ≤ 1 ms |
| 4 | 12 | share tokens | Inspect database | Only hashes stored | exact | N/A |
| 5 | 47 | uploads-hostile | Download and share each type | `attachment` + `nosniff` + sandbox CSP; allowlisted image/video inline with `nosniff` | exact | 0 inline renders of other types |

## Encryption at rest

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 43 | big-file 5 GiB | Read blob file from disk | Ciphertext only; no plaintext substring of a known marker | exact | N/A |
| 2 | 43 | one blob | Rotate master key | Blobs unchanged on disk; still readable | exact | N/A |
| 3 | 43 | one 1 GiB blob | Range read of 4 KiB at random offsets | Correct bytes | exact | N/A |

## Authentication, security and operation

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 23 | empty install | Social login without setup token | No account created; token works once | exact | N/A |
| 2 | 55 | oauth-mock | Provider returns a victim's unverified email | 0 accounts created or linked | exact | N/A |
| 3 | 56 | oauth-mock | Each of 12 invalid callbacks | All rejected | exact | 12 of 12 |
| 4 | 57 | device grant | Approve from a different IP | Page shows both locations with warning; code expires | exact; threshold_max | TTL ≤ 10 min |
| 5 | 57 | device token | Call approve-device, create-share, delete-account, link-provider | HTTP 403 each | exact | N/A |
| 6 | 20 | invalid inputs | Bad state, bad token, wrong share password | HTTP 429 with Retry-After beyond limits | exact | per code/account/link/IP |
| 7 | 21 | device session | Revoke it | Sync stream cut | threshold_max | ≤ 5 s |
| 8 | 58 | every state-changing operation | Cross-origin POST with valid cookie | HTTP 403 | exact | N/A |
| 9 | 58 | refresh token | 4 concurrent refreshes; replay after 30 s | One successor, no revocation; family revoked | exact; threshold_max | ≤ 5 s |
| 10 | 59 | OpenAPI | Two-account matrix over every operation and ID parameter | Cross-account calls | exact | 0 successes, all 404 |
| 11 | 59 | 1,000 stalled H2 connections | Hold open | Server RSS increase; effect on 1 GiB upload | threshold_max | ≤ 200 MB; ≤ 10% delay |
| 12 | 25 | redteam-media | Process every sample | Job fails; no DB credentials, blob mount or network in decoder | exact | 0 exposures |
| 13 | 24 | one blob | Flip one bit on disk | Scrub reports the mismatch; throttled | exact | detected in next scrub |
| 14 | 63 | CI | Add an unreviewed advisory; an ignore older than 90 days | Build fails | exact | N/A |
| 15 | 63 | release images | Scan worker image | HIGH/CRITICAL CVEs in ffmpeg, libheif, libvips, libwebp | exact | 0; rebuilt ≤ 7 days |
| 16 | 54 | mobile app | Sign in over plain HTTP | Refused by default | exact | N/A |
| 17 | 64 | old client | Call with unsupported version | Clear upgrade message | substring | N/A |
| 18 | 65 | backup-set | Restore on a clean host; upgrade to next release | All data present | exact (hash set) | N/A |
| 19 | 22 | clean host | Follow README, one provider ready, default Caddy | Working server | threshold_max | ≤ 30 min |
| 20 | 19 | oauth-mock | Sign in with GitHub and generic OIDC; Apple [v1.1] | Session created per provider | exact | N/A |

## Web client

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 50 | big-file 50 GiB | Upload from Chrome, Firefox, Safari; close tab; re-select file | Heap; re-sent bytes; file edited mid-upload restarts | threshold_max; exact | ≤ 150 MB; ≤ 10 MiB |
| 2 | 51 | big-file 50 GiB | Download in Chrome, Firefox, Safari | Browser heap | threshold_max | ≤ 200 MB |

## Vault [v1.2]

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 13 | vault-file 100 MiB | Upload to vault, read stored bytes | Stored bytes differ from plaintext | exact (not equal) | N/A |
| 2 | 13 | vault-file | Download and decrypt with right passphrase | Hash equals original | exact | N/A |
| 3 | 13 | vault-file | Wrong passphrase; tampered, truncated, reordered, wrong-vault chunks | Rejected with clear error, no output file | exact | N/A |
| 4 | 14 | 101 MiB file | Upload to vault | Rejected with clear message | exact | N/A |
| 5 | 60 | vault-file | 1,000 randomised swaps of wrapped key between files, vaults, parents | All rejected; empty and exact-chunk files round-trip; unknown format version rejected | exact | 1000 of 1000 |
| 6 | 61 | vault-fuzz | 10,000 mutated headers; out-of-bounds KDF parameters | None accepted | exact | 0 accepted |
| 7 | 62 | web vault page | Inspect CSP, third-party loads, heap after lock | No unsafe-inline/eval; 0 third-party hosts; no vault key in heap | exact | N/A |

## Thresholds added after owner confirmation of gap defaults

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 43 | one 1 GiB blob | 4 KiB range read at random offsets | Latency | threshold_max | ≤ 5 ms p95 |
| 2 | 28 [v1.1] | clip-1080-sdr | NVENC vs CPU | Speedup | threshold_min | ≥ 1.3x |
| 3 | 15 | tree-100k | Initial hashing while user is active | CPU of one core | threshold_max | ≤ 25% |
| 4 | 24 | scrub running | Concurrent user transfers | Throughput vs baseline | threshold_min | ≥ 90% |
| 5 | 25 | redteam-media | Decoder limits | CPU, memory, wall time | threshold_max | photos 2 CPU/2 GiB/120 s; video 4 CPU/4 GiB/40 min |
| 6 | 14 [v1.2] | vault | Key derivation | Unlock time | threshold_max | ≤ 1 s native; ≤ 2 s browser |
| 7 | 11 | clip-compliant | Spec: H.264 High or lower, ≤ 1920x1080, ≤ 10 Mbit/s, AAC, MP4 index first, SDR BT.709 | Not re-encoded | exact | derived transcode count 0 |
| 8 | 47 | uploads-hostile | jpeg, png, gif, webp, avif, mp4, webm | Served inline with nosniff; all other types as attachment | exact | N/A |
| 9 | 27 | account | Delete | Everything removed | threshold_max | ≤ 15 min |
| 10 | 20 | invalid inputs | 5/20/50 device approvals per session/account/IP; 10 share passwords per link per 10 min; 100 invalid tokens per min per IP; 10 sign-in failures per min per IP | HTTP 429 with Retry-After at the limit | exact | N/A |
| 11 | 37 | tricky-names | Server names `/`, NUL, `.`, `..`, control and bidi characters | Rejected by server; other names accepted; client escapes unrepresentable names and reports | exact | 0 writes outside root |
| 12 | 41 | restored database | Local-only and server-only files after resync | Uploaded / downloaded; deletions only via guard | exact | N/A |

## Thresholds added in the second gap round

| # | AC | Input | Input Description | Expected Result | Comparison | Tolerance |
|---|----|-------|-------------------|-----------------|------------|-----------|
| 1 | 10 | photos (HEIC, EXIF orientations 5-8) | Upload and thumbnail | Dimensions swapped correctly; match reference render | exact (dimensions); threshold_min | SSIM ≥ 0.95 |
| 2 | 9 | Samsung and Xiaomi phones | New photo, battery exempt, Wi-Fi | Photo on server | threshold_max | p95 ≤ 5 min; published per device |
| 3 | 66 | pair with patterns | gitignore-style include/exclude | Excluded path never uploaded, downloaded or deleted | exact | N/A |
| 4 | 21 | running install | Cut internet access | Existing sessions and device tokens keep working; new sign-in fails clearly | exact | N/A |
| 5 | 30 | test image / release image | `POST /test/clock` | Works in test image; HTTP 404 in release | exact | N/A |
| 6 | 14 [v1.2] | vault passphrase | 11 characters; 12 characters with zxcvbn score 2; 12 with score 3 | First two rejected, last accepted | exact | N/A |
| 7 | 58 | cookie request | State-changing request with neither Origin nor Sec-Fetch-Site | HTTP 403 | exact | N/A |
| 8 | 7 | Firefox/Safari | Download 1 GiB and 3 GiB | 1 GiB hash-verified; 3 GiB shows "not verified in the browser" | exact | N/A |
| 9 | 67 [v1.1] | macOS | Deny, then grant folder access | Message within 10 s; other folders sync; resume within 10 s | threshold_max | ≤ 10 s |
