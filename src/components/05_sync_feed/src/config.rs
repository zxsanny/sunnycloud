use std::time::Duration;

use sc_config::{KeyProblem, Reason, Settings};

pub const LONGPOLL_WAIT_MAX_FLOOR: Duration = Duration::from_secs(1);
pub const LONGPOLL_WAIT_MAX_CEILING: Duration = Duration::from_secs(55);

/// The feed section of the key table, typed. Defaults come from `sc_config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedConfig {
    pub longpoll_wait_max: Duration,
    pub longpoll_auth_recheck: Duration,
    pub feed_safety_recheck: Duration,
    pub feed_page_limit_default: u64,
    /// No default in the key table: absent means no ceiling beyond what the reader applies.
    pub feed_page_limit_max: Option<u64>,
    pub max_holds_per_principal: u64,
}

impl FeedConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        FeedConfig {
            longpoll_wait_max: settings.longpoll_wait_max.get(),
            longpoll_auth_recheck: settings.longpoll_auth_recheck.get(),
            feed_safety_recheck: settings.feed_safety_recheck.get(),
            feed_page_limit_default: settings.feed_page_limit_default,
            feed_page_limit_max: settings.feed_page_limit_max,
            max_holds_per_principal: settings.max_holds_per_principal,
        }
    }

    /// Start validation: every problem, each naming its key. `sc_config` binds the wait range only in
    /// production; it is a protocol bound (the client waits 10 s longer than the server), so it is checked in
    /// every environment. A zero interval would spin a loop, a zero count would refuse every read or hold, and a
    /// page ceiling below the default page size could never be reached.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        if !(LONGPOLL_WAIT_MAX_FLOOR..=LONGPOLL_WAIT_MAX_CEILING).contains(&self.longpoll_wait_max)
        {
            problems.push(out_of_range("longpoll_wait_max", "1s", "55s"));
        }
        for (key, value) in [
            ("longpoll_auth_recheck", self.longpoll_auth_recheck),
            ("feed_safety_recheck", self.feed_safety_recheck),
        ] {
            if value.is_zero() {
                problems.push(out_of_range(key, "1ms", "no ceiling"));
            }
        }
        for (key, value) in [
            ("feed_page_limit_default", self.feed_page_limit_default),
            ("max_holds_per_principal", self.max_holds_per_principal),
        ] {
            if value == 0 {
                problems.push(out_of_range(key, "1", "no ceiling"));
            }
        }
        if let Some(max) = self.feed_page_limit_max
            && max < self.feed_page_limit_default.max(1)
        {
            problems.push(out_of_range(
                "feed_page_limit_max",
                &self.feed_page_limit_default.max(1).to_string(),
                "no ceiling",
            ));
        }
        problems
    }
}

fn out_of_range(key: &str, min: &str, max: &str) -> KeyProblem {
    KeyProblem::new(
        key,
        Reason::OutOfRange {
            min: min.to_owned(),
            max: max.to_owned(),
        },
    )
}
