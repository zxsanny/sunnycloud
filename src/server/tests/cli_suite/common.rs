//! Helpers of more than one case module: a command line as words, and files of the crate read by path.

use std::path::{Path, PathBuf};

pub fn words(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

pub fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

pub fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}
