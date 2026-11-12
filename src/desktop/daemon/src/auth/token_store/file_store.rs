use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{Token, TokenStore, TokenStoreError};

const APP_DIR: &str = "sunnycloud";
const USER_ONLY_FILE: u32 = 0o600;
const USER_ONLY_DIR: u32 = 0o700;
const GROUP_OTHER_BITS: u32 = 0o077;

/// `$XDG_STATE_HOME`, or `~/.local/state` when it is unset or relative (14 §2). `None` without an absolute `$HOME`.
pub fn default_state_dir() -> Option<PathBuf> {
    let from_xdg = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    from_xdg.or_else(|| {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .map(|home| home.join(".local").join("state"))
    })
}

/// The Linux fallback without a Secret Service: `<state dir>/sunnycloud/token-<account id>`, mode 0600 in a 0700
/// directory, written to a 0600 temp file and renamed so it never exists with a wider mode. A file or directory
/// that other users can reach, or that another user owns, is refused and not read.
#[derive(Debug, Clone)]
pub struct UserFileTokenStore {
    dir: PathBuf,
}

impl UserFileTokenStore {
    /// `state_dir` is `$XDG_STATE_HOME` or its default ([`default_state_dir`]).
    pub fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join(APP_DIR),
        }
    }

    pub fn path_for(&self, account: Uuid) -> PathBuf {
        self.dir.join(format!("token-{account}"))
    }

    fn ensure_dir(&self) -> Result<(), TokenStoreError> {
        DirBuilder::new()
            .recursive(true)
            .mode(USER_ONLY_DIR)
            .create(&self.dir)
            .map_err(io_error)?;
        let meta = fs::symlink_metadata(&self.dir).map_err(io_error)?;
        check_private(&meta, true)
    }
}

impl TokenStore for UserFileTokenStore {
    fn load(&self, account: Uuid) -> Result<Option<Token>, TokenStoreError> {
        match fs::symlink_metadata(&self.dir) {
            Ok(meta) => check_private(&meta, true)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        }
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.path_for(account))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
                return Err(TokenStoreError::PermissionTooOpen);
            }
            Err(error) => return Err(io_error(error)),
        };
        check_private(&file.metadata().map_err(io_error)?, false)?;
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(io_error)?;
        if text.is_empty() {
            return Err(TokenStoreError::Io(io::ErrorKind::InvalidData));
        }
        Ok(Some(Token::new(text)))
    }

    fn save(&self, account: Uuid, token: &Token) -> Result<(), TokenStoreError> {
        self.ensure_dir()?;
        let target = self.path_for(account);
        let temp = self
            .dir
            .join(format!(".token-{account}.tmp-{}", Uuid::new_v4()));
        let written = write_new_private(&temp, token.expose().as_bytes())
            .and_then(|()| fs::rename(&temp, &target))
            .and_then(|()| File::open(&self.dir).and_then(|dir| dir.sync_all()));
        if let Err(error) = written {
            remove_leftover(&temp);
            return Err(io_error(error));
        }
        Ok(())
    }

    fn delete(&self, account: Uuid) -> Result<(), TokenStoreError> {
        match fs::remove_file(self.path_for(account)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }
}

fn write_new_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(USER_ONLY_FILE)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn remove_leftover(temp: &Path) {
    match fs::remove_file(temp) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            component = "token_store",
            kind = ?error.kind(),
            "a temporary token file could not be removed"
        ),
    }
}

/// Owned by this user, of the expected kind, and with no group or other bits.
fn check_private(meta: &Metadata, directory: bool) -> Result<(), TokenStoreError> {
    let right_kind = if directory {
        meta.file_type().is_dir()
    } else {
        meta.file_type().is_file()
    };
    let own = meta.uid() == rustix::process::geteuid().as_raw();
    if right_kind && own && meta.mode() & GROUP_OTHER_BITS == 0 {
        Ok(())
    } else {
        Err(TokenStoreError::PermissionTooOpen)
    }
}

fn io_error(error: io::Error) -> TokenStoreError {
    TokenStoreError::Io(error.kind())
}
