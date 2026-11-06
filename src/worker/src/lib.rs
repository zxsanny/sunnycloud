//! The `sunnycloud-worker` supervisor skeleton: typed config section, logging with `component=worker`, the
//! ops listener and the `healthcheck` subcommand.
//!
//! The worker holds only its bearer (`worker_api_token`, a `Secret`): no database, blob store or media
//! library (ADR-009, module-layout 09). Contract: `_docs/02_document/contracts/shared_observability`.
//! This library exists so the tests run the same entry points as the binary.

mod decoder_client;
mod error;
mod probe;
mod run;
mod scratch;
mod settings;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::process::ExitCode;

pub use decoder_client::{
    Backoff, BackoffError, CallError, DecoderConn, DecoderMetrics, EventSink, GateFailure,
    GatedConn, PROTOCOL_VERSION, Slot, SlotConfig, SlotName, SlotReadiness, SlotState,
};
pub use error::WorkerError;
pub use probe::{PROBE_TIMEOUT, ProbeError, get_ok, probe_target};
pub use run::{healthcheck, serve, serve_ops};
pub use scratch::{OutputFile, RejectReason, ScratchDir, ScratchError, ScratchRoot, sweep_all};
pub use settings::{load_settings, ops_addr};

/// Runs the process: no argument serves, `healthcheck` probes. Returns the exit code.
pub fn execute(args: Vec<OsString>, env: BTreeMap<String, String>) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: cannot start the async runtime: {error}");
            return ExitCode::from(2);
        }
    };
    let result = match args.first().map(|arg| arg.to_string_lossy()) {
        None => runtime.block_on(serve(env)),
        Some(arg) if arg == "healthcheck" && args.len() == 1 => runtime.block_on(healthcheck(env)),
        Some(arg) => Err(WorkerError::Usage(arg.into_owned())),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
