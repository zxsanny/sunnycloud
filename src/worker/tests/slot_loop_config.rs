//! AZ-3872 AC-8, configuration part: the limits and waits the loop reads from the worker section.

use std::collections::BTreeMap;
use std::time::Duration;

use media_shared::decoder_protocol::Limits;
use media_shared::types::JobKind;
use sunnycloud_worker::{LoopConfig, load_settings};

mod support;

use support::{TOKEN, TokenFile, token_env};

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

fn config(extra: &[(&str, &str)]) -> LoopConfig {
    let token = TokenFile::new(TOKEN);
    let mut env: BTreeMap<String, String> = token_env(&token);
    env.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
    );
    LoopConfig::from_settings(&load_settings(env).expect("settings load"))
}

#[test]
fn defaults_give_the_documented_photo_and_video_limits() {
    // Act
    let config = config(&[]);

    // Assert
    assert_eq!(
        config.photo.limits,
        Limits {
            cpu: 2,
            memory_bytes: 2 * GIB,
            wall: 120_000,
            stall_timeout: 90_000,
            max_output_bytes: 32 * MIB,
        }
    );
    assert_eq!(
        config.video.limits,
        Limits {
            cpu: 4,
            memory_bytes: 4 * GIB,
            wall: 40 * 60_000,
            stall_timeout: 90_000,
            max_output_bytes: 10 * GIB,
        }
    );
    assert_eq!(config.photo.max_input, 256 * MIB);
    assert_eq!(config.video.max_input, 10 * GIB);
    assert_eq!(config.claim_poll_interval, Duration::from_millis(500));
}

#[test]
fn one_stall_kill_setting_feeds_both_kinds_and_durations_follow_the_settings() {
    // Act
    let config = config(&[
        ("SUNNYCLOUD_DECODER_STALL_KILL", "30s"),
        ("SUNNYCLOUD_PHOTO_JOB_TIMEOUT", "2s"),
        ("SUNNYCLOUD_WORKER_CLAIM_POLL_INTERVAL", "100ms"),
    ]);

    // Assert
    assert_eq!(config.photo.limits.stall_timeout, 30_000);
    assert_eq!(config.video.limits.stall_timeout, 30_000);
    assert_eq!(config.photo.limits.wall, 2_000);
    assert_eq!(config.claim_poll_interval, Duration::from_millis(100));
}

#[test]
fn stills_use_the_photo_limits_and_poster_and_transcode_the_video_limits() {
    // Arrange
    let config = config(&[]);

    // Act
    let kinds = [JobKind::Stills, JobKind::Poster, JobKind::Transcode]
        .map(|kind| config.for_kind(kind).limits.cpu);

    // Assert
    assert_eq!(kinds, [2, 4, 4]);
}

#[test]
fn watchdog_durations_default_to_the_documented_values_and_follow_the_settings() {
    // Act
    let defaults = config(&[]);
    let tuned = config(&[
        ("SUNNYCLOUD_DECODER_SILENCE_TIMEOUT", "1s"),
        ("SUNNYCLOUD_DECODER_CANCEL_GRACE", "500ms"),
        ("SUNNYCLOUD_PHOTO_JOB_TIMEOUT", "2s"),
    ]);

    // Assert
    assert_eq!(defaults.silence_timeout, Duration::from_secs(20));
    assert_eq!(defaults.cancel_grace, Duration::from_secs(5));
    assert_eq!(defaults.photo.wall(), Duration::from_secs(120));
    assert_eq!(defaults.video.wall(), Duration::from_secs(40 * 60));
    assert_eq!(tuned.silence_timeout, Duration::from_secs(1));
    assert_eq!(tuned.cancel_grace, Duration::from_millis(500));
    assert_eq!(tuned.photo.wall(), Duration::from_secs(2));
}
