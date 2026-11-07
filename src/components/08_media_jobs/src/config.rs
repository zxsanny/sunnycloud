use std::time::Duration;

use sc_config::{KeyProblem, Reason, Secret, Settings};

pub const JOB_ATTEMPTS_MIN: u64 = 1;
pub const JOB_ATTEMPTS_MAX: u64 = 10;

/// The media section of the key table, typed. Defaults and the production ranges of `job_lease` come from
/// `sc_config`. The worker tokens stay `Secret` values: they print as `***` and are digested by the authenticator.
#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub job_lease: Duration,
    pub job_attempts: u64,
    pub job_retry_backoff: Duration,
    pub bulk_queue_threshold: u64,
    pub job_terminal_ttl: Duration,
    pub job_reap_interval: Duration,
    pub derived_janitor_interval: Duration,
    pub derived_max_thumb: u64,
    pub derived_max_preview: u64,
    pub derived_max_transcode: u64,
    pub worker_api_token: Secret<String>,
    /// Set only while the token is rotated.
    pub worker_api_token_next: Option<Secret<String>>,
}

impl MediaConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        MediaConfig {
            job_lease: settings.job_lease.get(),
            job_attempts: settings.job_attempts,
            job_retry_backoff: settings.job_retry_backoff.get(),
            bulk_queue_threshold: settings.bulk_queue_threshold,
            job_terminal_ttl: settings.job_terminal_ttl.get(),
            job_reap_interval: settings.job_reap_interval.get(),
            derived_janitor_interval: settings.derived_janitor_interval.get(),
            derived_max_thumb: settings.derived_max_thumb.bytes(),
            derived_max_preview: settings.derived_max_preview.bytes(),
            derived_max_transcode: settings.derived_max_transcode.bytes(),
            worker_api_token: settings.worker_api_token.clone(),
            worker_api_token_next: settings.worker_api_token_next.clone(),
        }
    }

    /// Start validation: every problem, each naming its key. `sc_config` binds the `job_lease` range only in
    /// production, so only a zero lease is refused here. A zero interval would spin a loop, a zero backoff would
    /// retry at once, a zero cap would refuse every output, an attempt count outside 1 to 10 would never run a
    /// job or never stop retrying it, and an empty token would admit an empty bearer.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        for (key, value) in [
            ("job_lease", self.job_lease),
            ("job_retry_backoff", self.job_retry_backoff),
            ("job_terminal_ttl", self.job_terminal_ttl),
            ("job_reap_interval", self.job_reap_interval),
            ("derived_janitor_interval", self.derived_janitor_interval),
        ] {
            if value.is_zero() {
                problems.push(out_of_range(key, "1ms", "no ceiling"));
            }
        }
        if !(JOB_ATTEMPTS_MIN..=JOB_ATTEMPTS_MAX).contains(&self.job_attempts) {
            problems.push(out_of_range("job_attempts", "1", "10"));
        }
        for (key, value) in [
            ("derived_max_thumb", self.derived_max_thumb),
            ("derived_max_preview", self.derived_max_preview),
            ("derived_max_transcode", self.derived_max_transcode),
        ] {
            if value == 0 {
                problems.push(out_of_range(key, "1B", "no ceiling"));
            }
        }
        if self.worker_api_token.expose().is_empty() {
            problems.push(KeyProblem::new("worker_api_token", Reason::MissingRequired));
        }
        if self
            .worker_api_token_next
            .as_ref()
            .is_some_and(|token| token.expose().is_empty())
        {
            problems.push(KeyProblem::new(
                "worker_api_token_next",
                Reason::MissingRequired,
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
