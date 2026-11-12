# Leftover: AZ-3747 gates not run on this host (owner option A, 2026-11-12)

kind: test (NOT-RUN(HW-REF), NOT-RUN(C-priv))
recorded: 2026-11-12; the security suite ST-04-1..8 passes on the developer workstation (ext4 on NVMe); the performance suite ran once as information only

Not run:
- PT-04-1..4 gates on REF-SRV with `TMPDIR` on the XFS blob volume: `cargo test -p sunnycloud-blobstore --release --test perf -- --ignored --nocapture --test-threads=1` (the ext4 comparison of PT-04-3 runs there too). Informational workstation figures are in `_docs/03_implementation/batch_47_loop1_report.md`.
- ST-04-1 whole-image scan (free space included): mount a loop XFS image with a user-writable directory, then `TMPDIR=<dir on the volume> ST_04_1_IMAGE=<image> cargo test -p sunnycloud-blobstore --test security -- --ignored stolen_disk` (needs sudo for the mount).

Replay: run both when REF-SRV exists (and the owner mounts the image), record the figures in the blobstore description section 12, then delete this file.
