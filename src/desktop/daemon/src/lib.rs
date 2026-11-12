//! Desktop sync daemon: sign-in and the device token in [`auth`], the local IPC server over `DaemonControl` lives in [`ipc`], file-system access in [`platform_fs`], the SQLite index in [`local_index`], the
//! signed-update check and the `update-sign` tool in [`updater`].

pub mod auth;
pub mod delete_guard;
pub mod ipc;
pub mod local_index;
pub mod platform_fs;
pub mod updater;
