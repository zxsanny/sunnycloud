use std::sync::Arc;

use sc_clock::SystemClock;
use sc_config::{ApiSettings, ConfigSources, load};

use crate::context::ServiceContext;
use crate::dispatch::Commands;
use crate::log::StderrSink;
use crate::run::CliEnv;
use crate::settings::CliSettings;
use crate::terminal::Terminal;

/// The environment of the real process: standard streams, the real handler and a context whose builders
/// read configuration from the process environment on first use. E-22/c03 adds the service builders here.
pub fn process_env() -> CliEnv {
    let services = ServiceContext::builder()
        .register::<ApiSettings, _, _>(|_| async {
            let sources = ConfigSources {
                env: std::env::vars().collect(),
                ..ConfigSources::default()
            };
            Ok(load::<ApiSettings>(&sources)?)
        })
        .register::<CliSettings, _, _>(|services| async move {
            let api = services.get::<ApiSettings>().await?;
            Ok(Arc::new(CliSettings::from_api(&api)?))
        })
        .finish();
    CliEnv {
        terminal: Terminal::process(),
        services,
        handler: Arc::new(Commands),
        log_sink: Arc::new(StderrSink),
        clock: Arc::new(SystemClock::new()),
    }
}
