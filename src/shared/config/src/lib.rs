//! Typed, validated settings for every sunnycloud binary and client.
//!
//! [`load`] binds compiled defaults, config files (JSON objects keyed by table key),
//! `SUNNYCLOUD_*` variables and secret files, later sources winning, into an immutable section
//! ([`ApiSettings`], [`WorkerSettings`], [`DecoderSettings`], [`ClientSettings`]). It refuses
//! with every problem listed at once ([`ConfigError::Invalid`]), or [`ConfigError::MissingSecret`]
//! when a required secret is the only problem.
//!
//! ```
//! use sc_config::{ApiSettings, ConfigSources, ConfigError, load};
//!
//! let mut sources = ConfigSources::default();
//! sources.env.insert("SUNNYCLOUD_RETENTION".into(), "91d".into());
//! match load::<ApiSettings>(&sources) {
//!     Err(ConfigError::Invalid(problems)) => assert!(problems.iter().any(|p| p.key == "retention")),
//!     other => panic!("expected a refusal, got {other:?}"),
//! }
//! ```
//!
//! # Rules
//!
//! - Environment name of a key is `SUNNYCLOUD_` plus the key upper-cased; there is no alias. Any
//!   other `SUNNYCLOUD_*` variable refuses start, except `SUNNYCLOUD_VERSION`.
//! - A binary binds and validates only the keys whose row lists it as a reader, so one `.env`
//!   serves all containers.
//! - A secret comes from `SUNNYCLOUD_<KEY>_FILE`, else `<secrets_dir>/<key>`, else the variable
//!   or file entry. It prints as `***` in `Debug` and `Display` and never enters an error.
//! - Duration floors and ceilings bind only when `environment` is `production`: a value below a
//!   floor is `ForbiddenInRelease`, above a ceiling `OutOfRange`. Count ranges always bind.
//! - Release-fixed keys (`.fixed()` rows) take the compiled values of
//!   [`ConfigSources::release_fixed`] and ignore file and environment.
//! - Client settings changes go through [`ClientSettingsHandle::reload_client_settings`]: one
//!   `Arc` swap, the previous settings stay active on error.
//!
//! # Adding a key row
//!
//! A component that reads a new key adds one line to the table in `src/table.rs` in the same
//! change, and the matching row to `_docs/02_document/common-helpers/01_helper_config.md`:
//!
//! ```text
//! retention: Dur = dflt("7d", A).range("0s", "90d");
//! ```
//!
//! - the name is the flat snake case key; the field type decides the syntax (`Count`, `Size`,
//!   `Dur`, `Flag`, `Text`, `List`, `ListenAddr`, `Sec` for a secret);
//! - `dflt("text", readers)` gives the compiled default in the key's own syntax; `unset(readers)`
//!   with `Opt<T>` makes an optional key, `unset(readers)` with a plain type a required one;
//! - `readers` is `A` (api), `W` (worker), `R` (decoder), `C` (clients) or a combination;
//! - `.range(min, max)` adds a floor and ceiling, `.one_of(&[..])` the allowed texts of a text key,
//!   `.fixed()` makes a client key release-fixed;
//! - cross-field rules belong to the section (`section.rs`);
//! - no test needs writing for the row itself: the table tests walk every row for its default,
//!   its type check, its range and its reader.

mod bind;
mod client;
mod error;
mod section;
mod sources;
mod spec;
mod table;
mod value;

pub use bind::{load, load_keys};
pub use client::ClientSettingsHandle;
pub use error::{ConfigError, KeyProblem, Reason};
pub use section::{ApiSettings, ClientSettings, ConfigSection, DecoderSettings, WorkerSettings};
pub use sources::ConfigSources;
pub use spec::{KeySpec, Reader, Readers};
pub use table::{Settings, TABLE};
pub use value::{
    DurationSetting, Environment, ListenAddr, Secret, SettingType, Size, ValueKind, parse_duration,
    parse_size,
};
