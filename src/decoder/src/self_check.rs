//! The part of the self-check (10 section 4) that holds on any host: the runner is not root
//! and holds no capability. The container-only items (forbidden targets unreachable, a clean
//! `environ`, `no_new_privs`, a read-only root, the mount list) are E-19/c12 and are not run;
//! the report names only what was checked, never an item that was not.

use std::fs;

use media_shared::decoder_protocol::{SelfCheckItem, SelfCheckReport};
use sc_observability::log_error;

pub fn report() -> SelfCheckReport {
    let status = match fs::read_to_string("/proc/self/status") {
        Ok(status) => status,
        Err(error) => {
            log_error!("self-check cannot read /proc/self/status", err = &error);
            return SelfCheckReport(vec![item("proc_status_readable", false)]);
        }
    };
    SelfCheckReport(vec![
        item("proc_status_readable", true),
        item(
            "not_root",
            field(&status, "Uid").is_some_and(|uid| uid != 0),
        ),
        item(
            "no_capabilities",
            capabilities(&status).is_some_and(|set| set == 0),
        ),
    ])
}

fn item(name: &str, ok: bool) -> SelfCheckItem {
    SelfCheckItem {
        name: name.into(),
        ok,
    }
}

/// The first number after `name:` (the real uid for `Uid`).
fn field(status: &str, name: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))?;
    line.split_whitespace().next()?.parse().ok()
}

fn capabilities(status: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))?;
    u64::from_str_radix(line.trim(), 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\tx\nUid:\t1000\t1000\t1000\t1000\nCapEff:\t0000000000000000\n";

    #[test]
    fn an_unprivileged_process_without_capabilities_passes() {
        assert_eq!(field(STATUS, "Uid"), Some(1000));
        assert_eq!(capabilities(STATUS), Some(0));
    }

    #[test]
    fn root_and_a_capability_are_read_as_such() {
        let status = "Uid:\t0\t0\t0\t0\nCapEff:\t000001ffffffffff\n";

        assert_eq!(field(status, "Uid"), Some(0));
        assert_ne!(capabilities(status), Some(0));
    }

    #[test]
    fn this_test_process_reports_all_items_ok_unless_it_runs_as_root() {
        let report = report();

        assert!(report.0.len() >= 3);
        if field(
            &fs::read_to_string("/proc/self/status").expect("status"),
            "Uid",
        ) != Some(0)
        {
            assert!(report.0.iter().all(|item| item.ok), "{report:?}");
        }
    }
}
