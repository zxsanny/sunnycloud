//! IT-09-12 and ST-09-3: every file the decoder could plant as `out/result.webp`, through both resolvers.

use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rustix::fs::{CWD, FileType, Mode, OFlags, fcntl_getfl, mknodat};
use tempfile::TempDir;
use uuid::Uuid;

use super::resolve::Resolver;
use super::{RejectReason, ScratchDir, ScratchError, ScratchRoot};

const CAP: u64 = 1024;
const SECRET: &[u8] = b"SECRET-worker-api-token";
const WEBP: &[u8] = b"RIFF\x04\0\0\0WEBPgood";

struct Fixture {
    _root_dir: TempDir,
    secrets: TempDir,
    job: ScratchDir,
}

impl Fixture {
    fn new(resolver: Resolver) -> Self {
        let root_dir = tempfile::tempdir().expect("temporary root");
        let secrets = tempfile::tempdir().expect("temporary secrets");
        fs::write(secrets.path().join("worker_api_token"), SECRET).expect("secret file");
        let gid = fs::metadata(root_dir.path()).expect("root metadata").gid();
        let mut root = ScratchRoot::new(root_dir.path(), gid);
        root.resolver = resolver;
        let job = root.create(Uuid::from_u128(1)).expect("job tree");
        Self {
            _root_dir: root_dir,
            secrets,
            job,
        }
    }

    fn result(&self) -> std::path::PathBuf {
        self.job.path().join("out/result.webp")
    }

    fn secret(&self) -> std::path::PathBuf {
        self.secrets.path().join("worker_api_token")
    }
}

fn resolvers() -> [(&'static str, Resolver); 2] {
    [
        ("openat2", Resolver::detect()),
        ("component walk", Resolver::component_walk()),
    ]
}

fn rejection(result: Result<super::OutputFile, ScratchError>) -> RejectReason {
    match result {
        Err(ScratchError::OutputRejected(reason)) => reason,
        other => panic!("expected output_rejected, got {other:?}"),
    }
}

#[test]
fn symlink_to_a_secret_file_is_rejected_without_reading() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        symlink(fixture.secret(), fixture.result()).expect("plant symlink");

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::Symlink, "{name}");
    }
}

#[test]
fn symlink_to_a_directory_is_rejected() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        symlink(fixture.secrets.path(), fixture.result()).expect("plant symlink");

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::Symlink, "{name}");
    }
}

#[test]
fn hard_link_to_a_file_in_the_input_directory_is_rejected() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        fs::write(fixture.job.path().join("in/other"), WEBP).expect("input file");
        fs::hard_link(fixture.job.path().join("in/other"), fixture.result()).expect("plant link");

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::Hardlinked, "{name}");
    }
}

#[test]
fn fifo_is_rejected_without_blocking() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        mknodat(
            CWD,
            fixture.result(),
            FileType::Fifo,
            Mode::RUSR | Mode::WUSR,
            0,
        )
        .expect("plant fifo");
        let started = Instant::now();

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::NotRegular, "{name}");
        assert!(started.elapsed() < Duration::from_secs(1), "{name}");
    }
}

#[test]
fn sparse_file_above_the_cap_is_rejected_by_its_fstat_size() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        let file = File::create(fixture.result()).expect("plant file");
        file.set_len(CAP + 1).expect("sparse length");

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::TooLarge, "{name}");
    }
}

#[test]
fn directory_named_like_the_result_is_rejected() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        fs::create_dir(fixture.result()).expect("plant directory");

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::NotRegular, "{name}");
    }
}

#[test]
fn missing_result_is_rejected() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::Missing, "{name}");
    }
}

#[test]
fn out_directory_replaced_by_a_symlink_is_rejected() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        let elsewhere = fixture.secrets.path().join("out");
        fs::create_dir(&elsewhere).expect("directory outside");
        fs::write(elsewhere.join("result.webp"), SECRET).expect("file outside");
        let out = fixture.job.path().join("out");
        fs::remove_dir_all(&out).expect("remove out");
        symlink(&elsewhere, &out).expect("plant symlink");

        // Act
        let result = fixture.job.open_output("webp", CAP);

        // Assert
        assert!(
            matches!(result, Err(ScratchError::OutputRejected(_))),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn extension_that_climbs_out_of_the_job_directory_is_rejected() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        fs::create_dir(fixture.job.path().join("out/result.")).expect("directory to climb from");
        let outside = fixture.job.path().parent().expect("root").join("result.x");
        fs::write(&outside, SECRET).expect("file beside the job");

        // Act
        let result = fixture.job.open_output("/../../../result.x", CAP);

        // Assert
        assert_eq!(rejection(result), RejectReason::Unusable, "{name}");
    }
}

#[test]
fn regular_single_link_file_is_readable_read_only_and_non_blocking() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        fs::write(fixture.result(), WEBP).expect("plant file");

        // Act
        let output = fixture.job.open_output("webp", CAP).expect("accepted");

        // Assert
        assert_eq!(output.size(), WEBP.len() as u64, "{name}");
        let mut bytes = Vec::new();
        let flags = fcntl_getfl(&output.file).expect("file status flags");
        assert!(flags.contains(OFlags::NONBLOCK), "{name}");
        assert!(flags.intersection(OFlags::ACCMODE).is_empty(), "{name}");
        output
            .into_reader()
            .read_to_end(&mut bytes)
            .expect("read output");
        assert_eq!(bytes, WEBP, "{name}");
    }
}

#[test]
fn reader_stops_at_the_checked_size_when_the_file_grows_afterwards() {
    // Arrange
    let fixture = Fixture::new(Resolver::detect());
    fs::write(fixture.result(), WEBP).expect("plant file");
    let output = fixture.job.open_output("webp", CAP).expect("accepted");
    fs::write(fixture.result(), [b'x'; 4096]).expect("grow file");

    // Act
    let mut bytes = Vec::new();
    output
        .into_reader()
        .read_to_end(&mut bytes)
        .expect("read output");

    // Assert
    assert!(bytes.len() <= WEBP.len());
}

fn swap_forever(out: &Path, secret: &Path, stop: &AtomicBool) {
    let regular_stage = out.join("stage-file");
    let link_stage = out.join("stage-link");
    let target = out.join("result.webp");
    while !stop.load(Ordering::Relaxed) {
        fs::write(&regular_stage, WEBP).expect("stage file");
        fs::rename(&regular_stage, &target).expect("swap in file");
        symlink(secret, &link_stage).expect("stage link");
        fs::rename(&link_stage, &target).expect("swap in link");
    }
}

#[test]
fn swapping_the_name_between_file_and_symlink_never_yields_the_secret() {
    for (name, resolver) in resolvers() {
        // Arrange
        let fixture = Fixture::new(resolver);
        fs::write(fixture.result(), WEBP).expect("plant file");
        let out = fixture.job.path().join("out");
        let secret = fixture.secret();
        let stop = AtomicBool::new(false);

        thread::scope(|scope| {
            let swapper = scope.spawn(|| swap_forever(&out, &secret, &stop));

            // Act
            let (mut accepted, mut rejected, mut runs) = (0u32, 0u32, 0u32);
            while runs < 10_000 || ((accepted == 0 || rejected == 0) && runs < 1_000_000) {
                runs += 1;
                match fixture.job.open_output("webp", CAP) {
                    Ok(output) => {
                        let mut bytes = Vec::new();
                        output
                            .into_reader()
                            .read_to_end(&mut bytes)
                            .expect("read output");
                        // Assert
                        assert_eq!(
                            bytes, WEBP,
                            "{name}: bytes of something other than the file"
                        );
                        accepted += 1;
                    }
                    Err(ScratchError::OutputRejected(_)) => rejected += 1,
                    Err(other) => panic!("{name}: unexpected error {other:?}"),
                }
            }
            stop.store(true, Ordering::Relaxed);
            swapper.join().expect("swapper thread");

            // Assert
            assert!(
                accepted > 0 && rejected > 0,
                "{name}: {accepted} / {rejected}"
            );
        });
    }
}
