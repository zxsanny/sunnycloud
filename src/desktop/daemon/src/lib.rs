//! Desktop sync daemon: the local IPC server over `DaemonControl` lives in [`ipc`], file-system access in [`platform_fs`], the SQLite index in [`local_index`].

pub mod delete_guard;
pub mod ipc;
pub mod local_index;
pub mod platform_fs;
