use sc_names::{FsProfile, NameError, portable_check, split_path};

use super::FsError;

/// A validated relative path: `/`-separated normal components, none of them `.`, `..` or empty.
/// Every `name` argument of [`PlatformFs`](super::PlatformFs) is one, resolved beneath a directory handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelPath(Vec<String>);

impl RelPath {
    /// `profile` is the file-system family the implementation writes to; a name that needs escaping on it
    /// is not representable there.
    pub fn parse(name: &str, profile: FsProfile) -> Result<Self, FsError> {
        if name.starts_with('/') {
            return Err(FsError::EscapesRoot);
        }
        let parts = split_path(name).map_err(|e| match e {
            NameError::DotDot => FsError::EscapesRoot,
            _ => FsError::NameNotRepresentable,
        })?;
        if parts.iter().any(|p| !portable_check(p, profile).is_ok()) {
            return Err(FsError::NameNotRepresentable);
        }
        Ok(Self(parts.into_iter().map(|p| p.into_string()).collect()))
    }

    pub fn components(&self) -> &[String] {
        &self.0
    }

    /// The last component; a parsed path always has one.
    pub fn leaf(&self) -> &str {
        self.0.last().map_or("", String::as_str)
    }

    /// Everything before the leaf, `/`-joined; empty when the leaf is directly under the handle.
    pub fn parent(&self) -> RelPath {
        RelPath(self.0[..self.0.len() - 1].to_vec())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn join(&self, leaf: &str) -> RelPath {
        let mut parts = self.0.clone();
        parts.push(leaf.to_owned());
        RelPath(parts)
    }

    pub fn as_string(&self) -> String {
        self.0.join("/")
    }
}
