//! In-memory [`PlatformFs`] for unit tests of the modules that sit on top of it (applier, merger, uploads).
//! One `FakeFs` is one tree; `Clone` shares it. It checks names with the same rules as the real
//! implementations, and lets a test make the trash unavailable, lock a file or mark a placeholder.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Bytes;
use sc_names::FsProfile;
use sync_core::{FileIdentity, FileSource};

use super::{
    DirEntry, DirListing, EntryKind, FileId, FileStat, FsError, PlatformFs, RelPath, TEMP_PREFIX,
    TempFile,
};

const PROFILE: FsProfile = FsProfile::PosixCaseSensitive;

#[derive(Default)]
struct State {
    files: BTreeMap<String, FakeFile>,
    dirs: BTreeSet<String>,
    trashed: Vec<String>,
    locked: BTreeSet<String>,
    placeholders: BTreeSet<String>,
    trash_unavailable: bool,
    case_insensitive: bool,
    next_id: u64,
}

#[derive(Clone)]
struct FakeFile {
    id: u64,
    data: Vec<u8>,
    mtime_ns: i64,
}

#[derive(Clone, Default)]
pub struct FakeFs {
    state: Arc<Mutex<State>>,
}

/// A directory of the fake tree: its path from the root (empty for the root).
#[derive(Clone, Debug)]
pub struct FakeDir(String);

pub struct FakeTemp {
    fs: FakeFs,
    path: String,
    name: String,
}

pub struct FakeSource {
    data: Vec<u8>,
    mtime_ns: i64,
}

impl FakeFs {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("fake file system state poisoned")
    }

    pub fn add_dir(&self, path: &str) {
        self.state().dirs.insert(path.to_owned());
    }

    pub fn add_file(&self, path: &str, data: &[u8]) {
        let mut st = self.state();
        st.next_id += 1;
        let id = st.next_id;
        st.files.insert(
            path.to_owned(),
            FakeFile {
                id,
                data: data.to_vec(),
                mtime_ns: 0,
            },
        );
    }

    /// Replaces the content and sets the modification time (what a user edit does).
    pub fn edit_file(&self, path: &str, data: &[u8], mtime_ns: i64) {
        let mut st = self.state();
        let file = st.files.get_mut(path).expect("edit_file: no such file");
        file.data = data.to_vec();
        file.mtime_ns = mtime_ns;
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.state().files.get(path).map(|f| f.data.clone())
    }

    /// Paths moved to the trash, in order.
    pub fn trashed(&self) -> Vec<String> {
        self.state().trashed.clone()
    }

    pub fn set_trash_unavailable(&self, unavailable: bool) {
        self.state().trash_unavailable = unavailable;
    }

    pub fn set_case_insensitive(&self, case_insensitive: bool) {
        self.state().case_insensitive = case_insensitive;
    }

    pub fn lock_file(&self, path: &str) {
        self.state().locked.insert(path.to_owned());
    }

    pub fn mark_placeholder(&self, path: &str) {
        self.state().placeholders.insert(path.to_owned());
    }

    fn resolve(dir: &FakeDir, rel: &RelPath) -> String {
        let mut parts: Vec<&str> = dir.0.split('/').filter(|p| !p.is_empty()).collect();
        parts.extend(rel.components().iter().map(String::as_str));
        parts.join("/")
    }

    fn require_dir(&self, path: &str) -> Result<(), FsError> {
        if path.is_empty() || self.state().dirs.contains(path) {
            Ok(())
        } else {
            Err(not_found())
        }
    }
}

fn not_found() -> FsError {
    FsError::Io(io::Error::from(io::ErrorKind::NotFound))
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn stat_of(file: &FakeFile) -> FileStat {
    FileStat {
        kind: EntryKind::File,
        size: file.data.len() as u64,
        mtime_ns: file.mtime_ns,
        id: FileId {
            volume: 1,
            index: file.id,
        },
    }
}

impl PlatformFs for FakeFs {
    type Dir = FakeDir;
    type Temp = FakeTemp;
    type Source = FakeSource;

    fn open_root(&self, _path: &Path) -> Result<FakeDir, FsError> {
        Ok(FakeDir(String::new()))
    }

    fn open_dir(&self, dir: &FakeDir, name: &str) -> Result<FakeDir, FsError> {
        let path = Self::resolve(dir, &RelPath::parse(name, PROFILE)?);
        self.require_dir(&path)?;
        Ok(FakeDir(path))
    }

    fn is_case_insensitive(&self, _root: &FakeDir) -> Result<bool, FsError> {
        Ok(self.state().case_insensitive)
    }

    fn create_new_in(&self, dir: &FakeDir, name: &str) -> Result<FakeTemp, FsError> {
        let target = Self::resolve(dir, &RelPath::parse(name, PROFILE)?);
        let parent = parent_of(&target);
        self.require_dir(parent)?;
        let mut st = self.state();
        st.next_id += 1;
        let temp_name = format!("{TEMP_PREFIX}{}", st.next_id);
        let path = if parent.is_empty() {
            temp_name.clone()
        } else {
            format!("{parent}/{temp_name}")
        };
        let id = st.next_id;
        st.files.insert(
            path.clone(),
            FakeFile {
                id,
                data: Vec::new(),
                mtime_ns: 0,
            },
        );
        Ok(FakeTemp {
            fs: self.clone(),
            path,
            name: temp_name,
        })
    }

    fn rename_over_in(&self, dir: &FakeDir, temp: FakeTemp, name: &str) -> Result<(), FsError> {
        let target = Self::resolve(dir, &RelPath::parse(name, PROFILE)?);
        if parent_of(&target) != parent_of(&temp.path) {
            return Err(FsError::Io(io::Error::from(io::ErrorKind::InvalidInput)));
        }
        let mut st = self.state();
        let file = st.files.remove(&temp.path).ok_or_else(not_found)?;
        st.files.insert(target, file);
        Ok(())
    }

    fn discard_temp(&self, _dir: &FakeDir, temp: FakeTemp) -> Result<(), FsError> {
        self.state()
            .files
            .remove(&temp.path)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    fn trash(&self, dir: &FakeDir, name: &str) -> Result<(), FsError> {
        let path = Self::resolve(dir, &RelPath::parse(name, PROFILE)?);
        let mut st = self.state();
        if st.trash_unavailable {
            return Err(FsError::TrashUnavailable);
        }
        st.files.remove(&path).ok_or_else(not_found)?;
        st.trashed.push(path);
        Ok(())
    }

    fn stat_no_follow(&self, dir: &FakeDir, name: &str) -> Result<FileStat, FsError> {
        let path = Self::resolve(dir, &RelPath::parse(name, PROFILE)?);
        self.state()
            .files
            .get(&path)
            .map(stat_of)
            .ok_or_else(not_found)
    }

    fn is_placeholder(&self, dir: &FakeDir, name: &str) -> Result<bool, FsError> {
        let path = Self::resolve(dir, &RelPath::parse(name, PROFILE)?);
        Ok(self.state().placeholders.contains(&path))
    }

    fn open_for_read(&self, dir: &FakeDir, name: &str) -> Result<FakeSource, FsError> {
        let path = Self::resolve(dir, &RelPath::parse(name, PROFILE)?);
        let st = self.state();
        if st.placeholders.contains(&path) {
            return Err(FsError::Placeholder);
        }
        if st.locked.contains(&path) {
            return Err(FsError::Locked);
        }
        let file = st.files.get(&path).ok_or_else(not_found)?;
        Ok(FakeSource {
            data: file.data.clone(),
            mtime_ns: file.mtime_ns,
        })
    }

    fn walk(&self, dir: &FakeDir) -> Result<DirListing, FsError> {
        self.require_dir(&dir.0)?;
        let st = self.state();
        let entries = st
            .files
            .iter()
            .filter(|(path, _)| parent_of(path) == dir.0)
            .map(|(path, file)| DirEntry {
                name: path.rsplit('/').next().unwrap_or(path).to_owned(),
                stat: stat_of(file),
            })
            .collect();
        Ok(DirListing {
            entries,
            unrepresentable: 0,
        })
    }
}

impl TempFile for FakeTemp {
    fn name(&self) -> &str {
        &self.name
    }

    fn write_all_at(&self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        let mut st = self.fs.state();
        let file = st
            .files
            .get_mut(&self.path)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        let end = offset as usize + bytes.len();
        if file.data.len() < end {
            file.data.resize(end, 0);
        }
        file.data[offset as usize..end].copy_from_slice(bytes);
        Ok(())
    }
}

impl FileSource for FakeSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes> {
        let start = (offset as usize).min(self.data.len());
        let end = (start + len as usize).min(self.data.len());
        Ok(Bytes::copy_from_slice(&self.data[start..end]))
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        Ok(FileIdentity {
            size: self.data.len() as u64,
            mtime_ns: self.mtime_ns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_file_has_the_temp_content_and_no_temp_is_left() {
        // Arrange
        let fs = FakeFs::new();
        fs.add_dir("d");
        fs.add_file("d/a.txt", b"old");
        let root = fs.open_root(Path::new("/any")).expect("root");

        // Act
        let temp = fs.create_new_in(&root, "d/a.txt").expect("temp");
        temp.write_all_at(0, b"new content").expect("write");
        fs.rename_over_in(&root, temp, "d/a.txt").expect("rename");

        // Assert
        assert_eq!(fs.read("d/a.txt").expect("file"), b"new content");
        let dir = fs.open_dir(&root, "d").expect("dir");
        assert_eq!(fs.walk(&dir).expect("walk").entries.len(), 1);
    }

    #[test]
    fn trash_records_the_path_or_reports_unavailable_and_keeps_the_file() {
        // Arrange
        let fs = FakeFs::new();
        fs.add_file("a.txt", b"x");
        fs.add_file("b.txt", b"y");
        let root = fs.open_root(Path::new("/any")).expect("root");

        // Act
        fs.trash(&root, "a.txt").expect("trash");
        fs.set_trash_unavailable(true);
        let refused = fs.trash(&root, "b.txt");

        // Assert
        assert_eq!(fs.trashed(), vec!["a.txt".to_owned()]);
        assert!(matches!(refused, Err(FsError::TrashUnavailable)));
        assert_eq!(fs.read("b.txt").expect("kept"), b"y");
    }

    #[test]
    fn locked_file_and_hostile_names_are_refused_like_the_real_implementation() {
        // Arrange
        let fs = FakeFs::new();
        fs.add_file("a.txt", b"x");
        fs.lock_file("a.txt");
        let root = fs.open_root(Path::new("/any")).expect("root");

        // Act
        let locked = fs.open_for_read(&root, "a.txt");
        let escape = fs.create_new_in(&root, "../x");

        // Assert
        assert!(matches!(locked, Err(FsError::Locked)));
        assert!(matches!(escape, Err(FsError::EscapesRoot)));
    }
}
