//! AZ-3857 AC-2 through the real runner binary: an image over the pixel limit is refused from its
//! header at once and without memory, one at the limit renders. Each time is the best of three
//! fresh runners (a refusal ends the runner), measured by the client around the request, so a
//! single slow scheduling slice on a shared machine cannot fail a bound; the memory is the
//! resident size of the runner and of every process it started, sampled while the job runs.
mod common;

use std::time::{Duration, Instant};

use common::media::{Media, MediaRunner, PeakWatch, limits, png_header_only, render};
use common::reference::record;
use media_shared::decoder_protocol::{DecodeError, LimitKind, Response};

const MIB: u64 = 1 << 20;
const PIXELS: Response = Response::Error(DecodeError::LimitExceeded {
    limit: LimitKind::Pixels,
});

/// One render on a fresh runner: the answer, how long it took and the peak resident size.
fn timed_render(env: &[(&str, &str)], bytes: &[u8]) -> (Response, Duration, u64) {
    let mut runner = MediaRunner::start_with(env, &std::env::var("PATH").expect("PATH"));
    let mut client = runner.connect();
    let job = runner.job(bytes);
    let watch = PeakWatch::start(runner.pid());
    let started = Instant::now();
    let response = render(&mut client, &job, 320, limits());
    let took = started.elapsed();
    (response, took, watch.peak())
}

fn best_of_three(env: &[(&str, &str)], bytes: &[u8]) -> (Duration, u64) {
    let mut best = Duration::MAX;
    let mut peak = 0;
    for _ in 0..3 {
        let (response, took, rss) = timed_render(env, bytes);
        assert_eq!(response, PIXELS);
        best = best.min(took);
        peak = peak.max(rss);
    }
    (best, peak)
}

#[test]
fn an_image_of_exactly_the_pixel_limit_renders_and_the_runner_group_stays_below_200_mib() {
    let png = Media::new().image("limit.png", 1000, 1000);

    let (response, took, peak) = timed_render(&[("SUNNYCLOUD_PHOTO_MAX_PIXELS", "1000000")], &png);

    assert!(matches!(response, Response::ImageResult(_)), "{response:?}");
    assert!(peak < 200 * MIB, "peak resident {} MiB", peak / MIB);
    record(
        "AZ-3857 AC-2 host",
        &format!(
            "1000x1000 png at the limit: rendered in {took:?}, peak resident {} MiB",
            peak / MIB
        ),
    );
}

#[test]
fn an_image_one_column_over_the_pixel_limit_is_refused_in_under_a_second() {
    let png = Media::new().image("over.png", 1001, 1000);

    let (best, peak) = best_of_three(&[("SUNNYCLOUD_PHOTO_MAX_PIXELS", "1000000")], &png);

    assert!(best < Duration::from_secs(1), "best of three {best:?}");
    assert!(peak < 200 * MIB, "peak resident {} MiB", peak / MIB);
    record(
        "AZ-3857 AC-2 host",
        &format!("1001x1000 png over the limit: refused, best of three {best:?} (bound 1 s)"),
    );
}

#[test]
fn a_900_megapixel_header_is_refused_in_under_two_seconds_below_200_mib() {
    let bomb = png_header_only(30_000, 30_000);

    let (best, peak) = best_of_three(&[], &bomb);

    assert!(best < Duration::from_secs(2), "best of three {best:?}");
    assert!(peak < 200 * MIB, "peak resident {} MiB", peak / MIB);
    record(
        "AZ-3857 AC-2 host",
        &format!(
            "30000x30000 png header only: refused, best of three {best:?} (bound 2 s), peak resident {} MiB",
            peak / MIB
        ),
    );
}
