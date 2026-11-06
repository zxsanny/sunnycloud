use std::fs;
use std::os::fd::AsFd;

use rustix::fs::{Mode, OFlags};

use super::super::RelPath;
use super::resolve::Resolver;

#[test]
fn exclusive_create_never_truncates_an_existing_file_in_either_resolver() {
    for resolver in [Resolver::detect(), Resolver::component_walk()] {
        // Arrange
        let tmp = tempfile::tempdir().expect("temp dir");
        fs::write(tmp.path().join("a.txt"), b"keep me").expect("seed file");
        let root = fs::File::open(tmp.path()).expect("open dir");
        let path = RelPath::parse("a.txt", sc_names::FsProfile::PosixCaseSensitive).expect("path");

        // Act
        let created = resolver.open(
            root.as_fd(),
            &path,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
            Mode::from_raw_mode(0o666),
        );

        // Assert
        let err = created.expect_err("existing name must not be reused");
        assert!(
            matches!(&err, crate::platform_fs::FsError::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists),
            "{err:?}"
        );
        assert_eq!(
            fs::read(tmp.path().join("a.txt")).expect("read"),
            b"keep me"
        );
    }
}
