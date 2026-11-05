//! Legal names, the case-insensitive name key, server-assigned conflict-copy names and client
//! portability for sunnycloud (helper 06; contract `shared_names/sc_names.md`).
//!
//! The server accepts every name except a short, exact list ([`validate_name`]). One fold
//! ([`name_key`]: NFC, Unicode full case folding, NFC, ICU4X data pinned at [`NAME_KEY_UNICODE_VERSION`])
//! decides uniqueness per folder everywhere. [`conflict_copy_name`] is the only place a conflict-copy name
//! is built. Clients use [`portable_check`] and [`escape_name`] for names their file system cannot hold.
//!
//! The crate is pure: no I/O, no global state, builds for `wasm32-unknown-unknown`.

mod conflict;
mod consts;
mod escape;
mod key;
mod lookup;
mod name;
mod path;
mod portable;
mod utc;

pub use conflict::{MAX_EXTENSION_BYTES, conflict_copy_name};
pub use consts::{COLLISION_SUFFIX_CLOSE, COLLISION_SUFFIX_OPEN, RESERVED_DEVICE_MARKER};
pub use escape::{EscapedName, escape_name};
pub use key::{NAME_KEY_UNICODE_VERSION, NAME_KEY_VERSION, NameKey, name_key};
pub use lookup::NameLookup;
pub use name::{MAX_NAME_BYTES, NameError, ValidName, validate_name};
pub use path::split_path;
pub use portable::{FsProfile, Portable, Reason, portable_check};
