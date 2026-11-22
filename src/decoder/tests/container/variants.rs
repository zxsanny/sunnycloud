//! AC-1 (production flags before any case) and AC-2 (the stubs exist only in the test variant).

use std::path::Path;
use std::process::Command;

use crate::decoder::{Decoder, Flavor, release_image, stub_image};
use crate::docker::docker_ok;
use crate::flags::assert_production_flags;

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn ac_1_both_decoders_start_with_the_production_flags() {
    for flavor in [Flavor::Interactive, Flavor::General] {
        // `start` inspects the container against the flags and panics on a mismatch.
        let decoder = Decoder::start(&release_image(), flavor, &[]);

        decoder.finish();
    }
}

fn check_image(image: &str) -> (bool, String) {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/ci/check-decoder-image.sh");
    let output = Command::new(script)
        .arg(image)
        .output()
        .expect("run check-decoder-image.sh");
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

fn files_of(image: &str) -> String {
    let script = "for f in /opt/media/bin/*.real /usr/local/bin/decoder-test-stub; do [ -e \"$f\" ] && echo \"$f\"; done; true";
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

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn ac_2_the_release_image_has_no_stub_and_the_variant_has_it() {
    let (release_passes, release_log) = check_image(&release_image());
    let (variant_passes, variant_log) = check_image(&stub_image());

    // The `no-test-hooks` property of check-decoder-image.sh is the one proof for the release image.
    assert!(
        release_passes,
        "the release image fails its image checks:\n{release_log}"
    );
    assert!(
        files_of(&release_image()).is_empty(),
        "stub files in the release image"
    );
    assert!(
        !variant_passes,
        "the test variant passes the release checks:\n{variant_log}"
    );
    assert!(
        variant_log.contains("FAIL no-test-hooks"),
        "the variant must fail the no-test-hooks property:\n{variant_log}"
    );
    let stubs = files_of(&stub_image());
    for file in [
        "/opt/media/bin/ffmpeg.real",
        "/opt/media/bin/ffprobe.real",
        "/opt/media/bin/vips.real",
        "/opt/media/bin/vipsheader.real",
        "/opt/media/bin/vipsthumbnail.real",
        "/usr/local/bin/decoder-test-stub",
    ] {
        assert!(
            stubs.lines().any(|line| line == file),
            "the variant lacks {file}: {stubs}"
        );
    }
    let label = docker_ok(&[
        "image",
        "inspect",
        "--format",
        "{{index .Config.Labels \"org.sunnycloud.variant\"}}",
        &stub_image(),
    ]);
    assert_eq!(label, "test-stubs");
}

/// The inspection must fail on a container that lacks a flag: a container made without a read-only root and
/// without the seccomp profile is refused, naming both.
#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn ac_1_a_container_without_the_production_flags_is_refused() {
    let name = format!("sc-h3-seeded-{}", std::process::id());
    let memory = (2_u64 << 30).to_string();
    docker_ok(&[
        "create",
        "--name",
        &name,
        "--label",
        "sunnycloud-suite=h3",
        "--network",
        "none",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges:true",
        "--init",
        "--restart",
        "unless-stopped",
        "--pids-limit",
        "512",
        "--cpus",
        "2",
        "--memory",
        &memory,
        "--memory-swap",
        &memory,
        "--user",
        "10003:10003",
        &release_image(),
    ]);

    let refused =
        std::panic::catch_unwind(|| assert_production_flags(&name, "interactive", 2, 2 << 30));
    docker_ok(&["rm", "--force", &name]);

    let payload = refused.expect_err("a container without the flags passed the inspection");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(
        message.contains("AC-1") && message.contains("read-only") && message.contains("seccomp"),
        "{message}"
    );
}
