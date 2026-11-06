//! The runner's own ceilings and the clamp that applies them to a request's `limits`.

use std::time::Duration;

use media_shared::decoder_protocol::Limits;

/// Which ceilings a job runs under: `render_still` is a photo job, `transcode` a video job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitClass {
    Photo,
    Video,
}

/// The limits a job runs with. Also the shape of a ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobLimits {
    pub cpu: u32,
    pub memory_bytes: u64,
    pub wall: Duration,
    pub stall_timeout: Duration,
    pub max_output_bytes: u64,
}

impl JobLimits {
    /// Each value is the smaller of the request and this ceiling. Every request value is
    /// compared as a plain integer or built with `Duration::from_millis` before the comparison,
    /// so a request of `u64::MAX` never reaches any `Duration` arithmetic.
    pub fn clamp(&self, request: &Limits) -> JobLimits {
        JobLimits {
            cpu: request.cpu.min(self.cpu),
            memory_bytes: request.memory_bytes.min(self.memory_bytes),
            wall: Duration::from_millis(request.wall).min(self.wall),
            stall_timeout: Duration::from_millis(request.stall_timeout).min(self.stall_timeout),
            max_output_bytes: request.max_output_bytes.min(self.max_output_bytes),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceilings {
    pub photo: JobLimits,
    pub video: JobLimits,
}

impl Ceilings {
    pub fn of(&self, class: LimitClass) -> &JobLimits {
        match class {
            LimitClass::Photo => &self.photo,
            LimitClass::Video => &self.video,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn photo() -> JobLimits {
        JobLimits {
            cpu: 2,
            memory_bytes: 128 << 20,
            wall: Duration::from_secs(3),
            stall_timeout: Duration::from_secs(90),
            max_output_bytes: 32 << 20,
        }
    }

    fn video() -> JobLimits {
        JobLimits {
            cpu: 4,
            memory_bytes: 4 * GIB,
            wall: Duration::from_secs(2400),
            stall_timeout: Duration::from_secs(90),
            max_output_bytes: 10 * GIB,
        }
    }

    fn greedy() -> Limits {
        Limits {
            cpu: 64,
            memory_bytes: 64 * GIB,
            wall: 3_600_000,
            stall_timeout: 3_600_000,
            max_output_bytes: 1 << 40,
        }
    }

    #[test]
    fn photo_request_above_every_ceiling_runs_with_the_photo_ceilings() {
        let ceiling = photo();

        let effective = ceiling.clamp(&greedy());

        assert_eq!(effective, ceiling);
    }

    #[test]
    fn video_request_above_every_ceiling_runs_with_the_video_ceilings() {
        let ceiling = video();

        let effective = ceiling.clamp(&greedy());

        assert_eq!(effective, ceiling);
    }

    #[test]
    fn values_below_the_ceiling_are_kept() {
        let request = Limits {
            cpu: 1,
            memory_bytes: 1 << 20,
            wall: 500,
            stall_timeout: 250,
            max_output_bytes: 4096,
        };

        let effective = photo().clamp(&request);

        assert_eq!(
            effective,
            JobLimits {
                cpu: 1,
                memory_bytes: 1 << 20,
                wall: Duration::from_millis(500),
                stall_timeout: Duration::from_millis(250),
                max_output_bytes: 4096,
            }
        );
    }

    #[test]
    fn a_request_of_the_type_maximum_clamps_without_overflow() {
        let request = Limits {
            cpu: u32::MAX,
            memory_bytes: u64::MAX,
            wall: u64::MAX,
            stall_timeout: u64::MAX,
            max_output_bytes: u64::MAX,
        };

        let effective = video().clamp(&request);

        assert_eq!(effective, video());
        assert!(effective.wall.checked_add(Duration::from_secs(1)).is_some());
    }

    #[test]
    fn ceilings_are_chosen_by_class() {
        let ceilings = Ceilings {
            photo: photo(),
            video: video(),
        };

        assert_eq!(ceilings.of(LimitClass::Photo), &photo());
        assert_eq!(ceilings.of(LimitClass::Video), &video());
    }
}
