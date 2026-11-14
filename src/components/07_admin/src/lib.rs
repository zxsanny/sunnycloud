//! Admin: user administration, the last-admin rule, health aggregation, account views and deletion, the
//! deletion purger and the development seed (component 07).
//!
//! This crate holds the typed settings section, the wire codes of the 07 errors, the constructors of the
//! services, the user administration and the health aggregation of `AdminServiceImpl`; the other port behaviour comes with later
//! tasks. 07 owns no table and issues no SQL. The services take every port and
//! list through their constructors and keep the list order they are given: there is no global registry.

mod account_service;
mod actor;
mod admin_service;
mod config;
mod deletion_purger;
mod deletion_service;
mod dev_seed;
mod dev_seed_content;
mod error;
mod health;
mod last_admin;
mod listing;
mod purge_step;
mod user_admin;

pub use account_service::AccountServiceImpl;
pub use admin_service::AdminServiceImpl;
pub use config::AdminConfig;
pub use deletion_purger::DeletionPurger;
pub use deletion_service::AccountDeletionServiceImpl;
pub use dev_seed::{
    ADMIN_SUBJECT, DevSeedImpl, DevSeedPorts, DevSeedTarget, ProviderLogin, USER_SUBJECTS,
};
pub use error::{admin_error_code, deletion_error_code};
