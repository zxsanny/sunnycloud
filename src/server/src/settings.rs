use std::str::FromStr;
use std::time::Duration;

use sc_config::Settings;
use sc_observability::logging::Level;
use sc_observability::{LogConfig, LogFormat};

use crate::context::ServiceContext;
use crate::error::CliError;

/// The configuration keys the CLI skeleton itself reads: `log`, `log_level`, `cli_confirm_required` and
/// `cli_progress_interval` (how often a long command prints a progress line on stderr).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliSettings {
    pub format: LogFormat,
    pub level: Level,
    pub confirm_required: bool,
    pub progress_interval: Duration,
}

/// The compiled defaults of the keys.
impl Default for CliSettings {
    fn default() -> Self {
        Self {
            format: LogFormat::Text,
            level: Level::INFO,
            confirm_required: true,
            progress_interval: Duration::from_secs(2),
        }
    }
}

impl CliSettings {
    /// The table keys the skeleton reads; `environment` decides the duration floors.
    pub const KEYS: [&'static str; 5] = [
        "environment",
        "log",
        "log_level",
        "cli_confirm_required",
        "cli_progress_interval",
    ];

    pub fn from_settings(settings: &Settings) -> Result<Self, CliError> {
        let format = LogFormat::from_str(&settings.log)
            .map_err(|error| CliError::Usage(error.to_string()))?;
        let level = Level::from_str(&settings.log_level).map_err(|_| {
            CliError::Usage(format!("log level `{}` is not a level", settings.log_level))
        })?;
        Ok(Self {
            format,
            level,
            confirm_required: settings.cli_confirm_required,
            progress_interval: settings.cli_progress_interval.get(),
        })
    }

    pub(crate) fn log_config(self) -> LogConfig {
        LogConfig {
            format: self.format,
            level: self.level,
            component: sc_observability::Component::Api,
        }
    }

    /// The settings in the context, or the compiled defaults with the reason when the context cannot give them.
    /// Commands that need configuration load it themselves and fail with exit 3; `version` does not read it.
    pub(crate) async fn resolve(context: &ServiceContext) -> (Self, Option<CliError>) {
        match context.get::<CliSettings>().await {
            Ok(settings) => (*settings, None),
            Err(error) => (Self::default(), Some(error)),
        }
    }
}
