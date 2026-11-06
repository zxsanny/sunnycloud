use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use decoder_runner::{Exit, NoExecutors, RunnerConfig, RunnerError, health, serve, socket};
use sc_config::ConfigSources;
use sc_observability::logging::Level;
use sc_observability::{Component, LogConfig, LogFormat, init_logging, log_error, log_info};

const EXIT_FAILURE: u8 = 1;
/// A client violation: the typed reply is sent and the process ends so the container restarts.
const EXIT_VIOLATION: u8 = 2;

fn main() -> ExitCode {
    let sources = ConfigSources {
        env: std::env::vars().collect(),
        ..ConfigSources::default()
    };
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (None, None) => run(&sources),
        (Some("health"), None) => report_health(&sources),
        _ => {
            eprintln!("{}", RunnerError::Usage);
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

fn report_health(sources: &ConfigSources) -> ExitCode {
    let (body, code) = match health::check(sources) {
        Ok(()) => (serde_json::json!({ "status": "ok" }), ExitCode::SUCCESS),
        Err(unhealthy) => (
            serde_json::json!({ "status": "fail", "reason": unhealthy.reason() }),
            ExitCode::from(EXIT_FAILURE),
        ),
    };
    println!("{body}");
    code
}

fn run(sources: &ConfigSources) -> ExitCode {
    let _guard = match init_logging(LogConfig {
        format: LogFormat::Json,
        level: Level::INFO,
        component: Component::DecoderRunner,
    }) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("cannot start logging: {error}");
            return ExitCode::from(EXIT_FAILURE);
        }
    };
    match serve_process(sources) {
        Ok(Exit::Stopped) => ExitCode::SUCCESS,
        Ok(Exit::ProtocolViolation) => ExitCode::from(EXIT_VIOLATION),
        Err(error) => {
            log_error!("decoder runner stopped", err = &error);
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

fn serve_process(sources: &ConfigSources) -> Result<Exit, RunnerError> {
    let config = RunnerConfig::load(sources)?;
    let listener = socket::bind(&config.socket_path, config.socket_gid)?;
    log_info!("decoder runner listening");
    serve(
        &listener,
        &Arc::new(NoExecutors),
        &config,
        &AtomicBool::new(false),
    )
}
