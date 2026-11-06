use std::str::FromStr;

use sc_config::ApiSettings;
use sc_observability::logging::Level;
use sc_observability::{LogConfig, LogFormat};

use crate::context::ServiceContext;
use crate::error::CliError;

/// The configuration keys the CLI skeleton itself reads: `log`, `log_level` and `cli_confirm_required`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliSettings {
    pub format: LogFormat,
    pub level: Level,
    pub confirm_required: bool,
}

/// The compiled defaults of the three keys.
impl Default for CliSettings {
    fn default() -> Self {
        Self {
            format: LogFormat::Text,
            level: Level::INFO,
            confirm_required: true,
        }
    }
}

impl CliSettings {
    pub fn from_api(settings: &ApiSettings) -> Result<Self, CliError> {
        let format = LogFormat::from_str(&settings.log)
            .map_err(|error| CliError::Usage(error.to_string()))?;
        let level = Level::from_str(&settings.log_level).map_err(|_| {
            CliError::Usage(format!("log level `{}` is not a level", settings.log_level))
        })?;
        Ok(Self {
            format,
            level,
            confirm_required: settings.cli_confirm_required,
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
