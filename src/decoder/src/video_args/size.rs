//! The output size of a transcode: the caps, the aspect ratio, the even sides.

/// The output size for an input of `width` x `height` stored pixels with `rotation` degrees of
/// display matrix: ffmpeg's autorotate shows the clip upright before the scale filter, so the
/// rule works on the displayed sides. The short side is at most `short_cap` and the long side
/// at most `long_cap` in either orientation, the aspect ratio is kept, nothing is enlarged and
/// both sides are even (at least 2).
pub(crate) fn target_size(
    width: u32,
    height: u32,
    rotation: Option<i32>,
    short_cap: u32,
    long_cap: u32,
) -> (u32, u32) {
    let turned = rotation.is_some_and(|degrees| degrees.rem_euclid(180) == 90);
    let (width, height) = if turned {
        (u64::from(height), u64::from(width))
    } else {
        (u64::from(width), u64::from(height))
    };
    let (short, long) = (width.min(height), width.max(height));
    let (short_cap, long_cap) = (u64::from(short_cap), u64::from(long_cap));
    // The scale factor is the smaller of the two cap ratios, kept as a fraction so that the
    // result is exact integer arithmetic.
    let (numerator, denominator) = if short_cap * long < long_cap * short {
        (short_cap, short)
    } else {
        (long_cap, long)
    };
    let (width, height) = if numerator < denominator {
        (
            width * numerator / denominator,
            height * numerator / denominator,
        )
    } else {
        (width, height)
    };
    (even(width), even(height))
}

fn even(side: u64) -> u32 {
    u32::try_from((side & !1).max(2)).unwrap_or(u32::MAX - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_keeps_each_orientation_within_the_caps_and_even() {
        let cases = [
            // (stored w, h, rotation) -> output w, h
            ((1920, 1080, None), (1920, 1080)),
            ((3840, 2160, None), (1920, 1080)),
            ((2160, 3840, None), (1080, 1920)),
            ((2160, 3840, Some(0)), (1080, 1920)),
            // A phone clip stored landscape and shown portrait through its display matrix.
            ((3840, 2160, Some(90)), (1080, 1920)),
            ((3840, 2160, Some(-90)), (1080, 1920)),
            ((3840, 2160, Some(270)), (1080, 1920)),
            ((3840, 2160, Some(180)), (1920, 1080)),
            // Larger than the long cap but a short side under its cap: the long cap rules.
            ((4096, 2160, None), (1920, 1012)),
            // Small inputs are not enlarged, odd sides lose a pixel.
            ((640, 360, None), (640, 360)),
            ((321, 241, None), (320, 240)),
            ((1281, 721, None), (1280, 720)),
            ((1, 1, None), (2, 2)),
            // Odd sides after a downscale.
            ((4001, 3001, None), (1438, 1080)),
        ];

        for ((width, height, rotation), expected) in cases {
            let size = target_size(width, height, rotation, 1080, 1920);

            assert_eq!(size, expected, "{width}x{height} rotation {rotation:?}");
            assert!(size.0.is_multiple_of(2) && size.1.is_multiple_of(2));
            assert!(size.0.min(size.1) <= 1080 && size.0.max(size.1) <= 1920);
        }
    }

    #[test]
    fn a_smaller_short_edge_from_the_plan_scales_down_further() {
        let size = target_size(1920, 1080, None, 720, 1920);

        assert_eq!(size, (1280, 720));
    }
}
