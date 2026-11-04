# Research: server file storage layout and metadata database (sunnycloud)

Researched 2026-11-02. Scope: blob layout, filesystem, metadata DB, realism of AC-1/2/3/4/5/6/7. Claims I could not verify are marked [limited source]. "Analysis" = my own arithmetic or reasoning, not a sourced fact.

## Sources

Tiers: L1 official/spec/primary docs, L2 maintainer/engineering blog/benchmark, L3 blog, L4 forum.

| # | URL | Title | Tier | Date | Summary |
|---|-----|-------|------|------|---------|
| 1 | https://manual.seafile.com/latest/develop/data_model/ | Seafile data model | L1 | 2026 (live) | Git-like commits/fs/blocks; variable-length content-defined chunks, avg ~8 MB; stored per repo dir or in object-store buckets |
| 2 | https://manual.seafile.com/latest/setup/overview/ | Seafile Docker overview | L1 | 2026 (live) | Components: MySQL, Redis, Caddy (+optional extras) |
| 3 | https://dropbox.tech/infrastructure/inside-the-magic-pocket | Inside Magic Pocket | L2 | n.d. | Blocks <=4 MB, keyed by SHA-256, compressed+encrypted; 1 GB buckets; Block Index = sharded MySQL; blocks immutable, changes go to separate FileJournal |
| 4 | https://dropbox.tech/infrastructure/panda-metadata-stack-petabyte-scale-transactional-key-value-store | Panda metadata stack | L2 | n.d. | Dropbox file metadata ("Filesystem") and Edgestore run on sharded MySQL; tens of millions QPS; MySQL has no cross-shard txns/rebalancing so Panda was added |
| 5 | https://dropbox.tech/infrastructure/rewriting-the-heart-of-our-sync-engine | Rewriting the sync engine (Nucleus) | L2 | n.d. | Rust engine; stable globally unique file IDs; atomic moves regardless of subtree size; old design (move=delete+add) lost files |
| 6 | https://doc.owncloud.com/ocis/9.0/admin/deployment/storage/general-considerations.html | oCIS storage general considerations | L1 | 2026 | Local for small, NFS medium, S3 blobs + NFS metadata for large; keep data on a separate partition; posixfs experimental |
| 7 | https://doc.owncloud.com/ocis/9.0/admin/deployment/storage/posixfs.html | oCIS posixfs driver | L1 | 2026 | Files in real tree, metadata in xattrs, needs nats-js-kv ID cache and inotify; experimental, "not for production"; NFSv3/CephFS/SMB unsupported |
| 8 | https://github.com/owncloud/QA/blob/master/tools/hetzner-deploy/bin/dump_decomposed.sh | dump_decomposed.sh | L3 | n.d. | oCIS 2 kept metadata in xattrs, oCIS 3 in MessagePack files (via search excerpt) |
| 9 | https://indico.cern.ch/event/970232/contributions/4157935/ | CERN talk on decomposedfs | L3 | 2021-01 | decomposedfs = FS store with lookup by path and by file ID; model for separating metadata from blobs |
| 10 | https://docs.nextcloud.com/server/latest/admin_manual/installation/system_requirements.html | Nextcloud system requirements | L1 | 2026 | PostgreSQL 18 and MariaDB 11.8 recommended; SQLite only for testing/minimal; no per-user scaling numbers |
| 11 | https://help.nextcloud.com/t/suspiciously-bloated-mariadb-database/154926/16 | Bloated oc_filecache thread | L4 | 2022-23 | oc_filecache ~2.4 GB on NC 25; orphan rows from removed storages |
| 12 | https://autoize.com/?p=6929 | Object storage with ownCloud/Nextcloud | L3 | n.d. | With S3 primary storage the DB is the only map to objects; vendor claims S3 slower than block volume |
| 13 | https://help.nextcloud.com/t/large-uploads-via-public-share-link-fail-at-the-end-behind-cloudflare-assembly-move-aborted-then-web-uploader-deletes-the-chunks/250855 | Large upload fails at final MOVE | L4 | 2026 | ~43 GB upload: chunk PUTs OK, final chunk-assembly MOVE exceeds ~100 s proxy timeout; client then deletes chunks |
| 14 | https://docs.immich.app/administration/backup-and-restore | Immich backup and restore | L1 | 2026 | Per-user dirs upload/library/thumbs/encoded-video; paths+metadata in DB, library not scanned; back up DB first, then files |
| 15 | https://docs.immich.app/install/requirements | Immich requirements | L1 | 2026 | Postgres on local SSD, never on a network share; 6-8 GB RAM; thumbnails+transcodes add 10-20% |
| 16 | https://docs.immich.app/administration/storage-template | Immich storage template | L1 | 2026 | Path templates for originals; a migration job moves existing files; "do not touch files" |
| 17 | https://jacar.es/?p=8029 (also https://docs.v2.0.0.archive.immich.app/install/upgrading) | Immich v3 "No vector extension found" | L3/L4 | 2026 | Immich moved pgvecto.rs -> VectorChord; upgrades broke on old images |
| 18 | https://github.com/syncthing/syncthing/releases/tag/v2.0.0 | Syncthing 2.0 notes | L1 | 2025 | LevelDB -> SQLite (one DB per folder), rationale only "easier to maintain, less buggy"; lengthy one-time migration |
| 19 | https://restic.readthedocs.io/en/latest/100_references.html | restic design | L1 | 2026 | Packs under data/<hex-prefix>/; blobs 512 KiB-8 MiB, ~1 MiB avg, Rabin CDC; index files |
| 20 | https://kopia.io/docs/advanced/architecture/ | Kopia architecture | L1 | 2026 | Blocks <=20 MB packed into 20-40 MB packs; index blobs map block->pack/offset |
| 21 | https://perkeep.org/doc/overview | Perkeep overview | L1 | 2013 (stale) | Immutable digest-named blobs; index is derived and rebuildable; backends local disk/S3 |
| 22 | https://github.com/seaweedfs/seaweedfs | SeaweedFS README | L1 | 2026 | Append-only volume files, 16 B in-memory index/blob; filer metadata in Postgres/MySQL/etc; Apache-2.0; `weed mini` for single node |
| 23 | https://garagehq.deuxfleurs.fr/documentation/reference-manual/configuration/ | Garage configuration | L1 | 2026 | LMDB default (can corrupt after unclean shutdown), SQLite slower; fsync off by default; 1 MiB blocks; zstd; monthly scrub; replication_factor 1 "test only" |
| 24 | https://garagehq.deuxfleurs.fr/documentation/design/goals/ | Garage goals | L1 | 2026 | Non-goals: extreme performance, erasure coding, POSIX; targets multi-node geo-distributed |
| 25 | https://garagehq.deuxfleurs.fr/documentation/reference-manual/s3-compatibility/ | Garage S3 compatibility | L1 | 2026 | Multipart, ListObjectsV2, CopyObject implemented; no versioning, partial lifecycle |
| 26 | https://itsfoss.com/news/minio-moves-away-from-open-source/ and https://vonng.com/en/db/minio-resurrect/ | MinIO status | L3 | 2025-26 | MinIO community edition: source-only, no binaries/images, "maintenance mode" (Dec 2025); one blog claims archived Feb 2026 |
| 27 | https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html | S3 multipart limits | L1 | 2026 | Max 10,000 parts, part 5 MiB-5 GiB, object up to 48.8 TiB |
| 28 | https://tus.io/protocols/resumable-upload | tus 1.0.0 | L1 | 2016 | Offset-based resume; HEAD returns Upload-Offset; PATCH with mismatched offset -> 409; concatenation extension |
| 29 | https://sqlite.org/wal.html | SQLite WAL | L1 | 2026 | One writer at a time; readers don't block writer; needs shared memory, no network FS; checkpoint starvation |
| 30 | https://sqlite.org/whentouse.html | Appropriate uses for SQLite | L1 | 2026 | Client/server preferred for many concurrent writers, network-separated data, ~TB scale |
| 31 | https://sqlite.org/fasterthanfs.html | 35% faster than the filesystem | L1 | 2026 | Small blobs (~10 KB) faster in SQLite than files; irrelevant to large blobs |
| 32 | https://fly.io/blog/litestream-revamped/ | Litestream revamped | L2 | 2025 | Litestream moving to VFS-based read replicas over S3-compatible storage; actively developed |
| 33 | https://www.kunalganglani.com/blog/sqlite-vs-postgresql-for-apps | SQLite vs PostgreSQL 2026 | L3 | 2026 | Practitioner summary: SQLite single writer, ~10-50k write tx/s on NVMe in WAL, degrades past ~20 concurrent writers; no rigorous benchmark |
| 34 | https://event-driven.io/en/ordering_in_postgres_outbox/ | Ordering in Postgres outbox | L3 | n.d. | Sequence order != commit order; gaps ambiguous; fix: xid8 + pg_snapshot_xmin fencing; global gapless counter serializes writes |
| 35 | https://www.postgresql.org/support/versioning/ | PostgreSQL versioning | L1 | 2026-10 | PG 18 current (18.6), 5-year support; PG 14 EOL 2026-11-12; PG 19 RC1 2026-10-15 |
| 36 | https://crates.io/api/v1/crates/{sqlx,diesel,redb,rusqlite} | crates.io metadata | L1 | 2026-11-02 | sqlx 0.9.0 (2026-05-21, 179M dl); diesel 2.3.14 (2026-10-07); redb 4.4.0 (2026-10-13); rusqlite 0.41.1 (2026-10-28) |
| 37 | https://www.sea-ql.org/blog/2026-07-27-sea-orm-2.0/ | SeaORM 2.0 | L2 | 2026-07 | SeaORM 2.0 stable, built on SQLx 0.9 |
| 38 | https://apple.github.io/foundationdb/known-limitations.html | FoundationDB limitations | L1 | 2026 | 10 MB per tx, 5 s tx limit, 10 KB keys/100 KB values, no auth boundary |
| 39 | https://docs.kernel.org/admin-guide/ext4.html | ext4 admin guide | L1 | 2026 | auto_da_alloc handles replace-via-rename; data=ordered default; only journal checksums mentioned, no data checksums |
| 40 | https://en.wikipedia.org/wiki/Ext4 | ext4 | L3 | 2026 | Max file 16 TiB (4 KiB blocks); inode count fixed at mkfs; metadata checksums; project quotas; large_dir; delayed allocation caveat |
| 41 | https://btrfs.readthedocs.io/en/latest/Scrub.html | btrfs scrub | L1 | 2026 | Verifies data checksums; repair needs replicated profile; ~80% device bandwidth when idle; monthly recommended |
| 42 | https://btrfs.readthedocs.io/en/latest/Status.html | btrfs status | L1 | 2026 | RAID56 unstable ("testing only"); reflink OK; defrag unshares extents |
| 43 | https://www.phoronix.com/review/linux-617-filesystems/4 (and /linux-615-filesystems/3) | Linux 6.17 / 6.15 FS benchmarks | L2 | 2025 | Default configs, single PCIe 5 SSD: ext4 narrowly ahead of XFS in geomean; XFS led FS-Mark 1 MB files (6.15); btrfs, ZFS, bcachefs behind |
| 44 | https://docs.kernel.org/filesystems/fscrypt.html | fscrypt | L1 | 2026 | Per-directory keys on ext4/F2FS/UBIFS/CephFS; does not hide sizes, mtimes, xattrs |
| 45 | https://blog.cloudflare.com/speeding-up-linux-disk-encryption/ | Speeding up Linux disk encryption | L2 | 2020-03 | RAM-disk test: default dm-crypt ~87% slower per direction; workqueue-bypass flags merged in kernel 5.9 |
| 46 | https://despairlabs.com/blog/posts/2024-10-27-openzfs-dedup-is-good-dont-use-it/ and https://klarasystems.com/articles/zfs-fast-dedup-for-proxmox-ve-9x/ | OpenZFS fast dedup | L2/L3 | 2024-26 | Fast dedup (2.3) still needs the DDT in ARC/special vdev; one-way feature flag; compression preferred |
| 47 | https://www.usenix.org/conference/fast-08/analysis-data-corruption-storage-stack | Corruption in the storage stack (FAST'08) | L1/L2 | 2008 | 1.53 M drives, 41 months, >400k checksum mismatches; nearline ~10x enterprise; errors cluster |
| 48 | https://tanelpoder.com/posts/using-pg-test-fsync-for-testing-low-latency-writes/ and https://computingforgeeks.com/nvme-vs-sata-ssd-proxmox-datastore/ | fsync on PLP vs consumer SSD | L3 | 2024-26 | Enterprise PLP NVMe ~13k fsync/s (74 us); consumer ~350-2,000 fsync/s (vendor-style figure) |
| 49 | https://github.com/BLAKE3-team/BLAKE3 | BLAKE3 README | L1 | 2026 | Tree hash, SIMD, optional multithreading; "order of magnitude faster than sha256sum on typical desktop"; no numbers in text |
| 50 | https://lwn.net/Articles/549471/ and https://redlib.hbubli.cc/r/linuxadmin/comments/1fm4thc/ext4_hashindexed_directory | ext4 htree discussions | L3/L4 | 2013/2024 | HTree gives constant-depth lookup; huge flat dirs waste RAM; hashed subdir tree advised |
| 51 | https://docs.kernel.org/admin-guide/xfs.html | XFS admin guide | L1 | 2026 | V5 format adds metadata checksumming; no data checksum mentioned |

## Fact cards

Layout and comparable systems
- F1. Seafile: content-addressed Git-like store, files cut by content-defined chunking into blocks averaging ~8 MB, dedup across versions/libraries; commits/fs/blocks per repo dir or in object-store buckets. Source 1. High.
- F2. Seafile Community needs offline GC to reclaim blocks (online GC only in Pro); blocks of deleted data persist until GC. Source: search excerpts of Seafile/SeaTable docs; [limited source]. Med.
- F3. Seafile deploys with MySQL + Redis (SQLite no longer in the Docker overview). Source 2. Med (absence of SQLite is inferred).
- F4. Dropbox: blocks <=4 MB, SHA-256 keys, compressed+encrypted, immutable; changes recorded in a separate FileJournal; block->location index is sharded MySQL, chosen because Dropbox already ran thousands of MySQL nodes. Sources 3, 4. High.
- F5. Dropbox file metadata (Filesystem, Edgestore) is sharded MySQL; Panda was added because MySQL lacks cross-shard transactions and rebalancing. Source 4. High. (Relevance: even the largest sync vendor keeps tree metadata in a relational DB, not on the FS.)
- F6. Dropbox's Rust sync engine requires stable globally unique file IDs and atomic moves independent of subtree size; the earlier model (move = delete + add) could lose files. Source 5. High. Implication: tree must be ID-based, not path-keyed.
- F7. oCIS decomposedfs is ID-based (UUID nodes) with blobs kept apart from metadata; metadata moved from xattrs (oCIS 2) to MessagePack files (oCIS 3). Sources 8, 9. Med (docs not retrieved; [limited source]).
- F8. oCIS posixfs (real path tree + xattr metadata) is documented as experimental and unfit for production; needs inotify watcher, NATS KV ID cache; NFSv3/CephFS/SMB unsupported. Source 7. High. (Lesson: a "plain tree" whose truth is the FS needs a change-detection and ID-cache subsystem as soon as stable IDs and sync cursors are required.)
- F9. oCIS advises data on its own partition; sharing a partition with OS makes a full disk unresponsive; expired/partial uploads consume space. Source 6. High.
- F10. Immich keeps originals as files under per-user dirs, with paths and metadata only in Postgres; the library is never scanned; backup order is DB first, then files (worst case orphan files); Postgres must be on local SSD. Sources 14, 15. High.
- F11. Nextcloud (FS tree + DB cache `oc_filecache`) reports DB bloat (2.4 GB, orphan rows) and, with S3 primary storage, the DB becomes the only map to objects. Sources 11, 12. Med (L3/L4).
- F12. Nextcloud chunked upload: the final "MOVE" that assembles chunks of a ~43 GB file exceeded a ~100 s proxy timeout and the client then deleted the chunks; another report: 2.5 GB took 60-75 s to assemble on NFS. Source 13. Med (L4, 2026).
- F13. restic/Kopia: content-defined chunks (~1 MiB restic; Kopia <=20 MB) aggregated into pack files, hex-prefix subdirectories, separate index; built for backup (versions + dedup), not for byte-range serving of whole files. Sources 19, 20. High.
- F14. Perkeep: immutable digest-named blobs plus a derived, rebuildable index. Source 21 (2013). Med.
- F15. SeaweedFS: append-only volume files, 16 B RAM index per blob, filer metadata in an external store (Postgres etc.), Apache-2.0, single-node `weed mini`. Source 22. High. Adds at least master + volume + filer + DB services.
- F16. Garage: S3 store aimed at multi-node geo-distribution; non-goals are extreme performance and POSIX; LMDB can corrupt after unclean shutdown; fsync off by default; its own doc says replication_factor=1 is for tests only. Sources 23, 24. High.
- F17. MinIO community edition: source-only, no binaries/images, "maintenance mode" since late 2025; archive claim single-source. Source 26. Med [limited source] for the Feb-2026 archive claim.
- F18. S3 multipart: max 10,000 parts, 5 MiB min part; 50 GiB at 10 MiB parts = 5,120 parts (analysis from source 27). High.
- F19. tus: resume by server-reported offset (HEAD), sequential PATCH, 409 on offset mismatch; concatenation extension for parallel parts. Source 28. High.

Filesystem
- F20. ext4: max file 16 TiB (4 KiB blocks); inode count fixed at mkfs; metadata and journal checksums only, no data checksums; HTree indexed dirs; `large_dir`; project quotas. Sources 39, 40. High. (XFS V5 also checksums metadata only, Source 51.)
- F21. ext4 `auto_da_alloc` (default) forces data allocation for replace-via-rename so a crash does not yield zero-length files, but correctness for durability still needs fsync(file) + rename + fsync(dir). Source 39 (+ standard POSIX practice). High for first half, Med for second.
- F22. Phoronix (default configs, single PCIe 5 SSD, kernel 6.17): ext4 narrowly ahead of XFS overall, then F2FS, btrfs, OpenZFS; 6.15 FS-Mark 1 MB files: XFS first, ext4 second. Source 43. Med (synthetic, single device, not our workload).
- F23. btrfs/ZFS give data checksums and scrub; self-heal needs redundancy (btrfs: replicated profile); btrfs scrub uses ~80% idle bandwidth; btrfs RAID56 unstable; nodatacow disables checksums. Sources 41, 42. High.
- F24. ZFS dedup (even "fast dedup" in 2.3) needs the dedup table in ARC/special vdev and is a one-way pool feature; compression is the recommended space saver. Source 46. Med.
- F25. Silent corruption is real in the field: FAST'08 saw >400k checksum mismatches over 1.53 M drives/41 months, nearline disks ~10x worse than enterprise, errors clustered. Source 47. High (2008 data, NetApp population).
- F26. Huge flat directories waste RAM; hashed subdirectory trees (git/restic/Squid style) are the norm; HTree keeps depth 1-2. Sources 19, 50. Med (L3/L4).
- F27. fsync cost depends on SSD power-loss protection: ~13k fsync/s (74 us) on an enterprise PLP NVMe vs roughly 350-2,000/s on consumer NVMe (second figure vendor-style). Source 48. Med.
- F28. Disk-level encryption: dm-crypt default path was ~87% slower in a RAM-disk test until kernel 5.9 workqueue-bypass flags; on real 1 Gbps-class workloads the bottleneck is elsewhere (analysis). fscrypt (ext4) offers per-directory keys but leaves sizes/mtimes visible. Sources 44, 45. Med.
- F29. File-level dedup benefit for photos/videos: no quantitative source found; compressed media rarely share blocks (general knowledge). [limited source]. Low.

Metadata database
- F30. PostgreSQL 18 is current (18.6), supported to Nov 2030; PG 14 EOL 2026-11-12; PG 19 RC1 Oct 2026. Source 35. High.
- F31. Nextcloud recommends PostgreSQL 18/MariaDB 11.8 and limits SQLite to test/minimal instances; Immich requires Postgres; Seafile uses MySQL; Dropbox uses sharded MySQL; oCIS has no SQL DB (metadata in files). Sources 2, 4, 10, 15, 7/8. High.
- F32. SQLite WAL: one writer at a time, readers unblocked, needs shared memory (same host, no network FS), checkpoint starvation possible. SQLite's own guidance: prefer client/server for many concurrent writers or network-separated data. Sources 29, 30. High.
- F33. SQLite write throughput on NVMe in WAL mode is reported at ~10-50k tx/s single writer, degrading beyond ~20 concurrent writers; no rigorous benchmark exists. Source 33. Low [limited source].
- F34. Litestream is actively developed (VFS replicas over S3-compatible storage), LiteFS status unclear. Source 32. Med. Litestream gives async replication/backup, not HA.
- F35. Syncthing moved LevelDB -> SQLite (DB per folder) in 2.0 for maintainability; single-process desktop daemon, not a multi-writer server. Source 18. High.
- F36. Postgres sequence/serial order is not commit order; a "changes since cursor" feed keyed on a global sequence can skip rows from slow transactions; fixes: xid8 + `pg_snapshot_xmin` fencing, or serialize writers per scope with a counter row; global gapless counter serializes all writes. Source 34. High.
- F37. FoundationDB: 10 MB / 5 s transaction limits, 10 KB keys, 100 KB values, no auth boundary; a distributed system for far larger scale than a self-hosted Compose install. Source 38. High.
- F38. redb 4.4.0: pure-Rust embedded ACID COW B+tree with MVCC readers + a writer, single process. Source 36 + docs.rs. Med (single-process property is from general design, [limited source]).
- F39. Rust crates (2026-11-02): sqlx 0.9.0 (async, SQL-first, migrations, Postgres/SQLite/MySQL; 179M downloads); Diesel 2.3.14 (sync, async via separate crate); SeaORM 2.0 stable on top of sqlx 0.9; rusqlite 0.41.1. Sources 36, 37. High.
- F40. Immich upgrade pain came from a Postgres extension swap (pgvecto.rs -> VectorChord; "No vector extension found" in v3 upgrades), not from core Postgres. Source 17. Med (L3/L4).
- F41. SQLite beats the FS only for small blobs (~10 KB, 35% faster reads); files are better for large blobs. Source 31. High.

## Options comparison

### Blob layout (what the bytes on disk look like)

| Criterion | A. ID-addressed immutable blobs, tree only in DB | B. Path-mirroring plain tree | C. Chunked CAS with dedup (Seafile/restic style) | D. S3-compatible backend (Garage/SeaweedFS) |
|---|---|---|---|---|
| Example systems | oCIS decomposedfs, Immich (DB owns paths), Dropbox blocks | Nextcloud default, oCIS posixfs, Syncthing | Seafile, Dropbox, restic, Kopia | Nextcloud/oCIS S3, Garage, SeaweedFS |
| Rename/move folder | DB row update, O(1), stable IDs (F6) | rename(2) plus DB mirror update; two systems can diverge on crash | DB only | DB only |
| Last-write-wins overwrite | write temp blob, DB txn swaps pointer, unlink old after commit; atomic | temp + fsync + rename; atomic on FS but DB must follow | new chunk list, old chunks wait for GC | new object, delete old |
| Tricky names (case, NFC/NFD, reserved, 300-char path) | FS never sees user names; zero problem | FS enforces 255-byte component, case rules, must escape; NFC/NFD stored as distinct bytes | none | none |
| Resumable 50 GiB upload (AC-3/4) | staging file `.part` pre-sized, pwrite at offsets, finalize = rename (no assembly copy) | same | chunks are natural unit but need chunk table + assembly metadata | multipart (10,000 part cap, 5,120 parts at 10 MiB) |
| Serve download/Range/share | sendfile/Range direct from file | same | reassemble from chunks; Range needs mapping | proxy or presigned URL |
| Integrity/scrub | re-hash blob vs DB sha256; FS-level scrub optional | same (hash in DB) | chunk hash = name, natural | backend scrub (Garage monthly) |
| Dedup | none by default; optional file-level via sha256 | none | block level (little gain on photos/video, F29) | none |
| Delta upload on big-file edit | none | none | yes (CDC) | none |
| Backup/rsync | blobs write-once => rsync/restic safe any time; need DB dump; order DB first (F10) | human-browsable, per-file restore without DB (best) but DB index still needed | repo-style, needs tool | backend-specific |
| Disk-full / quota | per-account byte counter in DB txn; fallocate on staging file fails early | same | same + GC lag | bucket-level |
| Extra moving parts | blob GC after DB commit (simple unlink queue) | FS<->DB reconciler (Nextcloud files:scan class), inotify (F8) | chunker, refcounts, GC, pack/index | extra services, extra DB, ops |
| Operator install (AC-22) | 2 services (app, Postgres) | same | same | 3-5 services |
| Verdict | recommended | acceptable only if "browse data dir by name" is a hard requirement | not justified; revisit if big-file delta needed | not for v1; keep a storage trait |

### Filesystem for the blob volume

| FS | Fit | Evidence | Note |
|---|---|---|---|
| ext4 | default, fine | F20, F22 | no data checksums; set `-i` (bytes-per-inode) if many tiny files; 16 TiB file cap far above 50 GiB |
| XFS | equal or better for big files/parallel IO | F22, F20 | dynamic inodes, reflink [limited source: kernel page did not confirm]; metadata checksums only |
| ZFS / btrfs | only if operator wants self-healing | F23, F24 | needs mirror/raidz to heal; ARC RAM; no dedup for photos |

Recommendation: ext4 or XFS by default; operator may use ZFS/btrfs mirror, app-level hash scrub works on all.

### Metadata DB

| Criterion | PostgreSQL 18 | SQLite (WAL) + Litestream | Embedded KV (redb/RocksDB) | FoundationDB |
|---|---|---|---|---|
| Concurrent writers (API + N workers) | MVCC, row locks | one writer, queueing (F32) | single writer/process | fine but overkill |
| Multi-container/host access | network, native | same host shared volume only, no network FS | one process owns the DB | cluster |
| Worker scale-out (AC-11) | yes | same-host only | no (API must proxy) | yes |
| Tree listing 10k rows | index range scan (ms, analysis) | same | hand-built secondary indexes | range read, 5 s tx limit |
| Change feed | per-account seq counter or xid8 fencing (F36) | single writer => seq is commit order, simplest | hand-built | versionstamps |
| Ad-hoc ops/queries, share links, admin | SQL | SQL | custom code | custom |
| Backup | pg_dump / PITR, pg_upgrade on majors | file copy, Litestream replicate | file copy | complex |
| Install burden | one more container, major-upgrade chore | none | none | high |
| Peer usage | Immich, Nextcloud (recommended), Dropbox-MySQL, Seafile-MySQL | Syncthing (single-user), Nextcloud (testing only) | none found | none found |
| Rust | sqlx 0.9 / SeaORM 2 / diesel | rusqlite / sqlx | redb 4.4 | bindings |
| Verdict | recommended | fallback if Compose must stay one container | reject | reject |

## AC realism

| Criterion | Our value | Researched value | Note |
|---|---|---|---|
| AC-1 users | 500 registered, 100 concurrent syncing | No vendor gives per-user scaling (Nextcloud docs: none; Immich: 2-4 cores, 6-8 GB RAM for photos). Dropbox scale (tens of millions QPS, F4) is 5+ orders higher. | Easily met on one Postgres node; the real limit is network and disk. 100 users at 100 Mbps each = 10 Gbps, so state test load as "100 sessions, X MB/s aggregate". Rows: 500 users x 50k files = 25M nodes, ~10-15 GB DB with indexes (analysis). |
| AC-2 throughput | >= 90% of 1 Gbps (~112 MB/s) | Single HDD sequential ~150-250 MB/s [limited source]; NVMe/SATA SSD far above. Hashing SHA-256 with SHA-NI ~0.5-2 GB/s [limited source]. | Met with a streaming pass; do not buffer into DB; avoid app-level re-encryption per chunk (use volume encryption). Check the AES-NI/volume-crypto cost on the target (F28). |
| AC-3 resume <= 10 MiB | 10 MiB | tus offset resume (F19); S3 min part 5 MiB (F18) | Pick chunk size <= 10 MiB and persist offsets so at most one in-flight chunk is lost; ack only after write (and fsync per N chunks); 50 GiB = 5,120 chunks. |
| AC-4 50 GiB | 50 GiB | ext4 16 TiB (F20); S3 multipart fine at 10 MiB parts; wire time at 1 Gbps ~430 s (analysis); SHA-256 pass ~30-100 s (analysis); Nextcloud 43 GB upload failed at synchronous assembly (F12) | Keep staging as one pre-allocated file and finalize by rename, so no copy; hash incrementally while uploading (or per-chunk hashes); finalize must be async or O(1) so proxy timeouts (~100 s) never apply. |
| AC-5 sync <= 5 s p95 (<= 10 MB) | 5 s | Postgres commit + NOTIFY wake-ups are ms; fsync per file 0.07 ms (PLP) to ~3 ms (consumer) (F27) | Achievable; cost is not the DB. Needs a push channel (SSE/WebSocket) plus cursor catch-up. |
| AC-6 list 10,000 entries <= 300 ms p95 | 300 ms | Index range scan of 10k rows is ms (analysis, [limited source]); payload ~2-3 MB JSON uncompressed (analysis) | Serialization and network dominate: 2.5 MB over 100 Mbps = 200 ms. Define as server-side time, keep paging (cursor) in the API; plain FS readdir at 10k entries is also fast but names/IDs/etag need stat or xattrs, so DB wins. |
| AC-7 hash verify, 0 silent corruption | 0 | In flight: end-to-end hash makes this achievable. At rest: ext4/XFS have no data checksums (F20, Source 51); field data shows silent mismatches exist (F25: >400k over 1.53 M drives, 41 months). | "0" cannot be guaranteed on ext4/XFS; restate as "all corruption is detected by a scheduled scrub and reported; repaired from backup or mirror". Scrub cost: re-reading 10 TB at 200 MB/s ~ 14 h (analysis); throttle. ECC RAM recommended. ZFS/btrfs mirror gives auto-repair (F23). |
| AC-12 revoke <= 5 s | 5 s | DB row check per request | Do not cache link state beyond a few seconds; trivial with Postgres. |

## Recommendation

Ranked options (storage layout plus DB):

1. **Immutable ID-addressed blobs on ext4 (or XFS), tree in PostgreSQL, accessed through a storage trait.** Blob path `data/blobs/<2-hex>/<2-hex>/<uuid>` (two-level fanout of 65,536 dirs keeps ~400 files per dir at 25M blobs, analysis). DB holds nodes (stable ID, parent_id, name, size, sha256, blob_id, per-account `seq`) and a blob-delete queue. Upload goes to a staging file on the same filesystem, is hashed, fsynced, renamed into place, then the DB commit publishes it; the old blob is unlinked after commit. Verdict: best fit for LWW, atomic overwrite, no name-edge-case exposure, O(1) folder moves, easy range serving, and no FS/DB reconciler.
2. **Path-mirroring plain tree on ext4 plus Postgres index.** This is the owner's hunch. It wins on "operator can browse and restore a single file by name". It loses on two sources of truth, tricky names hitting the FS, rename atomicity across FS+DB, and posixfs-style watcher/ID-cache machinery (F8, F11). Choose it only if browsability is a stated requirement.
3. **Chunked CAS with dedup (Seafile/Dropbox style).** Only worth it for delta sync of large edited files or heavy cross-user duplication. Dedup of photos/video is small [limited source]; costs are GC/refcounts, reassembly on every read, and a bigger operational surface.
4. **S3-compatible backend (SeaweedFS or Garage).** Not for v1: more services (AC-22), Garage's own single-node warning (F16), MinIO community edition source-only/maintenance (F17). Keep the storage trait so it can be added.

Metadata DB: **PostgreSQL 18, pinned image, core features only (no extensions)**, via **sqlx 0.9 with plain SQL migrations** (SQL-first, no ORM layer; SeaORM 2.0 is the only step up if wanted). Fallback: SQLite WAL if the install must be a single container and workers stay on the same host.

Deciding facts:
- (1) The tree must be ID-based with atomic moves; Dropbox needed that to stop losing files (F6), and a path-keyed FS tree forces a reconciler (oCIS posixfs experimental F8; Nextcloud oc_filecache drift F11).
- (2) AC-11 wants adding workers; SQLite WAL requires same-host shared memory and a single writer (F32), embedded KV is single-process (F38). Postgres is what Nextcloud recommends and Immich requires (F31).
- (3) 50 GiB resumable uploads: Nextcloud's chunk-assembly MOVE failed at ~43 GB behind a 100 s proxy limit (F12); a pre-sized staging file finalized by rename avoids any assembly copy.
- (4) ext4/XFS lack data checksums (F20); AC-7 "0 silent corruption" must be implemented as hash scrub + backup, or by ZFS/btrfs mirrors (F23, F25).

Design notes worth carrying forward:
- Change feed: increment `accounts.seq` inside the same transaction as each write (row lock held to commit, so per-account order equals commit order); feed query `WHERE account_id=$1 AND seq > $cursor ORDER BY seq`. This avoids the global-sequence skipping bug (F36) at the cost of serializing one account's writes, which is fine. For multi-user shared folders, use a counter per shared root (open question). Tombstones need a retention window.
- Durability order per upload: write data, fsync file, rename, fsync directory, then Postgres commit. A crash leaves at worst an orphan blob (swept by a GC with grace period), matching Immich's "DB first, files second" backup rule (F10) in reverse.
- Put Postgres and blobs on separate volumes (F9, F15); blobs write-once make rsync/restic backups safe at any moment, but the DB dump must come first.
- Encryption at rest: volume-level (LUKS) or ZFS native keeps sendfile, Range, hashing and transcoding workers simple; app-level per-blob encryption does not. fscrypt leaves sizes/mtimes visible (F28, F44). Other research areas should confirm.
- Per-fsync cost matters for the 100k-file initial sync: ~3 fsyncs per file x 100k = 300k fsyncs ≈ 23 s (13k/s PLP) to ~14 min (350/s consumer) depending on SSD class (analysis from F27); batch DB commits and use `syncfs` per batch for small files.

What would change the recommendation:
- Operator requirement to browse/restore the data dir by real names -> option 2.
- Users routinely edit huge files (VM images, PST, DB files) where a re-upload of 5-50 GiB per edit is unacceptable -> option 3 (CDC delta).
- Need to run workers on separate hosts without shared storage, or >1 PB / multi-site -> option 4 plus object-store blobs.
- Install must be a single container and writes are modest, workers co-located -> SQLite WAL.
- Operator can supply ZFS/btrfs mirrors and wants auto-repair -> same layout, FS changes only.

## Open questions

1. Must workers run on other hosts than the API? If yes, they need blob access over HTTP/S3/NFS, which changes the trait and the AC-11 test.
2. Is browsability of the data directory by real names a requirement (affects option 1 vs 2)?
3. Shared folders between users: which account owns the blob and quota, and what is the `seq` scope?
4. Per-account blob subdirectories (easy user delete/export/quota via ext4 project quotas, F20) vs one flat sharded tree. Leaning per-account; not researched in depth.
5. Tombstone retention and GC grace period lengths (Syncthing's 2.x uses a months-scale delete retention, Source 18; exact figure conflicts across sources, [limited source]).
6. Is AC-7 allowed to be re-stated as "detect + repair" for data at rest?
7. AC-6 definition: server time or end-to-end time over a client link; is paging acceptable?
8. Hash choice (SHA-256 vs BLAKE3 tree hash for parallel chunk verification, F49) belongs to the protocol research; no numeric benchmark found on SHA-NI hardware [limited source].
9. Not verified: XFS dynamic inode allocation/reflink details, ext4 default inode ratio for many-small-file installs, Garage license, Seafile community GC details (marked above).
