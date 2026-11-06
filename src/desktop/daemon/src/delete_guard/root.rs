use std::path::Path;

use sync_core::FileSource;
use uuid::Uuid;

use super::GuardError;
use crate::local_index::{IndexError, LocalIndexStore, Pair};
use crate::platform_fs::{FsError, PlatformFs, TempFile};

/// The file at the top of every pair root that holds the id stored in `pair.root_sentinel`. It is not a user
/// file: scans and uploads must skip it.
pub const SENTINEL_NAME: &str = ".sunnycloud-root-id";

const SENTINEL_MAX_BYTES: u64 = 64;

/// Why a root cannot be trusted. None of these is ever read as "the user deleted everything".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootFault {
    /// The root cannot be opened or its sentinel cannot be read (renamed away, unmounted, unreadable).
    Unreachable,
    /// The root opens but has no sentinel: an empty mount point or a folder someone emptied.
    SentinelMissing,
    /// The sentinel holds another id: another volume or folder now sits at the root path.
    SentinelMismatch,
    /// Nothing but the sentinel is left although the index lists entries.
    EmptyWithIndex,
}

/// Writes a new sentinel into `root` and returns its id for `NewPair::root_sentinel`.
pub fn create_sentinel<F: PlatformFs>(fs: &F, root: &F::Dir) -> Result<String, GuardError> {
    let id = Uuid::new_v4().to_string();
    let temp = fs.create_new_in(root, SENTINEL_NAME)?;
    if let Err(error) = temp.write_all_at(0, id.as_bytes()) {
        fs.discard_temp(root, temp)?;
        return Err(FsError::Io(error).into());
    }
    fs.rename_over_in(root, temp, SENTINEL_NAME)?;
    Ok(id)
}

pub(super) fn probe<F: PlatformFs>(
    fs: &F,
    index: &LocalIndexStore,
    pair: &Pair,
) -> Result<Option<RootFault>, IndexError> {
    let root = match fs.open_root(Path::new(&pair.local_root)) {
        Ok(root) => root,
        Err(error) => {
            tracing::warn!(%error, pair_id = pair.id, "pair root cannot be opened");
            return Ok(Some(RootFault::Unreachable));
        }
    };
    match read_sentinel(fs, &root) {
        Ok(Some(id)) if id == pair.root_sentinel.as_bytes() => {}
        Ok(Some(_)) => return Ok(Some(RootFault::SentinelMismatch)),
        Ok(None) => return Ok(Some(RootFault::SentinelMissing)),
        Err(error) => {
            tracing::warn!(%error, pair_id = pair.id, "root sentinel cannot be read");
            return Ok(Some(RootFault::Unreachable));
        }
    }
    let listing = match fs.walk(&root) {
        Ok(listing) => listing,
        Err(error) => {
            tracing::warn!(%error, pair_id = pair.id, "pair root cannot be listed");
            return Ok(Some(RootFault::Unreachable));
        }
    };
    let only_sentinel = listing.unrepresentable == 0
        && listing
            .entries
            .iter()
            .all(|entry| entry.name == SENTINEL_NAME);
    if only_sentinel && index.has_entries(pair.id)? {
        return Ok(Some(RootFault::EmptyWithIndex));
    }
    Ok(None)
}

/// `None` when there is no sentinel file. At most one byte more than a sentinel can hold is read, so a larger
/// file never equals an id.
fn read_sentinel<F: PlatformFs>(fs: &F, root: &F::Dir) -> Result<Option<Vec<u8>>, FsError> {
    let source = match fs.open_for_read(root, SENTINEL_NAME) {
        Ok(source) => source,
        Err(error) if error.is_not_found() => return Ok(None),
        Err(error) => return Err(error),
    };
    let wanted = source.len()?.min(SENTINEL_MAX_BYTES + 1);
    let bytes = source.read_at(0, u32::try_from(wanted).unwrap_or(0))?;
    Ok(Some(bytes.to_vec()))
}
