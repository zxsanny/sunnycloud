//! `update-sign --manifest <file> --key <seed file> --slot active|recovery` (ADR-017). Never prints the key.

use std::process::ExitCode;

use sunnycloud_daemon::updater::sign::{SignError, parse_args, refuse_on_ci, sign};

fn main() -> ExitCode {
    match run() {
        Ok(written) => {
            println!("wrote {}", written.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("update-sign: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}

fn run() -> Result<std::path::PathBuf, SignError> {
    refuse_on_ci(|name| std::env::var_os(name))?;
    let args = parse_args(std::env::args_os().skip(1))?;
    sign(&args)
}
