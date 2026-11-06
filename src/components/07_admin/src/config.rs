use std::time::Duration;

use sc_config::{KeyProblem, Reason, Settings};

/// Used when `account_delete_interval` is not set (07 §7: 5 s).
const DEFAULT_DELETE_INTERVAL: Duration = Duration::from_secs(5);
/// `erase_batch` takes the limit as `u32`, and `Page::limit` is `u32`.
const COUNT_CEILING: u64 = u32::MAX as u64;

/// The admin section of the key table, typed. Defaults and the deadline range come from `sc_config`.
///
/// `health_check_timeout` and `admin_list_page_size` have no default in the key table or in 07, so they stay
/// `None` until set; the code that needs them decides what an absent value means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminConfig {
    pub account_delete_batch_size: u64,
    pub account_delete_interval: Duration,
    pub account_delete_alert_after: Duration,
    pub account_delete_deadline: Duration,
    pub health_check_timeout: Option<Duration>,
    pub admin_list_page_size: Option<u64>,
}

impl AdminConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        AdminConfig {
            account_delete_batch_size: settings.account_delete_batch_size,
            account_delete_interval: settings
                .account_delete_interval
                .map_or(DEFAULT_DELETE_INTERVAL, |interval| interval.get()),
            account_delete_alert_after: settings.account_delete_alert_after.get(),
            account_delete_deadline: settings.account_delete_deadline.get(),
            health_check_timeout: settings.health_check_timeout.map(|timeout| timeout.get()),
            admin_list_page_size: settings.admin_list_page_size,
        }
    }

    /// Start validation: every problem, each naming its key. A zero would make a purger cycle or a listing
    /// page do no work, and a count above `u32::MAX` does not fit the port parameter.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        check_count(
            "account_delete_batch_size",
            self.account_delete_batch_size,
            &mut problems,
        );
        if let Some(page_size) = self.admin_list_page_size {
            check_count("admin_list_page_size", page_size, &mut problems);
        }
        if self.health_check_timeout == Some(Duration::ZERO) {
            problems.push(KeyProblem::new(
                "health_check_timeout",
                Reason::OutOfRange {
                    min: "1ms".to_owned(),
                    max: "no ceiling".to_owned(),
                },
            ));
        }
        problems
    }
}

fn check_count(key: &str, value: u64, problems: &mut Vec<KeyProblem>) {
    if !(1..=COUNT_CEILING).contains(&value) {
        problems.push(KeyProblem::new(
            key,
            Reason::OutOfRange {
                min: "1".to_owned(),
                max: COUNT_CEILING.to_string(),
            },
        ));
    }
}
