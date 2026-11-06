//! Conversions of I/O, database and cipher failures into the `BlobError` and `KeyError` of `contracts`.
//!
//! The payload of a converted failure is a [`BlobFault`]: the ids of the blob and account involved and the
//! class of the failure. The text of the cause is not kept, because an OS message, a server message or a
//! driver error can echo a path, a row value or key bytes; the ids stay, so the log line can be joined with the
//! row. A failure that has its own typed variant (`NoSpace`, `BlobMissing`, `Corrupt`, `KeyUnavailable`,
//! `FileMissing`) becomes that variant, which is the whole outcome the caller acts on.

use std::fmt;
use std::io::{self, ErrorKind};

use chacha20poly1305::aead;
use contracts::{BlobError, Fault, KeyError};
use sc_db::{DbError, RunError};
use uuid::Uuid;

/// The ids a failure is attributed to; a failure outside a blob or an account leaves the field empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlobIds {
    pub blob_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    Io(ErrorKind),
    Database(&'static str),
    Cipher,
}

/// The payload of `BlobError::Io` and `KeyError::Unexpected`. `Display` and `Debug` hold ids and the failure
/// class only.
#[derive(Debug)]
pub struct BlobFault {
    ids: BlobIds,
    cause: Cause,
}

impl BlobFault {
    pub fn ids(&self) -> BlobIds {
        self.ids
    }
}

fn fault(ids: BlobIds, cause: Cause) -> Fault {
    Box::new(BlobFault { ids, cause })
}

impl fmt::Display for BlobFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.cause {
            Cause::Io(kind) => write!(f, "i/o failure ({kind})")?,
            Cause::Database(class) => write!(f, "database failure ({class})")?,
            Cause::Cipher => f.write_str("cipher failure")?,
        }
        if let Some(blob_id) = self.ids.blob_id {
            write!(f, " blob {blob_id}")?;
        }
        if let Some(account_id) = self.ids.account_id {
            write!(f, " account {account_id}")?;
        }
        Ok(())
    }
}

impl std::error::Error for BlobFault {}

fn database_class(error: &DbError) -> &'static str {
    match error {
        DbError::UniqueViolation { .. } => "unique violation",
        DbError::ForeignKeyViolation => "foreign key violation",
        DbError::CheckViolation => "check violation",
        DbError::Serialization => "serialization failure",
        DbError::Unavailable => "unavailable",
        DbError::WrongRole(_) => "wrong role",
        DbError::Other(_) => "other",
    }
}

fn database_cause(error: RunError<DbError>) -> Cause {
    match error {
        RunError::Closure(error) | RunError::Db(error) => Cause::Database(database_class(&error)),
    }
}

/// A full volume is `NoSpace`, an absent file `BlobMissing`, any other I/O failure `Io`.
pub fn blob_error_from_io(error: io::Error, ids: BlobIds) -> BlobError {
    match error.kind() {
        ErrorKind::StorageFull => BlobError::NoSpace,
        ErrorKind::NotFound => BlobError::BlobMissing,
        kind => BlobError::Io(fault(ids, Cause::Io(kind))),
    }
}

/// The row of a blob could not be read or written: `Io`, the only variant that carries a fault.
pub fn blob_error_from_db(error: RunError<DbError>, ids: BlobIds) -> BlobError {
    BlobError::Io(fault(ids, database_cause(error)))
}

/// A chunk whose tag does not verify. The cipher error is opaque and carries no data.
pub fn blob_error_from_chunk(_: aead::Error) -> BlobError {
    BlobError::Corrupt
}

/// A data key that does not unwrap under its master key. The cipher error is opaque and carries no data.
pub fn blob_error_from_unwrap(_: aead::Error) -> BlobError {
    BlobError::KeyUnavailable
}

/// An absent key file is `FileMissing`, any other I/O failure `Unexpected`.
pub fn key_error_from_io(error: io::Error, ids: BlobIds) -> KeyError {
    match error.kind() {
        ErrorKind::NotFound => KeyError::FileMissing,
        kind => KeyError::Unexpected(fault(ids, Cause::Io(kind))),
    }
}

pub fn key_error_from_db(error: RunError<DbError>, ids: BlobIds) -> KeyError {
    KeyError::Unexpected(fault(ids, database_cause(error)))
}

/// A cipher failure outside the key-check constant and the sample unwrap, which have their own variants.
pub fn key_error_from_cipher(_: aead::Error, ids: BlobIds) -> KeyError {
    KeyError::Unexpected(fault(ids, Cause::Cipher))
}
