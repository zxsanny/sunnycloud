use sc_config::KeyProblem;
use sc_db::{DbError, RunError};
use thiserror::Error;

/// Failures of the identity component that are not port outcomes.
#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("sealing the session successor failed")]
    SuccessorSeal,
    /// The identity section failed its start validation; every problem is listed.
    #[error("invalid identity configuration: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Config(Vec<KeyProblem>),
    /// A stored value outside the set its CHECK constraint allows; a bug or a damaged database.
    #[error("unexpected value {value:?} in column {column}")]
    UnexpectedColumnValue { column: &'static str, value: String },
    /// The outbound HTTP client (TLS roots, client builder) could not be built.
    #[error("outbound HTTP client setup failed")]
    OutboundSetup(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// A configured duration added to the clock leaves the representable range.
    #[error("a configured duration is out of the clock's range")]
    TimeOutOfRange,
    /// An account operation named an account that does not exist.
    #[error("account not found")]
    AccountNotFound,
    /// An account operation the account's current state does not allow (state `deleting` cannot be changed,
    /// only a `deleting` account can be finalized).
    #[error("{operation} refused for an account in state {state}")]
    AccountStateRefused {
        operation: &'static str,
        state: String,
    },
    /// A statement of a job that has no port error to carry it failed.
    #[error("database operation failed")]
    Database(#[source] RunError<DbError>),
    /// Account creation ran before the composition root registered the lifecycle hooks; it fails closed.
    #[error("account lifecycle hooks are not registered")]
    HooksNotRegistered,
    /// The device grant could not draw an unused user code; the random source or the table is damaged.
    #[error("no unused user code after repeated draws")]
    UserCodeExhausted,
}

/// `register_hooks` was called a second time; the first list stays.
#[derive(Debug, Error)]
#[error("account lifecycle hooks are already registered")]
pub struct HooksAlreadyRegistered;
