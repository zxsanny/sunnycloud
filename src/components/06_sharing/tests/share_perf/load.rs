//! Measurement helpers of the PT-06 scenarios: percentiles, the host and its hardware printed with every result
//! line, the pass or fail verdict of each REF-SRV metric against its target and failure threshold (AZ-3704 AC-4),
//! the process CPU time and distinct visitor addresses.

use std::fmt::Debug;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

/// Linux reports process CPU time in `USER_HZ` ticks, 100 per second on every supported kernel configuration.
const USER_HZ: f64 = 100.0;

pub fn percentile(times: &mut [Duration], pct: usize) -> Duration {
    assert!(!times.is_empty(), "no samples");
    times.sort_unstable();
    times[(times.len() * pct / 100).min(times.len() - 1)]
}

/// The host the numbers were taken on with its CPU model, logical CPUs and memory, printed with every result line:
/// the hardware description every perf report records (`tests/environment.md`, REF-SRV).
pub fn host() -> String {
    let name = match std::fs::read_to_string("/proc/sys/kernel/hostname") {
        Ok(name) => name.trim().to_owned(),
        Err(error) => format!("unknown host ({error})"),
    };
    let cpu = match std::fs::read_to_string("/proc/cpuinfo") {
        Ok(info) => {
            let model = proc_field(&info, "model name").unwrap_or("unknown CPU model");
            let logical = info
                .lines()
                .filter(|line| line.starts_with("processor"))
                .count();
            format!("{model}, {logical} logical CPUs")
        }
        Err(error) => format!("unknown CPU ({error})"),
    };
    let memory = match std::fs::read_to_string("/proc/meminfo") {
        Ok(info) => proc_field(&info, "MemTotal")
            .unwrap_or("unknown")
            .to_owned(),
        Err(error) => format!("unknown ({error})"),
    };
    format!("{name} ({cpu}, memory {memory})")
}

/// The value of the first `key: value` line of a `/proc` file.
fn proc_field<'a>(info: &'a str, key: &str) -> Option<&'a str> {
    info.lines().find_map(|line| {
        let (field, value) = line.split_once(':')?;
        (field.trim() == key).then(|| value.trim())
    })
}

/// The verdict of one REF-SRV metric: `PASS` at or below its target, `TARGET-MISSED` above the target but at or
/// below the failure threshold, `FAIL` above it.
pub fn verdict<T: PartialOrd>(value: &T, target: &T, fail: &T) -> &'static str {
    if value <= target {
        "PASS"
    } else if value <= fail {
        "TARGET-MISSED"
    } else {
        "FAIL"
    }
}

/// Prints one report line of a REF-SRV metric with its verdict; the caller asserts after every line is printed.
pub fn report<T: PartialOrd + Debug>(case: &str, metric: &str, value: T, target: T, fail: T) {
    println!(
        "{case} {metric} {value:?}: {} (target {target:?}, fail above {fail:?})",
        verdict(&value, &target, &fail)
    );
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

#[test]
fn a_metric_passes_at_its_target_misses_it_up_to_the_failure_threshold_and_fails_above() {
    // Arrange
    let (target, fail) = (Duration::from_millis(10), Duration::from_millis(50));

    // Act
    let verdicts = [10, 11, 50, 51].map(|ms| verdict(&Duration::from_millis(ms), &target, &fail));

    // Assert
    assert_eq!(verdicts, ["PASS", "TARGET-MISSED", "TARGET-MISSED", "FAIL"]);
}
