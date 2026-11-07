//! The `sunnycloud` operator CLI: command tree, exit codes, `--json` envelope, confirmation guard, one-time
//! secret writer, cooperative Ctrl-C and the lazy service context.
//!
//! Contract: `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md`. The binary is `main.rs`;
//! this library exists so the command tests run the same entry point ([`execute`]) with injected streams,
//! services and handler.

mod command_config;
pub mod commands;
mod context;
mod dispatch;
mod error;
mod exit;
mod guard;
mod handler;
pub mod janitor;
mod log;
mod outcome;
mod output;
mod run;
mod secret;
mod settings;
mod terminal;
mod wiring;

use std::ffi::OsString;

pub use command_config::{ConfigCheck, ServeSettings};
pub use context::{ContextBuilder, ServiceContext};
pub use dispatch::Commands;
pub use error::CliError;
pub use exit::Exit;
pub use handler::{BoxFuture, Call, Handler};
pub use log::StderrSink;
pub use outcome::{Class, Outcome};
pub use run::{CliEnv, run};
pub use secret::OneTimeSecret;
pub use settings::CliSettings;
pub use terminal::Terminal;
pub use wiring::process_env;

/// Runs one command on a fresh runtime and returns the process exit code.
pub fn execute(args: Vec<OsString>, env: &CliEnv) -> u8 {
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run(args, env)),
        Err(error) => {
            eprintln!("error: cannot start the async runtime: {error}");
            Exit::Failed.code()
        }
    }
}
