//! Desktop sync daemon: the local IPC server over `DaemonControl` lives in [`ipc`], file-system access in [`platform_fs`].

pub mod ipc;
pub mod platform_fs;
