//! Append-only, ids-only security events (`audit_log`, data_model 10.1, ADR-014).
//!
//! Contract: `_docs/02_document/contracts/shared_audit/sc_audit.md`.
//!
//! - [`AuditWriter::record_in`] writes inside the caller's transaction (fail closed: the caller returns the
//!   error and the business change rolls back); [`AuditWriter::record`] uses its own short transaction;
//!   [`AuditWriter::record_best_effort`] never fails, logs at ERROR and counts
//!   ([`AuditWriter::stats`]). Only `share_accessed`, `scrub_mismatch` and `scrub_summary` may use it.
//! - [`AuditEvent`] holds ids, enumerations, an address and an [`AuditDetail`] of ids, codes and integers; no
//!   field takes a `String`, so a name, email, path or token cannot be passed.
//! - [`AuditPurge::purge_older_than`] is the only delete, through the `SECURITY DEFINER` function
//!   `audit_log_purge`; the API role has `INSERT` and `SELECT` on the table and nothing else.
//! - [`AuditReader::export`] pages through the rows, ids only.
//! - Every recorded event is also logged once with target `audit` (`"stream":"audit"`).
//!
//! The statements live in `repo.rs`; `scripts/ci/check-audit-sql.sh` fails on any other statement against
//! `audit_log`.

mod detail;
mod error;
mod event;
mod purge;
mod reader;
mod repo;
mod writer;

pub use detail::{AuditDetail, DetailValue, MAX_DETAIL_BYTES};
pub use error::{AuditError, Rejection};
pub use event::{AuditEvent, AuditEventKind, SubjectRef};
pub use purge::AuditPurge;
pub use reader::{AuditPage, AuditReader, AuditRow, EXPORT_PAGE_ROWS, PageCursor, StoredValue};
pub use writer::{AuditStats, AuditWriter};
