//! The runner's view of the `decoder-runner` config section: what the socket loop, the safe
//! open and the watchdog read. The remaining keys (memory poll, pixel and codec limits) belong
//! to the executors.

use std::path::PathBuf;
use std::time::Duration;

use sc_config::{ConfigError, ConfigSources, DecoderSettings, KeyProblem, Reason, load};

use crate::limits::{Ceilings, JobLimits};

/// How long a stopping job may take to return after its cancel flag is set.
const CANCEL_WAIT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub socket_path: PathBuf,
    pub socket_gid: Option<u32>,
    /// The tree every `job_dir` must lie in.
    pub scratch_root: PathBuf,
    pub alive_interval: Duration,
    /// How long the main loop may stay silent before the runner aborts itself.
    pub watchdog: Duration,
    /// A write that blocks this long (a supervisor that stopped reading) ends the connection.
    pub write_timeout: Duration,
    pub cancel_wait: Duration,
    pub ceilings: Ceilings,
}

impl RunnerConfig {
    /// Binds and validates the section; a refusal lists every problem.
    pub fn load(sources: &ConfigSources) -> Result<Self, ConfigError> {
        let settings = load::<DecoderSettings>(sources)?;
        Self::from_settings(&settings)
    }

    fn from_settings(settings: &DecoderSettings) -> Result<Self, ConfigError> {
        let mut problems = Vec::new();
        let socket_gid = settings
            .decoder_gid
            .and_then(|gid| match u32::try_from(gid) {
                Ok(gid) => Some(gid),
                Err(_) => {
                    problems.push(KeyProblem::new(
                        "decoder_gid",
                        Reason::OutOfRange {
                            min: "0".into(),
                            max: u32::MAX.to_string(),
                        },
                    ));
                    None
                }
            });
        let alive_interval = settings.decoder_alive_interval.get();
        require_nonzero(
            "decoder_alive_interval",
            alive_interval.is_zero(),
            &mut problems,
        );
        let watchdog = settings.decoder_watchdog.get();
        if watchdog <= alive_interval {
            problems.push(KeyProblem::new(
                "decoder_watchdog",
                Reason::OutOfRange {
                    min: "above decoder_alive_interval".into(),
                    max: "no maximum".into(),
                },
            ));
        }
        let scratch_root = PathBuf::from(&settings.decoder_scratch_root);
        if !scratch_root.is_absolute() {
            problems.push(KeyProblem::new("decoder_scratch_root", Reason::WrongType));
        }
        let photo = job_limits(
            settings.photo_cpu,
            settings.photo_memory.bytes(),
            settings.photo_job_timeout.get(),
            settings.decoder_stall_kill.get(),
            settings.decoder_max_output_photo.bytes(),
        );
        let video = job_limits(
            settings.video_cpu,
            settings.video_memory.bytes(),
            settings.video_job_timeout.get(),
            settings.decoder_stall_kill.get(),
            settings.decoder_max_output_video.bytes(),
        );
        check_limits(&PHOTO_KEYS, &photo, &mut problems);
        check_limits(&VIDEO_KEYS, &video, &mut problems);
        require_nonzero(
            "decoder_stall_kill",
            photo.stall_timeout.is_zero(),
            &mut problems,
        );
        if !problems.is_empty() {
            return Err(ConfigError::Invalid(problems));
        }
        Ok(Self {
            socket_path: PathBuf::from(&settings.decoder_socket_path),
            socket_gid,
            scratch_root,
            alive_interval,
            watchdog,
            // Half the watchdog: a write blocked by a supervisor that stopped reading ends that connection
            // before the silent main loop would make the watchdog abort the whole runner.
            write_timeout: watchdog / 2,
            cancel_wait: CANCEL_WAIT,
            ceilings: Ceilings { photo, video },
        })
    }
}

struct LimitKeys {
    cpu: &'static str,
    memory: &'static str,
    wall: &'static str,
    max_output: &'static str,
}

const PHOTO_KEYS: LimitKeys = LimitKeys {
    cpu: "photo_cpu",
    memory: "photo_memory",
    wall: "photo_job_timeout",
    max_output: "decoder_max_output_photo",
};

const VIDEO_KEYS: LimitKeys = LimitKeys {
    cpu: "video_cpu",
    memory: "video_memory",
    wall: "video_job_timeout",
    max_output: "decoder_max_output_video",
};

fn check_limits(keys: &LimitKeys, limits: &JobLimits, problems: &mut Vec<KeyProblem>) {
    require_nonzero(keys.cpu, limits.cpu == 0, problems);
    require_nonzero(keys.memory, limits.memory_bytes == 0, problems);
    require_nonzero(keys.wall, limits.wall.is_zero(), problems);
    require_nonzero(keys.max_output, limits.max_output_bytes == 0, problems);
}

/// A zero ceiling would make every job unrunnable, and the protocol range is 1 and up.
fn require_nonzero(key: &str, is_zero: bool, problems: &mut Vec<KeyProblem>) {
    if is_zero {
        problems.push(KeyProblem::new(
            key,
            Reason::OutOfRange {
                min: "1".into(),
                max: "no maximum".into(),
            },
        ));
    }
}

fn job_limits(
    cpu: u64,
    memory_bytes: u64,
    wall: Duration,
    stall_timeout: Duration,
    max_output_bytes: u64,
) -> JobLimits {
    JobLimits {
        // A request's cpu is a `u32`, so a larger ceiling clamps nothing.
        cpu: u32::try_from(cpu).unwrap_or(u32::MAX),
        memory_bytes,
        wall,
        stall_timeout,
        max_output_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(vars: &[(&str, &str)]) -> ConfigSources {
        let mut sources = ConfigSources::default();
        for (key, value) in vars {
            sources.env.insert((*key).into(), (*value).into());
        }
        sources
    }

    #[test]
    fn defaults_bind_the_documented_ceilings() {
        let config = RunnerConfig::load(&sources(&[])).expect("defaults are valid");

        assert_eq!(config.ceilings.photo.cpu, 2);
        assert_eq!(config.ceilings.photo.memory_bytes, 2 << 30);
        assert_eq!(config.ceilings.photo.wall, Duration::from_secs(120));
        assert_eq!(config.ceilings.video.cpu, 4);
        assert_eq!(config.ceilings.video.max_output_bytes, 10 << 30);
        assert_eq!(config.ceilings.video.stall_timeout, Duration::from_secs(90));
        assert_eq!(config.alive_interval, Duration::from_secs(5));
    }

    #[test]
    fn an_invalid_duration_and_an_unknown_variable_are_both_listed() {
        let result = RunnerConfig::load(&sources(&[
            ("SUNNYCLOUD_DECODER_ALIVE_INTERVAL", "banana"),
            ("SUNNYCLOUD_NOT_A_KEY", "1"),
        ]));

        let Err(ConfigError::Invalid(problems)) = result else {
            panic!("expected a refusal, got {result:?}");
        };
        let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
        assert!(keys.contains(&"decoder_alive_interval"), "{keys:?}");
        assert!(keys.contains(&"SUNNYCLOUD_NOT_A_KEY"), "{keys:?}");
    }

    #[test]
    fn a_gid_above_u32_and_a_zero_interval_are_refused() {
        let result = RunnerConfig::load(&sources(&[
            ("SUNNYCLOUD_DECODER_GID", "4294967296"),
            ("SUNNYCLOUD_ENVIRONMENT", "development"),
            ("SUNNYCLOUD_DECODER_ALIVE_INTERVAL", "0s"),
        ]));

        let Err(ConfigError::Invalid(problems)) = result else {
            panic!("expected a refusal, got {result:?}");
        };
        let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["decoder_gid", "decoder_alive_interval"]);
    }

    #[test]
    fn a_watchdog_not_above_the_alive_interval_and_a_relative_scratch_root_are_refused() {
        let result = RunnerConfig::load(&sources(&[
            ("SUNNYCLOUD_ENVIRONMENT", "development"),
            ("SUNNYCLOUD_DECODER_ALIVE_INTERVAL", "5s"),
            ("SUNNYCLOUD_DECODER_WATCHDOG", "5s"),
            ("SUNNYCLOUD_DECODER_SCRATCH_ROOT", "scratch"),
        ]));

        let Err(ConfigError::Invalid(problems)) = result else {
            panic!("expected a refusal, got {result:?}");
        };
        let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["decoder_watchdog", "decoder_scratch_root"]);
    }

    #[test]
    fn the_scratch_root_and_the_watchdog_default_to_the_documented_values() {
        let config = RunnerConfig::load(&sources(&[])).expect("defaults are valid");

        assert_eq!(config.scratch_root, PathBuf::from("/scratch"));
        assert_eq!(config.watchdog, Duration::from_secs(20));
        assert_eq!(config.write_timeout, Duration::from_secs(10));
    }

    #[test]
    fn a_zero_ceiling_is_refused() {
        let result = RunnerConfig::load(&sources(&[
            ("SUNNYCLOUD_ENVIRONMENT", "development"),
            ("SUNNYCLOUD_PHOTO_JOB_TIMEOUT", "0s"),
            ("SUNNYCLOUD_VIDEO_CPU", "0"),
        ]));

        let Err(ConfigError::Invalid(problems)) = result else {
            panic!("expected a refusal, got {result:?}");
        };
        let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert!(keys.contains(&"video_cpu") && keys.contains(&"photo_job_timeout"));
    }
}
