//! Performance suite PT-03-1..4 (AZ-3759) at 03's boundary on the real 02, 04, 05 and 07a. The thresholds are
//! release gates valid on REF-SRV (C-REF; PT-03-2: the C-LARGE host) only: every case is ignored by default as
//! `NOT-RUN(HW-REF)`, and a run on any other host is informational, never a pass of the gate.
//!
//! The blob volume is a folder under `TMPDIR`, so a run puts `TMPDIR` on the volume under test (XFS on REF-SRV).
//! Run in release, one case at a time:
//! `TMPDIR=<dir on the volume> cargo test --release -p sunnycloud-transfer --test perf -- --ignored --nocapture
//! --test-threads=1`. Each case prints one `PT-03-n` line with its numbers and the host before it asserts.
//! PT-03-5 is `progress_memory.rs` (progress table) and `download_memory.rs` (5 GiB stream).

#[allow(dead_code, unused_imports)]
#[path = "../transfer/batch_support.rs"]
mod batch_support;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/commit_ports.rs"]
mod commit_ports;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/commit_support.rs"]
mod commit_support;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/put_support.rs"]
mod put_support;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/rig_config.rs"]
mod rig_config;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/rig_wiring.rs"]
mod rig_wiring;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/start_ports.rs"]
mod start_ports;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/start_support.rs"]
mod start_support;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/support.rs"]
mod support;

mod commit_time;
mod ingest;
mod load;
mod ranges;
mod small_files;
