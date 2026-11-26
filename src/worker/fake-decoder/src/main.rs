//! `fake-decoder`: FD of harness H2 as a process on the decoder socket of the Compose test stack. The script and
//! record formats are in the library documentation.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use sunnycloud_fake_decoder::{Config, FakeDecoder, FakeError, Script};
use tokio::signal::unix::{SignalKind, signal};

const USAGE: &str =
    "usage: fake-decoder --socket <path> --script <file> --record <file> [--socket-gid <gid>]";

struct Args {
    socket: PathBuf,
    script: PathBuf,
    record: PathBuf,
    socket_gid: Option<u32>,
}

fn parse_args(mut args: impl Iterator<Item = OsString>) -> Result<Args, String> {
    let (mut socket, mut script, mut record, mut socket_gid) = (None, None, None, None);
    while let Some(flag) = args.next() {
        let flag = flag.to_string_lossy().into_owned();
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--socket" => socket = Some(PathBuf::from(value)),
            "--script" => script = Some(PathBuf::from(value)),
            "--record" => record = Some(PathBuf::from(value)),
            "--socket-gid" => {
                let gid = value.to_str().and_then(|text| text.parse().ok());
                socket_gid = Some(gid.ok_or_else(|| {
                    format!(
                        "--socket-gid takes a number, not {}",
                        value.to_string_lossy()
                    )
                })?);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        socket: socket.ok_or("--socket is required")?,
        script: script.ok_or("--script is required")?,
        record: record.ok_or("--record is required")?,
        socket_gid,
    })
}

async fn run(args: Args) -> Result<(), FakeError> {
    let script = Script::read(&args.script)?;
    let mut terminate = signal(SignalKind::terminate()).map_err(|source| FakeError::Io {
        what: "install the SIGTERM handler",
        source,
    })?;
    let socket = args.socket.display().to_string();
    let decoder = FakeDecoder::serve(Config {
        socket: args.socket,
        socket_gid: args.socket_gid,
        script,
        record: Some(args.record),
    })?;
    eprintln!("fake-decoder: listening on {socket}");
    tokio::select! {
        _ = terminate.recv() => Ok(()),
        stopped = decoder.stopped() => stopped,
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(problem) => {
            eprintln!("fake-decoder: {problem}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("fake-decoder: cannot start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fake-decoder: {error}");
            ExitCode::FAILURE
        }
    }
}
