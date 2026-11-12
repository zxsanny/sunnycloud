use sc_config::{ConfigError, KeyProblem, Reason, Settings};

/// Default of `max_vault_header_bytes` (description section 6, decision 2026-11-11): the key table has no
/// default for it, so the section applies it.
pub const DEFAULT_MAX_VAULT_HEADER_BYTES: u64 = 1024;

/// The vault section of the key table, typed. The 100 MiB default of `max_vault_file_bytes` comes from
/// `sc_config`, which also refuses a value that is negative, not a number or not a size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VaultConfig {
    pub max_vault_file_bytes: u64,
    pub max_vault_header_bytes: u64,
}

impl VaultConfig {
    /// Binds and validates the section; a refusal lists every problem, each naming its key.
    pub fn from_settings(settings: &Settings) -> Result<Self, ConfigError> {
        let config = VaultConfig {
            max_vault_file_bytes: settings.max_vault_file_bytes.bytes(),
            max_vault_header_bytes: settings
                .max_vault_header_bytes
                .map_or(DEFAULT_MAX_VAULT_HEADER_BYTES, |size| size.bytes()),
        };
        let problems = config.problems();
        if problems.is_empty() {
            Ok(config)
        } else {
            Err(ConfigError::Invalid(problems))
        }
    }

    /// A zero bound would refuse every vault file or every header, and the loader accepts a zero size.
    fn problems(&self) -> Vec<KeyProblem> {
        [
            ("max_vault_file_bytes", self.max_vault_file_bytes),
            ("max_vault_header_bytes", self.max_vault_header_bytes),
        ]
        .into_iter()
        .filter(|(_, bytes)| *bytes == 0)
        .map(|(key, _)| {
            KeyProblem::new(
                key,
                Reason::OutOfRange {
                    min: "1B".to_owned(),
                    max: "no ceiling".to_owned(),
                },
            )
        })
        .collect()
    }
}
