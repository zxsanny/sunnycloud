use std::collections::{BTreeSet, HashSet};
use std::hash::BuildHasher;

use crate::key::NameKey;

/// Is a name taken in the folder? Implemented by database-backed callers and by in-memory sets.
pub trait NameLookup {
    fn is_taken(&self, key: &NameKey) -> bool;
}

impl<S: BuildHasher> NameLookup for HashSet<NameKey, S> {
    fn is_taken(&self, key: &NameKey) -> bool {
        self.contains(key)
    }
}

impl NameLookup for BTreeSet<NameKey> {
    fn is_taken(&self, key: &NameKey) -> bool {
        self.contains(key)
    }
}
