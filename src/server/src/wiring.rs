use std::sync::Arc;

use sc_clock::Clock;
use sc_config::{ApiSettings, ConfigSources, load_keys};

use crate::command_config::{ServeSettings, load_serve};
use crate::context::{ContextBuilder, ServiceContext};
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
    let (clock, services) = register_clock(builder);
    CliEnv {
        terminal: Terminal::process(),
        services,
        handler: Arc::new(Commands),
        log_sink: Arc::new(StderrSink),
        clock,
    }
}

/// The process clock: [`sc_clock::SystemClock`], or in a `-test` build one [`sc_clock::OffsetClock`] that every
/// component and 11's clock routes share.
#[cfg(not(feature = "test-clock"))]
fn register_clock(builder: ContextBuilder) -> (Arc<dyn Clock>, ServiceContext) {
    let clock: Arc<dyn Clock> = Arc::new(sc_clock::SystemClock::new());
    let services = root::register(builder, Arc::clone(&clock)).finish();
    (clock, services)
}

#[cfg(feature = "test-clock")]
fn register_clock(builder: ContextBuilder) -> (Arc<dyn Clock>, ServiceContext) {
    let clock = Arc::new(sc_clock::OffsetClock::new());
    let services = root::register_with_test_clock(builder, Arc::clone(&clock)).finish();
    (clock, services)
}

fn process_sources() -> ConfigSources {
    ConfigSources {
        env: std::env::vars().collect(),
        ..ConfigSources::default()
    }
}
