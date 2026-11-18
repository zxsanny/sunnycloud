//! Measurement helpers of the PT-07 scenarios (copied from 06's `share_perf/load.rs`, which another crate's test
//! tree cannot share): percentiles, the host and its hardware printed with every result line, and the verdict of
//! each REF-SRV metric against its target and failure threshold.

use std::fmt::Debug;
use std::time::Duration;

pub fn percentile(times: &mut [Duration], pct: usize) -> Duration {
    assert!(!times.is_empty(), "no samples");
    times.sort_unstable();
    times[(times.len() * pct / 100).min(times.len() - 1)]
}

/// The host the numbers were taken on with its CPU model, logical CPUs and memory (`tests/environment.md`,
/// REF-SRV).
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

/// `PASS` at or below the target, `TARGET-MISSED` above it but at or below the failure threshold, `FAIL` above.
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

#[test]
fn a_metric_passes_at_its_target_misses_it_up_to_the_failure_threshold_and_fails_above() {
    // Arrange
    let (target, fail) = (Duration::from_millis(30), Duration::from_millis(100));

    // Act
    let verdicts = [30, 31, 100, 101].map(|ms| verdict(&Duration::from_millis(ms), &target, &fail));

    // Assert
    assert_eq!(verdicts, ["PASS", "TARGET-MISSED", "TARGET-MISSED", "FAIL"]);
}
