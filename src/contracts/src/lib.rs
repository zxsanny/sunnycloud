//! Server-only port traits and their DTOs (ADR-015): every cross-component call goes through a trait here,
//! so no component crate imports another. Signatures only; the components implement them and `sunnycloud`
//! (12) wires them as `Arc<dyn Port>`.
//!
//! Contract: `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` (registries and
//! lifecycle, 01 identity, 07 admin, 07a quota).
//!
//! # Conventions
//!
//! - Every port is object safe and `Send + Sync`; methods are async unless marked sync. Implement a port with
//!   `#[contracts::async_trait]`.
//! - A method that joins the caller's transaction takes `tx: &mut DbTx`; every other method opens its own
//!   short transaction in the provider. A `DbTx` cannot be built outside `sc_db`:
//!
//! ```compile_fail,E0423
//! let _tx = sc_db::DbTx;
//! ```
//!
//! ```compile_fail,E0451
//! let _tx = sc_db::DbTx { depth: 0 };
//! ```
//!
//! - Each port has one error enum. Its typed variants are the contract's; `Unexpected` carries a tier-3 fault
//!   for the log. Providers convert these to `sc_error::BusinessError`; only 11 chooses an HTTP status.
//! - A DTO that carries a token prints a placeholder in `Debug` (NFT-SEC-16).

mod admin;
mod common;
mod identity;
mod quota;
mod registries;

pub use async_trait::async_trait;

pub use admin::{
    AccountDeletionService, AccountService, AccountServiceError, AccountView, Actor, AdminError,
    AdminService, AdminUserView, DeletionAccepted, DeletionError, DevSeed, DevSeedError,
    HealthReport, IdentityView,
};
pub use common::{Fault, Page};
pub use identity::*;
pub use quota::{
    Drift, QuotaAdmin, QuotaAdminError, QuotaLedger, QuotaLedgerError, QuotaView, Reserved,
};
pub use registries::{
    AccountLifecycleHook, BlobCleanupError, BlobOwnerCleanup, CheckFuture, CycleStats, DoctorCheck,
    DoctorContext, DoctorTag, EraseProgress, Finding, FindingLevel, HealthContributor, HealthItem,
    HealthStatus, Janitor, JanitorError, LifecycleError, ReadinessCheck, ReadinessItem,
    ReadinessStatus,
};
