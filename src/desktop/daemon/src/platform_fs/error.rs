use std::io;

/// Why a [`PlatformFs`](super::PlatformFs) call failed. Nothing is created, replaced or unlinked
/// when one of these is returned, except where the operation says otherwise.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    /// Another process holds the file in a way that blocks the read (Windows sharing violation).
    /// Linux has no mandatory locks and never reports it.
    #[error("the file is locked by another process")]
    Locked,
    /// A cloud placeholder that must not be read, hydrated or deleted.
    #[error("the file is a placeholder")]
    Placeholder,
    /// The name is absolute, climbs with `..`, or resolves through a symlink or out of the handle.
    #[error("the name leaves the directory handle it is relative to")]
    EscapesRoot,
    /// The file system cannot hold the name (empty, too long, NUL, control or bidi characters).
    #[error("the file system cannot represent the name")]
    NameNotRepresentable,
    /// No usable trash location for the file's volume; the file stays where it is.
    #[error("no usable trash for the file's volume")]
    TrashUnavailable,
    #[error("permission denied")]
    PermissionDenied,
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
}

impl FsError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Io(e) if e.kind() == io::ErrorKind::NotFound)
    }
}
