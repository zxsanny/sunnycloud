/// The one mapping from outcome classes to process exit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Success,
    /// Warnings only; also an error a command defines as exit 1 (`admin epoch-bump`).
    Warnings,
    /// The operation failed, the owner refused it, or it found problems.
    Failed,
    /// Usage or configuration error, including a refused confirmation.
    Usage,
    /// `serve` stopped in order on SIGINT (128 + 2, G-12-7).
    Interrupted,
}

impl Exit {
    pub const fn code(self) -> u8 {
        match self {
            Exit::Success => 0,
            Exit::Warnings => 1,
            Exit::Failed => 2,
            Exit::Usage => 3,
            Exit::Interrupted => 130,
        }
    }
}
