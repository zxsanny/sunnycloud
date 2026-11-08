use std::time::Duration;

use crate::config::{HttpConfig, WorkerOutputLimits};
use crate::registry::{BodyClass, Timing};

/// The margin a long-poll gets on top of its `wait` (`api_v1_surface.md`, time limits).
pub(super) const LONG_POLL_MARGIN: Duration = Duration::from_secs(5);

/// What one request is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Plan {
    /// The most body bytes the route's class accepts.
    pub(super) ceiling: u64,
    /// How long the peer may send nothing while the handler waits for body data.
    pub(super) idle: Option<Duration>,
    /// How long the handler may take to answer; `None` is no cap.
    pub(super) request: Option<Duration>,
}

/// The configured durations and caps, and the rules that turn a route class into a [`Plan`].
#[derive(Debug, Clone)]
pub(super) struct Settings {
    request_timeout: Duration,
    body_idle_timeout: Duration,
    longpoll_max_wait: Duration,
    worker_output: WorkerOutputLimits,
}

impl Settings {
    pub(super) fn from_config(config: &HttpConfig) -> Settings {
        Settings {
            request_timeout: config.request_timeout,
            body_idle_timeout: config.body_idle_timeout,
            longpoll_max_wait: config.server_info.longpoll_max_wait,
            worker_output: config.worker_output_limits,
        }
    }

    /// `kind` is the `{kind}` of a worker output route; `wait` the `wait` query value of a long-poll.
    pub(super) fn plan(
        &self,
        class: BodyClass,
        timing: Timing,
        kind: Option<&str>,
        wait: Option<Duration>,
    ) -> Plan {
        let idle =
            matches!(class, BodyClass::Block | BodyClass::Staged).then_some(self.body_idle_timeout);
        let request = match timing {
            Timing::Download => None,
            Timing::LongPoll => Some(wait.map_or(self.request_timeout, |wait| {
                wait.min(self.longpoll_max_wait) + LONG_POLL_MARGIN
            })),
            Timing::Request if idle.is_some() => None,
            Timing::Request => Some(self.request_timeout),
        };
        Plan {
            ceiling: self.ceiling(class, kind),
            idle,
            request,
        }
    }

    /// An unknown output kind gets the largest cap, so the owner (08) answers with its own rejection.
    fn ceiling(&self, class: BodyClass, kind: Option<&str>) -> u64 {
        if class != BodyClass::WorkerOutput {
            return class.limit_bytes();
        }
        let caps = &self.worker_output;
        match kind {
            Some("thumb") => caps.thumb,
            Some("preview") => caps.preview,
            Some("transcode") => caps.transcode,
            _ => caps.thumb.max(caps.preview).max(caps.transcode),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;

    fn settings() -> Settings {
        Settings {
            request_timeout: Duration::from_secs(60),
            body_idle_timeout: Duration::from_secs(30),
            longpoll_max_wait: Duration::from_secs(30),
            worker_output: WorkerOutputLimits {
                thumb: 5 * MIB,
                preview: 20 * MIB,
                transcode: 10 * 1024 * MIB,
            },
        }
    }

    fn ceiling(class: BodyClass) -> u64 {
        settings().plan(class, Timing::Request, None, None).ceiling
    }

    #[test]
    fn each_class_has_the_limit_of_the_contract() {
        // Arrange
        let expected = [
            (BodyClass::Small, 64 * KIB),
            (BodyClass::Commit, 4 * MIB),
            (BodyClass::Batch, 4 * MIB),
            (BodyClass::Block, 2 * MIB),
            (BodyClass::Staged, 2 * MIB),
            (BodyClass::Query, 128 * KIB),
            (BodyClass::Auth, 16 * KIB),
            (BodyClass::None, 0),
        ];

        // Act
        let actual: Vec<_> = expected
            .iter()
            .map(|(class, _)| (*class, ceiling(*class)))
            .collect();

        // Assert
        assert_eq!(actual, expected);
    }

    #[test]
    fn a_worker_output_is_capped_by_its_kind_and_an_unknown_kind_by_the_largest() {
        // Arrange
        let settings = settings();

        // Act
        let cap = |kind| {
            settings
                .plan(BodyClass::WorkerOutput, Timing::Request, kind, None)
                .ceiling
        };

        // Assert
        assert_eq!(cap(Some("thumb")), 5 * MIB);
        assert_eq!(cap(Some("preview")), 20 * MIB);
        assert_eq!(cap(Some("transcode")), 10 * 1024 * MIB);
        assert_eq!(cap(Some("poster")), 10 * 1024 * MIB);
        assert_eq!(cap(None), 10 * 1024 * MIB);
    }

    #[test]
    fn block_and_staged_have_an_idle_timer_and_no_total_cap() {
        // Arrange
        let settings = settings();

        // Act
        let plans = [BodyClass::Block, BodyClass::Staged]
            .map(|class| settings.plan(class, Timing::Request, None, None));

        // Assert
        for plan in plans {
            assert_eq!(plan.idle, Some(Duration::from_secs(30)));
            assert_eq!(plan.request, None);
        }
    }

    #[test]
    fn other_classes_have_the_request_timeout_and_no_idle_timer() {
        // Arrange
        let settings = settings();

        // Act
        let plans = [
            BodyClass::Small,
            BodyClass::Commit,
            BodyClass::WorkerOutput,
            BodyClass::None,
        ]
        .map(|class| settings.plan(class, Timing::Request, None, None));

        // Assert
        for plan in plans {
            assert_eq!(plan.idle, None);
            assert_eq!(plan.request, Some(Duration::from_secs(60)));
        }
    }

    #[test]
    fn a_long_poll_runs_for_its_wait_plus_the_margin_capped_by_the_server_maximum() {
        // Arrange
        let settings = settings();

        // Act
        let plan = |wait| {
            settings
                .plan(BodyClass::None, Timing::LongPoll, None, wait)
                .request
        };

        // Assert
        assert_eq!(
            plan(Some(Duration::from_secs(4))),
            Some(Duration::from_secs(9))
        );
        assert_eq!(
            plan(Some(Duration::from_secs(300))),
            Some(Duration::from_secs(35))
        );
        assert_eq!(plan(None), Some(Duration::from_secs(60)));
    }

    #[test]
    fn a_download_has_no_total_cap() {
        // Arrange
        let settings = settings();

        // Act
        let plan = settings.plan(BodyClass::None, Timing::Download, None, None);

        // Assert
        assert_eq!(plan.request, None);
        assert_eq!(plan.idle, None);
    }
}
