//! H3 (AZ-3865 slice): the decoder test cases on the decoder container as shipped, started with the
//! production flags. Every case is `#[ignore]` so a plain `cargo test` never passes by skipping:
//! run them with `DECODER_IMAGE=<release image> cargo nextest run -p decoder-runner --run-ignored only`.
//! The cases marked `stub` need the `test-stubs` variant (`DECODER_TEST_IMAGE`, default
//! `<DECODER_IMAGE>-test`); the others run on the release image. The harness plays the supervisor;
//! reference outputs come from its own host tools, never from the runner's report.

#[path = "../common/mod.rs"]
mod common;
mod decoder;
mod docker;
mod flags;
mod host;
#[path = "../../stub/marker.rs"]
mod marker;
mod peer;
mod refs;

mod it_containment;
mod it_input;
mod it_limits;
mod it_media_image;
mod it_media_video;
mod it_protocol;
mod it_transcode;
mod it_watchdog;
mod variants;
