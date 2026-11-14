use std::sync::Arc;

use sc_clock::{Clock, SystemClock};
use sc_config::{ApiSettings, ConfigSources, load_keys};

use crate::command_config::{ServeSettings, load_serve};
use crate::context::ServiceContext;
use crate::dispatch::Commands;
use crate::log::StderrSink;
use crate::root;
use crate::run::CliEnv;
use crate::settings::CliSettings;
use crate::terminal::Terminal;

/// The environment of the real process: standard streams, the real handler and a context whose builders
/// read configuration from the process environment on first use, plus the service builders of the composition
/// root ([`crate::root`]).
pub fn process_env() -> CliEnv {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let builder = ServiceContext::builder()
        .register::<ConfigSources, _, _>(|_| async { Ok(Arc::new(process_sources())) })
        .register::<ServeSettings, _, _>(|services| async move {
            let sources = services.get::<ConfigSources>().await?;
            Ok(load_serve(&sources)?)
        })
        .register::<CliSettings, _, _>(|services| async move {
            let sources = services.get::<ConfigSources>().await?;
            let settings = load_keys::<ApiSettings>(&sources, &CliSettings::KEYS)?;
            Ok(Arc::new(CliSettings::from_settings(&settings)?))
        });
    let services = root::register(builder, Arc::clone(&clock)).finish();
    CliEnv {
        terminal: Terminal::process(),
        services,
        handler: Arc::new(Commands),
        log_sink: Arc::new(StderrSink),
        clock,
    }
}

fn process_sources() -> ConfigSources {
    ConfigSources {
        env: std::env::vars().collect(),
        ..ConfigSources::default()
    }
}
