//! Safe open of a job's input (10 section 3). `job_dir` arrives from a supervisor that may be
//! compromised, and `in/source` is a path in a tree a decoder could have written to, so the
//! runner never hands it to a media tool before this has run:
//!
//! 1. `job_dir` must be an absolute path inside `decoder_scratch_root`, without `.` or `..`.
//! 2. Every component below the root is opened with `O_PATH | O_NOFOLLOW` relative to the
//!    directory descriptor already held (as `/proc/self/fd/N/<name>`), so a symlink anywhere
//!    is a non-directory and no path is ever resolved twice.
//! 3. `in/source` must be a regular file with `st_nlink == 1`. It is then reopened through
//!    `/proc/self/fd/N` with `O_NONBLOCK`; a FIFO or a device never reaches that open.
//!
//! Anything else is refused; the caller answers `Corrupt`.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path};

use crate::sys::{O_NOFOLLOW, O_NONBLOCK, O_PATH};

const SOURCE: [&str; 2] = ["in", "source"];

#[derive(Debug, thiserror::Error)]
pub enum InputRefused {
    #[error("job_dir is not an absolute path inside the scratch root")]
    OutsideScratchRoot,
    #[error("a component of the job path is not a directory")]
    NotADirectory,
    #[error("in/source is not a regular file")]
    NotRegular,
    #[error("in/source has {links} links")]
    Linked { links: u64 },
    #[error("{what}: {source}")]
    Io {
        what: &'static str,
        source: io::Error,
    },
}

/// Opens `<job_dir>/in/source` for reading; the returned file is a regular, single-link file.
pub fn open_input(scratch_root: &Path, job_dir: &str) -> Result<File, InputRefused> {
    let job_dir = Path::new(job_dir);
    if !job_dir.is_absolute() {
        return Err(InputRefused::OutsideScratchRoot);
    }
    let relative = job_dir
        .strip_prefix(scratch_root)
        .map_err(|_| InputRefused::OutsideScratchRoot)?;
    let mut names = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => names.push(name),
            _ => return Err(InputRefused::OutsideScratchRoot),
        }
    }
    if names.is_empty() {
        return Err(InputRefused::OutsideScratchRoot);
    }
    names.extend(SOURCE.iter().map(OsStr::new));
    open_regular(scratch_root, &names)
}

/// Walks `names` below `root` without following a symlink and returns the last one reopened
/// for reading, provided it is a regular file with one link.
pub(crate) fn open_regular(root: &Path, names: &[&OsStr]) -> Result<File, InputRefused> {
    let mut current = open_path(root, O_PATH)?;
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            require_directory(&current)?;
        }
        let below = Path::new("/proc/self/fd")
            .join(current.as_raw_fd().to_string())
            .join(name);
        current = open_path(&below, O_PATH | O_NOFOLLOW)?;
    }
    let metadata = current.metadata().map_err(io_error("stat the input"))?;
    if !metadata.file_type().is_file() {
        return Err(InputRefused::NotRegular);
    }
    if metadata.nlink() != 1 {
        return Err(InputRefused::Linked {
            links: metadata.nlink(),
        });
    }
    File::options()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(Path::new("/proc/self/fd").join(current.as_raw_fd().to_string()))
        .map_err(io_error("reopen the input"))
}

fn open_path(path: &Path, flags: i32) -> Result<File, InputRefused> {
    File::options()
        .read(true)
        .custom_flags(flags)
        .open(path)
        .map_err(io_error("open a path component"))
}

fn require_directory(file: &File) -> Result<(), InputRefused> {
    let metadata = file.metadata().map_err(io_error("stat a path component"))?;
    if metadata.file_type().is_dir() {
        Ok(())
    } else {
        Err(InputRefused::NotADirectory)
    }
}

fn io_error(what: &'static str) -> impl FnOnce(io::Error) -> InputRefused {
    move |source| InputRefused::Io { what, source }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use tempfile::TempDir;

    use super::*;

    struct Scratch {
        dir: TempDir,
    }

    impl Scratch {
        fn new() -> Self {
            Self {
                dir: TempDir::new().expect("tempdir"),
            }
        }

        fn root(&self) -> &Path {
            self.dir.path()
        }

        fn job(&self, name: &str) -> String {
            self.root().join(name).to_str().expect("utf8").to_owned()
        }

        /// A job directory with `in/` and no `source`.
        fn empty_job(&self, name: &str) -> std::path::PathBuf {
            let input = self.root().join(name).join("in");
            fs::create_dir_all(&input).expect("create in/");
            input
        }

        fn valid_job(&self, name: &str, content: &[u8]) {
            fs::write(self.empty_job(name).join("source"), content).expect("write source");
        }
    }

    fn contents(mut file: File) -> Vec<u8> {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("read input");
        bytes
    }

    fn refused(scratch: &Scratch, job_dir: &str) -> InputRefused {
        open_input(scratch.root(), job_dir).expect_err("the input must be refused")
    }

    #[test]
    fn a_regular_single_link_source_is_opened_and_readable() {
        let scratch = Scratch::new();
        scratch.valid_job("job", b"control bytes");

        let file = open_input(scratch.root(), &scratch.job("job")).expect("open");

        assert_eq!(contents(file), b"control bytes");
    }

    #[test]
    fn a_fifo_is_refused_at_once_without_a_writer() {
        let scratch = Scratch::new();
        let input = scratch.empty_job("job");
        let made = Command::new("mkfifo")
            .arg(input.join("source"))
            .status()
            .expect("mkfifo");
        assert!(made.success());

        let started = Instant::now();
        let result = open_input(scratch.root(), &scratch.job("job"));

        assert!(
            matches!(result, Err(InputRefused::NotRegular)),
            "{result:?}"
        );
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn a_symlink_source_is_refused_and_its_target_is_not_read() {
        let scratch = Scratch::new();
        let input = scratch.empty_job("job");
        symlink("/etc/passwd", input.join("source")).expect("symlink");

        let error = refused(&scratch, &scratch.job("job"));

        assert!(matches!(error, InputRefused::NotRegular), "{error:?}");
    }

    #[test]
    fn a_hard_linked_source_is_refused() {
        let scratch = Scratch::new();
        scratch.valid_job("job", b"x");
        fs::hard_link(
            scratch.root().join("job/in/source"),
            scratch.root().join("other"),
        )
        .expect("hard link");

        let error = refused(&scratch, &scratch.job("job"));

        assert!(
            matches!(error, InputRefused::Linked { links: 2 }),
            "{error:?}"
        );
    }

    #[test]
    fn a_directory_source_is_refused() {
        let scratch = Scratch::new();
        fs::create_dir(scratch.empty_job("job").join("source")).expect("dir");

        let error = refused(&scratch, &scratch.job("job"));

        assert!(matches!(error, InputRefused::NotRegular), "{error:?}");
    }

    #[test]
    fn a_character_device_is_refused() {
        // Creating a device node needs a privilege, so the real /dev/null is reached with
        // /dev as the root: the same walk and the same classification as for `in/source`.
        let result = open_regular(Path::new("/dev"), &[OsStr::new("null")]);

        assert!(
            matches!(result, Err(InputRefused::NotRegular)),
            "{result:?}"
        );
    }

    #[test]
    fn a_missing_source_is_refused() {
        let scratch = Scratch::new();
        scratch.empty_job("job");

        let error = refused(&scratch, &scratch.job("job"));

        assert!(matches!(error, InputRefused::Io { .. }), "{error:?}");
    }

    #[test]
    fn job_dir_paths_that_leave_the_scratch_root_are_refused() {
        let scratch = Scratch::new();
        scratch.valid_job("job", b"x");
        let root = scratch.root().to_str().expect("utf8").to_owned();
        let cases = [
            "../x".to_owned(),
            "job".to_owned(),
            "/etc".to_owned(),
            format!("{root}/../job"),
            format!("{root}/job/../job"),
            format!("{root}x/job"),
            root.clone(),
            String::new(),
        ];

        for case in cases {
            let error = open_input(scratch.root(), &case).expect_err(&case);

            assert!(
                matches!(error, InputRefused::OutsideScratchRoot),
                "{case}: {error:?}"
            );
        }
    }

    #[test]
    fn a_symlinked_job_directory_is_refused_even_when_it_points_inside_the_root() {
        let scratch = Scratch::new();
        scratch.valid_job("real", b"x");
        symlink(scratch.root().join("real"), scratch.root().join("link")).expect("symlink");

        let error = refused(&scratch, &scratch.job("link"));

        assert!(matches!(error, InputRefused::NotADirectory), "{error:?}");
    }

    #[test]
    fn a_symlink_anywhere_in_the_path_is_refused() {
        let scratch = Scratch::new();
        scratch.valid_job("real/job", b"x");
        symlink(scratch.root().join("real"), scratch.root().join("link")).expect("symlink");
        let nested = format!("{}/job", scratch.job("link"));
        fs::create_dir_all(scratch.root().join("job2")).expect("job2");
        symlink(
            scratch.root().join("real/job/in"),
            scratch.root().join("job2/in"),
        )
        .expect("in symlink");

        let through_parent = refused(&scratch, &nested);
        let through_in = refused(&scratch, &scratch.job("job2"));

        assert!(matches!(through_parent, InputRefused::NotADirectory));
        assert!(matches!(through_in, InputRefused::NotADirectory));
    }
}
