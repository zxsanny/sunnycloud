//! Planting `out/result.<ext>`: the regular file of a well-behaved runner, or the hostile file of a compromised one.

use std::fs::{self, File};
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;

use rustix::fs::{CWD, FileType, Mode, mknodat};

use crate::script::Output;

/// Puts `output` at `path` (`content` is the body of a regular file) and returns the `bytes` the answer reports:
/// the size of the regular file behind it, else [`Output::reported_bytes`].
pub(crate) fn plant(
    path: &Path,
    output: &Output,
    content: &[u8],
    in_dir: &Path,
) -> io::Result<u64> {
    match output {
        Output::File => {
            fs::write(path, content)?;
            Ok(u64::try_from(content.len()).unwrap_or(u64::MAX).max(1))
        }
        Output::Bytes(bytes) => {
            fs::write(path, bytes)?;
            Ok(output.reported_bytes())
        }
        Output::Absent => Ok(output.reported_bytes()),
        Output::Symlink { target } => {
            symlink(target, path)?;
            Ok(output.reported_bytes())
        }
        Output::HardLink { in_file } => {
            fs::hard_link(in_dir.join(in_file), path)?;
            Ok(fs::symlink_metadata(path)?.len().max(1))
        }
        Output::Fifo => {
            mknodat(CWD, path, FileType::Fifo, Mode::from_raw_mode(0o644), 0)?;
            Ok(output.reported_bytes())
        }
        Output::Sparse { bytes } => {
            File::create_new(path)?.set_len(*bytes)?;
            Ok(output.reported_bytes())
        }
        Output::Directory => {
            fs::create_dir(path)?;
            Ok(output.reported_bytes())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    struct Job {
        dir: TempDir,
    }

    impl Job {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("job folder");
            fs::create_dir(dir.path().join("in")).expect("in/");
            fs::create_dir(dir.path().join("out")).expect("out/");
            fs::write(dir.path().join("in/source"), b"source bytes").expect("in/source");
            Self { dir }
        }

        fn result(&self) -> PathBuf {
            self.dir.path().join("out/result.webp")
        }

        fn plant(&self, output: &Output) -> u64 {
            plant(&self.result(), output, b"webp", &self.dir.path().join("in")).expect("planted")
        }
    }

    #[test]
    fn file_output_is_a_regular_file_with_the_content_and_its_size() {
        // Arrange
        let job = Job::new();

        // Act
        let bytes = job.plant(&Output::File);

        // Assert
        assert_eq!(fs::read(job.result()).expect("readable"), b"webp");
        assert_eq!(bytes, 4);
    }

    #[test]
    fn bytes_output_is_a_regular_file_of_exactly_those_bytes() {
        // Arrange
        let job = Job::new();
        let head = b"RIFF\x1a\x00\x00\x00WEBPVP8L".to_vec();

        // Act
        let bytes = job.plant(&Output::Bytes(head.clone()));

        // Assert
        assert_eq!(fs::read(job.result()).expect("readable"), head);
        assert_eq!(bytes, 16);
    }

    #[test]
    fn absent_output_leaves_nothing() {
        // Arrange
        let job = Job::new();

        // Act
        job.plant(&Output::Absent);

        // Assert
        assert!(fs::symlink_metadata(job.result()).is_err());
    }

    #[test]
    fn symlink_output_points_at_the_target() {
        // Arrange
        let job = Job::new();
        let target = PathBuf::from("/run/secrets/worker_api_token");

        // Act
        job.plant(&Output::Symlink {
            target: target.clone(),
        });

        // Assert
        assert_eq!(fs::read_link(job.result()).expect("a link"), target);
    }

    #[test]
    fn hard_link_output_shares_the_input_inode_with_nlink_2() {
        // Arrange
        let job = Job::new();

        // Act
        let bytes = job.plant(&Output::HardLink {
            in_file: "source".into(),
        });

        // Assert
        let planted = fs::symlink_metadata(job.result()).expect("planted");
        let source = fs::metadata(job.dir.path().join("in/source")).expect("source");
        assert_eq!((planted.nlink(), planted.ino()), (2, source.ino()));
        assert_eq!(bytes, 12);
    }

    #[test]
    fn fifo_output_is_a_named_pipe() {
        // Arrange
        let job = Job::new();

        // Act
        job.plant(&Output::Fifo);

        // Assert
        let planted = fs::symlink_metadata(job.result()).expect("planted");
        assert!(planted.file_type().is_fifo());
    }

    #[test]
    fn sparse_output_has_the_apparent_size_without_the_blocks() {
        // Arrange
        let job = Job::new();
        let size = 1 << 30;

        // Act
        let bytes = job.plant(&Output::Sparse { bytes: size });

        // Assert
        let planted = fs::symlink_metadata(job.result()).expect("planted");
        assert_eq!((planted.len(), bytes), (size, size));
        assert!(planted.blocks() * 512 < size, "the file is not sparse");
    }

    #[test]
    fn directory_output_is_a_directory() {
        // Arrange
        let job = Job::new();

        // Act
        job.plant(&Output::Directory);

        // Assert
        assert!(
            fs::symlink_metadata(job.result())
                .expect("planted")
                .is_dir()
        );
    }

    #[test]
    fn planting_where_a_file_already_is_fails_instead_of_overwriting() {
        // Arrange
        let job = Job::new();
        fs::write(job.result(), b"earlier").expect("an earlier output");

        // Act
        let result = plant(
            &job.result(),
            &Output::Sparse { bytes: 10 },
            b"webp",
            &job.dir.path().join("in"),
        );

        // Assert
        assert!(result.is_err());
        assert_eq!(fs::read(job.result()).expect("readable"), b"earlier");
    }
}
