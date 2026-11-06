use std::ffi::OsString;
use std::sync::Arc;
use std::time::Instant;

use clap::Parser;
use clap::error::ErrorKind;
use sc_clock::Clock;
use sc_error::ErrorCode;
use sc_observability::{LineSink, subscriber};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::Dispatch;
use tracing::instrument::WithSubscriber;

use crate::commands::{Cli, Command};
use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::guard;
use crate::handler::{Call, Handler};
use crate::log::command_finished;
use crate::outcome::Outcome;
use crate::output::{Mode, print_error, print_outcome};
use crate::settings::CliSettings;
use crate::terminal::Terminal;

/// Everything one run of the CLI touches from outside: streams, services, the command handler and the log sink.
pub struct CliEnv {
    pub terminal: Terminal,
    pub services: ServiceContext,
    pub handler: Arc<dyn Handler>,
    pub log_sink: Arc<dyn LineSink>,
    pub clock: Arc<dyn Clock>,
}

/// Parses, guards, runs the handler, prints, logs one line and returns the exit code.
pub async fn run(args: Vec<OsString>, env: &CliEnv) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => return print_parse_error(env, &error),
    };
    let mode = if cli.json { Mode::Json } else { Mode::Text };
    let started = Instant::now();

    let (settings, settings_error) = CliSettings::resolve(&env.services).await;
    if let Some(error) = settings_error {
        report_io(
            env,
            env.terminal.write_stderr(&format!(
                "warning: settings unavailable ({error}); using the compiled defaults for logging and confirmation\n"
            )),
        );
    }
    let (log_subscriber, _log_stats) = subscriber(
        settings.log_config(),
        Arc::clone(&env.clock),
        Arc::clone(&env.log_sink),
    );
    let dispatch = Dispatch::new(log_subscriber);

    let result = execute(&cli.command, env, settings)
        .with_subscriber(dispatch.clone())
        .await;
    let (exit, error) = match result {
        Ok(outcome) => {
            let exit = outcome.exit();
            match print_outcome(&env.terminal, mode, outcome) {
                Ok(()) => (exit, None),
                Err(error) => {
                    report_io(
                        env,
                        env.terminal
                            .write_stderr(&format!("error: cannot write the result: {error}\n")),
                    );
                    (Exit::Failed, None)
                }
            }
        }
        Err(error) => {
            report_io(env, print_error(&env.terminal, mode, &error));
            (error.exit(), Some(error))
        }
    };

    if settings.format == sc_observability::LogFormat::Json {
        tracing::dispatcher::with_default(&dispatch, || {
            command_finished(
                cli.command.route(),
                exit,
                started.elapsed(),
                error
                    .as_ref()
                    .map(|error| error as &(dyn std::error::Error + 'static)),
            );
        });
    }
    exit.code()
}

async fn execute(
    command: &Command,
    env: &CliEnv,
    settings: CliSettings,
) -> Result<Outcome, CliError> {
    if let Some(confirmation) = command.confirmation() {
        guard::confirm(&env.terminal, &confirmation, settings.confirm_required)?;
    }
    let cancel = CancellationToken::new();
    let _sigint = SigintWatch::start(cancel.clone())?;
    env.handler
        .run(Call {
            command,
            services: &env.services,
            cancel,
            terminal: &env.terminal,
        })
        .await
}

fn print_parse_error(env: &CliEnv, error: &clap::Error) -> u8 {
    let text = error.render().to_string();
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            report_io(env, env.terminal.write_stdout(&text));
            Exit::Success.code()
        }
        _ => {
            report_io(env, env.terminal.write_stderr(&text));
            Exit::Usage.code()
        }
    }
}

/// A failed write to the operator's terminal is reported on stderr; when stderr itself is gone there is no
/// channel left to report on.
fn report_io(env: &CliEnv, result: std::io::Result<()>) {
    if let Err(error) = result {
        drop(
            env.terminal
                .write_stderr(&format!("error: cannot write output: {error}\n")),
        );
    }
}

/// The first SIGINT triggers the token handed to the handler; the process keeps running until the handler
/// returns. Later SIGINTs are absorbed the same way.
struct SigintWatch(JoinHandle<()>);

impl SigintWatch {
    fn start(cancel: CancellationToken) -> Result<Self, CliError> {
        let mut sigint = signal(SignalKind::interrupt()).map_err(|error| CliError::Operator {
            code: ErrorCode::Internal.as_str(),
            detail: format!("cannot install the SIGINT handler: {error}"),
            exit: Exit::Failed,
        })?;
        Ok(Self(tokio::spawn(async move {
            while sigint.recv().await.is_some() {
                cancel.cancel();
            }
        })))
    }
}

impl Drop for SigintWatch {
    fn drop(&mut self) {
        self.0.abort();
    }
}
