//! IT-10-1 (handshake), IT-10-2 on the production container (the variants are AZ-3865), IT-10-3
//! (framing and busy handling).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use media_shared::decoder_protocol::{DecodeError, Request, Response};

use crate::common::Received;
use crate::decoder::{Decoder, Flavor, Variant, release_image};
use crate::docker::docker_ok;
use crate::peer::{greedy_limits, photo_limits, render};
use crate::refs::{pattern_heic, pattern_jpeg, record, workdir};

const WAIT: Duration = Duration::from_secs(30);

/// The output of a command run inside the image, which is where the tools' own versions are.
fn in_image(image: &str, script: &str) -> String {
    docker_ok(&[
        "run",
        "--rm",
        "--network",
        "none",
        "--entrypoint",
        "sh",
        image,
        "-c",
        script,
    ])
}

/// The versions the tools of the image print about themselves: vips, ffmpeg, and the libheif the
/// image links (the library file carries its version).
fn tool_versions(image: &str) -> (String, String, String) {
    let vips = in_image(image, "vips --version");
    let ffmpeg = in_image(image, "ffmpeg -version | head -n 1");
    let libheif = in_image(
        image,
        "ls /opt/media/lib | grep -E '^libheif\\.so\\.[0-9]+\\.[0-9]+\\.[0-9]+$'",
    );
    (
        vips.trim_start_matches("vips-").to_owned(),
        ffmpeg
            .strip_prefix("ffmpeg version ")
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or_else(|| panic!("ffmpeg -version said {ffmpeg:?}"))
            .to_owned(),
        libheif.trim_start_matches("libheif.so.").to_owned(),
    )
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_1_hello_returns_capabilities_and_a_wrong_version_is_refused() {
    for flavor in [Flavor::Interactive, Flavor::General] {
        let decoder = Decoder::start(&release_image(), flavor, &[]);
        let started = decoder.state();
        let mut peer = decoder.connect();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs();

        let answers: Vec<_> = (0..3).map(|_| peer.hello()).collect();
        let mismatch = peer.call(Request::Hello { protocol: 99 }, Duration::from_secs(5));
        let after = peer.hello();

        let capabilities = &answers[0];
        assert!(
            answers.iter().all(|answer| answer == capabilities),
            "{answers:?}"
        );
        assert_eq!(capabilities.protocol, 2);
        for text in [
            &capabilities.vips,
            &capabilities.ffmpeg,
            &capabilities.libheif,
        ] {
            assert!(!text.is_empty());
        }
        assert!(
            (1..=now).contains(&capabilities.build_timestamp),
            "{capabilities:?}"
        );
        let (vips, ffmpeg, libheif) = tool_versions(&release_image());
        assert_eq!(
            (
                &capabilities.vips,
                &capabilities.ffmpeg,
                &capabilities.libheif
            ),
            (&vips, &ffmpeg, &libheif),
            "hello names the versions the tools print"
        );
        assert!(!capabilities.nvenc_probe_ok && capabilities.nvenc_sessions == 0);
        assert_eq!(mismatch, Response::Error(DecodeError::ProtocolMismatch));
        assert_eq!(&after, capabilities);
        assert_eq!(
            decoder.state().started_at,
            started.started_at,
            "a mismatch must not restart the runner"
        );
        record("IT-10-1", &format!("{capabilities:?}"));
        decoder.finish();
    }
}

const FORBIDDEN_TARGETS: [&str; 5] = [
    "api:8080",
    "api:8081",
    "db:5432",
    "1.1.1.1:53",
    "example.com:443",
];

/// The item a variant flips, the variant in words, how the container differs, and its extra environment.
type Case<'a> = (&'a str, &'a str, Variant<'a>, &'a [(&'a str, &'a str)]);

/// `(name, ok)` of every item `self_check` reports on `decoder`.
fn self_check_items(decoder: &Decoder) -> Vec<(String, bool)> {
    let mut peer = decoder.connect();
    let answer = peer.call(
        Request::SelfCheck {
            forbidden_targets: FORBIDDEN_TARGETS.map(str::to_owned).into(),
        },
        WAIT,
    );
    let Response::SelfCheck(report) = answer else {
        panic!("self_check answered {answer:?}");
    };
    report
        .0
        .into_iter()
        .map(|item| (item.name, item.ok))
        .collect()
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_2_the_production_container_passes_every_self_check_item() {
    let decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);

    let items = self_check_items(&decoder);

    let names: Vec<&str> = items.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "forbidden_targets_unreachable",
            "environment_is_allowlist",
            "no_blob_or_secret_mounts",
            "not_root",
            "no_capabilities",
            "no_new_privs",
            "root_filesystem_read_only",
        ]
    );
    assert!(items.iter().all(|(_, ok)| *ok), "{items:?}");
    record("IT-10-2", &format!("production items: {names:?}"));
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_2_each_variant_flips_only_its_own_item() {
    let blobs = tempfile::tempdir().expect("a host directory to mount as the blob path");
    let blob_mount = format!("{}:/var/lib/sunnycloud/blobs", blobs.path().display());
    let production = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let names: Vec<String> = self_check_items(&production)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    production.finish();
    let cases: [Case; 5] = [
        (
            "no_capabilities",
            "cap_add NET_RAW",
            Variant {
                extra: &["--cap-add", "NET_RAW"],
                writable_root: false,
            },
            &[],
        ),
        (
            "not_root",
            "uid 0",
            Variant {
                extra: &["--user", "0:0"],
                writable_root: false,
            },
            &[],
        ),
        (
            "root_filesystem_read_only",
            "writable rootfs",
            Variant {
                extra: &[],
                writable_root: true,
            },
            &[],
        ),
        (
            "environment_is_allowlist",
            "X_TEST=1",
            Variant::default(),
            &[("X_TEST", "1")],
        ),
        (
            "no_blob_or_secret_mounts",
            "blob path mounted",
            Variant {
                extra: &["--volume", &blob_mount],
                writable_root: false,
            },
            &[],
        ),
    ];

    for (flipped, label, variant, env) in cases {
        let decoder = Decoder::start_variant(&release_image(), Flavor::Interactive, env, variant);
        let items = self_check_items(&decoder);
        decoder.finish();

        let flipped_items: Vec<&str> = items
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(name, _)| name.as_str())
            .collect();
        let same_names: Vec<&str> = items.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(flipped_items, [flipped], "variant {label}: {items:?}");
        assert_eq!(same_names, names, "variant {label}: item names");
        record(
            "IT-10-2",
            &format!("variant {label}: only {flipped} is not ok"),
        );
    }
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_3_a_second_render_still_answers_busy_and_the_first_job_completes() {
    let work = workdir();
    let heic = pattern_heic(work.path(), "slow.heic", 4000, 3000);
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let first = decoder.job(&heic);
    let second = decoder.job(&pattern_jpeg(work.path(), "other.jpg", 800, 600));
    let mut peer = decoder.connect();

    peer.send(render(&first, 1440, photo_limits()));
    peer.send(render(&second, 320, photo_limits()));
    let busy = peer.response(Duration::from_secs(5));
    let done = peer.response(WAIT);

    assert_eq!(busy, Response::Error(DecodeError::Busy));
    let Response::ImageResult(result) = done else {
        panic!("the first job answered {done:?}");
    };
    assert_eq!((result.width, result.height), (1440, 1080));
    decoder.finish();
}

fn ends_the_runner(name: &str, send: impl FnOnce(&mut crate::peer::Peer)) {
    let decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let waiter = decoder.waiter();
    let mut peer = decoder.connect();

    send(&mut peer);
    let reply = peer.next_response(Duration::from_secs(5));
    let stopped = waiter.stopped_within(Duration::from_secs(5));

    match reply {
        Received::Reply(media_shared::decoder_protocol::Reply::Response(response)) => {
            assert_eq!(
                response,
                Response::Error(DecodeError::ProtocolViolation),
                "{name}"
            );
        }
        Received::Closed => {}
        other => panic!("{name}: neither a violation reply nor a close: {other:?}"),
    }
    let (code, _) = stopped.unwrap_or_else(|| panic!("{name}: the runner did not exit within 5 s"));
    assert_eq!(code, 2, "{name}: exit code of a protocol violation");
    let mut again = decoder.wait_restarted(&before, WAIT);
    again.hello();
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_3_an_oversize_frame_is_a_violation_and_the_runner_exits() {
    ends_the_runner("65,537 byte frame", |peer| {
        peer.send_payload(&vec![b'x'; 65_537])
    });
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_3_a_frame_with_an_unknown_field_is_a_violation_and_the_runner_exits() {
    ends_the_runner("unknown field", |peer| {
        peer.send_payload(br#"{"request":{"hello":{"protocol":2,"extra":true}}}"#);
    });
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_3_a_length_prefix_of_4_gib_is_a_violation_and_the_runner_exits() {
    ends_the_runner("4 GiB prefix", |peer| {
        peer.send_raw(&u32::MAX.to_be_bytes())
    });
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_3_limits_above_every_ceiling_are_accepted_and_the_job_runs() {
    let work = workdir();
    let mut decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let job = decoder.job(&pattern_jpeg(work.path(), "p.jpg", 1200, 900));
    let mut peer = decoder.connect();

    let answer = peer.call(render(&job, 320, greedy_limits()), WAIT);

    assert!(matches!(answer, Response::ImageResult(_)), "{answer:?}");
    decoder.finish();
}
