//! ST-03-5 disk-filler half (AZ-3758 AC-5; AC-39): the free-space floor at 03's boundary on the real 04 volume.
//! Its own test binary: the floor is measured as the free bytes of the runner's temporary volume, so no other test
//! of 03 may write to that volume while it runs (inside the binary the two tests take turns).

#[allow(dead_code, unused_imports)]
#[path = "../transfer/commit_ports.rs"]
mod commit_ports;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/put_support.rs"]
mod put_support;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/rig_config.rs"]
mod rig_config;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/start_ports.rs"]
mod start_ports;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/start_support.rs"]
mod start_support;
#[allow(dead_code, unused_imports)]
#[path = "../transfer/support.rs"]
mod support;

mod floor;
