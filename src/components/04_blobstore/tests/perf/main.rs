//! Performance suite PT-04-1..4 (AZ-3747) on the real writer, reader, barrier and scrubber. The thresholds are
//! release gates valid on REF-SRV only (env C-REF): every case is ignored by default as `NOT-RUN(HW-REF)`, and a
//! run on any other host is informational and never a pass of the gate.
//!
//! The blob root is a folder under `TMPDIR`, so a run puts `TMPDIR` on the volume under test (the XFS blob
//! volume on REF-SRV; `tmpfs` measures nothing). Run in release:
//! `TMPDIR=<dir on the volume> cargo test --release -p sunnycloud-blobstore --test perf -- --ignored --nocapture`.
//! Each case prints one `PT-04-n` line with its numbers and the host before it asserts. Sizes and durations
//! default to the specification; the `PT_04_*` variables shrink them for a smoke run, thresholds unchanged.

#[allow(dead_code)]
#[path = "../scrub/support.rs"]
mod support;

mod group_commit;
mod load;
mod range;
mod scrub_impact;
mod stream;
