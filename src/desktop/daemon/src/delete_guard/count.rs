use std::collections::{BTreeSet, HashSet};

use crate::local_index::{IndexError, LocalIndexStore};

/// Number of individual deleted files in `candidates`. A candidate that is a directory stands for everything
/// below it in the index; the directories themselves are not counted, so a directory with 200 files is 200.
/// A candidate the index does not know counts as one file.
pub(super) fn deleted_files(
    index: &LocalIndexStore,
    pair_id: i64,
    candidates: &BTreeSet<String>,
) -> Result<usize, IndexError> {
    let mut all: HashSet<String> = HashSet::new();
    for candidate in candidates {
        for entry in index.entries_under(pair_id, candidate)? {
            all.insert(entry.rel_path);
        }
        all.insert(candidate.clone());
    }
    let mut directories: HashSet<&str> = HashSet::new();
    for path in &all {
        let mut ancestor = path.as_str();
        while let Some(slash) = ancestor.rfind('/') {
            ancestor = &ancestor[..slash];
            if !directories.insert(ancestor) {
                break;
            }
        }
    }
    Ok(all
        .iter()
        .filter(|path| !directories.contains(path.as_str()))
        .count())
}
