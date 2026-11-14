//! Measurement helpers of the PT-06 scenarios: percentiles, the host name printed with every result line, the
//! process CPU time and distinct visitor addresses.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

/// Linux reports process CPU time in `USER_HZ` ticks, 100 per second on every supported kernel configuration.
const USER_HZ: f64 = 100.0;

pub fn percentile(times: &mut [Duration], pct: usize) -> Duration {
    assert!(!times.is_empty(), "no samples");
    times.sort_unstable();
    times[(times.len() * pct / 100).min(times.len() - 1)]
}

/// The host the numbers were taken on, printed with every result line.
pub fn host() -> String {
    match std::fs::read_to_string("/proc/sys/kernel/hostname") {
        Ok(name) => name.trim().to_owned(),
        Err(error) => format!("unknown host ({error})"),
    }
}

/// User plus system CPU time of this process (`/proc/self/stat` fields 14 and 15).
pub fn process_cpu() -> Duration {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("read /proc/self/stat");
    let (_, after_name) = stat
        .rsplit_once(") ")
        .expect("stat has the command in parentheses");
    let fields: Vec<&str> = after_name.split_whitespace().collect();
    // `after_name` starts at field 3 (state), so fields 14 and 15 are at indexes 11 and 12.
    let ticks: f64 = fields[11..13]
        .iter()
        .map(|field| field.parse::<f64>().expect("a tick count"))
        .sum();
    Duration::from_secs_f64(ticks / USER_HZ)
}

/// The `n`th address of a /16 that no other test uses; `block` keeps visitors and attackers apart.
pub fn address(block: u8, n: usize) -> IpAddr {
    let n = u16::try_from(n).expect("at most 65,536 addresses");
    let [high, low] = n.to_be_bytes();
    IpAddr::V4(Ipv4Addr::new(10, block, high, low))
}
