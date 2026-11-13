//! The pure checks of a worker output before and after it is stored: the size cap of its kind, the dimension
//! bound, and whether a job has all its expected outputs.

use media_shared::policy::{OutputSpec, Params};
use media_shared::types::{DerivedKind, JobKind};

use crate::config::MediaConfig;

/// The per-kind byte caps (`derived_max_thumb`, `..._preview`, `..._transcode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputCaps {
    pub thumb: u64,
    pub preview: u64,
    pub transcode: u64,
}

impl OutputCaps {
    pub(crate) fn from_config(config: &MediaConfig) -> Self {
        OutputCaps {
            thumb: config.derived_max_thumb,
            preview: config.derived_max_preview,
            transcode: config.derived_max_transcode,
        }
    }

    /// The declared length as the writer takes it, or `None` when it is over the kind's cap.
    pub(crate) fn admit(&self, kind: DerivedKind, length: u64) -> Option<i64> {
        let cap = match kind {
            DerivedKind::Thumb => self.thumb,
            DerivedKind::Preview => self.preview,
            DerivedKind::Transcode => self.transcode,
        };
        if length > cap {
            return None;
        }
        i64::try_from(length).ok()
    }
}

/// The reported dimensions as the `derived` row stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dimensions {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration_ms: Option<i32>,
}

/// The reported width and height must be positive and within the largest output size the params define for the
/// kind (proposed default 2026-11-13: a still's long edge, a transcode's long and short side); the duration must
/// fit the column. `None` refuses the output.
pub(crate) fn bounded_dimensions(
    kind: DerivedKind,
    params: &Params,
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
) -> Option<Dimensions> {
    let (long_max, short_max) = match OutputSpec::for_kind(kind, params).max_edge {
        Some(edge) => (edge, edge),
        None => (params.video.max_long_side, params.video.max_short_side),
    };
    let sides: Vec<u32> = [width, height].into_iter().flatten().collect();
    let in_bounds = sides.iter().all(|side| (1..=long_max).contains(side))
        && (sides.len() < 2 || sides.iter().min().is_some_and(|short| *short <= short_max));
    if !in_bounds {
        return None;
    }
    Some(Dimensions {
        width: fit(width)?,
        height: fit(height)?,
        duration_ms: fit(duration_ms)?,
    })
}

/// `Some(None)` for an absent value, `None` for one the integer column cannot hold.
fn fit<T: TryInto<i32>>(value: Option<T>) -> Option<Option<i32>> {
    match value {
        None => Some(None),
        Some(value) => value.try_into().ok().map(Some),
    }
}

/// True when every output the job kind must produce is stored.
pub(crate) fn all_outputs_present(job: JobKind, present: &[DerivedKind]) -> bool {
    job.outputs().iter().all(|kind| present.contains(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> &'static Params {
        Params::for_version(1).expect("params v1")
    }

    #[test]
    fn a_job_closes_only_when_every_output_of_its_kind_exists() {
        // Arrange
        let cases = [
            (JobKind::Stills, vec![DerivedKind::Preview], false),
            (JobKind::Stills, vec![DerivedKind::Thumb], false),
            (
                JobKind::Stills,
                vec![DerivedKind::Preview, DerivedKind::Thumb],
                true,
            ),
            (JobKind::Poster, vec![DerivedKind::Thumb], true),
            (JobKind::Poster, vec![DerivedKind::Transcode], false),
            (JobKind::Transcode, vec![DerivedKind::Transcode], true),
            (JobKind::Transcode, vec![], false),
        ];

        // Act and Assert
        for (job, present, complete) in cases {
            assert_eq!(
                all_outputs_present(job, &present),
                complete,
                "{job:?} {present:?}"
            );
        }
    }

    #[test]
    fn the_cap_admits_up_to_its_value_and_refuses_one_byte_more() {
        // Arrange
        let caps = OutputCaps {
            thumb: 64 * 1024,
            preview: 10,
            transcode: u64::MAX,
        };

        // Act
        let admitted = [
            caps.admit(DerivedKind::Thumb, 64 * 1024),
            caps.admit(DerivedKind::Thumb, 65 * 1024),
            caps.admit(DerivedKind::Preview, 11),
            caps.admit(DerivedKind::Transcode, u64::MAX),
        ];

        // Assert
        assert_eq!(admitted, [Some(64 * 1024), None, None, None]);
    }

    #[test]
    fn still_dimensions_are_bounded_by_the_long_edge_of_the_kind() {
        // Arrange
        let cases = [
            (DerivedKind::Thumb, Some(320), Some(240), true),
            (DerivedKind::Thumb, Some(321), Some(240), false),
            (DerivedKind::Thumb, Some(0), None, false),
            (DerivedKind::Preview, Some(1080), Some(1440), true),
            (DerivedKind::Preview, Some(1441), None, false),
            (DerivedKind::Preview, None, None, true),
        ];

        // Act and Assert
        for (kind, width, height, ok) in cases {
            let bounded = bounded_dimensions(kind, params(), width, height, None);
            assert_eq!(bounded.is_some(), ok, "{kind:?} {width:?}x{height:?}");
        }
    }

    #[test]
    fn transcode_dimensions_are_bounded_on_both_sides_whatever_the_orientation() {
        // Arrange
        let cases = [
            (Some(1920), Some(1080), true),
            (Some(1080), Some(1920), true),
            (Some(1920), Some(1088), false),
            (Some(1921), None, false),
            (None, Some(1920), true),
        ];

        // Act and Assert
        for (width, height, ok) in cases {
            let bounded =
                bounded_dimensions(DerivedKind::Transcode, params(), width, height, Some(9_000));
            assert_eq!(bounded.is_some(), ok, "{width:?}x{height:?}");
        }
    }

    #[test]
    fn a_duration_the_column_cannot_hold_is_refused_and_values_are_kept() {
        // Arrange
        let too_long = Some(u64::try_from(i32::MAX).expect("fits") + 1);

        // Act
        let refused = bounded_dimensions(DerivedKind::Transcode, params(), None, None, too_long);
        let kept = bounded_dimensions(
            DerivedKind::Transcode,
            params(),
            Some(1280),
            Some(720),
            Some(5),
        );

        // Assert
        assert_eq!(refused, None);
        assert_eq!(
            kept,
            Some(Dimensions {
                width: Some(1280),
                height: Some(720),
                duration_ms: Some(5),
            })
        );
    }
}
