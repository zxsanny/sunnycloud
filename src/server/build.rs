//! Stamps the git commit into the binary for `sunnycloud version`.
//!
//! `SUNNYCLOUD_GIT_COMMIT` from the build environment wins (image builds have no `.git`); otherwise the commit
//! comes from `git rev-parse HEAD`; with neither the binary reports `unknown`.

use std::process::Command;

const COMMIT_VAR: &str = "SUNNYCLOUD_GIT_COMMIT";

fn main() {
    println!("cargo:rerun-if-env-changed={COMMIT_VAR}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
    if std::env::var_os(COMMIT_VAR).is_some() {
        return;
    }
    match git_head() {
        Ok(commit) => println!("cargo:rustc-env={COMMIT_VAR}={commit}"),
        Err(reason) => {
            println!("cargo:warning=git commit not stamped ({reason}); version reports unknown")
        }
    }
}

fn git_head() -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git exited with {}", output.status));
    }
    let commit = String::from_utf8(output.stdout)
        .map_err(|error| format!("git printed non-UTF-8: {error}"))?;
    let commit = commit.trim();
    if commit.is_empty() {
        return Err("git printed nothing".to_owned());
    }
    Ok(commit.to_owned())
}
